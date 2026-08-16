use async_trait::async_trait;
use chrono::{DateTime, Utc};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::ecdsa::signature::{Signer, Verifier};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, WristKeyError>;

#[derive(thiserror::Error, Debug, Clone)]
pub enum WristKeyError {
    #[error("crypto error: {0}")]
    Crypto(String),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("session error: {0}")]
    Session(String),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("ble error: {0}")]
    Ble(String),
    #[error("platform error: {0}")]
    Platform(String),
}

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

impl Default for MemoryStorage {
    fn default() -> Self { Self::new() }
}
impl MemoryStorage {
    pub fn new() -> Self {
        Self { devices: Arc::new(RwLock::new(HashMap::new())), config: Arc::new(RwLock::new(Config::default())) }
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn save_device(&self, device: &PairedDevice) -> Result<()> {
        self.devices.write().await.insert(device.id, device.clone());
        info!("saved device {}", device.id); Ok(())
    }
    async fn load_device(&self, id: Uuid) -> Result<Option<PairedDevice>> {
        Ok(self.devices.read().await.get(&id).cloned())
    }
    async fn list_devices(&self) -> Result<Vec<PairedDevice>> {
        Ok(self.devices.read().await.values().cloned().collect())
    }
    async fn delete_device(&self, id: Uuid) -> Result<()> {
        self.devices.write().await.remove(&id);
        info!("deleted device {}", id); Ok(())
    }
    async fn load_config(&self) -> Result<Config> {
        Ok(self.config.read().await.clone())
    }
    async fn save_config(&self, config: &Config) -> Result<()> {
        *self.config.write().await = config.clone(); Ok(())
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
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
        Self { auto_lock_timeout_sec: 30, rssi_threshold_offset_dbm: 15, challenge_timeout_sec: 10,
            log_to_file: true, log_to_console: true, log_level: "info".into() }
    }
}

impl Config {
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        if !path.exists() { return Ok(Self::default()); }
        let contents = std::fs::read_to_string(path).map_err(|e| WristKeyError::Config(format!("read config file: {}", e)))?;
        toml::from_str(&contents).map_err(|e| WristKeyError::Config(format!("parse config file: {}", e)))
    }
    pub fn to_file(&self, path: &std::path::Path) -> Result<()> {
        let contents = toml::to_string_pretty(self).map_err(|e| WristKeyError::Config(format!("serialize config: {}", e)))?;
        std::fs::write(path, contents).map_err(|e| WristKeyError::Config(format!("write config file: {}", e)))
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
    fn default() -> Self { Self::new() }
}
impl MockPlatformSecurity {
    pub fn new() -> Self { Self { locked: Arc::new(RwLock::new(false)) } }
}

#[async_trait]
impl PlatformSecurity for MockPlatformSecurity {
    async fn lock_screen(&self) -> Result<()> { *self.locked.write().await = true; info!("MOCK: screen locked"); Ok(()) }
    async fn unlock_screen(&self) -> Result<()> { *self.locked.write().await = false; info!("MOCK: screen unlocked"); Ok(()) }
    async fn is_locked(&self) -> Result<bool> { Ok(*self.locked.read().await) }
    async fn register_as_authenticator(&self) -> Result<()> { Ok(()) }
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
    pub fn is_authenticated(&self) -> bool { matches!(self, SessionState::Authenticated { .. }) }
    pub fn device_id(&self) -> Option<Uuid> {
        match self { SessionState::Authenticated { device_id, .. } => Some(*device_id), _ => None }
    }
    pub fn device_count(&self) -> usize { 0 }
    pub fn device_name(&self) -> Option<String> { None }
}

pub struct SessionManager {
    crypto: Arc<dyn CryptoEngine>,
    storage: Arc<dyn Storage>,
    state: Arc<RwLock<SessionState>>,
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        Self { crypto: Arc::clone(&self.crypto), storage: Arc::clone(&self.storage), state: Arc::clone(&self.state) }
    }
}

