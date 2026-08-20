# WristKey — AI Roadmap / Handoff checkpoint

**Дата: 2026-08-20**  
**Ветка: `fix/wristkey-20260818`**

## Текущий этап

### Сделано в Git

- Wear OS `MainActivity` держит экран включённым через `FLAG_KEEP_SCREEN_ON` во время открытой Activity. Это предназначено прежде всего для pairing/challenge UI.
- Pairing сохраняется в `SharedPreferences` на Wear OS: address, name и pairing key.
- При повторном подключении уже известного paired PC Wear OS больше не должен открывать новый Pairing UI.
- Добавлен `ProximityRssiTracker` со state machine: `UNKNOWN → NEAR → PRESENT → SUSPECTED_AWAY → AWAY`.
- Есть smoothing, hysteresis-пороги, несколько подтверждающих samples и recovery.
- RSSI не считается authentication.
- Desktop `ConnectionManager` переиспользует существующее BLE connection и перед reconnect останавливает scan.
- Desktop daemon получает RSSI через живое подключение `ConnectionManager`/`BleAdapter::read_rssi()`, а не из scan advertisement.
- Desktop daemon и BLE abstraction имеют `read_rssi()`.
- **Desktop GUI архитектурно является Tauri:** frontend вызывает Rust backend через `window.__TAURI__.core.invoke()`. BLE/RSSI логика не выносится в Web Bluetooth/JavaScript.
- Добавлена Tauri-команда `get_proximity_status`: она получает RSSI через общий `ConnectionManager` и активное BLE-соединение.
- Tauri GUI diagnostics теперь показывает raw RSSI, filtered RSSI, baseline, delta, address и proximity state.
- GUI больше не использует BLE scan для live RSSI diagnostics.
- Tauri и background daemon используют **один общий `ConnectionManager`**, чтобы диагностика и daemon переиспользовали одно соединение.
- Добавлен `desktop/crates/core/src/rssi_calibration.rs`: общий алгоритм калибровки с median, P10, P90 и away threshold, с unit tests.
- Linux platform теперь умеет получать реальное состояние lock через `loginctl show-session ... LockedHint`; unlock намеренно не эмулирует ввод пароля.
- macOS platform переведён на системный `CGSession -suspend` для lock; добавлена попытка определения lock state через CoreGraphics session hint; pairing secret хранится в Keychain.

## Важное архитектурное решение: где считать proximity

Wear OS работает как GATT server. Android `BluetoothGattServerCallback` не предоставляет peer RSSI в `onConnectionStateChange`.

Правильная схема:

```text
Windows / macOS / Linux BLE client
        ↓
active GATT connection
        ↓
read_rssi()
        ↓
Tauri Rust / daemon
        ↓
calibration + proximity filter
```

Watch остаётся endpoint/authenticator. Desktop OS является стороной proximity measurement.

## Tauri GUI правило

```text
Tauri WebView
    ↓ invoke()
Rust Tauri command
    ↓
ConnectionManager
    ↓
BleAdapter
    ↓
btleplug / native BLE backend
```

Не использовать Web Bluetooth API для WristKey BLE.

## RSSI calibration

Цель калибровки — не выбрать один случайный RSSI, а получить профиль конкретных часов на конкретном BLE adapter/PC:

```text
samples
  ↓
validation
  ↓
sort
  ├── P10
  ├── median
  └── P90
       ↓
away_threshold = median - margin
```

Минимум: 10 валидных samples. В production UI лучше собирать 30–60 samples.

**Важно:** calibration profile должен быть привязан к device identity/adapter context. Не использовать один baseline для всех Watch или всех ПК.

`rssi_calibration.rs` сейчас является чистым алгоритмическим слоем; Tauri UI wiring для длительного sample collection остаётся следующим шагом.

## Linux

Текущее направление:

```text
Tauri
 ↓
Rust daemon
 ↓
BLE
 ↓
PlatformSecurity(Linux)
 ↓
loginctl / PAM
```

Уже есть:

- `loginctl lock-session`;
- реальный `LockedHint`;
- PAM entry point;
- user-owned proof file с коротким TTL.

Следующий Linux этап:

1. вынести PAM proof из простого timestamp-файла в подписанный/одноразовый proof;
2. сделать install package для PAM без автоматической установки из приложения;
3. проверить GNOME/KDE/Wayland/X11;
4. затем подключать crypto challenge/response к PAM.

Не считать текущий `.last_auth` полноценной криптографической authentication boundary.

## macOS

Текущее направление:

```text
Tauri
 ↓
Rust daemon
 ↓
BLE
 ↓
Keychain + macOS session APIs
```

Уже есть:

- Keychain-backed pairing secret;
- системный lock через `CGSession -suspend`;
- lock-state probe;
- PAM-compatible entry points как задел.

Следующий macOS этап:

1. привязать Keychain item к конкретному WristKey device, а не глобальному `pairing_key`;
2. определить стабильный способ account/user mapping;
3. заменить временный proof-file flow на signed one-time authentication proof;
4. проверить macOS login/unlock ограничения отдельно от обычного screen lock;
5. только после этого делать полноценный authentication integration.

**Не обещать программный unlock macOS простым вызовом API:** screen lock и login authentication — разные уровни.

## Текущий статус auto-lock/auto-unlock

**Не добавлять новые автоматические действия только из RSSI.**

RSSI — proximity evidence. Crypto challenge/response — authentication.

```text
RSSI → proximity evidence
ECDSA challenge/response → identity/authentication
```

Никакого unlock только потому, что RSSI высокий.

## Следующий шаг

1. Подключить `rssi_calibration` к Tauri command.
2. Добавить в Tauri GUI режим `Calibrate` на 30–60 samples.
3. Показывать median/P10/P90 и сохранять baseline для конкретного Watch.
4. Проверить сборку desktop/Tauri.
5. Проверить calibration на реальном Galaxy Watch4.
6. Проверить reconnect: diagnostics → disconnect → reconnect без нового pairing.
7. После этого отдельно продолжить Linux PAM и macOS authentication integration.
8. Только затем подключать proximity policy к crypto flow.

## DLL

Credential Provider DLL **не собирать и не тестировать**, пока пользователь отдельно не даст команду.

## Безопасность

- Windows password не хранить в BLE/proximity слое.
- RSSI не является доказательством identity.
- MAC/address не является криптографической identity.
- BLE discovery не является authentication.
- Для unlock обязателен crypto challenge/response.
- Не создавать дополнительные BLE adapters для proximity без необходимости.
