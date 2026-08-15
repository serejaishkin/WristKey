# WristKey — AI Handoff & Roadmap

> ⚠️ **ПРОЧИТАЙ ЭТОТ ФАЙЛ ПЕРВЫМ ДЕЛОМ.** Если тебя зовут Кими (или ты любой
> другой AI-ассистент, начинающий новую сессию по этому проекту) — не трогай
> `desktop/` или `wear-os/` до того, как прочитаешь этот файл целиком. Он
> существует именно потому, что чат с ассистентом упирается в лимит длины, и
> без этого файла каждая новая сессия заново находит одни и те же баги,
> откатывает одни и те же фиксы, и тратит часы на то, что заняло бы 5 минут.
>
> **Этот файл нужно обновлять после каждой сессии.** Если ты внёс
> архитектурное изменение (сменил storage backend, GUI framework, протокол)
> — обнови соответствующий раздел здесь же, прежде чем закончить сессию.

---

## 0. Быстрый старт (если совсем нет времени читать всё)

- **GUI — это Tauri, не eframe.** Точка входа: `desktop/tauri/src-tauri/src/main.rs`. Старые файлы `daemon/src/main.rs`, `gui.rs`, `pair_gui.rs`, `tray.rs` **удалены** — `daemon` теперь чистый lib-крейт (`lib.rs` + `conn_mgr.rs`).
- **Storage — SQLite, не sled.** `wristkey_core::SqliteStorage`. Sled полностью выпилен.
- **Сборка:** `cd desktop && cargo check` (собирает `tauri/src-tauri` по умолчанию — см. `default-members` в `Cargo.toml`). Полная сборка: `cd desktop/tauri/src-tauri && cargo tauri build`.
- **`platform-macos` убран из workspace members** (иначе Windows-сборка пытается резолвить `core-foundation` и падает). Собирать отдельно: `cargo check -p wristkey-platform-macos` только на самой macOS.
- **Криптография (challenge-response) реально живёт в `daemon/src/lib.rs::Daemon::run()`**, вызывается из background-потока в `tauri/src-tauri/src/main.rs`. Если видишь, что разблокировка происходит без вызова `session.verify_unlock()` — это критичный регресс, см. раздел 3, баг №14 (было уже дважды).
- **Логи ПК** пишутся через `tracing_appender::rolling::daily` в файл `wristkey.YYYY-MM-DD` (**не** `wristkey.log`!) в папке, которую можно посмотреть во вкладке Settings → Diagnostics в самом приложении (команда `get_log_dir`).
- **Часы:** GATT-сервер (peripheral-роль) в `WristKeyBleService.kt`. Advertising разбит на два пакета (основной + scan response) — иначе `ADVERTISE_FAILED_DATA_TOO_LARGE`. PIN передаётся в manufacturer data основного пакета.

---

## 1. Текущая архитектура (актуально на дату последнего обновления файла)

### 1.1 Desktop — Rust workspace

```
desktop/
├── Cargo.toml              # workspace root; default-members = ["tauri/src-tauri"]
├── crates/
│   ├── core/                # crypto, SessionManager, SqliteStorage, Config, PlatformSecurity trait
│   ├── crypto/               # ECDSA P-256 primitives
│   ├── ble/                  # btleplug-based BleAdapter (scan/connect/write/notify/read_rssi)
│   ├── daemon/                # lib-only: Daemon::run() (proximity+crypto unlock loop), conn_mgr.rs
│   ├── platform-win/          # LockWorkStation, DPAPI password vault, named-pipe IPC → Credential Provider
│   ├── platform-linux/        # loginctl / D-Bus ScreenSaver, PAM module stub
│   ├── platform-macos/        # AppleScript lock, Keychain password vault (NOT a workspace member — see above)
│   └── credential-provider/   # separate C# (.NET) project — Windows Credential Provider COM DLL
└── tauri/
    ├── src/                   # frontend: index.html, main.js, style.css (vanilla JS, no framework)
    └── src-tauri/              # Tauri v2 Rust backend — THE actual desktop app entry point
```

