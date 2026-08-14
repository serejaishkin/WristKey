//! macOS platform adapter.
//!
//! Lock via AppleScript (Ctrl+Cmd+Q).
//! Unlock via Accessibility API — types stored password and presses Return.
//! Password is stored in macOS Keychain (secure, encrypted, system-managed).
//!
//! Requires: System Settings → Privacy & Security → Accessibility → WristKey (ON)

use async_trait::async_trait;
use wristkey_core::{PlatformSecurity, Result, WristKeyError};
use std::process::Command;

pub struct MacOSSecurity;

impl Default for MacOSSecurity {
    fn default() -> Self {
        Self::new()
    }
}

impl MacOSSecurity {
    pub fn new() -> Self {
        Self
    }

    /// Save password to macOS Keychain.
    /// Called from Settings GUI when user sets macOS password.
    pub fn save_password_to_keychain(password: &str) -> Result<()> {
        let service = "WristKey";
        let account = "macos_unlock_password";

        // Delete old entry if exists
        let _ = Command::new("security")
            .args(["delete-generic-password", "-s", service, "-a", account])
            .output();

        // Add new password
        let output = Command::new("security")
            .args([
                "add-generic-password",
                "-s", service,
                "-a", account,
                "-w", password,
                "-U",  // Update if exists
                "-T", "",  // Allow any app (WristKey) to access
            ])
            .output()
            .map_err(|e| WristKeyError::Platform(format!("keychain save failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(WristKeyError::Platform(format!("keychain save: {}", stderr)));
        }

        tracing::info!("macOS password saved to Keychain");
        Ok(())
    }

    /// Load password from macOS Keychain.
    fn load_password_from_keychain() -> Result<Option<String>> {
        let output = Command::new("security")
            .args([
                "find-generic-password",
                "-s", "WristKey",
                "-a", "macos_unlock_password",
                "-w",  // Output only password
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let password = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if password.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(password))
                }
            }
            _ => Ok(None),  // Password not set yet
        }
    }

    /// Delete password from Keychain.
    pub fn delete_password_from_keychain() -> Result<()> {
        let output = Command::new("security")
            .args([
                "delete-generic-password",
                "-s", "WristKey",
                "-a", "macos_unlock_password",
            ])
            .output()
            .map_err(|e| WristKeyError::Platform(format!("keychain delete failed: {}", e)))?;

        if output.status.success() {
            tracing::info!("macOS password deleted from Keychain");
        }
        Ok(())
    }

    /// Check if screen is locked using CGSessionCopyCurrentDictionary
    #[cfg(target_os = "macos")]
    fn is_screen_locked() -> bool {
        unsafe {
            let session = core_graphics::display::CGSessionCopyCurrentDictionary();
            if session.is_null() {
                return false;
            }
            let dict = core_foundation::base::TCFType::wrap_under_create_rule(
                session as core_foundation::dictionary::CGDictionaryRef
            );
            if let Some(on_console) = dict.find(core_foundation::string::CFString::new("OnConsoleKey")) {
                let value: i32 = core_foundation::base::TCFType::wrap_under_get_rule(
                    on_console as core_foundation::number::CFNumberRef
                ).to_i32().unwrap_or(1);
                return value == 0;
            }
            false
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn is_screen_locked() -> bool {
        false
    }

    /// Check if Accessibility permission is granted.
    /// Note: Runtime check requires ApplicationServices framework (system).
    /// We skip compile-time check and rely on osascript error handling.
    fn check_accessibility_permission() -> bool {
        // Try a harmless AppleScript to test if we have permission
        match std::process::Command::new("osascript")
            .arg("-e")
            .arg("tell application "System Events" to return name of first process")
            .output()
        {
            Ok(out) if out.status.success() => true,
            _ => false,
        }
    }
}

#[async_trait]
impl PlatformSecurity for MacOSSecurity {
    async fn lock_screen(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            let script = r#"tell application "System Events" to keystroke "q" using {command down, control down}"#;
            let output = Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output()
                .map_err(|e| WristKeyError::Platform(format!("osascript lock: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(WristKeyError::Platform(format!("macOS lock failed: {}", stderr)));
            }
            tracing::info!("macOS screen locked via AppleScript");
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(WristKeyError::Platform("not on macOS".into()))
        }
    }

    async fn unlock_screen(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            // Check Accessibility permission first
            if !Self::check_accessibility_permission() {
                return Err(WristKeyError::Platform(
                    "macOS unlock requires Accessibility permission. \
                    Go to System Settings → Privacy & Security → Accessibility → enable WristKey.".into()
                ));
            }

            // Check if screen is actually locked
            if !Self::is_screen_locked() {
                tracing::info!("macOS: screen not locked, nothing to unlock");
                return Ok(());
            }

            // Load password from Keychain
            let password = Self::load_password_from_keychain()?.ok_or_else(|| {
                WristKeyError::Platform(
                    "macOS unlock requires password. Set it in WristKey Settings → macOS Password.".into()
                )
            })?;

            // Wake display first (in case it's asleep)
            let _ = Command::new("caffeinate")
                .args(["-u", "-t", "1"])
                .spawn();

            // Type password and press Return via Accessibility API
            let escaped = password.replace("\", "\\").replace(""", "\\\"");
            let script = format!(
                r#"tell application "System Events"
    keystroke "{}"
    delay 0.1
    key code 36
end tell"#,
                escaped
            );

            let output = Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
                .map_err(|e| WristKeyError::Platform(format!("osascript unlock: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("not allowed to assistive access") {
                    return Err(WristKeyError::Platform(
                        "macOS unlock failed: Accessibility permission denied. \
                        Go to System Settings → Privacy & Security → Accessibility → enable WristKey.".into()
                    ));
                }
                return Err(WristKeyError::Platform(format!("macOS unlock failed: {}", stderr)));
            }

            tracing::info!("macOS screen unlocked via Accessibility API");
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(WristKeyError::Platform("not on macOS".into()))
        }
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(Self::is_screen_locked())
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        tracing::info!("macOS: WristKey uses Keychain + Accessibility for unlock. \
        Set password in Settings. Grant Accessibility permission when prompted.");
        Ok(())
    }
}
