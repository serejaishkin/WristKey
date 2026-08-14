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
use tracing::{info, warn, debug, error, trace};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

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
    address: String,
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
    log_to_file: bool,
    log_to_console: bool,
    log_level: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct PairRequest {
    id: String,
    name: String,
    rssi: i16,
    #[serde(default)]
    address: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct CalibrateResult {
    avg: i16,
    threshold: i16,
    samples: u32,
}

// --- Commands ---

#[cfg(target_os = "macos")]
#[tauri::command]
async fn set_macos_password(password: String) -> Result<(), String> {
    info!("set_macos_password called");
    MacOSSecurity::save_password_to_keychain(&password)
        .map_err(|e| { error!("save_password_to_keychain failed: {}", e); e.to_string() })
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn set_macos_password(_password: String) -> Result<(), String> {
    warn!("set_macos_password called on non-macOS platform");
    Err("macOS only".into())
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn delete_macos_password() -> Result<(), String> {
    info!("delete_macos_password called");
    MacOSSecurity::delete_password_from_keychain()
        .map_err(|e| { error!("delete_password_from_keychain failed: {}", e); e.to_string() })
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn delete_macos_password() -> Result<(), String> {
    warn!("delete_macos_password called on non-macOS platform");
    Err("macOS only".into())
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn check_macos_accessibility() -> Result<bool, String> {
    info!("check_macos_accessibility called");
    Ok(MacOSSecurity::check_accessibility_permission())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn check_macos_accessibility() -> Result<bool, String> {
    info!("check_macos_accessibility called on non-macOS platform");
    Ok(false)
}

#[tauri::command]
async fn get_status(state: State<'_, Arc<Mutex<AppState>>>) -> Result<StatusDto, String> {
    debug!("get_status called");
    let s = state.lock().await;
    let devices = s.session.list_devices().await.map_err(|e| { error!("list_devices failed: {}", e); e.to_string() })?;
    let session_state = s.session.state().await;
    let daemon_enabled = s.daemon_enabled.load(Ordering::Relaxed);
    let (state_str, detail) = match session_state {
        SessionState::Disconnected => ("disconnected".into(), "No watch connected".into()),
        SessionState::Pairing { .. } => ("pairing".into(), "Waiting for watch confirmation...".into()),
        SessionState::Verifying { .. } => ("verifying".into(), "Checking signature...".into()),
        SessionState::Authenticated { device_id, last_rssi, .. } => {
            ("authenticated".into(), format!("Device: {} - RSSI: {} dBm", device_id, last_rssi))
        }
        SessionState::Locked => ("locked".into(), "Screen locked".into()),
    };
    info!("get_status: state={}, devices={}, daemon={}", state_str, devices.len(), daemon_enabled);
    Ok(StatusDto { state: state_str, detail, device_count: devices.len(), daemon_enabled })
}

#[tauri::command]
async fn get_paired_devices(state: State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<DeviceDto>, String> {
    debug!("get_paired_devices called");
    let s = state.lock().await;
    let devices = s.session.list_devices().await.map_err(|e| { error!("list_devices failed: {}", e); e.to_string() })?;
    info!("get_paired_devices: returning {} devices", devices.len());
    Ok(devices.into_iter().map(|d| DeviceDto {
        id: d.id.to_string(),
        name: d.name,
        baseline_rssi: d.baseline_rssi,
        address: d.address.clone(),
        paired_at: d.paired_at.to_rfc3339(),
    }).collect())
}

#[tauri::command]
async fn scan_devices() -> Result<Vec<DiscoveredDeviceDto>, String> {
    info!("scan_devices called");
    let adapter = BtleplugAdapter::new().await.map_err(|e| { error!("BtleplugAdapter::new failed: {}", e); e.to_string() })?;
    let service_uuid = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        .map_err(|e| { error!("parse UUID failed: {}", e); e.to_string() })?;
    info!("scan_devices: starting BLE scan for 10s");
    let mut rx = adapter.scan(service_uuid).await.map_err(|e| { error!("adapter.scan failed: {}", e); e.to_string() })?;
    let mut found = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(tokio::time::Duration::from_millis(500), rx.recv()).await {
            Ok(Some(info)) => {
                let display_name = info.name
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| format!("Watch {}", &info.id));
                debug!("scan_devices: found device id={} name={} rssi={:?}", info.id, display_name, info.rssi);
                found.push(DiscoveredDeviceDto {
                    id: info.id.clone(),
                    name: display_name,
                    rssi: info.rssi.unwrap_or(-50),
                    address: info.id.clone(),
                });
            }
            Ok(None) => { trace!("scan_devices: channel closed"); break; }
            Err(_) => { trace!("scan_devices: timeout tick"); }
        }
    }
    info!("scan_devices: scan complete, found {} devices", found.len());
    Ok(found)
}

#[tauri::command]
async fn pair_device(req: PairRequest, state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    info!("pair_device called: id={} name={} rssi={} address={}", req.id, req.name, req.rssi, req.address);
    let s = state.lock().await;
    let adapter = BtleplugAdapter::new().await.map_err(|e| { error!("BtleplugAdapter::new failed: {}", e); e.to_string() })?;

    let device_address = if req.address.is_empty() { req.id.clone() } else { req.address };
    info!("pair_device: using address={}", device_address);

    let info = PeripheralInfo {
        id: req.id.clone(),
        name: Some(req.name.clone()),
        pin: None,
        device_id: None,
        rssi: Some(req.rssi),
        service_uuids: vec![],
        raw_manufacturer_data: None,
    };
    info!("pair_device: connecting to device id={}", req.id);
    let conn = adapter.connect(&info).await.map_err(|e| { error!("adapter.connect failed: {}", e); e.to_string() })?;
    let challenge_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567891").unwrap();
    let response_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567892").unwrap();

    info!("pair_device: beginning pairing session");
    let challenge = s.session.begin_pairing().await.map_err(|e| { error!("begin_pairing failed: {}", e); e.to_string() })?;

    let mut write_ok = false;
    for attempt in 1..=3 {
        info!("pair_device: writing challenge attempt {}/3", attempt);
        if adapter.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() {
            write_ok = true;
            info!("pair_device: challenge written successfully");
            break;
        }
        warn!("pair_device: challenge write attempt {} failed, retrying...", attempt);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    if !write_ok {
        let _ = adapter.disconnect(&conn).await;
        error!("pair_device: failed to write challenge after 3 attempts");
        return Err("Failed to write challenge after 3 attempts".into());
    }

    info!("pair_device: subscribing to response notifications");
    let mut rx = adapter.notify(&conn, response_char).await.map_err(|e| { error!("adapter.notify failed: {}", e); e.to_string() })?;
    info!("pair_device: waiting for watch response (timeout 10s)...");
    let response_data = match tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await {
        Ok(Some(d)) => { info!("pair_device: received response, {} bytes", d.len()); d }
        Ok(None) => {
            let _ = adapter.disconnect(&conn).await;
            error!("pair_device: notification channel closed before response");
            return Err("Notification channel closed".into());
        }
        Err(_) => {
            let _ = adapter.disconnect(&conn).await;
            error!("pair_device: timeout waiting for watch response (10s)");
            return Err("Timeout waiting for watch response (10s)".into());
        }
    };

    if response_data.len() != 130 {
        let _ = adapter.disconnect(&conn).await;
        error!("pair_device: invalid response size: {} bytes (expected 130)", response_data.len());
        return Err(format!("Invalid response: {} bytes (expected 130: 64 sig + 1 user_present + 65 pubkey)", response_data.len()));
    }
    let signature = response_data[..64].to_vec();
    let user_present = response_data[64] != 0;
    let public_key = response_data[65..].to_vec();
    info!("pair_device: response parsed: sig_len={}, user_present={}, pubkey_len={}", signature.len(), user_present, public_key.len());

    let response = Response { signature, user_present, timestamp: Utc::now() };
    info!("pair_device: calling complete_pairing...");
    s.session.complete_pairing(
        req.name,
        public_key,
        None,
        &response,
        req.rssi,
        device_address,
    ).await.map_err(|e| { error!("complete_pairing failed: {}", e); e.to_string() })?;

    info!("pair_device: pairing successful, disconnecting");
    let _ = adapter.disconnect(&conn).await;
    info!("pair_device: done");
    Ok(())
}

#[tauri::command]
async fn forget_device(id: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    info!("forget_device called: id={}", id);
    let device_uuid = Uuid::parse_str(&id).map_err(|e| { error!("parse UUID failed: {}", e); e.to_string() })?;
    let s = state.lock().await;
    s.storage.delete_device(device_uuid).await.map_err(|e| { error!("delete_device failed: {}", e); e.to_string() })?;
    info!("forget_device: device {} deleted", id);
    Ok(())
}

#[tauri::command]
async fn calibrate_proximity(id: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<CalibrateResult, String> {
    info!("calibrate_proximity called: id={}", id);
    let device_uuid = Uuid::parse_str(&id).map_err(|e| { error!("parse UUID failed: {}", e); e.to_string() })?;
    let s = state.lock().await;
    let device = s.session.load_device(device_uuid).await.map_err(|e| { error!("load_device failed: {}", e); e.to_string() })?
        .ok_or_else(|| { error!("calibrate_proximity: device {} not found", id); "Device not found".to_string() })?;

    info!("calibrate_proximity: connecting to device {} at address {}", device.name, device.address);
    let adapter = BtleplugAdapter::new().await.map_err(|e| { error!("BtleplugAdapter::new failed: {}", e); e.to_string() })?;
    let info = PeripheralInfo {
        id: device.address.clone(),
        name: Some(device.name.clone()),
        pin: None,
        device_id: None,
        rssi: None,
        service_uuids: vec![],
        raw_manufacturer_data: None,
    };
    let conn = adapter.connect(&info).await.map_err(|e| { error!("adapter.connect failed: {}", e); e.to_string() })?;
    let config_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567894").unwrap();

    info!("calibrate_proximity: sending calibration start command");
    adapter.write(&conn, config_char, &[0x01]).await.map_err(|e| { error!("write config_char failed: {}", e); e.to_string() })?;
    info!("calibrate_proximity: collecting RSSI samples for 10s...");

    let mut samples = Vec::new();
    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(500));
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(10) {
        ticker.tick().await;
        match adapter.read_rssi(&conn).await {
            Ok(rssi) => { samples.push(rssi); debug!("calibrate: RSSI sample {} dBm", rssi); }
            Err(e) => warn!("calibrate: RSSI read error: {}", e),
        }
    }

    if samples.is_empty() {
        let _ = adapter.write(&conn, config_char, &[0x03]).await;
        let _ = adapter.disconnect(&conn).await;
        error!("calibrate_proximity: no RSSI samples collected");
        return Err("No RSSI samples collected".into());
    }

    samples.sort();
    let median = samples[samples.len() / 2];
    let threshold = median.saturating_add(5).min(-20).max(-90);
    let rssi_byte = threshold as i8;
    info!("calibrate_proximity: median={} dBm, threshold={} dBm, samples={}", median, threshold, samples.len());
    adapter.write(&conn, config_char, &[0x02, rssi_byte as u8]).await.map_err(|e| { error!("write threshold failed: {}", e); e.to_string() })?;
    info!("calibrate_proximity: threshold sent to watch");

    s.session.update_baseline_rssi(device_uuid, threshold).await.map_err(|e| { error!("update_baseline_rssi failed: {}", e); e.to_string() })?;

    let _ = adapter.disconnect(&conn).await;
    info!("calibrate_proximity: done");
    Ok(CalibrateResult { avg: median, threshold, samples: samples.len() as u32 })
}

#[tauri::command]
async fn get_config(state: State<'_, Arc<Mutex<AppState>>>) -> Result<ConfigDto, String> {
    debug!("get_config called");
    let s = state.lock().await;
    let cfg = s.session.load_config().await.map_err(|e| { error!("load_config failed: {}", e); e.to_string() })?;
    info!("get_config: auto_lock={}s rssi_offset={}dBm challenge_timeout={}s log_file={} log_console={} log_level={}",
        cfg.auto_lock_timeout_sec, cfg.rssi_threshold_offset_dbm, cfg.challenge_timeout_sec,
        cfg.log_to_file, cfg.log_to_console, cfg.log_level);
    Ok(ConfigDto {
        auto_lock_timeout_sec: cfg.auto_lock_timeout_sec,
        rssi_threshold_offset_dbm: cfg.rssi_threshold_offset_dbm,
        challenge_timeout_sec: cfg.challenge_timeout_sec,
        log_to_file: cfg.log_to_file,
        log_to_console: cfg.log_to_console,
        log_level: cfg.log_level,
    })
}

#[tauri::command]
async fn set_config(state: State<'_, Arc<Mutex<AppState>>>, config: ConfigDto) -> Result<(), String> {
    info!("set_config called: auto_lock={}s rssi_offset={}dBm challenge_timeout={}s log_file={} log_console={} log_level={}",
        config.auto_lock_timeout_sec, config.rssi_threshold_offset_dbm, config.challenge_timeout_sec,
        config.log_to_file, config.log_to_console, config.log_level);
    let s = state.lock().await;
    let cfg = Config {
        auto_lock_timeout_sec: config.auto_lock_timeout_sec,
        rssi_threshold_offset_dbm: config.rssi_threshold_offset_dbm,
        challenge_timeout_sec: config.challenge_timeout_sec,
        log_to_file: config.log_to_file,
        log_to_console: config.log_to_console,
        log_level: config.log_level,
    };
    s.storage.save_config(&cfg).await.map_err(|e| { error!("save_config failed: {}", e); e.to_string() })?;
    info!("set_config: saved successfully (log settings apply after restart)");
    Ok(())
}

#[tauri::command]
async fn lock_screen(state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    info!("lock_screen called");
    let s = state.lock().await;
    s.platform.lock_screen().await.map_err(|e| { error!("lock_screen failed: {}", e); e.to_string() })
}

#[tauri::command]
async fn unlock_screen(state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    info!("unlock_screen called");
    let s = state.lock().await;
    s.platform.unlock_screen().await.map_err(|e| { error!("unlock_screen failed: {}", e); e.to_string() })
}

#[tauri::command]
async fn toggle_daemon(enabled: bool, state: State<'_, Arc<Mutex<AppState>>>) -> Result<bool, String> {
    info!("toggle_daemon called: enabled={}", enabled);
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
    // Try ProjectDirs first, fallback to exe_dir/logs, then to ./logs
    let log_dir = directories::ProjectDirs::from("", "", "WristKey")
        .map(|d| d.data_dir().to_path_buf().join("logs"))
        .or_else(|| {
            std::env::current_exe().ok()
                .and_then(|p| p.parent().map(|d| d.join("logs")))
        })
        .unwrap_or_else(|| std::path::PathBuf::from("logs"));

    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("[WristKey] WARNING: failed to create log dir {:?}: {}", log_dir, e);
    }
    let log_path = log_dir.join("wristkey.log");
    println!("[WristKey] Log directory: {:?}", log_dir);
    println!("[WristKey] Log file: {:?}", log_path);

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    let (session, storage, platform, config) = rt.block_on(async {
        let data_dir = directories::ProjectDirs::from("", "", "WristKey")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("data"));
        let _ = std::fs::create_dir_all(&data_dir);

        let storage: Arc<dyn Storage> = match wristkey_core::SqliteStorage::open_default() {
            Ok(s) => Arc::new(s),
            Err(e) => {
                eprintln!("[WristKey] Failed to open sqlite DB ({}), using memory storage", e);
                Arc::new(wristkey_core::MemoryStorage::new())
            }
        };

        let cfg = storage.load_config().await.unwrap_or_default();
        println!("[WristKey] Config loaded: log_to_file={}, log_to_console={}, log_level={}",
            cfg.log_to_file, cfg.log_to_console, cfg.log_level);

        let crypto = Arc::new(wristkey_core::EcdsaP256Crypto);
        let session = Arc::new(SessionManager::new(crypto, storage.clone()));
        let platform = create_platform_adapter();

        if let Err(e) = platform.register_as_authenticator().await {
            eprintln!("[WristKey] Failed to register as authenticator: {}", e);
        }

        (session, storage, platform, cfg)
    });

    // --- Initialize tracing with file + console based on config ---
    let env_filter = EnvFilter::try_new(&config.log_level)
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let console_layer = config.log_to_console.then(|| {
        println!("[WristKey] Console logging enabled at level: {}", config.log_level);
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_ansi(true)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(true)
            .with_line_number(true)
    });

    let (file_layer, _file_guard) = if config.log_to_file {
        let file_appender = tracing_appender::rolling::daily(&log_dir, "wristkey");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        println!("[WristKey] File logging enabled: {:?}", log_path);
        let layer = tracing_subscriber::fmt::layer()
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_target(true)
            .with_thread_ids(false)
            .with_file(true)
            .with_line_number(true);
        (Some(layer), Some(guard))
    } else {
        (None, None)
    };

    if console_layer.is_none() && file_layer.is_none() {
        eprintln!("[WristKey] WARNING: both console and file logging disabled!");
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(console_layer)
            .with(file_layer)
            .init();
        info!("WristKey started - logs at {:?}", log_path);
        info!("WristKey Tauri v2 starting - log_level={}", config.log_level);
    }

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
                        warn!("BLE adapter unavailable: {}. Retrying in 5s...", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let conn_mgr = Arc::new(wristkey_daemon::conn_mgr::ConnectionManager::new());
                let daemon = wristkey_daemon::Daemon::new(session, ble, platform, conn_mgr);

                info!("Daemon loop started");
                if let Err(e) = daemon.run().await {
                    warn!("Daemon crashed: {}. Restarting in 5s...", e);
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
            let lock_i = MenuItem::with_id(app, "lock_now", "Lock Now", true, None::<&str>)?;
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
                        info!("tray: Show clicked");
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "hide" => {
                        info!("tray: Hide clicked");
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.hide();
                        }
                    }
                    "lock_now" => {
                        info!("tray: Lock Now clicked");
                        let platform = create_platform_adapter();
                        let _ = std::thread::spawn(move || {
                            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
                            rt.block_on(async { let _ = platform.lock_screen().await; });
                        }).join();
                    }
                    "quit" => {
                        info!("tray: Quit clicked");
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { .. } = event {
                        info!("tray: icon clicked");
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
            toggle_daemon, set_macos_password, delete_macos_password,
            check_macos_accessibility
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                info!("window: close requested, hiding instead");
                window.hide().unwrap();
                api.prevent_close();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| match event {
            RunEvent::ExitRequested { api, code, .. } => {
                info!("RunEvent::ExitRequested code={:?}", code);
                if code != Some(0) {
                    api.prevent_exit();
                }
            }
            _ => {}
        });
}
