//! WristKey daemon binary entry point.

use std::sync::Arc;
use wristkey_core::{Config, CryptoEngine, MemoryStorage, SessionManager, SoftwareCrypto, Storage};
use wristkey_ble::BtleplugAdapter;
use wristkey_daemon::Daemon;
use wristkey_platform_linux::LinuxSecurity;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    info!("WristKey daemon starting");

    let crypto: Arc<dyn CryptoEngine> = Arc::new(SoftwareCrypto);
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
    if storage.load_config().await.is_err() {
        let _ = storage.save_config(&Config::default()).await;
    }

    let session = SessionManager::new(crypto, storage);
    let ble = Arc::new(BtleplugAdapter::new().await?);
    let platform = Arc::new(LinuxSecurity::new());

    let daemon = Daemon::new(session, ble, platform);
    daemon.run().await.map_err(|e| e.into())
}
