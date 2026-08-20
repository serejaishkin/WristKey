//! WristKey daemon -- proximity detection, crypto unlock, and auto-lock.

pub mod conn_mgr;
pub use conn_mgr::ConnectionManager;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{interval, timeout, sleep};
use tracing::{info, warn, debug};
use uuid::Uuid;

use wristkey_core::{
    SessionManager, PlatformSecurity, Response,
    Result, WristKeyError, RssiSmoother,
};
use wristkey_ble::{BleAdapter, PeripheralInfo};

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProximityAction {
    Lock,
    Unlock,
    None,
}

pub struct DebounceCounter {
    threshold: usize,
    count: usize,
}

impl DebounceCounter {
    pub fn new(threshold: usize) -> Self { Self { threshold, count: 0 } }
    pub fn tick(&mut self, weak: bool) -> bool {
        if weak { self.count += 1; self.count >= self.threshold }
        else { self.count = 0; false }
    }
    pub fn reset(&mut self) { self.count = 0; }
}

pub struct Daemon {
    session: Arc<SessionManager>,
    ble: Arc<dyn BleAdapter>,
    platform: Arc<dyn PlatformSecurity>,
    conn_mgr: Arc<ConnectionManager>,
    smoother: Mutex<RssiSmoother>,
    debounce: Mutex<DebounceCounter>,
}

impl Daemon {
    pub fn new(
        session: Arc<SessionManager>,
        ble: Arc<dyn BleAdapter>,
        platform: Arc<dyn PlatformSecurity>,
        conn_mgr: Arc<ConnectionManager>,
    ) -> Self {
        Self {
            session,
            ble,
            platform,
            conn_mgr,
            smoother: Mutex::new(RssiSmoother::new(-60i16)),
            debounce: Mutex::new(DebounceCounter::new(3)),
        }
    }

    pub async fn run(&self) -> Result<()> {
        let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
        let mut ticker = interval(Duration::from_secs(2));

        #[cfg(windows)]
        let _pipe_handle = {
            let session = self.session.clone();
            let ble = self.ble.clone();
            let conn_mgr = self.conn_mgr.clone();
            tokio::spawn(pipe_server::run(session, ble, conn_mgr))
        };

        let devices = self.session.list_paired_devices().await?;
        if !devices.is_empty() {
            let state = self.session.state().await;
            if !state.is_authenticated() {
                info!("Daemon started with paired device -- attempting silent reconnect");
                if let Err(e) = self.authenticate_device(&service_uuid, &devices).await {
                    warn!("Silent reconnect failed: {}", e);
                } else {
                    info!("Silent reconnect successful");
                }
            }
        }

        loop {
            ticker.tick().await;
            let devices = self.session.list_paired_devices().await?;
            if devices.is_empty() { sleep(Duration::from_secs(5)).await; continue; }

            let is_locked = self.platform.is_locked().await.unwrap_or(false);
            let session_state = self.session.state().await;
            let action = self.check_proximity(&service_uuid, &devices, is_locked).await?;

            match action {
                ProximityAction::Unlock if is_locked => {
                    info!("Watch nearby and locked -> crypto unlock");
                    if let Err(e) = self.unlock_with_crypto(&service_uuid, &devices).await { warn!("Unlock failed: {}", e); }
                }
                ProximityAction::Lock if !is_locked && session_state.is_authenticated() => {
                    info!("Watch far away and unlocked -> locking");
                    if let Err(e) = self.platform.lock_screen().await { warn!("Lock failed: {}", e); }
                    self.session.disconnect().await;
                }
                _ => {}
            }
        }
    }

