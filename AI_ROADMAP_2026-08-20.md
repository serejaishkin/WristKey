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

## Важное архитектурное решение: где считать proximity

Wear OS работает как GATT server. Android `BluetoothGattServerCallback` не предоставляет peer RSSI в `onConnectionStateChange`.

Правильная схема:

```text
Windows PC / btleplug
        ↓
active GATT connection
        ↓
read_rssi()
        ↓
Tauri Rust command
        ↓
GUI diagnostics / proximity filter
```

Watch остаётся endpoint/authenticator. PC является стороной proximity measurement.

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
btleplug / Windows BLE
```

Не использовать Web Bluetooth API для WristKey BLE.

## Текущий статус auto-lock/auto-unlock

**Не добавлять новые автоматические действия только из RSSI.**

RSSI — proximity evidence. Crypto challenge/response — authentication.

```text
RSSI → proximity evidence
ECDSA challenge/response → identity/authentication
```

Никакого unlock только потому, что RSSI высокий.

## Следующий шаг

1. Проверить сборку desktop/Tauri после live RSSI integration.
2. Проверить `get_proximity_status` на реальном Galaxy Watch4.
3. Собрать несколько минут RSSI в разных положениях/расстояниях.
4. Настроить baseline/calibration по реальным данным.
5. Проверить reconnect: diagnostics → disconnect → reconnect без нового pairing.
6. Только после этого подключать proximity policy к crypto flow.

## DLL

Credential Provider DLL **не собирать и не тестировать**, пока пользователь отдельно не даст команду.

## Безопасность

- Windows password не хранить в BLE/proximity слое.
- RSSI не является доказательством identity.
- MAC/address не является криптографической identity.
- BLE discovery не является authentication.
- Для unlock обязателен crypto challenge/response.
- Не создавать дополнительные BLE adapters для proximity без необходимости.
