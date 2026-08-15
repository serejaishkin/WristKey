//! Windows platform security implementation
//!
//! Features:
//! - LockWorkStation via user32.dll
//! - TPM 2.0 + CNG password encryption (NCryptProtectSecret / NCryptUnprotectSecret)
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

/// CLSID for WristKey Credential Provider (must match C# GUID)
pub const CP_CLSID: &str = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";
pub const CP_NAME: &str = "WristKey Credential Provider";

/// Key name for TPM/CNG storage
const WRISTKEY_KEY_NAME: &str = "WristKeyDevicePassword";

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

    /// Extract/find Credential Provider DLL next to the executable.
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

        // Fallback: try to find DLL in credential-provider folder (dev build)
        let dev_dll = exe_dir
            .parent().and_then(|p| p.parent()) // go up from tauri/src-tauri/target/release
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
            "WristKeyCredentialProvider.dll not found. Build it first: cd crates/credential-provider && csc ...".into()
        ))
    }

    /// Check if TPM 2.0 is available on this system.
    pub fn is_tpm_available() -> bool {
        use windows::Win32::Security::Cryptography::{
            NCryptOpenStorageProvider, NCRYPT_PROV_HANDLE,
        };
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let provider_name: Vec<u16> = OsStr::new("Microsoft Platform Crypto Provider")
            .encode_wide()
            .chain(Some(0))
            .collect();

        let mut handle = NCRYPT_PROV_HANDLE::default();
        unsafe {
            let result = NCryptOpenStorageProvider(
                &mut handle,
                windows::core::PCWSTR(provider_name.as_ptr()),
                0,
            );
            result == ERROR_SUCCESS.0
        }
    }

    /// Get human-readable storage type description.
    pub fn storage_type_description() -> &'static str {
        if Self::is_tpm_available() {
            "TPM 2.0 (Microsoft Platform Crypto Provider)"
        } else {
            "Software (Microsoft Software Key Storage Provider)"
        }
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
        let dll_path = WindowsSecurity::ensure_dll_extracted().await?;
        WindowsSecurity::register_credential_provider(&dll_path.to_string_lossy())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// TPM 2.0 / CNG PasswordVault Implementation
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl PasswordVault for WindowsSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        use windows::Win32::Security::Cryptography::*;
        use windows::Win32::Foundation::{ERROR_SUCCESS, LocalFree, HLOCAL};
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let password_bytes = password.as_bytes();

        // 1. Determine provider: TPM if available, else software
        let (provider_name, key_usage) = if WindowsSecurity::is_tpm_available() {
            info!("Using TPM 2.0 (Microsoft Platform Crypto Provider) for password encryption");
            ("Microsoft Platform Crypto Provider", NCRYPT_ALLOW_DECRYPT_FLAG)
        } else {
            warn!("TPM 2.0 not available, falling back to Software Key Storage Provider");
            ("Microsoft Software Key Storage Provider", NCRYPT_ALLOW_DECRYPT_FLAG)
        };

        let provider_name_wide: Vec<u16> = OsStr::new(provider_name)
            .encode_wide()
            .chain(Some(0))
            .collect();

        // 2. Open storage provider
        let mut prov_handle = NCRYPT_PROV_HANDLE::default();
        unsafe {
            let result = NCryptOpenStorageProvider(
                &mut prov_handle,
                PCWSTR(provider_name_wide.as_ptr()),
                0,
            );
            if result != ERROR_SUCCESS.0 {
                return Err(WristKeyError::Platform(
                    format!("NCryptOpenStorageProvider failed: 0x{:X}", result)
                ));
            }
        }

        // 3. Create or open persistent key
        let key_name_wide: Vec<u16> = OsStr::new(WRISTKEY_KEY_NAME)
            .encode_wide()
            .chain(Some(0))
            .collect();

        let mut key_handle = NCRYPT_KEY_HANDLE::default();
        let key_exists = unsafe {
            NCryptOpenKey(
                prov_handle,
                &mut key_handle,
                PCWSTR(key_name_wide.as_ptr()),
                0,
                0,
            ) == ERROR_SUCCESS.0
        };

        if !key_exists {
            info!("Creating new persistent key in {}", provider_name);
            unsafe {
                let result = NCryptCreatePersistedKey(
                    prov_handle,
                    &mut key_handle,
                    BCRYPT_AES_ALGORITHM,
                    PCWSTR(key_name_wide.as_ptr()),
                    0,
                    0,
                );
                if result != ERROR_SUCCESS.0 {
                    NCryptFreeObject(prov_handle);
                    return Err(WristKeyError::Platform(
                        format!("NCryptCreatePersistedKey failed: 0x{:X}", result)
                    ));
                }

                // Set key length to 256 bits
                let key_length: u32 = 256;
                let result = NCryptSetProperty(
                    key_handle,
                    NCRYPT_LENGTH_PROPERTY,
                    &key_length.to_le_bytes(),
                    0,
                );
                if result != ERROR_SUCCESS.0 {
                    NCryptFreeObject(key_handle);
                    NCryptFreeObject(prov_handle);
                    return Err(WristKeyError::Platform(
                        format!("NCryptSetProperty(LENGTH) failed: 0x{:X}", result)
                    ));
                }

                // Finalize the key
                let result = NCryptFinalizeKey(key_handle, 0);
                if result != ERROR_SUCCESS.0 {
                    NCryptFreeObject(key_handle);
                    NCryptFreeObject(prov_handle);
                    return Err(WristKeyError::Platform(
                        format!("NCryptFinalizeKey failed: 0x{:X}", result)
                    ));
                }
            }
        } else {
            info!("Opened existing key from {}", provider_name);
        }

        // 4. Generate random IV
        let mut iv = [0u8; 12]; // AES-GCM IV
        unsafe {
            let result = BCryptGenRandom(
                None,
                &mut iv,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            );
            if result != ERROR_SUCCESS.0 {
                NCryptFreeObject(key_handle);
                NCryptFreeObject(prov_handle);
                return Err(WristKeyError::Platform(
                    format!("BCryptGenRandom failed: 0x{:X}", result)
                ));
            }
        }

        // 5. Encrypt using NCryptEncrypt (AES-GCM via CNG)
        let mut encrypted_len: u32 = 0;
        let plaintext = password_bytes;

        unsafe {
            // First call to get size
            let result = NCryptEncrypt(
                key_handle,
                Some(plaintext),
                Some(&iv),
                None,
                &mut encrypted_len,
                NCRYPT_PAD_PKCS1_FLAG, // Use PKCS1 padding for compatibility
            );
            if result != ERROR_SUCCESS.0 && result != ERROR_SUCCESS.0 + 1 {
                // ERROR_SUCCESS + 1 = NTE_BUFFER_TOO_SMALL (expected)
            }

            let mut encrypted = vec![0u8; encrypted_len as usize];
            let result = NCryptEncrypt(
                key_handle,
                Some(plaintext),
                Some(&iv),
                Some(&mut encrypted),
                &mut encrypted_len,
                NCRYPT_PAD_PKCS1_FLAG,
            );
            if result != ERROR_SUCCESS.0 {
                NCryptFreeObject(key_handle);
                NCryptFreeObject(prov_handle);
                return Err(WristKeyError::Platform(
                    format!("NCryptEncrypt failed: 0x{:X}", result)
                ));
            }
            encrypted.truncate(encrypted_len as usize);

            // 6. Build output: [1 byte provider][12 bytes IV][rest: ciphertext]
            let provider_flag = if WindowsSecurity::is_tpm_available() { 1u8 } else { 0u8 };
            let mut output = vec![provider_flag];
            output.extend_from_slice(&iv);
            output.extend_from_slice(&encrypted);

            // Cleanup
            NCryptFreeObject(key_handle);
            NCryptFreeObject(prov_handle);

            info!("Password encrypted with {} ({} bytes)", provider_name, output.len());
            Ok(output)
        }
    }

    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String> {
        use windows::Win32::Security::Cryptography::*;
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        if ciphertext.len() < 14 {
            return Err(WristKeyError::Platform("Ciphertext too short".into()));
        }

        // Parse: [1 byte provider][12 bytes IV][rest: ciphertext]
        let provider_flag = ciphertext[0];
        let iv = &ciphertext[1..13];
        let encrypted = &ciphertext[13..];

        let provider_name = if provider_flag == 1 {
            info!("Decrypting with TPM 2.0 (Microsoft Platform Crypto Provider)");
            "Microsoft Platform Crypto Provider"
        } else {
            info!("Decrypting with Software Key Storage Provider");
            "Microsoft Software Key Storage Provider"
        };

        let provider_name_wide: Vec<u16> = OsStr::new(provider_name)
            .encode_wide()
            .chain(Some(0))
            .collect();

        // Open provider
        let mut prov_handle = NCRYPT_PROV_HANDLE::default();
        unsafe {
            let result = NCryptOpenStorageProvider(
                &mut prov_handle,
                PCWSTR(provider_name_wide.as_ptr()),
                0,
            );
            if result != ERROR_SUCCESS.0 {
                return Err(WristKeyError::Platform(
                    format!("NCryptOpenStorageProvider failed: 0x{:X}", result)
                ));
            }
        }

        // Open key
        let key_name_wide: Vec<u16> = OsStr::new(WRISTKEY_KEY_NAME)
            .encode_wide()
            .chain(Some(0))
            .collect();

        let mut key_handle = NCRYPT_KEY_HANDLE::default();
        unsafe {
            let result = NCryptOpenKey(
                prov_handle,
                &mut key_handle,
                PCWSTR(key_name_wide.as_ptr()),
                0,
                0,
            );
            if result != ERROR_SUCCESS.0 {
                NCryptFreeObject(prov_handle);
                return Err(WristKeyError::Platform(
                    format!("NCryptOpenKey failed: 0x{:X} — key not found. Did you set the password first?", result)
                ));
            }

            // Decrypt
            let mut decrypted_len: u32 = 0;
            let _result = NCryptDecrypt(
                key_handle,
                Some(encrypted),
                Some(iv),
                None,
                &mut decrypted_len,
                NCRYPT_PAD_PKCS1_FLAG,
            );

            let mut decrypted = vec![0u8; decrypted_len as usize];
            let result = NCryptDecrypt(
                key_handle,
                Some(encrypted),
                Some(iv),
                Some(&mut decrypted),
                &mut decrypted_len,
                NCRYPT_PAD_PKCS1_FLAG,
            );
            if result != ERROR_SUCCESS.0 {
                NCryptFreeObject(key_handle);
                NCryptFreeObject(prov_handle);
                return Err(WristKeyError::Platform(
                    format!("NCryptDecrypt failed: 0x{:X}", result)
                ));
            }
            decrypted.truncate(decrypted_len as usize);

            NCryptFreeObject(key_handle);
            NCryptFreeObject(prov_handle);

            let password = String::from_utf8(decrypted)
                .map_err(|e| WristKeyError::Platform(format!("UTF-8 decode: {}", e)))?;

            info!("Password decrypted successfully ({} chars)", password.len());
            Ok(password)
        }
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
                                    info!(
                                        "Password sent to Credential Provider ({} bytes)",
                                        password.len()
                                    );
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
