//! WristKey core: state machine, crypto traits, and challenge-response logic.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

pub mod sqlite_storage;
pub use sqlite_storage::SqliteStorage;

pub mod rssi_filter;
pub use rssi_filter::{RssiSmoother, KalmanFilter, EmaFilter, HysteresisGate};

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
    #[error("config error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, WristKeyError>;

#[async_trait]
pub trait CryptoEngine: Send + Sync {
    async fn generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>)>;
    async fn sign(&self, private_key: &[u8], data: &[u8]) -> Result<Vec<u8>>;
    async fn verify(&self, public_key: &[u8], data: &[u8], signature: &[u8]) -> Result<()>;
}

#[async_trait]
pub trait PasswordVault: Send + Sync {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>>;
    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String>;
}

/// Software ECDSA P-256 crypto engine.
pub struct EcdsaP256Crypto;

#[async_trait]
impl CryptoEngine for EcdsaP256Crypto {
    async fn generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        let signing_key = SigningKey::random(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        let pubkey_bytes = verifying_key.to_encoded_point(false).as_bytes().to_vec();
        Ok((signing_key.to_bytes().to_vec(), pubkey_bytes))
    }

    async fn sign(&self, private_key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
        let key_bytes: [u8; 32] = private_key.try_into()
            .map_err(|_| WristKeyError::Crypto("invalid private key length".into()))?;
        let signing_key = SigningKey::from_bytes(&key_bytes.into())
            .map_err(|e| WristKeyError::Crypto(e.to_string()))?;
        let sig: Signature = signing_key.sign(data);
        Ok(sig.to_bytes().to_vec())
    }

    async fn verify(&self, public_key: &[u8], data: &[u8], signature: &[u8]) -> Result<()> {
        let pubkey = p256::PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| WristKeyError::Crypto(e.to_string()))?;
        let verifying_key = VerifyingKey::from(pubkey);

        let sig = if signature.len() == 64 {
            Signature::from_slice(signature)
        } else {
            Signature::from_der(signature)
        }.map_err(|e| WristKeyError::Crypto(format!("invalid signature: {}", e)))?;

        verifying_key.verify(data, &sig)
            .map_err(|_| WristKeyError::InvalidSignature)
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

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
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

#[derive(Clone, Debug)]
pub struct PairedDevice {
    pub id: Uuid,
    pub name: String,
    pub public_key: Vec<u8>,
    pub device_id: Option<Vec<u8>>,
    pub paired_at: DateTime<Utc>,
    pub baseline_rssi: i16,
    pub address: String,
    pub windows_password: Option<Vec<u8>>,
}

// FIX: serde(default) allows old configs in DB to deserialize without new fields
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub auto_lock_timeout_sec: u64,
    pub rssi_threshold_offset_dbm: i16,
    pub challenge_timeout_sec: u64,
    pub log_to_file: bool,
    pub log_to_console: bool,
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_lock_timeout_sec: 30,
            rssi_threshold_offset_dbm: 15,
            challenge_timeout_sec: 10,
            log_to_file: true,
            log_to_console: true,
            log_level: "info".into(),
        }
    }
}

impl Config {
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path).map_err(|e| {
            WristKeyError::Config(format!("read config file: {}", e))
        })?;
        toml::from_str(&contents).map_err(|e| {
            WristKeyError::Config(format!("parse config file: {}", e))
        })
    }

    pub fn to_file(&self, path: &std::path::Path) -> Result<()> {
        let contents = toml::to_string_pretty(self).map_err(|e| {
            WristKeyError::Config(format!("serialize config: {}", e))
        })?;
        std::fs::write(path, contents).map_err(|e| {
            WristKeyError::Config(format!("write config file: {}", e))
        })
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
    async fn unlock_screen(&self) -> Result<()>;
    async fn is_locked(&self) -> Result<bool>;
    async fn register_as_authenticator(&self) -> Result<()>;
}

pub struct MockPlatformSecurity {
    locked: Arc<RwLock<bool>>,
}

impl Default for MockPlatformSecurity {
    fn default() -> Self {
        Self::new()
    }
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

