//! macOS unlock flow contract.
//! The Watch proves possession of its private key; the Mac keeps the login credential locally.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnlockDecision {
    Denied,
    Verified,
}

pub struct UnlockGate;

impl UnlockGate {
    /// The credential is deliberately not an argument here. A caller first verifies the
    /// ECDSA challenge with wristkey-core, then the platform helper may access Keychain.
    pub fn after_watch_verification(verified: bool) -> UnlockDecision {
        if verified { UnlockDecision::Verified } else { UnlockDecision::Denied }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn rejects_unverified_watch() { assert_eq!(UnlockGate::after_watch_verification(false), UnlockDecision::Denied); }
    #[test] fn accepts_verified_watch() { assert_eq!(UnlockGate::after_watch_verification(true), UnlockDecision::Verified); }
}
