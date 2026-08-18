//! BLE abstraction layer over btleplug.
//!
//! Windows-specific notes:
//! * listen for both DeviceDiscovered and DeviceUpdated; Windows often reports
//!   already-known peripherals through DeviceUpdated only;
//! * keep the adapter alive for the whole daemon lifetime;
//! * never treat "Samsung" or "Galaxy Watch" alone as WristKey. The custom
//!   WristKey service must be advertised or discovered after connection.

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
    pub device_id: Option<String>,
    pub id: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub service_uuids: Vec<Uuid>,
    pub raw_manufacturer_data: Option<Vec<u8>>,
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
    async fn read(&self, conn: &Connection, characteristic: Uuid) -> Result<Vec<u8>>;
    async fn stop_scan(&self) -> Result<()>;
    fn btleplug_adapter(&self) -> Option<Adapter> { None }
}

pub struct BtleplugAdapter {
    _manager: Manager,
    adapter: Adapter,
    connected: Arc<RwLock<HashMap<String, Peripheral>>>,
}

impl BtleplugAdapter {
    pub async fn new() -> Result<Self> {
        let manager = Manager::new().await
            .map_err(|e| WristKeyError::Ble(format!("manager: {}", e)))?;
        let mut adapters = manager.adapters().await
            .map_err(|e| WristKeyError::Ble(format!("adapters: {}", e)))?;
        if adapters.is_empty() {
            return Err(WristKeyError::Ble("no BLE adapter".into()));
        }

        // Prefer an adapter that is explicitly Bluetooth-capable on Windows,
        // but keep a deterministic fallback for machines with one adapter.
        let mut selected = None;
        for candidate in &adapters {
            match candidate.adapter_info().await {
                Ok(name) => {
                    info!("BLE adapter candidate: {}", name);
                    if name.to_lowercase().contains("bluetooth") {
                        selected = Some(candidate.clone());
                        break;
                    }
                }
                Err(e) => warn!("cannot query BLE adapter info: {}", e),
            }
        }
        let adapter = selected.unwrap_or_else(|| adapters.remove(0));
        info!("BLE adapter selected");
        Ok(Self { _manager: manager, adapter, connected: Arc::new(RwLock::new(HashMap::new())) })
    }

    async fn get_connected(&self, peripheral_id: &str) -> Result<Peripheral> {
        self.connected.read().await.get(peripheral_id).cloned()
            .ok_or_else(|| WristKeyError::Ble(format!("not connected: {}", peripheral_id)))
    }

    async fn find_peripheral(&self, id: &str) -> Result<Option<Peripheral>> {
        let peripherals = self.adapter.peripherals().await
            .map_err(|e| WristKeyError::Ble(format!("peripherals: {}", e)))?;
        Ok(peripherals.into_iter().find(|p| p.address().to_string() == id))
    }
}

const SAMSUNG_MANUF_ID: u16 = 0x0075;
const SAMSUNG_SERVICE_UUID: &str = "0000fd50-0000-1000-8000-00805f9b34fb";

fn is_likely_watch(name: &Option<String>, services: &[Uuid]) -> bool {
    if services.iter().any(|u| u.to_string().eq_ignore_ascii_case(SAMSUNG_SERVICE_UUID)) { return true; }
    name.as_ref().map(|n| {
        let n = n.to_lowercase();
        n.contains("watch") || n.contains("galaxy") || n.contains("gear") || n.contains("active") || n.contains("sm-r")
    }).unwrap_or(false)
}

