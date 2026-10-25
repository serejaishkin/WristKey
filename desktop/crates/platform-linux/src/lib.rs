use wristkey_core::{PlatformSecurity, Result, SessionManager};
use async_trait::async_trait;
use std::sync::Arc;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;

extern "C" {
    #[cfg(target_os = "linux")]
    fn getpwuid(uid: u32) -> *mut Passwd;
    #[cfg(target_os = "linux")]
    fn getpwnam(name: *const c_char) -> *mut Passwd;
    #[cfg(target_os = "linux")]
    fn getspnam(name: *const c_char) -> *mut Spwd;
    #[cfg(target_os = "linux")]
    fn crypt(key: *const c_char, salt: *const c_char) -> *const c_char;
    #[cfg(target_os = "linux")]
    fn getuid() -> u32;
}

#[repr(C)]
#[derive(Debug)]
#[cfg(target_os = "linux")]
struct Passwd {
    pw_name: *const c_char,
    pw_passwd: *const c_char,
    pw_uid: u32,
    pw_gid: u32,
    pw_gecos: *const c_char,
    pw_dir: *const c_char,
    pw_shell: *const c_char,
}

#[repr(C)]
#[derive(Debug)]
#[cfg(target_os = "linux")]
struct Spwd {
    sp_namp: *const c_char,
    sp_pwdp: *const c_char,
}

pub struct LinuxSecurity;
impl LinuxSecurity {
    pub fn new() -> Self { Self }
    pub fn set_session(&mut self, _session: Arc<SessionManager>) {}
    pub fn storage_type_description() -> &'static str { "file permissions (Linux)" }

    /// Get all non-system users from /etc/passwd (UID 1000-60000).
    #[cfg(target_os = "linux")]
    pub fn get_all_users() -> Vec<String> {
        let mut users = Vec::new();
        if let Ok(contents) = std::fs::read_to_string("/etc/passwd") {
            for line in contents.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() < 3 { continue; }
                if let Ok(uid) = parts[2].parse::<u32>() {
                    if uid == 0 || uid < 1000 || uid > 60000 { continue; }
                    users.push(parts[0].to_string());
                }
            }
        }
        users
    }

    /// Get current user name, respecting SUDO_UID / PKEXEC_UID env vars.
    #[cfg(target_os = "linux")]
    pub fn get_current_user() -> Option<String> {
        let uid = std::env::var("SUDO_UID")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .or_else(|| std::env::var("PKEXEC_UID").ok().and_then(|v| v.parse::<u32>().ok()))
            .unwrap_or_else(|| unsafe { getuid() });
        unsafe {
            let pw = getpwuid(uid);
            if pw.is_null() { return None; }
            CStr::from_ptr((*pw).pw_name).to_string_lossy().to_string().into()
        }
    }

    /// Verify a user's password against /etc/shadow (requires root or shadow group).
    #[cfg(target_os = "linux")]
    pub fn check_login(user_name: &str, password: &str) -> bool {
        unsafe {
            let c_user = std::ffi::CString::new(user_name).ok()?;
            let pw = getpwnam(c_user.as_ptr());
            if pw.is_null() { return false; }
            let salt = CStr::from_ptr((*pw).pw_passwd).to_string_lossy().to_string();
            if !salt.starts_with("x") {
                let c_password = std::ffi::CString::new(password).ok()?;
                let result = crypt(c_password.as_ptr(), salt.as_ptr());
                if !result.is_null() {
                    let result_str = CStr::from_ptr(result).to_string_lossy().to_string();
                    if result_str == salt {
                        return true;
                    }
                }
            }
            let shadow = getspnam(c_user.as_ptr());
            if shadow.is_null() { return false; }
            let c_password = std::ffi::CString::new(password).ok()?;
            let result = crypt(c_password.as_ptr(), (*shadow).sp_pwdp);
            if !result.is_null() {
                let result_str = CStr::from_ptr(result).to_string_lossy().to_string();
                if result_str == CStr::from_ptr((*shadow).sp_pwdp).to_string_lossy().to_string() {
                    return true;
                }
            }
            false
        }
    }

    /// Get user home directory.
    #[cfg(target_os = "linux")]
    pub fn get_user_home_dir(user_name: Option<&str>) -> Option<PathBuf> {
        unsafe {
            let pw = match user_name {
                Some(name) => {
                    let c_name = std::ffi::CString::new(name).ok()?;
                    getpwnam(c_name.as_ptr())
                }
                None => getpwuid(std::env::var("SUDO_UID").ok().and_then(|v| v.parse().ok()).unwrap_or_else(getuid)),
            };
            if pw.is_null() { return None; }
            CStr::from_ptr((*pw).pw_dir).to_string_lossy().to_string().into()
        }
    }

    /// XDG_DATA_HOME or ~/.local/share.
    #[cfg(target_os = "linux")]
    pub fn get_user_data_dir() -> Option<PathBuf> {
        if let Ok(dh) = std::env::var("XDG_DATA_HOME") {
            if !dh.is_empty() { return Some(PathBuf::from(dh)); }
        }
        Self::get_user_home_dir(None).map(|h| h.join(".local/share"))
    }

    /// XDG_STATE_HOME or ~/.local/state.
    #[cfg(target_os = "linux")]
    pub fn get_user_logs_dir() -> Option<PathBuf> {
        if let Ok(sh) = std::env::var("XDG_STATE_HOME") {
            if !sh.is_empty() { return Some(PathBuf::from(sh)); }
        }
        Self::get_user_home_dir(None).map(|h| h.join(".local/state"))
    }

    /// System data directory.
    #[cfg(target_os = "linux")]
    pub fn get_system_data_dir() -> PathBuf { PathBuf::from("/etc") }

    /// System logs directory.
    #[cfg(target_os = "linux")]
    pub fn get_system_logs_dir() -> PathBuf { PathBuf::from("/var/log") }

    /// Temp directory.
    #[cfg(target_os = "linux")]
    pub fn get_temp_dir() -> PathBuf {
        if let Ok(td) = std::env::var("TMPDIR") {
            if !td.is_empty() { return PathBuf::from(td); }
        }
        PathBuf::from("/tmp")
    }

    #[cfg(target_os = "linux")]
    async fn current_session_locked() -> bool {
        let session = std::env::var("XDG_SESSION_ID").unwrap_or_default();
        if session.is_empty() { return false; }
        match tokio::process::Command::new("loginctl").args(["show-session", &session, "-p", "LockedHint", "--value"]).output().await {
            Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().eq_ignore_ascii_case("yes"),
            _ => false,
        }
    }
}

