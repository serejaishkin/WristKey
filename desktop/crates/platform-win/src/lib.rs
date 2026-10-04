use wristkey_core::vault::{DeviceVault, KeyProtector};
use wristkey_core::{PlatformSecurity, WristKeyError, Result, SessionManager};
use wristkey_crypto::generate_key;
use std::sync::Arc;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};
use windows::Win32::Foundation::{HLOCAL, LocalFree, HKEY};
use windows::Win32::System::Registry::{RegOpenKeyExW, RegCloseKey, RegQueryValueExW, HKEY_LOCAL_MACHINE, HKEY_CLASSES_ROOT, KEY_READ};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub struct WindowsKeyProtector;

impl KeyProtector for WindowsKeyProtector {
    fn protect(&self, plaintext: &[u8]) -> Vec<u8> {
        unsafe {
            let mut data_in = CRYPT_INTEGER_BLOB {
                cbData: plaintext.len() as u32,
                pbData: plaintext.as_ptr() as *mut u8,
            };
            let mut data_out = CRYPT_INTEGER_BLOB::default();
            CryptProtectData(&mut data_in, None, None, None, None, 0, &mut data_out)
                .expect("CryptProtectData failed");
            let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
            let result = slice.to_vec();
            let _ = LocalFree(HLOCAL(data_out.pbData as *mut std::ffi::c_void));
            result
        }
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Option<Vec<u8>> {
        unsafe {
            let mut data_in = CRYPT_INTEGER_BLOB {
                cbData: ciphertext.len() as u32,
                pbData: ciphertext.as_ptr() as *mut u8,
            };
            let mut data_out = CRYPT_INTEGER_BLOB::default();
            CryptUnprotectData(&mut data_in, None, None, None, None, 0, &mut data_out).ok()?;
            let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
            let result = slice.to_vec();
            let _ = LocalFree(HLOCAL(data_out.pbData as *mut std::ffi::c_void));
            Some(result)
        }
    }
}

pub struct WindowsVault {
    vault: DeviceVault<WindowsKeyProtector>,
}

impl WindowsVault {
    pub fn new() -> Self {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_else(|_| ".".to_string());
        let path = std::path::PathBuf::from(home).join(".wristkey/devices.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        Self {
            vault: DeviceVault::new(path, WindowsKeyProtector),
        }
    }

    pub fn store_password(&self, device_id: &str, password: &str) -> std::result::Result<(), String> {
        let pairing_key = generate_key();
        self.vault
            .add_device(
                device_id.to_string(),
                "WristKey Device".to_string(),
                std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string()),
                password,
                &pairing_key,
                "".to_string(),
            )
            .map_err(|e| e.to_string())
    }

    pub fn retrieve_password(&self, device_id: &str) -> std::result::Result<String, String> {
        self.vault
            .get_device_password(device_id)
            .map_err(|e| e.to_string())
    }

    pub async fn encrypt_password(&self, password: &str) -> std::result::Result<Vec<u8>, String> {
        let pairing_key = generate_key();
        let encrypted = wristkey_crypto::encrypt(password.as_bytes(), &pairing_key);
        Ok(encrypted)
    }

    pub fn ensure_device(&self, device_id: &str, name: &str, ble_address: &str) -> std::result::Result<[u8; 32], String> {
        self.vault.ensure_device(
            device_id.to_string(),
            name.to_string(),
            std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string()),
            ble_address.to_string(),
        ).map_err(|e| e.to_string())
    }

    pub fn set_password(&self, device_id: &str, password: &str) -> std::result::Result<(), String> {
        self.vault.set_password(device_id, password).map_err(|e| e.to_string())
    }

    pub fn get_pairing_key(&self, device_id: &str) -> std::result::Result<[u8; 32], String> {
        self.vault.get_pairing_key(device_id).map_err(|e| e.to_string())
    }
}

pub struct WindowsSecurity {
    session: Option<Arc<SessionManager>>,
}

impl WindowsSecurity {
    pub fn new() -> Self {
        Self { session: None }
    }

    pub fn set_session(&mut self, session: Arc<SessionManager>) {
        self.session = Some(session);
    }

    pub fn start_pipe_server() {}

    pub fn is_credential_provider_registered() -> bool {
        // Check both CLSID registration and Credential Provider registration
        let clsid = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";
        
        // Check CLSID in HKCR
        let clsid_path = format!("CLSID\\{}", clsid);
        if !registry_key_exists(HKEY_CLASSES_ROOT, &clsid_path) {
            return false;
        }
        
        // Check InprocServer32
        let inproc_path = format!("{}\\InprocServer32", clsid_path);
        if !registry_key_exists(HKEY_CLASSES_ROOT, &inproc_path) {
            return false;
        }
        
        // Check Credential Provider registration in HKLM
        let cp_path = format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Authentication\\Credential Providers\\{}", clsid);
        if !registry_key_exists(HKEY_LOCAL_MACHINE, &cp_path) {
            return false;
        }
        
        true
    }

    pub fn storage_type_description() -> &'static str {
        "DPAPI (Windows Data Protection)"
    }

    pub async fn ensure_dll_extracted() -> std::result::Result<String, String> {
        Err("DLL extraction not yet implemented".to_string())
    }

    pub fn register_credential_provider(_dll_path: &str) -> std::result::Result<(), String> {
        Ok(())
    }

    pub fn unregister_credential_provider() -> std::result::Result<(), String> {
        Ok(())
    }
    
    fn registry_key_exists(hkey: HKEY, path: &str) -> bool {
        let wide_path: Vec<u16> = OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();
        let mut key_handle = HKEY::default();
        let result = unsafe {
            RegOpenKeyExW(hkey, wide_path.as_ptr(), 0, KEY_READ, &mut key_handle)
        };
        if result.0 == 0 {
            unsafe { RegCloseKey(key_handle); }
            true
        } else {
            false
        }
    }
}

#[async_trait::async_trait]
impl PlatformSecurity for WindowsSecurity {
    async fn lock_screen(&self) -> Result<()> {
        unsafe {
            let _ = windows::Win32::System::Shutdown::LockWorkStation();
        }
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        Ok(())
    }
}
