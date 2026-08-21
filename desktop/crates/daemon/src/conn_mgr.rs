//! Connection manager — tracks BLE connections and reuses them.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};
use wristkey_ble::{BleAdapter, Connection, PeripheralInfo};
use wristkey_core::{Result, WristKeyError};

pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<String, Connection>>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self { connections: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub async fn get_or_connect(
        &self,
        adapter: &Arc<dyn BleAdapter>,
        info: &PeripheralInfo,
    ) -> Result<Connection> {
        // Windows can report a peripheral while an active scan is still running.
        // Never open GATT while scanning.
        let _ = adapter.stop_scan().await;

        // Reuse the existing connection only if it is still alive. A process
        // restart cannot preserve this map, so the first call after restart
        // always reaches the reconnect path below.
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

        // A Samsung Wear OS peripheral can reject the first connection while its
        // BLE/GATT stack is switching from advertising to connected state.
        // Retry with increasing delays and always perform a fresh connect.
        let mut last_error = None;
        for attempt in 1..=6 {
            let _ = adapter.stop_scan().await;
            info!("BLE reconnect attempt {}/6 for {}", attempt, info.id);
            match adapter.connect(info).await {
                Ok(conn) => {
                    info!("BLE reconnect successful for {}", info.id);
                    self.connections.write().await.insert(info.id.clone(), conn.clone());
                    return Ok(conn);
                }
                Err(e) => {
                    warn!("BLE connect attempt {}/6 failed for {}: {}", attempt, info.id, e);
                    last_error = Some(e);
                    if attempt < 6 {
                        sleep(Duration::from_millis(500 * attempt as u64)).await;
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
