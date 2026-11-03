# WristKey — дизайн автологина с часами (v2)

Статус: **проект, не реализовано**. Цель — разблокировка Windows часами в стиле Apple Watch ↔ Mac: подошёл с часами → экран входа открылся сам; без часов → обычный пароль.

## 1. Что нельзя использовать (проверено по документации)

| Вариант | Почему нет |
|---|---|
| Windows Companion Device Framework (`SecondaryAuthenticationFactor*`) | Класс помечен **deprecated**, требует *restricted capability* `secondaryAuthenticationFactor`; без одобрения Microsoft (cdfonboard@microsoft.com) вызовы падают. [Док](https://learn.microsoft.com/en-us/uwp/api/windows.security.authentication.identity.provider.secondaryauthenticationfactorinfo) |
| `unlock_screen()` из пользовательского процесса | Windows не даёт разблокировать сессию снаружи; рабочий путь только Credential Provider (CP). `platform-win::unlock_screen` — заглушка. |
| «Hibernation Mode» из чужого совета | Не разблокирует, отброшено ранее. |

Единственный поддерживаемый путь: **CP + автологон**. Механика из доков: флаг `pbAutoLogonWithDefault` в `GetCredentialCount` заставляет LogonUI сразу вызвать `GetSerialization` ([док](https://learn.microsoft.com/en-us/windows/win32/api/credentialprovider/nf-credentialprovider-icredentialprovider-getcredentialcount)); чтобы перечитать его по внешнему событию, провайдер зовёт `ICredentialProviderEvents::CredentialsChanged` из фонового потока ([обсуждение](https://kaweah.freedombox.rocks/kiwix/stackoverflow.com_en_all_2023-11/questions/8619323/windows7-credential-provider-to-automatically-logon-user-on-the-hardware-event)). Это уже заложено в нашей DLL (эталон).

## 2. Что не так в текущей схеме

1. Автологон решается по «намёку» присутствия (8 с), а **БЛЕ-раунд с часами происходит уже внутри `GetSerialization`** → LogonUI «висит» до 25 с, а при отказе/таймауте плитка остаётся в странном состоянии.
2. Нет понятия «сессия разблокировки»: демон не знает, показан ли экран входа, и не готовит ответ заранее.
3. Нет защиты от петель/блокировки учётки: неверный пароль → автологон снова → lockout.
4. Нет режимов (тихий / с подтверждением) и учёта «часы на руке».
5. Разработку тормозит то, что любая проверка требует часов + блокировки экрана.

## 3. Новая модель: «grant» подготавливается заранее

```
 Watch ──BLE──▶ Daemon (SYSTEM service) ◀──pipe──▶ CP DLL (в LogonUI)
                 │ Presence FSM                     │ тонкий: только UI + пакует KERB
                 │ UnlockSession FSM                │
                 └ Grant store (память, TTL)        │
```

### 3.1 Presence FSM (демон, есть) 
`AWAY → NEAR → PRESENT` по Калману+гистерезису (`RssiSmoother`). Выход только факты: `present`, `since`, `rssi`.

### 3.2 UnlockSession FSM (новое, демон)
Состояния: `UNLOCKED → LOCKED_UI_SHOWN → PREAUTH → GRANT_READY → CONSUMED → UNLOCKED`, ветки `FAILED(backoff)`, `DISABLED`.

- CP шлёт по pipe `{"action":"ui_shown","scenario":"unlock|logon","user":"DOM\\u","sid":"S-1-5-.."}` (из `SetUsageScenario`/`Advise`) и `ui_hidden`.
- Пока `LOCKED_UI_SHOWN` и `present` → демон **сам** запускает challenge/response (`PREAUTH`), не дожидаясь CP. На часах — запрос подтверждения (режим B) или тихая подпись (режим A).
- Успешная подпись → `GRANT_READY`: `{grant_id, nonce, sid, expires=now+15s, single_use}` лежит только в памяти демона. Пароль **не** покидает демон до потребления.
- Демон отвечает CP на `poll` (или CP получает событие по длинному опросу) `{"state":"grant_ready"}` → CP зовёт `CredentialsChanged` → `GetCredentialCount` ставит autologon → `GetSerialization` делает `consume{grant_id}` и мгновенно получает пароль → `KERB_INTERACTIVE_UNLOCK_LOGON`.
- Нет grant → плитка показывает статус из `state` («Ищу часы…», «Подтвердите на часах», «Часы далеко»), кнопка «Unlock» запускает PREAUTH вручную.

### 3.3 Режимы (`Config.unlock_mode`)
| Режим | Условие | Для кого |
|---|---|---|
| `silent` | present + часы **на руке** (датчик off-body, проверяет сама часы) + недавнее движение; подпись без касания | как Apple Watch |
| `confirm` (по умолчанию) | present + подпись после тапа на часах | безопаснее |
| `manual` | только по кнопке на плитке | без автологона |

Дополнительно: после **явной блокировки пользователем** (Win+L) первые N секунд `silent` не срабатывает (защита от мгновенного «возврата» при уходе от ПК).

### 3.4 Безопасность
- Grant: одноразовый, TTL 15 с, привязан к `sid` и `ui_shown` сессии, nonce из свежего challenge (`UNLOCK_DOMAIN` для LAN уже есть).
- Pipe: SDDL SYSTEM/Administrators (есть). Пароль DPAPI (есть).
- **Анти-lockout:** `ReportResult` с ошибкой → CP шлёт `unlock_failed` → демон: grant сгорает, backoff 30 с → 2 мин → `DISABLED` до ручного входа паролем; никогда не более 2 автопопыток подряд.
- Лог аудита: каждый grant/consume/fail с причиной в `wristkey-service.log`.

## 4. Протокол pipe v2 (JSON-lines, обратно совместим)
Существующие: `unlock`, `get_username`, `proximity`, `set_password`, … остаются.

Новые:
```
→ {"action":"ui_shown","scenario":"unlock","user":"PC\\sergei","sid":"S-1-5-21-…"}   ← {"status":"success","state":"PREAUTH"}
→ {"action":"session_state"}      ← {"status":"success","state":"GRANT_READY","grant_id":"…","mode":"confirm","present":true,"hint":"Подтвердите на часах (12с)"}
→ {"action":"consume","grant_id":"…"}  ← {"status":"success","password":"…"} | {"status":"error","message":"…"}
→ {"action":"unlock_failed","code":"0xC000006A"} ← {"status":"success","state":"FAILED","retry_in":30}
→ {"action":"ui_hidden"}
```
Правило: `unlock` (старый, блокирующий) остаётся как запасной путь для кнопки.

## 5. Облегчение разработки (главное)

1. **Фейковые часы** — `wristkeyd --mock-watch`: программный ECDSA-подписчик за тем же трейтом `BleAdapter`; управляется через `wristkeyctl mock approve|deny|away|present`. Покрывает FSM без железа и без Windows.
2. **`wristkeyctl`** (CLI, один бинарь на `pipe`): `state`, `ui-shown`, `consume`, `fail`, `log -f`. Любую ветку FSM можно прогнать из консоли, не блокируя экран.
3. **Чистая FSM** (`daemon/src/session.rs`): `fn step(state, event, now) -> (state, actions)` без IO → юнит-тесты `cargo test` на Linux (TTL, backoff, одноразовость, режимы, Win+L-окно).
4. **Тест-харнесс CP** (CMake, не регистрируется в LogonUI): фейковый `ICredentialProviderEvents` + тестовый pipe-сервер проверяют: `ui_shown` отправлен, `CredentialsChanged` вызван ровно на фронте `grant_ready`, `consume` в `GetSerialization`, `unlock_failed` в `ReportResult`.
5. **VM-сценарий**: Windows-VM со снапшотом + `build-dll.cmd` + `regsvr32`; блокировка `Win+L` → `wristkeyctl mock approve` → вход. Записать как чек-лист.

### Сценарий без часов (Администраторская консоль)
```
setx /M WRISTKEY_MOCK_WATCH 1         # затем перезапустить службу: sc stop WristKey & sc start WristKey
cargo run -p wristkeyctl -- watch     # в 2-м окне: живое состояние
wristkeyctl mock presence on          # «часы рядом»
wristkeyctl ui-shown                  # экран входа показан -> preauth
wristkeyctl mock approve              # «часы подписали» -> grant_ready
wristkeyctl consume <grant_id>        # пароль выдаётся один раз
wristkeyctl fail ; wristkeyctl state  # backoff 30 с -> 2 мин -> disabled
wristkeyctl reset                     # как вход паролем
```
Не оставляйте `WRISTKEY_MOCK_WATCH` на рабочей машине: mock_approve кладёт произвольный пароль в grant.

## 6. План работ (каждый шаг отдельным коммитом, DLL — минимальные диффы)

| # | Шаг | Где | Проверка без Windows |
|---|---|---|---|
| 1 | ✅ FSM + 12 тестов: `core/src/unlock_session.rs` (в `core`, т.к. `daemon` не собирается на Linux) | `core` | `cargo test -p wristkey-core --lib unlock_session` |
| 2 | ✅ Pipe v2: `ui_shown`, `ui_hidden`, `user_locked`, `on_body`, `manual_unlock`, `session_state`, `consume`, `unlock_failed`, `reset_session` (`daemon/lib.rs`, не собрано — Windows-only) | `daemon/lib.rs` | юнит + `wristkeyctl` |
| 3 | ✅ PREAUTH: `session_driver` (демон сам запускает challenge), `Hub` (grant + секрет с обнулением), `Config.unlock_mode`. Пока `silent` = тот же обмен с тапом: тихая подпись нужна на стороне часов (шаг 6) | `daemon`, `core` | mock-watch |
| 4 | ✅ `wristkeyctl` (`desktop/crates/wristkeyctl`, 3 теста) + mock-режим демона: служба с `WRISTKEY_MOCK_WATCH=1` принимает `mock presence|approve|deny` вместо часов | `daemon`, `wristkeyctl` | см. «Сценарий без часов» |
| 5 | ✅ CP: `ui_shown`/`ui_hidden` из потока слежения, опрос `session_state`, автологон только при `grant_ready`, `consume` в `GetSerialization`, `unlock_failed` из `ReportResult`; со старой службой — прежний путь (`unlock`/`proximity`). Разрешено владельцем 2026-11-03. mingw-сборка проходит, на экране входа не проверено | `Provider.cpp` | mingw + сценарий без часов |
| 6 | ✅ Часы: off-body датчик (`OnBodyDetector`), тихая подпись только на руке + при недавнем движении; PC в режиме `confirm` добавляет к challenge флаг-байт (25-й) «обязательный тап», часы отрезают его перед подписью. **Часы и ПК нужно обновить вместе**: старое приложение на часах подпишет 25 байт, и проверка на ПК не пройдёт. Kotlin не собирался (нет Android SDK) | Wear OS | вручную |
| 7 | UI приложения: выбор режима, статус сессии, «недавние попытки» | Tauri | вручную |
| 8 | Win+L-окно, backoff, аудит-лог | `daemon` | юнит |

## 7. Решения владельца
- Режимы: поддерживаются оба (`silent` и `confirm`), по умолчанию `confirm`; `silent` без данных on-body сам работает как `confirm`.
- Датчик on-body: наличие не подтверждено, определяется на часах в рантайме.

## 7a. Ещё открыто
1. Режим по умолчанию: `confirm` (рекомендую) или `silent`?
2. Есть ли у часов датчик on-body в нужной модели (Wear OS `Sensor.TYPE_LOW_LATENCY_OFFBODY_DETECT`)? Без него `silent` небезопасен.
3. Разрешено ли менять эталонную DLL (шаг 5)? Объём: ~60 строк, только pipe-вызовы.
4. LAN/MSA-путь (часы без BLE) вести в тот же grant-механизм (рекомендую) или оставить отдельно?
