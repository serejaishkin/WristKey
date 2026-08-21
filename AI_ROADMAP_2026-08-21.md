# WristKey — AI Roadmap / Handoff checkpoint

**Дата: 2026-08-21**  
**Ветка: `fix/wristkey-20260818`**

## Текущий этап

### Сделано в Git

- Wear OS pairing сохраняется на часах.
- `ProximityRssiTracker` использует подтверждение по последовательным raw RSSI samples.
- Desktop proximity loop использует paired-device identity и cryptographic challenge/response для unlock.
- Tauri desktop GUI остаётся текущим desktop entry point.
- Desktop pairing ранее создавал `PairedDevice` только в `MemoryStorage`, поэтому после убийства процесса список paired devices исчезал.
- Исправлено: Tauri desktop теперь открывает persistent `SqliteStorage` при старте.
- SQLite storage используется для paired devices, baseline RSSI, device identity и локального encrypted credential field.
- Если persistent SQLite storage не открывается, приложение теперь завершает запуск с ошибкой вместо тихого перехода на volatile memory storage.

### Последний commit

- `83dcb98` — `Use persistent SQLite storage for desktop pairing`

## Следующая проверка — desktop restart / reconnect

После сборки проверить строго этот сценарий:

1. Запустить desktop WristKey.
2. Выполнить pairing с часами.
3. Убедиться, что paired device отображается в GUI.
4. Полностью завершить desktop процесс.
5. Запустить desktop снова.
6. Проверить, что paired device всё ещё отображается.
7. Проверить, что daemon начинает BLE discovery без нового pairing.
8. Проверить automatic reconnect / `get_or_connect()` для известного устройства.
9. Проверить cryptographic unlock после reconnect.

Важно: сохранение `PairedDevice` и восстановление списка устройств теперь отделены от lifetime desktop процесса. Само BLE connection состояние намеренно не сохраняется — после restart соединение должно быть создано заново через discovery + known-device matching.

## Calibration — отдельный блок

Текущая ошибка:

`Calibration failed: Command calibrate_proximity not found`

Это не проблема SQLite/reconnect. Нужно отдельно проверить GATT `CONFIG_CHAR` и Wear OS command handling.

План:

1. Проверить фактический UUID `CONFIG_CHAR` на Wear OS.
2. Проверить, что desktop пишет именно ожидаемую команду.
3. Проверить command parser на Wear OS.
4. Проверить ответ/notification после `START_CALIBRATION`.
5. Только после этого возвращать calibration UI в рабочий статус.

Не маскировать `Command calibrate_proximity not found` изменением текста ошибки: сначала исправить protocol mismatch.

## RSSI

- RSSI остаётся proximity evidence, а не authentication.
- Baseline RSSI должен храниться в persistent storage.
- Proximity thresholds должны использовать baseline конкретного paired device.
- Не создавать второй независимый BLE adapter для calibration без необходимости.

## Security invariants

- RSSI → proximity evidence.
- ECDSA challenge/response → authentication.
- MAC/address → transport identity only.
- Password → local platform credential store only.
- Unlock → только после cryptographic verification + native platform authentication.

## Что пока НЕ делаем

- Не трогаем player/music control.
- Не собираем и не тестируем Windows Credential Provider DLL без отдельной команды.
- Не считаем успешный BLE reconnect доказательством успешного unlock: unlock должен пройти полный cryptographic challenge/response.
