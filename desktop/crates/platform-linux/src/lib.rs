use wristkey_core::KeyProtector;

/// Linux "protector": no additional encryption beyond file permissions (chmod 600).
/// The JSON file itself is protected by `Storage::save` via `std::fs::set_permissions(0o600)`.
pub struct LinuxKeyProtector;

impl KeyProtector for LinuxKeyProtector {
    fn protect(&self, key: &[u8]) -> Vec<u8> {
        key.to_vec()
    }

    fn unprotect(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.to_vec())
    }
}

pub fn create_protector() -> LinuxKeyProtector {
    LinuxKeyProtector
}