**Why Tauri and not the eframe/egui GUI from earlier in the project's history:** an earlier iteration built a full GUI in `daemon/src/gui.rs` using eframe. That entire approach was abandoned and the files deleted. If you find any reference to `wristkey_daemon::gui` or `pair_gui` in old context/chat history, **it no longer exists** — don't try to "restore" it, the Tauri app is the real, current one.

### 1.2 Wear OS — Kotlin

```
wear-os/app/src/main/java/com/wristkey/
├── MainActivity.kt              # runtime permission requests, service binding, Unlock/Pair UI
├── ble/WristKeyBleService.kt    # GATT SERVER (peripheral role) — this is what the PC connects to
├── security/SecurityManager.kt  # ECDSA P-256 keypair, signing, raw SEC1 public key export
├── sensors/MotionDetector.kt    # accelerometer-based motion gate
├── WristKeySettings.kt          # SharedPreferences: confirmMode, calibration, paired devices
└── boot/BootReceiver.kt         # restarts service after reboot
```

### 1.3 Protocol — GATT characteristics (must match exactly on both sides)

```
Service UUID:    a1b2c3d4-e5f6-7890-abcd-ef1234567890
CHALLENGE_CHAR:  a1b2c3d4-e5f6-7890-abcd-ef1234567891  (WRITE — PC sends nonce+timestamp)
RESPONSE_CHAR:   a1b2c3d4-e5f6-7890-abcd-ef1234567892  (NOTIFY — watch sends signed response)
STATUS_CHAR:     a1b2c3d4-e5f6-7890-abcd-ef1234567893  (READ/NOTIFY/INDICATE)
CONFIG_CHAR:     a1b2c3d4-e5f6-7890-abcd-ef1234567894  (WRITE/READ — calibration commands)
```

**Response wire format** (watch → PC, both pairing and unlock use the exact same 130-byte layout):
```
signature(64 bytes, raw r||s) || user_present_flag(1 byte) || public_key(65 bytes, raw SEC1 uncompressed)
```
Unlock only needs the first 65 bytes (signature + flag); pairing needs all 130 (also captures the public key). **Public key must be raw SEC1** (`0x04 || X(32) || Y(32)`), not X.509 DER — `SecurityManager.getPublicKey()` on the Kotlin side extracts this manually via `ECPublicKey.w.affineX/affineY`, because `PublicKey.getEncoded()` returns X.509 DER which the Rust side's `p256::PublicKey::from_sec1_bytes` can't parse.

**Advertising is split into two packets** (fixed after `ADVERTISE_FAILED_DATA_TOO_LARGE` — legacy BLE advertising has a hard 31-byte limit per packet):
- Primary advertisement: just manufacturer data (0xFFFF) containing the 4-digit PIN as ASCII — small, always fits.
- Scan response: service UUID + manufacturer data (PIN + 4-byte device_id fingerprint, 8 bytes total).

PIN is shown to the person during pairing (desktop GUI) purely as a visual sanity check ("is this really the watch I'm holding") — it is **not** cryptographic; the real security is the ECDSA challenge-response. If you ever see manufacturer data replaced with a static marker like `"WRST"` instead of an actual random PIN — that's a regression, the desktop still parses `manufacturer_data[..4]` as an ASCII PIN string and will display garbage.

`device_id` fingerprint = first 4 bytes of SHA-256(public key), used by the desktop to re-identify a paired device across BLE address rotation, independent of whether custom advertising is even working (see 3.1 below).

---

## 2. Критичные повторяющиеся баги — НЕ чини их заново вслепую, они уже описаны

