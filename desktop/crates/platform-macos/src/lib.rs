use wristkey_core::vault::KeyProtector;
use security_framework::os::macos::keychain::SecKeychain;

const SERVICE: &str = "com.wristkey.pairing";
const ACCOUNT: &str = "pairing_key";

pub struct MacosKeyProtector;

impl KeyProtector for MacosKeyProtector {
    fn protect(&self, key: &[u8]) -> Vec<u8> {
        let _ = SecKeychain::default().and_then(|kc| kc.find_generic_password(SERVICE, ACCOUNT)).and_then(|(_, item)| item.delete());
        let _ = SecKeychain::default().and_then(|kc| kc.add_generic_password(SERVICE, ACCOUNT, key));
        vec![]
    }
    fn unprotect(&self, _data: &[u8]) -> Option<Vec<u8>> {
        let keychain = SecKeychain::default().ok()?;
        let (password, _) = keychain.find_generic_password(SERVICE, ACCOUNT).ok()?;
        Some(password.as_ref().to_vec())
    }
}

pub fn create_protector() -> MacosKeyProtector { MacosKeyProtector }
