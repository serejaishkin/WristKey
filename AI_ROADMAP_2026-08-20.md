# WristKey — AI Roadmap / Handoff checkpoint

**Дата:** 2026-08-20  
**Ветка:** `fix/wristkey-20260818`

## Текущий фокус

Сейчас работаем **только с Wear OS APK, BLE pairing/reconnect и proximity foundation**.

### Credential Provider DLL — ЗАМОРОЖЕНО

Пока не делаем:
- сборку DLL;
- установку/регистрацию DLL;
- тест Windows LogonUI;
- реальный Windows unlock.

Возвращаемся к DLL только отдельной командой.

## Уже сделано

### Wear OS screen awake

В `MainActivity` добавлен `FLAG_KEEP_SCREEN_ON`, чтобы экран WristKey не гас во время интерактивного pairing/challenge flow.

### Persistent pairing

`WristKeyBleService` загружает paired device из `SharedPreferences` при старте и сохраняет address/name после pairing. Этот механизм не ломаем.

### BLE reconnect policy — добавлено

Добавлен:

```text
wear-os/app/src/main/java/com/wristkey/ble/BleReconnectPolicy.kt
```

Policy отделяет известный paired BLE address от нового устройства:

```text
known address
    ↓
reconnect path / no pairing UI

unknown address
    ↓
new pairing flow
```

Добавлены unit tests:

```text
wear-os/app/src/test/java/com/wristkey/ble/BleReconnectPolicyTest.kt
```

**Важно:** policy пока является отдельным безопасным слоем. Фактический `WristKeyBleService.onConnectionStateChange()` ещё нужно подключить к нему, чтобы полностью убрать повторное открытие PairingActivity для известного устройства.

### Proximity RSSI — добавлено

Добавлен:

```text
wear-os/app/src/main/java/com/wristkey/ble/ProximityRssiTracker.kt
```

Состояния:

```text
UNKNOWN → NEAR → PRESENT → SUSPECTED_AWAY → AWAY
```

Есть:
- EMA smoothing RSSI;
- подтверждение близости несколькими samples;
- подтверждение ухода несколькими samples;
- hysteresis через разные near/away thresholds;
- восстановление из `SUSPECTED_AWAY`;
- reset;
- определение резкого изменения RSSI;
- unit tests для основных переходов.

Тесты:

```text
wear-os/app/src/test/java/com/wristkey/ble/ProximityRssiTrackerTest.kt
```

### Важно

`ProximityRssiTracker` пока **ничего не блокирует и не разблокирует**.

RSSI не является authentication и не является доказательством identity.

Целевая схема:

```text
BLE RSSI → ProximityRssiTracker → proximity state
                         ↓
                 crypto verification
                         ↓
                  security action
```

## Следующий этап

1. Подключить `BleReconnectPolicy` непосредственно в `WristKeyBleService`.
2. При disconnect сохранить состояние paired device и разрешить нормальный GATT reconnect без нового pairing.
3. Подключить `ProximityRssiTracker` к реальному RSSI источнику BLE.
4. Обновлять `lastRssi` и proximity state только для debug/UI; никаких auto-lock/unlock.
5. Проверить реальные RSSI samples на Galaxy Watch4.
6. При необходимости откалибровать thresholds/EMA/Kalman после реальных данных.
7. Только после стабильного proximity слоя обсуждать auto-lock/auto-unlock.

## Reference

Proximity reference: `https://proximitylock.app/`

Берём общие идеи filtering/hysteresis/grace/cooldown, но не копируем реализацию.

## Правила

- Не хранить Windows password.
- Не делать BLE внутри Credential Provider.
- Не использовать RSSI как proof identity.
- Не считать MAC адрес криптографической identity.
- Не делать auto-unlock на одном факте близости.
- Не считать build успешным тестом функциональности.
- После каждого существенного изменения: commit → roadmap.
