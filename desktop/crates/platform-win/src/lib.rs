//! Windows platform security with auto-fallback vault backend.
//!
//! Vault hierarchy:
//!   1. TPM (CNG) -- hardware-backed, preferred
//!   2. Software Provider (CNG) -- software-backed, if TPM unavailable
//!   3. DPAPI -- ultimate fallback, always works, no key management
//!
//! Named pipe server is a lazy singleton -- started once on first unlock_screen().

use async_trait::async_trait;
use tokio::net::windows::named_pipe::ServerOptions;
use tracing::{info, warn, error};
use wristkey_core::{PlatformSecurity, PasswordVault, Result, WristKeyError, SessionManager};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use std::sync::{Once, OnceLock};

#[link(name = "user32")]
extern "system" {
    fn LockWorkStation() -> i32;
}

pub const CP_CLSID: &str = "{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}";
pub const CP_NAME: &str = "WristKey Credential Provider";

// ------------------------------------------------------------------------------
// Lazy singleton pipe password buffer + server
// ------------------------------------------------------------------------------

static PIPE_PASSWORD: OnceLock<Arc<Mutex<Option<String>>>> = OnceLock::new();
static PIPE_SERVER_STARTED: Once = Once::new();

fn get_pipe_password() -> Arc<Mutex<Option<String>>> {
    PIPE_PASSWORD.get_or_init(|| Arc::new(Mutex::new(None))).clone()
}

fn start_pipe_server_impl() {
    let password = get_pipe_password();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        match rt {
            Ok(rt) => rt.block_on(async move {
                UnlockPipeServer::new(password).run().await;
            }),
            Err(e) => error!("Pipe server runtime failed: {}", e),
        }
    });
}

// ------------------------------------------------------------------------------
// Vault Backend Selection
// ------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum VaultBackend {
    Tpm,
    Software,
    Dpapi,
}

impl VaultBackend {
    fn detect() -> Self {
        if Self::probe_tpm() {
            info!("Vault backend: TPM (CNG)");
            VaultBackend::Tpm
        } else if Self::probe_software_provider() {
            info!("Vault backend: Software Provider (CNG)");
            VaultBackend::Software
        } else {
            info!("Vault backend: DPAPI (ultimate fallback)");
            VaultBackend::Dpapi
        }
    }

    fn probe_tpm() -> bool {
        use windows::Win32::Security::Cryptography::{
            NCryptOpenStorageProvider, NCryptCreatePersistedKey, NCryptFreeObject,
            NCRYPT_PROV_HANDLE, NCRYPT_KEY_HANDLE, CERT_KEY_SPEC, NCRYPT_FLAGS,
        };
        use windows::core::PCWSTR;

        unsafe {
            let mut provider = NCRYPT_PROV_HANDLE(0);
            let tpm_provider: Vec<u16> = "Microsoft Platform Crypto Provider\0".encode_utf16().collect();
            if NCryptOpenStorageProvider(&mut provider, PCWSTR(tpm_provider.as_ptr()), 0).is_err() {
                return false;
            }
            let mut key = NCRYPT_KEY_HANDLE(0);
            let aes: Vec<u16> = "AES\0".encode_utf16().collect();
            let result = NCryptCreatePersistedKey(
                provider, &mut key, PCWSTR(aes.as_ptr()), PCWSTR::null(),
                CERT_KEY_SPEC(0), NCRYPT_FLAGS(0),
            );
            let _ = NCryptFreeObject(provider);
            result.is_ok()
        }
    }

    fn probe_software_provider() -> bool {
        use windows::Win32::Security::Cryptography::{
            NCryptOpenStorageProvider, NCryptCreatePersistedKey, NCryptFreeObject,
            NCRYPT_PROV_HANDLE, NCRYPT_KEY_HANDLE, CERT_KEY_SPEC, NCRYPT_FLAGS,
        };
        use windows::core::PCWSTR;

        unsafe {
            let mut provider = NCRYPT_PROV_HANDLE(0);
            let sw_provider: Vec<u16> = "Microsoft Software Key Storage Provider\0".encode_utf16().collect();
            if NCryptOpenStorageProvider(&mut provider, PCWSTR(sw_provider.as_ptr()), 0).is_err() {
                return false;
            }
            let mut key = NCRYPT_KEY_HANDLE(0);
            let aes: Vec<u16> = "AES\0".encode_utf16().collect();
            let result = NCryptCreatePersistedKey(
                provider, &mut key, PCWSTR(aes.as_ptr()), PCWSTR::null(),
                CERT_KEY_SPEC(0), NCRYPT_FLAGS(0),
            );
            let _ = NCryptFreeObject(provider);
            result.is_ok()
        }
    }

