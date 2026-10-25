use wristkey_core::{PlatformSecurity, Result, SessionManager};
use async_trait::async_trait;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(target_os = "macos")]
use security_framework::os::macos::keychain::SecKeychain;

extern "C" {
    #[cfg(target_os = "macos")]
    fn getpwuid(uid: u32) -> *mut Passwd;
    #[cfg(target_os = "macos")]
    fn getpwnam(name: *const c_char) -> *mut Passwd;
    #[cfg(target_os = "macos")]
    fn getuid() -> u32;
}

#[repr(C)]
#[derive(Debug)]
#[cfg(target_os = "macos")]
struct Passwd {
    pw_name: *const c_char,
    pw_passwd: *const c_char,
    pw_uid: u32,
    pw_gid: u32,
    pw_gecos: *const c_char,
    pw_dir: *const c_char,
    pw_shell: *const c_char,
}

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

    /// Get all non-system users from /Local/Default/Users via dscl.
    #[cfg(target_os = "macos")]
    pub fn get_all_users() -> Vec<String> {
        let mut users = Vec::new();
        if let Ok(output) = std::process::Command::new("dscl")
            .args(["localhost", "-list", "/Local/Default/Users"])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    let name = line.trim();
                    if name.is_empty() || name.starts_with('_') || name == "daemon" || name == "nobody" || name == "root" {
                        continue;
                    }
                    users.push(name.to_string());
                }
            }
        }
        users
    }

    /// Get current user name via getpwuid(getuid()).
    #[cfg(target_os = "macos")]
    pub fn get_current_user() -> Option<String> {
        unsafe {
            let pw = getpwuid(getuid());
            if pw.is_null() { return None; }
            CStr::from_ptr((*pw).pw_name).to_string_lossy().to_string().into()
        }
    }

    /// Get user home directory.
    #[cfg(target_os = "macos")]
    pub fn get_user_home_dir(user_name: Option<&str>) -> Option<PathBuf> {
        unsafe {
            let pw = match user_name {
                Some(name) => {
                    let c_name = std::ffi::CString::new(name).ok()?;
                    getpwnam(c_name.as_ptr())
                }
                None => getpwuid(getuid()),
            };
            if pw.is_null() { return None; }
            CStr::from_ptr((*pw).pw_dir).to_string_lossy().to_string().into()
        }
    }

    /// ~/Library/Application Support.
    #[cfg(target_os = "macos")]
    pub fn get_user_data_dir() -> Option<PathBuf> {
        Self::get_user_home_dir(None).map(|h| h.join("Library/Application Support"))
    }

    /// ~/Library/Logs.
    #[cfg(target_os = "macos")]
    pub fn get_user_logs_dir() -> Option<PathBuf> {
        Self::get_user_home_dir(None).map(|h| h.join("Library/Logs"))
    }

    /// /etc.
    #[cfg(target_os = "macos")]
    pub fn get_system_data_dir() -> PathBuf { PathBuf::from("/etc") }

    /// /Library/Logs.
    #[cfg(target_os = "macos")]
    pub fn get_system_logs_dir() -> PathBuf { PathBuf::from("/Library/Logs") }

    /// /tmp.
    #[cfg(target_os = "macos")]
    pub fn get_temp_dir() -> PathBuf {
        if let Ok(td) = std::env::var("TMPDIR") {
            if !td.is_empty() { return PathBuf::from(td); }
        }
        PathBuf::from("/tmp")
    }
}

#[async_trait]
impl PlatformSecurity for MacosSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        { let _ = tokio::process::Command::new("/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession").args(["-suspend"]).output().await; }
        Ok(())
    }
    async fn unlock_screen(&self) -> Result<()> {
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
