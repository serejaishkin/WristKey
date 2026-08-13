#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::Utc;
use wristkey_core::{SessionManager, Config, PairedDevice, Response, Storage};
use wristkey_ble::{BtleplugAdapter, BleAdapter, PeripheralInfo};

struct AppState {
    session: Arc<SessionManager>,
    storage: Arc<dyn Storage>,
}

#[derive(Serialize, Deserialize, Clone)]
struct DeviceDto {
    id: String,
    name: String,
    baseline_rssi: i16,
    address: String,
    paired_at: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct DiscoveredDeviceDto {
    id: String,
    name: String,
    rssi: i16,
}

#[derive(Serialize, Deserialize, Clone)]
struct StatusDto {
    state: String,
    detail: String,
    device_count: usize,
}

#[derive(Serialize, Deserialize, Clone)]
struct ConfigDto {
    auto_lock_timeout_sec: u64,
    rssi_threshold_offset_dbm: i16,
    challenge_timeout_sec: u64,
}

#[derive(Serialize, Deserialize, Clone)]
struct PairRequest {
    id: String,
    name: String,
    rssi: i16,
}

#[tauri::command]
async fn get_status(state: State<'_, Arc<Mutex<AppState>>>) -> Result<StatusDto, String> {
    let s = state.lock().await;
    let devices = s.session.list_devices().await.map_err(|e| e.to_string())?;
    let session_state = s.session.state().await;
    let (state_str, detail) = match session_state {
        wristkey_core::SessionState::Disconnected => ("disconnected".into(), "No watch connected".into()),
        wristkey_core::SessionState::Pairing { .. } => ("pairing".into(), "Waiting for watch confirmation…".into()),
        wristkey_core::SessionState::Verifying { .. } => ("verifying".into(), "Checking signature…".into()),
        wristkey_core::SessionState::Authenticated { device_id, last_rssi, .. } => {
            ("authenticated".into(), format!("Device: {} • RSSI: {} dBm", device_id, last_rssi))
        }
        wristkey_core::SessionState::Locked => ("locked".into(), "Screen locked".into()),
    };
    Ok(StatusDto { state: state_str, detail, device_count: devices.len() })
}

#[tauri::command]
async fn get_paired_devices(state: State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<DeviceDto>, String> {
    let s = state.lock().await;
    let devices = s.session.list_devices().await.map_err(|e| e.to_string())?;
    Ok(devices.into_iter().map(|d| DeviceDto {
        id: d.id.to_string(),
        name: d.name,
        baseline_rssi: d.baseline_rssi,
        address: d.address,
        paired_at: d.paired_at.to_rfc3339(),
    }).collect())
}

#[tauri::command]
async fn scan_devices() -> Result<Vec<DiscoveredDeviceDto>, String> {
    let adapter = BtleplugAdapter::new().await.map_err(|e| e.to_string())?;
    let service_uuid = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        .map_err(|e| e.to_string())?;
    let mut rx = adapter.scan(service_uuid).await.map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(tokio::time::Duration::from_millis(500), rx.recv()).await {
            Ok(Some(info)) => {
                found.push(DiscoveredDeviceDto {
                    id: info.id.clone(),
                    name: info.name.unwrap_or_else(|| "Unknown".into()),
                    rssi: info.rssi.unwrap_or(-50),
                });
            }
            _ => break,
        }
    }
    Ok(found)
}

#[tauri::command]
async fn pair_device(req: PairRequest, state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    let s = state.lock().await;
    let adapter = BtleplugAdapter::new().await.map_err(|e| e.to_string())?;
    let info = PeripheralInfo {
        id: req.id.clone(),
        name: Some(req.name.clone()),
        pin: None,
        device_id: None,
        rssi: Some(req.rssi),
        service_uuids: vec![],
        raw_manufacturer_data: None,
    };
    let conn = adapter.connect(&info).await.map_err(|e| e.to_string())?;
    let challenge = s.session.begin_pairing().await.map_err(|e| e.to_string())?;
    let challenge_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567891").unwrap();
    let response_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567892").unwrap();

    let mut write_ok = false;
    for _ in 1..=3 {
        if adapter.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() {
            write_ok = true; break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    if !write_ok {
        let _ = adapter.disconnect(&conn).await;
        return Err("Failed to write challenge after 3 attempts".into());
    }

    let mut rx = adapter.notify(&conn, response_char).await.map_err(|e| e.to_string())?;
    let response_data = match tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await {
        Ok(Some(d)) => d,
        _ => { let _ = adapter.disconnect(&conn).await; return Err("Timeout waiting for watch response (10s)".into()); }
    };

    if response_data.len() != 130 {
        let _ = adapter.disconnect(&conn).await;
        return Err(format!("Invalid response: {} bytes (expected 130: 64 sig + 1 user_present + 65 pubkey)", response_data.len()));
    }
    let signature = response_data[..64].to_vec();
    let user_present = response_data[64] != 0;
    let public_key = response_data[65..].to_vec();

    let response = Response { signature, user_present, timestamp: Utc::now() };
    s.session.complete_pairing(
        req.name,
        public_key,
        vec![],
        &response,
        req.rssi,
        req.id,
    ).await.map_err(|e| e.to_string())?;

    let _ = adapter.disconnect(&conn).await;
    Ok(())
}

#[tauri::command]
async fn forget_device(id: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    let device_uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let s = state.lock().await;
    s.storage.delete_device(device_uuid).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn calibrate_proximity(id: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<i16, String> {
    let device_uuid = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    let s = state.lock().await;
    let device = s.session.load_device(device_uuid).await.map_err(|e| e.to_string())?
        .ok_or_else(|| "Device not found".to_string())?;

    let adapter = BtleplugAdapter::new().await.map_err(|e| e.to_string())?;
    let info = PeripheralInfo {
        id: device.address.clone(),
        name: Some(device.name.clone()),
        pin: None,
        device_id: None,
        rssi: None,
        service_uuids: vec![],
        raw_manufacturer_data: None,
    };
    let conn = adapter.connect(&info).await.map_err(|e| e.to_string())?;
    let config_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567894").unwrap();

    adapter.write(&conn, config_char, &[0x01]).await.map_err(|e| e.to_string())?;

    let mut samples = Vec::new();
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(10) {
        ticker.tick().await;
        match adapter.read_rssi(&conn).await {
            Ok(rssi) => samples.push(rssi),
            Err(e) => eprintln!("RSSI error: {}", e),
        }
    }

    if samples.is_empty() {
        let _ = adapter.write(&conn, config_char, &[0x03]).await;
        let _ = adapter.disconnect(&conn).await;
        return Err("No RSSI samples collected".into());
    }

    let avg = samples.iter().sum::<i16>() / samples.len() as i16;
    let threshold = avg.saturating_add(5).min(-20).max(-90);
    let rssi_byte = threshold as i8;
    adapter.write(&conn, config_char, &[0x02, rssi_byte as u8]).await.map_err(|e| e.to_string())?;

    let mut updated = device;
    updated.baseline_rssi = threshold;
    s.storage.save_device(&updated).await.map_err(|e| e.to_string())?;

    let _ = adapter.disconnect(&conn).await;
    Ok(threshold)
}

#[tauri::command]
async fn get_config(state: State<'_, Arc<Mutex<AppState>>>) -> Result<ConfigDto, String> {
    let s = state.lock().await;
    let cfg = s.session.load_config().await.map_err(|e| e.to_string())?;
    Ok(ConfigDto {
        auto_lock_timeout_sec: cfg.auto_lock_timeout_sec,
        rssi_threshold_offset_dbm: cfg.rssi_threshold_offset_dbm,
        challenge_timeout_sec: cfg.challenge_timeout_sec,
    })
}

#[tauri::command]
async fn set_config(state: State<'_, Arc<Mutex<AppState>>>, config: ConfigDto) -> Result<(), String> {
    let s = state.lock().await;
    let cfg = Config {
        auto_lock_timeout_sec: config.auto_lock_timeout_sec,
        rssi_threshold_offset_dbm: config.rssi_threshold_offset_dbm,
        challenge_timeout_sec: config.challenge_timeout_sec,
    };
    s.storage.save_config(&cfg).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn lock_screen() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        Command::new("rundll32.exe")
            .args(["user32.dll,LockWorkStation"])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        Command::new("loginctl")
            .args(["lock-session"])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn unlock_screen() -> Result<(), String> {
    Err("Unlock requires Windows Credential Provider integration (not available in Tauri GUI)".into())
}

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    let (session, storage) = rt.block_on(async {
        let storage: Arc<dyn Storage> = match wristkey_core::SledStorage::open_default() {
            Ok(s) => Arc::new(s),
            Err(e) => {
                eprintln!("Warning: Failed to open sled DB ({}), using memory storage", e);
                Arc::new(wristkey_core::MemoryStorage::new())
            }
        };
        let crypto = Arc::new(wristkey_core::EcdsaP256Crypto);
        let session = Arc::new(SessionManager::new(crypto, storage.clone()));
        (session, storage)
    });

    tauri::Builder::default()
        .manage(Arc::new(Mutex::new(AppState { session, storage })))
        .invoke_handler(tauri::generate_handler![
            get_status, get_paired_devices, scan_devices,
            pair_device, forget_device, calibrate_proximity,
            get_config, set_config, lock_screen, unlock_screen
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
