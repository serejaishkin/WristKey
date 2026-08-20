# WristKey — AI Roadmap / Handoff checkpoint

**Дата контрольной точки: 2026-08-20**  
**Рабочая ветка: `fix/wristkey-20260818`**

> Этот файл — актуальная контрольная точка. Перед изменениями сверять фактический Git, этот roadmap и последний чат. Не считать задачу готовой только по успешной сборке.

---

## 1. Текущий статус

### Подтверждено пользователем

- Wear OS APK собирается.
- Desktop/PC часть собирается.
- Pairing с Galaxy Watch4 работает.
- На часах появляется `Pairing request`, пользователь может нажать `Allow`/подтвердить pairing.
- BLE/GATT subscription-проблема `HRESULT(0x80650003)` была исправлена добавлением CCCD.
- Desktop storage переведён с временного `MemoryStorage` на persistent storage, чтобы paired watch не забывались после закрытия программы.
- 2026-08-20 в Wear OS `MainActivity` добавлен `FLAG_KEEP_SCREEN_ON`: экран часов не должен автоматически гаснуть, пока основная WristKey Activity открыта. Это особенно важно во время pairing и подтверждения challenge.

### Важное уточнение

- **Credential Provider DLL пользователь пока НЕ собирал и НЕ тестировал.**
- Наличие `.sln`/`.vcxproj` означает только готовность проекта к сборке, а не подтверждённую работу DLL.
- Реальный Windows unlock пока НЕ подтверждён.

### Пока НЕ подтверждено

- Silent reconnect после перезапуска desktop.
- Сохранение pairing на реальном сценарии `pair → close → restart → reconnect`.
- Сборка `WristKeyCredentialProvider.dll` пользователем через Visual Studio.
- Появление WristKey tile на реальном Windows Lock Screen.
- Authenticated IPC daemon ↔ Credential Provider.
- Полный unlock Windows через Credential Provider.
- Автоматическое определение Windows account по public key часов.

---

## 2. Целевая архитектура

```text
                 Windows LogonUI
                       │
                       ▼
       WristKeyCredentialProvider.dll
                       │
              authenticated IPC
                       │
                       ▼
                 WristKey daemon
                       │
                 BLE + crypto
                       │
                       ▼
                 Galaxy Watch
```

Credential Provider должен оставаться тонким. BLE scan/GATT/crypto не должны жить внутри DLL.

Daemon отвечает за:

- BLE discovery/connection;
- pairing;
- challenge/response;
- public-key verification;
- persistent paired devices;
- proximity engine;
- связь с Credential Provider.

Credential Provider отвечает за:

- Windows LogonUI;
- account tiles;
- выбранную Windows account;
- ожидание результата daemon;
- `CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION`.

---

## 3. Multi-account модель

Одна универсальная DLL должна работать с разными Windows accounts:

```text
Windows PC
│
├── Сергей → Galaxy Watch A / public key A
├── Иван   → Galaxy Watch B / public key B
└── Анна   → Galaxy Watch C / public key C
```

Windows passwords в WristKey не хранятся.

Основная enrollment-модель:

```text
обычный Windows login / admin enrollment
        ↓
выбор Windows account
        ↓
pair Galaxy Watch
        ↓
получение public key
        ↓
привязка public key ↔ SID
```

Будущий automatic selection:

```text
Watch detected
      ↓
public key matched
      ↓
SID/account identified
      ↓
challenge
      ↓
Allow on watch
      ↓
Windows unlock
```

Ручной account tile должен остаться доступным.

---

## 4. Credential Provider

Каталог:

```text
windows-credential-provider/
├── WristKeyCredentialProvider.sln
├── WristKeyCredentialProvider.vcxproj
├── WristKeyCredentialProvider.def
├── CMakeLists.txt
├── README.md
├── install.ps1
├── include/Provider.h
└── src/
    ├── Credential.cpp
    ├── Guid.cpp
    ├── Provider.cpp
    └── exports.cpp
```

Уже реализовано:

