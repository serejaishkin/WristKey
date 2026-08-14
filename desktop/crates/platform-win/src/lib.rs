//! Windows platform security implementation
//!
//! Features:
//! - LockWorkStation via user32.dll
//! - DPAPI password encryption (CryptProtectData / CryptUnprotectData)
//! - Named pipe server for Credential Provider communication

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
        // On Windows, direct screen unlock from a service is restricted.
        // Instead, we provide the password via a named pipe to the
        // WristKey Credential Provider, which runs in the Winlogon process
        // and can perform the actual unlock.
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
        // TODO: detect locked state via OpenInputDesktop or WTS API
        warn!("is_locked is a placeholder on Windows — returns false");
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        // TODO: auto-register Credential Provider DLL via registry
        warn!("register_as_authenticator is a placeholder — run register.ps1 as Administrator");
        Ok(())
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
///
/// The Credential Provider (running in Winlogon) connects to this pipe when the user
/// selects the WristKey tile. The daemon writes the decrypted password here after
/// successful BLE challenge-response.
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
