#!/usr/bin/env bash
set -euo pipefail

FILE="wear-os/app/src/main/java/com/wristkey/ble/WristKeyBleService.kt"

if [[ ! -f "$FILE" ]]; then
  echo "ERROR: $FILE not found"
  exit 1
fi

python3 - "$FILE" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")

old = '''    fun forgetDevice() {
        clearPairedDevice()
        stopGattServer()
        resetPin()
        Log.i(TAG, "Device forgotten, restarting advertising")
    }
'''

new = '''    fun forgetDevice() {
        clearPairedDevice()
        stopGattServer()
        startGattServer()
        resetPin()
        Log.i(TAG, "Device forgotten, GATT server restarted")
    }
'''

if old not in text:
    raise SystemExit("ERROR: expected forgetDevice() block not found; file was not modified")

if text.count(old) != 1:
    raise SystemExit("ERROR: expected exactly one forgetDevice() block; file was not modified")

path.write_text(text.replace(old, new), encoding="utf-8")
print(f"Updated: {path}")
PY

echo "Done. Pairing code was not intentionally modified."
