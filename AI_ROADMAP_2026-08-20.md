# WristKey — AI Roadmap / Handoff checkpoint

**Дата: 2026-08-20**  
**Ветка: `fix/wristkey-20260818`**

## Текущий этап

### Сделано в Git

- Wear OS `MainActivity` держит экран включённым через `FLAG_KEEP_SCREEN_ON` во время открытой Activity.
- Pairing сохраняется на Wear OS.
- Повторное подключение известного paired PC не должно открывать новый Pairing UI.
- `ProximityRssiTracker` реализует smoothing/hysteresis state machine.
- Desktop `ConnectionManager` переиспользует активное BLE-соединение и `read_rssi()`.
- Desktop GUI — **Tauri**: frontend вызывает Rust backend через `invoke()`, BLE/RSSI логика остаётся в Rust.
- Есть Tauri proximity diagnostics.
- Есть общий `rssi_calibration` core algorithm с median/P10/P90/away threshold.
- Linux platform имеет real `loginctl` lock state и lock-session integration.
- macOS platform использует device-scoped Keychain для локального password credential.
- macOS screen lock использует `CGSession -suspend`.
- Добавлен отдельный `wristkey-auth` helper.
- `wristkey-auth` теперь имеет отдельный Unix-domain socket boundary с mode `0600`.
- Добавлен per-user launchd template для helper.
- macOS password не передаётся в BLE, Watch, Tauri frontend или daemon responses.

## macOS architecture — current

```text
Watch
  ↓
ECDSA challenge/response
  ↓
WristKey daemon
  ↓
verified assertion
  ↓
launchd → wristkey-auth
  ↓
0600 Unix socket
  ↓
MacosVault / Keychain
  ↓
macOS native authentication adapter
```

Пароль хранится только локально в Keychain и привязан к конкретному `device_id`.

### Что сознательно НЕ делаем

- не отправляем пароль на часы;
- не передаём пароль через BLE;
- не отдаём пароль Tauri frontend;
- не используем WebView для системной аутентификации;
- не эмулируем клавиатуру как основной механизм unlock;
- не считаем RSSI authentication.

### Остаток macOS

Архитектурная сторона практически закрыта. Остались только platform-specific authentication details:

1. заменить временный timestamp proof на nonce-bound signed proof;
2. helper должен независимо проверять assertion/authorization token перед доступом к Keychain credential;
3. определить и реализовать native macOS authentication adapter для нужного сценария Lock Screen/Login Window;
4. собрать на реальном macOS и проверить Keychain/launchd/lock state;
5. только после реального теста включать auto-unlock policy.

**Важно:** macOS Lock Screen и Login Window — разные security paths. Нельзя считать `CGSession -suspend` доказательством того, что автоматический login/unlock уже работает.

## Linux

Следующий блок после закрытия macOS:

1. signed one-time proof вместо timestamp file;
2. PAM install package;
3. GNOME/KDE + Wayland/X11 matrix;
4. crypto challenge/response в PAM.

## RSSI calibration

Tauri UI wiring для 30–60 sample collection остаётся отдельным следующим шагом.

## DLL

Credential Provider DLL **не собирать и не тестировать**, пока пользователь отдельно не даст команду.

## Security invariants

- RSSI → proximity evidence.
- ECDSA challenge/response → authentication.
- MAC/address → transport identity only.
- Password → local platform credential store only.
- Unlock → только после cryptographic verification + native platform authentication.
