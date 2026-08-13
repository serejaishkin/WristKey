//! WristKey daemon library — GUI, connection manager, and presence loop.

pub mod conn_mgr;
pub mod gui;
pub mod pair_gui;
pub mod tray;

use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, sleep};
use tracing::{info, warn};
use uuid::Uuid;
use wristkey_core::{SessionManager, PlatformSecurity, Result, WristKeyError, Storage};
use wristkey_ble::{BleAdapter, PeripheralInfo};
use conn_mgr::ConnectionManager;

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
        loop {
            sleep(Duration::from_secs(60)).await;
        }
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
        
        // FIX: real CONFIG_CHAR UUID from WristKeyBleService.kt
        let config_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567894")
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