| # | Баг | Где | Статус / сколько раз повторялся |
|---|---|---|---|
| 1 | `verify_unlock` генерировал новый challenge вместо использования отправленного | `core` | Исправлено, повторялось ≥2 раза |
| 5 | `device_id` при пейринге читался из несуществующего диапазона байт ответа | `daemon` | Исправлено, повторялось ≥3 раза |
| 6 | Тесты в `core` звали `complete_pairing`/`PairedDevice{}` со старой сигнатурой (арность росла: 4→5→6 аргументов) | `core` | Исправлялось **4 раза за проект**. Если снова добавляешь поле в `PairedDevice` или параметр в `complete_pairing` — **сразу** правь все struct-литералы и вызовы в `#[cfg(test)]`, не откладывай |
| 14 | **Криптографическая проверка полностью выпадала из цикла разблокировки** — `Daemon::run()`/`check_proximity()` просто сравнивал RSSI и сразу звал `platform.unlock_screen()`, без connect/challenge/verify вообще | `daemon` | Исправлено (см. раздел 1.3, `attempt_unlock`). **Это самый опасный класс регрессии в проекте** — при каждом переписывании `Daemon::run()` первым делом проверяй, что unlock идёт через `session.begin_unlock()` → GATT write/notify → `session.verify_unlock()`, а не напрямую по RSSI |
| — | `ADVERTISE_FAILED_DATA_TOO_LARGE` на часах | `WristKeyBleService.kt` | Исправлено — advertising разбит на primary+scan response (см. 1.3) |
| — | PIN в manufacturer data заменён на статичный `"WRST"` при фиксе advertising-размера | `WristKeyBleService.kt` | Исправлено — восстановлен реальный PIN, тот же байт-размер |
| — | `CONFIRM_BUTTON` режим подтверждения всегда возвращал `false` | `WristKeyBleService.kt` | Исправлено — теперь через `pendingUserPresent` atomic-флаг |
| — | Лог-файл ПК называется не так, как сообщает приложение (`wristkey.log` vs реальное `wristkey.YYYY-MM-DD`) | `tauri/src-tauri/main.rs` | Исправлено — сообщение и Settings-вкладка показывают реальное имя |
| — | `Config` без `#[serde(default)]` — фронтенд не мог сохранить настройки (`missing field log_to_file`) | `core` | Исправлено — `#[serde(default)]` на всём struct |
| — | Калибровка (`calibrate_proximity`) не сохраняла `baseline_rssi` в desktop storage — влияла только на копию на часах | `daemon` | Исправлено — `SessionManager::update_baseline_rssi` |
| — | Мусорные файлы `desktop/lib.rs`, `lib (2/3/4).rs` в корне `desktop/` от неаккуратной распаковки патчей | — | Периодически появляются повторно; безвредны для сборки (Cargo их не видит), но стоит удалять для порядка |

---

## 3. Известные открытые вопросы / архитектурные компромиссы

### 3.1 Разные модели часов — Samsung блокирует часть advertising

Обнаружение часов на ПК (`ble/src/lib.rs`) использует несколько независимых
сигналов, ни один не завязан жёстко на бренд:
1. Наш `SERVICE_UUID` в advertising/scan response (работает, если кастомный advertising не заблокирован прошивкой).
2. Наши manufacturer data (PIN+device_id, тот же критерий).
3. Samsung-специфичные эвристики (Accessory Service UUID, manufacturer ID 0x0075) — **только как один из сигналов "похоже на часы", никогда не как единственный критерий сопоставления с уже спаренным устройством**.
4. Сопоставление уже *спаренного* устройства (`Daemon::match_device`) — по адресу, затем по `device_id`-отпечатку, затем по имени без учёта регистра. Все три работают вне зависимости от бренда.

Пользователь тестирует не только Samsung Galaxy Watch — **не добавляй новых
Samsung-специфичных веток в код сопоставления устройства**, только в код
первичного обнаружения (это разные вещи, см. `scan_for_best_match` vs
`match_device` в `daemon/src/lib.rs`).

### 3.2 Дублирование кода калибровки

