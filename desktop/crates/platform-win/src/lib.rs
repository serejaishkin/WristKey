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
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        let inproc = hklm.open_subkey(format!(
            r"SOFTWARE\Classes\CLSID\{}\InprocServer32",
            Self::CP_CLSID
        ));
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
        use winreg::enums::HKEY_LOCAL_MACHINE;
        let clean_path = dll_path.trim_start_matches(r"\\?\");
        if !std::path::Path::new(clean_path).exists() {
            return Err(format!("Credential Provider DLL not found: {}", clean_path));
        }

        // Copy DLL to System32 for reliable COM loading
        let system32 = std::env::var("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\Windows"))
            .join("System32");
        let dest_dll = system32.join("WristKeyCredentialProvider.dll");
        std::fs::copy(clean_path, &dest_dll)
            .map_err(|e| format!("Failed to copy DLL to System32: {} -- run as Administrator", e))?;

        let dll_filename = dest_dll.file_name().unwrap().to_string_lossy().to_string();

        let write_err = |e: std::io::Error| {
            format!("registry write failed ({}) -- run as Administrator", e)
        };

        let hkcr = winreg::RegKey::predef(winreg::enums::HKEY_CLASSES_ROOT);

        // CLSID registration — for .NET COM servers, InprocServer32 must point
        // to mscoree.dll (the CLR shim), NOT the assembly itself.
        let clsid_path = format!(r"CLSID\{}", Self::CP_CLSID);
        let (clsid_key, _) = hkcr.create_subkey(&clsid_path).map_err(write_err)?;
        clsid_key.set_value("", &Self::CP_NAME).map_err(write_err)?;

        // .NET COM interop keys — mscoree.dll reads these to load the assembly
        let assembly_name = "WristKeyCredentialProvider, Version=1.0.0.0, Culture=neutral, PublicKeyToken=null";
        let class_name = "WristKeyCredentialProvider.WristKeyCredentialProvider";
        clsid_key.set_value("Assembly", &assembly_name).map_err(write_err)?;
        clsid_key.set_value("Class", &class_name).map_err(write_err)?;
        clsid_key.set_value("RuntimeVersion", &"v4.0.30319").map_err(write_err)?;
        clsid_key.set_value("CodeBase", &format!("file:///{}", dest_dll.display())).map_err(write_err)?;

        // InprocServer32 → mscoree.dll (CLR hosting shim)
        let (inproc, _) = clsid_key.create_subkey("InprocServer32").map_err(write_err)?;
        inproc.set_value("", &"mscoree.dll").map_err(write_err)?;
        inproc.set_value("ThreadingModel", &"Apartment").map_err(write_err)?;

        // Credential Provider registration
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        let cp_path = format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            Self::CP_CLSID
        );
        let (cp_key, _) = hklm.create_subkey(&cp_path).map_err(write_err)?;
        cp_key.set_value("", &Self::CP_NAME).map_err(write_err)?;

        // Set UserTile for default CP
        if let Ok(username) = std::env::var("USERNAME") {
            if let Ok(domain) = std::env::var("USERDOMAIN") {
                use std::process::Command;
                let output = Command::new("wmic")
                    .args(["useraccount", "where", &format!("name='{}'and domain='{}'", username, domain), "get", "sid"])
                    .output();
                if let Ok(out) = output {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    for line in stdout.lines() {
                        let sid = line.trim();
                        if sid.starts_with("S-1-") {
                            let tile_path = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\LogonUI\UserTile";
                            if let Ok((key, _)) = hklm.create_subkey(tile_path) {
                                let _ = key.set_value(sid, &Self::CP_CLSID);
                            }
                            break;
                        }
                    }
                }
            }
        }

        tracing::info!("Credential Provider registered: CLSID={} dll={}", Self::CP_CLSID, dest_dll.display());
        Ok(())
    }

    pub fn unregister_credential_provider() -> std::result::Result<(), String> {
        use winreg::enums::{HKEY_LOCAL_MACHINE, HKEY_CLASSES_ROOT, KEY_READ, KEY_WRITE};
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
        // Delete COM registration from HKCR
        let hkcr = winreg::RegKey::predef(HKEY_CLASSES_ROOT);
        delete_tree(&hkcr, &format!(r"CLSID\{}", Self::CP_CLSID))
            .map_err(|e| format!("failed to delete COM registration: {}", e))?;
        // Delete Credential Provider registration
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        delete_tree(
            &hklm,
            &format!(
                r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
                Self::CP_CLSID
            ),
        )
        .map_err(|e| format!("failed to delete provider registration: {}", e))?;
        // Delete UserTile entry for current user
        if let Ok(tile_key) = hklm.open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\LogonUI\UserTile",
            KEY_READ | KEY_WRITE,
        ) {
            if let Ok(username) = std::env::var("USERNAME") {
                if let Ok(domain) = std::env::var("USERDOMAIN") {
                    use std::process::Command;
                    let output = Command::new("wmic")
                        .args(["useraccount", "where", &format!("name='{}'and domain='{}'", username, domain), "get", "sid"])
                        .output();
                    if let Ok(out) = output {
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        for line in stdout.lines() {
                            let sid = line.trim();
                            if sid.starts_with("S-1-") {
                                let _ = tile_key.delete_value(sid);
                                break;
                            }
                        }
                    }
                }
            }
        }
        // Remove DLL from System32
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
