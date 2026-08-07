//! macOS platform adapter.
//!
//! Lock via AppleScript. NO Apple Watch emulation. Unlock manual only.

use async_trait::async_trait;
use tracing::{info, warn};
use wristkey_core::{PlatformSecurity, Result, WristKeyError};

pub struct MacOSSecurity;

impl MacOSSecurity {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PlatformSecurity for MacOSSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let script = r#"tell application "System Events" to keystroke "q" using {command down, control down}"#;
            let output = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output()
                .map_err(|e| WristKeyError::Platform(format!("osascript: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(WristKeyError::Platform(format!("macOS lock: {}", stderr)));
            }
            info!("screen locked via AppleScript");
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            warn!("MacOSSecurity::lock_screen called on non-macOS");
            Err(WristKeyError::Platform("not on macOS".into()))
        }
    }

    async fn is_locked(&self) -> Result<bool> {
        #[cfg(target_os = "macos")]
        {
            // TODO: CGSession check
            Ok(false)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Ok(false)
        }
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        info!("macOS: presence detection only, no pluggable GUI auth");
        Ok(())
    }
}
