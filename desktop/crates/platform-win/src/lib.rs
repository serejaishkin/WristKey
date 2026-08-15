use wristkey_core::KeyProtector;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
};
use windows::Win32::Foundation::HLOCAL;
use windows::Win32::System::Memory::LocalFree;

/// Windows DPAPI protector. Encrypts pairingKey with the current user credential.
/// Works on any Windows 10/11 without TPM (TPM is used automatically if available).
pub struct WindowsKeyProtector;

impl KeyProtector for WindowsKeyProtector {
    fn protect(&self, plaintext: &[u8]) -> Vec<u8> {
        unsafe {
            let mut data_in = CRYPT_INTEGER_BLOB {
                cbData: plaintext.len() as u32,
                pbData: plaintext.as_ptr() as *mut u8,
            };
            let mut data_out = CRYPT_INTEGER_BLOB::default();

            CryptProtectData(
                &mut data_in,
                None,       // no description
                None,       // no optional entropy
                None,       // reserved
                None,       // no prompt struct
                0,          // flags
                &mut data_out,
            ).expect("CryptProtectData failed");

            let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
            let result = slice.to_vec();
            let _ = LocalFree(HLOCAL(data_out.pbData as *mut std::ffi::c_void));
            result
        }
    }

    fn unprotect(&self, ciphertext: &[u8]) -> Option<Vec<u8>> {
        unsafe {
            let mut data_in = CRYPT_INTEGER_BLOB {
                cbData: ciphertext.len() as u32,
                pbData: ciphertext.as_ptr() as *mut u8,
            };
            let mut data_out = CRYPT_INTEGER_BLOB::default();

            CryptUnprotectData(
                &mut data_in,
                None,
                None,
                None,
                None,
                0,
                &mut data_out,
            ).ok()?;

            let slice = std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize);
            let result = slice.to_vec();
            let _ = LocalFree(HLOCAL(data_out.pbData as *mut std::ffi::c_void));
            Some(result)
        }
    }
}

/// Convenience constructor.
pub fn create_protector() -> WindowsKeyProtector {
    WindowsKeyProtector
}
