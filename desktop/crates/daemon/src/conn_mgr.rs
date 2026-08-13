//! Stateless BLE presence tracker for WristKey
//!
//! Solves Windows BLE conflict:
//! - Presence detection via advertisement ONLY (no connect)
//! - RSSI tracking without persistent connection
//! - Maps device_id (from advertisement) to MAC address

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use futures::StreamExt;
use btleplug::api::{Central, CentralEvent, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Peripheral};
use tracing::info;
use uuid::Uuid;

const WRISTKEY_MANUF_ID: u16 = 0xFFFF;
const SAMSUNG_MANUF_ID: u16 = 0x0075;
const SAMSUNG_SERVICE_UUID: &str = "0000fd50-0000-1000-8000-00805f9b34fb";
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
    device_id_to_addr: Mutex<HashMap<String, String>>,
    peripherals: Mutex<HashMap<String, Peripheral>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            presence: Mutex::new(HashMap::new()),
            device_id_to_addr: Mutex::new(HashMap::new()),
            peripherals: Mutex::new(HashMap::new()),
        }
    }

    pub async fn on_advertisement(&self, peripheral: &Peripheral) {
        if let Ok(Some(props)) = peripheral.properties().await {
            let addr = peripheral.address().to_string();
            let rssi = props.rssi.unwrap_or(-100);
            let svc_str: Vec<String> = props.services.iter().map(|u| u.to_string()).collect();

            // Check if this is a WristKey device or Samsung Galaxy Watch
            let has_wristkey_manuf = props.manufacturer_data.contains_key(&WRISTKEY_MANUF_ID);
            let is_samsung = props.manufacturer_data.contains_key(&SAMSUNG_MANUF_ID);
            let has_samsung_svc = props.services.iter().any(|u| {
                u.to_string().eq_ignore_ascii_case(SAMSUNG_SERVICE_UUID)
            });
            let is_watch_name = props.local_name.as_ref().map(|n| {
                let lower = n.to_lowercase();
                lower.contains("watch") || lower.contains("galaxy") || lower.contains("wristkey")
                    || lower.contains("gear") || lower.contains("active") || lower.contains("sm-r")
            }).unwrap_or(false);

            let is_wristkey = has_wristkey_manuf || is_samsung || has_samsung_svc || is_watch_name;

            if !is_wristkey {
                return;
            }

            let (pin, device_id) = if has_wristkey_manuf {
                let manuf = props.manufacturer_data.get(&WRISTKEY_MANUF_ID).unwrap();
                let pin = String::from_utf8_lossy(&manuf[..4.min(manuf.len())]).to_string();
                let device_id = if manuf.len() >= 8 { manuf[4..8].to_vec() } else { vec![] };
                (pin, device_id)
            } else {
                // Samsung fallback: use last 4 chars of MAC as pseudo-device-id
                let pseudo_id = if addr.len() >= 4 {
                    addr[addr.len()-4..].bytes().collect()
                } else {
                    addr.bytes().collect()
                };
                ("----".into(), pseudo_id)
            };

            let device_id_hex = hex::encode(&device_id);

            let mut p = self.presence.lock().await;
            p.insert(addr.clone(), WristKeyPresence {
                last_seen: Instant::now(),
                last_rssi: rssi,
                device_id: device_id.clone(),
                pin: pin.clone(),
            });

            let mut m = self.device_id_to_addr.lock().await;
            if !device_id_hex.is_empty() {
                m.insert(device_id_hex, addr.clone());
            }

            let mut per = self.peripherals.lock().await;
            per.insert(addr.clone(), peripheral.clone());

            info!(%addr, %rssi, %pin, %is_samsung, %has_samsung_svc, "Presence updated");
        }
    }

    pub async fn is_present(&self, addr: &str) -> bool {
        let p = self.presence.lock().await;
        p.get(addr)
            .map(|s| s.last_seen.elapsed() < PRESENCE_TIMEOUT && s.last_rssi > RSSI_LOCK_THRESHOLD)
            .unwrap_or(false)
    }

    pub async fn is_present_by_device_id(&self, device_id_hex: &str) -> bool {
        let m = self.device_id_to_addr.lock().await;
        if let Some(addr) = m.get(device_id_hex) {
            self.is_present(addr).await
        } else {
            false
        }
    }

    pub async fn get_rssi(&self, addr: &str) -> Option<i16> {
        self.presence.lock().await.get(addr).map(|s| s.last_rssi)
    }

    pub async fn get_rssi_by_device_id(&self, device_id_hex: &str) -> Option<i16> {
        let m = self.device_id_to_addr.lock().await;
        if let Some(addr) = m.get(device_id_hex) {
            self.get_rssi(addr).await
        } else {
            None
        }
    }

    pub async fn get_addr_by_device_id(&self, device_id_hex: &str) -> Option<String> {
        self.device_id_to_addr.lock().await.get(device_id_hex).cloned()
    }

    pub async fn get_peripheral(&self, addr: &str) -> Option<Peripheral> {
        self.peripherals.lock().await.get(addr).cloned()
    }

    pub async fn clear(&self) {
        let mut p = self.presence.lock().await;
        let count = p.len();
        p.clear();
        let mut m = self.device_id_to_addr.lock().await;
        m.clear();
        let mut per = self.peripherals.lock().await;
        per.clear();
        info!("Cleared {} presence entries", count);
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

pub async fn run_presence_loop(
    adapter: Adapter,
    mgr: Arc<ConnectionManager>,
) -> Result<(), Box<dyn std::error::Error>> {
    adapter.start_scan(ScanFilter::default()).await?;
    info!("Presence loop started (advertisement-only)");

    let mut events = adapter.events().await?;
    while let Some(event) = events.next().await {
        match event {
            CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => {
                if let Ok(p) = adapter.peripheral(&id).await {
                    mgr.on_advertisement(&p).await;
                }
            }
            CentralEvent::DeviceDisconnected(id) => {
                info!(?id, "BLE disconnected (ignored, advertisement-only mode)");
            }
            _ => {}
        }
        mgr.cleanup().await;
    }
    Ok(())
}
