//! Connection manager — tracks BLE connections and reuses them.
//!
//! Windows note: the Bluetooth address exposed by advertisements is not a
//! reliable persistent key for a Wear OS device. A paired watch can advertise
//! again with a different address after a process/adapter restart. Therefore
//! reconnect first resolves the currently advertised peripheral by the saved
//! device name/device id, then connects using that fresh peripheral id.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, timeout, Duration, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;
use wristkey_ble::{BleAdapter, Connection, PeripheralInfo};
use wristkey_core::{Result, WristKeyError};

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<String, Connection>>>,
    // BT connection is the product priority. Remember the last successfully
    // connected peripheral id (current Windows address) for each saved device
    // so reconnect first tries a DIRECT GATT connect with no scanning at all.
    // Discovery is only the fallback when the direct link cannot be made.
    resolved: Arc<RwLock<HashMap<String, String>>>,
    reconnect_lock: Arc<Mutex<()>>,
    last_attempt: Arc<Mutex<Option<Instant>>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            resolved: Arc::new(RwLock::new(HashMap::new())),
            reconnect_lock: Arc::new(Mutex::new(())),
            last_attempt: Arc::new(Mutex::new(None)),
        }
    }

    /// Return a cached connection without initiating a BLE scan or reconnect.
    /// UI/status code should use this instead of get_or_connect so it cannot
    /// create a second reconnect loop while the daemon is already reconnecting.
    pub async fn get_cached(&self, id: &str) -> Option<Connection> {
        self.connections.read().await.get(id).cloned()
    }

    async fn resolve_current_peripheral(
        &self,
        adapter: &Arc<dyn BleAdapter>,
        info: &PeripheralInfo,
    ) -> Result<PeripheralInfo> {
        let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
        let mut rx = adapter.scan(service_uuid).await?;
        let deadline = Instant::now() + Duration::from_secs(8);

        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match timeout(remaining.min(Duration::from_millis(500)), rx.recv()).await {
                Ok(Some(candidate)) => {
                    let address_match = candidate.id.eq_ignore_ascii_case(&info.id);
                    let name_match = match (&info.name, &candidate.name) {
                        (Some(saved), Some(found)) => saved.eq_ignore_ascii_case(found),
                        _ => false,
                    };
                    let device_id_match = match (&info.device_id, &candidate.device_id) {
                        (Some(saved), Some(found)) => saved.eq_ignore_ascii_case(found),
                        _ => false,
                    };

                    // A Galaxy Watch can expose several BLE endpoints. Matching
                    // the saved name alone is not sufficient in theory, but in
                    // RECONNECT mode (paired watch) Android does not advertise
                    // manufacturer data, and WinRT frequently fails to surface
                    // custom 128-bit service UUIDs from advertisements. Rejecting
                    // candidates without WristKey advertisement markers made
                    // reconnect impossible after address rotation or a restart.
                    // BleAdapter::connect therefore remains the authoritative
                    // validation: it disconnects any peer that does not expose
                    // the WristKey custom GATT service.
                    let wristkey_match = candidate.service_uuids.iter().any(|uuid| uuid.eq(&service_uuid));
                    let matched = address_match || device_id_match || name_match || wristkey_match;
                    if matched {
                        let wristkey_advertised =
                            candidate.service_uuids.iter().any(|uuid| uuid.eq(&service_uuid))
                                || candidate.raw_manufacturer_data.is_some()
                                || candidate.device_id.is_some();
                        debug!(
                            "BLE reconnect resolved: saved_id={} -> current_id={} name={:?} wristkey_advertised={}",
                            info.id, candidate.id, candidate.name, wristkey_advertised
                        );
                        let _ = adapter.stop_scan().await;
                        return Ok(candidate);
                    }
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }

        let _ = adapter.stop_scan().await;
        Err(WristKeyError::Ble(format!(
            "paired BLE peripheral not discovered with WristKey advertisement (saved id {}, name {:?})",
            info.id, info.name
        )))
    }

    pub async fn get_or_connect(
        &self,
        adapter: &Arc<dyn BleAdapter>,
        info: &PeripheralInfo,
    ) -> Result<Connection> {
        let _ = adapter.stop_scan().await;

        if let Some(conn) = self.connections.read().await.get(&info.id).cloned() {
            match adapter.read_rssi(&conn).await {
                Ok(rssi) => {
                    debug!("BLE connection alive for {} (RSSI {})", info.id, rssi);
                    return Ok(conn);
                }
                Err(e) => {
                    warn!("BLE connection stale for {}: {}; removing cached connection", info.id, e);
                    self.connections.write().await.remove(&info.id);
                    let _ = adapter.disconnect(&conn).await;
                }
            }
        }

        // Daemon and UI can ask for a connection at the same time. Serialize
        // discovery/connect so WinRT never gets several competing scans.
        let _guard = self.reconnect_lock.lock().await;

        if let Some(conn) = self.connections.read().await.get(&info.id).cloned() {
            if adapter.read_rssi(&conn).await.is_ok() {
                return Ok(conn);
            }
        }

        {
            let mut last = self.last_attempt.lock().await;
            if let Some(previous) = *last {
                if previous.elapsed() < Duration::from_secs(4) {
                    return Err(WristKeyError::Ble("reconnect throttled; waiting for next retry".into()));
                }
            }
            *last = Some(Instant::now());
        }

        let mut last_error = None;
        for attempt in 1..=3 {
            let _ = adapter.stop_scan().await;
            info!("BLE reconnect attempt {}/{}", attempt, 3);

            // BT connection first: a paired watch that was connected before is
            // most likely still reachable at the same Windows peripheral id.
            // Try the direct GATT connect with zero scanning; only fall back
            // to discovery if the direct link fails (address rotated, reboot).
            let remembered = self.resolved.read().await.get(&info.id).cloned();
            if let Some(current_id) = remembered {
                let candidate = PeripheralInfo {
                    id: current_id.clone(),
                    name: info.name.clone(),
                    pin: None,
                    device_id: info.device_id.clone(),
                    rssi: None,
                    service_uuids: vec![Uuid::parse_str(SERVICE_UUID).unwrap()],
                    raw_manufacturer_data: None,
                };
                match adapter.connect(&candidate).await {
                    Ok(conn) => {
                        info!("BLE reconnect direct (BT-first) for {} -> {}", info.id, current_id);
                        self.resolved.write().await.insert(info.id.clone(), current_id);
                        self.connections.write().await.insert(info.id.clone(), conn.clone());
                        return Ok(conn);
                    }
                    Err(e) => {
                        warn!("BT-first direct connect to {} failed: {}; falling back to discovery", current_id, e);
                        self.resolved.write().await.remove(&info.id);
                    }
                }
            }

            // Resolve the current Windows peripheral before connecting. This
            // handles address changes across application restarts.
            let current = match self.resolve_current_peripheral(adapter, info).await {
                Ok(current) => current,
                Err(e) => {
                    warn!("BLE discovery attempt {}/3 failed for {}: {}", attempt, info.id, e);
                    last_error = Some(e);
                    if attempt < 3 {
                        sleep(Duration::from_secs(attempt as u64)).await;
                    }
                    continue;
                }
            };

            match adapter.connect(&current).await {
                Ok(conn) => {
                    info!("BLE reconnect successful for {} (current id {})", info.id, current.id);
                    self.resolved.write().await.insert(info.id.clone(), current.id.clone());
                    self.connections.write().await.insert(info.id.clone(), conn.clone());
                    return Ok(conn);
                }
                Err(e) => {
                    warn!("BLE connect attempt {}/3 failed for {}: {}", attempt, info.id, e);
                    last_error = Some(e);
                    if attempt < 3 {
                        sleep(Duration::from_millis(700 * attempt as u64)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| WristKeyError::Ble("connection failed".into())))
    }

    pub async fn disconnect(&self, adapter: &Arc<dyn BleAdapter>, id: &str) -> Result<()> {
        let conn = self.connections.write().await.remove(id);
        if let Some(conn) = conn {
            adapter.disconnect(&conn).await?;
        }
        Ok(())
    }

    pub async fn disconnect_all(&self, adapter: &Arc<dyn BleAdapter>) {
        let ids: Vec<String> = self.connections.read().await.keys().cloned().collect();
        for id in ids {
            let _ = self.disconnect(adapter, &id).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_starts_empty() {
        let manager = ConnectionManager::new();
        let _ = manager;
    }
}