impl SessionManager {
    pub fn new(crypto: Arc<dyn CryptoEngine>, storage: Arc<dyn Storage>) -> Self {
        Self { crypto, storage, state: Arc::new(RwLock::new(SessionState::Disconnected)) }
    }
    pub async fn state(&self) -> SessionState { self.state.read().await.clone() }
    pub async fn load_device(&self, device_id: Uuid) -> Result<Option<PairedDevice>> { self.storage.load_device(device_id).await }
    pub async fn list_paired_devices(&self) -> Result<Vec<PairedDevice>> { self.storage.list_devices().await }
    pub async fn load_config(&self) -> Result<Config> { self.storage.load_config().await }
    pub async fn begin_pairing(&self) -> Result<Challenge> {
        let challenge = Challenge::generate();
        *self.state.write().await = SessionState::Pairing { challenge: challenge.clone(), started_at: Utc::now() };
        info!("pairing started"); Ok(challenge)
    }
    pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, device_id: Option<Vec<u8>>, response: &Response, baseline_rssi: i16, address: String) -> Result<PairedDevice> {
        let state = self.state.read().await.clone();
        let challenge = match state { SessionState::Pairing { challenge, .. } => challenge, other => return Err(WristKeyError::Session(format!("expected Pairing, got {:?}", other))) };
        self.crypto.verify(&public_key, &challenge.to_bytes(), &response.signature).await?;
        if !response.user_present { return Err(WristKeyError::Protocol("user presence required".into())); }
        let device = PairedDevice { id: Uuid::new_v4(), name: device_name, public_key, device_id, paired_at: Utc::now(), baseline_rssi, address, windows_password: None };
        self.storage.save_device(&device).await?;
        *self.state.write().await = SessionState::Authenticated { device_id: device.id, last_rssi: baseline_rssi, last_seen: Utc::now() };
        info!("pairing completed for {}", device.id); Ok(device)
    }
    pub async fn begin_unlock(&self, device_id: Uuid) -> Result<Challenge> {
        let challenge = Challenge::generate();
        *self.state.write().await = SessionState::Verifying { device_id, challenge: challenge.clone(), started_at: Utc::now() };
        info!("unlock verification started for {}", device_id); Ok(challenge)
    }
    pub async fn verify_unlock(&self, response: &Response) -> Result<()> {
        let (device_id, challenge) = match self.state.read().await.clone() {
            SessionState::Verifying { device_id, challenge, .. } => (device_id, challenge),
            other => return Err(WristKeyError::Session(format!("expected Verifying, got {:?}", other))),
        };
        let device = self.storage.load_device(device_id).await?.ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        self.crypto.verify(&device.public_key, &challenge.to_bytes(), &response.signature).await?;
        if !response.user_present { return Err(WristKeyError::Protocol("user presence required".into())); }
        *self.state.write().await = SessionState::Authenticated { device_id, last_rssi: device.baseline_rssi, last_seen: Utc::now() };
        info!("unlock verified for {}", device_id); Ok(())
    }
    pub async fn set_device_password(&self, device_id: Uuid, encrypted: Vec<u8>) -> Result<()> {
        let mut device = self.storage.load_device(device_id).await?.ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        device.windows_password = Some(encrypted); self.storage.save_device(&device).await?;
        info!("stored encrypted password for device {}", device_id); Ok(())
    }
    pub async fn get_device_password(&self, device_id: Uuid) -> Result<Option<Vec<u8>>> {
        let device = self.storage.load_device(device_id).await?.ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        Ok(device.windows_password.clone())
    }
    pub async fn pair_device(&self, id: &str, name: &str, rssi: i32, address: &str) -> Result<()> {
        let (priv_key, pub_key) = self.crypto.generate_keypair().await?;
        let challenge = self.begin_pairing().await?;
        let sig = self.crypto.sign(&priv_key, &challenge.to_bytes()).await?;
        let response = Response { signature: sig, user_present: true, timestamp: Utc::now() };
        let _device = self.complete_pairing(name.to_string(), pub_key, Some(id.as_bytes().to_vec()), &response, rssi as i16, address.to_string()).await?;
        Ok(())
    }
    pub async fn forget_device(&self, id: &str) -> Result<()> {
        let uuid = Uuid::parse_str(id).map_err(|e| WristKeyError::Storage(format!("invalid uuid: {}", e)))?;
        self.storage.delete_device(uuid).await
    }
    pub async fn calibrate_device(&self, id: &str) -> Result<(i32, i32, usize)> {
        let uuid = Uuid::parse_str(id).map_err(|e| WristKeyError::Storage(format!("invalid uuid: {}", e)))?;
        let device = self.storage.load_device(uuid).await?.ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        Ok((device.baseline_rssi as i32, (device.baseline_rssi - 15) as i32, 10))
    }
    pub async fn scan_ble(&self) -> Result<Vec<PeripheralInfo>> { Ok(vec![]) }
    pub async fn update_rssi(&self, rssi: i16) -> Result<bool> {
        let mut state = self.state.write().await;
        match *state {
            SessionState::Authenticated { ref mut last_rssi, ref mut last_seen, device_id } => {
                *last_rssi = rssi; *last_seen = Utc::now();
                let config = self.storage.load_config().await?;
                let device = self.storage.load_device(device_id).await?.ok_or_else(|| WristKeyError::Storage("device missing".into()))?;
                let threshold = device.baseline_rssi - config.rssi_threshold_offset_dbm;
                Ok(rssi < threshold)
            }
            _ => Ok(false),
        }
    }
    pub async fn update_baseline_rssi(&self, device_id: Uuid, baseline_rssi: i16) -> Result<()> {
        let mut device = self.storage.load_device(device_id).await?.ok_or_else(|| WristKeyError::Storage("device not found".into()))?;
        device.baseline_rssi = baseline_rssi; self.storage.save_device(&device).await?;
        info!("Updated baseline_rssi for device {} to {} dBm", device_id, baseline_rssi); Ok(())
    }
    pub async fn disconnect(&self) {
        *self.state.write().await = SessionState::Disconnected;
        info!("session disconnected");
    }
}

