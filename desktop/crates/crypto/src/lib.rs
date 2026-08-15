use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use rand::RngCore;

pub type Key = [u8; 32];

/// Generate a random 256-bit key.
pub fn generate_key() -> Key {
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    key
}

/// AES-256-GCM encrypt. Prepends 12-byte nonce.
pub fn encrypt(plaintext: &[u8], key: &Key) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(key).expect("valid key length");
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let mut ciphertext = cipher.encrypt(&nonce, plaintext).expect("encryption failed");
    let mut result = nonce.to_vec();
    result.append(&mut ciphertext);
    result
}

/// AES-256-GCM decrypt. Expects 12-byte nonce prefix.
pub fn decrypt(ciphertext: &[u8], key: &Key) -> Option<Vec<u8>> {
    if ciphertext.len() < 12 {
        return None;
    }
    let nonce = Nonce::from_slice(&ciphertext[..12]);
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    cipher.decrypt(nonce, &ciphertext[12..]).ok()
}

/// Encrypt password string → base64.
pub fn encrypt_password(password: &str, pairing_key: &Key) -> String {
    use base64::{Engine as _, engine::general_purpose};
    general_purpose::STANDARD.encode(encrypt(password.as_bytes(), pairing_key))
}

/// Decrypt base64 password → plaintext string.
pub fn decrypt_password(b64: &str, pairing_key: &Key) -> Option<String> {
    use base64::{Engine as _, engine::general_purpose};
    let bytes = general_purpose::STANDARD.decode(b64).ok()?;
    let plain = decrypt(&bytes, pairing_key)?;
    String::from_utf8(plain).ok()
}
