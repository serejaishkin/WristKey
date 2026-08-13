//! Windows platform security implementation
//! 
//! Features:
//! - LockWorkStation via user32.dll
//! - DPAPI password encryption (CryptProtectData)
//! - Named pipe server for Credential Provider communication

use async_trait::async_trait;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn, error};
use wristkey_core::{PlatformSecurity, PasswordVault, Result, WristKeyError};
use std::sync::Arc;
use tokio::sync::Mutex;

#[link(name = "user32")]
extern "system" {
    fn LockWorkStation() -> i32;
}

pub struct WindowsSecurity {
    pipe_server: Arc<Mutex<Option<UnlockPipeServer>>>,
}

impl WindowsSecurity {
    pub fn new() -> Self {
        Self {
            pipe_server: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start_pipe_server(&self, password_provider: Arc<dyn PasswordProvider>) -> Result<()> {
        let server = UnlockPipeServer::new(password_provider)?;
        server.run().await;
        *self.pipe_server.lock().await = Some(server);
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
        info!("Workstation locked");
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        // Windows unlock is handled by Credential Provider, not directly
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        // TODO: detect locked state via OpenInputDesktop or WTS API
        warn!("is_locked is a placeholder on Windows — returns false");
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        // TODO: register Windows Credential Provider DLL
        warn!("register_as_authenticator is a placeholder — CP not yet registered");
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
            ).map_err(|e| WristKeyError::Platform(format!("CryptProtectData: {:?}", e)))?;

            let slice = std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize);
            Ok(slice.to_vec())
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
            ).map_err(|e| WristKeyError::Platform(format!("CryptUnprotectData: {:?}", e)))?;

            let slice = std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize);
            String::from_utf8(slice.to_vec())
                .map_err(|e| WristKeyError::Platform(format!("invalid UTF-8: {}", e)))
        }
    }
}

/// Trait for providing password to Credential Provider
#[async_trait]
pub trait PasswordProvider: Send + Sync {
    async fn get_password(&self, device_id: &str) -> Result<Option<String>>;
}

/// Named pipe server that listens for Credential Provider requests
pub struct UnlockPipeServer;

impl UnlockPipeServer {
    pub fn new(_password_provider: Arc<dyn PasswordProvider>) -> Result<Self> {
        // TODO: implement named pipe server logic
        Ok(Self)
    }

    pub async fn run(&self) {
        // TODO: implement pipe listening loop
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
    }
}
