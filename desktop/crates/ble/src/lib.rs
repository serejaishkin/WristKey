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
    pub pin: Option<String>,
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
        let (tx, rx) = mpsc::channel(32);
        let filter = ScanFilter::default();

        self.adapter.start_scan(filter).await
            .map_err(|e| WristKeyError::Ble(format!("scan: {}", e)))?;

        let adapter = self.adapter.clone();
        tokio::spawn(async move {
            let mut events = match adapter.events().await {
                Ok(e) => e,
                Err(e) => {
                    error!("failed to get BLE events: {}", e);
                    return;
                }
            };

            while let Some(event) = events.next().await {
                if let CentralEvent::DeviceDiscovered(id) = event {
                    match adapter.peripheral(&id).await {
                        Ok(peripheral) => {
                            match peripheral.properties().await {
                                Ok(Some(props)) => {
                                    info!("BLE discovered: {} name={:?} rssi={:?} svcs={:?}", peripheral.address(), props.local_name, props.rssi, props.services);
                                info!("  → ALL devices: addr={} name={:?} rssi={:?} manuf={:?}", peripheral.address(), props.local_name, props.rssi, props.manufacturer_data);
                                    if !props.services.contains(&service_uuid) {
                                        debug!("  → skipping, no matching service UUID");
                                        continue;
                                    }
                                    let info = PeripheralInfo {
                                    pin: None,
                                        id: peripheral.address().to_string(),
                                        name: props.local_name.clone(),
                                        rssi: props.rssi,
                                        service_uuids: props.services.clone(),
                                    };
                                    if tx.send(info).await.is_err() {
                                        break;
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => warn!("failed to get properties: {}", e),
                            }
                        }
                        Err(e) => warn!("failed to get peripheral: {}", e),
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection> {
        debug!("connecting to {}", info.id);

        let mut peripheral = {
            let peripherals = self.adapter.peripherals().await
                .map_err(|e| WristKeyError::Ble(format!("peripherals: {}", e)))?;
            peripherals.into_iter()
                .find(|p| p.address().to_string() == info.id)
        };

        if peripheral.is_none() {
            info!("peripheral {} not in cache, starting discovery scan", info.id);
            let filter = ScanFilter::default();
            self.adapter.start_scan(filter).await
                .map_err(|e| WristKeyError::Ble(format!("scan: {}", e)))?;
            for _ in 0..50 {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                let peripherals = self.adapter.peripherals().await
                    .map_err(|e| WristKeyError::Ble(format!("peripherals: {}", e)))?;
                if let Some(p) = peripherals.into_iter().find(|p| p.address().to_string() == info.id) {
                    peripheral = Some(p);
                    break;
                }
            }
            let _ = self.adapter.stop_scan().await;
        }

        let peripheral = peripheral
            .ok_or_else(|| WristKeyError::Ble(format!("peripheral {} not found after scan", info.id)))?;

        peripheral.connect().await
            .map_err(|e| WristKeyError::Ble(format!("connect: {}", e)))?;

        peripheral.discover_services().await
            .map_err(|e| WristKeyError::Ble(format!("discover_services: {}", e)))?;

        self.connected.write().await.insert(info.id.clone(), peripheral);

        info!("connected to {} ({})", info.name.as_deref().unwrap_or("unknown"), info.id);
        Ok(Connection {
            peripheral_id: info.id.clone(),
            device_name: info.name.clone().unwrap_or_else(|| "Unknown".into()),
        })
    }

    async fn disconnect(&self, conn: &Connection) -> Result<()> {
        debug!("disconnecting {}", conn.peripheral_id);

        if let Some(peripheral) = self.connected.write().await.remove(&conn.peripheral_id) {
            peripheral.disconnect().await
                .map_err(|e| WristKeyError::Ble(format!("disconnect: {}", e)))?;
            info!("disconnected {}", conn.peripheral_id);
        }

        Ok(())
    }

    async fn write(&self, conn: &Connection, characteristic: Uuid, data: &[u8]) -> Result<()> {
        debug!("write {} bytes to {}", data.len(), characteristic);

        let peripheral = self.get_connected(&conn.peripheral_id).await?;
        let characteristics = peripheral.characteristics();
        let char = characteristics.iter()
            .find(|c| c.uuid == characteristic)
            .ok_or_else(|| WristKeyError::Ble(format!("characteristic {} not found", characteristic)))?;

        peripheral.write(char, data, WriteType::WithResponse).await
            .map_err(|e| WristKeyError::Ble(format!("write: {}", e)))?;

        Ok(())
    }

    async fn notify(&self, conn: &Connection, characteristic: Uuid) -> Result<mpsc::Receiver<Vec<u8>>> {
        debug!("subscribe {}", characteristic);

        let peripheral = self.get_connected(&conn.peripheral_id).await?;
        let characteristics = peripheral.characteristics();
        let char = characteristics.iter()
            .find(|c| c.uuid == characteristic)
            .ok_or_else(|| WristKeyError::Ble(format!("characteristic {} not found", characteristic)))?;

        peripheral.subscribe(char).await
            .map_err(|e| WristKeyError::Ble(format!("subscribe: {}", e)))?;

        let peripheral_clone = peripheral.clone();
        let mut notifications = peripheral_clone.notifications().await
            .map_err(|e| WristKeyError::Ble(format!("notifications: {}", e)))?;

        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            while let Some(notification) = notifications.next().await {
                if notification.uuid == characteristic && tx.send(notification.value).await.is_err() {
                    break;
                }
            }
        });

        Ok(rx)
    }

    async fn read_rssi(&self, conn: &Connection) -> Result<i16> {
        debug!("rssi for {}", conn.peripheral_id);

        let peripheral = self.get_connected(&conn.peripheral_id).await?;
        let props = peripheral.properties().await
            .map_err(|e| WristKeyError::Ble(format!("properties: {}", e)))?
            .ok_or_else(|| WristKeyError::Ble("no properties".into()))?;

        Ok(props.rssi.unwrap_or(-100))
    }
}

pub struct MockBleAdapter {
    scripted: std::sync::Mutex<Vec<Vec<u8>>>,
    scripted_rssi: std::sync::Mutex<Vec<i16>>,
}

impl Default for MockBleAdapter {
    fn default() -> Self {
        Self::new()
    }
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
                    pin: None,
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
