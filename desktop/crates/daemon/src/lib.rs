//! WristKey daemon -- proximity detection, crypto unlock, and auto-lock.
//!
//! Platform-agnostic: all platform-specific code lives in platform-* crates.

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
    pub fn new(threshold: usize) -> Self {
        Self { threshold, count: 0 }
    }
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

        // Silent reconnect on startup
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
            if devices.is_empty() {
                sleep(Duration::from_secs(5)).await;
                continue;
            }

            let is_locked = self.platform.is_locked().await.unwrap_or(false);
            let session_state = self.session.state().await;

            let action = self.check_proximity(&service_uuid, &devices, is_locked).await?;

            match action {
                ProximityAction::Unlock if is_locked => {
                    info!("Watch nearby and locked -> crypto unlock");
                    if let Err(e) = self.unlock_with_crypto(&service_uuid, &devices).await {
                        warn!("Unlock failed: {}", e);
                    }
                }
                ProximityAction::Lock if !is_locked && session_state.is_authenticated() => {
                    info!("Watch far away and unlocked -> locking");
                    if let Err(e) = self.platform.lock_screen().await {
                        warn!("Lock failed: {}", e);
                    }
                    self.session.disconnect().await;
                }
                _ => {}
            }
        }
    }

    async fn authenticate_device(
        &self,
        service_uuid: &Uuid,
        devices: &[wristkey_core::PairedDevice],
    ) -> Result<()> {
        let device = devices.first()
            .ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;

        let info = PeripheralInfo {
            id: device.address.clone(),
            name: Some(device.name.clone()),
            pin: None,
            device_id: device.device_id.as_ref().and_then(|v| String::from_utf8(v.clone()).ok()),
            rssi: None,
            service_uuids: vec![*service_uuid],
            raw_manufacturer_data: None,
        };

        let conn = self.conn_mgr.get_or_connect(&self.ble, &info).await?;
        let challenge_char = Uuid::parse_str(CHALLENGE_CHAR).unwrap();
        let response_char = Uuid::parse_str(RESPONSE_CHAR).unwrap();

        let challenge = self.session.begin_unlock(device.id).await?;

        let mut write_ok = false;
        for attempt in 1..=3 {
            if self.ble.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() {
                write_ok = true;
                break;
            }
            warn!("Auth write attempt {} failed", attempt);
            sleep(Duration::from_millis(300)).await;
        }
        if !write_ok {
            let _ = self.ble.disconnect(&conn).await;
            return Err(WristKeyError::Ble("auth write failed".into()));
        }

        let mut rx = self.ble.notify(&conn, response_char).await?;
        let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(d)) => d,
            _ => {
                let _ = self.ble.disconnect(&conn).await;
                return Err(WristKeyError::Ble("auth response timeout".into()));
            }
        };

        if response_data.len() < 65 {
            let _ = self.ble.disconnect(&conn).await;
            return Err(WristKeyError::Protocol(format!(
                "auth response too short: {} bytes", response_data.len()
            )));
        }

        let signature = response_data[..64].to_vec();
        let user_present = response_data[64] != 0;

        let response = Response {
            signature,
            user_present,
            timestamp: chrono::Utc::now(),
        };

        self.session.verify_unlock(&response).await?;
        info!("Silent authenticate OK for {}", device.name);
        Ok(())
    }

    async fn check_proximity(
        &self,
        service_uuid: &Uuid,
        devices: &[wristkey_core::PairedDevice],
        _is_locked: bool,
    ) -> Result<ProximityAction> {
        let mut rx = self.ble.scan(*service_uuid).await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut found_rssi: Option<i16> = None;

        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { break; }
            match timeout(remaining, rx.recv()).await {
                Ok(Some(info)) => {
                    let matched = devices.iter().find(|d| {
                        d.address == info.id
                        || d.device_id.as_ref().and_then(|v| String::from_utf8(v.clone()).ok()).as_ref() == info.device_id.as_ref()
                    });
                    if let Some(device) = matched {
                        if let Some(rssi) = info.rssi {
                            let mut smoother = self.smoother.lock().await;
                            let (should_unlock, changed) = smoother.update(rssi);
                            debug!("{} raw={} smoothed={:?} unlock={}",
                                device.name, rssi, smoother.current_rssi(), should_unlock);
                            let threshold = device.baseline_rssi;
                            if smoother.current_rssi().is_none() || changed {
                                drop(smoother);
                                let mut s = self.smoother.lock().await;
                                *s = RssiSmoother::new(threshold);
                            }
                            found_rssi = Some(rssi);
                            if should_unlock {
                                self.debounce.lock().await.reset();
                                let _ = self.ble.stop_scan().await;
                                return Ok(ProximityAction::Unlock);
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
        let _ = self.ble.stop_scan().await;
        if found_rssi.is_none() {
            let should_lock = self.debounce.lock().await.tick(true);
            if should_lock { return Ok(ProximityAction::Lock); }
        }
        Ok(ProximityAction::None)
    }

    async fn unlock_with_crypto(
        &self,
        service_uuid: &Uuid,
        devices: &[wristkey_core::PairedDevice],
    ) -> Result<()> {
        let device = devices.first()
            .ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;

        let info = PeripheralInfo {
            id: device.address.clone(),
            name: Some(device.name.clone()),
            pin: None,
            device_id: device.device_id.as_ref().and_then(|v| String::from_utf8(v.clone()).ok()),
            rssi: None,
            service_uuids: vec![*service_uuid],
            raw_manufacturer_data: None,
        };

        let conn = self.conn_mgr.get_or_connect(&self.ble, &info).await?;
        let challenge_char = Uuid::parse_str(CHALLENGE_CHAR).unwrap();
        let response_char = Uuid::parse_str(RESPONSE_CHAR).unwrap();

        let challenge = self.session.begin_unlock(device.id).await?;

        let mut write_ok = false;
        for attempt in 1..=3 {
            if self.ble.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() {
                write_ok = true;
                break;
            }
            warn!("Unlock write attempt {} failed", attempt);
            sleep(Duration::from_millis(300)).await;
        }
        if !write_ok {
            let _ = self.ble.disconnect(&conn).await;
            return Err(WristKeyError::Ble("unlock write failed".into()));
        }

        let mut rx = self.ble.notify(&conn, response_char).await?;
        let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(d)) => d,
            _ => {
                let _ = self.ble.disconnect(&conn).await;
                return Err(WristKeyError::Ble("unlock response timeout".into()));
            }
        };

        if response_data.len() < 65 {
            let _ = self.ble.disconnect(&conn).await;
            return Err(WristKeyError::Protocol(format!(
                "unlock response too short: {} bytes", response_data.len()
            )));
        }

        let signature = response_data[..64].to_vec();
        let user_present = response_data[64] != 0;

        let response = Response {
            signature,
            user_present,
            timestamp: chrono::Utc::now(),
        };

        self.session.verify_unlock(&response).await?;
        self.platform.unlock_screen().await?;
        info!("Screen unlocked via crypto");
        Ok(())
    }
}