`Daemon::calibrate_proximity` в `daemon/lib.rs` и логика в Tauri-фронтенде
дублируют часть одной и той же логики. Не критично, но при правке одного
места — проверь второе.

### 3.3 Credential Provider (Windows) — не протестирован в песочнице

`desktop/crates/credential-provider/` — отдельный C#/.NET проект (COM DLL
для Winlogon). Компилируется/тестируется **только на реальной Windows
машине**. Named-pipe IPC между демоном и Credential Provider реализован в
`platform-win/src/lib.rs`. Если правишь протокол pipe-сообщений — синхронизируй
оба конца одновременно, они не имеют общего типа (C# и Rust — сериализация
вручную).

### 3.4 PAM-модуль на Linux — не функционален как реальный фактор входа

Возвращает `PAM_IGNORE` безусловно (безопасный дефолт). Чтобы стал
функционален — нужна проверка короткоживущего доказательства от демона
(Unix-сокет/файл с окном свежести в несколько секунд). См. код в
`platform-linux/src/lib.rs` — там есть заготовка и подробный комментарий.

### 3.5 `default-members` — важно при локальной разработке

`cargo check`/`cargo build` без `-p` теперь по умолчанию собирают **только**
`tauri/src-tauri` (см. `default-members` в `desktop/Cargo.toml`). Если нужно
проверить конкретный крейт — используй `-p wristkey-core` и т.п. явно, иначе
можно ошибочно решить, что что-то не компилируется, хотя оно просто не
входит в default build.

---

## 4. Практический процесс работы с ограничением длины чата

Раз чат с ассистентом (Кими) периодически упирается в лимит и приходится
начинать новую сессию с копией старой переписки — вот что реально помогает:

1. **В начале каждой новой сессии — явно попроси ассистента прочитать этот
   файл первым делом**, например: "Прочитай `AI_HANDOFF.md` в корне
   репозитория, прежде чем что-либо менять." Не полагайся на то, что
   скопированная переписка сама донесёт контекст — она может быть частичной
   или устаревшей (что уже происходило: например, весь эпизод с eframe/GUI
   в старых чатах больше не отражает реальность).
2. **После каждой сессии, где было архитектурное изменение** (смена
   storage backend, GUI framework, добавление/удаление крейта, изменение
   протокола) — попроси ассистента обновить этот файл, прежде чем
   заканчивать сессию. Один явный промпт: "Обнови `AI_HANDOFF.md` с учётом
   того, что мы сегодня сделали."
3. **Раздел 0 (быстрый старт) должен оставаться коротким** — если он
   разрастается, выноси детали в разделы ниже, а раздел 0 держи как
   TL;DR на 10 строк. Это единственная часть файла, которую точно прочитают
   в условиях нехватки контекста.
4. **Таблица в разделе 2 — это чёрный список.** Перед тем как писать
   исправление в `daemon/lib.rs`, `core/lib.rs` или протокольных UUID —
   пробегись по таблице глазами, вдруг твоя "находка" там уже третий раз.
5. Если возможно технически (зависит от того, как устроен интерфейс Кими) —
   стоит проверить, можно ли прикрепить этот файл как системный
   промпт/контекстный документ, который подгружается автоматически в каждую
   новую сессию, а не полагаться на то, что человек вручную скопирует и
   пришлёт его каждый раз.

---

## 5. Чек-лист перед коммитом любого изменения протокольного слоя

```bash
# Core — единственное, что стабильно проверяется в песочницах без полного
# Windows/Android toolchain:
cd desktop && cargo test -p wristkey-core

# Полная проверка (на машине с реальным toolchain):
cargo check --workspace
cd tauri/src-tauri && cargo tauri build

# Wear OS:
cd wear-os && ./gradlew assembleDebug

# Если менял байтовый формат BLE-сообщений или UUID характеристик —
# перепроверь ОБЕ стороны (Kotlin отправку и Rust парсинг) одновременно,
# это самое частое место рассинхронизации за весь проект.
```
