# WristKey — AI Roadmap / Handoff checkpoint

**Дата контрольной точки: 2026-08-19**  
**Рабочая ветка: `fix/wristkey-20260818`**

> Этот файл — актуальная дорожная карта для следующего AI-сеанса. Перед изменениями сначала сверять фактический Git, затем этот roadmap и последний чат.

---

## 1. Где мы сейчас

### Подтверждено пользователем

- Wear OS APK собирается.
- Desktop/PC часть собирается.
- **Pairing с Galaxy Watch4 теперь работает.** Пользователь подтвердил успешный pairing после серии исправлений BLE/GATT/crypto.
- На часах реально появляется `Pairing request` и пользователь может подтвердить `Allow`.
- Ошибки `subscribe: HRESULT(0x80650003) / Не удается записать атрибут` были пройдены после добавления CCCD.
- Следующий основной практический баг desktop: **после закрытия/перезапуска ПК-программы paired watch забывается**. Для этого в текущей ветке был переведён desktop storage на persistent storage вместо временного `MemoryStorage`.

### Пока НЕ подтверждено

- Silent reconnect после перезапуска desktop.
- Полный unlock Windows через Credential Provider.
- Появление WristKey tile на реальном Windows Lock Screen.
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

Credential Provider должен быть **тонким**. Он не должен самостоятельно заниматься BLE, scan, GATT или криптографическим протоколом часов.

Daemon отвечает за:

- BLE discovery/connection;
- pairing;
- challenge/response;
- public-key verification;
- persistent paired devices;
- связь с Credential Provider.

Credential Provider отвечает за:

- Windows LogonUI;
- список/плитки учётных записей;
- выбранную Windows account;
- ожидание результата от daemon;
- Windows Credential Provider serialization.

---

## 3. Multi-account модель

Цель проекта — **одна универсальная DLL для разных Windows-учётных записей**, а не отдельная DLL под пользователя.

Пример:

```text
Windows PC
│
├── Сергей
│    └── Galaxy Watch A / public key A
│
├── Иван
│    └── Galaxy Watch B / public key B
│
└── Анна
     └── Galaxy Watch C / public key C
```

Пароли Windows **не должны храниться в WristKey**.

Первичная enrollment-схема:

```text
обычный Windows login / admin enrollment
        ↓
выбор Windows account
        ↓
pair Galaxy Watch
        ↓
получение public key
        ↓
привязка public key ↔ account/SID
```

Будущий автоматический режим:

```text
Watch detected
      ↓
public key matched
      ↓
Windows account identified
      ↓
challenge
      ↓
Allow on watch
      ↓
Windows unlock
```

Ручной режим также должен оставаться доступным: пользователь выбирает account tile и подтверждает на часах.

---

## 4. Credential Provider — текущий статус

Каталог:

```text
windows-credential-provider/
├── WristKeyCredentialProvider.sln
├── WristKeyCredentialProvider.vcxproj
├── WristKeyCredentialProvider.def
├── CMakeLists.txt
├── README.md
├── install.ps1
├── include/
│   └── Provider.h
└── src/
    ├── Credential.cpp
    ├── Guid.cpp
    ├── Provider.cpp
    └── exports.cpp
```

### Уже сделано

- native C++ x64 Credential Provider;
- COM factory;
- CLSID registration/unregistration;
- `CPUS_LOGON`;
- `CPUS_UNLOCK_WORKSTATION`;
- enumeration enabled local accounts;
- отдельная WristKey credential для каждой перечисленной локальной account;
- Visual Studio `.sln` + `.vcxproj`, чтобы **не требовать CMake** для обычной сборки.

### Пока не сделано

`GetSerialization()` пока намеренно возвращает:

```text
CPGSR_NO_CREDENTIAL_NOT_FINISHED
```

Это означает: **DLL пока не разблокирует Windows**.

Следующая задача — authenticated IPC с daemon и реальная Windows authentication serialization.

---

## 5. Сборка DLL для пользователя

Теперь пользователю не нужен отдельный CMake workflow.

Открыть:

```text
D:\GitHub\WristKey\windows-credential-provider\WristKeyCredentialProvider.sln
```

Visual Studio:

```text
Configuration: Release
Platform: x64
Build → Build Solution
```

Ожидаемый результат:

```text
windows-credential-provider\build\Release\WristKeyCredentialProvider.dll
```

Проект использует MSVC v143 и Windows SDK.

CMake остаётся дополнительным способом сборки, но больше не является обязательным для пользователя.

---

## 6. PC Bio Unlock — внешний архитектурный ориентир

Проект, на который пользователь явно ориентируется:

`https://github.com/MeisApps/pcbu-desktop`

Название: **PC Bio Unlock Desktop**.

