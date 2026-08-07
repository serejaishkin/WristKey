# WristKey Protocol v1.0.0

## GATT Service
- Service UUID: `a1b2c3d4-e5f6-7890-abcd-ef1234567890`
- CHALLENGE (write) — PC sends 16-byte nonce
- RESPONSE (notify) — Watch sends Ed25519 signature
- STATUS (read/notify) — connection health

## Flow
1. PC writes nonce to CHALLENGE
2. Watch signs `nonce || timestamp || user_present_flag`
3. Watch notifies signature on RESPONSE
4. PC verifies against stored public key

## Auto-Lock
- RSSI < baseline - 15dBm for 30s → lock
