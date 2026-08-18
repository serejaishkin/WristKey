# WristKey — AI Handoff & Roadmap

> ⚠️ **ПРОЧИТАЙ ЭТОТ ФАЙЛ ПЕРВЫМ ДЕЛОМ.** Если тебя зовут Кими (или ты любой другой AI-ассистент, начинающий новую сессию по этому проекту) — сначала прочитай этот файл, затем текущий GitHub и только потом начинай менять код.
>
> Этот файл нужен для передачи долгосрочного инженерного контекста между сессиями. **AI_HANDOFF.md может отставать от последнего чата.** Последний прикреплённый чат может содержать более свежие эксперименты и решения, которые ещё не были зафиксированы здесь.
>
> История в разделе 6 — **append-only**. Старые записи не удалять и не переписывать ради актуальности. Если решение изменилось — добавить новую запись и явно указать, что старое решение отменено/заменено.

---

## 0. Быстрый старт

- **GUI — Tauri v2, не eframe.** Точка входа: `desktop/tauri/src-tauri/src/main.rs`. Старые `daemon/src/main.rs`, `gui.rs`, `pair_gui.rs`, `tray.rs` не являются текущей архитектурой.
- **Storage — SQLite, не sled.** `wristkey_core::SqliteStorage`.
- **Daemon — library crate.** `desktop/crates/daemon/src/lib.rs` содержит proximity loop и crypto unlock; `conn_mgr.rs` управляет BLE connections.
- **Crypto unlock НЕ должен обходиться RSSI.** Рабочий путь: `session.begin_unlock()` → GATT challenge write → notify response → `session.verify_unlock()` → `platform.unlock_screen()`.
- **Windows Credential Provider** — отдельный C#/.NET проект в `desktop/crates/credential-provider/`; он требует проверки на реальной Windows.
- **Логи ПК:** `tracing_appender::rolling::daily` создаёт `wristkey.YYYY-MM-DD`, а не `wristkey.log`. В Tauri есть Diagnostics/`get_log_dir`.
- **Wear OS:** GATT server — `WristKeyBleService.kt`; advertising разбит на primary + scan response из-за лимита legacy BLE advertising.

### Критическое правило о состоянии источников

Не считать `AI_HANDOFF.md` последним состоянием проекта автоматически.

При начале новой сессии используй следующую схему:

1. **Текущий GitHub-код** — фактическое состояние исходников.
2. **Прикреплённый последний чат** — самое свежее состояние рабочей сессии; он может быть новее `AI_HANDOFF.md`.
3. **AI_HANDOFF.md** — накопленная контрольная точка архитектуры и истории.
4. **`docs/`** — полный архив старых чатов и деталей.

Это не означает, что чат важнее Git. Если чат говорит, что файл изменён, но текущий Git этого не содержит, изменение считать **неподтверждённым/незакоммиченным**, пока фактический репозиторий не подтверждает обратное.

Разделяй состояния:

- **ПОДТВЕРЖДЕНО В GIT** — есть в текущем репозитории.
- **ПОДТВЕРЖДЕНО В ПОСЛЕДНЕМ ЧАТЕ** — результат подтверждён рабочей сессией, но ещё может отсутствовать в Git.
- **ЗАФИКСИРОВАНО В AI_HANDOFF** — долгосрочный инженерный контекст.
- **ИСТОРИЯ** — старое решение/эксперимент.
- **ПРЕДПОЛОЖЕНИЕ** — обсуждалось, но не подтверждено.

### Когда обновлять AI_HANDOFF

Не нужно менять этот файл после каждого сообщения или каждого эксперимента.

Обновляй его после значимой контрольной точки: крупного фикса, архитектурного изменения, подтверждённого бага, изменения протокола, нового устойчивого блокера или накопления нескольких связанных изменений.

Если работа ещё идёт и результат не подтверждён — не записывай гипотезу как факт.

---

## 1. Текущая архитектура

### 1.1 Desktop — Rust workspace

