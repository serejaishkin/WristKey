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

## Новый этап — proximity foundation

Добавлены:

```text
wear-os/app/src/main/java/com/wristkey/ble/ProximityEngine.kt
wear-os/app/src/main/java/com/wristkey/ble/ProximityEngineTest.kt
```

Состояния:

```text
UNKNOWN → NEAR → PRESENT → SUSPECTED_AWAY → AWAY
```

Есть:
- подтверждение близости несколькими RSSI samples;
- несколько samples для ухода;
- hysteresis между near/away thresholds;
- восстановление из `SUSPECTED_AWAY`;
- reset;
- последний RSSI;
- unit tests для основных переходов.

### Важно

`ProximityEngine` пока **ничего не блокирует и не разблокирует**.

RSSI не является authentication и не является доказательством identity.

Целевая схема:

```text
BLE RSSI → ProximityEngine → proximity state
                         ↓
                 crypto verification
                         ↓
                  security action
```

## Следующий этап

1. Проверить текущую persistent pairing логику по коду и не вызывать pairing UI для уже известного device.
2. Добавить контролируемый BLE reconnect/recovery после disconnect.
3. Подключить `ProximityEngine` к BLE RSSI только для debug/state.
4. После реальных RSSI samples добавить EMA/Kalman filtering при необходимости.
5. Только после стабильного proximity слоя обсуждать auto-lock/auto-unlock.

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
