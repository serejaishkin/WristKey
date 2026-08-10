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
    #[arg(long)]
    pair: bool,
    #[arg(long)]
    list_devices: bool,
    #[arg(long, default_value = "true")]
    foreground: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.pair {
        // PAIRING MODE: no tokio on main thread (winit requirement on Windows)
        println!("Pairing mode: opening GUI…");
        let crypto: Arc<dyn CryptoEngine> = Arc::new(EcdsaP256Crypto);
        let storage = Arc::new(SledStorage::open_default().expect("storage"));
        let session = Arc::new(SessionManager::new(crypto, storage.clone()));
        wristkey_daemon::pair_gui::run_pairing_gui(session, storage);
        return;
    }

    // DAEMON MODE: tokio runtime
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Err(e) = rt.block_on(run_daemon(cli)) {
        eprintln!("Daemon error: {}", e);
        std::process::exit(1);
    }
}

async fn run_daemon(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
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

    if cli.list_devices {
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
        return Ok(());
    }

    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<tray::TrayCommand>();

    let tray_thread = std::thread::spawn(move || {
        tray::run_tray(tray_tx);
    });

    let pair_flag_path = data_dir.join(".pairing_request");

    println!("╔══════════════════════════════════════╗");
    println!("║  WristKey Daemon v0.1.0              ║");
    println!("║  PC unlock via Wear OS               ║");
    println!("╚══════════════════════════════════════╝");
    println!("Logs: {}", log_dir.display());
    println!("Right-click tray icon to control.");

    let platform = create_platform_adapter();

    let mut storage: Option<Arc<dyn Storage>> = Some(Arc::new(SledStorage::open_default()?));
    let mut session: Option<Arc<SessionManager>> = Some(Arc::new(SessionManager::new(crypto.clone(), storage.as_ref().unwrap().clone())));

    loop {
        let _ = std::fs::remove_file(&pair_flag_path);

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
        tokio::spawn(async move {
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
            if pair_flag_path.exists() {
                let _ = std::fs::remove_file(&pair_flag_path);
                info!("Pairing GUI requested — showing instructions");
                if let Some(handle) = daemon_handle.take() {
                    handle.abort();
                    while !handle.is_finished() {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    drop(handle);
                }
                drop(session.take());
                drop(storage.take());
                tokio::time::sleep(Duration::from_millis(300)).await;
                println!("========================================");
                println!("To pair a new device, please run:");
                println!("  wristkeyd.exe --pair");
                println!("  (close this daemon first)");
                println!("========================================");
                restart = true;
                continue;
            }

            match tray_rx.try_recv() {
                Ok(tray::TrayCommand::Quit) => {
                    info!("Quit command from tray");
                    if let Some(handle) = daemon_handle.take() {
                        handle.abort();
                    }
                    quit = true;
                }
                Ok(tray::TrayCommand::PairDevice) => {
                    let _ = std::fs::write(&pair_flag_path, b"1");
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
        storage = Some(Arc::new(SledStorage::open_default()?));
        session = Some(Arc::new(SessionManager::new(crypto.clone(), storage.as_ref().unwrap().clone())));
    }

    tray_thread.join().unwrap();
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
