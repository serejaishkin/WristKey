use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Manager, State, RunEvent};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tracing::{info, warn, error};

use wristkey_core::{Config, SessionManager, EcdsaP256Crypto, SqliteStorage, MemoryStorage, PlatformSecurity, Response};
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

#[derive(serde::Serialize)]
struct StatusDto {
    state: String,
    detail: String,
    device_count: usize,
    daemon_enabled: bool,
    #[cfg(target_os = "windows")]
    cp_registered: bool,
    storage_type: String,
}

#[derive(serde::Serialize)]
struct DeviceDto {
    id: String,
    name: String,
    address: String,
    baseline_rssi: i32,
}

#[derive(serde::Serialize)]
struct ScanResultDto {
    id: String,
    name: String,
    rssi: i32,
    address: String,
}

#[derive(serde::Serialize)]
struct CalibrationResultDto {
    avg: i32,
    threshold: i32,
    samples: usize,
}

#[derive(serde::Deserialize)]
struct PairRequest {
    id: String,
    name: String,
    rssi: i32,
    address: String,
}

struct AppState {
    session: Arc<SessionManager>,
    config: Arc<Mutex<Config>>,
    daemon: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    platform: Arc<dyn PlatformSecurity>,
    ble: Arc<dyn BleAdapter>,
}

fn create_platform_adapter(session: Arc<SessionManager>) -> Arc<dyn PlatformSecurity> {
    #[cfg(target_os = "windows")]
    {
        let mut win = WindowsSecurity::new();
        win.set_session(session.clone());
        Arc::new(win)
    }
    #[cfg(target_os = "linux")]
    {
        let mut linux = LinuxSecurity::new();
        linux.set_session(session.clone());
        Arc::new(linux)
    }
    #[cfg(target_os = "macos")]
    {
        let mut mac = MacosSecurity::new();
        mac.set_session(session.clone());
        Arc::new(mac)
    }
}

#[tauri::command]
async fn get_log_dir() -> Result<String, String> {
    LOG_DIR.get()
        .map(|p| p.display().to_string())
        .ok_or_else(|| "log directory not initialized".to_string())
}

#[tauri::command]
async fn get_status(state: State<'_, Arc<AppState>>) -> Result<StatusDto, String> {
    let session_state = state.session.state().await;
    let devices = state.session.list_paired_devices().await.map_err(|e| e.to_string())?;
    let daemon_guard = state.daemon.lock().await;
    let daemon_enabled = daemon_guard.is_some();

    #[cfg(target_os = "windows")]
    let cp_registered = WindowsSecurity::is_credential_provider_registered();
    #[cfg(not(target_os = "windows"))]
    let cp_registered = false;

    #[cfg(target_os = "windows")]
    let storage_type = WindowsSecurity::storage_type_description().to_string();
    #[cfg(target_os = "linux")]
    let storage_type = LinuxSecurity::storage_type_description().to_string();
    #[cfg(target_os = "macos")]
    let storage_type = MacosSecurity::storage_type_description().to_string();

    let device_count = devices.len();
    let state_str = if session_state.is_authenticated() {
        "authenticated"
    } else if device_count > 0 {
        "disconnected"
    } else {
        "disconnected"
    };

    let detail = if session_state.is_authenticated() {
        let name = devices.first().map(|d| d.name.clone()).unwrap_or_else(|| "unknown".to_string());
        format!("Authenticated with {}", name)
    } else if device_count > 0 {
        format!("{} device(s) paired, not connected", device_count)
    } else {
        "No paired devices".to_string()
    };

    Ok(StatusDto {
        state: state_str.to_string(),
        detail,
        device_count,
        daemon_enabled,
        #[cfg(target_os = "windows")]
        cp_registered,
        storage_type,
    })
}

#[tauri::command]
async fn get_paired_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<DeviceDto>, String> {
    let devices = state.session.list_paired_devices().await.map_err(|e| e.to_string())?;
    Ok(devices.into_iter().map(|d| DeviceDto {
        id: d.id.to_string(),
        name: d.name,
        address: d.address,
        baseline_rssi: d.baseline_rssi as i32,
    }).collect())
}

