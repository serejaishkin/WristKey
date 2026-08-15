//! Linux platform security implementation
//!
//! Lock:   loginctl lock-session
//! Unlock: loginctl unlock-session (requires configured PAM / polkit)
//! Vault:  secret-service (placeholder)

use async_trait::async_trait;
use wristkey_core::{PlatformSecurity, PasswordVault, Result, WristKeyError, SessionManager};
use std::sync::Arc;

pub struct LinuxSecurity {
    session: Option<Arc<SessionManager>>,
}

impl LinuxSecurity {
    pub fn new() -> Self {
        Self { session: None }
    }
    pub fn set_session(&mut self, session: Arc<SessionManager>) {
        self.session = Some(session);
    }
    pub fn storage_type_description() -> &'static str { "Linux secret-service (placeholder)" }
}

impl Default for LinuxSecurity {
    fn default() -> Self { Self::new() }
}

#[async_trait]
impl PlatformSecurity for LinuxSecurity {
    async fn lock_screen(&self) -> Result<()> {
        let output = tokio::process::Command::new("loginctl")
            .args(&["lock-session"])
            .output()
            .await
            .map_err(|e| WristKeyError::Platform(format!("loginctl lock failed: {}", e)))?;
        if !output.status.success() {
            return Err(WristKeyError::Platform(format!(
                "loginctl lock error: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        let output = tokio::process::Command::new("loginctl")
            .args(&["unlock-session"])
            .output()
            .await
            .map_err(|e| WristKeyError::Platform(format!("loginctl unlock failed: {}", e)))?;
        if !output.status.success() {
            return Err(WristKeyError::Platform(format!(
                "loginctl unlock error: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
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
impl PasswordVault for LinuxSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        // TODO: integrate with secret-service or keyutils
        // Placeholder: XOR obfuscation -- NOT SECURE, replace before production!
        Ok(password.bytes().map(|b| b ^ 0x55).collect())
    }

    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String> {
        Ok(ciphertext.iter().map(|b| (b ^ 0x55) as char).collect())
    }
}
