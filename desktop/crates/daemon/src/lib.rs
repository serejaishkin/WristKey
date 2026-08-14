//! WristKey daemon library — GUI, connection manager, and presence loop.
//!
//! Merged: cryptographic unlock (Claude) + RSSI smoothing (Kimi) + debounce.

pub mod conn_mgr;

use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, timeout, Instant};
use tracing::{info, warn};
use uuid::Uuid;
use wristkey_core::{SessionManager, PlatformSecurity, Result, WristKeyError, Response};
use wristkey_ble::{BleAdapter, PeripheralInfo};
use conn_mgr::ConnectionManager;

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
const CONFIG_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567894";

#[derive(Debug, Clone, Copy, PartialEq)]
enum ProximityAction {
    Unlock,
    Lock,
    None,
}

/// Debounce counter for lock decisions.
/// Requires 3 consecutive weak readings before locking.
struct DebounceCounter {
    weak_count: u8,
    threshold: u8,
}

impl DebounceCounter {
    fn new(threshold: u8) -> Self {
        Self { weak_count: 0, threshold }
    }

    /// Returns true if should lock (threshold reached)
    fn record_weak(&mut self) -> bool {
        self.weak_count += 1;
        self.weak_count >= self.threshold
    }

    fn record_strong(&mut self) {
        self.weak_count = 0;
    }

    fn reset(&mut self) {
        self.weak_count = 0;
    }
}

pub struct Daemon {
    pub session: Arc<SessionManager>,
    pub ble: Arc<dyn BleAdapter>,
    pub platform: Arc<dyn PlatformSecurity>,
    pub conn_mgr: Arc<ConnectionManager>,
    debounce: std::sync::Mutex<DebounceCounter>,
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
            debounce: std::sync::Mutex::new(DebounceCounter::new(3)),
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Daemon main loop running (cryptographic unlock + RSSI smoothing)");
        self.platform.register_as_authenticator().await?;

        let mut ticker = interval(Duration::from_secs(5));
        let mut last_unlocked: Option<Instant> = None;
        let mut _last_locked: Option<Instant> = None;

