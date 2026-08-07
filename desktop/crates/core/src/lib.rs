//! WristKey core: state machine, crypto traits, and challenge-response logic.
//!
//! This crate is platform-agnostic and contains all business logic.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum WristKeyError {
    #[error("crypto operation failed: {0}")]
    Crypto(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("session error: {0}")]
    Session(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("platform error: {0}")]
    Platform(String),
    #[error("ble error: {0}")]
    Ble(String),
}

pub type Result<T> = std::result::Result<T, WristKeyError>;

#[async_trait]
pub trait CryptoEngine: Send + Sync {
    async fn generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>)>;
    async fn sign(&self, private_key: &[u8], data: &[u8]) -> Result<Vec<u8>>;
    async fn verify(&self, public_key: &[u8], data: &[u8], signature: &[u8]) -> Result<()>;
}

pub struct SoftwareCrypto;

#[async_trait]
impl CryptoEngine for SoftwareCrypto {
    async fn generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();
        Ok((signing_key.to_bytes().to_vec(), verifying_key.to_bytes().to_vec()))
    }

    async fn sign(&self, private_key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
        let key_bytes: [u8; 32] = private_key.try_into()
            .map_err(|_| WristKeyError::Crypto("invalid private key length".into()))?;
        let signing_key = SigningKey::from_bytes(&key_bytes);
        let signature = signing_key.sign(data);
        Ok(signature.to_bytes().to_vec())
    }

    async fn verify(&self, public_key: &[u8], data: &[u8], signature: &[u8]) -> Result<()> {
        let key_bytes: [u8; 32] = public_key.try_into()
            .map_err(|_| WristKeyError::Crypto("invalid public key length".into()))?;
        let verifying_key = VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| WristKeyError::Crypto(e.to_string()))?;
        let sig_bytes: [u8; 64] = signature.try_into()
            .map_err(|_| WristKeyError::Crypto("invalid signature length".into()))?;
        let sig = Signature::from_bytes(&sig_bytes);
        verifying_key.verify(data, &sig).map_err(|_| WristKeyError::InvalidSignature)
    }
}

#[async_trait]
pub trait Storage: Send + Sync {
    async fn save_device(&self, device: &PairedDevice) -> Result<()>;
    async fn load_device(&self, id: Uuid) -> Result<Option<PairedDevice>>;
    async fn list_devices(&self) -> Result<Vec<PairedDevice>>;
    async fn delete_device(&self, id: Uuid) -> Result<()>;
    async fn load_config(&self) -> Result<Config>;
    async fn save_config(&self, config: &Config) -> Result<()>;
}

pub struct MemoryStorage {
    devices: Arc<RwLock<HashMap<Uuid, PairedDevice>>>,
    config: Arc<RwLock<Config>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(Config::default())),
        }
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn save_device(&self, device: &PairedDevice) -> Result<()> {
        self.devices.write().await.insert(device.id, device.clone());
        info!("saved device {}", device.id);
        Ok(())
    }
    async fn load_device(&self, id: Uuid) -> Result<Option<PairedDevice>> {
        Ok(self.devices.read().await.get(&id).cloned())
    }
    async fn list_devices(&self) -> Result<Vec<PairedDevice>> {
        Ok(self.devices.read().await.values().cloned().collect())
    }
    async fn delete_device(&self, id: Uuid) -> Result<()> {
        self.devices.write().await.remove(&id);
        info!("deleted device {}", id);
        Ok(())
    }
    async fn load_config(&self) -> Result<Config> {
        Ok(self.config.read().await.clone())
    }
    async fn save_config(&self, config: &Config) -> Result<()> {
        *self.config.write().await = config.clone();
        Ok(())
    }
}

/// Persistent storage using sled (embedded key-value store).
pub struct SledStorage {
    db: sled::Db,
}

impl SledStorage {
    /// Open or create database at the given path.
    pub fn new(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let db = sled::open(&path).map_err(|e| {
            WristKeyError::Storage(format!("failed to open sled db at {:?}: {}", path, e))
        })?;
        Ok(Self { db })
    }