```text
desktop/
├── Cargo.toml
├── crates/
│   ├── core/                # crypto, SessionManager, SqliteStorage, Config
│   ├── crypto/              # ECDSA P-256 primitives
│   ├── ble/                 # btleplug BLE adapter
│   ├── daemon/              # proximity + crypto unlock loop, conn_mgr
│   ├── platform-win/        # Windows lock/vault/named pipe
│   ├── platform-linux/      # Linux security + PAM proof module
│   ├── platform-macos/      # macOS Keychain + PAM proof module
│   └── credential-provider/ # Windows Credential Provider COM project
└── tauri/
    ├── src/                 # vanilla JS UI
    └── src-tauri/           # Tauri v2 backend
```

Tauri — текущий desktop entry point. Старый eframe/egui GUI является только историей и не должен восстанавливаться.

### 1.2 Wear OS

```text
wear-os/app/src/main/java/com/wristkey/
├── MainActivity.kt
├── ble/WristKeyBleService.kt
├── security/SecurityManager.kt
├── sensors/MotionDetector.kt
├── WristKeySettings.kt
└── boot/BootReceiver.kt
```

`MainActivity.kt` в текущем Git действительно использует `WristKeyBleService`, показывает PIN, статус pairing и имя paired PC, но **пока не показывает MAC часов и отдельное имя подключённого ПК так, как требовалось в последнем чате**.

### 1.3 GATT protocol

Базовые UUID:

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

`RESPONSE_CHAR` и `UNLOCK_RESPONSE` используют `NOTIFY` + стандартный CCCD `0x2902`. Windows/btleplug должен сначала выполнить `subscribe()`, затем challenge/write.

Публичный ключ теперь читается отдельной `PUBLIC_KEY_CHAR`, а не должен встраиваться в response. На Wear OS он берётся из существующего Android Keystore `KeyStoreManager` и должен соответствовать ключу, которым подписывается challenge.

Pairing response текущей Wear OS реализации:

```text
ECDSA signature || user_present_flag(1)
```

**Важно:** предыдущая запись `signature(64) || user_present_flag(1) || public_key(65)` относится к старой протокольной схеме и заменена текущей отдельной `PUBLIC_KEY_CHAR`. Не возвращать public key в response без подтверждённой необходимости.

Public key — raw SEC1 `0x04 || X || Y`, не X.509 DER.

Advertising:

- primary packet — manufacturer data `0xFFFF`, PIN;
- scan response — device name/дополнительные данные в рамках legacy BLE лимита.

PIN — только визуальная sanity-проверка, не криптографический фактор.

### 1.4 Текущий BLE pairing flow

Текущая схема должна быть:

```text
Windows scan
  ↓
stop scan
  ↓
BLE connect
  ↓
GATT discovery
  ↓
subscribe RESPONSE (CCCD 0x2902)
  ↓
read PUBLIC_KEY
  ↓
write challenge
  ↓
Watch показывает Pair WristKey
  ↓
USER: Allow / Cancel
  ↓
Watch signs challenge
  ↓
notify RESPONSE to connected PC
  ↓
Windows verifies signature
  ↓
pairing complete
```

**Не использовать системное Windows Bluetooth Pairing как механизм WristKey pairing.** Наше pairing — прикладной GATT/crypto flow. Если Windows показывает системный диалог Bluetooth Pairing, это отдельная проблема, которую нужно диагностировать, а не считать успешным WristKey pairing.

---

## 2. Критичные повторяющиеся баги — НЕ чинить заново вслепую

| # | Баг | Где | Статус |
|---|---|---|---|
| 1 | `verify_unlock` генерировал новый challenge вместо отправленного | `core` | Исправлено, повторялось ≥2 раза |
| 5 | `device_id` читался из несуществующего диапазона response | `daemon` | Исправлено, повторялось ≥3 раза |
| 6 | Старые вызовы `complete_pairing`/`PairedDevice{}` после изменения арности | `core` | Исправлялось 4 раза |
| 14 | Crypto verification выпадала из unlock loop, RSSI сразу вызывал unlock | `daemon` | Исправлено; критичный регресс |
| — | `ADVERTISE_FAILED_DATA_TOO_LARGE` | Wear OS | Исправлено split advertising |
| — | PIN заменялся на статичный `WRST` | Wear OS | Исправлено, реальный PIN восстановлен |
| — | `CONFIRM_BUTTON` возвращал false | Wear OS | Исправлено через `pendingUserPresent` |
| — | Имя лог-файла не совпадало с реальным daily rolling файлом | Tauri | Исправлено |
| — | `Config` ломал сохранение из-за отсутствия serde defaults | core | Исправлено |
| — | `baseline_rssi` не сохранялся в desktop storage | daemon/Tauri | Исправлено |
| — | Мусорные `desktop/lib.rs`, `lib (2/3/4).rs` | desktop | Периодически появляются, Cargo их не использует |
| — | Два pipe server давали `Access denied` | platform-win | Исправлено lazy singleton / явный start |
| — | Старый `wear-os/app/.../app/MainActivity.kt` | Wear OS | Мёртвый файл, удалять; активный путь `com/wristkey/MainActivity.kt` |
| — | Windows `subscribe()` падал `HRESULT(0x80650003) / Не удается записать атрибут` | Wear OS GATT | Исправлено: добавлен CCCD `0x2902` к notify characteristics |
| — | После subscribe Windows не мог читать public key | Wear OS GATT | Исправлено в Git: добавлена `PUBLIC_KEY_CHAR` + READ из KeyStore |

