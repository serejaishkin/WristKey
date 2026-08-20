# WristKey macOS auth helper

`wristkey-auth` is a deliberately small local boundary between the verified WristKey daemon and future macOS authentication integration.

## Current flow

```text
Watch
  -> ECDSA challenge/response
  -> daemon verifies public key
  -> helper receives one-shot local proof
  -> proof is consumed once
  -> future native authentication adapter
```

The helper does **not** receive the Watch private key and does **not** receive the user's password.

The current proof is a short-lived local marker. It is an integration scaffold, not a security boundary by itself. The daemon must only issue it after successful cryptographic verification, and the production implementation must replace the marker with an authenticated IPC channel (Unix domain socket / launchd service) and a signed, nonce-bound proof.

Do not use this helper to simulate keyboard password entry. The next macOS phase should integrate with the appropriate native authentication service and keep the credential in Keychain.
