//! Windows platform security implementation

use async_trait::async_trait;
use tracing::info;
use wristkey_core::{PlatformSecurity, Result, WristKeyError};

#[link(name = "user32")]
extern "system" {
    fn LockWorkStation() -> i32;
}

pub struct WindowsSecurity;

impl WindowsSecurity {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PlatformSecurity for WindowsSecurity {
    async fn lock_screen(&self) -> Result<()> {
        info!("Locking workstation");
        unsafe {
            let result = LockWorkStation();
            if result == 0 {
                return Err(WristKeyError::Platform("LockWorkStation failed".into()));
            }
        }
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        // Windows has no public API to unlock the workstation programmatically
        // without valid credentials. The user must type their password or use
        // Windows Hello. Returning Ok so the flow doesn't break.
        info!("Windows: unlock_screen is a no-op (OS requires manual unlock)");
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(false)
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        info!("Windows credential provider registration not yet implemented");
        Ok(())
    }
}

pub struct MockPlatformSecurity {
    locked: std::sync::Mutex<bool>,
}

impl MockPlatformSecurity {
    pub fn new() -> Self {
        Self {
            locked: std::sync::Mutex::new(false),
        }
    }
}

#[async_trait]
impl PlatformSecurity for MockPlatformSecurity {
    async fn lock_screen(&self) -> Result<()> {
        *self.locked.lock().unwrap() = true;
        Ok(())
    }

    async fn unlock_screen(&self) -> Result<()> {
        *self.locked.lock().unwrap() = false;
        Ok(())
    }

    async fn is_locked(&self) -> Result<bool> {
        Ok(*self.locked.lock().unwrap())
    }

    async fn register_as_authenticator(&self) -> Result<()> {
        Ok(())
    }
}