        loop {
            ticker.tick().await;

            match self.check_proximity().await {
                Ok(ProximityAction::Unlock) => {
                    self.debounce.lock().unwrap().record_strong();
                    match self.platform.is_locked().await {
                        Ok(true) => {
                            // Cryptographic unlock: connect → challenge → verify
                            if let Err(e) = self.unlock_with_crypto().await {
                                warn!("Crypto unlock failed: {}", e);
                            } else {
                                info!("Auto-unlocked: watch authenticated");
                                last_unlocked = Some(Instant::now());
                            }
                        }
                        Ok(false) => {}
                        Err(e) => warn!("is_locked check failed: {}", e),
                    }
                }
                Ok(ProximityAction::Lock) => {
                    let should_lock = {
                        let mut d = self.debounce.lock().unwrap();
                        d.record_weak()
                    };
                    if should_lock {
                        match self.platform.is_locked().await {
                            Ok(false) => {
                                if let Err(e) = self.platform.lock_screen().await {
                                    warn!("Lock failed: {}", e);
                                } else {
                                    info!("Auto-locked: watch out of range (3x debounce)");
                                    _last_locked = Some(Instant::now());
                                }
                            }
                            Ok(true) => {}
                            Err(e) => warn!("is_locked check failed: {}", e),
                        }
                    }
                }
                Ok(ProximityAction::None) => {}
                Err(e) => warn!("Proximity check failed: {}", e),
            }

            // Auto-lock by timeout
            if let Ok(config) = self.session.load_config().await {
                if let Ok(false) = self.platform.is_locked().await {
                    if let Some(last_seen) = last_unlocked {
                        if last_seen.elapsed() > Duration::from_secs(config.auto_lock_timeout_sec) {
                            if let Err(e) = self.platform.lock_screen().await {
                                warn!("Timeout lock failed: {}", e);
                            } else {
                                info!("Auto-locked: timeout {}s", config.auto_lock_timeout_sec);
                                _last_locked = Some(Instant::now());
                                last_unlocked = None;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Cryptographic unlock: connect → write challenge → read signed response → verify.
    async fn unlock_with_crypto(&self) -> Result<()> {
        let devices = self.session.list_devices().await?;
        if devices.is_empty() {
            return Err(WristKeyError::Daemon("no paired devices".into()));
        }

        let device = &devices[0];  // Primary device
        let info = PeripheralInfo {
            id: device.address.clone(),
            name: Some(device.name.clone()),
            pin: None,
            device_id: device.device_id.clone(),
            rssi: None,
            service_uuids: vec![],
            raw_manufacturer_data: None,
        };

        let conn = self.ble.connect(&info).await
            .map_err(|e| WristKeyError::Ble(format!("connect: {}", e)))?;

        let challenge_char = Uuid::parse_str(CHALLENGE_CHAR)
            .map_err(|e| WristKeyError::Config(format!("invalid challenge UUID: {}", e)))?;
        let response_char = Uuid::parse_str(RESPONSE_CHAR)
            .map_err(|e| WristKeyError::Config(format!("invalid response UUID: {}", e)))?;

        // Subscribe to response notifications
        let mut rx = self.ble.notify(&conn, response_char).await
            .map_err(|e| WristKeyError::Ble(format!("notify: {}", e)))?;

        // Begin cryptographic unlock
        let challenge = self.session.begin_unlock(device.id).await?;

        // Write challenge to watch
        self.ble.write(&conn, challenge_char, &challenge.to_bytes()).await
            .map_err(|e| WristKeyError::Ble(format!("write challenge: {}", e)))?;
        info!("Challenge written ({} bytes)", challenge.to_bytes().len());

        // Wait for signed response (10s timeout)
        let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
            Ok(Some(d)) => d,
            Ok(None) => {
                let _ = self.ble.disconnect(&conn).await;
                return Err(WristKeyError::Daemon("Watch disconnected before responding".into()));
            }
            Err(_) => {
                let _ = self.ble.disconnect(&conn).await;
                return Err(WristKeyError::Daemon("Timeout waiting for watch response (10s)".into()));
            }
        };

        info!("Response received: {} bytes", response_data.len());

        // Parse response: [signature 64][user_present 1][public_key 65] = 130 bytes
        if response_data.len() != 130 {
            let _ = self.ble.disconnect(&conn).await;
            return Err(WristKeyError::Daemon(
                format!("Invalid response length: {} bytes (expected 130)", response_data.len())
            ));
        }

        let signature = response_data[..64].to_vec();
        let user_present = response_data[64] != 0;
        let public_key = response_data[65..].to_vec();

        let response = Response {
            signature,
            user_present,
            timestamp: chrono::Utc::now(),
        };

        // Verify cryptographic response
        match self.session.verify_unlock(device.id, &response, &public_key).await {
            Ok(true) => {
                info!("Signature verified, user_present={}", user_present);
                let _ = self.ble.disconnect(&conn).await;
                self.platform.unlock_screen().await?;
                Ok(())
            }
            Ok(false) => {
                let _ = self.ble.disconnect(&conn).await;
                Err(WristKeyError::Daemon("Signature verification failed".into()))
            }
            Err(e) => {
                let _ = self.ble.disconnect(&conn).await;
                Err(e)
            }
        }
    }

    async fn check_proximity(&self) -> Result<ProximityAction> {
        let devices = self.session.list_devices().await?;
        if devices.is_empty() {
            return Ok(ProximityAction::None);
        }

        let config = self.session.load_config().await?;
        let service_uuid = Uuid::parse_str(SERVICE_UUID)
            .map_err(|e| WristKeyError::Config(format!("invalid service UUID: {}", e)))?;

        let mut rx = self.ble.scan(service_uuid).await?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut best_rssi: Option<i16> = None;
        let mut any_matched = false;

        while Instant::now() < deadline {
            match timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Some(info)) => {
                    // Multi-level matching: address → device_id fingerprint → name
                    let matched = devices.iter().find(|d| d.address == info.id)
                        .or_else(|| {
                            // Match by device_id fingerprint (SHA-256 of public key from manufacturer data)
                            if let Some(ref scanned_device_id) = info.device_id {
                                devices.iter().find(|d| {
                                    d.device_id.as_ref() == Some(scanned_device_id)
                                })
                            } else {
                                None
                            }
                        })
                        .or_else(|| {
                            // Fallback: match by name (case-insensitive)
                            if let Some(ref name) = info.name {
                                devices.iter().find(|d| d.name.eq_ignore_ascii_case(name))
                            } else {
                                None
                            }
                        });

                    if let Some(device) = matched {
                        any_matched = true;
                        if let Some(raw_rssi) = info.rssi {
                            // Use Kalman + Hysteresis from rssi_filter
                            let threshold = device.baseline_rssi - config.rssi_threshold_offset_dbm;
                            let mut smoother = wristkey_core::RssiSmoother::new(threshold);
                            let (should_unlock, _changed) = smoother.update(raw_rssi);
                            let smoothed = smoother.current_rssi().unwrap_or(raw_rssi);

                            if should_unlock {
                                info!("Device {} in range: raw={} dBm, smoothed={} dBm (threshold {})",
                                      device.name, raw_rssi, smoothed, threshold);
                                return Ok(ProximityAction::Unlock);
                            }
                            if best_rssi.map_or(true, |best| smoothed > best) {
                                best_rssi = Some(smoothed);
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        if any_matched && best_rssi.is_some() {
            return Ok(ProximityAction::Lock);
        }

        Ok(ProximityAction::Lock)
    }

    /// Calibrate proximity unlock threshold for a paired device.
    pub async fn calibrate_proximity(&self, device_id: Uuid) -> Result<i16> {
        let device = self.session.load_device(device_id).await?
            .ok_or_else(|| WristKeyError::Storage("device not found".into()))?;

        info!("Starting proximity calibration for {}", device_id);

        let info = PeripheralInfo {
            id: device.address.clone(),
            name: Some(device.name.clone()),
            pin: None,
            device_id: device.device_id.clone(),
            rssi: None,
            service_uuids: vec![],
            raw_manufacturer_data: None,
        };

        let conn = self.ble.connect(&info).await?;

        let config_char = Uuid::parse_str(CONFIG_CHAR)
            .map_err(|e| WristKeyError::Config(format!("invalid config UUID: {}", e)))?;

        self.ble.write(&conn, config_char, &[0x01]).await?;
        info!("Sent START_CALIBRATION to watch");

        let mut samples = Vec::new();
        let mut ticker = interval(Duration::from_millis(500));
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            ticker.tick().await;
            match self.ble.read_rssi(&conn).await {
                Ok(rssi) => { info!("RSSI sample: {} dBm", rssi); samples.push(rssi); }
                Err(e) => warn!("Failed to read RSSI: {}", e),
            }
        }

        if samples.is_empty() {
            let _ = self.ble.write(&conn, config_char, &[0x03]).await;
            let _ = self.ble.disconnect(&conn).await;
            return Err(WristKeyError::Ble("No RSSI samples collected".into()));
        }

        // Use median instead of average (more robust to outliers)
        samples.sort();
        let median = samples[samples.len() / 2];
        let threshold = median.saturating_sub(config.rssi_threshold_offset_dbm).min(-20).max(-90);
        info!("Calibration: median={} dBm, threshold={} dBm ({} samples)", median, threshold, samples.len());

        // Save baseline to desktop storage
        self.session.update_baseline_rssi(device_id, threshold).await?;
        info!("Saved baseline_rssi={} to desktop storage", threshold);

        let rssi_byte = threshold as i8;
        self.ble.write(&conn, config_char, &[0x02, rssi_byte as u8]).await?;
        info!("Sent CALIBRATION_RESULT: {} dBm", threshold);

        let _ = self.ble.disconnect(&conn).await;
        Ok(threshold)
    }
}