---

## 3. Открытые вопросы и компромиссы

### 3.1 Samsung / BLE discovery

Samsung Galaxy Watch может скрывать кастомный advertising WristKey. Поэтому discovery использует несколько сигналов, но Samsung-specific сигналы (`fd50`, manufacturer ID `0x0075`) допустимы только как fallback для обнаружения кандидата.

После подключения необходимо проверять именно WristKey custom GATT service. Нельзя превращать Samsung `fd50` в протокол WristKey.

Не использовать широкий fallback вроде «любой BLE с RSSI > -80»: в него попадали Midea, телефоны, наушники и другие устройства.

### 3.2 RSSI / proximity

Последний анализ выявил проблему с порядком инициализации `RssiSmoother`: в daemon сейчас создаётся `RssiSmoother::new(-60)`, а затем при обнаружении устройства он может быть переинициализирован threshold устройства. Это место нужно проверять внимательно, потому что calibration и smoothing должны использовать baseline конкретного paired device.

Также нельзя создавать независимые BLE adapters для разных операций без необходимости. Последний чат обнаружил конфликт множественных Bluetooth adapters; calibration сейчас в Tauri создаёт отдельный `BtleplugAdapter::new()` и подключается напрямую. Это потенциально конфликтует с adapter/connection manager daemon.

### 3.3 Calibration

Tauri `calibrate_proximity` собирает RSSI 10 секунд, считает median, формирует threshold `median + 5` с ограничением `[-90,-20]`, отправляет его на `CONFIG_CHAR` и вызывает `SessionManager::update_baseline_rssi`.

Сам факт наличия этой логики **не означает, что calibration реально работает на устройстве**. Нужно отдельно проверить:

1. правильное paired device;
2. один и тот же BLE adapter/connection lifecycle;
3. RSSI samples;
4. запись threshold на часы;
5. сохранение baseline в SQLite;
6. использование baseline в proximity loop.

### 3.4 Windows Credential Provider

Credential Provider — отдельный C#/.NET COM проект. Для lock screen плитки недостаточно просто иметь Rust daemon или DLL в репозитории: COM registration, правильная архитектура DLL, registry entries и Windows logon integration должны быть реально проверены на Windows.

В прошлых чатах была попытка регистрации CLSID вручную через PowerShell. Регистрация командой прошла, но **наличие плитки WristKey на lock screen не было подтверждено**.

На машине пользователя `dotnet build`/`msbuild` не запускались из-за отсутствия .NET SDK/MSBuild. Это означает, что старые утверждения «DLL уже собрана» не считать доказательством полноценной установки CP без проверки фактического файла и регистрации.

### 3.5 Linux PAM

В текущем Git `platform-linux/src/lib.rs` уже содержит `pam_sm_authenticate`, который читает `~/.wristkey/.last_auth`, проверяет timestamp до 5 секунд и ownership файла. Это уже не просто старый `PAM_IGNORE` stub.

Но реальный PAM login flow на Linux ещё не считается доказанно рабочим только по наличию функции.

### 3.6 macOS PAM

В текущем Git `platform-macos/src/lib.rs` есть cfg-gated Keychain protector и PAM proof handler с 5-секундным окном. Реальная интеграция на macOS ещё требует проверки на macOS.

### 3.7 Текущий BLE pairing blocker

