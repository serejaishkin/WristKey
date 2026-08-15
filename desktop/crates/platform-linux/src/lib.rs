use wristkey_core::vault::KeyProtector;

pub struct LinuxKeyProtector;

impl KeyProtector for LinuxKeyProtector {
    fn protect(&self, key: &[u8]) -> Vec<u8> { key.to_vec() }
    fn unprotect(&self, data: &[u8]) -> Option<Vec<u8>> { Some(data.to_vec()) }
}

pub fn create_protector() -> LinuxKeyProtector { LinuxKeyProtector }
