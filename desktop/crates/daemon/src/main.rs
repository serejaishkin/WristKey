//! WristKey daemon binary entry point.

mod tray;

use std::sync::Arc;
use std::time::Duration;
use clap::Parser;
use tracing::{error, info, warn};
use wristkey_core::{Config, CryptoEngine, EcdsaP256Crypto, SessionManager, SledStorage, Storage};
use wristkey_ble::BleAdapter;
use wristkey_daemon::conn_mgr::{ConnectionManager, run_presence_loop};

#[cfg(target_os = "linux")]
use wristkey_platform_linux::LinuxSecurity;
#[cfg(windows)]
use wristkey_platform_win::WindowsSecurity;
#[cfg(target_os = "macos")]
use wristkey_platform_macos::MacOSSecurity;

#[derive(Parser, Debug)]
#[command(name = "wristkeyd")]
#[command(about = "WristKey daemon — unlock your PC via Wear OS")]
struct Cli {
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,
    /// Opens the unified GUI (Status / Devices / Settings). `--pair` is kept
    /// as an alias for backward compatibility with existing shortcuts/docs.
    #[arg(long)]
    gui: bool,
    #[arg(long)]
    pair: bool,
    #[arg(long)]
    list_devices: bool,
    #[arg(long, default_value = "true")]
    foreground: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.gui || cli.pair {
        println!("Opening WristKey…");
        let crypto: Arc<dyn CryptoEngine> = Arc::new(EcdsaP256Crypto);
        let storage = Arc::new(SledStorage::open_default().expect("storage"));
        let session = Arc::new(SessionManager::new(crypto, storage.clone()));
        wristkey_daemon::gui::run_app(session, storage);
        return;
    }

    if cli.list_devices {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        if let Err(e) = rt.block_on(list_devices()) {
            eprintln!("Error: {}", e);
        }
        return;
    }

    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<tray::TrayCommand>();

    let daemon_thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        if let Err(e) = rt.block_on(run_daemon(cli, tray_rx)) {
            eprintln!("Daemon error: {}", e);
        }
    });

    tray::run_tray(tray_tx);
    let _ = daemon_thread.join();
}

async fn list_devices() -> Result<(), Box<dyn std::error::Error>> {
    let crypto: Arc<dyn CryptoEngine> = Arc::new(EcdsaP256Crypto);
    let storage = Arc::new(SledStorage::open_default()?);
    let session = Arc::new(SessionManager::new(crypto, storage.clone()));
    match session.list_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                println!("No paired devices.");
            } else {
                for d in &devices {
                    println!("{} - {} (paired at {}, baseline RSSI: {} dBm)",
                        d.id, d.name, d.paired_at.format("%Y-%m-%d %H:%M"), d.baseline_rssi);
                }
            }
        }
        Err(e) => println!("Error: {}", e),
    }
    Ok(())
}

