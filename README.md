# WristKey

[![CI](https://github.com/serejaishkin/WristKey/actions/workflows/ci.yml/badge.svg)](https://github.com/serejaishkin/WristKey/actions)
[![Build All](https://github.com/serejaishkin/WristKey/actions/workflows/release.yml/badge.svg)](https://github.com/serejaishkin/WristKey/actions)

**Разблокируй PC с Wear OS. Без паролей. Без USB. Только Bluetooth.**

WristKey — это демон для десктопа (Windows / Linux / macOS) + приложение для Wear OS.
Когда часы на запястье рядом с компьютером — экран разблокирован.
Отходишь — экран автоматически блокируется.

---

## Быстрый старт

### 1. Скачай билды (каждый push → свежие артефакты)

| Платформа | Артефакт | Ссылка |
|-----------|----------|--------|
| Windows | `wristkey-tauri.exe` + MSI/NSIS установщики | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| Linux | `wristkeyd` (headless) | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| macOS | `wristkeyd` + трей | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| Wear OS | `app-debug.apk` | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |

### 2. Установи

**Windows:**
```powershell
# Установка через MSI
msiexec /i WristKey_0.1.0_x64_en-US.msi

# Или запусти напрямую
.\wristkey-tauri.exe
# Иконка появится в системном трее (стрелка вверх рядом с часами)
```

**Wear OS (часы / эмулятор):**
```bash
adb install app-debug.apk
adb shell am start -n com.wristkey/.MainActivity
```

### 3. Pairing

1. Запусти `wristkey-tauri.exe` на ПК
2. Открой WristKey на часах → нажми "Pair with PC"
3. Часы начнут BLE Advertising — демон найдёт их, обменяется публичными ключами ECDSA P-256
4. Статус сменится на Connected

### 4. Использование

| Действие | Результат |
|----------|-----------|
| Часы рядом с ПК (RSSI > -65) | Экран разблокирован |
| Отходишь с часами (RSSI < -65) | Экран блокируется автоматически |
| Нажал Unlock PC на часах | Ручная разблокировка по challenge-response |
| Touch Point на часах | Разблокировка касанием личной точки |

---

## Функции

### Основные
- **BLE-аутентификация** — ECDSA P-256, AndroidKeyStore (hardware-backed)
- **Автоматическая блокировка/разблокировка** — по RSSI proximity
- **Touch Point Unlock** — касание личной точки на экране часов
- **Challenge-Response** — защита от replay-атак (±30 сек)
- **Anti-relay** — акселерометр + user presence
- **Multiple devices** — поддержка нескольких часов

### Windows
- **Credential Provider** — тайл на экране блокировки (как PIN/пароль)
- **Windows Service** — автозапуск демона при загрузке
- **DPAPI** — шифрование пароля через TPM/Windows Data Protection
- **MSI/NSIS установщики** — автоматическая установка

### Wear OS
- **Touch Point Training** — обучение личной точки на часах
- **Watch Training Control** — управление обучением с ПК через BLE
- **Настройки** — RSSI threshold, таймауты

---

## Архитектура

```
WristKey/
├── desktop/                        # Rust workspace
│   ├── crates/
│   │   ├── core/                   # Crypto, storage, config, session manager
│   │   │   ├── src/lib.rs          # TouchPoint, SessionManager, Storage trait
│   │   │   └── ...
│   │   ├── ble/                    # btleplug adapter (central) + NullBleAdapter
│   │   │   ├── src/lib.rs          # BleAdapter trait, BtleplugAdapter
│   │   │   └── ...
│   │   ├── daemon/                 # Daemon loop + ConnectionManager
│   │   │   ├── src/lib.rs          # Daemon::run(), reconnect, RSSI proximity
│   │   │   └── ...
│   │   ├── crypto/                 # AES-256-GCM, ECDSA P-256
│   │   ├── credential-provider/    # C# .NET 4.8 Credential Provider
│   │   │   ├── WristKeyCredentialProvider.cs
│   │   │   └── WristKeyCredentialProvider.csproj
│   │   ├── platform-win/           # Windows: LockWorkStation, DPAPI, CP registration
│   │   │   └── src/lib.rs          # WindowsSecurity, WindowsVault, register/unregister CP
│   │   ├── platform-linux/         # Linux: loginctl lock
│   │   └── platform-macos/         # macOS: AppleScript lock
│   └── tauri/                      # Tauri v2 desktop GUI
│       ├── src-tauri/
│       │   ├── src/main.rs         # Tauri commands, daemon init, tray icon
│       │   ├── src/service.rs      # Windows Service (--service mode)
│       │   ├── src/log_rolling.rs  # Log rotation (20MB × 5)
│       │   └── Cargo.toml
│       └── src/
│           ├── index.html          # Frontend: devices, settings, Windows tabs
│           └── main.js             # Frontend logic
│
├── wear-os/                        # Android (Kotlin, Wear OS)
│   └── app/src/main/java/com/wristkey/
│       ├── MainActivity.kt         # Main screen, BLE service binding
│       ├── ble/
│       │   └── WristKeyBleService.kt   # BLE GATT server, training control
│       ├── ui/
│       │   ├── PairingActivity.kt      # Pairing flow
│       │   ├── TrainingActivity.kt     # Touch point training
│       │   └── UnlockActivity.kt       # Touch point unlock
│       ├── security/
│       │   └── TouchPointStore.kt      # Touch point storage
│       ├── sensors/
│       │   └── MotionDetector.kt       # Anti-relay (accelerometer)
│       └── SettingsActivity.kt         # Settings
│
├── windows-credential-provider/    # C++ native CP (alternative, not primary)
└── docs/
    └── REFERENCE_PROJECTS.md
```

---

## Протокол BLE (GATT)

**Service UUID:** `a1b2c3d4-e5f6-7890-abcd-ef1234567890`

| Характеристика | UUID | Свойства | Описание |
|---------------|------|----------|----------|
| CHALLENGE | ...7891 | Write | PC пишет 24 байта: nonce(16) + timestamp(8) |
| RESPONSE | ...7892 | Notify | Часы отвечают 65 байт: raw ECDSA sig(64) + user_present(1) |
| PUBLIC_KEY | ...7893 | Read | Публичный ключ ECDSA P-256 |
| PC_NAME | ...7898 | Read | Имя ПК для отображения на часах |
| TRAINING_CONTROL | ...7899 | Write/Notify | Управление обучением touch point |

**Anti-relay:**
- Часы подписывают challenge только если акселерометр фиксирует движение в последние 30 секунд (часы на руке)
- Пользователь нажал кнопку на экране в течение 30 секунд (user_present = 1)

---

## Windows Service

Сервис позволяет запускать демон без GUI при загрузке Windows:

```bash
# Установка (от админа)
# Нажми "Install Service" в GUI или:
WristKeyTauri.exe  # запусти от админа → Windows tab → Install Service

# Управление
net start WristKey    # запуск
net stop WristKey     # остановка
sc config WristKey start= auto  # автозапуск

# Удаление
# Нажми "Uninstall Service" в GUI
```

---

## Windows Credential Provider

Тайл на экране блокировки (рядом с PIN и паролем):

1. Введи пароль Windows в GUI → "Windows Password" → Save
2. Нажми "Register Credential Provider" (от админа)
3. Перезагрузись
4. На экране блокировки появится тайл "WristKey Credential Provider"

---

## Безопасность

- **ECDSA P-256** — ключи генерируются в AndroidKeyStore (hardware-backed, нельзя извлечь)
- **Raw 64-byte signature** — фиксированный размер, без DER-вариативности
- **Timestamp validation** — ±30 секунд, защита от replay-атак
- **Motion detection** — подпись без движения невозможна (relay-атака отсекается)
- **User presence** — требуется явное нажатие на экране часов
- **BLE address rotation** — PC находит часы по service UUID, а не по MAC-адресу
- **DPAPI** — пароль шифруется через TPM 2.0 (или software fallback)

---

## Roadmap

| Версия | Статус | Фича |
|--------|--------|------|
| v0.1 | Готово | Скелет: core, BLE, 3 платформы, daemon, CLI, sled |
| v1.1 | Готово | Tray GUI, GATT Server, Keystore, Motion, Packaging, Touch unlock, Log rotation |
| v1.2 | Готово | Windows Credential Provider, Windows Service, Watch Training Control |
| v2.0 | План | PAM модуль Linux, Touch ID macOS, защита от relay-атак через UWB |

---

## Локальная разработка

**Desktop (Rust):**
```bash
cd desktop
cargo check --workspace
cargo test --workspace
cargo build --release

# Сборка Tauri приложения
cd tauri
cargo tauri build
```

**Wear OS (Android Studio):**
```bash
cd wear-os
./gradlew assembleDebug
```

**Credential Provider (.NET):**
```bash
cd desktop/crates/credential-provider
dotnet build -c Release
```

---

## Лицензия

MIT © 2026 serejaishkin