На реальном Galaxy Watch4 последняя подтверждённая тестовая цепочка дошла до:

```text
PC → connect
PC → GATT discovery
PC → subscribe RESPONSE
PC → pairing flow
Watch → Pairing request
Watch → Allow
```

Предыдущая ошибка:

```text
Pairing failed: ble error: subscribe: Error {
  code: HRESULT(0x80650003),
  message: "Не удается записать атрибут."
}
```

была вызвана отсутствием CCCD у notification characteristics и исправлена.

Следующая подтверждённая ошибка до последнего фикса была:

```text
Pairing failed: Failed to read public key:
ble error: read: Windows UWP threw error on read: GattReadResult(...)
```

Причина была подтверждена в коде: desktop ожидал public key, а GATT server не предоставлял отдельную READ characteristic. В коммите `05c95b5` добавлена `PUBLIC_KEY_CHAR` и чтение реального public key из Android Keystore.

**Статус после `05c95b5`: НЕ ПРОТЕСТИРОВАНО пользователем.** Следующий тест должен подтвердить, что `read public key` проходит.

### 3.8 Pairing UI на часах

В `PairingActivity` добавлен отдельный экран подтверждения. Он должен показывать:

```text
Pair WristKey
Windows PC
Allow this PC?
[ALLOW]
[CANCEL]
```

Экран рассчитан на маленький экран Wear OS; добавлен scroll как страховка. Пользователь ранее сообщил, что без scroll кнопка Allow уходила вниз — это было исправлено.

После `Allow` часы должны отправить подпись challenge + `user_present=1` через `RESPONSE_CHAR`.

### 3.9 Windows системный Bluetooth pairing

Пользователь сообщил, что после сброса pairing на часах Windows иногда сразу показывает системный запрос Bluetooth pairing. Это **не является желаемым механизмом WristKey pairing**.

Не считать системное Bluetooth pairing успешным. Нужно выяснить, почему Windows UWP/btleplug инициирует системное pairing для текущего GATT сценария, если оно повторится после успешного `PUBLIC_KEY` read.

---

## 4. Проверка перед изменениями

Перед изменением кода:

```bash
git status
git diff
```

После изменений:

```bash
cd desktop
cargo check
cargo test -p wristkey-core

cd ../wear-os
./gradlew assembleDebug
```

Для полного Tauri build:

```bash
cd desktop/tauri/src-tauri
cargo tauri build
```

Не считать «собралось» доказательством работоспособности BLE, unlock или Credential Provider.

Для текущего Wear OS BLE blocker обязательно тестировать на реальном Galaxy Watch4, потому что Windows UWP/btleplug и Android GATT Server дают device-specific поведение, которое обычный compile не проверяет.

---

## 5. Правило работы с ограничением длины чата

Новый AI должен:

1. загрузить актуальный GitHub;
2. прочитать `AI_HANDOFF.md`;
3. прочитать прикреплённый последний чат;
4. при необходимости искать детали в `docs/`;
5. сравнить чат с фактическим Git;
6. только после этого продолжить работу.

Не нужно читать весь `docs/` на каждой сессии. Это архив, а не обязательный полный контекст.

Если последний чат содержит более свежий эксперимент, которого ещё нет в `AI_HANDOFF.md`, использовать чат для текущего продолжения, но не выдавать его как подтверждённое состояние Git.

---

## 6. История проекта и накопленный контекст сессий

> **APPEND-ONLY.** Старые записи ниже не удалять и не переписывать ради актуальности. Если новое решение заменяет старое — добавлять новую запись.

### 6.1 2026-08-07 — переход от v0.1 к desktop-продукту

- Исходная архитектура: Rust workspace с `core`, `crypto`, `ble`, платформенными модулями, daemon и Wear OS.
- Началась доводка tray, GUI, pairing и Windows unlock.
- В ранних версиях использовался `sled`; позже заменён на SQLite из-за межпроцессных блокировок.
- Ранний GUI был eframe/egui; позднее архитектура перешла на Tauri.

### 6.2 2026-08-08 — BLE advertising и discovery

- Desktop первоначально ожидал manufacturer data `0xFFFF`.
- Wear OS advertising превышал legacy BLE limit 31 byte и давал `ADVERTISE_FAILED_DATA_TOO_LARGE`.
- Advertising разделён на primary packet + scan response.
- Samsung Galaxy Watch может не показывать кастомный advertising приложения.

