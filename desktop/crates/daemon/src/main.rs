//! WristKey daemon binary entry point.

use std::sync::Arc;
use clap::Parser;
use tracing::info;
use wristkey_core::{Config, CryptoEngine, SessionManager, SledStorage, SoftwareCrypto, Storage};

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

    let crypto: Arc<dyn CryptoEngine> = Arc::new(SoftwareCrypto);
    let storage: Arc<dyn Storage> = Arc::new(SledStorage::default()?);
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
        println!("Pairing mode: scanning for 30 seconds...");
        println!("Use daemon auto-pairing for now.");
        return Ok(());
    }

    let ble = Arc::new(wristkey_ble::BtleplugAdapter::new().await?);
    let platform = create_platform_adapter();

    let daemon = wristkey_daemon::Daemon::new(session, ble, platform);
    daemon.run().await.map_err(|e| e.into())
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
