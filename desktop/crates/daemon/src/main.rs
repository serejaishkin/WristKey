//! WristKey daemon binary entry point.

use std::sync::Arc;
use clap::Parser;
use tracing::info;
use wristkey_core::{Config, CryptoEngine, SessionManager, SledStorage, SoftwareCrypto, Storage};
use wristkey_ble::BtleplugAdapter;
use wristkey_daemon::Daemon;
use wristkey_platform_linux::LinuxSecurity;

#[derive(Parser, Debug)]
#[command(name = "wristkeyd")]
#[command(about = "WristKey daemon — unlock your PC via Wear OS")]
struct Cli {
    /// Path to config file (TOML)
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    /// One-shot pairing mode (scan for 30s and pair new device)
    #[arg(long)]
    pair: bool,

    /// List paired devices and exit
    #[arg(long)]
    list_devices: bool,

    /// Run in foreground (don't daemonize)
    #[arg(long, default_value = "true")]
    foreground: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Logging: stdout + file rotation
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

    // Load config
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
        // TODO: implement one-shot pairing CLI flow
        println!("Pairing not yet implemented in CLI mode. Use daemon auto-pairing.");
        return Ok(());
    }

    // Normal daemon mode
    let ble = Arc::new(BtleplugAdapter::new().await?);
    let platform = Arc::new(LinuxSecurity::new());

    let daemon = Daemon::new(session, ble, platform);
    daemon.run().await.map_err(|e| e.into())
}
