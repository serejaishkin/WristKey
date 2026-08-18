use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Manager, State, RunEvent};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tracing::{info, error};

use wristkey_core::{Config, SessionManager, EcdsaP256Crypto, MemoryStorage, PlatformSecurity, Response};
use wristkey_daemon::{Daemon, ConnectionManager};
use wristkey_ble::{BleAdapter, BtleplugAdapter, MockBleAdapter, PeripheralInfo};

#[cfg(target_os = "windows")]
use wristkey_platform_win::WindowsSecurity;
#[cfg(target_os = "linux")]
use wristkey_platform_linux::LinuxSecurity;
#[cfg(target_os = "macos")]
use wristkey_platform_macos::MacosSecurity;

static LOG_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
const PUBLIC_KEY_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567893";

fn get_pc_name() -> String {
    #[cfg(target_os = "windows")]
    { std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Unknown PC".to_string()) }
    #[cfg(not(target_os = "windows"))]
    { std::env::var("HOSTNAME").unwrap_or_else(|_| "Unknown PC".to_string()) }
}

#[derive(serde::Serialize)]
struct StatusDto { state: String, detail: String, device_count: usize, daemon_enabled: bool, #[cfg(target_os = "windows")] cp_registered: bool, storage_type: String }
#[derive(serde::Serialize)]
struct DeviceDto { id: String, name: String, address: String, baseline_rssi: i32 }
#[derive(serde::Serialize)]
struct ScanResultDto { id: String, name: String, rssi: i32, address: String }
#[derive(serde::Serialize)]
struct CalibrationResultDto { avg: i32, threshold: i32, samples: usize }
#[derive(serde::Deserialize)]
struct PairRequest { id: String, name: String, rssi: i32, address: String }

struct AppState {
    session: Arc<SessionManager>,
    config: Arc<Mutex<Config>>,
    daemon: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    platform: Arc<dyn PlatformSecurity>,
    ble: Arc<dyn BleAdapter>,
}

fn create_platform_adapter(session: Arc<SessionManager>) -> Arc<dyn PlatformSecurity> {
    #[cfg(target_os = "windows")]
    { let mut win = WindowsSecurity::new(); win.set_session(session.clone()); Arc::new(win) }
    #[cfg(target_os = "linux")]
    { let mut linux = LinuxSecurity::new(); linux.set_session(session.clone()); Arc::new(linux) }
    #[cfg(target_os = "macos")]
    { let mut mac = MacosSecurity::new(); mac.set_session(session.clone()); Arc::new(mac) }
}

#[tauri::command]
async fn get_log_dir() -> Result<String, String> { LOG_DIR.get().map(|p| p.display().to_string()).ok_or_else(|| "log directory not initialized".to_string()) }

#[tauri::command]
async fn get_status(state: State<'_, Arc<AppState>>) -> Result<StatusDto, String> {
    let session_state = state.session.state().await;
    let devices = state.session.list_paired_devices().await.map_err(|e| e.to_string())?;
    let daemon_enabled = state.daemon.lock().await.is_some();
    #[cfg(target_os = "windows")] let cp_registered = WindowsSecurity::is_credential_provider_registered();
    #[cfg(not(target_os = "windows"))] let cp_registered = false;
    #[cfg(target_os = "windows")] let storage_type = WindowsSecurity::storage_type_description().to_string();
    #[cfg(target_os = "linux")] let storage_type = LinuxSecurity::storage_type_description().to_string();
    #[cfg(target_os = "macos")] let storage_type = MacosSecurity::storage_type_description().to_string();
    let device_count = devices.len();
    let state_str = if session_state.is_authenticated() { "authenticated" } else { "disconnected" };
    let detail = if session_state.is_authenticated() { format!("Authenticated with {}", devices.first().map(|d| d.name.clone()).unwrap_or_else(|| "unknown".to_string())) } else if device_count > 0 { format!("{} device(s) paired, not connected", device_count) } else { "No paired devices".to_string() };
    Ok(StatusDto { state: state_str.to_string(), detail, device_count, daemon_enabled, #[cfg(target_os = "windows")] cp_registered, storage_type })
}

#[tauri::command]
async fn get_paired_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<DeviceDto>, String> {
    let devices = state.session.list_paired_devices().await.map_err(|e| e.to_string())?;
    Ok(devices.into_iter().map(|d| DeviceDto { id: d.id.to_string(), name: d.name, address: d.address, baseline_rssi: d.baseline_rssi as i32 }).collect())
}

#[tauri::command]
async fn scan_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<ScanResultDto>, String> {
    let service_uuid = uuid::Uuid::parse_str(SERVICE_UUID).unwrap();
    let mut rx = state.ble.scan(service_uuid).await.map_err(|e| e.to_string())?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut found = Vec::new();
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { break; }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(info)) => found.push(ScanResultDto { id: info.id.clone(), name: info.name.unwrap_or_else(|| "Unknown".to_string()), rssi: info.rssi.unwrap_or(-100) as i32, address: info.id }),
            Ok(None) | Err(_) => break,
        }
    }
    let _ = state.ble.stop_scan().await;
    Ok(found)
}

#[tauri::command]
async fn pair_device(state: State<'_, Arc<AppState>>, req: PairRequest) -> Result<(), String> {
    let service_uuid = uuid::Uuid::parse_str(SERVICE_UUID).unwrap();
    let challenge_char = uuid::Uuid::parse_str(CHALLENGE_CHAR).unwrap();
    let response_char = uuid::Uuid::parse_str(RESPONSE_CHAR).unwrap();
    let public_key_char = uuid::Uuid::parse_str(PUBLIC_KEY_CHAR).unwrap();
    let pc_name_char = uuid::Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567898").unwrap();

    let info = PeripheralInfo { id: req.address.clone(), name: Some(req.name.clone()), pin: None, device_id: Some(req.id.clone()), rssi: Some(req.rssi as i16), service_uuids: vec![service_uuid], raw_manufacturer_data: None };
    info!("Pairing: connecting to {} ({})", req.name, req.address);
    let conn = state.ble.connect(&info).await.map_err(|e| e.to_string())?;
    info!("Pairing: connected");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let pc_name = get_pc_name();
    state.ble.write(&conn, pc_name_char, pc_name.as_bytes()).await.map_err(|e| e.to_string())?;
    info!("Pairing: PC name sent");

    // Subscribe BEFORE sending the challenge. The watch can answer immediately after ALLOW.
    let mut rx = state.ble.notify(&conn, response_char).await.map_err(|e| e.to_string())?;
    info!("Pairing: subscribed to response");

    let challenge = state.session.begin_pairing().await.map_err(|e| e.to_string())?;
    state.ble.write(&conn, challenge_char, &challenge.to_bytes()).await.map_err(|e| e.to_string())?;
    info!("Pairing: challenge written");

    let response_data = match tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv()).await {
        Ok(Some(d)) => d,
        _ => { let _ = state.ble.disconnect(&conn).await; return Err("Pairing response timeout".into()); }
    };
    if response_data.len() < 65 {
        let _ = state.ble.disconnect(&conn).await;
        return Err(format!("Pairing response too short: {} bytes", response_data.len()));
    }

    let signature = response_data[..64].to_vec();
    let user_present = response_data[64] != 0;

    // The public key has its own READ characteristic. Previously this accidentally read CHALLENGE_CHAR.
    let public_key = state.ble.read(&conn, public_key_char).await
        .map_err(|e| format!("Failed to read public key: {}", e))?;

    let response = Response { signature, user_present, timestamp: chrono::Utc::now() };
    state.session.complete_pairing(req.name, public_key, Some(req.id.into_bytes()), &response, req.rssi as i16, req.address.clone()).await.map_err(|e| e.to_string())?;
    info!("Pairing: completed successfully");

    #[cfg(target_os = "windows")]
    {
        use wristkey_platform_win::WindowsVault;
        let vault = WindowsVault::new();
        let device = state.session.list_paired_devices().await.map_err(|e| e.to_string())?.into_iter().find(|d| d.address == req.address).ok_or("Paired device not found")?;
        let pairing_key = vault.ensure_device(&device.id.to_string(), &device.name, &device.address).map_err(|e| e.to_string())?;
        let pairing_key_char = uuid::Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567897").unwrap();
        state.ble.write(&conn, pairing_key_char, &pairing_key).await.map_err(|e| format!("Failed to send pairing key: {}", e))?;
    }

    let _ = state.ble.disconnect(&conn).await;
    Ok(())
}

