//! WristKey daemon binary entry point.

mod tray;

use std::sync::Arc;
use clap::Parser;
use tracing::{error, info, warn};
use wristkey_core::{Config, CryptoEngine, EcdsaP256Crypto, SessionManager, SledStorage, Storage};

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

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

    let config_path = cli.config.unwrap_or_else(|| {
        directories::ProjectDirs::from("", "", "WristKey")
            .map(|d| d.config_dir().join("config.toml"))
            .unwrap_or_else(|| std::path::PathBuf::from("config.toml"))
    });

    let config = if config_path.exists() {
        Config::from_file(&config_path)?
    } else {
        let default = Config::default();
        if let Some(parent) = config_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        default.to_file(&config_path)?;
        default
    };

    let crypto: Arc<dyn CryptoEngine> = Arc::new(EcdsaP256Crypto);
    let storage: Arc<dyn Storage> = Arc::new(SledStorage::open_default()?);
    let _ = storage.save_config(&config).await;

    let session = Arc::new(SessionManager::new(crypto, storage.clone()));

    // Multi-device: show paired count on startup
    match session.list_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                println!("No paired devices. Use pairing mode on your watch.");
            } else {
                println!("Paired devices ({}):", devices.len());
                for d in &devices {
                    println!("  {} - {} (paired at {}, baseline RSSI: {} dBm)",
                        d.id, d.name, d.paired_at.format("%Y-%m-%d %H:%M"), d.baseline_rssi);
                }
            }
        }
        Err(e) => {
            warn!("Failed to list devices: {}", e);
        }
    }

    if cli.list_devices {
        return Ok(());
    }

    if cli.pair {
        println!("Pairing mode: scanning for 30 seconds…");
        println!("Use daemon auto-pairing for now.");
        return Ok(());
    }

    let platform = create_platform_adapter();

    // Tray command channel
    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<tray::TrayCommand>();
    let data_dir = directories::ProjectDirs::from("", "", "WristKey")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/wristkey"));

    let tray_handle = std::thread::spawn(move || {
        while let Ok(cmd) = tray_rx.recv() {
            match cmd {
                tray::TrayCommand::Quit => {
                    info!("Quit command received from tray handler");
                    break;
                }
                tray::TrayCommand::ResetPairing => {
                    let db_path = data_dir.join("wristkey.db");
                    if db_path.exists() {
                        match std::fs::remove_dir_all(&db_path) {
                            Ok(_) => info!("Pairing database removed: {}", db_path.display()),
                            Err(e) => warn!("Failed to remove database (daemon may have it open): {}. Restart daemon and try again.", e),
                        }
                    } else {
                        info!("No pairing database to reset");
                    }
                }
                tray::TrayCommand::OpenLogs => {
                    let log_dir = data_dir.join("logs");
                    #[cfg(target_os = "windows")]
                    let _ = std::process::Command::new("explorer").arg(&log_dir).spawn();
                    #[cfg(target_os = "macos")]
                    let _ = std::process::Command::new("open").arg(&log_dir).spawn();
                    #[cfg(target_os = "linux")]
                    let _ = std::process::Command::new("xdg-open").arg(&log_dir).spawn();
                }
            }
        }
    });

    println!("╔══════════════════════════════════════╗");
    println!("║     WristKey Daemon v0.1.0           ║");
    println!("║     PC unlock via Wear OS            ║");
    println!("╚══════════════════════════════════════╝");
    println!("Logs: {}", log_dir.display());

    match wristkey_ble::BtleplugAdapter::new().await {
        Ok(adapter) => {
            info!("BLE adapter initialized");
            let mgr = Arc::new(ConnectionManager::new());
            
            // Start advertisement-only presence loop (solves Windows BLE conflict)
            let adapter_clone = adapter.btleplug_adapter().expect("BtleplugAdapter required");
            let mgr_clone = mgr.clone();
            tokio::spawn(async move {
                if let Err(e) = run_presence_loop(adapter_clone, mgr_clone).await {
                    error!("presence loop crashed: {}", e);
                }
            });
            
            let ble = Arc::new(adapter);
            let daemon = wristkey_daemon::Daemon::new(session, ble, platform, mgr);

            let daemon_handle = tokio::spawn(async move {
                if let Err(e) = daemon.run().await {
                    error!("daemon crashed: {}", e);
                }
            });

            #[cfg(feature = "tray")]
            println!("Tray icon enabled. Right-click the icon to control.");
            #[cfg(not(feature = "tray"))]
            println!("Running in headless mode (no tray icon). Use Task Manager to stop.");

            tray::run_tray(tray_tx);
            daemon_handle.abort();
        }
        Err(e) => {
            warn!("BLE adapter unavailable: {}. Running in tray-only mode.", e);
            println!("⚠️  Bluetooth unavailable: {}", e);
            println!("   WristKey will run in tray-only mode. Connect a BLE adapter and restart.");
            tray::run_tray(tray_tx);
        }
    }

    tray_handle.join().unwrap();
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
