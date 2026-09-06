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
        self.vault.get_device_password(device_id).map_err(|e| e.to_string())
    }

    pub async fn encrypt_password(&self, password: &str) -> std::result::Result<Vec<u8>, String> {
        let pairing_key = generate_key();
        Ok(wristkey_crypto::encrypt(password.as_bytes(), &pairing_key))
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

    // The production provider is the native x64 V2 implementation under
    // windows-credential-provider/. The old managed C# provider used another
    // CLSID and could be registered successfully while still not producing a
    // Windows 10/11 Sign-in options tile.
    const CP_CLSID: &'static str = "{7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}";
    const CP_NAME: &'static str = "WristKey";
    const LEGACY_CP_CLSID: &'static str = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";

    pub fn is_credential_provider_registered() -> bool {
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        let inproc = hklm.open_subkey(format!(
            r"SOFTWARE\Classes\CLSID\{}\InprocServer32",
            Self::CP_CLSID
        ));
        let registered = match inproc {
            Ok(key) => key
                .get_value::<String, _>("")
                .map(|path| std::path::Path::new(&path).exists())
                .unwrap_or(false),
            Err(_) => false,
        };
        registered && hklm.open_subkey(format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            Self::CP_CLSID
        )).is_ok()
    }

    pub fn storage_type_description() -> &'static str {
        "DPAPI (Windows Data Protection)"
    }

    pub async fn ensure_dll_extracted() -> std::result::Result<String, String> {
        Err("Native Credential Provider DLL is not bundled in this development build. Build/download windows-credential-provider x64 first.".to_string())
    }

    pub fn register_credential_provider(dll_path: &str) -> std::result::Result<(), String> {
        let clean_path = dll_path.trim_start_matches(r"\\?\");
        let source = std::path::Path::new(clean_path);
        if !source.exists() {
            return Err(format!("Credential Provider DLL not found: {}", clean_path));
        }

        let system32 = std::env::var("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\Windows"))
            .join("System32");
        let dest_dll = system32.join("WristKeyCredentialProvider.dll");
        std::fs::copy(source, &dest_dll)
            .map_err(|e| format!("Failed to copy native Credential Provider DLL to System32: {} -- run as Administrator", e))?;

        // Remove the legacy managed registration so Windows cannot resolve the
        // old provider while the native V2 provider is being installed.
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
        let hkcr = winreg::RegKey::predef(HKEY_CLASSES_ROOT);
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        fn delete_tree(hive: &winreg::RegKey, path: &str) {
            if let Ok(key) = hive.open_subkey_with_flags(path, KEY_READ | KEY_WRITE) {
                let subkeys: Vec<String> = key.enum_keys().flatten().collect();
                for sub in subkeys {
                    delete_tree(hive, &format!(r"{}\{}", path, sub));
                }
            }
            let _ = hive.delete_subkey(path);
        }
        delete_tree(&hkcr, &format!(r"CLSID\{}", Self::LEGACY_CP_CLSID));
        delete_tree(&hklm, &format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            Self::LEGACY_CP_CLSID
        ));

        let regsvr32 = system32.join("regsvr32.exe");
        let output = std::process::Command::new(&regsvr32)
            .args(["/s", dest_dll.to_string_lossy().as_ref()])
            .output()
            .map_err(|e| format!("Failed to start regsvr32: {}", e))?;
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(format!("Native Credential Provider registration failed (regsvr32 exit {}). Run WristKey as Administrator and verify the DLL is x64.", code));
        }

        tracing::info!("Native V2 Credential Provider registered: CLSID={} dll={}", Self::CP_CLSID, dest_dll.display());
        Ok(())
    }

    pub fn unregister_credential_provider() -> std::result::Result<(), String> {
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
        fn delete_tree(hive: &winreg::RegKey, path: &str) {
            if let Ok(key) = hive.open_subkey_with_flags(path, KEY_READ | KEY_WRITE) {
                let subkeys: Vec<String> = key.enum_keys().flatten().collect();
                for sub in subkeys {
                    delete_tree(hive, &format!(r"{}\{}", path, sub));
                }
            }
            let _ = hive.delete_subkey(path);
        }

        let hkcr = winreg::RegKey::predef(HKEY_CLASSES_ROOT);
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        delete_tree(&hkcr, &format!(r"CLSID\{}", Self::CP_CLSID));
        delete_tree(&hklm, &format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            Self::CP_CLSID
        ));
        delete_tree(&hkcr, &format!(r"CLSID\{}", Self::LEGACY_CP_CLSID));
        delete_tree(&hklm, &format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            Self::LEGACY_CP_CLSID
        ));

        let system32 = std::env::var("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\Windows"))
            .join("System32")
            .join("WristKeyCredentialProvider.dll");
        let _ = std::fs::remove_file(system32);
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