### 6.3 2026-08-09 — pairing, motion gate, protocol fixes

- Исправлена критичная ошибка `verify_unlock`: challenge должен совпадать с отправленным часам.
- Response закреплён как `signature[64] || user_present[1] || public_key[65]`.
- Public key переведён на raw SEC1.
- `device_id` стал fingerprint SHA-256 public key.
- Motion gate оказался чувствителен к задержкам; отключать его полностью нельзя.

### 6.4 2026-08-10 — Samsung, MAC randomization, pairing UI

- Samsung показывал `fd50` и manufacturer ID `0x0075`, а кастомный service иногда не был виден.
- Предложенная замена WristKey UUID на Samsung UUID отменена.
- Samsung-specific признаки допустимы только для discovery fallback.
- MAC нельзя считать постоянным идентификатором из-за BLE Privacy.
- Добавлялись pairing dialog, Pair/Reject, MAC и fallback discovery.

### 6.5 2026-08-11/12 — Windows, storage locking, Credential Provider

- Выявлен `pairing database still in use` при одновременной работе daemon и GUI с sled.
- Решение — SQLite и единый основной desktop process.
- Начата интеграция Windows Credential Provider и named pipe.
- Credential Provider требует реального Windows-тестирования.

### 6.6 2026-08-13 — discovery и отказ от широкого RSSI fallback

- Fallback «любой BLE с RSSI > -80» признан неправильным: находились Midea, телефоны, наушники и т.д.
- RSSI не является доказательством принадлежности устройства.
- Samsung `fd50`/`0x0075` — только discovery candidate.
- После подключения нужен WristKey custom GATT service.

### 6.7 2026-08-14 — pairing фактически заработал

- В рабочей конфигурации pairing был подтверждён логами Wear OS:
  `Pairing request -> showing dialog`,
  `confirmPairing -> response built (user_present=true)`,
  `notifyCharacteristicChanged SUCCESS`.
- Это контрольная точка: pairing-схема реально работала на устройстве.
- Следующий этап — реальный unlock и Credential Provider.

### 6.8 2026-08-15 — Tauri + SQLite и BLE blocker

- Desktop GUI закреплён на Tauri v2.
- Storage закреплён на SQLite.
- Tauri crash `there is no reactor running` исправлялся запуском фоновой async-задачи через собственный Tokio runtime/thread.
- Приложение после фикса запускалось: Tauri setup, tray, storage, crypto, session, Windows pipe server и BLE adapter проходили инициализацию.
- Основной blocker тогда: `BLE adapter ready`, но часы не появлялись в поиске.

### 6.9 2026-08-15 — сохранение истории и handoff

- `AI_HANDOFF.md` впервые был дополнен инженерным append-only журналом из архивов чатов.
- Зафиксировано правило: старую историю не переписывать; изменения добавлять новыми подразделами.
- Добавлен индекс архивных документов, включая `2.4.zip` и отдельные `.docx` из истории.

### 6.10 2026-08-15/16 — последние чаты: фазы vault/unlock/CP и новые проверки

Источник: прикреплённый архив `обновить.zip`, содержащий:

- `ZIP-замена.docx`
- `Продолжение проекта WristKey.docx`
- `fix claud2.docx`
- `WristKey проект завершение.docx`

#### 6.10.1 Vault и план фаз

В чатах был сформирован план:

1. унификация vault/storage;
2. BLE unlock protocol;
3. Windows Credential Provider;
4. Linux PAM;
5. macOS PAM;
6. Wear OS UI/confirmation.

Обсуждался формат `~/.wristkey/devices.json`, AES-GCM для `passwordEnc`, pairing key и платформенная защита ключа.

**Важно:** часть этого плана была предложением архитектуры, а не подтверждённым финальным состоянием. Не считать весь предложенный JSON/protocol автоматически текущим протоколом, пока его нет в Git.

#### 6.10.2 Phase 2/3 и pipe

В чатах создавались патчи для Wear OS unlock protocol, daemon named pipe и Credential Provider.

На промежуточном этапе pipe server был прямо описан как **stub**, возвращающий `test_password`; это не является рабочим security flow.

