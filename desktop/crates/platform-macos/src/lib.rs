//! macOS platform security implementation
//!
//! Lock:   CGSessionCopyCurrentDictionary / osascript
//! Unlock: Placeholder (macOS secure unlock requires Touch ID / password dialog)
//! Vault:  Keychain Services (placeholder)

use async_trait::async_trait;
use wristkey_core::{PlatformSecurity, PasswordVault, Result, WristKeyError, SessionManager};
use std::sync::Arc;

pub struct MacosSecurity;

impl MacosSecurity {
    pub fn new() -> Self { Self }
    pub fn set_session(&mut self, _session: Arc<SessionManager>) {}
    pub fn storage_type_description() -> &'static str { "macOS Keychain (placeholder)" }
}

impl Default for MacosSecurity {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl PlatformSecurity for MacosSecurity {
    async fn lock_screen(&self) -> Result<()> {
        let output = tokio::process::Command::new("osascript")
            .args(&["-e", "tell application \"System Events\" to keystroke \"q\" using {control down, command down}"])
            .output()
            .await
            .map_err(|e| WristKeyError::Platform(format!("macOS lock failed: {}", e)))?;
        if !output.status.success() {
            return Err(WristKeyError::Platform(format!(
                "macOS lock script failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        // macOS does not allow programmatic unlock without user interaction.
        // Touch ID / Apple Watch unlock is handled by the OS.
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl PasswordVault for MacosSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        // TODO: replace with Keychain Services (Security.framework)
        // Placeholder: XOR obfuscation -- NOT SECURE, replace before production!
        Ok(password.bytes().map(|b| b ^ 0xAA).collect())
    }

    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String> {
        Ok(ciphertext.iter().map(|b| (b ^ 0xAA) as char).collect())
    }
}
