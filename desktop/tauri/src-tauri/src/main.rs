#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use wristkey_core::{SessionManager, Config, PairedDevice};
use wristkey_ble::{BtleplugAdapter, BleAdapter, PeripheralInfo};

struct AppState {
    session: Arc<SessionManager>,
    ble: Arc<dyn BleAdapter>,
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
async fn scan_devices(state: State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<DeviceDto>, String> {
    let s = state.lock().await;
    let service_uuid = uuid::Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        .map_err(|e| e.to_string())?;
    let mut rx = s.ble.scan(service_uuid).await.map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(tokio::time::Duration::from_millis(500), rx.recv()).await {
            Ok(Some(info)) => {
                found.push(DeviceDto {
                    id: info.id.clone(),
                    name: info.name.unwrap_or_else(|| "Unknown".into()),
                    baseline_rssi: info.rssi.unwrap_or(-50),
                    address: info.id,
                    paired_at: chrono::Utc::now().to_rfc3339(),
                });
            }
            _ => break,
        }
    }
    Ok(found)
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
    // Need storage access — simplified for now
    Ok(())
}

#[tauri::command]
async fn lock_screen() -> Result<(), String> {
    // Platform-specific lock
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

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Mutex::new(AppState {
            session: Arc::new(SessionManager::new(
                Arc::new(wristkey_core::EcdsaP256Crypto),
                Arc::new(wristkey_core::MemoryStorage::new()),
            )),
            ble: Arc::new(BtleplugAdapter::new_blocking().expect("BLE adapter")),
        })))
        .invoke_handler(tauri::generate_handler![
            get_status, get_paired_devices, scan_devices,
            get_config, set_config, lock_screen
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
