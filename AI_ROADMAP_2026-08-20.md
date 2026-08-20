# WristKey — AI Roadmap / Handoff checkpoint

**Дата: 2026-08-20**  
**Ветка: `fix/wristkey-20260818`**

## Текущий этап

### Сделано в Git

- Wear OS `MainActivity` держит экран включённым через `FLAG_KEEP_SCREEN_ON` во время открытой Activity. Это предназначено прежде всего для pairing/challenge UI.
- Pairing сохраняется в `SharedPreferences` на Wear OS: address, name и pairing key.
- При повторном подключении уже известного paired PC Wear OS больше не должен открывать новый Pairing UI.
- Добавлен `ProximityRssiTracker` со state machine:
  `UNKNOWN → NEAR → PRESENT → SUSPECTED_AWAY → AWAY`.
- Есть smoothing, hysteresis-пороги, несколько подтверждающих samples и recovery.
- RSSI не считается authentication.
- Desktop `ConnectionManager` уже переиспользует существующее BLE connection и перед reconnect останавливает scan.
- Desktop daemon теперь получает RSSI **через живое подключение** `ConnectionManager`/`BleAdapter::read_rssi()`, а не из scan advertisement. Это важно для реального RSSI именно текущего paired Watch и не требует второго BLE adapter.
- Desktop daemon и BLE abstraction уже имеют `read_rssi()`.

## Важное архитектурное решение: где считать proximity

После проверки текущего BLE протокола выяснилось, что Wear OS работает как GATT server. Android `BluetoothGattServerCallback` не предоставляет peer RSSI в `onConnectionStateChange`.

Поэтому не надо пытаться делать:

```text
Watch GATT server → сам узнаёт RSSI PC
```

Правильная схема:

```text
Windows PC / btleplug
        ↓
active GATT connection
        ↓
read_rssi()
        ↓
proximity filter
        ↓
NEAR / PRESENT / SUSPECTED_AWAY / AWAY
```

Watch остаётся endpoint/authenticator. PC является стороной proximity measurement.

Это также лучше соответствует реальной физике BLE: RSSI измеряется со стороны принимающего радио.

## Текущий статус auto-lock/auto-unlock

**НЕ включать новые автоматические действия только из RSSI на этом этапе.**

RSSI должен быть диагностическим/proximity signal. Перед включением auto-lock/auto-unlock необходимо отдельно завершить policy layer и сохранить правило:

```text
RSSI → proximity evidence
ECDSA challenge/response → identity/authentication
```

Никакого unlock только потому, что RSSI высокий.

## Следующий шаг

1. Собрать/проверить desktop workspace после изменения live RSSI.
2. Убедиться, что `read_rssi()` стабильно работает на реальном Galaxy Watch4.
3. Вывести в diagnostics текущие значения:
   - raw RSSI;
   - filtered RSSI;
   - proximity state;
   - paired device address.
4. Только после этого настраивать baseline/calibration.
5. Затем отдельно подключать proximity policy к crypto flow.

## DLL

Credential Provider DLL **не собирать и не тестировать**, пока пользователь отдельно не даст команду.

## Безопасность

- Windows password не хранить.
- RSSI не является доказательством identity.
- MAC/address не является криптографической identity.
- BLE discovery не является authentication.
- Для unlock обязателен crypto challenge/response.
- Не создавать дополнительные BLE adapters для proximity без необходимости: текущий daemon должен использовать один живой adapter/ConnectionManager.
