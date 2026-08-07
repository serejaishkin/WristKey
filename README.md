# 🔷 WristKey

[![CI](https://github.com/serejaishkin/WristKey/actions/workflows/ci.yml/badge.svg)](https://github.com/serejaishkin/WristKey/actions)
[![Build All](https://github.com/serejaishkin/WristKey/actions/workflows/release.yml/badge.svg)](https://github.com/serejaishkin/WristKey/actions)

**Разблокируй PC с Wear OS. Без паролей. Без USB. Только Bluetooth.**

WristKey — это демон для десктопа (Windows / Linux / macOS) + приложение для Wear OS.  
Когда часы на запястье рядом с компьютером — экран разблокирован.  
Отходишь — экран автоматически блокируется.  

---

## 🚀 Быстрый старт

### 1. Скачай билды (каждый push → свежие артефакты)

| Платформа | Артефакт | Ссылка |
|-----------|----------|--------|
| Windows | `wristkeyd.exe` + трей | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| Linux | `wristkeyd` (headless) | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| macOS | `wristkeyd` + трей | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| Wear OS | `app-debug.apk` | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |

### 2. Установи

**Windows:**
```powershell
# Распакуй zip, запусти
.\wristkeyd.exe
# Иконка появится в системном трее (стрелка вверх рядом с часами)
Wear OS (часы / эмулятор):
bash
adb install app-debug.apk
adb shell am start -n com.wristkey/.app.MainActivity
3. Pairing
Запусти wristkeyd.exe на ПК
Открой WristKey на часах → нажми "Pair with PC"
Часы начнут BLE Advertising — демон найдёт их, обменяется публичными ключами ECDSA P-256
Статус сменится на Connected
4. Использование
Table
***
Действие	Результат
Часы рядом с ПК (RSSI > -65)	Экран разблокирован
Отходишь с часами (RSSI < -65)	Экран блокируется автоматически
Нажал Unlock PC на часах	Ручная разблокировка по challenge-response
🏗 Архитектура
plain
WristKey/
├── desktop/                    # Rust workspace
│   ├── crates/
│   │   ├── core/               # Crypto, storage, config, session manager
│   │   ├── ble/                # btleplug adapter (central)
│   │   ├── daemon/             # GUI трей + daemon loop
│   │   ├── platform-win/       # LockWorkStation (raw FFI user32.dll)
│   │   ├── platform-linux/     # loginctl lock
│   │   └── platform-macos/     # AppleScript lock
│   └── Cargo.toml
│
└── wear-os/                    # Android (Kotlin)
    └── app/
        ├── ble/                # BluetoothGattServer (peripheral)
        ├── security/           # AndroidKeyStore + ECDSA signing
        ├── sensors/            # MotionDetector (anti-relay)
        └── app/                # MainActivity + UI


🔐 Протокол BLE (GATT)
Service UUID: a1b2c3d4-e5f6-7890-abcd-ef1234567890
Table
Характеристика	UUID	Свойства	Описание
CHALLENGE	...7891	Write	PC пишет 24 байта: nonce(16) + timestamp(8)
RESPONSE	...7892	Notify	Часы отвечают 65 байт: raw ECDSA sig(64) + user_present(1)
STATUS	...7893	Read/Notify	0x00 = disconnected, 0x01 = pairing, 0x02 = authenticated
Anti-relay:
Часы подписывают challenge только если акселерометр фиксирует движение (часы на руке)
пользователь нажал кнопку на экране в течение 10 секунд (user_present = 1)
🛡 Безопасность
ECDSA P-256 — ключи генерируются в AndroidKeyStore (hardware-backed, нельзя извлечь)
Raw 64-byte signature — фиксированный размер, без DER-вариативности
Timestamp validation — ±30 секунд, защита от replay-атак
Motion detection — подпись без движения невозможна (relay-атака отсекается)
User presence — требуется явное нажатие на экране часов
📋 Roadmap
Table
Версия	Статус	Фича
v0.1	✅ Готово	Скелет: core, BLE, 3 платформы, daemon, CLI, sled
v1.1	🔄 В работе	Tray GUI, GATT Server, Keystore, Motion, Packaging
v1.2	📅 План	Windows Hello Credential Provider (unlock, не только lock)
v2.0	📅 План	PAM модуль Linux, Touch ID macOS, защита от relay-атак через UWB
🧪 Локальная разработка
Desktop (Rust):
bash
cd desktop
cargo check --workspace
cargo test --workspace
cargo run --bin wristkeyd -- --pair
Wear OS (Android Studio):
bash
cd wear-os
./gradlew assembleDebug
Windows cross-check (в Codespaces):
bash
cargo check --target x86_64-pc-windows-msvc -p wristkey-platform-win
📄 Лицензия
MIT © 2026 serejaishkin