#[async_trait]
impl BleAdapter for BtleplugAdapter {
    async fn scan(&self, service_uuid: Uuid) -> Result<mpsc::Receiver<PeripheralInfo>> {
        let (tx, rx) = mpsc::channel(64);
        let _ = self.adapter.stop_scan().await;
        self.adapter.start_scan(ScanFilter::default()).await
            .map_err(|e| WristKeyError::Ble(format!("scan: {}", e)))?;

        let adapter = self.adapter.clone();
        tokio::spawn(async move {
            let mut events = match adapter.events().await {
                Ok(events) => events,
                Err(e) => { error!("BLE events unavailable: {}", e); return; }
            };
            while let Some(event) = events.next().await {
                let id = match event {
                    // On Windows an already-known Galaxy Watch commonly comes
                    // through DeviceUpdated rather than DeviceDiscovered.
                    CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => id,
                    _ => continue,
                };
                let peripheral = match adapter.peripheral(&id).await {
                    Ok(p) => p,
                    Err(e) => { warn!("BLE peripheral lookup failed: {}", e); continue; }
                };
                let props = match peripheral.properties().await {
                    Ok(Some(p)) => p,
                    Ok(None) => continue,
                    Err(e) => { warn!("BLE properties failed: {}", e); continue; }
                };

                let is_wristkey_service = props.services.contains(&service_uuid);
                let manufacturer_data = props.manufacturer_data.get(&0xFFFF).cloned().unwrap_or_default();
                let is_wristkey_manufacturer = props.manufacturer_data.contains_key(&0xFFFF);
                let is_samsung = props.manufacturer_data.contains_key(&SAMSUNG_MANUF_ID);
                let is_watch = is_likely_watch(&props.local_name, &props.services);

                info!("BLE device: addr={} name={:?} rssi={:?} wristkey_service={} wristkey_manufacturer={} samsung={} watch={}",
                    peripheral.address(), props.local_name, props.rssi, is_wristkey_service,
                    is_wristkey_manufacturer, is_samsung, is_watch);

                // Samsung/watch is a candidate, not proof of WristKey. We still
                // emit it so the higher layer can display/debug it, while the
                // actual connection verifies the WristKey GATT service.
                let pin = if manufacturer_data.len() >= 4 {
                    String::from_utf8(manufacturer_data[..4].to_vec()).ok()
                } else { None };
                let device_id = if manufacturer_data.len() > 4 {
                    Some(hex::encode(&manufacturer_data[manufacturer_data.len() - 4..]))
                } else { None };
                let info = PeripheralInfo {
                    pin,
                    device_id,
                    id: peripheral.address().to_string(),
                    name: props.local_name.clone(),
                    rssi: props.rssi,
                    service_uuids: props.services.clone(),
                    raw_manufacturer_data: if is_wristkey_service || is_wristkey_manufacturer { Some(manufacturer_data) } else { None },
                };
                if tx.send(info).await.is_err() { break; }
            }
        });
        Ok(rx)
    }

    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection> {
        let peripheral = if let Some(p) = self.find_peripheral(&info.id).await? {
            p
        } else {
            let _ = self.adapter.stop_scan().await;
            self.adapter.start_scan(ScanFilter::default()).await
                .map_err(|e| WristKeyError::Ble(format!("scan before connect: {}", e)))?;
            let mut found = None;
            for _ in 0..60 {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                if let Some(p) = self.find_peripheral(&info.id).await? { found = Some(p); break; }
            }
            let _ = self.adapter.stop_scan().await;
            found.ok_or_else(|| WristKeyError::Ble(format!("peripheral {} not found", info.id)))?
        };

        if peripheral.is_connected().await.unwrap_or(false) {
            info!("BLE peripheral already connected: {}", info.id);
        } else {
            peripheral.connect().await.map_err(|e| WristKeyError::Ble(format!("connect: {}", e)))?;
        }

        // Samsung Wear OS can take a moment before its GATT database becomes
        // visible. Retry discovery instead of accepting an empty database.
        let mut services = Vec::new();
        for attempt in 1..=8 {
            if let Err(e) = peripheral.discover_services().await {
                warn!("GATT discovery attempt {} failed: {}", attempt, e);
            }
            services = peripheral.services();
            if !services.is_empty() { break; }
            tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
        }

        let has_wristkey = services.iter().any(|s| s.uuid == info.service_uuids.iter().copied().find(|u| u.to_string().eq_ignore_ascii_case("a1b2c3d4-e5f6-7890-abcd-ef1234567890")).unwrap_or_else(|| Uuid::nil()))
            || services.iter().any(|s| s.uuid.to_string().eq_ignore_ascii_case("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        for svc in &services {
            info!("GATT service: {}", svc.uuid);
            for ch in &svc.characteristics { debug!("  characteristic {} props={:?}", ch.uuid, ch.properties); }
        }
        if !has_wristkey {
            let _ = peripheral.disconnect().await;
            return Err(WristKeyError::Ble("connected device does not expose WristKey GATT service".into()));
        }

        self.connected.write().await.insert(info.id.clone(), peripheral);
        Ok(Connection { peripheral_id: info.id.clone(), device_name: info.name.clone().unwrap_or_else(|| "Unknown".into()) })
    }

    async fn disconnect(&self, conn: &Connection) -> Result<()> {
        if let Some(peripheral) = self.connected.write().await.remove(&conn.peripheral_id) {
            peripheral.disconnect().await.map_err(|e| WristKeyError::Ble(format!("disconnect: {}", e)))?;
        }
        Ok(())
    }

    async fn write(&self, conn: &Connection, characteristic: Uuid, data: &[u8]) -> Result<()> {
        let peripheral = self.get_connected(&conn.peripheral_id).await?;
        let chars = peripheral.characteristics();
        let ch = chars.iter().find(|c| c.uuid == characteristic)
            .ok_or_else(|| WristKeyError::Ble(format!("characteristic {} not found", characteristic)))?;
        peripheral.write(ch, data, WriteType::WithoutResponse).await
            .map_err(|e| WristKeyError::Ble(format!("write: {}", e)))
    }

    async fn notify(&self, conn: &Connection, characteristic: Uuid) -> Result<mpsc::Receiver<Vec<u8>>> {
        let peripheral = self.get_connected(&conn.peripheral_id).await?;
        let chars = peripheral.characteristics();
        let ch = chars.iter().find(|c| c.uuid == characteristic)
            .ok_or_else(|| WristKeyError::Ble(format!("characteristic {} not found", characteristic)))?;
        peripheral.subscribe(ch).await.map_err(|e| WristKeyError::Ble(format!("subscribe: {}", e)))?;
        let mut notifications = peripheral.notifications().await.map_err(|e| WristKeyError::Ble(format!("notifications: {}", e)))?;
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            while let Some(n) = notifications.next().await {
                if n.uuid == characteristic && tx.send(n.value).await.is_err() { break; }
            }
        });
        Ok(rx)
    }

    async fn read_rssi(&self, conn: &Connection) -> Result<i16> {
        let peripheral = self.get_connected(&conn.peripheral_id).await?;
        Ok(peripheral.properties().await.map_err(|e| WristKeyError::Ble(format!("properties: {}", e)))?.and_then(|p| p.rssi).unwrap_or(-100))
    }

    async fn read(&self, conn: &Connection, characteristic: Uuid) -> Result<Vec<u8>> {
        let peripheral = self.get_connected(&conn.peripheral_id).await?;
        let chars = peripheral.characteristics();
        let ch = chars.iter().find(|c| c.uuid == characteristic)
            .ok_or_else(|| WristKeyError::Ble(format!("characteristic {} not found", characteristic)))?;
        peripheral.read(ch).await.map_err(|e| WristKeyError::Ble(format!("read: {}", e)))
    }

    async fn stop_scan(&self) -> Result<()> {
        self.adapter.stop_scan().await.map_err(|e| WristKeyError::Ble(format!("stop_scan: {}", e)))
    }

    fn btleplug_adapter(&self) -> Option<Adapter> { Some(self.adapter.clone()) }
}

