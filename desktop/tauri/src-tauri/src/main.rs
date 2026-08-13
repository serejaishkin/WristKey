#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{State, Manager, RunEvent};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::Utc;
use tracing::{info, warn};

use wristkey_core::{SessionManager, Config, Response, Storage, SessionState, PlatformSecurity};
use wristkey_ble::{BtleplugAdapter, BleAdapter, PeripheralInfo};

#[cfg(windows)]
use wristkey_platform_win::WindowsSecurity;
#[cfg(target_os = "linux")]
use wristkey_platform_linux::LinuxSecurity;
#[cfg(target_os = "macos")]
use wristkey_platform_macos::MacOSSecurity;

struct AppState {
    session: Arc<SessionManager>,
    storage: Arc<dyn Storage>,
    platform: Arc<dyn PlatformSecurity>,
    daemon_enabled: AtomicBool,
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
    daemon_enabled: bool,
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

#[derive(Serialize, Deserialize, Clone)]
struct CalibrateResult {
    avg: i16,
    threshold: i16,
    samples: u32,
}

// --- Commands ---

#[tauri::command]
async fn get_status(state: State<'_, Arc<Mutex<AppState>>>) -> Result<StatusDto, String> {
    let s = state.lock().await;
    let devices = s.session.list_devices().await.map_err(|e| e.to_string())?;
    let session_state = s.session.state().await;
    let daemon_enabled = s.daemon_enabled.load(Ordering::Relaxed);
    let (state_str, detail) = match session_state {
        SessionState::Disconnected => ("disconnected".into(), "No watch connected".into()),
        SessionState::Pairing { .. } => ("pairing".into(), "Waiting for watch confirmation…".into()),
        SessionState::Verifying { .. } => ("verifying".into(), "Checking signature…".into()),
        SessionState::Authenticated { device_id, last_rssi, .. } => {
            ("authenticated".into(), format!("Device: {} • RSSI: {} dBm", device_id, last_rssi))
        }
        SessionState::Locked => ("locked".into(), "Screen locked".into()),
    };
    Ok(StatusDto { state: state_str, detail, device_count: devices.len(), daemon_enabled })
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
        None,
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
async fn calibrate_proximity(id: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<CalibrateResult, String> {
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
    info!("Calibration started — hold watch near PC for 10s");

    let mut samples = Vec::new();
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(10) {
        ticker.tick().await;
        match adapter.read_rssi(&conn).await {
            Ok(rssi) => { samples.push(rssi); info!("RSSI sample: {} dBm", rssi); }
            Err(e) => warn!("RSSI error: {}", e),
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
    info!("Calibration complete: avg={} dBm, threshold={} dBm", avg, threshold);

    let mut updated = device;
    updated.baseline_rssi = threshold;
    s.storage.save_device(&updated).await.map_err(|e| e.to_string())?;

    let _ = adapter.disconnect(&conn).await;
    Ok(CalibrateResult { avg, threshold, samples: samples.len() as u32 })
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
async fn lock_screen(state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    let s = state.lock().await;
    s.platform.lock_screen().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn unlock_screen(state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    let s = state.lock().await;
    s.platform.unlock_screen().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn toggle_daemon(enabled: bool, state: State<'_, Arc<Mutex<AppState>>>) -> Result<bool, String> {
    let s = state.lock().await;
    s.daemon_enabled.store(enabled, Ordering::Relaxed);
    info!("Daemon auto-lock {}", if enabled { "enabled" } else { "disabled" });
    Ok(enabled)
}

// --- Platform adapter ---

fn create_platform_adapter() -> Arc<dyn PlatformSecurity> {
    #[cfg(windows)]
    { Arc::new(WindowsSecurity::new()) }
    #[cfg(target_os = "linux")]
    { Arc::new(LinuxSecurity::new()) }
    #[cfg(target_os = "macos")]
    { Arc::new(MacOSSecurity::new()) }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    { compile_error!("unsupported platform") }
}

// --- Main ---

fn main() {
    // Logs next to the .exe
    let log_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("logs")))
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("wristkey.log");
    println!("[WristKey] Logs: {:?}", log_path);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "wristkey");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("WristKey Tauri v2 starting");

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    let (session, storage, platform) = rt.block_on(async {
        let data_dir = directories::ProjectDirs::from("", "", "WristKey")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("data"));
        let _ = std::fs::create_dir_all(&data_dir);

        let storage: Arc<dyn Storage> = match wristkey_core::SqliteStorage::open_default() {
            Ok(s) => Arc::new(s),
            Err(e) => {
                warn!("Failed to open sqlite DB ({}), using memory storage", e);
                Arc::new(wristkey_core::MemoryStorage::new())
            }
        };

        let crypto = Arc::new(wristkey_core::EcdsaP256Crypto);
        let session = Arc::new(SessionManager::new(crypto, storage.clone()));
        let platform = create_platform_adapter();

        if let Err(e) = platform.register_as_authenticator().await {
            warn!("Failed to register as authenticator: {}", e);
        }

        (session, storage, platform)
    });

    let app_state = Arc::new(Mutex::new(AppState {
        session: session.clone(),
        storage: storage.clone(),
        platform: platform.clone(),
        daemon_enabled: AtomicBool::new(false),
    }));

    // Start daemon loop in background
    let daemon_state = app_state.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");

        rt.block_on(async {
            loop {
                let enabled = {
                    let s = daemon_state.lock().await;
                    s.daemon_enabled.load(Ordering::Relaxed)
                };

                if !enabled {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }

                let (session, platform, has_devices) = {
                    let s = daemon_state.lock().await;
                    let devices = s.session.list_devices().await.unwrap_or_default();
                    (s.session.clone(), s.platform.clone(), !devices.is_empty())
                };

                if !has_devices {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }

                let ble = match BtleplugAdapter::new().await {
                    Ok(a) => Arc::new(a) as Arc<dyn BleAdapter>,
                    Err(e) => {
                        warn!("BLE adapter unavailable: {}. Retrying in 5s…", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let conn_mgr = Arc::new(wristkey_daemon::conn_mgr::ConnectionManager::new());
                let daemon = wristkey_daemon::Daemon::new(session, ble, platform, conn_mgr);

                info!("Daemon loop started");
                if let Err(e) = daemon.run().await {
                    warn!("Daemon crashed: {}. Restarting in 5s…", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        });
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let hide_i = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
            let lock_i = MenuItem::with_id(app, "lock_now", "🔒 Lock Now", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

            let menu = Menu::with_items(app, &[
                &show_i,
                &hide_i,
                &PredefinedMenuItem::separator(app)?,
                &lock_i,
                &PredefinedMenuItem::separator(app)?,
                &quit_i,
            ])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "hide" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    "lock_now" => {
                        let platform = create_platform_adapter();
                        let _ = std::thread::spawn(move || {
                            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                            rt.block_on(async { let _ = platform.lock_screen().await; });
                        }).join();
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
                .build(app)?;

            Ok(())
        })
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_status, get_paired_devices, scan_devices,
            pair_device, forget_device, calibrate_proximity,
            get_config, set_config, lock_screen, unlock_screen,
            toggle_daemon
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
            RunEvent::ExitRequested { api, .. } => {
                api.prevent_exit();
            }
            _ => {}
        });
}
