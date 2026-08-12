import io

with io.open('wear-os/app/src/main/java/com/wristkey/ble/WristKeyBleService.kt', 'r', encoding='utf-8') as f:
    lines = f.readlines()

out = []
for i, line in enumerate(lines):
    out.append(line)
    
    # 1. Лог перед handleChallenge
    if 'handleChallenge(device, requestId, responseNeeded, value)' in line and 'About to call' not in lines[i-1]:
        out.append('                    Log.i(TAG, "About to call handleChallenge")\n')
    
    # 2. Лог в начало handleChallenge
    if 'private fun handleChallenge(' in line:
        # Ищем следующую строку с "{"
        j = i + 1
        while j < len(lines) and '{' not in lines[j]:
            out.append(lines[j])
            j += 1
        if j < len(lines):
            out.append(lines[j])  # строка с "{"
            out.append('        Log.i(TAG, "handleChallenge START")\n')
        i = j
        continue
    
    # 3. Лог перед vibrate
    if 'vibrator?.vibrate(longArrayOf(0, 300, 200, 300), -1)' in line:
        out.append('        Log.i(TAG, "handleChallenge: about to vibrate")\n')
    
    # 4. Лог в корутину
    if 'serviceScope.launch {' in line:
        out.append('            Log.i(TAG, "handleChallenge: coroutine STARTED")\n')
    
    # 5. Лог в цикл
    if 'while (System.currentTimeMillis() - startTime < 6000) {' in line:
        out.append('                Log.i(TAG, "handleChallenge: checking motion=" + motionDetector.isMoving + " present=" + isUserPresent())\n')
    
    # 6. Лог после confirmed
    if 'confirmed = true' in line and 'Log.i' not in lines[i+1]:
        out.append('                    Log.i(TAG, "handleChallenge: confirmed!")\n')
    
    # 7. Лог перед notify
    if 'responseCharacteristic?.value = response' in line:
        out.append('            Log.i(TAG, "handleChallenge: about to notify")\n')
    
    # 8. Лог после notify
    if 'Log.i(TAG, "Challenge signed and notified.' in line:
        out.insert(-1, '            Log.i(TAG, "handleChallenge: notify returned, notified=" + notified)\n')

with io.open('wear-os/app/src/main/java/com/wristkey/ble/WristKeyBleService.kt', 'w', encoding='utf-8') as f:
    f.writelines(out)

print("OK")