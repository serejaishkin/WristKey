//! Windows platform security implementation
//!
//! Features:
//! - LockWorkStation via user32.dll
//! - DPAPI password encryption (CryptProtectData / CryptUnprotectData)
//! - Named pipe server for Credential Provider communication
//! - Auto-registration of Credential Provider in Windows Registry

use async_trait::async_trait;
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::io::AsyncWriteExt;
use tracing::{info, warn, error};
use wristkey_core::{PlatformSecurity, PasswordVault, Result, WristKeyError, SessionManager};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

#[link(name = "user32")]
extern "system" {
    fn LockWorkStation() -> i32;
}

/// CLSID for WristKey Credential Provider (must match C# GUID)
pub const CP_CLSID: &str = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";
pub const CP_NAME: &str = "WristKey Credential Provider";

pub struct WindowsSecurity {
    session: Option<Arc<SessionManager>>,
    pipe_password: Arc<Mutex<Option<String>>>,
    _pipe_handle: Option<tokio::task::JoinHandle<()>>,
}

impl WindowsSecurity {
    pub fn new() -> Self {
        let pipe_password = Arc::new(Mutex::new(None));
        let password_clone = pipe_password.clone();

        let handle = tokio::spawn(async move {
            UnlockPipeServer::new(password_clone).run().await;
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

    /// Check if Credential Provider is already registered in registry.
    pub fn is_credential_provider_registered() -> bool {
        use winreg::RegKey;
        use winreg::enums::HKEY_LOCAL_MACHINE;

        let path = format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            CP_CLSID
        );
        match RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(&path) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Register Credential Provider DLL in Windows Registry.
    /// Requires Administrator privileges.
    pub fn register_credential_provider(dll_path: &str) -> Result<()> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE};

        // 1. Register COM CLSID
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

        // 2. Register as Credential Provider
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

    /// Unregister Credential Provider.
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
                        warn!(
                            "No Windows password stored for device {}. Use 'Set Windows Password' in GUI.",
                            device_id
                        );
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
        // Auto-detect DLL path: try exe_dir first, then fallback
        let dll_path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("WristKeyCredentialProvider.dll")))
            .filter(|p| p.exists())
            .or_else(|| {
                std::path::PathBuf::from(r"C:\Program Files\WristKey\WristKeyCredentialProvider.dll")
                    .exists()
                    .then(|| std::path::PathBuf::from(r"C:\Program Files\WristKey\WristKeyCredentialProvider.dll"))
            })
            .ok_or_else(|| WristKeyError::Platform(
                "WristKeyCredentialProvider.dll not found. Build and copy it first.".into()
            ))?;

        Self::register_credential_provider(&dll_path.to_string_lossy())
    }
}

#[async_trait]
impl PasswordVault for WindowsSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE,
        };

        let mut data = password.as_bytes().to_vec();
        let mut blob_in = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_mut_ptr(),
        };
        let mut blob_out = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptProtectData(
                &mut blob_in,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_LOCAL_MACHINE,
                &mut blob_out,
            )
            .map_err(|e| WristKeyError::Platform(format!("CryptProtectData: {:?}", e)))?;

            let slice = std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize);
            let result = slice.to_vec();
            let _ = windows::Win32::Security::Cryptography::LocalFree(blob_out.pbData as _);
            Ok(result)
        }
    }

    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String> {
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPT_INTEGER_BLOB,
        };

        let mut data = ciphertext.to_vec();
        let mut blob_in = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_mut_ptr(),
        };
        let mut blob_out = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptUnprotectData(
                &mut blob_in,
                None,
                None,
                None,
                None,
                0,
                &mut blob_out,
            )
            .map_err(|e| WristKeyError::Platform(format!("CryptUnprotectData: {:?}", e)))?;

            let slice = std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize);
            let result = String::from_utf8(slice.to_vec())
                .map_err(|e| WristKeyError::Platform(format!("UTF-8 decode: {}", e)))?;
            let _ = windows::Win32::Security::Cryptography::LocalFree(blob_out.pbData as _);
            Ok(result)
        }
    }
}

/// Named pipe server that serves the unlock password to the Windows Credential Provider.
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
                Ok(server) => {
                    info!("Named pipe server waiting for Credential Provider...");
                    match server.connect().await {
                        Ok(mut connected) => {
                            info!("Credential Provider connected to named pipe");
                            if let Some(password) = self.password.lock().await.take() {
                                if let Err(e) = connected.write_all(password.as_bytes()).await {
                                    error!("Failed to write password to pipe: {}", e);
                                } else {
                                    info!(
                                        "Password sent to Credential Provider ({} bytes)",
                                        password.len()
                                    );
                                }
                                if let Err(e) = connected.write_all(b"\n").await {
                                    error!("Failed to write newline: {}", e);
                                }
                                if let Err(e) = connected.flush().await {
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
