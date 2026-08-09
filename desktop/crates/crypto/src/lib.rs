//! ECDSA P-256 verification for WristKey

use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use sha2::{Digest, Sha256};

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
/// * `public_key_der` — X.509/SPKI encoded public key (from Android Keystore)
/// * `message` — original payload (nonce + timestamp + user_present_byte)
/// * `signature_raw` — raw 64-byte signature (r||s)
pub fn verify_ecdsa_p256(
    public_key_der: &[u8],
    message: &[u8],
    signature_raw: &[u8],
) -> Result<bool, CryptoError> {
    if signature_raw.len() != 64 {
        return Err(CryptoError::InvalidSignature);
    }

    let verifying_key = VerifyingKey::from_sec1_bytes(public_key_der)
        .map_err(|_| CryptoError::InvalidPublicKey)?;

    let sig = Signature::from_slice(signature_raw)
        .map_err(|_| CryptoError::InvalidSignature)?;

    let hash = Sha256::digest(message);

    verifying_key.verify(&hash, &sig)
        .map_err(|_| CryptoError::VerificationFailed)?;

    Ok(true)
}