Позднее в текущем Git уже появился реальный Windows pipe server в `daemon/src/lib.rs`, который запускает `pipe_server::run(...)` и обрабатывает `unlock`. Поэтому старый `test_password` считать историческим промежуточным состоянием, а не текущим кодом.

#### 6.10.3 Windows pipe access denied

Была обнаружена причина повторяющегося:

```text
Pipe create error: Отказано в доступе
```

Причина — несколько экземпляров pipe server: `WindowsSecurity::new()` создавал сервер в конструкторе, а другой объект пытался создать pipe с тем же именем.

Решение, предложенное и затем отражённое в репозитории: lazy/singleton pipe server + явный `WindowsSecurity::start_pipe_server()`, а `set_windows_password` должен использовать чистый `WindowsVault::new()` без побочных эффектов.

#### 6.10.4 Wear OS screen-off

В чатах зафиксирована проблема: при гашении экрана Wear OS Activity закрывалась/теряла UI.

Предлагались:

- `android:keepScreenOn="true"` для Activity;
- `android:launchMode="singleTask"`;
- `WAKE_LOCK`;
- `PARTIAL_WAKE_LOCK` внутри `WristKeyBleService`;
- foreground service.

Часть этих изменений присутствует в истории/патчах, но реальное поведение нужно подтверждать на часах. Не считать «патч создан» равным «проблема полностью решена».

#### 6.10.5 Windows Lock Screen / Credential Provider

В чатах пытались зарегистрировать CLSID Credential Provider вручную через PowerShell. Registry-команды выполнялись успешно, но **плитка WristKey на Windows Lock Screen не была подтверждена как появившаяся**.

Также на тестовой машине отсутствовали `dotnet` SDK и `msbuild`, поэтому команда `dotnet build -c Release` не могла быть выполнена.

Следовательно, текущий статус CP:

- код/регистрация обсуждались и частично реализованы;
- lock-screen tile — **НЕПОДТВЕРЖДЕНО**;
- реальный Winlogon unlock — **НЕПОДТВЕРЖДЕНО**.

#### 6.10.6 Логи

В чатах обнаружена ошибка ожидания файла `wristkey.log`. Реальный `tracing_appender::rolling::daily(&log_dir, "wristkey")` создаёт `wristkey.YYYY-MM-DD`.

Были добавлены/обсуждались:

- `get_log_dir` Tauri command;
- Diagnostics в Settings;
- отображение пути к логам;
- copy-to-clipboard.

Это исправление уже отражено в текущем handoff и должно считаться проверяемым по текущему Git.

#### 6.10.7 BLE scan, RSSI и calibration — последние найденные проблемы

Последняя аналитическая сессия в `fix claud2.docx` подтвердила следующие направления:

- crypto unlock fix сохранился в Git: `Daemon::run()` вызывает proximity → `unlock_with_crypto()` → `begin_unlock()` → GATT challenge → response → `verify_unlock()` → `platform.unlock_screen()`;
- обнаружена проблема с порядком инициализации `RssiSmoother`;
- `ConnectionManager` кэширует BLE connections по ID и проверяет живость через RSSI;
- Tauri `calibrate_proximity` создаёт **отдельный** `BtleplugAdapter`, подключается к адресу устройства, собирает RSSI 10 секунд и пишет threshold;
- это может конфликтовать с основным BLE adapter/daemon lifecycle;
- отдельно выявлен конфликт множественных Bluetooth adapters;
- `MainActivity.kt` текущего Git показывает статус и имя paired PC, но **не показывает требуемые MAC часов и отдельные данные текущего подключённого ПК**.

Это последние известные направления диагностики. Не начинать с нуля поиск проблемы discovery/calibration, пока не проверены эти места.

### 6.11 2026-08-18 — BLE pairing на Galaxy Watch4: от connect до public-key read

Это текущая контрольная точка ветки `fix/wristkey-20260818`.

#### 6.11.1 Discovery / connection

Пользователь тестирует реальный Samsung Galaxy Watch4. На Windows устройство появляется примерно как:

```text
Galaxy Watch4 (N5XN)
RSSI: -75 dBm
6A:61:5D:62:88:BB
```

Имя часов ранее появлялось нестабильно. Важно не считать BLE address постоянным identity из-за возможной BLE Privacy/randomization.

