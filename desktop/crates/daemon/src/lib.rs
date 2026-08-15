//! WristKey daemon — proximity detection, crypto unlock, and auto-lock.

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

/// Debounce counter — requires N consecutive weak readings before locking.
pub struct DebounceCounter {
    threshold: usize,
    count: usize,
}

impl DebounceCounter {
    pub fn new(threshold: usize) -> Self {
        Self { threshold, count: 0 }
    }

    /// Call with `true` if signal is weak (should lock), `false` if strong.
    /// Returns `true` when threshold reached.
    pub fn tick(&mut self, weak: bool) -> bool {
        if weak {
            self.count += 1;
            self.count >= self.threshold
        } else {
            self.count = 0;
            false
        }
    }

    pub fn reset(&mut self) {
        self.count = 0;
    }
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
        let baseline = -60i16; // default, updated after first calibration
        Self {
            session,
            ble,
            platform,
            conn_mgr,
            smoother: Mutex::new(RssiSmoother::new(baseline)),
            debounce: Mutex::new(DebounceCounter::new(3)),
        }
    }

    /// Main daemon loop.
    pub async fn run(&self) -> Result<()> {
        let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
        let mut ticker = interval(Duration::from_secs(2));

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
                    info!("Watch nearby and screen locked -> attempting crypto unlock");
                    if let Err(e) = self.unlock_with_crypto(&service_uuid, &devices).await {
                        warn!("Crypto unlock failed: {}", e);
                    }
                }
                ProximityAction::Lock if !is_locked && session_state.is_authenticated() => {
                    info!("Watch far away and screen unlocked -> locking");
                    if let Err(e) = self.platform.lock_screen().await {
                        warn!("Lock screen failed: {}", e);
                    }
                    self.session.disconnect().await;
                }
                _ => {}
            }
        }
    }

    /// Scan for paired watch, apply RSSI smoothing + hysteresis + debounce.
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
            if remaining.is_zero() {
                break;
            }
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
                            debug!(
                                "Device {} raw_rssi={} smoothed={:?} should_unlock={} changed={}",
                                device.name, rssi, smoother.current_rssi(), should_unlock, changed
                            );

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
            if should_lock {
                return Ok(ProximityAction::Lock);
            }
        }

        Ok(ProximityAction::None)
    }

    /// Perform cryptographic unlock via BLE challenge-response.
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
            warn!("Challenge write attempt {} failed, retrying...", attempt);
            sleep(Duration::from_millis(300)).await;
        }
        if !write_ok {
            let _ = self.ble.disconnect(&conn).await;
            return Err(WristKeyError::Ble("failed to write challenge after 3 attempts".into()));
        }

        let mut rx = self.ble.notify(&conn, response_char).await?;

        // FIX: expect 65 bytes (64 sig + 1 user_present), not 130
        // Public key is already stored in PairedDevice from pairing
        let response_data = match timeout(
            Duration::from_secs(10),
            rx.recv()
        ).await {
            Ok(Some(d)) => d,
            _ => {
                let _ = self.ble.disconnect(&conn).await;
                return Err(WristKeyError::Ble("timeout waiting for unlock response".into()));
            }
        };

        if response_data.len() < 65 {
            let _ = self.ble.disconnect(&conn).await;
            return Err(WristKeyError::Protocol(format!(
                "invalid response: {} bytes (expected at least 65)", response_data.len()
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
        info!("Screen unlocked via crypto challenge-response");

        Ok(())
    }
}
