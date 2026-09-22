# WristKey Protocol v1.0.0

## Crypto
- ECDSA P-256 (SHA-256withECDSA), ключ в AndroidKeyStore.
- Public key: SEC1 uncompressed `04 || X(32) || Y(32)`.
- Signature: raw `R || S` (64 байта), из Android DER-сигнатуры конвертится на часах.

## GATT Service
- Service UUID: `a1b2c3d4-e5f6-7890-abcd-ef1234567890`
- CHALLENGE (write) — PC sends 16-byte nonce
- RESPONSE (notify) — Watch sends ECDSA signature
- STATUS (read/notify) — connection health

## Flow (BLE)
1. PC writes nonce to CHALLENGE
2. Watch signs `nonce || timestamp || user_present_flag`
3. Watch notifies signature on RESPONSE
4. PC verifies against stored public key

## MSA binding / LAN mode (HTTP)
Для ПК без Bluetooth — `server/crates/msa` (wristkey-msa), режимы `local`/`lan`:
- `POST /api/v1/challenge` → `{"nonce_b64","ttl_secs"}` (одноразовый nonce, TTL 300 с)
- `POST /api/v1/register` — подпись P-256 над `b"wristkey-msa/register/v1" || nonce`:
  `{"pc_name","watch_pubkey_b64","msa_account","nonce_b64","signature_b64"}`
- `GET /api/v1/status/{wristkey_id}`
- `GET /api/v1/health` — публичный.
- `lan` требует `WRISTKEY_MSA_TOKEN` (`Authorization: Bearer`), часы получают его при спаривании.

## Auto-Lock
- RSSI < baseline - 15dBm for 30s → lock
