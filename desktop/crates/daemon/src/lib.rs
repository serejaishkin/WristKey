//! WristKey daemon library — GUI, connection manager, and presence loop.

pub mod conn_mgr;

use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, timeout, Instant};
use tracing::{info, warn};
use uuid::Uuid;
use wristkey_core::{SessionManager, PlatformSecurity, Result, WristKeyError};
use wristkey_ble::{BleAdapter, PeripheralInfo};
use conn_mgr::ConnectionManager;

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CONFIG_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567894";

#[derive(Debug, Clone, Copy, PartialEq)]
enum ProximityAction {
    Unlock,
    Lock,
    None,
}

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

        let mut ticker = interval(Duration::from_secs(5));
        let mut last_unlocked: Option<Instant> = None;
        let mut _last_locked: Option<Instant> = None;

        loop {
            ticker.tick().await;

            match self.check_proximity().await {
                Ok(ProximityAction::Unlock) => {
                    match self.platform.is_locked().await {
                        Ok(true) => {
                            if let Err(e) = self.platform.unlock_screen().await {
                                warn!("Unlock failed: {}", e);
                            } else {
                                info!("Auto-unlocked: watch in range");
                                last_unlocked = Some(Instant::now());
                            }
                        }
                        Ok(false) => {}
                        Err(e) => warn!("is_locked check failed: {}", e),
                    }
                }
                Ok(ProximityAction::Lock) => {
                    match self.platform.is_locked().await {
                        Ok(false) => {
                            if let Err(e) = self.platform.lock_screen().await {
                                warn!("Lock failed: {}", e);
                            } else {
                                info!("Auto-locked: watch out of range");
                                _last_locked = Some(Instant::now());
                            }
                        }
                        Ok(true) => {}
                        Err(e) => warn!("is_locked check failed: {}", e),
                    }
                }
                Ok(ProximityAction::None) => {}
                Err(e) => warn!("Proximity check failed: {}", e),
            }

            // Auto-lock by timeout if no watch seen for N seconds
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

        while Instant::now() < deadline {
            match timeout(Duration::from_millis(500), rx.recv()).await {
                Ok(Some(info)) => {
                    // Try to match by address first, then by name for Samsung watches
                    let matched = devices.iter().find(|d| d.address == info.id)
                        .or_else(|| {
                            if let Some(ref name) = info.name {
                                devices.iter().find(|d| d.name.eq_ignore_ascii_case(name))
                            } else {
                                None
                            }
                        });

                    if let Some(device) = matched {
                        if let Some(rssi) = info.rssi {
                            let threshold = device.baseline_rssi - config.rssi_threshold_offset_dbm;
                            if rssi > threshold {
                                return Ok(ProximityAction::Unlock);
                            }
                            if best_rssi.map_or(true, |best| rssi > best) {
                                best_rssi = Some(rssi);
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        if best_rssi.is_some() {
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

        let avg = samples.iter().sum::<i16>() / samples.len() as i16;
        let threshold = avg.saturating_add(5).min(-20).max(-90);
        info!("Calibration: avg={} dBm, threshold={} dBm ({} samples)", avg, threshold, samples.len());

        let rssi_byte = threshold as i8;
        self.ble.write(&conn, config_char, &[0x02, rssi_byte as u8]).await?;
        info!("Sent CALIBRATION_RESULT: {} dBm", threshold);

        let _ = self.ble.disconnect(&conn).await;
        Ok(threshold)
    }
}
