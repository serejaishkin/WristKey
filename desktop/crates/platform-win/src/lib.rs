//! Windows platform security implementation
//!
//! Features:
//! - LockWorkStation via user32.dll
//! - DPAPI password encryption (works with any TPM or software, no key management)
//! - Named pipe server for Credential Provider communication
//! - Auto-registration of Credential Provider in Windows Registry

use async_trait::async_trait;
use tokio::net::windows::named_pipe::ServerOptions;
use tracing::{info, warn, error};
use wristkey_core::{PlatformSecurity, PasswordVault, Result, WristKeyError, SessionManager};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

#[link(name = "user32")]
extern "system" {
    fn LockWorkStation() -> i32;
}

pub const CP_CLSID: &str = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";
pub const CP_NAME: &str = "WristKey Credential Provider";

pub struct WindowsSecurity {
    session: Option<Arc<SessionManager>>,
    pipe_password: Arc<Mutex<Option<String>>>,
    _pipe_handle: Option<std::thread::JoinHandle<()>>,
}

impl WindowsSecurity {
    pub fn new() -> Self {
        let pipe_password = Arc::new(Mutex::new(None));
        let password_clone = pipe_password.clone();

        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("pipe server tokio runtime");
            rt.block_on(async move {
                UnlockPipeServer::new(password_clone).run().await;
            });
        });

        Self {
            session: None,
            pipe_password,
            _pipe_handle: Some(handle),
        }
    }

    pub fn set_session(&mut self, session: Arc<SessionManager>) {
        self.session = Some(session);
    }

    pub async fn set_unlock_password(&self, password: String) {
        *self.pipe_password.lock().await = Some(password);
        info!("Unlock password buffered for Credential Provider");
    }

    pub fn is_credential_provider_registered() -> bool {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;

        let path = format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            CP_CLSID
        );
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(&path).is_ok()
    }

    pub fn register_credential_provider(dll_path: &str) -> Result<()> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE};

        let clsid_path = format!(r"CLSID\{}", CP_CLSID);
        let (clsid_key, _) = RegKey::predef(HKEY_CLASSES_ROOT)
            .create_subkey(&clsid_path)
            .map_err(|e| WristKeyError::Platform(format!("Failed to create CLSID key: {}", e)))?;
        clsid_key.set_value("", &CP_NAME)
            .map_err(|e| WristKeyError::Platform(format!("Failed to set CLSID name: {}", e)))?;

        let (inproc, _) = clsid_key.create_subkey("InprocServer32")
            .map_err(|e| WristKeyError::Platform(format!("Failed to create InprocServer32: {}", e)))?;
        inproc.set_value("", &dll_path)
            .map_err(|e| WristKeyError::Platform(format!("Failed to set DLL path: {}", e)))?;
        inproc.set_value("ThreadingModel", &"Apartment")
            .map_err(|e| WristKeyError::Platform(format!("Failed to set ThreadingModel: {}", e)))?;

        let cp_path = format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            CP_CLSID
        );
        let (cp_key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .create_subkey(&cp_path)
            .map_err(|e| WristKeyError::Platform(format!("Failed to create CP key: {}", e)))?;
        cp_key.set_value("", &CP_NAME)
            .map_err(|e| WristKeyError::Platform(format!("Failed to set CP name: {}", e)))?;

        info!("Credential Provider registered: CLSID={}, DLL={}", CP_CLSID, dll_path);
        Ok(())
    }

    pub fn unregister_credential_provider() -> Result<()> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE};

        let clsid_path = format!(r"CLSID\{}", CP_CLSID);
        let _ = RegKey::predef(HKEY_CLASSES_ROOT).delete_subkey_all(&clsid_path);

        let cp_path = format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            CP_CLSID
        );
        let _ = RegKey::predef(HKEY_LOCAL_MACHINE).delete_subkey_all(&cp_path);

        info!("Credential Provider unregistered: CLSID={}", CP_CLSID);
        Ok(())
    }

    pub async fn ensure_dll_extracted() -> Result<std::path::PathBuf> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let dll_path = exe_dir.join("WristKeyCredentialProvider.dll");

        if dll_path.exists() {
            info!("Credential Provider DLL already exists: {:?}", dll_path);
            return Ok(dll_path);
        }

        let dev_dll = exe_dir
            .parent().and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.join("crates/credential-provider/WristKeyCredentialProvider.dll"))
            .filter(|p| p.exists());

        if let Some(dev_path) = dev_dll {
            info!("Using dev DLL from: {:?}", dev_path);
            std::fs::copy(&dev_path, &dll_path).map_err(|e| {
                WristKeyError::Platform(format!("Failed to copy dev DLL: {}", e))
            })?;
            return Ok(dll_path);
        }

        Err(WristKeyError::Platform(
            "WristKeyCredentialProvider.dll not found. Build it first.".into()
        ))
    }

    pub fn storage_type_description() -> &'static str {
        "Windows Data Protection API (DPAPI)"
    }
}