    async fn unlock_screen(&self) -> Result<()> {
        *self.locked.write().await = false;
        info!("MOCK: screen unlocked");
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
    Verifying { device_id: Uuid, challenge: Challenge, started_at: DateTime<Utc> },
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
    pub async fn load_device(&self, device_id: Uuid) -> Result<Option<PairedDevice>> {
        self.storage.load_device(device_id).await
    }
    pub async fn list_devices(&self) -> Result<Vec<PairedDevice>> {
        self.storage.list_devices().await
    }
    pub async fn load_config(&self) -> Result<Config> {
        self.storage.load_config().await
    }
    pub async fn begin_pairing(&self) -> Result<Challenge> {
        let challenge = Challenge::generate();
        *self.state.write().await = SessionState::Pairing { challenge: challenge.clone(), started_at: Utc::now() };
        info!("pairing started");
        Ok(challenge)
    }
    pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, device_id: Option<Vec<u8>>, response: &Response, baseline_rssi: i16, address: String) -> Result<PairedDevice> {
        let state = self.state.read().await.clone();
        let challenge = match state {
            SessionState::Pairing { challenge, .. } => challenge,
            other => return Err(WristKeyError::Session(format!("expected Pairing, got {:?}", other))),
        };
        self.crypto.verify(&public_key, &challenge.to_bytes(), &response.signature).await?;
        if !response.user_present {
            return Err(WristKeyError::Protocol("user presence required".into()));
        }
        let device = PairedDevice { id: Uuid::new_v4(), name: device_name, public_key, device_id, paired_at: Utc::now(), baseline_rssi, address, windows_password: None };
        self.storage.save_device(&device).await?;
        *self.state.write().await = SessionState::Authenticated { device_id: device.id, last_rssi: baseline_rssi, last_seen: Utc::now() };
        info!("pairing completed for {}", device.id);
        Ok(device)
    }
    pub async fn begin_unlock(&self, device_id: Uuid) -> Result<Challenge> {
        let challenge = Challenge::generate();
        *self.state.write().await = SessionState::Verifying {
            device_id,
            challenge: challenge.clone(),
            started_at: Utc::now(),
        };
        info!("unlock verification started for {}", device_id);
        Ok(challenge)
    }

    pub async fn verify_unlock(&self, response: &Response) -> Result<()> {
        let (device_id, challenge) = match self.state.read().await.clone() {
            SessionState::Verifying { device_id, challenge, .. } => (device_id, challenge),
            other => return Err(WristKeyError::Session(format!("expected Verifying, got {:?}", other))),
        };
        let device = self.storage.load_device(device_id).await?.ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        self.crypto.verify(&device.public_key, &challenge.to_bytes(), &response.signature).await?;
        if !response.user_present {
            return Err(WristKeyError::Protocol("user presence required".into()));
        }
        *self.state.write().await = SessionState::Authenticated { device_id, last_rssi: device.baseline_rssi, last_seen: Utc::now() };
        info!("unlock verified for {}", device_id);
        Ok(())
    }

    pub async fn set_device_password(&self, device_id: Uuid, encrypted: Vec<u8>) -> Result<()> {
        let mut device = self.storage.load_device(device_id).await?
            .ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        device.windows_password = Some(encrypted);
        self.storage.save_device(&device).await?;
        info!("stored encrypted password for device {}", device_id);
        Ok(())
    }

