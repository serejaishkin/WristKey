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

PIN is shown to the person during pairing (desktop GUI) purely as a visual sanity check ("is this really the watch I'm holding") — it is **not cryptographic**; the real security is the ECDSA challenge-response. If you ever see manufacturer data replaced with a static marker like `"WRST"` instead of an actual random PIN — that's a regression, the desktop still parses `manufacturer_data[..4]` as an ASCII PIN string and will display garbage.

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

---

## 6. История проекта и накопленный контекст сессий

> **ВАЖНО:** этот раздел является append-only журналом. Не переписывай старые записи и
> не удаляй их ради "актуальности". Если новая сессия меняет архитектуру или
> исправляет старое решение, добавь новую запись с датой и явно укажи, что именно
> стало неактуальным. Полная исходная переписка хранится в чат-документах из архива
> `2.4.zip`; этот раздел содержит инженерный конспект, который должен переживать
> обрезание контекста чата.

### 6.1 2026-08-07 — переход от v0.1 к рабочему desktop-продукту

- Исходная архитектура: Rust workspace с `core`, `crypto`, `ble`, платформенными
  модулями, daemon и Wear OS приложением.
- Началась доводка system tray, GUI, pairing и Windows unlock.
- В ранних сессиях использовался `sled`; позднее это решение признано источником
  межпроцессных блокировок и заменено на SQLite.
- В ранних версиях GUI/daemon существовало несколько отдельных бинарей и
  `eframe/egui`. Это исторический этап. Финальная архитектура перешла на Tauri.

### 6.2 2026-08-08 — BLE advertising и обнаружение

- Desktop первоначально ожидал `manufacturer_data` с ключом `0xFFFF`.
- На Wear OS advertising не укладывался в legacy BLE лимит 31 байт:
  service UUID + PIN + `device_id` приводили к `ADVERTISE_FAILED_DATA_TOO_LARGE`.
- Advertising был разделён на primary packet и scan response.
- PIN и короткий fingerprint `device_id` должны использоваться для визуальной
  идентификации и повторного обнаружения, но PIN не является криптографическим
  фактором.
- Появилась важная реальность Samsung Galaxy Watch: часы могут не отдавать
  кастомный advertising приложения. Поэтому нельзя считать отсутствие
  `0xFFFF`/PIN доказательством, что часов рядом нет.

### 6.3 2026-08-09 — pairing, motion gate и protocol fixes

- Обнаружена критичная ошибка `verify_unlock`: desktop генерировал новый
  challenge вместо проверки того, который реально был отправлен часам.
  Исправлено через состояние `SessionState::Verifying`.
- Исправлен порядок pairing response:
  `signature[64] || user_present[1] || public_key[65]`.
- Исправлен формат публичного ключа: Android должен передавать raw SEC1
  `0x04 || X || Y`, а не X.509 DER.
- `device_id` вынесен в отдельный стабильный fingerprint на основе SHA-256
  публичного ключа, чтобы не зависеть от BLE MAC.
- Motion gate оказался чувствителен к задержке между нажатием/движением и
  приходом challenge. Для диагностики временно рассматривалось увеличение окна
  активности; отключать motion gate насовсем нельзя.
- Обнаружено, что `device_id` нельзя извлекать из несуществующего диапазона
  response payload — pairing должен получать его из advertising/стабильного
  идентификатора.

### 6.4 2026-08-10 — Samsung, MAC randomization и pairing UI

- Samsung Galaxy Watch в скане показывали системный Accessory Service
  (`fd50`) и manufacturer ID `0x0075`, тогда как кастомный WristKey service
  иногда не был виден.
- Было ошибочно предложено заменить универсальные WristKey UUID на Samsung
  UUID. Это решение **отменено**: протокол WristKey должен оставаться
  универсальным для разных Wear OS часов.
- Правильная стратегия: использовать Samsung-сигналы только как fallback
  обнаружения, но после подключения искать именно кастомный WristKey GATT
  service. Нельзя превращать Samsung `fd50` в основной протокол WristKey.
- В GUI убран ложный PIN `----`, если устройство его не advertising-ит.
- Добавлены отображение MAC, pairing dialog/fallback, кнопки Pair/Reject и
  сохранение адреса.
- BLE Privacy приводит к смене MAC. Поэтому MAC нельзя считать постоянным
  идентификатором paired device; приоритет — `device_id`, затем другие
  устойчивые признаки.
- Был добавлен BLE bonding как вспомогательный механизм, но он не заменяет
  криптографическое challenge-response.

### 6.5 2026-08-11/12 — Windows, storage locking и Credential Provider

- Выявлена проблема `pairing database still in use`: два процесса
  (`wristkeyd` и GUI) одновременно открывали `sled` DB.
- Сначала применялся workaround с остановкой daemon перед GUI.
- Архитектурное решение — один основной desktop-процесс + SQLite, чтобы GUI,
  daemon и Tauri могли совместно работать с одной БД без sled lock.
- Начата интеграция Windows Credential Provider из
  `WristKeyCredentialProvider.cs`.
- Windows platform получил named pipe для связи с Credential Provider и
  хранилище пароля через Windows CNG/TPM/software fallback.
- Важно: Credential Provider требует тестирования на реальной Windows; обычная
  сборка Rust не доказывает работоспособность Winlogon COM-интеграции.

### 6.6 2026-08-13 — универсальное обнаружение и отказ от широкого RSSI fallback

