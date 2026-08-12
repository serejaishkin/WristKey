//! ECDSA P-256 verification for WristKey

use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("invalid public key format")]
    InvalidPublicKey,
    #[error("invalid signature format")]
    InvalidSignature,
    #[error("signature verification failed")]
    VerificationFailed,
}

/// Verify ECDSA P-256 signature (raw 64-byte r||s)
///
/// # Arguments
/// * `public_key` — SEC1 uncompressed public key (65 bytes: 0x04 || X || Y)
/// * `message` — original payload (nonce + timestamp + user_present_byte)
/// * `signature_raw` — raw 64-byte signature (r||s)
///
/// NOTE: p256::ecdsa::VerifyingKey::verify internally hashes with SHA256.
/// Do NOT pre-hash the message — that would cause double-hashing.
pub fn verify_ecdsa_p256(
    public_key: &[u8],
    message: &[u8],
    signature_raw: &[u8],
) -> Result<bool, CryptoError> {
    if signature_raw.len() != 64 {
        return Err(CryptoError::InvalidSignature);
    }

    let verifying_key = VerifyingKey::from_sec1_bytes(public_key)
        .map_err(|_| CryptoError::InvalidPublicKey)?;

    let sig = Signature::from_slice(signature_raw)
        .map_err(|_| CryptoError::InvalidSignature)?;

    // p256::ecdsa internally uses SHA256 — pass raw message, not a hash
    verifying_key.verify(message, &sig)
        .map_err(|_| CryptoError::VerificationFailed)?;

    Ok(true)
}