    async fn authenticate_device(&self, service_uuid: &Uuid, devices: &[wristkey_core::PairedDevice]) -> Result<()> {
        let device = devices.first().ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;
        let info = PeripheralInfo { id: device.address.clone(), name: Some(device.name.clone()), pin: None,
            device_id: device.device_id.as_ref().and_then(|v| String::from_utf8(v.clone()).ok()), rssi: None,
            service_uuids: vec![*service_uuid], raw_manufacturer_data: None };
        let conn = self.conn_mgr.get_or_connect(&self.ble, &info).await?;
        let challenge_char = Uuid::parse_str(CHALLENGE_CHAR).unwrap();
        let response_char = Uuid::parse_str(RESPONSE_CHAR).unwrap();
        let challenge = self.session.begin_unlock(device.id).await?;
        let mut write_ok = false;
        for attempt in 1..=3 {
            if self.ble.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() { write_ok = true; break; }
            warn!("Auth write attempt {} failed", attempt); sleep(Duration::from_millis(300)).await;
        }
        if !write_ok { let _ = self.ble.disconnect(&conn).await; return Err(WristKeyError::Ble("auth write failed".into())); }
        let mut rx = self.ble.notify(&conn, response_char).await?;
        let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(d)) => d,
            _ => { let _ = self.ble.disconnect(&conn).await; return Err(WristKeyError::Ble("auth response timeout".into())); }
        };
        if response_data.len() < 65 { let _ = self.ble.disconnect(&conn).await; return Err(WristKeyError::Protocol(format!("auth response too short: {} bytes", response_data.len()))); }
        let response = Response { signature: response_data[..64].to_vec(), user_present: response_data[64] != 0, timestamp: chrono::Utc::now() };
        self.session.verify_unlock(&response).await?;
        info!("Silent authenticate OK for {}", device.name);
        Ok(())
    }

    /// Proximity is measured on the Windows/desktop side because btleplug can
    /// read the RSSI of the connected Watch. The Watch is a GATT server and
    /// Android's GattServer callback does not expose the peer RSSI.
    ///
    /// Therefore RSSI is sampled from the *existing ConnectionManager connection*
    /// instead of relying on scan advertisements. This avoids a second adapter,
    /// avoids scan/connect races, and gives us RSSI for the actual paired link.
    async fn check_proximity(&self, service_uuid: &Uuid, devices: &[wristkey_core::PairedDevice], _is_locked: bool) -> Result<ProximityAction> {
        let device = devices.first().ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;
        let info = PeripheralInfo {
            id: device.address.clone(), name: Some(device.name.clone()), pin: None,
            device_id: device.device_id.as_ref().and_then(|v| String::from_utf8(v.clone()).ok()), rssi: None,
            service_uuids: vec![*service_uuid], raw_manufacturer_data: None,
        };

        let conn = match self.conn_mgr.get_or_connect(&self.ble, &info).await {
            Ok(c) => c,
            Err(e) => {
                debug!("proximity connection unavailable: {}", e);
                let should_lock = self.debounce.lock().await.tick(true);
                return if should_lock { Ok(ProximityAction::Lock) } else { Ok(ProximityAction::None) };
            }
        };

        let rssi = match self.ble.read_rssi(&conn).await {
            Ok(v) => v,
            Err(e) => {
                debug!("RSSI read failed: {}", e);
                let should_lock = self.debounce.lock().await.tick(true);
                return if should_lock { Ok(ProximityAction::Lock) } else { Ok(ProximityAction::None) };
            }
        };

        let baseline = device.baseline_rssi;
        let mut smoother = self.smoother.lock().await;
        let current = smoother.current_rssi();
        if current.is_none() {
            *smoother = RssiSmoother::new(baseline);
        }
        let (should_unlock, changed) = smoother.update(rssi);
        let filtered = smoother.current_rssi();
        debug!("proximity {} raw_rssi={} filtered_rssi={:?} baseline={} unlock_candidate={} changed={}", device.name, rssi, filtered, baseline, should_unlock, changed);
        drop(smoother);

        if should_unlock {
            self.debounce.lock().await.reset();
            return Ok(ProximityAction::Unlock);
        }

        // A valid RSSI sample means the paired Watch is physically visible to
        // the connected BLE link. It must NOT by itself authenticate/unlock.
        // Existing crypto unlock flow remains the final authentication step.
        self.debounce.lock().await.reset();
        Ok(ProximityAction::None)
    }

    async fn unlock_with_crypto(&self, service_uuid: &Uuid, devices: &[wristkey_core::PairedDevice]) -> Result<()> {
        let device = devices.first().ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;
        let info = PeripheralInfo { id: device.address.clone(), name: Some(device.name.clone()), pin: None,
            device_id: device.device_id.as_ref().and_then(|v| String::from_utf8(v.clone()).ok()), rssi: None,
            service_uuids: vec![*service_uuid], raw_manufacturer_data: None };
        let conn = self.conn_mgr.get_or_connect(&self.ble, &info).await?;
        let challenge_char = Uuid::parse_str(CHALLENGE_CHAR).unwrap();
        let response_char = Uuid::parse_str(RESPONSE_CHAR).unwrap();
        let challenge = self.session.begin_unlock(device.id).await?;
        let mut write_ok = false;
        for attempt in 1..=3 {
            if self.ble.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() { write_ok = true; break; }
            warn!("Unlock write attempt {} failed", attempt); sleep(Duration::from_millis(300)).await;
        }
        if !write_ok { let _ = self.ble.disconnect(&conn).await; return Err(WristKeyError::Ble("unlock write failed".into())); }
        let mut rx = self.ble.notify(&conn, response_char).await?;
        let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(d)) => d,
            _ => { let _ = self.ble.disconnect(&conn).await; return Err(WristKeyError::Ble("unlock response timeout".into())); }
        };
        if response_data.len() < 65 { let _ = self.ble.disconnect(&conn).await; return Err(WristKeyError::Protocol(format!("unlock response too short: {} bytes", response_data.len()))); }
        let response = Response { signature: response_data[..64].to_vec(), user_present: response_data[64] != 0, timestamp: chrono::Utc::now() };
        self.session.verify_unlock(&response).await?;
        self.platform.unlock_screen().await?;
        info!("Crypto unlock successful for {}", device.name);
        Ok(())
    }
}

