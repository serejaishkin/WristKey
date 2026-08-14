//! WristKey daemon library — GUI, connection manager, and presence loop.

pub mod conn_mgr;
pub mod gui;
pub mod pair_gui;
pub mod tray;

use std::sync::Arc;
use std::time::Duration;
use chrono::Utc;
use tokio::time::{interval, timeout, Instant};
use tracing::{info, warn};
use uuid::Uuid;
use wristkey_core::{SessionManager, PlatformSecurity, PairedDevice, Response, Result, WristKeyError};
use wristkey_ble::{BleAdapter, PeripheralInfo};
use conn_mgr::ConnectionManager;

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
const CONFIG_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567894";

/// Consecutive low-signal scan cycles required before locking. Smooths over
/// a single noisy RSSI reading (body position, momentary obstruction) so the
/// screen doesn't flicker-lock — same idea as the debounce that used to live
/// in SessionManager::update_rssi, reimplemented here since this loop now
/// drives locking directly from scan results rather than a held connection.
const LOW_SIGNAL_LOCK_THRESHOLD: u32 = 3;

pub struct Daemon {
    pub session: Arc<SessionManager>,
    pub ble: Arc<dyn BleAdapter>,
    pub platform: Arc<dyn PlatformSecurity>,
    pub conn_mgr: Arc<ConnectionManager>,
}

