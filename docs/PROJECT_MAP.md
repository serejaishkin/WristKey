# WristKey — Карта проекта

## Структура репозитория

```
WristKey/
│
├── desktop/                            # Десктопная часть (Rust workspace)
│   │
│   ├── Cargo.toml                      # Workspace root
│   │
│   ├── crates/
│   │   ├── core/                       # Ядро: криптография, хранилище, сессии
│   │   │   └── src/
│   │   │       ├── lib.rs              # TouchPoint, PC_TOUCH_TOLERANCE, Storage trait
│   │   │       ├── session.rs          # SessionManager: pairing, auth, challenges
│   │   │       ├── config.rs           # Config: timeouts, thresholds
│   │   │       ├── sqlite_storage.rs   # SQLite: devices, touch_points
│   │   │       ├── vault.rs            # DeviceVault: шифрование паролей
│   │   │       └── types.rs            # PairedDevice, Challenge, Response
│   │   │
│   │   ├── ble/                        # BLE-адаптер (btleplug)
│   │   │   └── src/
│   │   │       └── lib.rs              # BleAdapter trait, BtleplugAdapter, NullBleAdapter
│   │   │
│   │   ├── daemon/                     # Демон: reconnect, proximity, pipe server
│   │   │   └── src/
│   │   │       ├── lib.rs              # Daemon::run(), auth loop
│   │   │       └── conn_mgr.rs         # ConnectionManager: get_or_connect, reconnect
│   │   │
│   │   ├── crypto/                     # Криптография
│   │   │   └── src/
│   │   │       └── lib.rs              # AES-256-GCM, ECDSA P-256
│   │   │
│   │   ├── credential-provider/        # C# .NET 4.8 Credential Provider
│   │   │   ├── WristKeyCredentialProvider.cs      # ICredentialProvider, ICredentialProviderCredential
│   │   │   ├── WristKeyCredentialProvider.csproj  # net48, x64, ComVisible
│   │   │   └── Program.cs              # Точка входа для тестирования
│   │   │
│   │   ├── platform-win/               # Windows-специфичный код
│   │   │   └── src/
│   │   │       └── lib.rs              # WindowsSecurity: lock, CP registration, DPAPI
│   │   │                               # WindowsVault: шифрование пароля через TPM
│   │   │
│   │   ├── platform-linux/             # Linux-специфичный код
│   │   │   └── src/
│   │   │       └── lib.rs              # LinuxSecurity: loginctl lock
│   │   │
│   │   └── platform-macos/             # macOS-специфичный код
│   │       └── src/
│   │           └── lib.rs              # MacosSecurity: AppleScript lock
│   │
│   └── tauri/                          # Tauri v2 десктопное приложение
│       │
│       ├── src-tauri/
│       │   ├── Cargo.toml              # Зависимости: tauri, windows-service, winreg
│       │   ├── tauri.conf.json         # Конфигурация Tauri: bundle,窗口, плагины
│       │   └── src/
│       │       ├── main.rs             # Tauri commands, daemon init, tray icon
│       │       │                       # Commands: get_status, scan, pair, lock, etc.
│       │       ├── service.rs          # Windows Service: --service mode
│       │       │                       # install_service, uninstall_service, get_service_status
│       │       └── log_rolling.rs      # Log rotation (20MB × 5 файлов)
│       │
│       └── src/                        # Frontend (HTML/JS)
│           ├── index.html              # UI: 3 таба (Devices, Settings, Windows)
│           └── main.js                 # Frontend logic: scan, pair, calibrate, etc.
│
├── wear-os/                            # Wear OS приложение (Kotlin)
│   │
│   ├── build.gradle.kts                # Зависимости: Compose, BLE, Wear OS
│   ├── local.properties                # Android SDK path
│   └── app/src/main/
│       ├── AndroidManifest.xml         # Permissions, activities, keepScreenOn
│       └── java/com/wristkey/
│           │
│           ├── MainActivity.kt         # Главный экран: scan, pair, training buttons
│           │                           # BLE service binding, permission handling
│           │
│           ├── ble/
│           │   └── WristKeyBleService.kt   # BLE GATT Server ( peripheral)
│           │                               # Advertising, characteristics
│           │                               # TRAINING_CONTROL for PC control
│           │
│           ├── ui/
│           │   ├── PairingActivity.kt      # Экран pairing
│           │   ├── TrainingActivity.kt     # Touch point training ( binds to BLE service)
│           │   └── UnlockActivity.kt       # Touch point unlock gate
│           │
│           ├── security/
│           │   └── TouchPointStore.kt      # Хранилище touch point ( EncryptedSharedPreferences)
│           │
│           ├── sensors/
│           │   └── MotionDetector.kt       # Anti- relay: акселерометр ( 30 sec window)
│           │
│           └── SettingsActivity.kt         # Настройки: RSSI threshold, timeouts
│
├── windows-credential-provider/        # C++ native CP (альтернативный, не основной)
│   ├── src/
│   │   ├── Provider.cpp                # ICredentialProvider implementation
│   │   ├── Guid.cpp                    # CLSID definition
│   │   └── exports.cpp                 # DllGetClassObject, DllCanUnloadNow
│   ├── include/
│   │   └── Provider.h
│   └── WristKeyCredentialProvider.vcxproj
│
├── docs/
│   └── REFERENCE_PROJECTS.md           # Ссылки на похожие проекты
│
└── logs/                               # Локальные логи (gitignore)
```

---

## Ключевые файлы

### Desktop — Основные entry points