- Широкий fallback вида "любой BLE девайс с RSSI > -80" оказался плохим:
  в список попадали Midea, наушники, телефоны и другие устройства.
- Этот подход считать регрессом. RSSI сам по себе не является доказательством,
  что устройство — WristKey.
- Для Samsung fallback использовались Accessory Service `fd50` и manufacturer
  ID `0x0075`, но только для **обнаружения кандидата**, не для протокола.
- При подключении desktop должен проверять наличие WristKey custom GATT service.
  Если custom service отсутствует — сообщать об этом явно, а не подменять
  протокол Samsung-сервисом.
- Обнаружена проблема с пустыми/неожидаемыми Android logcat: сначала необходимо
  проверять `adb devices` и правильный tag `WristKeyBleService`, а затем искать
  crash/fatal в общем logcat.

### 6.7 2026-08-14 — pairing фактически заработал

- В одной из рабочих конфигураций pairing был успешно выполнен.
- Подтверждение из логов часов:
  `Pairing request -> showing dialog`,
  `confirmPairing -> response built (user_present=true)`,
  `notifyCharacteristicChanged SUCCESS`.
- Это важная контрольная точка: криптографическая/прикладная pairing-схема
  может работать на реальном устройстве. Если новая архитектура перестала
  pairing-ить, сначала сравнивать её с этой рабочей точкой, а не заново
  перепридумывать протокол.
- После pairing следующим этапом должен быть проверен реальный unlock PC и
  затем Windows Credential Provider.

### 6.8 2026-08-15 — финальный переход на Tauri + SQLite и текущий блокер

- Desktop GUI окончательно закреплён на **Tauri v2**. Старый `eframe/egui`
  GUI больше не является рабочей точкой входа.
- Storage окончательно переведён на **SQLite** (`SqliteStorage`). Старый sled
  больше не должен возвращаться только ради совместимости со старым кодом.
- В Tauri был обнаружен crash:
  `there is no reactor running, must be called from the context of a Tokio 1.x runtime`
  из `tokio::spawn` внутри `setup()`.
- Исправление: запуск фоновой async-задачи через отдельный
  `std::thread::spawn` + собственный Tokio runtime.
- После этого приложение стартует нормально. Зафиксированный лог:
  - Tauri setup проходит;
  - tray создаётся;
  - storage/crypto/session/platform создаются;
  - Windows named pipe server запускается;
  - daemon auto-start проходит;
  - `wristkey_ble: BLE adapter ready`.
- **Текущий блокер на 2026-08-15:** приложение запускается, но часы не появляются
  в поиске. Последний лог заканчивается на `BLE adapter ready` без событий
  обнаружения.
- Следующая диагностика должна начинаться с текущего
  `desktop/crates/ble/src/lib.rs` и пути вызова scan из Tauri, а не с переписывания
  Wear OS протокола.
- Отдельно проверить, что Tauri UI действительно вызывает scan API и получает
  результат, а daemon не только инициализирует BLE adapter.
- Затем проверить фильтр обнаружения: он не должен быть слишком узким для
  Samsung, но и не должен возвращать весь BLE мусор.
- После восстановления scan необходимо отдельно проверить:
  1. обнаружение Galaxy Watch;
  2. обнаружение не-Samsung Wear OS часов;
  3. `discover_services()` после подключения;
  4. pairing;
  5. unlock;
  6. lock on departure;
  7. Credential Provider.

### 6.9 Источники истории

История этой секции собрана из переданных чат-документов, включая:

- `WristKey Monorepo Setup.docx`
- `Ошибка Ble.docx`
- `WristKey.docx`
- `Ошибка сборки APK.docx`
- `WristKey配对失败.docx`
- `WristKey配对错误.docx`
- `Проблема BLE Advertising.docx`
- `WristKey Build Failure.docx`
- `Проверка репозитория.docx`
- `Продолжить проект WristKey.docx`
- `Репо-реш.docx`
- `Метод не найден.docx`
- `WristKey сборка.docx`
- `WristKey завершение.docx`
- `fix claud.docx`
- `fix claud2.docx`
- `продолжить.docx`
- и остальные документы из переданного архива `2.4.zip`.

Для следующего AI-сессии **не требуется загружать все эти документы заново**:
`AI_HANDOFF.md` должен содержать инженерно значимый итог. Если в новой сессии
появится новый чат-архив, его не надо использовать для перезаписи этого раздела:
нужно добавить новый подраздел с датой.

---

## 7. Правило сохранения истории — append-only

1. **Никогда не переписывать существующую историю `AI_HANDOFF.md`.**
2. Новая сессия = новый подраздел с датой в разделе 6.
3. Исправление старого решения не удаляет старую запись: добавить запись
   `ОТМЕНЕНО` / `ЗАМЕНЕНО` и указать новое решение.
4. Текущую архитектуру держать в разделах 0–5, а историю решений — в разделе 6.
5. Ссылки на старые чат-документы сохранять как индекс источников, но не тащить
   в handoff полный текст переписки.
6. После каждой рабочей сессии обновлять как минимум:
   - текущий блокер;
   - что реально проверено;
   - что только предположено;
   - какие решения отменены;
   - следующий конкретный диагностический шаг.
7. Не считать "собралось" доказательством работоспособности BLE/unlock.
   Контрольные точки должны подтверждаться реальными логами устройства/ПК.