impl Daemon {
    pub fn new(
        session: Arc<SessionManager>,
        ble: Arc<dyn BleAdapter>,
        platform: Arc<dyn PlatformSecurity>,
        conn_mgr: Arc<ConnectionManager>,
    ) -> Self {
        Self { session, ble, platform, conn_mgr }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Daemon main loop running");
        self.platform.register_as_authenticator().await?;

        let service_uuid = Uuid::parse_str(SERVICE_UUID)
            .map_err(|e| WristKeyError::Config(format!("invalid service UUID: {}", e)))?;

        let mut ticker = interval(Duration::from_secs(2));
        let mut last_unlocked: Option<Instant> = None;
        let mut low_signal_streak: u32 = 0;

        loop {
            ticker.tick().await;

            match self.scan_for_best_match(service_uuid).await {
                Ok(Some((device, rssi))) => {
                    let config = self.session.load_config().await?;
                    let threshold = device.baseline_rssi - config.rssi_threshold_offset_dbm;

                    if rssi > threshold {
                        low_signal_streak = 0;
                        match self.platform.is_locked().await {
                            Ok(true) => {
                                // Real challenge-response — this is the part that
                                // was missing entirely: previously any device that
                                // matched by name/RSSI unlocked immediately, with
                                // no signature verification at all.
                                match self.attempt_unlock(&device).await {
                                    Ok(()) => {
                                        info!("Unlocked via {} (verified)", device.name);
                                        last_unlocked = Some(Instant::now());
                                    }
                                    Err(e) => warn!("Unlock attempt for {} failed: {}", device.name, e),
                                }
                            }
                            Ok(false) => {
                                // Already unlocked — just refresh the timeout clock,
                                // no need to reconnect and re-verify constantly.
                                last_unlocked = Some(Instant::now());
                            }
                            Err(e) => warn!("is_locked check failed: {}", e),
                        }
                    } else {
                        low_signal_streak += 1;
                        self.maybe_lock(&mut low_signal_streak).await;
                    }
                }
                Ok(None) => {
                    low_signal_streak += 1;
                    self.maybe_lock(&mut low_signal_streak).await;
                }
                Err(e) => warn!("Proximity scan failed: {}", e),
            }

            // Auto-lock by timeout if no successful unlock/refresh for N seconds,
            // independent of RSSI (covers the watch's Bluetooth radio dying,
            // being turned off, going out of Bluetooth range entirely, etc.)
            if let Ok(config) = self.session.load_config().await {
                if let Ok(false) = self.platform.is_locked().await {
                    if let Some(last_seen) = last_unlocked {
                        if last_seen.elapsed() > Duration::from_secs(config.auto_lock_timeout_sec) {
                            if let Err(e) = self.platform.lock_screen().await {
                                warn!("Timeout lock failed: {}", e);
                            } else {
                                info!("Auto-locked: timeout {}s", config.auto_lock_timeout_sec);
                                last_unlocked = None;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn maybe_lock(&self, streak: &mut u32) {
        if *streak < LOW_SIGNAL_LOCK_THRESHOLD {
            return;
        }
        if let Ok(false) = self.platform.is_locked().await {
            if let Err(e) = self.platform.lock_screen().await {
                warn!("Lock failed: {}", e);
            } else {
                info!("Auto-locked: watch out of range ({} low readings)", streak);
            }
        }
    }

    /// Scans (does NOT connect) and returns the paired device with the
    /// strongest matching signal, if any. Matching is deliberately
    /// brand-agnostic — none of these signals are Samsung-specific:
    ///   1. BLE address — exact, works for any watch as long as the OS
    ///      hasn't rotated its random address since pairing.
    ///   2. device_id fingerprint (SHA-256 prefix of the public key,
    ///      broadcast in manufacturer data) — survives address rotation,
    ///      works on any watch whose custom advertising isn't blocked.
    ///   3. Case-insensitive name match — the fallback of last resort,
    ///      useful specifically when OEM firmware (Samsung One UI Watch is
    ///      the known offender) blocks/overrides custom advertising data,
    ///      but the watch's Bluetooth name still comes through.
    /// The Samsung-specific heuristics in ble/lib.rs (Accessory Service
    /// UUID, manufacturer ID 0x0075) only affect *discovery* — whether a
    /// device shows up as a candidate at all — never identity matching.
    async fn scan_for_best_match(&self, service_uuid: Uuid) -> Result<Option<(PairedDevice, i16)>> {
        let devices = self.session.list_devices().await?;
        if devices.is_empty() {
            return Ok(None);
        }

        let mut rx = self.ble.scan(service_uuid).await?;
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut best: Option<(PairedDevice, i16)> = None;

        while Instant::now() < deadline {
            match timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Some(info)) => {
                    if let Some(device) = Self::match_device(&devices, &info) {
                        if let Some(rssi) = info.rssi {
                            let better = best.as_ref().map_or(true, |(_, b)| rssi > *b);
                            if better {
                                best = Some((device.clone(), rssi));
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => break, // no events in the last 500ms — scan window is over
            }
        }
        let _ = self.ble.stop_scan().await;

        Ok(best)
    }

    fn match_device<'a>(devices: &'a [PairedDevice], info: &PeripheralInfo) -> Option<&'a PairedDevice> {
        devices.iter().find(|d| d.address == info.id)
            .or_else(|| {
                info.device_id.as_ref().and_then(|id| {
                    devices.iter().find(|d| d.device_id.as_deref() == Some(id.as_str()))
                })
            })
            .or_else(|| {
                info.name.as_ref().and_then(|name| {
                    devices.iter().find(|d| d.name.eq_ignore_ascii_case(name))
                })
            })
    }

    /// The actual crypto handshake: connect, send a fresh challenge, verify
    /// the signed response, and only then report success. This — not RSSI
    /// proximity alone — is what's supposed to gate unlocking.
    async fn attempt_unlock(&self, device: &PairedDevice) -> Result<()> {
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
        let result = self.attempt_unlock_inner(device.id, &conn).await;
        let _ = self.ble.disconnect(&conn).await;
        result
    }

    async fn attempt_unlock_inner(&self, device_id: Uuid, conn: &wristkey_ble::Connection) -> Result<()> {
        let challenge_char = Uuid::parse_str(CHALLENGE_CHAR)
            .map_err(|e| WristKeyError::Config(format!("invalid challenge UUID: {}", e)))?;
        let response_char = Uuid::parse_str(RESPONSE_CHAR)
            .map_err(|e| WristKeyError::Config(format!("invalid response UUID: {}", e)))?;

        let challenge = self.session.begin_unlock(device_id).await?;

        let mut rx = self.ble.notify(conn, response_char).await?;
        self.ble.write(conn, challenge_char, &challenge.to_bytes()).await?;

        let response_data = timeout(Duration::from_secs(10), rx.recv()).await
            .map_err(|_| WristKeyError::Ble("unlock response timeout".into()))?
            .ok_or_else(|| WristKeyError::Ble("BLE channel closed before response".into()))?;

        // Watch always sends signature(64) || user_present_flag(1) || public_key(65)
        // = 130 bytes regardless of context; unlock only needs the first 65.
        if response_data.len() < 65 {
            return Err(WristKeyError::Protocol(format!(
                "unlock response too short: {} bytes", response_data.len()
            )));
        }
        let signature = response_data[..64].to_vec();
        let user_present = response_data[64] != 0;

        let response = Response { signature, user_present, timestamp: Utc::now() };
        self.session.verify_unlock(&response).await?;
        self.platform.unlock_screen().await
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
            device_id: None,
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

        // Median instead of mean — RSSI has occasional large outliers
        // (multipath fades, momentary obstruction) that skew a plain
        // average more than they skew the middle value.
        samples.sort_unstable();
        let median = samples[samples.len() / 2];
        // Uses the person's own configured margin (Settings tab) instead of
        // a second, disconnected hardcoded number — previously this was a
        // separate fixed +5dB that had nothing to do with rssi_threshold_offset_dbm.
        let config = self.session.load_config().await?;
        let threshold = (median - config.rssi_threshold_offset_dbm).min(-20).max(-90);
        info!("Calibration: median={} dBm, threshold={} dBm ({} samples)", median, threshold, samples.len());

        let rssi_byte = threshold as i8;
        self.ble.write(&conn, config_char, &[0x02, rssi_byte as u8]).await?;
        info!("Sent CALIBRATION_RESULT: {} dBm", threshold);

        let _ = self.ble.disconnect(&conn).await;

        // The part that was entirely missing before: persist the new
        // baseline on the desktop side too, not just on the watch. Without
        // this, calibrating never actually changed when the PC locks.
        self.session.update_baseline_rssi(device_id, median).await?;

        Ok(threshold)
    }
}