| Файл | Назначение |
|------|------------|
| `desktop/tauri/src-tauri/src/main.rs` | Tauri commands, daemon init, tray icon |
| `desktop/tauri/src-tauri/src/service.rs` | Windows Service mode (--service flag) |
| `desktop/crates/core/src/lib.rs` | TouchPoint, Storage trait, SessionManager |
| `desktop/crates/ble/src/lib.rs` | BleAdapter trait, BtleplugAdapter |
| `desktop/crates/daemon/src/lib.rs` | Daemon::run(), auth loop |
| `desktop/crates/daemon/src/conn_mgr.rs` | ConnectionManager: reconnect logic |
| `desktop/crates/platform-win/src/lib.rs` | Windows: lock, CP registration, DPAPI |
| `desktop/crates/credential-provider/WristKeyCredentialProvider.cs` | .NET CP implementation |

### Wear OS — Основные entry points

| Файл | Назначение |
|------|------------|
| `wear-os/app/src/main/java/com/wristkey/MainActivity.kt` | Main screen, BLE binding |
| `wear-os/app/src/main/java/com/wristkey/ble/WristKeyBleService.kt` | GATT server, training control |
| `wear-os/app/src/main/java/com/wristkey/ui/TrainingActivity.kt` | Touch point training |
| `wear-os/app/src/main/java/com/wristkey/ui/UnlockActivity.kt` | Touch point unlock |
| `wear-os/app/src/main/java/com/wristkey/security/TouchPointStore.kt` | Touch point storage |

---

## BLE Protocol

```
┌─────────────────────────────────────────────────────────────┐
│                     Wear OS (Peripheral)                     │
│                                                             │
│  WristKeyBleService                                         │
│  ├── GATT Server                                           │
│  │   ├── SERVICE_UUID: a1b2c3d4-e5f6-7890-abcd-ef1234567890│
│  │   │                                                     │
│  │   ├── CHALLENGE_CHAR (Write)    ← PC writes nonce       │
│  │   ├── RESPONSE_CHAR (Notify)    → Watch sends signature  │
│  │   ├── PUBLIC_KEY_CHAR (Read)    → Watch sends pubkey     │
│  │   ├── PC_NAME_CHAR (Read)       → Watch reads PC name    │
│  │   └── TRAINING_CONTROL (Write/Notify) ←→ Training sync   │
│  │                                                     │
│  │                                                     │
│  └── MotionDetector (accelerometer, 30s window)       │
│      └── user_present flag                           │
└─────────────────────────────────────────────────────────────┘
         │                                    │
         │  BLE GATT                          │
         │                                    │
┌────────▼────────────────────────────────────▼───────────────┐
│                     Desktop (Central)                        │
│                                                             │
│  ConnectionManager                                          │
│  ├── get_or_connect()                                      │
│  ├── resolve_current_peripheral() (with initial drain)      │
│  └── reconnect_throttle: 4s                                │
│                                                             │
│  Daemon                                                     │
│  ├── Auth loop: challenge → verify → lock/unlock            │
│  ├── RSSI proximity: PRESENT/NEAR/AWAY                     │
│  └── Touch point verification                               │
│                                                             │
│  Credential Provider (Windows)                              │
│  ├── ICredentialProvider → GetCredentialAt                  │
│  ├── ICredentialProviderCredential → Advise                 │
│  └── Unlocks Windows with stored password                   │
└─────────────────────────────────────────────────────────────┘
```

---

## Сборка и установка

### Desktop

```bash
# Проверка
cd desktop && cargo check --workspace

# Release сборка
cd tauri && cargo tauri build

# Результаты:
# - target/release/wristkey-tauri.exe
# - target/release/bundle/msi/WristKey_0.1.0_x64_en-US.msi
# - target/release/bundle/nsis/WristKey_0.1.0_x64-setup.exe
```

### Wear OS

```bash
cd wear-os && ./gradlew assembleDebug

# Результат:
# - app/build/outputs/apk/debug/app-debug.apk
```

### Credential Provider

```bash
cd desktop/crates/credential-provider
dotnet build -c Release

# Результат:
# - bin/Release/net48/WristKeyCredentialProvider.dll
```

---

## Windows Service Commands

```bash
# Установка (от админа)
# GUI: Windows tab → Install Service
# Или: WristKeyTauri.exe (от админа) → Install

# Управление
net start WristKey
net stop WristKey
sc query WristKey

# Автозапуск
sc config WristKey start= auto

# Удаление
# GUI: Windows tab → Uninstall Service
# Или: sc delete WristKey
```

---

## Credential Provider Commands

```bash
# Регистрация (от админа)
# GUI: Windows tab → Register Credential Provider

# Проверка
reg query "HKCR\CLSID\{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}" /s
reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}"

# Удаление
# GUI: Windows tab → Unregister
```

---

## Диагностика

### Логи

```bash
# Десктоп логи
# %LOCALAPPDATA%\WristKey\logs\wristkey.log

# Wear OS логи
adb logcat -s WristKeyBLE
```

### BLE отладка

```bash
# Проверка BLE адаптера
# Windows: Settings → Bluetooth & devices
# Linux: bluetoothctl
```

### Реестр (Windows)

```bash
# Credential Provider
reg query "HKCR\CLSID\{A1B2C3D4-E5F6-7890-ABCD-EF1234567895}" /s

# Windows Service
reg query "HKLM\SYSTEM\CurrentControlSet\Services\WristKey"

# UserTile
reg query "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\LogonUI\UserTile"
```