#[cfg(windows)]
mod pipe_server {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    pub async fn run(session: Arc<SessionManager>, ble: Arc<dyn BleAdapter>, conn_mgr: Arc<ConnectionManager>) {
        loop {
            match ServerOptions::new().create(r"\\.\pipe\wristkey") {
                Ok(server) => handle_client(server, session.clone(), ble.clone(), conn_mgr.clone()).await,
                Err(e) => { warn!("pipe server create failed: {}", e); sleep(Duration::from_secs(1)).await; }
            }
        }
    }

    async fn handle_client(mut server: tokio::net::windows::named_pipe::NamedPipeServer, session: Arc<SessionManager>, ble: Arc<dyn BleAdapter>, conn_mgr: Arc<ConnectionManager>) {
        let mut reader = BufReader::new(&mut server);
        let mut line = String::new();
        if reader.read_line(&mut line).await.is_err() { return; }
        let request: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => { let _ = reader.get_mut().write_all(format!(r#"{{"status":"error","message":"{}"}}\n"#, e).as_bytes()).await; return; }
        };
        let response = match request.get("action").and_then(|v| v.as_str()).unwrap_or("") {
            "unlock" => match do_ble_unlock(session, ble, conn_mgr).await {
                Ok(password) => serde_json::json!({"status":"success","password":password}),
                Err(e) => serde_json::json!({"status":"error","message":e}),
            },
            _ => serde_json::json!({"status":"error","message":"unknown action"}),
        };
        let _ = reader.get_mut().write_all(format!("{}\n", response).as_bytes()).await;
        let _ = reader.get_mut().flush().await;
    }

    async fn do_ble_unlock(session: Arc<SessionManager>, ble: Arc<dyn BleAdapter>, conn_mgr: Arc<ConnectionManager>) -> Result<String> {
        let devices = session.list_paired_devices().await?;
        let device = devices.first().ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;
        let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
        let info = PeripheralInfo { id: device.address.clone(), name: Some(device.name.clone()), pin: None,
            device_id: device.device_id.as_ref().and_then(|v| String::from_utf8(v.clone()).ok()), rssi: None,
            service_uuids: vec![service_uuid], raw_manufacturer_data: None };
        let conn = conn_mgr.get_or_connect(&ble, &info).await?;
        let challenge = session.begin_unlock(device.id).await?;
        ble.write(&conn, Uuid::parse_str(CHALLENGE_CHAR).unwrap(), &challenge.to_bytes()).await?;
        let mut rx = ble.notify(&conn, Uuid::parse_str(RESPONSE_CHAR).unwrap()).await?;
        let data = timeout(Duration::from_secs(10), rx.recv()).await.ok().flatten().ok_or_else(|| WristKeyError::Ble("unlock response timeout".into()))?;
        if data.len() < 65 { return Err(WristKeyError::Protocol("unlock response too short".into())); }
        let response = Response { signature: data[..64].to_vec(), user_present: data[64] != 0, timestamp: chrono::Utc::now() };
        session.verify_unlock(&response).await?;
        Ok("authenticated".to_string())
    }
}
