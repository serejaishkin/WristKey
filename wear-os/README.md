# WristKey Wear OS

Android companion app (Kotlin) for Wear OS 3+.

## Structure

| Package | Purpose |
|---------|---------|
| `app` | MainActivity, Application, Foreground Service lifecycle |
| `ble` | BLE Peripheral (GATT server), advertising, challenge-response |
| `security` | Android Keystore ECDSA P-256 key management |
| `sensors` | Accelerometer motion detection (anti-relay) |

## Protocol

Implements BLE GATT Peripheral with service UUID `a1b2c3d4-e5f6-7890-abcd-ef1234567890`.

### Characteristics
- `CHALLENGE` (write) — receives 16-byte nonce from PC
- `RESPONSE` (notify) — sends ECDSA signature + user_present flag

### Security
- Private key never leaves Android Keystore
- Signing only if `MotionDetector.isMoving == true` (watch on wrist)
- User must tap screen to set `user_present = true`

## Build

Requires Android Studio Hedgehog+ and Wear OS emulator or physical watch.

```bash
./gradlew assembleDebug
adb install app/build/outputs/apk/debug/app-debug.apk