#[tauri::command]
async fn forget_device(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> { state.session.forget_device(&id).await.map_err(|e| e.to_string()) }
#[tauri::command]
async fn calibrate_device(state: State<'_, Arc<AppState>>, id: String) -> Result<CalibrationResultDto, String> { let (avg, threshold, samples) = state.session.calibrate_device(&id).await.map_err(|e| e.to_string())?; Ok(CalibrationResultDto { avg, threshold, samples }) }
#[tauri::command]
async fn start_daemon(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let mut daemon_guard = state.daemon.lock().await;
    if daemon_guard.is_some() { return Err("Daemon already running".into()); }
    let daemon = Daemon::new(state.session.clone(), state.ble.clone(), state.platform.clone(), Arc::new(ConnectionManager::new()));
    let handle = tokio::spawn(async move { if let Err(e) = daemon.run().await { error!("Daemon error: {}", e); } });
    *daemon_guard = Some(handle); info!("Daemon started"); Ok(())
}
#[tauri::command]
async fn stop_daemon(state: State<'_, Arc<AppState>>) -> Result<(), String> { let mut guard = state.daemon.lock().await; if let Some(handle) = guard.take() { handle.abort(); info!("Daemon stopped"); } Ok(()) }

#[cfg(target_os = "windows")]
#[tauri::command]
async fn set_windows_password(state: State<'_, Arc<AppState>>, password: String) -> Result<(), String> { use wristkey_platform_win::WindowsVault; let vault = WindowsVault::new(); let devices = state.session.list_paired_devices().await.map_err(|e| e.to_string())?; if let Some(device) = devices.first() { vault.set_password(&device.id.to_string(), &password).map_err(|e| e.to_string())?; info!("Windows password stored in vault for device {}", device.id); Ok(()) } else { Err("No paired device found. Pair a watch first.".into()) } }
#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn set_windows_password(_state: State<'_, Arc<AppState>>, _password: String) -> Result<(), String> { Err("Windows password storage is only available on Windows".into()) }

#[tauri::command]
async fn get_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> { Ok(state.config.lock().await.clone()) }

#[tauri::command]
async fn update_config(state: State<'_, Arc<AppState>>, new_config: Config) -> Result<(), String> { *state.config.lock().await = new_config; Ok(()) }

// Backward-compatible alias for older frontend builds which invoke set_config.
#[tauri::command]
async fn set_config(state: State<'_, Arc<AppState>>, config: Config) -> Result<(), String> { update_config(state, config).await }

#[tauri::command]
async fn get_logs() -> Result<Vec<String>, String> { Ok(vec!["Log viewing not yet implemented".to_string()]) }

#[tokio::main]
async fn main() {
    let log_dir = std::env::var("WRISTKEY_LOG_DIR").map(|s| std::path::PathBuf::from(s)).unwrap_or_else(|_| { let mut path = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from(".")); path.push("WristKey/logs"); path });
    std::fs::create_dir_all(&log_dir).ok(); LOG_DIR.set(log_dir.clone()).ok();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "wristkey.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt().with_writer(non_blocking).with_ansi(false).with_level(true).with_target(true).init();
    info!("WristKey starting up...");

    let config = Config::from_file(&dirs::config_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join("WristKey/config.toml")).unwrap_or_default();
    let storage: Arc<dyn wristkey_core::Storage> = Arc::new(MemoryStorage::new());
    let crypto = Arc::new(EcdsaP256Crypto);
    let session = Arc::new(SessionManager::new(crypto, storage));
    let platform = create_platform_adapter(session.clone());
    let ble: Arc<dyn BleAdapter> = match BtleplugAdapter::new().await { Ok(adapter) => Arc::new(adapter), Err(_) => Arc::new(MockBleAdapter::new()) };
    let app_state = Arc::new(AppState { session: session.clone(), config: Arc::new(Mutex::new(config)), daemon: Arc::new(Mutex::new(None)), platform, ble });

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![get_status, get_paired_devices, scan_devices, pair_device, forget_device, calibrate_device, start_daemon, stop_daemon, set_windows_password, get_config, update_config, set_config, get_logs, get_log_dir])
        .setup(|app| {
            let app_state: tauri::State<Arc<AppState>> = app.state();
            let session = app_state.session.clone(); let ble = app_state.ble.clone(); let platform = app_state.platform.clone();
            tauri::async_runtime::spawn(async move { let conn_mgr = Arc::new(ConnectionManager::new()); let daemon = Daemon::new(session, ble, platform, conn_mgr); if let Err(e) = daemon.run().await { error!("Background daemon error: {}", e); } });
            let handle = app.handle().clone();
            let quit_item = MenuItem::with_id(&handle, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(&handle, &[&PredefinedMenuItem::separator(&handle)?, &quit_item])?;
            let _tray = TrayIconBuilder::new().icon(handle.default_window_icon().unwrap().clone()).menu(&menu).on_menu_event(|app, event| { if event.id().as_ref() == "quit" { app.exit(0); } }).on_tray_icon_event(|tray, event| { if let TrayIconEvent::DoubleClick { .. } = event { let app = tray.app_handle(); if let Some(window) = app.get_webview_window("main") { let _ = window.show(); let _ = window.set_focus(); } } }).build(&handle)?;
            Ok(())
        })
        .on_window_event(|window, event| { if let tauri::WindowEvent::CloseRequested { api, .. } = event { window.hide().ok(); api.prevent_close(); } })
        .build(tauri::generate_context!()).expect("error while running tauri application")
        .run(|_app_handle, event| { if let RunEvent::ExitRequested { api, .. } = event { api.prevent_exit(); } });
}
