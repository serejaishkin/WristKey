# WristKey Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅ Active development |

## Threat Model

### Assets
1. **Private key** (Android Keystore / desktop software fallback)
2. **Paired device list** (sled database, encrypted at rest)
3. **BLE communication** (challenge-response nonce)

### Threats & Mitigations

| Threat | Severity | Mitigation |
|--------|----------|------------|
| BLE eavesdropping | Medium | LE Secure Connections (Mode 1 Level 4) |
| Relay attack | High | User-present tap required + motion detection |
| Key extraction | Critical | Android Keystore hardware-backed keys |
| Replay attack | Medium | 16-byte nonce, 30-second timeout |
| Malicious pairing | Medium | Mutual signature verification |
| Database tampering | Low | Sled integrity + OS-level access controls |

### Attack Scenarios

#### 1. Relay Attack (NFC/BLE proxy)
**Attack:** Attacker relays BLE signal from victim's watch to distant PC.
**Defense:** 
- `user_present` flag requires physical tap on watch screen
- `MotionDetector` verifies watch is on wrist and moving
- Both must be true for signature

#### 2. Stolen Watch
**Attack:** Attacker steals watch, tries to unlock PC.
**Defense:**
- Watch must be paired (public key exchange during pairing)
- No unlock without PC sending valid challenge first
- Auto-lock on RSSI drop prevents "leave watch near PC" attack

#### 3. Compromised Desktop
**Attack:** Malware on PC extracts paired keys.
**Defense:**
- Private keys NEVER stored on PC (only public keys)
- Keys in `~/.local/share/WristKey/` protected by OS permissions

### Reporting Vulnerabilities

Email: security@wristkey.dev (placeholder)
Do NOT open public issues for security bugs.

## Audit Checklist

```bash
# Run before each release
cargo audit
cargo deny check
cargo test --workspace
cargo clippy --workspace -- -D warnings
