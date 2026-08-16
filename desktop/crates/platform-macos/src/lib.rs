#[cfg(target_os = "macos")]
use wristkey_core::vault::KeyProtector;
use wristkey_core::{PlatformSecurity, WristKeyError, Result, SessionManager};
#[cfg(target_os = "macos")]
use security_framework::os::macos::keychain::SecKeychain;
use async_trait::async_trait;
use std::sync::Arc;

#[cfg(target_os = "macos")]
const SERVICE: &str = "com.wristkey.pairing";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "pairing_key";

#[cfg(target_os = "macos")]
pub struct MacosKeyProtector;

#[cfg(target_os = "macos")]
impl KeyProtector for MacosKeyProtector {
    fn protect(&self, key: &[u8]) -> Vec<u8> {
        let _ = SecKeychain::default().and_then(|kc| kc.find_generic_password(SERVICE, ACCOUNT)).and_then(|(_, item)| item.delete());
        let _ = SecKeychain::default().and_then(|kc| kc.add_generic_password(SERVICE, ACCOUNT, key));
        vec![]
    }
    fn unprotect(&self, _data: &[u8]) -> Option<Vec<u8>> {
        let keychain = SecKeychain::default().ok()?;
        let (password, _) = keychain.find_generic_password(SERVICE, ACCOUNT).ok()?;
        Some(password.as_ref().to_vec())
    }
}

#[cfg(not(target_os = "macos"))]
pub struct MacosKeyProtector;

#[cfg(not(target_os = "macos"))]
impl wristkey_core::vault::KeyProtector for MacosKeyProtector {
    fn protect(&self, key: &[u8]) -> Vec<u8> { key.to_vec() }
    fn unprotect(&self, data: &[u8]) -> Option<Vec<u8>> { Some(data.to_vec()) }
}

pub fn create_protector() -> MacosKeyProtector { MacosKeyProtector }

pub struct MacosSecurity;

impl Default for MacosSecurity {
    fn default() -> Self { Self::new() }
}

impl MacosSecurity {
    pub fn new() -> Self { Self }
    pub fn set_session(&mut self, _session: Arc<SessionManager>) {}
    pub fn storage_type_description() -> &'static str {
        "macOS Keychain"
    }
}

#[async_trait]
impl PlatformSecurity for MacosSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let _ = tokio::process::Command::new("osascript")
                .args(&["-e", "tell application \"System Events\" to keystroke \"q\" using {control down, command down}"])
                .output().await;
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

#[cfg(target_os = "macos")]
const PAM_SUCCESS: libc::c_int = 0;
#[cfg(target_os = "macos")]
const PAM_IGNORE: libc::c_int = 25;

#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn pam_sm_authenticate(
    _pamh: *mut libc::c_void,
    _flags: libc::c_int,
    _argc: libc::c_int,
    _argv: *const *const libc::c_char,
) -> libc::c_int {
    let uid = unsafe { libc::getuid() };
    let home = std::env::var("HOME").unwrap_or_else(|_| format!("/Users/{}", uid));
    let proof_path = std::path::PathBuf::from(home).join(".wristkey/.last_auth");

    let contents = match std::fs::read_to_string(&proof_path) {
        Ok(c) => c,
        Err(_) => return PAM_IGNORE,
    };

    let mut lines = contents.lines();
    let timestamp_str = lines.next().unwrap_or("0");
    let timestamp: u64 = timestamp_str.parse().unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    if now.saturating_sub(timestamp) > 5000 {
        return PAM_IGNORE;
    }

    let _ = std::fs::remove_file(&proof_path);
    PAM_SUCCESS
}

#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn pam_sm_setcred(
    _pamh: *mut libc::c_void,
    _flags: libc::c_int,
    _argc: libc::c_int,
    _argv: *const *const libc::c_char,
) -> libc::c_int {
    PAM_SUCCESS
}