- native C++ x64 Credential Provider;
- COM factory;
- CLSID registration/unregistration;
- `CPUS_LOGON`;
- `CPUS_UNLOCK_WORKSTATION`;
- enumeration enabled local accounts;
- отдельная WristKey credential для каждой перечисленной local account;
- Visual Studio `.sln` + `.vcxproj`;
- CMake остаётся дополнительным workflow.

Пока `GetSerialization()` намеренно возвращает `CPGSR_NO_CREDENTIAL_NOT_FINISHED`. Поэтому DLL пока не делает Windows unlock.

### Следующая проверка DLL

```text
D:\GitHub\WristKey\windows-credential-provider\WristKeyCredentialProvider.sln

Configuration: Release
Platform: x64
Build → Build Solution

Ожидаемый результат:
windows-credential-provider\build\Release\WristKeyCredentialProvider.dll
```

**Не устанавливать непроверенную Credential Provider DLL на единственную рабочую Windows без рабочего fallback login. Сначала тестовая Windows/VM.**

---

## 5. Wear OS — screen awake

### Сделано 2026-08-20

В `wear-os/app/src/main/java/com/wristkey/MainActivity.kt` добавлен:

```kotlin
window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
```

Это предотвращает обычное автоматическое гашение дисплея, пока MainActivity WristKey находится на переднем плане.

Цель:

```text
открыт WristKey
      ↓
pairing request
      ↓
экран не гаснет
      ↓
Allow / Confirm Pairing
```

### Проверить на реальных Galaxy Watch4

1. открыть WristKey;
2. оставить экран без касаний;
3. дождаться обычного timeout;
4. проверить, что экран остаётся активным;
5. инициировать pairing с PC;
6. дождаться `Pairing request`;
7. проверить, что экран не гаснет до подтверждения.

Если конкретная версия Wear OS всё равно переводит устройство в ambient/lock state, следующим шагом проверить lifecycle/ambient API отдельно. Не добавлять бесконечный wake lock без необходимости.

---

## 6. Приоритеты разработки

### P0 — persistent pairing

Проверить реальный сценарий:

```text
pair
 ↓
close desktop
 ↓
start desktop
 ↓
list_paired_devices()
 ↓
silent reconnect
```

Не должно быть повторного Pairing Request без явного unpair.

Проверить:

- фактический путь persistent DB;
- `SqliteStorage`/текущий storage implementation;
- сохранение device после successful pairing;
- загрузку devices при startup;
- identity/public key, а не только BLE MAC.

### P1 — Credential Provider tile

```text
Windows lock screen
        ↓
WristKey
        ↓
account tiles
```

Сначала собрать DLL и проверить регистрацию/появление tile в тестовой Windows.

### P2 — daemon ↔ Credential Provider IPC

Использовать существующий authenticated local IPC/named pipe механизм.

```text
Provider → daemon: authenticate account / request unlock
Daemon → Provider: waiting
Daemon → Provider: watch challenge result
Provider → LogonUI: serialization
```

Pipe:

- только local;
- безопасные ACL;
- без remote clients;
- без передачи Windows password.

### P3 — Windows account enrollment

```text
Windows SID
      ↕
WristKey public key
```

Хранить только необходимые SID/public-key данные.

### P4 — реальный Windows unlock

Реализовать корректный `CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION` и проверить:

- logon;
- unlock workstation;
- wrong/non-matching watch;
- cancelled Allow;
- watch unavailable;
- fallback password/PIN.

### P5 — automatic user selection

```text
watch public key
      ↓
account mapping
      ↓
auto-select tile
```

Только после надёжного ручного unlock.

### P6 — proximity engine

Внешний ориентир: **ProximityLock** (`https://proximitylock.app/`). Использовать идеи поведения, но не копировать код/протокол.

Нужные механизмы:

- RSSI baseline/calibration;
- filtering;
- hysteresis;
- grace period;
- cooldown;
- BLE reconnect/recovery;
- учёт резкого изменения RSSI;
- один BLE adapter lifecycle;
- защита от ложных triggers.

Ключевое правило WristKey:

```text
RSSI = сигнал близости
ECDSA challenge/response = доказательство identity
```

RSSI никогда не является proof identity и никогда не заменяет crypto verification.

### P7 — auto-lock