В `ConnectionManager` добавлена остановка active scan перед connect и retry до 4 раз с возрастающей задержкой. Цель — убрать Samsung/Windows ситуацию, когда устройство уже найдено, но GATT connect завершается `Not connected`.

#### 6.11.2 Wear OS pairing UI

Добавлен `PairingActivity`. Теперь при подключении нового ПК/получении challenge часы действительно показывают запрос подтверждения. Пользователь подтвердил, что на часах появляется `Pairing request`.

Проблема с кнопкой `Allow`, уходившей за нижнюю границу экрана, исправлена переработкой UI под маленький Wear OS display и добавлением scroll fallback.

#### 6.11.3 CCCD / subscribe

Первоначально Windows падал на:

```text
Pairing failed: ble error: subscribe: Error {
  code: HRESULT(0x80650003),
  message: "Не удается записать атрибут."
}
```

Причина: notification characteristics не имели стандартного CCCD descriptor `00002902-0000-1000-8000-00805f9b34fb`.

В `WristKeyBleService.kt` CCCD добавлен к `RESPONSE_CHAR` и `UNLOCK_RESPONSE`, а `onDescriptorWriteRequest()` обрабатывает запись CCCD.

После этого ошибка `subscribe` исчезла в тесте и flow дошёл дальше до чтения public key.

#### 6.11.4 Public key GATT characteristic

Следующая ошибка была:

```text
Pairing failed: Failed to read public key:
ble error: read: Windows UWP threw error on read: GattReadResult(...)
```

Причина: desktop ожидал public key, но Wear OS GATT server не предоставлял отдельный READ attribute для него.

Коммит `05c95b5` добавил:

```text
PUBLIC_KEY_CHAR_UUID = a1b2c3d4-e5f6-7890-abcd-ef1234567893
```

Характеристика имеет `PROPERTY_READ`/`PERMISSION_READ`. Значение берётся из `KeyStoreManager` и должно быть raw SEC1 public key.

**Важно: после `05c95b5` пользователь ещё не подтвердил повторный тест. Поэтому считать `read public key fixed` кодовым исправлением, но не подтверждённым hardware result.**

#### 6.11.5 Current next test

После установки APK из последнего коммита нужно проверить чистый pairing:

1. обновить `fix/wristkey-20260818`;
2. `./gradlew clean`;
3. `./gradlew :app:assembleDebug`;
4. установить APK на Galaxy Watch4;
5. сбросить старое WristKey pairing на часах;
6. при необходимости удалить старую системную Bluetooth запись Windows;
7. запустить desktop daemon;
8. выполнить pairing.

Ожидаемая последовательность:

```text
connect
→ GATT discovery
→ subscribe RESPONSE
→ read PUBLIC_KEY
→ write challenge
→ PairingActivity
→ Allow
→ signature + user_present
→ Windows verification
```

Если `read PUBLIC_KEY` проходит, следующий ожидаемый blocker может быть в формате ECDSA signature. Android `SHA256withECDSA` потенциально возвращает DER-encoded ECDSA signature, тогда как desktop protocol исторически ожидал fixed-size `r || s` (64 bytes). Это **пока предположение**, не считать подтверждённой ошибкой до следующего теста.

Если после `Allow` возникает `auth response too short` или `invalid signature`, первым делом сравнить фактическую длину/байты response на Windows и Android, затем привести формат подписи к единому protocol representation.

#### 6.11.6 Windows system Bluetooth pairing

Пользователь сообщил, что после сброса pairing на часах Windows иногда сразу предлагает системное Bluetooth pairing. Это не должно считаться успехом WristKey pairing. Если повторится после прохождения `PUBLIC_KEY` read, исследовать btleplug/UWP pairing state и свойства GATT service отдельно.

#### 6.11.7 Recent commits on current branch

- `fix/wristkey-20260818`: основной рабочий branch для текущей BLE pairing отладки.
- `05c95b5` — `fix: expose watch public key over GATT`.
- Ранее в этой ветке добавлялись CCCD/targeted notifications, retry connect/stop scan и `PairingActivity`.

**Не мержить `fix/wristkey-20260818` в `main`, пока реальный Galaxy Watch4 pairing не пройдёт end-to-end.**