#[tauri::command]
async fn scan_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<ScanResultDto>, String> {
    let service_uuid = uuid::Uuid::parse_str(SERVICE_UUID).unwrap();
    let mut rx = state.ble.scan(service_uuid).await.map_err(|e| e.to_string())?;
    let mut found = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { break; }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(info)) => {
                found.push(ScanResultDto {
                    id: info.id.clone(),
                    name: info.name.unwrap_or_else(|| "Unknown".to_string()),
                    rssi: info.rssi.unwrap_or(-100) as i32,
                    address: info.id,
                });
            }
            Ok(None) => break,
            Err(_) => break,
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

    let info = PeripheralInfo {
        id: req.address.clone(),
        name: Some(req.name.clone()),
        pin: None,
        device_id: Some(req.id.clone()),
        rssi: Some(req.rssi as i16),
        service_uuids: vec![service_uuid],
        raw_manufacturer_data: None,
    };

    info!("Pairing: connecting to {} ({})", req.name, req.address);
    let conn = state.ble.connect(&info).await.map_err(|e| e.to_string())?;
    info!("Pairing: connected");

    let challenge = state.session.begin_pairing().await.map_err(|e| e.to_string())?;
    info!("Pairing: challenge generated");

    state.ble.write(&conn, challenge_char, &challenge.to_bytes()).await.map_err(|e| e.to_string())?;
    info!("Pairing: challenge written");

    let mut rx = state.ble.notify(&conn, response_char).await.map_err(|e| e.to_string())?;
    info!("Pairing: subscribed to response");

    let response_data = match tokio::time::timeout(
        std::time::Duration::from_secs(10),
        rx.recv()
    ).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            let _ = state.ble.disconnect(&conn).await;
            return Err("Response channel closed".into());
        }
        Err(_) => {
            let _ = state.ble.disconnect(&conn).await;
            return Err("Timeout waiting for pairing response".into());
        }
    };

    info!("Pairing: received {} bytes response", response_data.len());

    if response_data.len() < 65 {
        let _ = state.ble.disconnect(&conn).await;
        return Err(format!("Invalid response: {} bytes (expected at least 65)", response_data.len()));
    }

    let signature = response_data[..64].to_vec();
    let user_present = response_data[64] != 0;

    let public_key = if response_data.len() > 65 {
        response_data[65..].to_vec()
    } else {
        let _ = state.ble.disconnect(&conn).await;
        return Err("Pairing response missing public key".into());
    };

    let response = Response {
        signature,
        user_present,
        timestamp: chrono::Utc::now(),
    };

    state.session.complete_pairing(
        req.name,
        public_key,
        Some(req.id.into_bytes()),
        &response,
        req.rssi as i16,
        req.address,
    ).await.map_err(|e| e.to_string())?;

    info!("Pairing: completed successfully");
    let _ = state.ble.disconnect(&conn).await;
    Ok(())
}

