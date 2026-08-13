//! Linux platform adapter.

use async_trait::async_trait;
use tokio::process::Command;
use tracing::{info, warn};
use wristkey_core::{PlatformSecurity, PasswordVault, Result, WristKeyError};

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

    async fn unlock_screen(&self) -> Result<()> {
        let output = Command::new("loginctl").args(["unlock-session"]).output().await
            .map_err(|e| WristKeyError::Platform(format!("loginctl unlock: {}", e)))?;
        if !output.status.success() {
            return Err(WristKeyError::Platform(format!("loginctl unlock error: {}", String::from_utf8_lossy(&output.stderr))));
        }
        info!("session unlocked via loginctl");
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        // Placeholder: detect if session is locked via loginctl
        let output = Command::new("loginctl").args(["show-session", "--property=LockedHint"]).output().await
            .map_err(|e| WristKeyError::Platform(format!("loginctl show-session: {}", e)))?;
        if !output.status.success() {
            return Ok(false);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim().contains("yes"))
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        // Credential Provider registration is Windows-only
        warn!("register_as_authenticator is a no-op on Linux");
        Ok(())
    }
}

#[async_trait]
impl PasswordVault for LinuxSecurity {
    async fn encrypt_password(&self, password: &str) -> Result<Vec<u8>> {
        warn!("Linux password encryption uses placeholder XOR — replace with secret-service!");
        let key = b"wristkey-placeholder-key";
        let mut out = Vec::with_capacity(password.len());
        for (i, b) in password.bytes().enumerate() {
            out.push(b ^ key[i % key.len()]);
        }
        Ok(out)
    }

    async fn decrypt_password(&self, ciphertext: &[u8]) -> Result<String> {
        let key = b"wristkey-placeholder-key";
        let mut out = Vec::with_capacity(ciphertext.len());
        for (i, b) in ciphertext.iter().enumerate() {
            out.push(b ^ key[i % key.len()]);
        }
        String::from_utf8(out)
            .map_err(|e| WristKeyError::Platform(format!("decrypt UTF-8 error: {}", e)))
    }
}