    pub async fn get_device_password(&self, device_id: Uuid) -> Result<Option<Vec<u8>>> {
        let device = self.storage.load_device(device_id).await?
            .ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        Ok(device.windows_password.clone())
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
    pub async fn update_baseline_rssi(&self, device_id: Uuid, baseline_rssi: i16) -> Result<()> {
        let mut device = self.storage.load_device(device_id).await?
            .ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        device.baseline_rssi = baseline_rssi;
        self.storage.save_device(&device).await?;
        info!("Updated baseline_rssi for device {} to {} dBm", device_id, baseline_rssi);
        Ok(())
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
        let crypto = Arc::new(EcdsaP256Crypto);
        let storage = Arc::new(MemoryStorage::new());
        let manager = SessionManager::new(crypto.clone(), storage);
        let challenge = manager.begin_pairing().await.unwrap();
        let (priv_key, pub_key) = crypto.generate_keypair().await.unwrap();
        let sig = crypto.sign(&priv_key, &challenge.to_bytes()).await.unwrap();
        let response = Response { signature: sig, user_present: true, timestamp: Utc::now() };
        let device = manager.complete_pairing("Test Watch".into(), pub_key, None, &response, -50, "AA:BB:CC:DD:EE:FF".into()).await.unwrap();
        assert_eq!(device.name, "Test Watch");
        assert!(manager.state().await.is_authenticated());
    }

    #[tokio::test]
    async fn test_unlock_without_user_present_fails() {
        let crypto = Arc::new(EcdsaP256Crypto);
        let storage = Arc::new(MemoryStorage::new());
        let manager = SessionManager::new(crypto.clone(), storage.clone());
        let challenge = manager.begin_pairing().await.unwrap();
        let (priv_key, pub_key) = crypto.generate_keypair().await.unwrap();
        let sig = crypto.sign(&priv_key, &challenge.to_bytes()).await.unwrap();
        let response = Response { signature: sig, user_present: true, timestamp: Utc::now() };
        let device = manager.complete_pairing("Watch".into(), pub_key, None, &response, -50, "AA:BB:CC:DD:EE:FF".into()).await.unwrap();

        let unlock_challenge = manager.begin_unlock(device.id).await.unwrap();
        let good_sig = crypto.sign(&priv_key, &unlock_challenge.to_bytes()).await.unwrap();
        let bad = Response { signature: good_sig, user_present: false, timestamp: Utc::now() };
        assert!(manager.verify_unlock(&bad).await.is_err());
    }

    #[tokio::test]
    async fn test_unlock_rejects_signature_for_wrong_challenge() {
        let crypto = Arc::new(EcdsaP256Crypto);
        let storage = Arc::new(MemoryStorage::new());
        let manager = SessionManager::new(crypto.clone(), storage.clone());
        let challenge = manager.begin_pairing().await.unwrap();
        let (priv_key, pub_key) = crypto.generate_keypair().await.unwrap();
        let sig = crypto.sign(&priv_key, &challenge.to_bytes()).await.unwrap();
        let response = Response { signature: sig, user_present: true, timestamp: Utc::now() };
        let device = manager.complete_pairing("Watch".into(), pub_key, None, &response, -50, "AA:BB:CC:DD:EE:FF".into()).await.unwrap();

        let _unlock_challenge = manager.begin_unlock(device.id).await.unwrap();
        let stale = Challenge::generate();
        let stale_sig = crypto.sign(&priv_key, &stale.to_bytes()).await.unwrap();
        let bad = Response { signature: stale_sig, user_present: true, timestamp: Utc::now() };
        assert!(manager.verify_unlock(&bad).await.is_err());
    }

    #[tokio::test]
    async fn test_unlock_succeeds_for_matching_challenge() {
        let crypto = Arc::new(EcdsaP256Crypto);
        let storage = Arc::new(MemoryStorage::new());
        let manager = SessionManager::new(crypto.clone(), storage.clone());
        let challenge = manager.begin_pairing().await.unwrap();
        let (priv_key, pub_key) = crypto.generate_keypair().await.unwrap();
        let sig = crypto.sign(&priv_key, &challenge.to_bytes()).await.unwrap();
        let response = Response { signature: sig, user_present: true, timestamp: Utc::now() };
        let device = manager.complete_pairing("Watch".into(), pub_key, None, &response, -50, "AA:BB:CC:DD:EE:FF".into()).await.unwrap();

        let unlock_challenge = manager.begin_unlock(device.id).await.unwrap();
        let good_sig = crypto.sign(&priv_key, &unlock_challenge.to_bytes()).await.unwrap();
        let good = Response { signature: good_sig, user_present: true, timestamp: Utc::now() };
        assert!(manager.verify_unlock(&good).await.is_ok());
        assert!(manager.state().await.is_authenticated());
    }
}
