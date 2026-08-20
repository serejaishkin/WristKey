use wristkey_core::{PlatformSecurity, Result, SessionManager};
use async_trait::async_trait;
use std::sync::Arc;

#[cfg(target_os = "macos")]
use security_framework::os::macos::keychain::SecKeychain;

/// macOS-local credential vault. The password never enters BLE or the Watch.
#[cfg(target_os = "macos")]
pub struct MacosVault;

#[cfg(target_os = "macos")]
impl MacosVault {
    fn service(device_id: &str) -> String { format!("com.wristkey.unlock.{}", device_id) }
    fn account() -> &'static str { "login_password" }

    pub fn set_password(device_id: &str, password: &str) -> Result<()> {
        let service = Self::service(device_id);
        let keychain = SecKeychain::default().map_err(|e| wristkey_core::WristKeyError::Platform(e.to_string()))?;
        if let Ok((_, item)) = keychain.find_generic_password(&service, Self::account()) {
            let _ = item.delete();
        }
        keychain.add_generic_password(&service, Self::account(), password.as_bytes())
            .map_err(|e| wristkey_core::WristKeyError::Platform(e.to_string()))?;
        Ok(())
    }

    pub fn get_password(device_id: &str) -> Result<String> {
        let service = Self::service(device_id);
        let keychain = SecKeychain::default().map_err(|e| wristkey_core::WristKeyError::Platform(e.to_string()))?;
        let (password, _) = keychain.find_generic_password(&service, Self::account())
            .map_err(|e| wristkey_core::WristKeyError::Platform(e.to_string()))?;
        String::from_utf8(password.as_ref().to_vec())
            .map_err(|_| wristkey_core::WristKeyError::Platform("Keychain password is not UTF-8".into()))
    }

    pub fn delete_password(device_id: &str) -> Result<()> {
        let service = Self::service(device_id);
        let keychain = SecKeychain::default().map_err(|e| wristkey_core::WristKeyError::Platform(e.to_string()))?;
        if let Ok((_, item)) = keychain.find_generic_password(&service, Self::account()) {
            let _ = item.delete();
        }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub struct MacosVault;

#[cfg(not(target_os = "macos"))]
impl MacosVault {
    pub fn set_password(_device_id: &str, _password: &str) -> Result<()> { Err(wristkey_core::WristKeyError::Platform("macOS vault unavailable on this platform".into())) }
    pub fn get_password(_device_id: &str) -> Result<String> { Err(wristkey_core::WristKeyError::Platform("macOS vault unavailable on this platform".into())) }
    pub fn delete_password(_device_id: &str) -> Result<()> { Err(wristkey_core::WristKeyError::Platform("macOS vault unavailable on this platform".into())) }
}

pub struct MacosSecurity;
impl Default for MacosSecurity { fn default() -> Self { Self::new() } }
impl MacosSecurity {
    pub fn new() -> Self { Self }
    pub fn set_session(&mut self, _session: Arc<SessionManager>) {}
    pub fn storage_type_description() -> &'static str { "macOS Keychain" }
}

#[async_trait]
impl PlatformSecurity for MacosSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let _ = tokio::process::Command::new("/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession")
                .args(["-suspend"]).output().await;
        }
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        // The daemon must not synthesize keystrokes. Authentication is performed by
        // the dedicated macOS authentication/helper layer after Watch verification.
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        #[cfg(target_os = "macos")]
        {
            let out = tokio::process::Command::new("ioreg").args(["-n", "Root", "-d", "1"]).output().await;
            if let Ok(out) = out {
                let text = String::from_utf8_lossy(&out.stdout).to_ascii_lowercase();
                if text.contains("cgssessionscreenislocked = yes") { return Ok(true); }
                if text.contains("cgssessionscreenislocked = no") { return Ok(false); }
            }
        }
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> { Ok(()) }
}

/// Temporary PAM bridge: it only consumes a short-lived local proof.
/// The proof is created only after the daemon has verified the Watch signature.
#[cfg(target_os = "macos")]
const PAM_SUCCESS: libc::c_int = 0;
#[cfg(target_os = "macos")]
const PAM_IGNORE: libc::c_int = 25;

#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn pam_sm_authenticate(_pamh: *mut libc::c_void, _flags: libc::c_int, _argc: libc::c_int, _argv: *const *const libc::c_char) -> libc::c_int {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/unknown".to_string());
    let proof_path = std::path::PathBuf::from(home).join(".wristkey/.last_auth");
    let contents = match std::fs::read_to_string(&proof_path) { Ok(c) => c, Err(_) => return PAM_IGNORE };
    let timestamp: u64 = contents.lines().next().unwrap_or("0").parse().unwrap_or(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    if now.saturating_sub(timestamp) > 5000 { return PAM_IGNORE; }
    let _ = std::fs::remove_file(&proof_path);
    PAM_SUCCESS
}

#[cfg(target_os = "macos")]
#[no_mangle]
pub extern "C" fn pam_sm_setcred(_pamh: *mut libc::c_void, _flags: libc::c_int, _argc: libc::c_int, _argv: *const *const libc::c_char) -> libc::c_int { PAM_SUCCESS }
