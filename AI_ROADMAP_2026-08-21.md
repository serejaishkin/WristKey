# WristKey — AI Roadmap / Handoff checkpoint

**Дата: 2026-08-21**  
**Ветка: `fix/wristkey-20260818`**

## Текущий этап

### Сделано

- Wear OS pairing сохраняется на часах.
- `ProximityRssiTracker` использует подтверждение по последовательным raw RSSI samples.
- Desktop proximity loop использует paired-device identity и cryptographic challenge/response для unlock.
- Tauri desktop GUI остаётся текущим desktop entry point.
- Desktop pairing переведён на persistent `SqliteStorage`; paired devices переживают перезапуск процесса.
- Если persistent SQLite storage не открывается, приложение завершается с ошибкой вместо silent volatile fallback.
- Desktop daemon автоматически запускается при старте Tauri.
- При старте daemon пытается выполнить silent reconnect/authenticate для сохранённого paired device.
- `ConnectionManager` теперь удаляет stale connection после неудачного RSSI probe и выполняет до 6 свежих reconnect attempts с backoff.
- GUI daemon toggle исправлен: использует зарегистрированные `start_daemon` / `stop_daemon`, а не несуществующий `toggle_daemon`.
- GUI pairing исправлен: передаёт BLE `address`, который Rust-команда требует явно.
- Ошибка `Command calibrate_proximity not found` исправлена: GUI теперь вызывает зарегистрированный `calibrate_device`.
- Calibration больше не возвращает фиктивные 10 samples: desktop собирает 20 реальных RSSI samples, требует минимум 10 валидных samples, вычисляет average и сохраняет новый `baseline_rssi` в SQLite.
- Desktop больше не молча переходит на `MockBleAdapter`, если реальный BLE adapter не инициализировался.

### Последние commits

- `83dcb98` — `Use persistent SQLite storage for desktop pairing`
- `c4b5da9` — `Fix Tauri calibration command and BLE pairing address`
- `24b97ee` — `Harden BLE reconnect after desktop restart`
- `896cb56` — `Implement real RSSI calibration and require real BLE adapter`

## Следующая проверка — desktop restart / reconnect

После `git pull` и сборки проверить строго:

1. Запустить desktop WristKey.
2. Выполнить pairing с часами.
3. Убедиться, что paired device отображается.
4. Полностью убить desktop процесс.
5. Запустить desktop снова.
6. Убедиться, что paired device НЕ исчез.
7. Посмотреть log: должен появиться `Daemon started` / `Silent reconnect`.
8. Убедиться, что BLE соединение создаётся без нового pairing.
9. Проверить cryptographic challenge/response.
10. Проверить unlock.

Если reconnect не произойдёт, нужен именно новый runtime log с момента запуска до `Silent reconnect failed`; кодовая цепочка теперь существует и должна быть диагностируема.

## Calibration — теперь реальная

GUI должен:

- вызвать `calibrate_device`;
- подключиться к сохранённым часам через `ConnectionManager`;
- снять 20 RSSI readings примерно за 6 секунд;
- отбросить значения вне `-127..0`;
- потребовать минимум 10 валидных samples;
- записать среднее как новый baseline;
- вернуть average / threshold / sample count.

Никаких `calibrate_proximity` или `START_CALIBRATION` command protocol для этой операции больше не требуется: calibration выполняется desktop-side по RSSI существующего BLE GATT connection.

## RSSI

- RSSI остаётся proximity evidence, а не authentication.
- Baseline RSSI хранится в persistent storage.
- Threshold считается относительно baseline конкретного paired device.
- Не создавать второй независимый BLE adapter для calibration.

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
