# WristKey Linux authentication — final architecture

## Local trust boundary

```text
WristKey Watch
   |
   | ECDSA challenge/response
   v
WristKey daemon
   |
   | authenticated local Unix socket
   | SO_PEERCRED + nonce
   v
wristkey-auth
   |
   v
PAM module / native desktop authentication
```

The helper now rejects clients whose Unix peer UID differs from its own UID. The socket is created under `$XDG_RUNTIME_DIR` with mode `0600` and its owner is verified.

## Remaining integration

The socket helper is intentionally only the local trust boundary. A production PAM module must call the helper using the same nonce generated for the verified WristKey transaction and must never treat RSSI, Bluetooth address, or a timestamp file as authentication.

For desktop unlock, the PAM module should use the normal PAM conversation and return success only after the helper confirms the one-time nonce. No password is sent to the Watch.

For login managers that do not use the same PAM stack, the installer should enable the module through the distribution's supported PAM configuration rather than modifying files silently.

## Supported environments to verify

- systemd-logind + GNOME
- systemd-logind + KDE Plasma
- X11 sessions
- Wayland sessions
- common display managers (GDM, SDDM, LightDM where applicable)

No automatic unlock should be enabled by default until each target stack is verified.
