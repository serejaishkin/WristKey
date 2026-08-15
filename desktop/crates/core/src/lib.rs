use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use wristkey_crypto::{decrypt_password, encrypt_password, Key};

pub const DEVICES_FILE: &str = ".wristkey/devices.json";

/// Single paired device record. Stored in JSON.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PairedDevice {
    pub id: String,
    pub name: String,
    pub user: String,
    /// AES-GCM(password, pairingKey) → base64
    pub password_enc: String,
    /// Platform-protected pairingKey → base64
    /// Windows: DPAPI blob. macOS: empty (stored in Keychain). Linux: raw key (file perms).
    pub pairing_key_enc: String,
    pub ble_address: String,
    pub created_at: String,
}

/// Root JSON structure.
#[derive(Serialize, Deserialize, Debug)]
pub struct DevicesFile {
    pub version: u32,
    pub devices: Vec<PairedDevice>,
}

/// Platform-specific protector for the 32-byte pairingKey.
pub trait KeyProtector: Send + Sync {
    fn protect(&self, key: &[u8]) -> Vec<u8>;
    fn unprotect(&self, data: &[u8]) -> Option<Vec<u8>>;
}

/// BLE unlock request sent from Desktop → Watch.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UnlockRequest {
    /// 64-char random token (replay protection)
    pub token: String,
    /// Unix timestamp ms
    pub timestamp: u64,
    /// Windows: DOMAIN\user, Linux/macOS: username
    pub user: String,
    /// Device ID from JSON
    pub device_id: String,
}

/// BLE unlock response sent from Watch → Desktop.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UnlockResponse {
    pub token: String,
    /// base64(passwordKey) — 32-byte AES key to decrypt passwordEnc
    pub password_key: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
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

/// Unified device storage. Platform protection is injected via `KeyProtector`.
pub struct Storage<P: KeyProtector> {
    path: PathBuf,
    protector: P,
}

impl<P: KeyProtector> Storage<P> {
    pub fn new(path: PathBuf, protector: P) -> Self {
        Self { path, protector }
    }

    pub fn load(&self) -> Result<DevicesFile, StorageError> {
        let content = std::fs::read_to_string(&self.path)?;
        let file: DevicesFile = serde_json::from_str(&content)?;
        Ok(file)
    }

    pub fn save(&self, file: &DevicesFile) -> Result<(), StorageError> {
        let dir = self.path.parent().expect("valid path");
        std::fs::create_dir_all(dir)?;
        let content = serde_json::to_string_pretty(file)?;
        std::fs::write(&self.path, content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&self.path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&self.path, perms)?;
        }
        Ok(())
    }

    /// Add or replace a device. `pairing_key` must be the raw 32-byte key.
    pub fn add_device(
        &self,
        id: String,
        name: String,
        user: String,
        password: &str,
        pairing_key: &Key,
        ble_address: String,
    ) -> Result<(), StorageError> {
        let mut file = self.load().unwrap_or(DevicesFile {
            version: 2,
            devices: vec![],
        });

        let password_enc = encrypt_password(password, pairing_key);
        use base64::{Engine as _, engine::general_purpose};
        let pairing_key_enc = general_purpose::STANDARD.encode(self.protector.protect(pairing_key));

        file.devices.retain(|d| d.id != id);
        file.devices.push(PairedDevice {
            id,
            name,
            user,
            password_enc,
            pairing_key_enc,
            ble_address,
            created_at: format!("{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()),
        });

        self.save(&file)
    }

    /// Retrieve decrypted password for a device.
    pub fn get_device_password(&self, device_id: &str) -> Result<String, StorageError> {
        let file = self.load()?;
        let device = file
            .devices
            .into_iter()
            .find(|d| d.id == device_id)
            .ok_or_else(|| StorageError::DeviceNotFound(device_id.to_string()))?;

        use base64::{Engine as _, engine::general_purpose};
        let pairing_key_bytes = general_purpose::STANDARD.decode(&device.pairing_key_enc)?;
        let pairing_key_raw = self
            .protector
            .unprotect(&pairing_key_bytes)
            .ok_or(StorageError::Crypto)?;
        let pairing_key: Key = pairing_key_raw.as_slice()
            .try_into()
            .map_err(|_| StorageError::Crypto)?;

        decrypt_password(&device.password_enc, &pairing_key)
            .ok_or(StorageError::Crypto)
    }

    pub fn list_devices(&self) -> Result<Vec<PairedDevice>, StorageError> {
        let file = self.load()?;
        Ok(file.devices)
    }

    pub fn remove_device(&self, device_id: &str) -> Result<(), StorageError> {
        let mut file = self.load()?;
        file.devices.retain(|d| d.id != device_id);
        self.save(&file)
    }
}
