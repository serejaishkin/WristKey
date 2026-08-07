//! Windows platform adapter.
//!
//! Lock via LockWorkStation (user32.dll). Unlock via Windows Hello CDF — v2.

use async_trait::async_trait;
use tracing::info;
use wristkey_core::{PlatformSecurity, Result, WristKeyError};

pub struct WindowsSecurity;

impl Default for WindowsSecurity {
    fn default() -> Self {
        Self::new()
    }
}
impl WindowsSecurity {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn LockWorkStation() -> i32;
}

#[async_trait]
impl PlatformSecurity for WindowsSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(windows)]
        {
            let result = unsafe { LockWorkStation() };
            if result == 0 {
                return Err(WristKeyError::Platform(
                    "LockWorkStation failed".into()
                ));
            }
            info!("workstation locked via WinAPI");
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Err(WristKeyError::Platform("not on Windows".into()))
        }
    }

    async fn is_locked(&self) -> Result<bool> {
        // MVP: assume not locked. Proper impl requires checking foreground window.
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        info!("Windows authenticator: Credential Provider V2 — planned for v2");
        Ok(())
    }
}
