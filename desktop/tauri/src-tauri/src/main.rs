use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::{Manager, SystemTray, SystemTrayEvent, SystemTrayMenu, SystemTrayMenuItem, CustomMenuItem};
use tracing::{info, warn, error};

use wristkey_core::{Config, Result, SessionManager, WristKeyError, EcdsaP256Crypto, SqliteStorage};
use wristkey_daemon::{Daemon, ConnectionManager};
use wristkey_ble::BtleplugAdapter;

#[cfg(target_os = "windows")]
use wristkey_platform_win::WindowsSecurity;

#[cfg(target_os = "linux")]
use wristkey_platform_linux::LinuxSecurity;

#[cfg(target_os = "windows")]
use wristkey_core::PasswordVault;

// ─── DTOs ───────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct StatusDto {
    state: String,
    detail: String,
    device_count: usize,
    daemon_enabled: bool,
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

// ─── State ────────────────────────────────────────────────────────────────────

struct AppState {
    session: Arc<SessionManager>,
    config: Arc<Mutex<Config>>,
    daemon: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    platform: Arc<dyn wristkey_core::PlatformSecurity + Send + Sync>,
}

fn create_platform_adapter(session: Arc<SessionManager>) -> Arc<dyn wristkey_core::PlatformSecurity + Send + Sync> {
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

// ─── Commands ─────────────────────────────────────────────────────────────────

#[tauri::command]
async fn get_status(state: tauri::State<'_, Arc<AppState>>) -> Result<StatusDto> {
    let session_state = state.session.state().await;
    let devices = state.session.list_paired_devices().await.unwrap_or_default();
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
        cp_registered,
        storage_type,
    })
}

#[tauri::command]
async fn get_paired_devices(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<DeviceDto>> {
    let devices = state.session.list_paired_devices().await?;
    Ok(devices.into_iter().map(|d| DeviceDto {
        id: d.id.to_string(),
        name: d.name,
        address: d.address,
        baseline_rssi: d.baseline_rssi as i32,
    }).collect())
}

#[tauri::command]
async fn scan_devices(state: tauri::State<'_, Arc<AppState>>) -> Result<Vec<ScanResultDto>> {
    let found = state.session.scan_ble().await?;
    Ok(found.into_iter().map(|(id, name, rssi, address)| ScanResultDto {
        id,
        name,
        rssi,
        address,
    }).collect())
}

#[tauri::command]
async fn pair_device(state: tauri::State<'_, Arc<AppState>>, req: PairRequest) -> Result<()> {
    state.session.pair_device(&req.id, &req.name, req.rssi, &req.address).await?;
    Ok(())
}

#[tauri::command]
async fn forget_device(state: tauri::State<'_, Arc<AppState>>, id: String) -> Result<()> {
    state.session.forget_device(&id).await?;
    Ok(())
}

#[tauri::command]
async fn calibrate_proximity(state: tauri::State<'_, Arc<AppState>>, id: String) -> Result<CalibrationResultDto> {
    let (avg, threshold, samples) = state.session.calibrate_device(&id).await?;
    Ok(CalibrationResultDto {
        avg,
        threshold,
        samples,
    })
}

#[tauri::command]
async fn get_config(state: tauri::State<'_, Arc<AppState>>) -> Result<Config> {
    let cfg = state.config.lock().await.clone();
    Ok(cfg)
}

#[tauri::command]
async fn set_config(state: tauri::State<'_, Arc<AppState>>, config: Config) -> Result<()> {
    let mut cfg = state.config.lock().await;
    *cfg = config;
    let config_path = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("WristKey")
        .join("config.toml");
    if let Ok(toml_str) = toml::to_string_pretty(&*cfg) {
        let _ = std::fs::create_dir_all(config_path.parent().unwrap());
        let _ = std::fs::write(&config_path, toml_str);
    }
    Ok(())
}

#[tauri::command]
async fn toggle_daemon(state: tauri::State<'_, Arc<AppState>>, enabled: bool) -> Result<()> {
    let mut daemon_guard = state.daemon.lock().await;
    if enabled && daemon_guard.is_none() {
        let session = Arc::clone(&state.session);
        let platform = Arc::clone(&state.platform);
        let ble = Arc::new(BtleplugAdapter::new().await.map_err(|e| WristKeyError::Ble(e.to_string()))?);
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
async fn lock_screen(state: tauri::State<'_, Arc<AppState>>) -> Result<()> {
    state.platform.lock_screen().await
}

// ─── Windows-specific commands ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
#[tauri::command]
async fn set_windows_password(state: tauri::State<'_, Arc<AppState>>, password: String) -> Result<()> {
    let win_sec = WindowsSecurity::new();
    let encrypted = win_sec.encrypt_password(&password).await?;

    let devices = state.session.list_paired_devices().await?;
    if let Some(device) = devices.first() {
        state.session.set_device_password(device.id, encrypted).await?;
        info!("Windows password encrypted and stored for device {}", device.id);
    } else {
        return Err(WristKeyError::Platform("No paired device found. Pair a watch first.".into()));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn register_credential_provider() -> Result<()> {
    let dll_path = WindowsSecurity::ensure_dll_extracted().await?;
    WindowsSecurity::register_credential_provider(&dll_path.to_string_lossy())
}

#[cfg(target_os = "windows")]
#[tauri::command]
async fn unregister_credential_provider() -> Result<()> {
    WindowsSecurity::unregister_credential_provider()
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn set_windows_password(_password: String) -> Result<()> {
    Err(WristKeyError::Platform("Windows password only available on Windows".into()))
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn register_credential_provider() -> Result<()> {
    Err(WristKeyError::Platform("Credential Provider only available on Windows".into()))
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
async fn unregister_credential_provider() -> Result<()> {
    Err(WristKeyError::Platform("Credential Provider only available on Windows".into()))
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let quit = CustomMenuItem::new("quit".to_string(), "Quit");
    let show = CustomMenuItem::new("show".to_string(), "Show");
    let tray_menu = SystemTrayMenu::new()
        .add_item(show)
        .add_native_item(SystemTrayMenuItem::Separator)
        .add_item(quit);

    let tray = SystemTray::new().with_menu(tray_menu);

    tauri::Builder::default()
        .system_tray(tray)
        .on_system_tray_event(|app, event| match event {
            SystemTrayEvent::LeftClick { .. } => {
                let window = app.get_window("main").unwrap();
                window.show().unwrap();
                window.set_focus().unwrap();
            }
            SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                "quit" => {
                    std::process::exit(0);
                }
                "show" => {
                    let window = app.get_window("main").unwrap();
                    window.show().unwrap();
                    window.set_focus().unwrap();
                }
                _ => {}
            },
            _ => {}
        })
        .setup(|app| {
            let config_path = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("WristKey")
                .join("config.toml");

            let config: Config = if config_path.exists() {
                let content = std::fs::read_to_string(&config_path).unwrap_or_default();
                toml::from_str(&content).unwrap_or_default()
            } else {
                Config::default()
            };

            let storage = Arc::new(SqliteStorage::open_default().unwrap_or_else(|_| {
                wristkey_core::MemoryStorage::new()
            }));

            let crypto = Arc::new(EcdsaP256Crypto);
            let session = Arc::new(SessionManager::new(crypto, storage));
            let platform = create_platform_adapter(session.clone());
            let state = Arc::new(AppState {
                session: session.clone(),
                config: Arc::new(Mutex::new(config)),
                daemon: Arc::new(Mutex::new(None)),
                platform: platform.clone(),
            });

            app.manage(state);
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
            set_windows_password,
            register_credential_provider,
            unregister_credential_provider,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
