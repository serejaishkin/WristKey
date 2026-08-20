# WristKey macOS unlock design

## Principle

The Watch is a cryptographic proof device. The Mac owns the login credential.

```text
Watch private key
    ↓ ECDSA challenge/response
WristKey daemon
    ↓ verify
UnlockGate
    ↓
macOS Keychain (device-scoped credential)
    ↓
platform authentication helper
```

The password MUST NOT cross BLE, enter the Watch, or be exposed to the Tauri frontend.

## Current implementation

- `MacosVault` stores the credential in Keychain using a service scoped to `device_id`.
- `MacosSecurity` provides screen lock and lock-state detection.
- `UnlockGate` refuses unlock unless the Watch verification result is true.
- The old global `pairing_key` model is removed from the macOS vault layer.

## Required next wiring

1. Verify the ECDSA response with `SessionManager`.
2. Pass only the boolean verified result into `UnlockGate`.
3. Expose Tauri commands for setting/removing/checking the local credential without returning it to JS.
4. Create a one-time local proof after successful Watch verification.
5. Consume that proof from the macOS authentication helper/PAM integration.
6. Test normal Lock Screen separately from Login Window.

## Explicit non-goals

- No password transmission to Watch.
- No password over BLE.
- No password in logs/config files.
- No unlock based on RSSI alone.
- No fake keyboard typing as the primary authentication mechanism.

Keyboard automation can only be a separately gated fallback if a specific macOS version makes the native authentication path impossible.
