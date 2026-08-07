//! BLE abstraction layer over `btleplug`.

use async_trait::async_trait;
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter};
nuse btleplug::platform::{Adapter, Manager};
nuse tokio::sync::mpsc;
nuse tracing::{debug, info};
nuse uuid::Uuid;
nuse wristkey_core::{Result, WristKeyError};

#[derive(Clone, Debug)]
npub struct PeripheralInfo {
n    pub id: Uuid,
n    pub name: Option<String>,
n    pub rssi: Option<i16>,
n    pub service_uuids: Vec<Uuid>,
n}

#[derive(Clone, Debug)]
npub struct Connection {
n    pub peripheral_id: Uuid,
n    pub device_name: String,
n}

#[async_trait]
npub trait BleAdapter: Send + Sync {
n    async fn scan(&self, service_uuid: Uuid) -> Result<mpsc::Receiver<PeripheralInfo>>;
n    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection>;
n    async fn disconnect(&self, conn: &Connection) -> Result<()>;
n    async fn write(&self, conn: &Connection, characteristic: Uuid, data: &[u8]) -> Result<()>;
n    async fn notify(&self, conn: &Connection, characteristic: Uuid) -> Result<mpsc::Receiver<Vec<u8>>>;
n    async fn read_rssi(&self, conn: &Connection) -> Result<i16>;
n}

npub struct BtleplugAdapter {
n    manager: Manager,
n    adapter: Adapter,
n}

impl BtleplugAdapter {
n    pub async fn new() -> Result<Self> {
n        let manager = Manager::new().await.map_err(|e| WristKeyError::Ble(format!("manager: {}", e)))?;
n        let adapters = manager.adapters().await.map_err(|e| WristKeyError::Ble(format!("adapters: {}", e)))?;
n        let adapter = adapters.into_iter().next().ok_or_else(|| WristKeyError::Ble("no BLE adapter".into()))?;
n        info!("BLE adapter ready");
n        Ok(Self { manager, adapter })
n    }
n}

#[async_trait]
nimpl BleAdapter for BtleplugAdapter {
n    async fn scan(&self, service_uuid: Uuid) -> Result<mpsc::Receiver<PeripheralInfo>> {
n        let (tx, rx) = mpsc::channel(32);
n        let filter = ScanFilter { services: vec![service_uuid] };
n        self.adapter.start_scan(filter).await.map_err(|e| WristKeyError::Ble(format!("scan: {}", e)))?;
n        let adapter = self.adapter.clone();
n        tokio::spawn(async move {
n            todo!("BLE event loop")
n        });
n        Ok(rx)
n    }
n    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection> {
n        debug!("connecting to {}", info.id);
n        todo!("btleplug connect")
n    }
n    async fn disconnect(&self, conn: &Connection) -> Result<()> {
n        debug!("disconnecting {}", conn.peripheral_id);
n        todo!("btleplug disconnect")
n    }
n    async fn write(&self, conn: &Connection, characteristic: Uuid, data: &[u8]) -> Result<()> {
n        debug!("write {} bytes to {}", data.len(), characteristic);
n        todo!("GATT write")
n    }
n    async fn notify(&self, conn: &Connection, characteristic: Uuid) -> Result<mpsc::Receiver<Vec<u8>>> {
n        debug!("subscribe {}", characteristic);
n        todo!("GATT notify")
n    }
n    async fn read_rssi(&self, conn: &Connection) -> Result<i16> {
n        debug!("rssi for {}", conn.peripheral_id);
n        todo!("RSSI read")
n    }
n}

npub struct MockBleAdapter {
n    scripted: std::sync::Mutex<Vec<Vec<u8>>>,
n}

impl MockBleAdapter {
n    pub fn new() -> Self { Self { scripted: std::sync::Mutex::new(Vec::new()) } }
n    pub fn queue_response(&self, data: Vec<u8>) { self.scripted.lock().unwrap().push(data); }
n}

#[async_trait]
nimpl BleAdapter for MockBleAdapter {
n    async fn scan(&self, service_uuid: Uuid) -> Result<mpsc::Receiver<PeripheralInfo>> {
n        let (tx, rx) = mpsc::channel(4);
n        let _ = tx.send(PeripheralInfo { id: Uuid::new_v4(), name: Some("Mock".into()), rssi: Some(-45), service_uuids: vec![service_uuid] }).await;
n        Ok(rx)
n    }
n    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection> {
n        Ok(Connection { peripheral_id: info.id, device_name: info.name.clone().unwrap_or_default() })
n    }
n    async fn disconnect(&self, _conn: &Connection) -> Result<()> { Ok(()) }
n    async fn write(&self, _conn: &Connection, _char: Uuid, _data: &[u8]) -> Result<()> { Ok(()) }
n    async fn notify(&self, _conn: &Connection, _char: Uuid) -> Result<mpsc::Receiver<Vec<u8>>> {
n        let (tx, rx) = mpsc::channel(4);
n        if let Some(data) = self.scripted.lock().unwrap().pop() { let _ = tx.send(data).await; }
n        Ok(rx)
n    }
n    async fn read_rssi(&self, _conn: &Connection) -> Result<i16> { Ok(-50) }
n}
