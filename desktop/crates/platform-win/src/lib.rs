use wristkey_core::{PlatformSecurity, Result, SessionManager};
use wristkey_ble::{BleAdapter, BtleplugAdapter};
use wristkey_daemon::{Daemon, ConnectionManager};
use wristkey_crypto::generate_key;
use std::sync::Arc;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPT_PROTECT_LOCAL_MACHINE,
};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE};
use winreg::RegKey;

pub struct WindowsKeyProtector;
impl KeyProtector for WindowsKeyProtector {
    fn protect(&self, plaintext: &[u8]) -> Vec<u8> {
        unsafe {
            let mut data_in = CRYPT_INTEGER_BLOB {
                cbData: plaintext.len() as u32,
                pbData: plaintext.as_ptr() as *mut u8,
            };
            let mut data_out = CRYPT_INTEGER_BLOB::default();
            CryptProtectData(&mut data_in, None, None, None, None, CRYPT_PROTECT_LOCAL_MACHINE, &mut data_out)
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
    vault: wristkey_core::vault::DeviceVault<WindowsKeyProtector>,
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
            vault: wristkey_core::vault::DeviceVault::new(path, WindowsKeyProtector),
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
}

// CLSID constants - defined at module level
pub const CP_CLSID: &str = "{7E1B7B8A-4C8B-4C2F-9D8A-8D3A7F2E51A1}";
pub const CP_NAME: &str = "WristKey";
pub const LEGACY_CP_CLSID: &str = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";
pub const SAMPLE_CONTROL_CLSID: &str = "{5FD3D285-0DD9-4362-8855-E0ABAACD4AF6}";

impl WindowsSecurity {
    pub fn is_credential_provider_registered() -> bool {
        let hklm = winreg::RegKey::predef(HKEY_LOCAL_MACHINE);
        let inproc = hklm.open_subkey(format!(
            r"SOFTWARE\Classes\CLSID\{}\InprocServer32",
            CP_CLSID
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
            CP_CLSID
        )).is_ok()
    }

    pub fn relaunch_elevated(args: &[&str]) -> bool {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                tracing::warn!("Cannot resolve executable path for elevation: {}", e);
                return false;
            }
        };
        let to_wide = |s: &std::ffi::OsStr| -> Vec<u16> {
            s.encode_wide().chain(std::iter::once(0)).collect()
        };
        let verb = to_wide(std::ffi::OsStr::new("runas"));
        let file = to_wide(exe.as_os_str());
        let params = to_wide(std::ffi::OsStr::new(&args.join(" ")));

        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(verb.as_ptr()),
                PCWSTR(file.as_ptr()),
                PCWSTR(params.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if (result.0 as isize) <= 32 {
            tracing::warn!(
                "Elevation was cancelled or failed (code {})",
                result.0 as isize
            );
            false
        } else {
            tracing::info!("Requested elevated relaunch: {:?}", args);
            true
        }
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
        delete_tree(&hkcr, &format!(r"CLSID\{}", LEGACY_CP_CLSID));
        delete_tree(&hklm, &format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            LEGACY_CP_CLSID
        ));

        // Same cleanup for a leftover Microsoft sample control-test
        // registration -- see SAMPLE_CONTROL_CLSID. Unregister its DLL
        // through regsvr32 (not just a registry delete) so its own
        // DllUnregisterServer runs too, then remove the file.
        let sample_dll = system32.join("SampleV2CredentialProvider.dll");
        if sample_dll.exists() {
            let regsvr32_path = system32.join("regsvr32.exe");
            let _ = std::process::Command::new(&regsvr32_path)
                .args(["/u", "/s", sample_dll.to_string_lossy().as_ref()])
                .output();
            let _ = std::fs::remove_file(&sample_dll);
        }
        delete_tree(&hkcr, &format!(r"CLSID\{}", SAMPLE_CONTROL_CLSID));
        delete_tree(&hklm, &format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            SAMPLE_CONTROL_CLSID
        ));

        // Tell the DLL's own DllRegisterServer where WristKey.exe lives, so
        // it can install/start the background service itself (regsvr32 is
        // already elevated at this point, so this is the one reliable place
        // to do it, independent of whether the service was ever installed
        // before, whether this app was ever run elevated, or whether anyone
        // has signed in yet).
        if let Ok(exe) = std::env::current_exe() {
            if let Ok((key, _)) = hklm.create_subkey_with_flags("SOFTWARE\\WristKey", KEY_WRITE) {
                let exe_path: String = exe.to_string_lossy().into_owned();
                let _ = key.set_value("ExePath", &exe_path);
                // Same folder the app/installer passes as `--data-dir`: the
                // DLL creates the service with it so the SYSTEM service reads
                // the *user's* paired-device database.
                if let Ok(appdata) = std::env::var("APPDATA") {
                    let data_dir = std::path::Path::new(&appdata).join("WristKey").join("data");
                    let _ = key.set_value("DataDir", &data_dir.to_string_lossy().into_owned());
                }
            }
        }

        let regsvr32 = system32.join("regsvr32.exe");
        let output = std::process::Command::new(&regsvr32)
            .args(["/s", dest_dll.to_string_lossy().as_ref()])
            .output()
            .map_err(|e| format!("Failed to start regsvr32: {}", e))?;
        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(format!("Native Credential Provider registration failed (regsvr32 exit {}). Run WristKey as Administrator and verify the DLL is x64.", code));
        }

        tracing::info!("Native V2 Credential Provider registered: CLSID={} dll={}", CP_CLSID, dest_dll.display());
        Ok(())
    }

    pub fn unregister_credential_provider() -> std::result::Result<(), String> {
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
        delete_tree(&hkcr, &format!(r"CLSID\{}", CP_CLSID));
        delete_tree(&hklm, &format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            CP_CLSID
        ));
        delete_tree(&hkcr, &format!(r"CLSID\{}", LEGACY_CP_CLSID));
        delete_tree(&hklm, &format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            LEGACY_CP_CLSID
        ));
        delete_tree(&hkcr, &format!(r"CLSID\{}", SAMPLE_CONTROL_CLSID));
        delete_tree(&hklm, &format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            SAMPLE_CONTROL_CLSID
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

    /// Full repair of the sign-in screen: remove every WristKey-related CLSID
    /// registration (native, legacy, sample) and re-register the current
    /// native DLL. Use this when duplicate tiles appear on the logon screen.
    pub fn fix_signin_tiles(dll_path: &str) -> std::result::Result<(), String> {
        // First unregister everything we know about.
        Self::unregister_credential_provider()?;

        // Then register the current DLL fresh.
        Self::register_credential_provider(dll_path)
    }

    /// Ensure WristKey starts automatically on Windows login.
    /// Adds the current executable to HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
    pub fn ensure_auto_start() -> Result<(), String> {
        let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
        let run_key = hkcu.open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            winreg::enums::KEY_READ | KEY_WRITE,
        )?;

        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Cannot get exe path: {}", e))?;

        let existing: Option<String> = run_key.get_value("WristKey").unwrap_or(None);
        if existing.is_none() || existing.as_deref() != Some(exe_path.to_str().unwrap_or("")) {
            run_key.set_value("WristKey", &exe_path.to_string_lossy().to_string())
                .map_err(|e| format!("Failed to set autostart: {}", e))?;
            tracing::info!("WristKey added to Windows startup: {:?}", exe_path);
        }

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