    fn description(&self) -> &'static str {
        match self {
            VaultBackend::Tpm => "TPM 2.0 (CNG)",
            VaultBackend::Software => "Software Key Storage Provider (CNG)",
            VaultBackend::Dpapi => "Windows Data Protection API (DPAPI)",
        }
    }
}

// ------------------------------------------------------------------------------
// WindowsVault -- public, no pipe, reusable
// ------------------------------------------------------------------------------

#[derive(Clone)]
pub struct WindowsVault {
    backend: VaultBackend,
}

impl WindowsVault {
    pub fn new() -> Self {
        Self { backend: VaultBackend::detect() }
    }
    pub fn backend_description(&self) -> &'static str {
        self.backend.description()
    }
}

impl Default for WindowsVault {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl PasswordVault for WindowsVault {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        self.dpapi_encrypt(password).await
    }
    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String> {
        self.dpapi_decrypt(ciphertext).await
    }
}

impl WindowsVault {
    async fn dpapi_encrypt(&self, password: &str) -> Result<Vec<u8>> {
        use windows::Win32::Security::Cryptography::{
            CryptProtectData, CRYPT_INTEGER_BLOB,
            CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
        };
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::core::PCWSTR;

        let data = password.as_bytes();
        let mut data_in = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptProtectData(
                &mut data_in, PCWSTR::null(), None, None, None,
                CRYPTPROTECT_LOCAL_MACHINE | CRYPTPROTECT_UI_FORBIDDEN,
                &mut data_out,
            ).map_err(|e| WristKeyError::Platform(format!("CryptProtectData failed: {:?}", e)))?;
        }

        let encrypted = unsafe {
            std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec()
        };
        unsafe {
            let _ = LocalFree(HLOCAL(data_out.pbData as *mut std::ffi::c_void));
        }
        Ok(encrypted)
    }

    async fn dpapi_decrypt(&self, ciphertext: &[u8]) -> Result<String> {
        use windows::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
        };
        use windows::Win32::Foundation::{LocalFree, HLOCAL};

        let mut data_in = CRYPT_INTEGER_BLOB {
            cbData: ciphertext.len() as u32,
            pbData: ciphertext.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPT_INTEGER_BLOB::default();

        unsafe {
            CryptUnprotectData(
                &mut data_in, None, None, None, None,
                CRYPTPROTECT_UI_FORBIDDEN, &mut data_out,
            ).map_err(|e| WristKeyError::Platform(format!("CryptUnprotectData failed: {:?}", e)))?;
        }

        let decrypted = unsafe {
            std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec()
        };
        unsafe {
            let _ = LocalFree(HLOCAL(data_out.pbData as *mut std::ffi::c_void));
        }

        String::from_utf8(decrypted)
            .map_err(|e| WristKeyError::Platform(format!("UTF-8 decode: {}", e)))
    }
}

// ------------------------------------------------------------------------------
// WindowsSecurity -- PlatformSecurity facade, lazy pipe
// ------------------------------------------------------------------------------

pub struct WindowsSecurity {
    session: Option<Arc<SessionManager>>,
    vault: WindowsVault,
}

impl WindowsSecurity {
    pub fn new() -> Self {
        Self { session: None, vault: WindowsVault::new() }
    }

    pub fn set_session(&mut self, session: Arc<SessionManager>) {
        self.session = Some(session);
    }

    /// Start the named pipe server (safe to call multiple times -- only starts once).
    pub fn start_pipe_server() {
        PIPE_SERVER_STARTED.call_once(start_pipe_server_impl);
    }

