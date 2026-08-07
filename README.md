┌─────────────────┐      BLE (Custom GATT)      ┌─────────────────┐
│   Wear OS       │  ═══════════════════════════► │   ПК-клиент     │
│   (Kotlin)      │      Challenge-Response      │   (Rust)        │
│                 │      + RSSI + Motion         │                 │
│  • Keystore     │                             │  • btleplug     │
│  • Foreground   │                             │  • Platform     │
│    Service      │                             │    Unlockers    │
└─────────────────┘                             └────────┬────────┘
│
┌────────────────────────┼────────────────────────┐
▼                        ▼                        ▼
┌─────────┐              ┌──────────┐             ┌──────────┐
│ Windows │              │  Linux   │             │  macOS   │
│  Hello  │              │  PAM     │             │  Access  │
│   CDF   │              │  Module  │             │   API    │
└─────────┘              └──────────┘             └──────────┘
plain

---

## Возможности

- 🔐 **Криптографическая верификация** — каждая разблокировка подписывается приватным ключом из Android Keystore.
- 📏 **Адаптивная зона доверия** — калибровка расстояния при первой настройке (RSSI + порог).
- 👆 **Подтверждение жестом** — защита от relay-атак: тап по экрану часов для разблокировки.
- 🔋 **Оптимизация батареи** — умный BLE-цикл, учёт Doze mode на Wear OS.
- 🖥️ **Кроссплатформенность** — Windows, Linux, macOS из одной кодовой базы на Rust.

---

## Установка

### Wear OS (Kotlin)

```bash
git clone https://github.com/yourusername/wristkey.git
cd wristkey/wear-os
./gradlew installDebug
Требования: Wear OS 3.0+, Bluetooth Low Energy.
ПК-клиент (Rust)
bash
git clone https://github.com/yourusername/wristkey.git
cd wristkey/desktop
cargo build --release
Windows
powershell
# Требуется Windows 10 1903+ для BLE
.\target\release\wristkey.exe --install
Linux
bash
# Сборка PAM-модуля
cargo build --release --features pam
sudo cp target/release/libpam_wristkey.so /lib/security/
sudo cp wristkeyd.service /etc/systemd/system/
sudo systemctl enable --now wristkeyd
macOS
bash
# Требуется доступ к Accessibility
cargo build --release --features macos
sudo cp target/release/wristkey /usr/local/bin/
# Добавить в System Preferences → Security → Accessibility
Безопасность
Table
Угроза	Митигация
Relay-атака	Обязательный жест подтверждения на часах + таймаут 30 сек
Спуфинг BLE	LE Secure Connections + ECDSA challenge-response
Потеря часов	Автоблокировка ПК при отсутствии сигнала
Перехват ключа	Приватный ключ никогда не покидает Android Keystore
Roadmap
[x] Прототип BLE-связи
[ ] Windows Hello CDF интеграция
[ ] Linux PAM-модуль
[ ] macOS демон (Accessibility API)
[ ] Калибровка RSSI с машинным обучением
[ ] F-Droid / Google Play релиз
[ ] Поддержка UWB (для будущих Wear OS-устройств)
Контрибьютинг
PR приветствуются! Сначала загляни в CONTRIBUTING.md.
Лицензия
MIT License. См. LICENSE.