#[tauri::command]
async fn forget_device(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    state.session.forget_device(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn calibrate_proximity(state: State<'_, Arc<AppState>>, id: String) -> Result<CalibrationResultDto, String> {
    let (avg, threshold, samples) = state.session.calibrate_device(&id).await.map_err(|e| e.to_string())?;
    Ok(CalibrationResultDto { avg, threshold, samples })
}

#[tauri::command]
async fn get_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    let cfg = state.config.lock().await.clone();
    Ok(cfg)
}

#[tauri::command]
async fn set_config(state: State<'_, Arc<AppState>>, config: Config) -> Result<(), String> {
    let mut cfg = state.config.lock().await;
    *cfg = config;
    Ok(())
}

#[tauri::command]
async fn toggle_daemon(state: State<'_, Arc<AppState>>, enabled: bool) -> Result<(), String> {
    let mut daemon_guard = state.daemon.lock().await;
    if enabled && daemon_guard.is_none() {
        let session = Arc::clone(&state.session);
        let platform = Arc::clone(&state.platform);
        let ble = Arc::clone(&state.ble);
        let conn_mgr = Arc::new(ConnectionManager::new());
        let daemon = Arc::new(Daemon::new(session, ble, platform, conn_mgr));
        let handle = tokio::spawn(async move {
            if let Err(e) = daemon.run().await {
                error!("Daemon error: {}", e);
            }
        });
        *daemon_guard = Some(handle);
        info!("Daemon started");
    } else if !enabled && daemon_guard.is_some() {
        if let Some(handle) = daemon_guard.take() {
            handle.abort();
        }
        info!("Daemon stopped");
    }
    Ok(())
}

#[tauri::command]
async fn lock_screen(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.platform.lock_screen().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn unlock_screen(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.platform.unlock_screen().await.map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn set_windows_password(state: State<'_, Arc<AppState>>, password: String) -> Result<(), String> {
    use wristkey_core::PasswordVault;
    let win_sec = WindowsSecurity::new();
    let encrypted = win_sec.encrypt_password(&password).await.map_err(|e| e.to_string())?;

    let devices = state.session.list_paired_devices().await.map_err(|e| e.to_string())?;
    if let Some(device) = devices.first() {
        state.session.set_device_password(device.id, encrypted).await.map_err(|e| e.to_string())?;
        info!("Windows password encrypted and stored for device {}", device.id);
    } else {
        return Err("No paired device found. Pair a watch first.".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn register_credential_provider() -> Result<(), String> {
    let dll_path = WindowsSecurity::ensure_dll_extracted().await.map_err(|e| e.to_string())?;
    WindowsSecurity::register_credential_provider(&dll_path.to_string_lossy()).map_err(|e| e.to_string())
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn unregister_credential_provider() -> Result<(), String> {
    WindowsSecurity::unregister_credential_provider().map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn set_windows_password(_password: String) -> Result<(), String> {
    Err("Windows password only available on Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn register_credential_provider() -> Result<(), String> {
    Err("Credential Provider only available on Windows".into())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn unregister_credential_provider() -> Result<(), String> {
    Err("Credential Provider only available on Windows".into())
}

fn main() {
    std::env::set_var("RUST_BACKTRACE", "1");
    println!("[WristKey] main() started");

    let log_dir = directories::ProjectDirs::from("", "", "WristKey")
        .map(|d| d.data_dir().to_path_buf().join("logs"))
        .or_else(|| {
            std::env::current_exe().ok()
                .and_then(|p| p.parent().map(|d| d.join("logs")))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));
    let _ = std::fs::create_dir_all(&log_dir);
    let _ = LOG_DIR.set(log_dir.clone());

    let today = chrono::Local::now().format("%Y-%m-%d");
    let actual_log_path = log_dir.join(format!("wristkey.{}", today));
    println!("[WristKey] log path: {:?}", actual_log_path);

    {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&actual_log_path) {
            let _ = writeln!(f, "[{}] [WristKey] ---- process starting (pid {}) ----",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), std::process::id());
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();
    info!("WristKey started - logs at {:?}", actual_log_path);
    println!("[WristKey] tracing initialized");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            println!("[WristKey] setup() started");

            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)
                .map_err(|e| { println!("[WristKey] MenuItem quit error: {}", e); e })?;
            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)
                .map_err(|e| { println!("[WristKey] MenuItem show error: {}", e); e })?;
            let lock_i = MenuItem::with_id(app, "lock_now", "Lock Now", true, None::<&str>)
                .map_err(|e| { println!("[WristKey] MenuItem lock error: {}", e); e })?;

            let menu = Menu::with_items(app, &[
                &show_i,
                &PredefinedMenuItem::separator(app).map_err(|e| { println!("[WristKey] separator error: {}", e); e })?,
                &lock_i,
                &PredefinedMenuItem::separator(app).map_err(|e| { println!("[WristKey] separator error: {}", e); e })?,
                &quit_i,
            ]).map_err(|e| { println!("[WristKey] Menu error: {}", e); e })?;
            println!("[WristKey] tray menu created");

            println!("[WristKey] creating storage...");
            let storage: Arc<dyn wristkey_core::Storage> = match SqliteStorage::open_default() {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    println!("[WristKey] SQLite failed ({}), using memory storage", e);
                    Arc::new(MemoryStorage::new())
                }
            };
            println!("[WristKey] storage created");

            println!("[WristKey] creating crypto...");
            let crypto = Arc::new(EcdsaP256Crypto);
            println!("[WristKey] creating session...");
            let session = Arc::new(SessionManager::new(crypto, storage));
            println!("[WristKey] creating platform adapter...");
            let platform = create_platform_adapter(session.clone());
            println!("[WristKey] platform adapter created");

            println!("[WristKey] creating BLE adapter...");
            let ble: Arc<dyn BleAdapter> = match tauri::async_runtime::block_on(BtleplugAdapter::new()) {
                Ok(adapter) => {
                    println!("[WristKey] BLE adapter ready");
                    Arc::new(adapter)
                }
                Err(e) => {
                    println!("[WristKey] BLE adapter failed ({}), using mock", e);
                    Arc::new(MockBleAdapter::new())
                }
            };

            let state = Arc::new(AppState {
                session: session.clone(),
                config: Arc::new(Mutex::new(Config::default())),
                daemon: Arc::new(Mutex::new(None)),
                platform: platform.clone(),
                ble: ble.clone(),
            });
            println!("[WristKey] AppState created");

            // Auto-start daemon
            let state_for_daemon = state.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("daemon auto-start tokio runtime");
                rt.block_on(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    println!("[WristKey] auto-starting daemon...");
                    let mut daemon_guard = state_for_daemon.daemon.lock().await;
                    if daemon_guard.is_none() {
                        let session = Arc::clone(&state_for_daemon.session);
                        let platform = Arc::clone(&state_for_daemon.platform);
                        let ble = Arc::clone(&state_for_daemon.ble);
                        let conn_mgr = Arc::new(ConnectionManager::new());
                        let daemon = Arc::new(Daemon::new(session, ble, platform, conn_mgr));
                        let handle = tokio::spawn(async move {
                            if let Err(e) = daemon.run().await {
                                error!("Daemon error: {}", e);
                            }
                        });
                        *daemon_guard = Some(handle);
                        info!("Daemon auto-started on app launch");
                        println!("[WristKey] daemon auto-started");
                    }
                });
            });

            app.manage(state);
            println!("[WristKey] state managed");

            let mut tray_builder = TrayIconBuilder::new()
                .menu(&menu);

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
                println!("[WristKey] tray icon set");
            } else {
                println!("[WristKey] WARNING: no default window icon");
            }

            let _tray = tray_builder
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "lock_now" => {
                        let state_clone: Arc<AppState> = Arc::clone(&*app.state::<Arc<AppState>>());
                        tokio::spawn(async move {
                            if let Err(e) = state_clone.platform.lock_screen().await {
                                warn!("Tray lock failed: {}", e);
                            }
                        });
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)
                .map_err(|e| { println!("[WristKey] Tray build error: {}", e); e })?;
            println!("[WristKey] tray built successfully");

            println!("[WristKey] setup() complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_paired_devices,
            scan_devices,
            pair_device,
            forget_device,
            calibrate_proximity,
            get_config,
            set_config,
            toggle_daemon,
            lock_screen,
            unlock_screen,
            set_windows_password,
            register_credential_provider,
            unregister_credential_provider,
            get_log_dir,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                window.hide().unwrap();
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| match event {
            RunEvent::ExitRequested { api, code, .. } => {
                if code != Some(0) {
                    api.prevent_exit();
                }
            }
            _ => {}
        });
}
