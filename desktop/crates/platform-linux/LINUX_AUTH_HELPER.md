# Linux authentication helper

The Linux side now has a dedicated `wristkey-auth` process boundary.

```text
Watch
  -> ECDSA challenge/response
  -> WristKey daemon verifies
  -> local Unix socket
  -> wristkey-auth
  -> PAM integration
```

The helper never receives the user's password and must not simulate keyboard input.

The current socket protocol is intentionally a scaffold: production wiring must enforce `SO_PEERCRED`, per-user `0700/0600` permissions, a nonce bound to the verified challenge, and one-time consumption before PAM is enabled for real unlock.

The existing timestamp proof is not considered an authentication boundary and should not be used as a substitute for the authenticated IPC path.