#[async_trait]
impl PlatformSecurity for LinuxSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(target_os = "linux")]
        { let _ = tokio::process::Command::new("loginctl").args(["lock-session"]).output().await; }
        Ok(())
    }
    async fn unlock_screen(&self) -> Result<()> {
        Ok(())
    }
    async fn is_locked(&self) -> Result<bool> {
        #[cfg(target_os = "linux")]
        { return Ok(Self::current_session_locked().await); }
        #[allow(unreachable_code)]
        Ok(false)
    }
    async fn register_as_authenticator(&self) -> Result<()> { Ok(()) }
}

#[cfg(target_os = "linux")]
const PAM_SUCCESS: libc::c_int = 0;
#[cfg(target_os = "linux")]
const PAM_IGNORE: libc::c_int = 25;

#[cfg(target_os = "linux")]
#[no_mangle]
pub extern "C" fn pam_sm_authenticate(_pamh: *mut libc::c_void, _flags: libc::c_int, _argc: libc::c_int, _argv: *const *const libc::c_char) -> libc::c_int {
    let uid = unsafe { libc::getuid() };
    let home = if uid == 0 { std::env::var("HOME").unwrap_or_else(|_| "/root".to_string()) } else {
        match unsafe { libc::getpwuid(uid) }.as_ref() {
            Some(pw) => unsafe { std::ffi::CStr::from_ptr(pw.pw_dir).to_string_lossy().to_string() },
            None => return PAM_IGNORE,
        }
    };
    let proof_path = std::path::PathBuf::from(home).join(".wristkey/.last_auth");
    let contents = match std::fs::read_to_string(&proof_path) { Ok(c) => c, Err(_) => return PAM_IGNORE };
    let timestamp: u64 = contents.lines().next().unwrap_or("0").parse().unwrap_or(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
    if now.saturating_sub(timestamp) > 5000 { return PAM_IGNORE; }
    let meta = match std::fs::metadata(&proof_path) { Ok(m) => m, Err(_) => return PAM_IGNORE };
    #[cfg(unix)] { use std::os::unix::fs::MetadataExt; if meta.uid() != uid { return PAM_IGNORE; } }
    let _ = std::fs::remove_file(&proof_path);
    PAM_SUCCESS
}
