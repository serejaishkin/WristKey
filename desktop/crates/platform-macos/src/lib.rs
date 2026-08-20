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
        if let Ok((_, item)) = keychain.find_generic_password(&service, Self::account()) { let _ = item.delete(); }
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
        if let Ok((_, item)) = keychain.find_generic_password(&service, Self::account()) { let _ = item.delete(); }
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub struct MacosVault;
#[cfg(not(target_os = "macos"))]
impl MacosVault {
    pub fn set_password(_: &str, _: &str) -> Result<()> { Err(wristkey_core::WristKeyError::Platform("macOS vault unavailable on this platform".into())) }
    pub fn get_password(_: &str) -> Result<String> { Err(wristkey_core::WristKeyError::Platform("macOS vault unavailable on this platform".into())) }
    pub fn delete_password(_: &str) -> Result<()> { Err(wristkey_core::WristKeyError::Platform("macOS vault unavailable on this platform".into())) }
}

pub struct MacosSecurity;
impl Default for MacosSecurity { fn default() -> Self { Self::new() } }
impl MacosSecurity {
    pub fn new() -> Self { Self }
    pub fn set_session(&mut self, _session: Arc<SessionManager>) {}
    pub fn storage_type_description() -> &'static str { "macOS Keychain + launchd auth helper" }
}

#[async_trait]
impl PlatformSecurity for MacosSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        { let _ = tokio::process::Command::new("/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession").args(["-suspend"]).output().await; }
        Ok(())
    }
    async fn unlock_screen(&self) -> Result<()> {
        // Deliberately no synthetic keyboard input. The daemon must first verify
        // the Watch and then use the dedicated macOS authentication helper.
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
