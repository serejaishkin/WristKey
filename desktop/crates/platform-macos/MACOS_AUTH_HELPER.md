# WristKey macOS authentication helper

The macOS side is split into three boundaries:

```text
Tauri / daemon
      │
      │ verified WristKey challenge
      ▼
launchd → wristkey-auth
      │
      ├── 0600 Unix socket
      ├── device-scoped Keychain credential
      └── native macOS authentication adapter
```

## Implemented

- `MacosVault` stores the login password in macOS Keychain under a service scoped to the WristKey device ID.
- `wristkey-auth` is a separate executable, not part of the Tauri WebView.
- A per-user launchd plist keeps the helper available.
- IPC uses a Unix domain socket with filesystem mode `0600`.
- Proofs are short-lived (5 seconds) and consumed once.
- The helper never sends the password back to the daemon or Watch.
- The daemon does not synthesize keyboard input.
- Screen locking uses the macOS `CGSession` mechanism.

## Final native-auth boundary

The remaining adapter is deliberately isolated from BLE, Tauri and the Watch protocol. macOS screen lock and Login Window authentication are different security layers, so the helper is the only component allowed to bridge from a verified WristKey assertion to a native macOS authentication mechanism.

Before production unlock, the temporary timestamp proof must be replaced by a nonce-bound proof over the Unix socket, and the helper must independently validate the WristKey challenge/signature or a signed one-time authorization token. The password remains only in Keychain.

## Installation

The launchd plist is a template under `launchd/`. Installation belongs to the package/installer layer, not automatic Tauri startup. The final package should place the helper under `/Library/Application Support/WristKey/` and load the user LaunchAgent in the user's context.