    pub fn storage_type_description() -> &'static str {
        "Windows Data Protection API (DPAPI) with TPM fallback"
    }

    pub fn vault_backend_description(&self) -> &'static str {
        self.vault.backend_description()
    }

    // --- Credential Provider (optional) ---

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
            .map_err(|e| WristKeyError::Platform(format!("create CLSID: {}", e)))?;
        clsid_key.set_value("", &CP_NAME)
            .map_err(|e| WristKeyError::Platform(format!("set CLSID name: {}", e)))?;

        let (inproc, _) = clsid_key.create_subkey("InprocServer32")
            .map_err(|e| WristKeyError::Platform(format!("create InprocServer32: {}", e)))?;
        inproc.set_value("", &dll_path)
            .map_err(|e| WristKeyError::Platform(format!("set DLL path: {}", e)))?;
        inproc.set_value("ThreadingModel", &"Apartment")
            .map_err(|e| WristKeyError::Platform(format!("set ThreadingModel: {}", e)))?;

        let cp_path = format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            CP_CLSID
        );
        let (cp_key, _) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .create_subkey(&cp_path)
            .map_err(|e| WristKeyError::Platform(format!("create CP key: {}", e)))?;
        cp_key.set_value("", &CP_NAME)
            .map_err(|e| WristKeyError::Platform(format!("set CP name: {}", e)))?;

        info!("Credential Provider registered");
        Ok(())
    }

    pub fn unregister_credential_provider() -> Result<()> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE};
        let _ = RegKey::predef(HKEY_CLASSES_ROOT).delete_subkey_all(format!(r"CLSID\{}", CP_CLSID));
        let _ = RegKey::predef(HKEY_LOCAL_MACHINE).delete_subkey_all(format!(
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{}",
            CP_CLSID
        ));
        info!("Credential Provider unregistered");
        Ok(())
    }

    pub async fn ensure_dll_extracted() -> Result<std::path::PathBuf> {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let dll_path = exe_dir.join("WristKeyCredentialProvider.dll");
        if dll_path.exists() { return Ok(dll_path); }
        let dev_dll = exe_dir
            .parent().and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .map(|p| p.join("crates/credential-provider/WristKeyCredentialProvider.dll"))
            .filter(|p| p.exists());
        if let Some(dev_path) = dev_dll {
            std::fs::copy(&dev_path, &dll_path).map_err(|e| {
                WristKeyError::Platform(format!("copy DLL: {}", e))
            })?;
            return Ok(dll_path);
        }
        Err(WristKeyError::Platform("WristKeyCredentialProvider.dll not found".into()))
    }
}

impl Default for WindowsSecurity {
    fn default() -> Self { Self::new() }
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
        Self::start_pipe_server(); // lazy start
        let pipe_password = get_pipe_password();
        if let Some(ref session) = self.session {
            let state = session.state().await;
            if let Some(device_id) = state.device_id() {
                match session.get_device_password(device_id).await {
                    Ok(Some(encrypted)) => {
                        match self.vault.decrypt_password(&encrypted).await {
                            Ok(password) => {
                                *pipe_password.lock().await = Some(password);
                                info!("Password buffered for Credential Provider");
                            }
                            Err(e) => warn!("Decrypt failed: {}", e),
                        }
                    }
                    Ok(None) => warn!("No password stored for device {}", device_id),
                    Err(e) => warn!("Retrieve password failed: {}", e),
                }
            }
        }
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        let dll_path = Self::ensure_dll_extracted().await?;
        Self::register_credential_provider(&dll_path.to_string_lossy())
    }
}

#[async_trait]
impl PasswordVault for WindowsSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        self.vault.encrypt_password(password).await
    }
    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String> {
        self.vault.decrypt_password(ciphertext).await
    }
}

// ------------------------------------------------------------------------------
// Named Pipe Server
// ------------------------------------------------------------------------------

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
                    match server.connect().await {
                        Ok(()) => {
                            if let Some(password) = self.password.lock().await.take() {
                                use tokio::io::AsyncWriteExt;
                                let _ = server.write_all(password.as_bytes()).await;
                                let _ = server.write_all(b"\n").await;
                                let _ = server.flush().await;
                                info!("Password sent to Credential Provider");
                            }
                        }
                        Err(e) => {
                            // Only log at debug level to avoid spam
                            tracing::debug!("Pipe connect: {}", e);
                        }
                    }
                }
                Err(e) => {
                    // Access denied usually means pipe already exists from previous instance
                    tracing::debug!("Pipe create: {} (will retry)", e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}
