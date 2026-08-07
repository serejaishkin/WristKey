//! Windows platform adapter.
//!
//! Lock via LockWorkStation. Unlock via Windows Hello CDF — v2.

use async_trait::async_trait;
use tracing::{info, warn};
use wristkey_core::{PlatformSecurity, Result, WristKeyError};

pub struct WindowsSecurity;

impl WindowsSecurity {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PlatformSecurity for WindowsSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(windows)]
        {
            use windows::Win32::System::WindowsProgramming::LockWorkStation;
            unsafe {
                LockWorkStation().map_err(|e| {
                    WristKeyError::Platform(format!("LockWorkStation failed: {:?}", e))
                })?;
            }
            info!("workstation locked via WinAPI");
            Ok(())
        }
        #[cfg(not(windows))]
        {
            warn!("WindowsSecurity::lock_screen called on non-Windows");
            Err(WristKeyError::Platform("not on Windows".into()))
        }
    }

    async fn is_locked(&self) -> Result<bool> {
        #[cfg(windows)]
        {
            // MVP: assume not locked. Proper impl requires checking foreground window.
            Ok(false)
        }
        #[cfg(not(windows))]
        {
            Ok(false)
        }
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        info!("Windows authenticator: Credential Provider V2 — planned for v2");
        Ok(())
    }
}
