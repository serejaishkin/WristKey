//! BLE abstraction layer over `btleplug`.

use async_trait::async_trait;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use wristkey_core::{Result, WristKeyError};

#[derive(Clone, Debug)]
pub struct PeripheralInfo {
    pub id: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub service_uuids: Vec<Uuid>,
}

#[derive(Clone, Debug)]
pub struct Connection {
    pub peripheral_id: String,
    pub device_name: String,
}

#[async_trait]
pub trait BleAdapter: Send + Sync {
    async fn scan(&self, service_uuid: Uuid) -> Result<mpsc::Receiver<PeripheralInfo>>;
    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection>;
    async fn disconnect(&self, conn: &Connection) -> Result<()>;
    async fn write(&self, conn: &Connection, characteristic: Uuid, data: &[u8]) -> Result<()>;
    async fn notify(&self, conn: &Connection, characteristic: Uuid) -> Result<mpsc::Receiver<Vec<u8>>>;
    async fn read_rssi(&self, conn: &Connection) -> Result<i16>;
}

pub struct BtleplugAdapter {
    _manager: Manager,
    adapter: Adapter,
    connected: Arc<RwLock<HashMap<String, Peripheral>>>,
}

impl BtleplugAdapter {
    pub async fn new() -> Result<Self> {
        let manager = Manager::new().await.map_err(|e| WristKeyError::Ble(format!("manager: {}", e)))?;
        let adapters = manager.adapters().await.map_err(|e| WristKeyError::Ble(format!("adapters: {}", e)))?;
        let adapter = adapters.into_iter().next().ok_or_else(|| WristKeyError::Ble("no BLE adapter".into()))?;
        info!("BLE adapter ready");
        Ok(Self {
            _manager: manager,
            adapter,
            connected: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    async fn get_connected(&self, peripheral_id: &str) -> Result<Peripheral> {
        self.connected.read().await
            .get(peripheral_id)
            .cloned()
            .ok_or_else(|| WristKeyError::Ble(format!("not connected: {}", peripheral_id)))
    }
}

#[async_trait]
impl BleAdapter for BtleplugAdapter {
    async fn scan(&self, service_uuid: Uuid) -> Result<mpsc::Receiver<PeripheralInfo>> {
        let (_tx, rx) = mpsc::channel(32);
        let filter = ScanFilter { services: vec![service_uuid] };
        self.adapter.start_scan(filter).await
            .map_err(|e| WristKeyError::Ble(format!("scan: {}", e)))?;
        let _adapter = self.adapter.clone();
        tokio::spawn(async move {
            todo!("BLE event loop")
        });
        Ok(rx)
    }
    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection> {
        debug!("connecting to {}", info.id);
        todo!("btleplug connect")
    }
    async fn disconnect(&self, conn: &Connection) -> Result<()> {
        debug!("disconnecting {}", conn.peripheral_id);
        todo!("btleplug disconnect")
    }
    async fn write(&self, _conn: &Connection, characteristic: Uuid, data: &[u8]) -> Result<()> {
        debug!("write {} bytes to {}", data.len(), characteristic);
        todo!("GATT write")
    }
    async fn notify(&self, _conn: &Connection, characteristic: Uuid) -> Result<mpsc::Receiver<Vec<u8>>> {
        debug!("subscribe {}", characteristic);
        todo!("GATT notify")
    }
    async fn read_rssi(&self, conn: &Connection) -> Result<i16> {
        debug!("rssi for {}", conn.peripheral_id);
        todo!("RSSI read")
    }
}

pub struct MockBleAdapter {
    scripted: std::sync::Mutex<Vec<Vec<u8>>>,
    scripted_rssi: std::sync::Mutex<Vec<i16>>,
}

impl MockBleAdapter {
    pub fn new() -> Self {
        Self {
            scripted: std::sync::Mutex::new(Vec::new()),
            scripted_rssi: std::sync::Mutex::new(Vec::new()),
        }
    }
    pub fn queue_response(&self, data: Vec<u8>) {
        self.scripted.lock().unwrap().push(data);
    }
    pub fn queue_rssi(&self, rssi: i16) {
        self.scripted_rssi.lock().unwrap().push(rssi);
    }
}

#[async_trait]
impl BleAdapter for MockBleAdapter {
    async fn scan(&self, service_uuid: Uuid) -> Result<mpsc::Receiver<PeripheralInfo>> {
        let (tx, rx) = mpsc::channel(4);
        let _ = tx.send(PeripheralInfo {
            id: "AA:BB:CC:DD:EE:FF".into(),
            name: Some("Mock Watch".into()),
            rssi: Some(-45),
            service_uuids: vec![service_uuid],
        }).await;
        Ok(rx)
    }
    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection> {
        Ok(Connection {
            peripheral_id: info.id.clone(),
            device_name: info.name.clone().unwrap_or_default(),
        })
    }
    async fn disconnect(&self, _conn: &Connection) -> Result<()> { Ok(()) }
    async fn write(&self, _conn: &Connection, _char: Uuid, _data: &[u8]) -> Result<()> { Ok(()) }
    async fn notify(&self, _conn: &Connection, _char: Uuid) -> Result<mpsc::Receiver<Vec<u8>>> {
        let (tx, rx) = mpsc::channel(4);
        let data = self.scripted.lock().unwrap().pop();
        if let Some(data) = data { let _ = tx.send(data).await; }
        Ok(rx)
    }
    async fn read_rssi(&self, _conn: &Connection) -> Result<i16> {
        let rssi = self.scripted_rssi.lock().unwrap().pop();
        Ok(rssi.unwrap_or(-50))
    }
}