pub mod vault {
    use serde::{Deserialize, Serialize};
    use std::path::PathBuf;
    use wristkey_crypto::{decrypt_password, encrypt_password, Key};

    pub const DEVICES_FILE: &str = ".wristkey/devices.json";

    #[derive(Serialize, Deserialize, Clone, Debug)]
    pub struct DeviceRecord {
        pub id: String,
        pub name: String,
        pub user: String,
        pub password_enc: String,
        pub pairing_key_enc: String,
        pub ble_address: String,
        pub created_at: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct DevicesFile {
        pub version: u32,
        pub devices: Vec<DeviceRecord>,
    }

    pub trait KeyProtector: Send + Sync {
        fn protect(&self, key: &[u8]) -> Vec<u8>;
        fn unprotect(&self, data: &[u8]) -> Option<Vec<u8>>;
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct UnlockRequest {
        pub token: String,
        pub timestamp: u64,
        pub user: String,
        pub device_id: String,
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    pub struct UnlockResponse {
        pub token: String,
        pub password_key: Option<String>,
        pub error: Option<String>,
    }

    #[derive(Debug, thiserror::Error)]
    pub enum VaultError {
        #[error("IO error: {0}")]
        Io(#[from] std::io::Error),
        #[error("JSON error: {0}")]
        Json(#[from] serde_json::Error),
        #[error("Base64 error: {0}")]
        Base64(#[from] base64::DecodeError),
        #[error("Crypto error")]
        Crypto,
        #[error("Device not found: {0}")]
        DeviceNotFound(String),
    }

    pub struct DeviceVault<P: KeyProtector> {
        path: PathBuf,
        protector: P,
    }

    impl<P: KeyProtector> DeviceVault<P> {
        pub fn new(path: PathBuf, protector: P) -> Self {
            Self { path, protector }
        }
        pub fn load(&self) -> Result<DevicesFile, VaultError> {
            let content = std::fs::read_to_string(&self.path)?;
            let file: DevicesFile = serde_json::from_str(&content)?;
            Ok(file)
        }
        pub fn save(&self, file: &DevicesFile) -> Result<(), VaultError> {
            let dir = self.path.parent().expect("valid path");
            std::fs::create_dir_all(dir)?;
            let content = serde_json::to_string_pretty(file)?;
            std::fs::write(&self.path, content)?;
            #[cfg(unix)] {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&self.path)?.permissions();
                perms.set_mode(0o600);
                std::fs::set_permissions(&self.path, perms)?;
            }
            Ok(())
        }
        pub fn add_device(&self, id: String, name: String, user: String, password: &str, pairing_key: &Key, ble_address: String) -> Result<(), VaultError> {
            let mut file = self.load().unwrap_or(DevicesFile { version: 2, devices: vec![] });
            let password_enc = encrypt_password(password, pairing_key);
            let pairing_key_enc = base64::encode(self.protector.protect(pairing_key));
            file.devices.retain(|d| d.id != id);
            file.devices.push(DeviceRecord { id, name, user, password_enc, pairing_key_enc, ble_address,
                created_at: format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()),
            });
            self.save(&file)
        }
        pub fn get_device_password(&self, device_id: &str) -> Result<String, VaultError> {
            let file = self.load()?;
            let device = file.devices.into_iter().find(|d| d.id == device_id)
                .ok_or_else(|| VaultError::DeviceNotFound(device_id.to_string()))?;
            let pairing_key_bytes = base64::decode(&device.pairing_key_enc)?;
            let pairing_key_raw = self.protector.unprotect(&pairing_key_bytes).ok_or(VaultError::Crypto)?;
            let pairing_key: Key = pairing_key_raw.as_slice().try_into().map_err(|_| VaultError::Crypto)?;
            decrypt_password(&device.password_enc, &pairing_key).ok_or(VaultError::Crypto)
        }
        pub fn list_devices(&self) -> Result<Vec<DeviceRecord>, VaultError> {
            let file = self.load()?; Ok(file.devices)
        }
        pub fn remove_device(&self, device_id: &str) -> Result<(), VaultError> {
            let mut file = self.load()?; file.devices.retain(|d| d.id != device_id); self.save(&file)
        }
        // NEW METHODS
        pub fn ensure_device(&self, id: String, name: String, user: String, ble_address: String) -> Result<Key, VaultError> {
            let mut file = self.load().unwrap_or(DevicesFile { version: 2, devices: vec![] });
            if let Some(existing) = file.devices.iter().find(|d| d.id == id) {
                let pairing_key_enc = base64::decode(&existing.pairing_key_enc)?;
                let pairing_key_raw = self.protector.unprotect(&pairing_key_enc).ok_or(VaultError::Crypto)?;
                let pairing_key: Key = pairing_key_raw.as_slice().try_into().map_err(|_| VaultError::Crypto)?;
                return Ok(pairing_key);
            }
            let pairing_key = wristkey_crypto::generate_key();
            let password_enc = encrypt_password("", &pairing_key);
            let pairing_key_enc = base64::encode(self.protector.protect(&pairing_key));
            let created_at = format!("{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs());
            file.devices.push(DeviceRecord { id, name, user, password_enc, pairing_key_enc, ble_address, created_at });
            self.save(&file)?;
            Ok(pairing_key)
        }
        pub fn set_password(&self, device_id: &str, password: &str) -> Result<(), VaultError> {
            let mut file = self.load()?;
            let device = file.devices.iter_mut().find(|d| d.id == device_id)
                .ok_or_else(|| VaultError::DeviceNotFound(device_id.to_string()))?;
            let pairing_key_enc = base64::decode(&device.pairing_key_enc)?;
            let pairing_key_raw = self.protector.unprotect(&pairing_key_enc).ok_or(VaultError::Crypto)?;
            let pairing_key: Key = pairing_key_raw.as_slice().try_into().map_err(|_| VaultError::Crypto)?;
            device.password_enc = encrypt_password(password, &pairing_key);
            self.save(&file)
        }
        pub fn get_pairing_key(&self, device_id: &str) -> Result<Key, VaultError> {
            let file = self.load()?;
            let device = file.devices.into_iter().find(|d| d.id == device_id)
                .ok_or_else(|| VaultError::DeviceNotFound(device_id.to_string()))?;
            let pairing_key_enc = base64::decode(&device.pairing_key_enc)?;
            let pairing_key_raw = self.protector.unprotect(&pairing_key_enc).ok_or(VaultError::Crypto)?;
            let pairing_key: Key = pairing_key_raw.as_slice().try_into().map_err(|_| VaultError::Crypto)?;
            Ok(pairing_key)
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeripheralInfo {
    pub id: String,
    pub name: Option<String>,
    pub pin: Option<String>,
    pub device_id: Option<String>,
    pub rssi: Option<i16>,
    pub service_uuids: Vec<Uuid>,
    pub raw_manufacturer_data: Option<Vec<u8>>,
}

pub struct RssiSmoother;
impl RssiSmoother {
    pub fn new(_baseline: i16) -> Self { Self }
    pub fn update(&mut self, _rssi: i16) -> (bool, bool) { (true, true) }
    pub fn current_rssi(&self) -> Option<i16> { None }
}
