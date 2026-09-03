# WristKey — AI Roadmap / Handoff checkpoint

**Дата: 2026-09-03**  
**Ветка: `fix/wristkey-20260818`**

## Текущий этап

### Сделано

- **NullBleAdapter**: Приложение запускается без BLE hardware (fallback вместо crash).
- **Lock screen Tauri command**: `invoke('lock_screen')` → `LockWorkStation()` (FFI).
- **Watch challenge response**: `respondToPairedChallenge()` с motion gate (anti-relay).
- **Paired PCs list**: `SettingsActivity.kt` — `PairedDevicesScreen` showing paired desktop devices.
- **Log rotation**: `RollingWriter` — daily rolling, 20 MB × 5 generations (no size explosion).
- **Graceful daemon shutdown**: `Daemon::run()` accepts `watch::Receiver<()>`; `tokio::select!` loop.
- **BLE address rotation fix (watch)**: `WristKeyBleService.kt` — removed strict address checks from 4 locations; crypto challenge is the real identity check.
- **wristkey_match (PC)**: `conn_mgr.rs` — `resolve_current_peripheral()` matches by WristKey service UUID when address/name/device_id don't match (address rotation).
- **Motion window 30s**: `MotionDetector.hasRecentMotion()` default increased from 10s to 30s for daemon silent reconnect.

### Последние commits

- `0802896` — `fix: wristkey_service matching in conn_mgr + 30s motion window`
- `7ed2726` — `feat: add lock_screen Tauri command`
- `5159fd0` — `fix: relax address checks in WristKeyBleService for BLE rotation`
- `1e64276` — `feat: NullBleAdapter for apps without BLE hardware`
- `9a7414e` — `fix: limit log file size with RollingWriter`
- `66df52f` — `feat: size-capped rolling log writer`
- `14143de` — `feat: challenge response with motion gate + paired PCs list`
- `8aa8c37` — `fix: graceful daemon shutdown`

## Что проверить

1. Desktop restart → silent reconnect (wristkey_service match discovers watch by service UUID, not address)
2. Auth response: watch signs challenge when on wrist (30s motion window)
3. Log rotation: logs stay under 100 MB total
4. NullBleAdapter: app starts on PC without BLE adapter
5. Lock screen: `invoke('lock_screen')` works from frontend

## Что НЕ делаем

- Wi-Fi/UDP transport (BT is the product priority)
- Windows Credential Provider DLL (без отдельной команды)
- Player/music control
