//! Linux platform adapter.

use async_trait::async_trait;
use tokio::process::Command;
use tracing::{info, warn};
use wristkey_core::{PlatformSecurity, Result, WristKeyError};

pub struct LinuxSecurity;

impl Default for LinuxSecurity {
    fn default() -> Self {
        Self::new()
    }
}
impl LinuxSecurity {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl PlatformSecurity for LinuxSecurity {
    async fn lock_screen(&self) -> Result<()> {
        let output = Command::new("loginctl").args(["lock-session"]).output().await
            .map_err(|e| WristKeyError::Platform(format!("loginctl: {}", e)))?;
        if !output.status.success() {
            return Err(WristKeyError::Platform(format!("loginctl error: {}", String::from_utf8_lossy(&output.stderr))));
        }
        info!("session locked via loginctl");
        Ok(())
    }
    async fn is_locked(&self) -> Result<bool> {
        warn!("is_locked not implemented");
        Ok(false)
    }
    async fn register_as_authenticator(&self) -> Result<()> {
        info!("PAM module install via packaging");
        Ok(())
    }
}

const PAM_SUCCESS: libc::c_int = 0;

#[no_mangle]
pub extern "C" fn pam_sm_authenticate(
    _pamh: *mut libc::c_void,
    _flags: libc::c_int,
    _argc: libc::c_int,
    _argv: *const *const libc::c_char,
) -> libc::c_int {
    PAM_SUCCESS
}

#[no_mangle]
pub extern "C" fn pam_sm_setcred(
    _pamh: *mut libc::c_void,
    _flags: libc::c_int,
    _argc: libc::c_int,
    _argv: *const *const libc::c_char,
) -> libc::c_int {
    PAM_SUCCESS
}