```text
Watch verified/present
        ↓
RSSI degrades
        ↓
SUSPECTED_AWAY
        ↓
grace period
        ↓
AWAY
        ↓
Windows lock
```

Кратковременный BLE dropout не должен сразу блокировать ПК.

### P8 — auto-unlock

```text
Watch detected
      ↓
proximity filter
      ↓
public key known
      ↓
cryptographic challenge
      ↓
Allow / verified presence
      ↓
Windows unlock
```

Никогда не реализовывать auto-unlock через хранение Windows password и имитацию ввода пароля.

### P9 — Linux/macOS

После стабильного Windows flow:

- Linux PAM;
- macOS PAM/Keychain;
- общая account/device model.

---

## 7. ProximityLock — что берём как reference

ProximityLock интересен именно proximity-частью: RSSI filtering, hysteresis, cooldown, адаптация поведения и защита от ложных lock/unlock triggers.

Архитектурно WristKey должен идти дальше:

```text
                 BLE proximity
                      │
                      ▼
                Proximity Engine
                      │
                 Watch nearby?
                      │
                      ▼
              Crypto challenge
                      │
                ECDSA verified?
                      │
              ┌───────┴───────┐
              ▼               ▼
            ALLOW            DENY
```

Не использовать RSSI как authentication factor.

---

## 8. Критические правила безопасности

1. Не хранить Windows passwords в WristKey.
2. Не делать BLE внутри Credential Provider DLL.
3. Не считать RSSI доказательством identity.
4. Не считать MAC адрес постоянной identity.
5. Не использовать системный Windows Bluetooth Pairing как proof WristKey pairing.
6. Не устанавливать непроверенную Credential Provider DLL на единственную рабочую Windows без fallback login.
7. Не считать compile успешным unlock-тестом.
8. Не возвращать public key внутри response, если он уже предоставляется через `PUBLIC_KEY_CHAR`.
9. Для ECDSA явно фиксировать формат signature (`r||s` или DER) и длину.
10. Не считать чат/AI_HANDOFF доказательством состояния, если Git показывает другое.
11. RSSI может запускать/откладывать проверку proximity, но crypto verification обязательна перед unlock.
12. Экран Wear OS должен оставаться активным во время интерактивного pairing/challenge flow, но не превращать приложение в бесконечный глобальный wake lock.

---

## 9. Последний известный BLE protocol

```text
Service UUID:    a1b2c3d4-e5f6-7890-abcd-ef1234567890
CHALLENGE_CHAR:  a1b2c3d4-e5f6-7890-abcd-ef1234567891
RESPONSE_CHAR:   a1b2c3d4-e5f6-7890-abcd-ef1234567892
PUBLIC_KEY_CHAR: a1b2c3d4-e5f6-7890-abcd-ef1234567893
CONFIG_CHAR:     a1b2c3d4-e5f6-7890-abcd-ef1234567894
UNLOCK_REQUEST:  a1b2c3d4-e5f6-7890-abcd-ef1234567895
UNLOCK_RESPONSE: a1b2c3d4-e5f6-7890-abcd-ef1234567896
PAIRING_KEY:     a1b2c3d4-e5f6-7890-abcd-ef1234567897
PC_NAME:         a1b2c3d4-e5f6-7890-abcd-ef1234567898
```

Public key:

```text
SEC1 raw: 65 bytes
04 || X32 || Y32
```

Pairing response:

```text
signature || user_present_flag
```

`RESPONSE_CHAR` и `UNLOCK_RESPONSE` должны иметь CCCD `0x2902` для Windows/btleplug subscription.

---

## 10. Следующая контрольная точка

```text
Wear OS screen stays awake during pairing
        ↓
persistent pairing survives desktop restart
        ↓
user builds Credential Provider DLL in Visual Studio
        ↓
WristKey tile appears in LogonUI
        ↓
local authenticated IPC
        ↓
selected account ↔ Watch public key
        ↓
Allow on watch
        ↓
real Windows unlock
        ↓
automatic account selection
        ↓
proximity engine
        ↓
auto-lock / auto-unlock
```

### После каждого изменения

```text
compile
→ real device test
→ record result
→ commit
→ update roadmap
```
