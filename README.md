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
| Windows | `wristkey-tauri.exe` + трей | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| Linux | `wristkeyd` (headless) | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| macOS | `wristkeyd` + трей | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |
| Wear OS | `app-debug.apk` | [Actions → Build All Artifacts](https://github.com/serejaishkin/WristKey/actions) |

### 2. Установи

**Windows:**
```powershell
# Распакуй zip, запусти
.\wristkey-tauri.exe
# Иконка появится в системном трее (стрелка вверх рядом с часами)
```

**Wear OS (часы / эмулятор):**
```bash
adb install app-debug.apk
adb shell am start -n com.wristkey/.app.MainActivity
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

---

## Архитектура

```
WristKey/
├── desktop/                    # Rust workspace
│   ├── crates/
│   │   ├── core/               # Crypto, storage, config, session manager
│   │   ├── ble/                # btleplug adapter (central) + NullBleAdapter
│   │   ├── daemon/             # GUI трей + daemon loop + ConnectionManager
│   │   ├── platform-win/       # LockWorkStation (raw FFI user32.dll)
│   │   ├── platform-linux/     # loginctl lock
│   │   └── platform-macos/     # AppleScript lock
│   └── tauri/                  # Tauri desktop GUI
│       ├── src-tauri/          # Rust backend (commands, log_rolling)
│       └── src/                # HTML/JS frontend
│
└── wear-os/                    # Android (Kotlin)
    └── app/
        ├── ble/                # BluetoothGattServer (peripheral) + WristKeyBleService
        ├── security/           # AndroidKeyStore + ECDSA signing
        ├── sensors/            # MotionDetector (anti-relay)
        └── app/                # MainActivity + UI + SettingsActivity
```

---

## Протокол BLE (GATT)

**Service UUID:** `a1b2c3d4-e5f6-7890-abcd-ef1234567890`

| Характеристика | UUID | Свойства | Описание |
|---------------|------|----------|----------|
| CHALLENGE | ...7891 | Write | PC пишет 24 байта: nonce(16) + timestamp(8) |
| RESPONSE | ...7892 | Notify | Часы отвечают 65 байт: raw ECDSA sig(64) + user_present(1) |
| STATUS | ...7893 | Read/Notify | 0x00 = disconnected, 0x01 = pairing, 0x02 = authenticated |

**Anti-relay:**
- Часы подписывают challenge только если акселерометр фиксирует движение в последние 30 секунд (часы на руке)
- Пользователь нажал кнопку на экране в течение 30 секунд (user_present = 1)

---

## Безопасность

- **ECDSA P-256** — ключи генерируются в AndroidKeyStore (hardware-backed, нельзя извлечь)
- **Raw 64-byte signature** — фиксированный размер, без DER-вариативности
- **Timestamp validation** — ±30 секунд, защита от replay-атак
- **Motion detection** — подпись без движения невозможна (relay-атака отсекается)
- **User presence** — требуется явное нажатие на экране часов
- **BLE address rotation** — PC находит часы по service UUID, а не по MAC-адресу

---

## Roadmap

| Версия | Статус | Фича |
|--------|--------|------|
| v0.1 | Готово | Скелет: core, BLE, 3 платформы, daemon, CLI, sled |
| v1.1 | Готово | Tray GUI, GATT Server, Keystore, Motion, Packaging, Touch unlock, Log rotation |
| v1.2 | В работе | Windows Hello Credential Provider (unlock, не только lock) |
| v2.0 | План | PAM модуль Linux, Touch ID macOS, защита от relay-атак через UWB |

---

## Локальная разработка

**Desktop (Rust):**
```bash
cd desktop
cargo check --workspace
cargo test --workspace
cargo build --release
```

**Wear OS (Android Studio):**
```bash
cd wear-os
./gradlew assembleDebug
```

---

## Лицензия

MIT © 2026 serejaishkin