По публичному описанию проекта, он предоставляет desktop app для разблокировки PC Android-телефоном, поддерживает TCP/Bluetooth и Windows/Linux/macOS, а на Windows поддерживает Login/Unlock screen и UAC. Текущий репозиторий преимущественно C++, с QML и CMake. Последняя найденная release-линейка включает v3.3.x. 

WristKey использует PCBU как **архитектурный ориентир**, а не как dependency или источник копируемого кода.

Главная идея, которую стоит перенять концептуально:

```text
heavy device/service logic
        ↓
thin Windows Credential Provider
```

У WristKey транспорт и протокол другие:

```text
PCBU: phone ↔ desktop via TCP/Bluetooth
WristKey: Galaxy Watch ↔ desktop via custom BLE GATT + ECDSA
```

Не копировать протокол, ключи, UI или исходный код PCBU. Использовать только архитектурные идеи и публично документированное поведение как reference.

---

## 7. Приоритеты разработки

### P0 — сохранить pairing

**Цель:** после успешного pairing закрытие desktop не должно уничтожать привязку.

Проверить:

1. pairing;
2. закрыть desktop;
3. запустить снова;
4. `list_paired_devices()` должен вернуть часы;
5. daemon должен попробовать silent reconnect;
6. никаких повторных Pairing Request без явного unpair.

Если не работает:

- проверить фактический путь persistent DB;
- проверить `SqliteStorage`/текущий storage implementation;
- проверить `save paired device` после successful pairing;
- проверить загрузку devices при startup;
- проверить device identity/public key, а не только BLE MAC.

### P1 — Credential Provider tile

Цель:

```text
Windows lock screen
        ↓
WristKey
        ↓
account tiles
```

Проверить DLL в тестовой Windows/VM прежде чем ставить на основную машину.

### P2 — daemon ↔ Credential Provider IPC

Использовать существующий локальный authenticated IPC/named pipe механизм, а не новый параллельный BLE канал.

Цель:

```text
Provider → daemon: authenticate account / request unlock
Daemon → Provider: waiting
Daemon → Provider: watch challenge result
Provider → LogonUI: serialization
```

Pipe должен иметь безопасные ACL, не принимать remote clients и не передавать пароль в открытом виде.

### P3 — Windows account enrollment

Привязать:

```text
Windows SID
      ↕
WristKey public key
```

Хранить только необходимые идентификаторы/публичные данные. Не хранить Windows password.

### P4 — реальный Windows unlock

Реализовать корректный `CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION` для выбранной account и проверить:

- logon;
- unlock workstation;
- wrong/non-matching watch;
- cancelled Allow;
- watch unavailable;
- fallback to password/PIN.

### P5 — automatic user selection

После ручного режима сделать:

```text
watch public key
      ↓
account mapping
      ↓
auto-select tile
```

Только после надёжной ручной схемы.

### P6 — proximity / RSSI

После стабилизации pairing/unlock проверить:

- baseline RSSI;
- calibration;
- один BLE adapter lifecycle;
- smoothing;
- отсутствие ложных unlock;
- unlock только после cryptographic verification.

RSSI никогда не заменяет cryptographic proof.

### P7 — Linux/macOS

После рабочего Windows flow довести:

- Linux PAM;
- macOS PAM/Keychain;
- одинаковую account/device model.

---

## 8. Критические правила безопасности

1. Не хранить Windows passwords в WristKey.
2. Не делать BLE внутри Credential Provider DLL.
3. Не считать RSSI доказательством identity.
4. Не считать MAC адрес постоянной identity.
5. Не использовать системный Windows Bluetooth Pairing как proof WristKey pairing.
6. Не устанавливать непроверенную Credential Provider DLL на единственную рабочую Windows без fallback login.
7. Не тестировать unlock только на уровне compile — нужен реальный LogonUI/lock screen.
8. Не возвращать public key внутри response, если он уже предоставляется через `PUBLIC_KEY_CHAR`.
9. Для ECDSA всегда явно фиксировать формат signature (`r||s` или DER) и длину.
10. Не считать чат/AI_HANDOFF доказательством состояния, если Git показывает другое.

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

## 10. Как работать следующему AI

Перед изменениями:

1. `git status`;
2. `git branch --show-current`;
3. проверить последние commits;
4. прочитать этот файл;
5. сверить с `AI_HANDOFF.md`;
6. сверить последний чат;
7. только затем менять код.

После изменений:

```text
compile
→ real device test
→ log result
→ commit
→ update roadmap
```

Не заявлять «готово», если проверена только сборка.

**Следующая идеальная контрольная точка:**

```text
Pairing works
    ↓
persistent pairing survives restart
    ↓
Credential Provider DLL builds in Visual Studio
    ↓
WristKey tile appears in LogonUI
    ↓
selected account ↔ watch public key
    ↓
Allow on watch
    ↓
real Windows unlock
```
