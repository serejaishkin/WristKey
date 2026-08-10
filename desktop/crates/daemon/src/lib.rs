//! WristKey daemon — main application logic.

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, timeout, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::Utc;
use wristkey_core::*;
pub mod conn_mgr;
pub mod pair_gui;
use crate::conn_mgr::ConnectionManager;
use wristkey_ble::{BleAdapter, Connection, PeripheralInfo};

pub struct Daemon {
    session: Arc<SessionManager>,
    ble: Arc<dyn BleAdapter>,
    platform: Arc<dyn PlatformSecurity>,
    conn_mgr: Arc<ConnectionManager>,
    service_uuid: Uuid,
    challenge_char: Uuid,
    response_char: Uuid,
    current_conn: Arc<RwLock<Option<Connection>>>,
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
            service_uuid: Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap(),
            challenge_char: Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567891").unwrap(),
            response_char: Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567892").unwrap(),
            current_conn: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn run(&self) -> Result<()> {
        let mut tick = interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            if let Err(e) = self.tick().await {
                error!("tick error: {}", e);
                self.cleanup().await;
            }
        }
    }

    pub async fn run_once(&self) -> Result<()> {
        self.tick().await
    }

    pub async fn pair_once(&self) -> Result<PairedDevice> {
        info!("Pairing mode: scanning for 30 seconds...");
        let mut rx = self.ble.scan(self.service_uuid).await?;
        let info = timeout(Duration::from_secs(30), rx.recv())
            .await
            .map_err(|_| WristKeyError::Ble("scan timeout".into()))?
            .ok_or_else(|| WristKeyError::Ble("no devices found".into()))?;

        info!("Found device: {} ({})", info.name.as_deref().unwrap_or("unknown"), info.id);
        let conn = self.ble.connect(&info).await?;
        self.perform_pairing(&conn, info).await
    }

    async fn tick(&self) -> Result<()> {
        match self.session.state().await {
            SessionState::Disconnected => {
                info!("state: disconnected, scanning...");
                self.try_connect().await?;
            }
            SessionState::Authenticated { device_id, .. } => {
                debug!("state: authenticated, checking RSSI for {}", device_id);
                if let Some(conn) = self.current_conn.read().await.as_ref() {
                    let rssi = self.ble.read_rssi(conn).await?;
                    let should_lock = self.session.update_rssi(rssi).await?;
                    if should_lock {
                        info!("RSSI too low ({} dBm), locking screen", rssi);
                        self.platform.lock_screen().await?;
                        self.session.disconnect().await;
                        self.cleanup().await;
                    }
                } else {
                    warn!("authenticated but no connection, resetting state");
                    self.session.disconnect().await;
                }
            }
            SessionState::Verifying { .. } => {
                debug!("state: verifying unlock challenge...");
            }
            SessionState::Pairing { .. } => {
                debug!("state: pairing in progress...");
            }
            SessionState::Locked => {
                debug!("state: locked");
            }
        }
        Ok(())
    }

    async fn try_connect(&self) -> Result<()> {
        let mut rx = self.ble.scan(self.service_uuid).await?;
        let info = timeout(Duration::from_secs(10), rx.recv())
            .await
            .map_err(|_| WristKeyError::Ble("scan timeout".into()))?
            .ok_or_else(|| WristKeyError::Ble("no devices found".into()))?;

        info!("found device: {:?}", info.name);
        let conn = self.ble.connect(&info).await?;
        *self.current_conn.write().await = Some(conn.clone());

        let devices = self.session.list_devices().await?;
        // Match by static device_id (survives MAC randomization), fallback to any paired device
        let matched = devices.clone().into_iter().find(|d| {
            info.device_id.as_ref().map(|id| d.device_id.as_ref() == Some(id)).unwrap_or(false)
        }).or_else(|| {
            if devices.len() == 1 { devices.into_iter().next() } else { None }
        });

        if let Some(device) = matched {
            info!("matched device: {} (id={:?})", device.name, device.device_id);
            self.perform_unlock(&conn, device.id).await?;
            self.cleanup().await;
            return Ok(());
        } else {
            info!("no matched paired device, starting pairing");
            let _device = self.perform_pairing(&conn, info).await?;
            self.cleanup().await;
            return Ok(());
        }
    }

    async fn write_with_retry(&self, conn: &Connection, char: Uuid, data: &[u8]) -> Result<()> {
        // Windows BLE needs a moment after discovery before accepting writes
        tokio::time::sleep(Duration::from_millis(300)).await;
        for attempt in 1..=3 {
            match self.ble.write(conn, char, data).await {
                Ok(()) => return Ok(()),
                Err(e) if attempt < 3 => {
                    warn!("write attempt {} failed: {}, retrying...", attempt, e);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    async fn perform_pairing(&self, conn: &Connection, info: PeripheralInfo) -> Result<PairedDevice> {
        let challenge = self.session.begin_pairing().await?;
        self.write_with_retry(conn, self.challenge_char, &challenge.to_bytes()).await?;

        let mut rx = self.ble.notify(conn, self.response_char).await?;
        let response_data = timeout(Duration::from_secs(10), rx.recv())
            .await
            .map_err(|_| WristKeyError::Ble("pairing timeout".into()))?
            .ok_or_else(|| WristKeyError::Ble("no pairing response".into()))?;

        // Watch always sends: raw_signature(64) || user_present(1) || public_key(rest).
        // This MUST match wristkey-ble/WristKeyBleService.handleChallenge byte order.
        if response_data.len() < 66 {
            return Err(WristKeyError::Protocol(
                format!("pairing response too short: {} bytes", response_data.len())
            ));
        }
        let signature = response_data[..64].to_vec();
        let user_present = response_data[64] != 0;
        let public_key = response_data[65..].to_vec();

        let response = Response {
            signature,
            user_present,
            timestamp: Utc::now(),
        };

        let baseline_rssi = self.ble.read_rssi(conn).await?;
        let device_id = response_data.get(130..134).map(|b| hex::encode(b));
        self.session.complete_pairing(
            info.name.unwrap_or_else(|| "Unknown Watch".into()),
            public_key,
            device_id,
            &response,
            baseline_rssi,
        ).await
    }

    async fn perform_unlock(&self, conn: &Connection, device_id: Uuid) -> Result<()> {
        // begin_unlock stores this exact challenge in session state so that
        // verify_unlock checks the signature against what was actually sent.
        let challenge = self.session.begin_unlock(device_id).await?;
        info!("🖐️ Двигайте рукой на часах для подтверждения разблокировки");
        println!("🖐️ Двигайте рукой на часах для подтверждения разблокировки");
        self.write_with_retry(conn, self.challenge_char, &challenge.to_bytes()).await?;

        let mut rx = self.ble.notify(conn, self.response_char).await?;
        let response_data = timeout(Duration::from_secs(10), rx.recv())
            .await
            .map_err(|_| WristKeyError::Ble("unlock timeout".into()))?
            .ok_or_else(|| WristKeyError::Ble("no unlock response".into()))?;

        if response_data.len() < 65 {
            return Err(WristKeyError::Protocol(
                format!("unlock response too short: {} bytes", response_data.len())
            ));
        }
        let signature = response_data[..64].to_vec();
        let user_present = response_data[64] != 0;

        let response = Response {
            signature,
            user_present,
            timestamp: Utc::now(),
        };

        self.session.verify_unlock(&response).await
    }

    async fn cleanup(&self) {
        if let Some(conn) = self.current_conn.write().await.take() {
            let _ = self.ble.disconnect(&conn).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wristkey_ble::MockBleAdapter;

    #[tokio::test]
    async fn test_platform_lock_called() {
        let platform = Arc::new(MockPlatformSecurity::new());
        assert!(!platform.is_locked().await.unwrap());
        platform.lock_screen().await.unwrap();
        assert!(platform.is_locked().await.unwrap());
    }

    #[tokio::test]
    async fn test_e2e_pairing_flow() {
        let crypto = Arc::new(EcdsaP256Crypto);
        let storage = Arc::new(MemoryStorage::new());
        let session = Arc::new(SessionManager::new(crypto.clone(), storage));
        let _daemon = Daemon::new(session.clone(), Arc::new(MockBleAdapter::new()), Arc::new(MockPlatformSecurity::new()), Arc::new(ConnectionManager::new()));

        let (priv_key, pub_key) = crypto.generate_keypair().await.unwrap();
        let challenge = session.begin_pairing().await.unwrap();
        let sig = crypto.sign(&priv_key, &challenge.to_bytes()).await.unwrap();
        let response = Response { signature: sig, user_present: true, timestamp: Utc::now() };
        let device = session.complete_pairing("Mock Watch".into(), pub_key, None, &response, -50).await.unwrap();
        assert_eq!(device.name, "Mock Watch");
        assert!(session.state().await.is_authenticated());
    }

    #[tokio::test]
    async fn test_e2e_auto_lock_on_rssi_drop() {
        let crypto = Arc::new(EcdsaP256Crypto);
        let storage = Arc::new(MemoryStorage::new());
        let session = Arc::new(SessionManager::new(crypto.clone(), storage.clone()));
        let ble = Arc::new(MockBleAdapter::new());
        let platform = Arc::new(MockPlatformSecurity::new());
        let daemon = Daemon::new(session.clone(), ble.clone(), platform.clone(), Arc::new(ConnectionManager::new()));

        let (priv_key, pub_key) = crypto.generate_keypair().await.unwrap();
        let challenge = session.begin_pairing().await.unwrap();
        let sig = crypto.sign(&priv_key, &challenge.to_bytes()).await.unwrap();
        let response = Response { signature: sig, user_present: true, timestamp: Utc::now() };
        session.complete_pairing("Mock Watch".into(), pub_key, None, &response, -50).await.unwrap();

        let conn = Connection { peripheral_id: "AA:BB:CC:DD:EE:FF".into(), device_name: "Mock Watch".into() };
        daemon.current_conn.write().await.replace(conn);

        ble.queue_rssi(-55);
        daemon.run_once().await.unwrap();
        assert!(!platform.is_locked().await.unwrap());
        assert!(session.state().await.is_authenticated());

        ble.queue_rssi(-70);
        daemon.run_once().await.unwrap();
        assert!(platform.is_locked().await.unwrap());
        assert!(!session.state().await.is_authenticated());
    }
}
