use wristkey_core::{PlatformSecurity, Result, SessionManager};
use async_trait::async_trait;
use std::sync::Arc;

pub struct LinuxSecurity;
impl LinuxSecurity {
    pub fn new() -> Self { Self }
    pub fn set_session(&mut self, _session: Arc<SessionManager>) {}
    pub fn storage_type_description() -> &'static str { "file permissions (Linux)" }
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
        // No password simulation. Unlock must be performed by the authenticated PAM flow.
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