    /// Default path: platform-specific data directory.
    pub fn default() -> Result<Self> {
        let dirs = directories::ProjectDirs::from("", "", "WristKey")
            .ok_or_else(|| WristKeyError::Storage("cannot determine data directory".into()))?;
        let path = dirs.data_dir();
        std::fs::create_dir_all(path).map_err(|e| {
            WristKeyError::Storage(format!("failed to create data dir: {}", e))
        })?;
        Self::new(path.join("wristkey.db"))
    }
}

#[async_trait]
impl Storage for SledStorage {
    async fn save_device(&self, device: &PairedDevice) -> Result<()> {
        let key = format!("device:{}", device.id);
        let value = bincode::serialize(device).map_err(|e| {
            WristKeyError::Storage(format!("serialize device: {}", e))
        })?;
        self.db.insert(key, value).map_err(|e| {
            WristKeyError::Storage(format!("sled insert: {}", e))
        })?;
        self.db.flush_async().await.map_err(|e| {
            WristKeyError::Storage(format!("sled flush: {}", e))
        })?;
        info!("persisted device {}", device.id);
        Ok(())
    }

    async fn load_device(&self, id: Uuid) -> Result<Option<PairedDevice>> {
        let key = format!("device:{}", id);
        match self.db.get(&key) {
            Ok(Some(value)) => {
                let device: PairedDevice = bincode::deserialize(&value).map_err(|e| {
                    WristKeyError::Storage(format!("deserialize device: {}", e))
                })?;
                Ok(Some(device))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(WristKeyError::Storage(format!("sled get: {}", e))),
        }
    }

    async fn list_devices(&self) -> Result<Vec<PairedDevice>> {
        let mut devices = Vec::new();
        for item in self.db.scan_prefix("device:") {
            let (_, value) = item.map_err(|e| {
                WristKeyError::Storage(format!("sled scan: {}", e))
            })?;
            let device: PairedDevice = bincode::deserialize(&value).map_err(|e| {
                WristKeyError::Storage(format!("deserialize device: {}", e))
            })?;
            devices.push(device);
        }
        Ok(devices)
    }

    async fn delete_device(&self, id: Uuid) -> Result<()> {
        let key = format!("device:{}", id);
        self.db.remove(&key).map_err(|e| {
            WristKeyError::Storage(format!("sled remove: {}", e))
        })?;
        self.db.flush_async().await.map_err(|e| {
            WristKeyError::Storage(format!("sled flush: {}", e))
        })?;
        info!("deleted device {}", id);
        Ok(())
    }

    async fn load_config(&self) -> Result<Config> {
        match self.db.get("config") {
            Ok(Some(value)) => {
                let config: Config = bincode::deserialize(&value).map_err(|e| {
                    WristKeyError::Storage(format!("deserialize config: {}", e))
                })?;
                Ok(config)
            }
            Ok(None) => Ok(Config::default()),
            Err(e) => Err(WristKeyError::Storage(format!("sled get config: {}", e))),
        }
    }

    async fn save_config(&self, config: &Config) -> Result<()> {
        let value = bincode::serialize(config).map_err(|e| {
            WristKeyError::Storage(format!("serialize config: {}", e))
        })?;
        self.db.insert("config", value).map_err(|e| {
            WristKeyError::Storage(format!("sled insert config: {}", e))
        })?;
        self.db.flush_async().await.map_err(|e| {
            WristKeyError::Storage(format!("sled flush config: {}", e))
        })?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PairedDevice {
    pub id: Uuid,
    pub name: String,
    pub public_key: Vec<u8>,
    pub paired_at: DateTime<Utc>,
    pub baseline_rssi: i16,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub auto_lock_timeout_sec: u64,
    pub rssi_threshold_offset_dbm: i16,
    pub challenge_timeout_sec: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_lock_timeout_sec: 30,
            rssi_threshold_offset_dbm: 15,
            challenge_timeout_sec: 10,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Challenge {
    pub nonce: [u8; 16],
    pub issued_at: DateTime<Utc>,
}

impl Challenge {
    pub fn generate() -> Self {
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);
        Self { nonce, issued_at: Utc::now() }
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = self.nonce.to_vec();
        let ts = self.issued_at.timestamp() as u64;
        buf.extend_from_slice(&ts.to_le_bytes());
        buf
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub signature: Vec<u8>,
    pub user_present: bool,
    pub timestamp: DateTime<Utc>,
}

#[async_trait]
pub trait PlatformSecurity: Send + Sync {
    async fn lock_screen(&self) -> Result<()>;
    async fn is_locked(&self) -> Result<bool>;
    async fn register_as_authenticator(&self) -> Result<()>;
}

/// Mock platform security for testing.
pub struct MockPlatformSecurity {
    locked: Arc<RwLock<bool>>,
}

impl MockPlatformSecurity {
    pub fn new() -> Self {
        Self {
            locked: Arc::new(RwLock::new(false)),
        }
    }
}

#[async_trait]
impl PlatformSecurity for MockPlatformSecurity {
    async fn lock_screen(&self) -> Result<()> {
        *self.locked.write().await = true;
        info!("MOCK: screen locked");
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(*self.locked.read().await)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum SessionState {
    Disconnected,
    Pairing { challenge: Challenge, started_at: DateTime<Utc> },
    Authenticated { device_id: Uuid, last_rssi: i16, last_seen: DateTime<Utc> },
    Locked,
}

impl SessionState {
    pub fn is_authenticated(&self) -> bool {
        matches!(self, SessionState::Authenticated { .. })
    }
    pub fn device_id(&self) -> Option<Uuid> {
        match self {
            SessionState::Authenticated { device_id, .. } => Some(*device_id),
            _ => None,
        }
    }
}

pub struct SessionManager {
    crypto: Arc<dyn CryptoEngine>,
    storage: Arc<dyn Storage>,
    state: Arc<RwLock<SessionState>>,
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        Self {
            crypto: Arc::clone(&self.crypto),
            storage: Arc::clone(&self.storage),
            state: Arc::clone(&self.state),
        }
    }
}
impl SessionManager {
    pub fn new(crypto: Arc<dyn CryptoEngine>, storage: Arc<dyn Storage>) -> Self {
        Self { crypto, storage, state: Arc::new(RwLock::new(SessionState::Disconnected)) }
    }
    pub async fn state(&self) -> SessionState {
        self.state.read().await.clone()
    }
    pub async fn list_devices(&self) -> Result<Vec<PairedDevice>> {
        self.storage.list_devices().await
    }
    pub async fn begin_pairing(&self) -> Result<Challenge> {
        let challenge = Challenge::generate();
        *self.state.write().await = SessionState::Pairing { challenge: challenge.clone(), started_at: Utc::now() };
        info!("pairing started");
        Ok(challenge)
    }
    pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, response: &Response, baseline_rssi: i16) -> Result<PairedDevice> {
        let state = self.state.read().await.clone();
        let challenge = match state {
            SessionState::Pairing { challenge, .. } => challenge,
            other => return Err(WristKeyError::Session(format!("expected Pairing, got {:?}", other))),
        };
        self.crypto.verify(&public_key, &challenge.to_bytes(), &response.signature).await?;
        if !response.user_present {
            return Err(WristKeyError::Protocol("user presence required".into()));
        }
        let device = PairedDevice { id: Uuid::new_v4(), name: device_name, public_key, paired_at: Utc::now(), baseline_rssi };
        self.storage.save_device(&device).await?;
        *self.state.write().await = SessionState::Authenticated { device_id: device.id, last_rssi: baseline_rssi, last_seen: Utc::now() };
        info!("pairing completed for {}", device.id);
        Ok(device)
    }
    pub async fn verify_unlock(&self, device_id: Uuid, response: &Response) -> Result<()> {
        let device = self.storage.load_device(device_id).await?.ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        let challenge = Challenge::generate();
        self.crypto.verify(&device.public_key, &challenge.to_bytes(), &response.signature).await?;
        if !response.user_present {
            return Err(WristKeyError::Protocol("user presence required".into()));
        }
        *self.state.write().await = SessionState::Authenticated { device_id, last_rssi: device.baseline_rssi, last_seen: Utc::now() };
        info!("unlock verified for {}", device_id);
        Ok(())
    }
    pub async fn update_rssi(&self, rssi: i16) -> Result<bool> {
        let mut state = self.state.write().await;
        match *state {
            SessionState::Authenticated { ref mut last_rssi, ref mut last_seen, device_id } => {
                *last_rssi = rssi;
                *last_seen = Utc::now();
                let config = self.storage.load_config().await?;
                let device = self.storage.load_device(device_id).await?.ok_or_else(|| WristKeyError::Storage("device missing".into()))?;
                let threshold = device.baseline_rssi - config.rssi_threshold_offset_dbm;
                Ok(rssi < threshold)
            }
            _ => Ok(false),
        }
    }
    pub async fn disconnect(&self) {
        *self.state.write().await = SessionState::Disconnected;
        info!("session disconnected");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[tokio::test]
    async fn test_challenge_generation() {
        let c1 = Challenge::generate();
        let c2 = Challenge::generate();
        assert_ne!(c1.nonce, c2.nonce);
    }

    #[tokio::test]
    async fn test_full_pairing_flow() {
        let crypto = Arc::new(SoftwareCrypto);
        let storage = Arc::new(MemoryStorage::new());
        let manager = SessionManager::new(crypto.clone(), storage);
        let challenge = manager.begin_pairing().await.unwrap();
        let (priv_key, pub_key) = crypto.generate_keypair().await.unwrap();
        let sig = crypto.sign(&priv_key, &challenge.to_bytes()).await.unwrap();
        let response = Response { signature: sig.clone(), user_present: true, timestamp: Utc::now() };
        let device = manager.complete_pairing("Test Watch".into(), pub_key, &response, -50).await.unwrap();
        assert_eq!(device.name, "Test Watch");
        assert!(manager.state().await.is_authenticated());
    }

    #[tokio::test]
    async fn test_unlock_without_user_present_fails() {
        let crypto = Arc::new(SoftwareCrypto);
        let storage = Arc::new(MemoryStorage::new());
        let manager = SessionManager::new(crypto.clone(), storage.clone());
        let challenge = manager.begin_pairing().await.unwrap();
        let (priv_key, pub_key) = crypto.generate_keypair().await.unwrap();
        let sig = crypto.sign(&priv_key, &challenge.to_bytes()).await.unwrap();
        let response = Response { signature: sig.clone(), user_present: true, timestamp: Utc::now() };
        let device = manager.complete_pairing("Watch".into(), pub_key, &response, -50).await.unwrap();
        let bad = Response { signature: sig, user_present: false, timestamp: Utc::now() };
        assert!(manager.verify_unlock(device.id, &bad).await.is_err());
    }

    #[tokio::test]
    async fn test_sled_storage_persistence() {
        let tmp = temp_dir().join(format!("wristkey_test_{}", Uuid::new_v4()));
        let storage = SledStorage::new(&tmp).unwrap();

        let device = PairedDevice {
            id: Uuid::new_v4(),
            name: "Sled Watch".into(),
            public_key: vec![1, 2, 3],
            paired_at: Utc::now(),
            baseline_rssi: -55,
        };

        storage.save_device(&device).await.unwrap();
        let loaded = storage.load_device(device.id).await.unwrap().unwrap();
        assert_eq!(loaded.name, "Sled Watch");
        assert_eq!(loaded.public_key, vec![1, 2, 3]);

        let devices = storage.list_devices().await.unwrap();
        assert_eq!(devices.len(), 1);

        storage.delete_device(device.id).await.unwrap();
        assert!(storage.load_device(device.id).await.unwrap().is_none());

        let config = Config { auto_lock_timeout_sec: 60, ..Default::default() };
        storage.save_config(&config).await.unwrap();
        let loaded_config = storage.load_config().await.unwrap();
        assert_eq!(loaded_config.auto_lock_timeout_sec, 60);

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
