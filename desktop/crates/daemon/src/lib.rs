//! WristKey daemon -- proximity detection, crypto unlock, and auto-lock.

pub mod conn_mgr;
pub use conn_mgr::ConnectionManager;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio::time::{interval, timeout, sleep};
use tracing::{info, warn, debug};
use uuid::Uuid;

use wristkey_core::{
    SessionManager, PlatformSecurity, Response,
    Result, WristKeyError, RssiSmoother,
};
use wristkey_core::MsaClient;
use wristkey_ble::{BleAdapter, Connection, PeripheralInfo};

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
// The watch sends its explicit refusal ~10s after a challenge it cannot
// confirm. The PC must wait LONGER than that to ever see it (10s vs 10s was
// a guaranteed race that failed every unlock whose user was not already
// present). A 65-byte signed answer arrives in milliseconds when motion is
// recent, so this window only matters for negative answers.
const RESPONSE_WAIT_SECS: u64 = 25;

/// Wait for a watch response to a written challenge.
///
/// Delivery is polled by READING the response characteristic (the Android
/// GattServer always serves fresh reads) with the NOTIFY stream as a cheap
/// auxiliary path; btleplug's WinRT notifications have proven unreliable
/// across reconnects, so nothing may depend on them alone.
///
/// Every poll read runs inside its own spawned task and is awaited for at
/// most a short slice: a WinRT call that blocks its worker synchronously must
/// not be able to freeze the shared daemon worker (which kills cycle caps).
pub async fn wait_for_response(
    ble: &Arc<dyn BleAdapter>,
    conn: &Connection,
    response_char: Uuid,
    wait_secs: u64,
) -> Result<Vec<u8>> {
    let mut rx = ble.notify(conn, response_char).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(wait_secs);
    let mut read_attempts = 0usize;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(WristKeyError::Ble("unlock response timeout".into()));
        }
        if read_attempts < 24 {
            read_attempts += 1;
            let ble = ble.clone();
            let conn = conn.clone();
            let read_task = tokio::spawn(async move { ble.read(&conn, response_char).await });
            if let Ok(join_result) = timeout(Duration::from_millis(1500), read_task).await {
                if let Ok(read_result) = join_result {
                    if let Ok(data) = read_result {
                        if !data.is_empty() {
                            debug!("response via read: {} bytes", data.len());
                            return Ok(data);
                        }
                    }
                }
            }
        }
        // Bounded peek at the notify stream so a working notification does
        // not have to wait for the next poll epoch.
        if let Ok(Ok(data)) = timeout(Duration::from_millis(250), rx.recv()).await {
            if !data.is_empty() {
                debug!("response via notify: {} bytes", data.len());
                return Ok(data);
            }
        }
    }
}

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
    last_unlock: Mutex<Option<std::time::Instant>>,
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
            last_unlock: Mutex::new(None),
        }
    }

    /// Run the daemon loop. The loop exits cleanly when `shutdown_rx`
    /// receives a closed signal (i.e. the sender was dropped / aborted).
    pub async fn run(&self, mut shutdown_rx: watch::Receiver<()>) -> Result<()> {
        let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
        let mut ticker = interval(Duration::from_secs(2));

        #[cfg(windows)]
        let _pipe_handle = {
            let session = self.session.clone();
            let ble = self.ble.clone();
            let conn_mgr = self.conn_mgr.clone();
            let shutdown = shutdown_rx.clone();
            tokio::spawn(pipe_server::run(session, ble, conn_mgr, shutdown))
        };

        let devices = self.session.list_paired_devices().await?;
        if !devices.is_empty() {
            let state = self.session.state().await;
            if !state.is_authenticated() {
                info!("Daemon started with paired device -- attempting silent reconnect");
                match timeout(Duration::from_secs(45), self.authenticate_device(&service_uuid, &devices)).await {
                    Ok(Ok(())) => info!("Silent reconnect successful"),
                    Ok(Err(e)) => warn!("Silent reconnect failed: {}", e),
                    Err(_) => warn!("Silent reconnect exceeded 45s; abandoned (daemon keeps cycling)"),
                }
            }
        }

        let mut last_cycle = std::time::Instant::now();
        loop {
            let cycle_start = std::time::Instant::now();
            if cycle_start.duration_since(last_cycle) > Duration::from_secs(10) {
                info!("daemon loop: previous cycle stalled for {}s", cycle_start.duration_since(last_cycle).as_secs());
            }
            tokio::select! {
                _ = ticker.tick() => {},
                _ = shutdown_rx.changed() => {
                    info!("Daemon shutting down via signal");
                    return Ok(());
                }
            }
            let devices = self.session.list_paired_devices().await?;
            if devices.is_empty() { sleep(Duration::from_secs(5)).await; continue; }

            let is_locked = self.platform.is_locked().await.unwrap_or(false);
            let session_state = self.session.state().await;
            // Run the proximity cycle under a hard slice. A wedged WinRT/
            // btleplug call must never be able to stall the daemon loop: the
            // future is abandoned and the loop keeps cycling (reconnecting).
            let action = match timeout(Duration::from_secs(45), self.check_proximity(&service_uuid, &devices, is_locked)).await {
                Ok(Ok(a)) => a,
                Ok(Err(e)) => { warn!("proximity cycle error: {}", e); ProximityAction::None }
                Err(_) => { warn!("proximity cycle exceeded 45s; abandoned (daemon keeps cycling)"); ProximityAction::None }
            };
            last_cycle = cycle_start;

            match action {
                ProximityAction::Unlock if is_locked => {
                    // Debounce repeated challenges: while the PC stays locked
                    // and the watch in range, each 2s cycle would otherwise
                    // fire another concurrent unlock exchange.
                    let allow_unlock = {
                        let mut last = self.last_unlock.lock().await;
                        match *last {
                            Some(t) if t.elapsed() < Duration::from_secs(15) => false,
                            _ => { *last = Some(std::time::Instant::now()); true }
                        }
                    };
                    if allow_unlock {
                        info!("Watch nearby and locked -> crypto unlock");
                        if let Err(e) = self.unlock_with_crypto(&service_uuid, &devices).await { warn!("Unlock failed: {}", e); }
                    }
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
        let response_data = wait_for_response(&self.ble, &conn, response_char, RESPONSE_WAIT_SECS).await?;
        if response_data.len() < 65 {
            // The watch sends a 1-byte refusal ([0]) when the user declines or
            // the confirmation UI times out. The session is healthy: keep the
            // cached connection so the next cycle does not force a reconnect.
            warn!("auth: watch returned {} bytes (user denied?) -- keeping connection", response_data.len());
            return Err(WristKeyError::Protocol(format!("auth response too short: {} bytes", response_data.len())));
        }
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
        let response_data = wait_for_response(&self.ble, &conn, response_char, RESPONSE_WAIT_SECS).await?;
        if response_data.len() < 65 {
            warn!("unlock: watch returned {} bytes (user denied?) -- keeping connection", response_data.len());
            return Err(WristKeyError::Protocol(format!("unlock response too short: {} bytes", response_data.len())));
        }
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
    use wristkey_core::vault::KeyProtector;
    use wristkey_platform_win::WindowsKeyProtector;

    /// DPAPI-protect the Windows password before it touches storage.
    /// Only the same user on the same machine can decrypt it.
    fn protect_password(password: &str) -> Result<Vec<u8>> {
        Ok(WindowsKeyProtector.protect(password.as_bytes()))
    }

    fn unprotect_password(encrypted: &[u8]) -> Result<String> {
        let plain = WindowsKeyProtector.unprotect(encrypted)
            .ok_or_else(|| WristKeyError::Storage("failed to decrypt stored password (DPAPI)".into()))?;
        String::from_utf8(plain).map_err(|_| WristKeyError::Storage("stored password is not valid UTF-8".into()))
    }

    pub async fn run(session: Arc<SessionManager>, ble: Arc<dyn BleAdapter>, conn_mgr: Arc<ConnectionManager>, mut shutdown: watch::Receiver<()>) {
        loop {
            // A named-pipe instance serves exactly one client. Create a fresh
            // instance for every connection and hand the connected instance to a
            // per-connection task, so the daemon keeps accepting clients.
            let server = match ServerOptions::new().create(r"\\.\pipe\wristkey") {
                Ok(server) => server,
                Err(e) => {
                    warn!("pipe server create failed: {}", e);
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            let connected = tokio::select! {
                _ = shutdown.changed() => {
                    info!("Pipe server shutting down");
                    return;
                }
                result = server.connect() => match result {
                    Ok(()) => true,
                    Err(e) => { warn!("pipe client connect failed: {}", e); false }
                },
            };
            if !connected { continue; }

            let session = session.clone();
            let ble = ble.clone();
            let conn_mgr = conn_mgr.clone();
            tokio::spawn(async move {
                handle_client(server, session, ble, conn_mgr).await;
            });
        }
    }

    /// Same location the Tauri app reads its config from, so the CLI edits
    /// and the app share one file.
    pub fn config_path() -> std::path::PathBuf {
        dirs_config_dir().join("WristKey/config.toml")
    }

    fn dirs_config_dir() -> std::path::PathBuf {
        std::env::var_os("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    /// Effective MSA settings for one request: fields present in the request
    /// win, anything missing or empty falls back to the stored config.
    fn msa_settings(request: &serde_json::Value, config: &wristkey_core::Config) -> (String, String, String) {
        let pick = |key: &str| -> String {
            request.get(key).and_then(|v| v.as_str()).map(|s| s.trim()).unwrap_or("").to_owned()
        };
        let pick_or = |key: &str, fallback: &str| -> String {
            let v = pick(key);
            if v.is_empty() { fallback.to_owned() } else { v }
        };
        let account = pick_or("msa_account", &config.msa_account);
        // Windows login names arrive as DOMAIN\user; the server stores the bare
        // account, so send the part after the last separator.
        let account = match account.rsplit_once('\\') {
            Some((_, user)) if !user.is_empty() => user.to_owned(),
            _ => account,
        };
        (
            pick_or("server_url", &config.msa_server_url),
            pick_or("token", &config.msa_token),
            account,
        )
    }

    /// Resolve this PC's MSA binding over HTTP (LAN mode, no Bluetooth).
    ///
    /// A PC without BLE never learns the `wristkey_id` the server minted at
    /// register time, so it looks its own binding up by the MSA account it
    /// authenticates with. A 401 means the stored token is wrong for the
    /// server's lan mode.
    async fn msa_status(request: &serde_json::Value, config: &wristkey_core::Config) -> serde_json::Value {
        let (server_url, token, account) = msa_settings(request, config);

        if server_url.is_empty() {
            return serde_json::json!({"status":"error","message":"msa server url is not configured (set msa_server_url)"});
        }
        if account.is_empty() {
            return serde_json::json!({"status":"error","message":"msa account is not configured (set msa_account)"});
        }

        let client = MsaClient::new(server_url, Some(token).filter(|t| !t.is_empty()));
        let bindings = match client.bindings_for_account(&account).await {
            Ok(b) => b,
            Err(e) => return serde_json::json!({"status":"error","message": e.to_string()}),
        };
        match bindings.into_iter().next() {
            // No binding yet is a normal state: the watch has not registered.
            None => serde_json::json!({"status":"success","linked":false,"msa_account":account}),
            Some(b) => serde_json::json!({"status":"success","linked":true,"binding":b}),
        }
    }

    async fn handle_client(mut server: tokio::net::windows::named_pipe::NamedPipeServer, session: Arc<SessionManager>, ble: Arc<dyn BleAdapter>, conn_mgr: Arc<ConnectionManager>) {
        let mut reader = BufReader::new(&mut server);
        let mut line = String::new();
        if reader.read_line(&mut line).await.is_err() { return; }
        let request: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(e) => {
                let _ = reader.get_mut().write_all(format!("{{\"status\":\"error\",\"message\":\"{}\"}}\n", e).as_bytes()).await;
                return;
            }
        };
        let config = wristkey_core::Config::from_file(&config_path()).unwrap_or_default();
        let response = match request.get("action").and_then(|v| v.as_str()).unwrap_or("") {
            "unlock" => match do_ble_unlock(session, ble, conn_mgr, config).await {
                Ok(password) => serde_json::json!({"status":"success","password":password}),
                Err(e) => serde_json::json!({"status":"error","message":e.to_string()}),
            },
            "set_password" => {
                let password = request.get("password").and_then(|v| v.as_str()).unwrap_or("");
                if password.is_empty() {
                    serde_json::json!({"status":"error","message":"empty password"})
                } else {
                    // Paired watch first; a PC with no Bluetooth uses the LAN binding.
                    let result = match set_stored_password(session.clone(), password).await {
                        Ok(()) => Ok(()),
                        Err(e) => set_stored_password_lan(session, &request, &config).await.map_err(|_| e),
                    };
                    match result {
                        Ok(()) => serde_json::json!({"status":"success"}),
                        Err(e) => serde_json::json!({"status":"error","message":e.to_string()}),
                    }
                }
            }
            "clear_password" => match clear_stored_password(session).await {
                Ok(()) => serde_json::json!({"status":"success"}),
                Err(e) => serde_json::json!({"status":"error","message":e.to_string()}),
            },
            "has_password" => {
                let paired = session.list_paired_devices().await
                    .ok()
                    .and_then(|devices| devices.first().map(|d| d.windows_password.is_some()))
                    .unwrap_or(false);
                let linked = msa_settings(&request, &config).0 != "";
                let configured = paired || (linked && msa_status(&request, &config).await
                    .get("linked").and_then(|v| v.as_bool()).unwrap_or(false));
                serde_json::json!({"status":"success","configured":configured})
            }
            "msa_status" => msa_status(&request, &config).await,
            _ => serde_json::json!({"status":"error","message":"unknown action"}),
        };
        let _ = reader.get_mut().write_all(format!("{}\n", response).as_bytes()).await;
        let _ = reader.get_mut().flush().await;
    }

    async fn set_stored_password(session: Arc<SessionManager>, password: &str) -> Result<()> {
        let device = session.list_paired_devices().await?
            .into_iter().next().ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;
        session.set_device_password(device.id, protect_password(password)?).await
    }

    async fn clear_stored_password(session: Arc<SessionManager>) -> Result<()> {
        let device = session.list_paired_devices().await?
            .into_iter().next().ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;
        session.clear_device_password(device.id).await
    }

    async fn do_ble_unlock(session: Arc<SessionManager>, ble: Arc<dyn BleAdapter>, conn_mgr: Arc<ConnectionManager>, config: wristkey_core::Config) -> Result<String> {
        let devices = session.list_paired_devices().await?;
        // No BLE watch paired: fall back to the LAN binding if one is configured.
        // Otherwise the credential provider has no way to unlock this PC.
        let device = match devices.first() {
            Some(d) => d,
            None => return unlock_via_lan(&session, &config).await,
        };
        let encrypted_password = device.windows_password.clone()
            .ok_or_else(|| WristKeyError::Session("windows password not configured; use set_password first".into()))?;
        let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
        let info = PeripheralInfo { id: device.address.clone(), name: Some(device.name.clone()), pin: None,
            device_id: device.device_id.as_ref().and_then(|v| String::from_utf8(v.clone()).ok()), rssi: None,
            service_uuids: vec![service_uuid], raw_manufacturer_data: None };
        let conn = conn_mgr.get_or_connect(&ble, &info).await?;
        let response_char = Uuid::parse_str(RESPONSE_CHAR).unwrap();
        let challenge = session.begin_unlock(device.id).await?;
        ble.write(&conn, Uuid::parse_str(CHALLENGE_CHAR).unwrap(), &challenge.to_bytes()).await?;
        let data = wait_for_response(&ble, &conn, response_char, RESPONSE_WAIT_SECS).await?;
        if data.len() < 65 { return Err(WristKeyError::Protocol(format!("unlock response too short: {} bytes", data.len()))); }
        let response = Response { signature: data[..64].to_vec(), user_present: data[64] != 0, timestamp: chrono::Utc::now() };
        session.verify_unlock(&response).await?;
        // The watch confirmed presence; only now decrypt and hand over the
        // stored Windows password to the credential provider.
        unprotect_password(&encrypted_password)
    }

    /// Unlock for a PC without Bluetooth: resolve the binding by MSA account and
    /// hand over the stored Windows password.
    ///
    /// This does NOT prove the watch is currently present — it only proves the
    /// watch registered with this server at some point. Treat it as the LAN
    /// equivalent of a paired device.
    async fn unlock_via_lan(session: &Arc<SessionManager>, config: &wristkey_core::Config) -> Result<String> {
        let (server_url, token, account) = msa_settings(&serde_json::json!({}), config);
        if server_url.is_empty() {
            return Err(WristKeyError::Session("no paired devices and no MSA server configured".into()));
        }
        if account.is_empty() {
            return Err(WristKeyError::Session("no paired devices and no MSA account configured".into()));
        }
        let client = MsaClient::new(server_url, Some(token).filter(|t| !t.is_empty()));
        let binding = client.resolve_own_binding(&account).await
            .map_err(|e| WristKeyError::Session(format!("msa resolve failed: {}", e)))?
            .ok_or_else(|| WristKeyError::Session("msa server has no binding for this account; register on the watch first".into()))?;
        info!("LAN unlock using binding {} ({})", binding.wristkey_id, binding.msa_account);

        let encrypted = session.get_lan_device_password(&binding).await?
            .ok_or_else(|| WristKeyError::Session("windows password not configured; use set_password first".into()))?;
        unprotect_password(&encrypted)
    }

    async fn set_stored_password_lan(session: Arc<SessionManager>, request: &serde_json::Value, config: &wristkey_core::Config) -> Result<()> {
        let (server_url, token, account) = msa_settings(request, config);
        let password = request.get("password").and_then(|v| v.as_str()).unwrap_or("");
        if password.is_empty() { return Err(WristKeyError::Session("empty password".into())); }
        let client = MsaClient::new(server_url, Some(token).filter(|t| !t.is_empty()));
        let binding = client.resolve_own_binding(&account).await
            .map_err(|e| WristKeyError::Session(format!("msa resolve failed: {}", e)))?
            .ok_or_else(|| WristKeyError::Session("msa server has no binding for this account; register on the watch first".into()))?;
        session.set_lan_device_password(&binding, protect_password(password)?).await
    }

    #[cfg(test)]
    mod msa_tests {
        use super::*;

        fn config() -> wristkey_core::Config {
            wristkey_core::Config {
                msa_server_url: "http://10.0.0.5:8787".into(),
                msa_token: "stored-token".into(),
                msa_account: "stored@outlook.com".into(),
                ..Default::default()
            }
        }

        #[test]
        fn falls_back_to_config_when_request_omits_fields() {
            let (url, token, account) = msa_settings(&serde_json::json!({"action":"msa_status"}), &config());
            assert_eq!(url, "http://10.0.0.5:8787");
            assert_eq!(token, "stored-token");
            assert_eq!(account, "stored@outlook.com");
        }

        #[test]
        fn request_fields_override_config() {
            let req = serde_json::json!({
                "server_url": "http://192.168.1.50:9000",
                "token": "req-token",
                "msa_account": "req@outlook.com",
            });
            let (url, token, account) = msa_settings(&req, &config());
            assert_eq!(url, "http://192.168.1.50:9000");
            assert_eq!(token, "req-token");
            assert_eq!(account, "req@outlook.com");
        }

        #[test]
        fn empty_and_wrong_typed_request_values_fall_back_to_config() {
            let req = serde_json::json!({
                "server_url": "",
                "token": "   ",
                "msa_account": 42,
            });
            let (url, token, account) = msa_settings(&req, &config());
            assert_eq!(url, "http://10.0.0.5:8787");
            assert_eq!(token, "stored-token");
            assert_eq!(account, "stored@outlook.com");
        }

        #[test]
        fn windows_domain_prefix_is_stripped_from_account() {
            let req = serde_json::json!({"msa_account": "WORK\\serge@outlook.com"});
            let (_, _, account) = msa_settings(&req, &config());
            assert_eq!(account, "serge@outlook.com", "server stores the bare account");

            // No username after the separator: keep the value as-is.
            let req = serde_json::json!({"msa_account": "WORK\\"});
            let (_, _, account) = msa_settings(&req, &config());
            assert_eq!(account, "WORK\\");
        }

        #[test]
        fn unconfigured_lan_yields_empty_settings() {
            let empty = wristkey_core::Config::default();
            let (url, token, account) = msa_settings(&serde_json::json!({}), &empty);
            assert!(url.is_empty(), "BLE-only PC must report LAN as unconfigured");
            assert!(token.is_empty());
            assert!(account.is_empty());
        }

        #[test]
        fn config_path_lives_next_to_the_tauri_config() {
            let p = config_path();
            assert!(p.ends_with(std::path::Path::new("WristKey").join("config.toml")), "got {}", p.display());
        }
    }
}