pub struct MockBleAdapter {
    scripted: std::sync::Mutex<Vec<Vec<u8>>>,
    scripted_rssi: std::sync::Mutex<Vec<i16>>,
}
impl Default for MockBleAdapter { fn default() -> Self { Self::new() } }
impl MockBleAdapter {
    pub fn new() -> Self { Self { scripted: std::sync::Mutex::new(Vec::new()), scripted_rssi: std::sync::Mutex::new(Vec::new()) } }
    pub fn queue_response(&self, data: Vec<u8>) { self.scripted.lock().unwrap().push(data); }
    pub fn queue_rssi(&self, rssi: i16) { self.scripted_rssi.lock().unwrap().push(rssi); }
}
#[async_trait]
impl BleAdapter for MockBleAdapter {
    async fn scan(&self, service_uuid: Uuid) -> Result<mpsc::Receiver<PeripheralInfo>> {
        let (tx, rx) = mpsc::channel(4);
        let _ = tx.send(PeripheralInfo { id: "AA:BB:CC:DD:EE:FF".into(), pin: None, device_id: None, name: Some("Mock Watch".into()), rssi: Some(-45), service_uuids: vec![service_uuid], raw_manufacturer_data: None }).await;
        Ok(rx)
    }
    async fn connect(&self, info: &PeripheralInfo) -> Result<Connection> { Ok(Connection { peripheral_id: info.id.clone(), device_name: info.name.clone().unwrap_or_default() }) }
    async fn disconnect(&self, _conn: &Connection) -> Result<()> { Ok(()) }
    async fn write(&self, _conn: &Connection, _char: Uuid, _data: &[u8]) -> Result<()> { Ok(()) }
    async fn notify(&self, _conn: &Connection, _char: Uuid) -> Result<mpsc::Receiver<Vec<u8>>> {
        let (tx, rx) = mpsc::channel(4);
        if let Some(data) = self.scripted.lock().unwrap().pop() { let _ = tx.send(data).await; }
        Ok(rx)
    }
    async fn read_rssi(&self, _conn: &Connection) -> Result<i16> { Ok(self.scripted_rssi.lock().unwrap().pop().unwrap_or(-50)) }
    async fn read(&self, _conn: &Connection, _char: Uuid) -> Result<Vec<u8>> { Ok(vec![]) }
    async fn stop_scan(&self) -> Result<()> { Ok(()) }
}
