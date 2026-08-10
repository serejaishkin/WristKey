//! Stateless BLE connection manager for WristKey
//! 
//! Solves Windows BLE conflict:
//! - Presence detection via advertisement ONLY (no connect)
//! - Unlock via stateless connect → operation → immediate disconnect
//! - Cooldown prevents connect/disconnect loops

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use btleplug::api::{Central, CentralEvent, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Peripheral};
use tracing::{info, warn};
use uuid::Uuid;

const WRISTKEY_MANUF_ID: u16 = 0xFFFF;
const COOLDOWN: Duration = Duration::from_secs(5);
const PRESENCE_TIMEOUT: Duration = Duration::from_secs(12);
const RSSI_LOCK_THRESHOLD: i16 = -78;

#[derive(Debug, Clone)]
pub struct WristKeyPresence {
    pub last_seen: Instant,
    pub last_rssi: i16,
    pub device_id: Vec<u8>,
    pub pin: String,
}

pub struct ConnectionManager {
    presence: Mutex<HashMap<String, WristKeyPresence>>,
    cooldown_until: Mutex<Option<Instant>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            presence: Mutex::new(HashMap::new()),
            cooldown_until: Mutex::new(None),
        }
    }

    /// Обновляет presence по advertisement. НЕ требует connect().
    /// Работает даже если часы уже connected к Windows для уведомлений.
    pub async fn on_advertisement(&self, peripheral: &Peripheral) {
        if let Ok(Some(props)) = peripheral.properties().await {
            if let Some(manuf) = props.manufacturer_data.get(&WRISTKEY_MANUF_ID) {
                if manuf.len() >= 8 {
                    let pin = String::from_utf8_lossy(&manuf[..4]).to_string();
                    let device_id = manuf[4..8].to_vec();
                    let addr = peripheral.address().to_string();
                    let rssi = props.rssi.unwrap_or(-100);
                    
                    let mut p = self.presence.lock().await;
                    p.insert(addr.clone(), WristKeyPresence {
                        last_seen: Instant::now(),
                        last_rssi: rssi,
                        device_id,
                        pin,
                    });
                    
                    info!(%addr, %rssi, %pin, "Presence updated (advertisement only)");
                }
            }
        }
    }

    /// true = часы в зоне (видели advertisement < 12 сек и RSSI > -78)
    pub async fn is_present(&self, addr: &str) -> bool {
        let p = self.presence.lock().await;
        p.get(addr)
            .map(|s| s.last_seen.elapsed() < PRESENCE_TIMEOUT && s.last_rssi > RSSI_LOCK_THRESHOLD)
            .unwrap_or(false)
    }

    pub async fn get_rssi(&self, addr: &str) -> Option<i16> {
        self.presence.lock().await.get(addr).map(|s| s.last_rssi)
    }

    /// Stateless операция: connect → f() → disconnect → cooldown.
    /// Windows успевает восстановить своё соединение после disconnect.
    pub async fn execute_stateless<F, Fut>(&self, peripheral: &Peripheral, f: F) -> Result<(), String>
    where
        F: FnOnce(&Peripheral) -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        {
            let cd = self.cooldown_until.lock().await;
            if let Some(t) = *cd {
                if Instant::now() < t {
                    warn!("Cooldown active, skipping connect");
                    return Ok(());
                }
            }
        }

        info!(addr=%peripheral.address(), "Stateless connect for unlock");
        peripheral.connect().await.map_err(|e| format!("connect: {}", e))?;
        tokio::time::sleep(Duration::from_millis(400)).await;
        
        let res = f(peripheral).await;
        
        info!(addr=%peripheral.address(), "Stateless disconnect");
        let _ = peripheral.disconnect().await;
        *self.cooldown_until.lock().await = Some(Instant::now() + COOLDOWN);
        
        res
    }

    pub async fn cleanup(&self) {
        let mut p = self.presence.lock().await;
        let before = p.len();
        p.retain(|_, v| v.last_seen.elapsed() < PRESENCE_TIMEOUT);
        if before != p.len() {
            info!("Cleaned up {} stale presence entries", before - p.len());
        }
    }
}

/// Главный loop: только advertisement scanning, никаких persistent connections.
/// 
/// Использование в main.rs:
/// let mgr = Arc::new(ConnectionManager::new());
/// run_presence_loop(adapter, mgr.clone()).await?;
pub async fn run_presence_loop(
    adapter: Adapter,
    mgr: std::sync::Arc<ConnectionManager>,
) -> Result<(), Box<dyn std::error::Error>> {
    adapter.start_scan(ScanFilter::default()).await?;
    info!("Stateless presence loop started (advertisement-only)");
    
    let mut events = adapter.events().await?;
    while let Some(event) = events.next().await {
        match event {
            CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => {
                if let Ok(p) = adapter.peripheral(&id).await {
                    mgr.on_advertisement(&p).await;
                }
            }
            CentralEvent::DeviceDisconnected(id) => {
                info!(?id, "BLE disconnected (ignored, we use advertisements)");
            }
            _ => {}
        }
        mgr.cleanup().await;
    }
    Ok(())
}
