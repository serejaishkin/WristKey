//! Connection manager — tracks BLE connections and reuses them.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use wristkey_ble::{BleAdapter, Connection, PeripheralInfo};
use wristkey_core::{Result, WristKeyError};

pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<String, Connection>>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_or_connect(
        &self,
        adapter: &Arc<dyn BleAdapter>,
        info: &PeripheralInfo,
    ) -> Result<Connection> {
        // Windows can report a peripheral while an active scan is still running.
        // Trying to connect at that moment is a common source of:
        // "connect: Not connected". Always stop scanning before opening GATT.
        let _ = adapter.stop_scan().await;

        {
            let conns = self.connections.read().await;
            if let Some(conn) = conns.get(&info.id) {
                if adapter.read_rssi(conn).await.is_ok() {
                    return Ok(conn.clone());
                }
            }
        }

        // A Samsung Wear OS peripheral can reject the first connection while its
        // BLE/GATT stack is switching from advertising to connected state. Retry
        // with a clean disconnect between attempts.
        let mut last_error = None;
        for attempt in 1..=4 {
            let _ = adapter.stop_scan().await;
            match adapter.connect(info).await {
                Ok(conn) => {
                    self.connections.write().await.insert(info.id.clone(), conn.clone());
                    return Ok(conn);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < 4 {
                        sleep(Duration::from_millis(500 * attempt as u64)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| WristKeyError::Ble("connection failed".into())))
    }

    pub async fn disconnect(&self, adapter: &Arc<dyn BleAdapter>, id: &str) -> Result<()> {
        let mut conns = self.connections.write().await;
        if let Some(conn) = conns.remove(id) {
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