//! Connection manager — tracks BLE connections and reuses them.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use wristkey_ble::{BleAdapter, Connection, PeripheralInfo};
use wristkey_core::Result;

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
        let mut conns = self.connections.write().await;
        if let Some(conn) = conns.get(&info.id) {
            // Verify connection is still alive by reading RSSI
            if adapter.read_rssi(conn).await.is_ok() {
                return Ok(conn.clone());
            }
        }
        let conn = adapter.connect(info).await?;
        conns.insert(info.id.clone(), conn.clone());
        Ok(conn)
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
