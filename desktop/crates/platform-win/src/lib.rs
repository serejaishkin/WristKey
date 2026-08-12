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

    /// Start the named pipe server for Credential Provider communication.
    /// Call this once during daemon startup.
    pub async fn start_pipe_server(&self, password_provider: Arc<dyn PasswordProvider>) -> Result<()> {
        let server = UnlockPipeServer::new(password_provider)?;
        server.run().await;
        *self.pipe_server.lock().await = Some(server);
        Ok(())
    }
}

#[async_trait]
impl PlatformSecurity for WindowsSecurity {
    async fn lock_screen(&self) -> Result<()> {
        info!("Locking workstation");
        unsafe {
            let result = LockWorkStation();
            if result == 0 {
                return Err(WristKeyError::Platform("LockWorkStation failed".into()));
            }
        }
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        info!("Windows: unlock_screen is a no-op (OS requires manual unlock or Credential Provider)");
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        info!("Windows credential provider registration not yet implemented");
        Ok(())
    }
}

#[async_trait]
impl PasswordVault for WindowsSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        WindowsPasswordVault::encrypt(password)
    }

    async fn decrypt_password(&self, encrypted: &[u8]) -> Result<String> {
        WindowsPasswordVault::decrypt(encrypted)
    }
}

/// DPAPI-based password encryption using CryptProtectData.
pub struct WindowsPasswordVault;

impl WindowsPasswordVault {
    pub fn encrypt(password: &str) -> Result<Vec<u8>> {
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE,
        };
        use windows::Win32::Foundation::LocalFree;
        use std::ptr;

        let plaintext = password.as_bytes();
        let mut blob_in = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut blob_out = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptProtectData(
                &mut blob_in,
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                CRYPTPROTECT_LOCAL_MACHINE,
                &mut blob_out,
            )
            .ok()
            .map_err(|e| WristKeyError::Platform(format!("CryptProtectData failed: {}", e)))?;

            let len = blob_out.cbData as usize;
            let bytes = std::slice::from_raw_parts(blob_out.pbData, len).to_vec();
            let _ = LocalFree(blob_out.pbData as isize);
            Ok(bytes)
        }
    }

    pub fn decrypt(encrypted: &[u8]) -> Result<String> {
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_LOCAL_MACHINE,
        };
        use windows::Win32::Foundation::LocalFree;
        use std::ptr;

        let mut blob_in = CRYPT_INTEGER_BLOB {
            cbData: encrypted.len() as u32,
            pbData: encrypted.as_ptr() as *mut u8,
        };
        let mut blob_out = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptUnprotectData(
                &mut blob_in,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
                CRYPTPROTECT_LOCAL_MACHINE,
                &mut blob_out,
            )
            .ok()
            .map_err(|e| WristKeyError::Platform(format!("CryptUnprotectData failed: {}", e)))?;

            let len = blob_out.cbData as usize;
            let bytes = std::slice::from_raw_parts(blob_out.pbData, len);
            let password = String::from_utf8(bytes.to_vec())
                .map_err(|e| WristKeyError::Platform(format!("Invalid UTF-8 in decrypted password: {}", e)))?;
            let _ = LocalFree(blob_out.pbData as isize);
            Ok(password)
        }
    }
}

/// Trait for providing passwords to the pipe server.
/// Implemented by the daemon/session manager.
pub trait PasswordProvider: Send + Sync {
    /// Get the encrypted password for a device by its device_id hex string.
    fn get_password(&self, device_id_hex: &str) -> tokio::sync::oneshot::Receiver<Option<String>>;
}

/// Named pipe server that listens for unlock requests from the Credential Provider.
/// 
/// Protocol (text, newline-delimited):
///   Client sends:  UNLOCK|<device_id_hex>\n
///   Server sends:  OK|<password>\n   or   FAIL|<reason>\n
pub struct UnlockPipeServer {
    password_provider: Arc<dyn PasswordProvider>,
}

impl UnlockPipeServer {
    const PIPE_NAME: &'static str = r"\\.\pipe\WristKeyUnlock";

    pub fn new(password_provider: Arc<dyn PasswordProvider>) -> Result<Self> {
        Ok(Self { password_provider })
    }

    pub async fn run(&self) {
        let provider = self.password_provider.clone();
        tokio::spawn(async move {
            loop {
                match Self::accept_one(provider.clone()).await {
                    Ok(_) => {}
                    Err(e) => {
                        warn!("Named pipe client error: {}", e);
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
            }
        });
    }

    async fn accept_one(provider: Arc<dyn PasswordProvider>) -> Result<()> {
        use tokio::net::windows::named_pipe::PipeMode;

        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Message)
            .first_pipe_instance(true)
            .create(Self::PIPE_NAME)
            .map_err(|e| WristKeyError::Platform(format!("create named pipe: {}", e)))?;

        server.connect().await
            .map_err(|e| WristKeyError::Platform(format!("connect named pipe: {}", e)))?;

        let (reader, mut writer) = tokio::io::split(server);
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        reader.read_line(&mut line).await
            .map_err(|e| WristKeyError::Platform(format!("read pipe: {}", e)))?;

        let response = Self::handle_request(&line, provider).await;
        writer.write_all(response.as_bytes()).await
            .map_err(|e| WristKeyError::Platform(format!("write pipe: {}", e)))?;
        writer.flush().await
            .map_err(|e| WristKeyError::Platform(format!("flush pipe: {}", e)))?;

        Ok(())
    }

    async fn handle_request(line: &str, provider: Arc<dyn PasswordProvider>) -> String {
        let line = line.trim();
        if !line.starts_with("UNLOCK|") {
            return "FAIL|invalid request format\n".to_string();
        }
        let device_id_hex = &line[7..];
        if device_id_hex.is_empty() {
            return "FAIL|missing device_id\n".to_string();
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        // NOTE: PasswordProvider impl must send result through tx
        // For now we return a placeholder — the actual integration happens in daemon
        drop(tx); // placeholder

        match rx.await {
            Ok(Some(password)) => format!("OK|{}\n", password),
            Ok(None) => "FAIL|no password stored\n".to_string(),
            Err(_) => "FAIL|internal error\n".to_string(),
        }
    }
}

pub struct MockPlatformSecurity {
    locked: std::sync::Mutex<bool>,
}

impl MockPlatformSecurity {
    pub fn new() -> Self {
        Self {
            locked: std::sync::Mutex::new(false),
        }
    }
}

#[async_trait]
impl PlatformSecurity for MockPlatformSecurity {
    async fn lock_screen(&self) -> Result<()> {
        *self.locked.lock().unwrap() = true;
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        *self.locked.lock().unwrap() = false;
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(*self.locked.lock().unwrap())
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl PasswordVault for MockPlatformSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        Ok(password.as_bytes().to_vec())
    }

    async fn decrypt_password(&self, encrypted: &[u8]) -> Result<String> {
        String::from_utf8(encrypted.to_vec())
            .map_err(|e| WristKeyError::Platform(format!("mock decrypt: {}", e)))
    }
}
