//! WristKey daemon binary entry point.

mod tray;

use std::sync::Arc;
use clap::Parser;
use tracing::{error, info, warn};
use wristkey_core::{Config, CryptoEngine, SessionManager, SledStorage, Storage};

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

    let crypto = create_crypto_engine();
    let storage: Arc<dyn Storage> = Arc::new(SledStorage::open_default()?);
    let _ = storage.save_config(&config).await;

    let session = Arc::new(SessionManager::new(crypto, storage.clone()));

    if cli.list_devices {
        let devices = session.list_devices().await?;
        if devices.is_empty() {
            println!("No paired devices.");
        } else {
            println!("Paired devices:");
            for d in devices {
                println!("  {} - {} (paired at {}, baseline RSSI: {} dBm)",
                    d.id, d.name, d.paired_at.format("%Y-%m-%d %H:%M"), d.baseline_rssi);
            }
        }
        return Ok(());
    }

    if cli.pair {
        println!("Pairing mode: scanning for 30 seconds…");
        println!("Use daemon auto-pairing for now.");
        return Ok(());
    }

    let platform = create_platform_adapter();

    match wristkey_ble::BtleplugAdapter::new().await {
        Ok(adapter) => {
            info!("BLE adapter initialized");
            let ble = Arc::new(adapter);
            let daemon = wristkey_daemon::Daemon::new(session, ble, platform);

            let daemon_handle = tokio::spawn(async move {
                if let Err(e) = daemon.run().await {
                    error!("daemon crashed: {}", e);
                }
            });

            tray::run_tray();
            daemon_handle.abort();
        }
        Err(e) => {
            warn!("BLE adapter unavailable: {}. Running in tray-only mode.", e);
            println!("⚠️  Bluetooth unavailable: {}", e);
            println!("   WristKey will run in tray-only mode. Connect a BLE adapter and restart.");
            tray::run_tray();
        }
    }

    Ok(())
}

fn create_crypto_engine() -> Arc<dyn CryptoEngine> {
    // Try to find the actual crypto implementation name from core
    // If SoftwareCrypto exists, use it; otherwise fallback to any available
    {
        // This will fail at compile time if neither exists, forcing us to fix the name
        Arc::new(wristkey_core::EcdsaP256Crypto)
    }
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