async fn run_daemon(cli: Cli, tray_rx: std::sync::mpsc::Receiver<tray::TrayCommand>) -> Result<(), Box<dyn std::error::Error>> {
    let log_dir = directories::ProjectDirs::from("", "", "WristKey")
        .map(|d| d.data_dir().join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/wristkey/logs"));
    std::fs::create_dir_all(&log_dir)?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "wristkeyd.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(non_blocking)
        .with_target(false)
        .with_thread_ids(true)
        .init();

    info!("WristKey daemon starting");

    let _config_path = cli.config.clone().unwrap_or_else(|| {
        directories::ProjectDirs::from("", "", "WristKey")
            .map(|d| d.config_dir().join("config.toml"))
            .unwrap_or_else(|| std::path::PathBuf::from("config.toml"))
    });

    let _config = if _config_path.exists() {
        Config::from_file(&_config_path)?
    } else {
        let default = Config::default();
        if let Some(parent) = _config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        default.to_file(&_config_path)?;
        default
    };

    let crypto: Arc<dyn CryptoEngine> = Arc::new(EcdsaP256Crypto);
    let data_dir = directories::ProjectDirs::from("", "", "WristKey")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/wristkey"));

    println!("╔══════════════════════════════════════╗");
    println!("║  WristKey Daemon v0.1.0              ║");
    println!("║  PC unlock via Wear OS               ║");
    println!("╚══════════════════════════════════════╝");
    println!("Logs: {}", log_dir.display());
    println!("Right-click tray icon to control.");

    let platform = create_platform_adapter();

    let mut storage: Option<Arc<dyn Storage>> = Some(Arc::new(SledStorage::open_default()?));
    let mut session: Option<Arc<SessionManager>> = Some(Arc::new(SessionManager::new(crypto.clone(), storage.as_ref().unwrap().clone())));
    storage.as_ref().unwrap().save_config(&_config).await?;

    loop {
        let ble = match wristkey_ble::BtleplugAdapter::new().await {
            Ok(adapter) => Arc::new(adapter),
            Err(e) => {
                warn!("BLE adapter unavailable: {}. Retrying in 5s…", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let mgr = Arc::new(ConnectionManager::new());
        let adapter_clone = ble.btleplug_adapter().expect("BtleplugAdapter required");
        let mgr_clone = mgr.clone();
        let presence_handle = tokio::spawn(async move {
            if let Err(e) = run_presence_loop(adapter_clone, mgr_clone).await {
                error!("presence loop crashed: {}", e);
            }
        });

        let daemon = wristkey_daemon::Daemon::new(
            session.as_ref().unwrap().clone(),
            ble,
            platform.clone(),
            mgr,
        );
        let mut daemon_handle: Option<tokio::task::JoinHandle<()>> = Some(tokio::spawn(async move {
            if let Err(e) = daemon.run().await {
                error!("daemon crashed: {}", e);
            }
        }));

        let mut quit = false;
        let mut restart = false;
        while !quit && !restart {
            match tray_rx.try_recv() {
                Ok(tray::TrayCommand::Quit) => {
                    info!("Quit command from tray");
                    if let Some(handle) = daemon_handle.take() {
                        handle.abort();
                    }
                    quit = true;
                }
                Ok(tray::TrayCommand::PairDevice) => {
                    info!("Pairing requested — opening GUI");
                    if let Some(handle) = daemon_handle.take() {
                        handle.abort();
                    }
                    drop(session.take());
                    drop(storage.take());
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("wristkeyd"));
                    match std::process::Command::new(&exe).arg("--gui").spawn() {
                        Ok(_) => info!("Launched WristKey GUI"),
                        Err(e) => warn!("Failed to launch WristKey GUI: {}", e),
                    }
                    // Fall through to the wait-for-storage-to-free loop below
                    // instead of returning — otherwise the daemon would die
                    // permanently every time "Pair Device" is used from the tray,
                    // requiring the person to manually restart wristkeyd.exe.
                    restart = true;
                }
                Ok(tray::TrayCommand::ListDevices) => {
                    if let Some(ref s) = session {
                        match s.list_devices().await {
                            Ok(devices) => {
                                if devices.is_empty() {
                                    info!("No paired devices.");
                                    println!("No paired devices.");
                                } else {
                                    info!("Paired devices:");
                                    println!("=== Paired Devices ===");
                                    for d in &devices {
                                        let line = format!("📱 {} (ID: {}, RSSI: {} dBm)", d.name, d.id, d.baseline_rssi);
                                        info!("{}", line);
                                        println!("{}", line);
                                    }
                                    println!("=======================");
                                }
                            }
                            Err(e) => warn!("Failed to list devices: {}", e),
                        }
                    }
                }
                Ok(tray::TrayCommand::ResetPairing) => {
                    let db_path = data_dir.join("wristkey.db");
                    if db_path.exists() {
                        match std::fs::remove_dir_all(&db_path) {
                            Ok(_) => info!("Pairing database removed"),
                            Err(e) => warn!("Failed to remove database: {}", e),
                        }
                    }
                }
                Ok(tray::TrayCommand::OpenLogs) => {
                    let log_dir = data_dir.join("logs");
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("explorer").arg(&log_dir).spawn();
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&log_dir).spawn();
                    #[cfg(target_os = "linux")]
                    let _ = std::process::Command::new("xdg-open").arg(&log_dir).spawn();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if let Some(handle) = daemon_handle.take() {
                        handle.abort();
                    }
                    quit = true;
                }
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if quit {
            break;
        }
        info!("Waiting for the pairing database to be free before resuming...");
        let reopened = loop {
            if let Ok(tray::TrayCommand::Quit) = tray_rx.try_recv() {
                info!("Quit received while waiting for pairing to finish");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            match SledStorage::open_default() {
                Ok(s) => break Arc::new(s),
                Err(e) => {
                    warn!("pairing database still in use ({}), still waiting...", e);
                }
            }
        };
        storage = Some(reopened as Arc<dyn Storage>);
        session = Some(Arc::new(SessionManager::new(crypto.clone(), storage.as_ref().unwrap().clone())));
    }

    info!("WristKey daemon stopped");
    Ok(())
}

fn create_platform_adapter() -> Arc<dyn wristkey_core::PlatformSecurity> {
    #[cfg(target_os = "linux")]
    {
        Arc::new(LinuxSecurity::new())
    }
    #[cfg(windows)]
    {
        Arc::new(WindowsSecurity::new())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacOSSecurity::new())
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        compile_error!("unsupported platform")
    }
}
