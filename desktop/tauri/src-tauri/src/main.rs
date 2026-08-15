use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Manager, State, RunEvent};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tracing::{info, warn, error};

use wristkey_core::{Config, SessionManager, EcdsaP256Crypto, SqliteStorage, MemoryStorage};
use wristkey_daemon::{Daemon, ConnectionManager};
use wristkey_ble::BtleplugAdapter;

#[cfg(target_os = "windows")]
use wristkey_platform_win::WindowsSecurity;
#[cfg(target_os = "linux")]
use wristkey_platform_linux::LinuxSecurity;

// Log directory — set once in main() so get_log_dir can expose it to UI
static LOG_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

// ─── DTOs ───────────────────────────────────────────────────────────────────

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

// ─── State ──────────────────────────────────────────────────────────────────

struct AppState {
    session: Arc<SessionManager>,
    config: Arc<Mutex<Config>>,
    daemon: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    platform: Arc<dyn wristkey_core::PlatformSecurity>,
}

fn create_platform_adapter(session: Arc<SessionManager>) -> Arc<dyn wristkey_core::PlatformSecurity> {
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
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        compile_error!("Unsupported platform")
    }
}

// ─── Commands (std::result::Result for Tauri v2 compatibility) ─────────────

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
    #[cfg(not(target_os = "windows"))]
    let storage_type = "N/A".to_string();

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
    let found = state.session.scan_ble().await.map_err(|e| e.to_string())?;
    Ok(found.into_iter().map(|(id, name, rssi, address)| ScanResultDto {
        id, name, rssi, address,
    }).collect())
}

#[tauri::command]
async fn pair_device(state: State<'_, Arc<AppState>>, req: PairRequest) -> Result<(), String> {
    state.session.pair_device(&req.id, &req.name, req.rssi, &req.address).await.map_err(|e| e.to_string())
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
        let ble = Arc::new(BtleplugAdapter::new().await.map_err(|e| e.to_string())?);
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

// ─── Windows-specific commands ──────────────────────────────────────────────

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

// ─── Main ───────────────────────────────────────────────────────────────────

fn main() {
    std::env::set_var("RUST_BACKTRACE", "1");
    println!("[WristKey] main() started");

    // Resolve log directory early
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

    // Bootstrap log line (before tracing init)
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

            // Tray menu
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

            // State initialization
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

            let state = Arc::new(AppState {
                session: session.clone(),
                config: Arc::new(Mutex::new(Config::default())),
                daemon: Arc::new(Mutex::new(None)),
                platform: platform.clone(),
            });
            println!("[WristKey] AppState created");

            // FIX: auto-start daemon so auto-unlock works immediately
            let state_for_daemon = state.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                println!("[WristKey] auto-starting daemon...");
                let mut daemon_guard = state_for_daemon.daemon.lock().await;
                if daemon_guard.is_none() {
                    let session = Arc::clone(&state_for_daemon.session);
                    let platform = Arc::clone(&state_for_daemon.platform);
                    match BtleplugAdapter::new().await {
                        Ok(ble_adapter) => {
                            let ble = Arc::new(ble_adapter);
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
                        Err(e) => {
                            error!("Failed to create BLE adapter for daemon: {}", e);
                            println!("[WristKey] BLE adapter failed: {}", e);
                        }
                    }
                }
            });

            app.manage(state);
            println!("[WristKey] state managed");

            // Tray icon — SAFE: handle missing icon gracefully
            let mut tray_builder = TrayIconBuilder::new()
                .menu(&menu);

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
                println!("[WristKey] tray icon set");
            } else {
                println!("[WristKey] WARNING: no default window icon, tray will have no icon");
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