impl Default for WindowsSecurity {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PlatformSecurity for WindowsSecurity {
    async fn lock_screen(&self) -> Result<()> {
        unsafe {
            if LockWorkStation() == 0 {
                return Err(WristKeyError::Platform("LockWorkStation failed".into()));
            }
        }
        info!("Workstation locked via LockWorkStation");
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        if let Some(ref session) = self.session {
            let state = session.state().await;
            if let Some(device_id) = state.device_id() {
                match session.get_device_password(device_id).await {
                    Ok(Some(encrypted)) => {
                        match self.decrypt_password(&encrypted).await {
                            Ok(password) => {
                                self.set_unlock_password(password).await;
                                info!("Password prepared for Credential Provider via named pipe");
                            }
                            Err(e) => {
                                warn!("Failed to decrypt Windows password: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        warn!("No Windows password stored for device {}. Use 'Set Windows Password' in GUI.", device_id);
                    }
                    Err(e) => {
                        warn!("Failed to retrieve device password: {}", e);
                    }
                }
            } else {
                warn!("unlock_screen: no authenticated device in session");
            }
        } else {
            warn!("unlock_screen: session not set in WindowsSecurity");
        }
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        warn!("is_locked is a placeholder on Windows — returns false");
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        let dll_path = WindowsSecurity::ensure_dll_extracted().await?;
        WindowsSecurity::register_credential_provider(&dll_path.to_string_lossy())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DPAPI PasswordVault Implementation (replaces broken CNG AES)
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl PasswordVault for WindowsSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPT_INTEGER_BLOB,
            CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
        };
        use windows::Win32::Foundation::LocalFree;
        use windows::core::PCWSTR;

        let data = password.as_bytes();
        let mut data_in = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptProtectData(
                &mut data_in,
                PCWSTR::null(),
                None,       // optional entropy
                None,       // reserved
                None,       // prompt struct
                CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
                &mut data_out,
            ).map_err(|e| WristKeyError::Platform(format!("CryptProtectData failed: {:?}", e)))?;
        }

        let encrypted = unsafe {
            std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec()
        };

        unsafe {
            let _ = LocalFree(data_out.pbData as _);
        }

        info!("Password encrypted with DPAPI ({} bytes)", encrypted.len());
        Ok(encrypted)
    }

    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String> {
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPT_INTEGER_BLOB,
            CRYPTPROTECT_UI_FORBIDDEN,
        };
        use windows::Win32::Foundation::LocalFree;

        let mut data_in = CRYPT_INTEGER_BLOB {
            cbData: ciphertext.len() as u32,
            pbData: ciphertext.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptUnprotectData(
                &mut data_in,
                None,       // description
                None,       // optional entropy
                None,       // reserved
                None,       // prompt struct
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut data_out,
            ).map_err(|e| WristKeyError::Platform(format!("CryptUnprotectData failed: {:?}", e)))?;
        }

        let decrypted = unsafe {
            std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec()
        };

        unsafe {
            let _ = LocalFree(data_out.pbData as _);
        }

        let password = String::from_utf8(decrypted)
            .map_err(|e| WristKeyError::Platform(format!("UTF-8 decode: {}", e)))?;

        info!("Password decrypted with DPAPI ({} chars)", password.len());
        Ok(password)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Named pipe server for Credential Provider
// ─────────────────────────────────────────────────────────────────────────────

struct UnlockPipeServer {
    password: Arc<Mutex<Option<String>>>,
}

impl UnlockPipeServer {
    fn new(password: Arc<Mutex<Option<String>>>) -> Self {
        Self { password }
    }

    async fn run(&self) {
        loop {
            let server = ServerOptions::new()
                .pipe_mode(tokio::net::windows::named_pipe::PipeMode::Byte)
                .create(r"\\.\pipe\WristKeyUnlock");

            match server {
                Ok(mut server) => {
                    info!("Named pipe server waiting for Credential Provider...");
                    match server.connect().await {
                        Ok(()) => {
                            info!("Credential Provider connected to named pipe");
                            if let Some(password) = self.password.lock().await.take() {
                                use tokio::io::AsyncWriteExt;
                                if let Err(e) = server.write_all(password.as_bytes()).await {
                                    error!("Failed to write password to pipe: {}", e);
                                } else {
                                    info!("Password sent to Credential Provider ({} bytes)", password.len());
                                }
                                if let Err(e) = server.write_all(b"\n").await {
                                    error!("Failed to write newline: {}", e);
                                }
                                if let Err(e) = server.flush().await {
                                    error!("Failed to flush pipe: {}", e);
                                }
                            } else {
                                warn!("No password available for Credential Provider");
                            }
                        }
                        Err(e) => {
                            error!("Pipe connection error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to create named pipe: {}", e);
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}
