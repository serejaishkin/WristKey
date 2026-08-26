use wristkey_core::vault::{DeviceVault, KeyProtector};
use wristkey_core::{PlatformSecurity, Result, SessionManager};
use wristkey_crypto::generate_key;
use std::sync::Arc;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};
use windows::Win32::Foundation::{HLOCAL, LocalFree};

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

    // The CLSID must match the Guid attribute in the C# Credential Provider
    // (desktop/crates/credential-provider/WristKeyCredentialProvider.cs) and
    // register.ps1. All three sides reference the same provider class.
    const CP_CLSID: &'static str = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";
    const CP_NAME: &'static str = "WristKey Credential Provider";

    pub fn is_credential_provider_registered() -> bool {
        use winreg::enums::HKEY_CLASSES_ROOT;
        let hkcr = winreg::RegKey::predef(HKEY_CLASSES_ROOT);
        let inproc = hkcr.open_subkey(format!(r"CLSID\{}\InprocServer32", Self::CP_CLSID));
        match inproc {
            Ok(key) => key
                .get_value::<String, _>("")
                .map(|path| std::path::Path::new(&path).exists())
                .unwrap_or(false),
            Err(_) => false,
        }
    }

    pub fn storage_type_description() -> &'static str {
        "DPAPI (Windows Data Protection)"
    }

    pub async fn ensure_dll_extracted() -> std::result::Result<String, String> {
        Err("DLL extraction not yet implemented".to_string())
    }

    pub fn register_credential_provider(dll_path: &str) -> std::result::Result<(), String> {
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE};
        if !std::path::Path::new(dll_path).exists() {
            return Err(format!("Credential Provider DLL not found: {}", dll_path));
        }
        // Writing HKCR/HKLM requires Administrator. Surface a clear error
        // instead of silently succeeding when elevation is missing.
        let write_err = |e: std::io::Error| {
            format!(
                "registry write failed ({}) -- run the app once as Administrator",
                e
            )
        };
        let hkcr = winreg::RegKey::predef(HKEY_CLASSES_ROOT);
        let (clsid_key, _) =
            hkcr.create_subkey(format!(r"CLSID\{}", Self::CP_CLSID)).map_err(write_err)?;
        clsid_key.set_value("", &Self::CP_NAME).map_err(write_err)?;
        let (inproc, _) = clsid_key.create_subkey("InprocServer32").map_err(write_err)?;
        inproc.set_value("", &dll_path).map_err(write_err)?;
        inproc.set_value("ThreadingModel", &"Apartment").map_err(write_err)?;

        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        let (cp_key, _) = hklm
            .create_subkey(format!(
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
                Self::CP_CLSID
            ))
            .map_err(write_err)?;
        cp_key.set_value("", &Self::CP_NAME).map_err(write_err)?;
        tracing::info!("Credential Provider registered: CLSID={} dll={}", Self::CP_CLSID, dll_path);
        Ok(())
    }

    pub fn unregister_credential_provider() -> std::result::Result<(), String> {
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
        // winreg 0.52 has no delete_subkey_tree; recurse manually. Collect
        // subkeys before deleting because enumeration invalidates on change.
        fn delete_tree(hive: &winreg::RegKey, path: &str) -> std::io::Result<()> {
            if let Ok(key) = hive.open_subkey_with_flags(path, KEY_READ | KEY_WRITE) {
                let subkeys: Vec<String> = key.enum_keys().flatten().collect();
                for sub in subkeys {
                    let child = format!(r"{}\{}", path, sub);
                    delete_tree(hive, &child)?;
                }
            }
            hive.delete_subkey(path)
        }
        let hkcr = winreg::RegKey::predef(HKEY_CLASSES_ROOT);
        delete_tree(&hkcr, format!(r"CLSID\{}", Self::CP_CLSID).as_str())
            .map_err(|e| format!("failed to delete COM registration: {}", e))?;
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        delete_tree(
            &hklm,
            format!(
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
                Self::CP_CLSID
            )
            .as_str(),
        )
        .map_err(|e| format!("failed to delete provider registration: {}", e))?;
        tracing::info!("Credential Provider unregistered");
        Ok(())
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
