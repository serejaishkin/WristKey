import re

with open('desktop/crates/daemon/src/gui.rs', 'r') as f:
    gui = f.read()

# 1. Импорты
gui = gui.replace(
    'use std::sync::{Arc, Mutex, mpsc};\nuse std::time::Duration;',
    'use std::sync::{Arc, Mutex, mpsc};\nuse std::fs::OpenOptions;\nuse std::io::Write;\nuse std::time::Duration;'
)

# 2. Функция gui_log
gui = gui.replace(
    'const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";\nconst CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";\nconst RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";',
    '''const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";\nconst CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";\nconst RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";\n\nfn gui_log(msg: &str) {\n    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");\n    let line = format!("[{}] {}\\n", ts, msg);\n    let _ = OpenOptions::new().create(true).append(true).open("wristkey_gui.log")\n        .and_then(|mut f| f.write_all(line.as_bytes()));\n}'''
)

# 3. Заменяем eprintln! на gui_log
gui = gui.replace('eprintln!("BLE adapter: {}", e)', 'gui_log(&format!("BLE adapter: {}", e))')
gui = gui.replace('eprintln!("Connect failed: {}", e)', 'gui_log(&format!("Connect failed: {}", e))')
gui = gui.replace('eprintln!("Begin pairing failed: {}", e)', 'gui_log(&format!("Begin pairing failed: {}", e))')
gui = gui.replace('eprintln!("Notify subscribe failed: {}", e)', 'gui_log(&format!("Notify subscribe failed: {}", e))')
gui = gui.replace('eprintln!("Write attempt {} failed: {}", attempt, e)', 'gui_log(&format!("Write attempt {} failed: {}", attempt, e))')
gui = gui.replace('eprintln!("BLE adapter error: {}", e)', 'gui_log(&format!("BLE adapter error: {}", e))')
gui = gui.replace('eprintln!("Scan error: {}", e)', 'gui_log(&format!("Scan error: {}", e))')

# 4. Логи в do_pairing
gui = gui.replace(
    '    fn do_pairing(&mut self, info: PeripheralInfo) {\n        self.scan_state = ScanState::Pairing;\n        self.pairing_status = format!(\n            "🖐️ Pairing with {}…\\nPress the button on the watch to confirm",\n            info.name.as_deref().unwrap_or("Unknown")\n        );',
    '    fn do_pairing(&mut self, info: PeripheralInfo) {\n        self.scan_state = ScanState::Pairing;\n        let dev_name = info.name.as_deref().unwrap_or("Unknown");\n        gui_log(&format!("=== do_pairing started for {} ===", dev_name));\n        self.pairing_status = format!(\n            "🖐️ Pairing with {}…\\nPress the button on the watch to confirm",\n            dev_name\n        );'
)

gui = gui.replace(
    '                let conn = match adapter.connect(&info).await {\n                    Ok(c) => c,',
    '                let conn = match adapter.connect(&info).await {\n                    Ok(c) => { gui_log("Connected to device"); c }'
)

gui = gui.replace(
    '                let mut rx = match adapter.notify(&conn, response_char).await {\n                    Ok(r) => r,',
    '                let mut rx = match adapter.notify(&conn, response_char).await {\n                    Ok(r) => { gui_log("Subscribed to notify (response char)"); r }'
)

gui = gui.replace(
    '                    match adapter.write(&conn, challenge_char, &challenge.to_bytes()).await {\n                        Ok(_) => { write_ok = true; break; }',
    '                    match adapter.write(&conn, challenge_char, &challenge.to_bytes()).await {\n                        Ok(_) => { gui_log(&format!("Challenge written ({} bytes)", challenge.to_bytes().len())); write_ok = true; break; }'
)

gui = gui.replace(
    '                let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {\n                    Ok(Some(d)) => d,',
    '                gui_log("Waiting for response (10s)...");\n                let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {\n                    Ok(Some(d)) => { gui_log(&format!("Response received: {} bytes", d.len())); d }'
)

gui = gui.replace(
    '                    Err(_) => {\n                        let _ = adapter.disconnect(&conn).await;\n                        return Err("Timeout waiting for watch response (10s). Make sure you shook your wrist or pressed the button on the watch.".into());\n                    }',
    '                    Err(_) => {\n                        gui_log("TIMEOUT: no response from watch in 10s");\n                        let _ = adapter.disconnect(&conn).await;\n                        return Err("Timeout waiting for watch response (10s). Make sure you shook your wrist or pressed the button on the watch.".into());\n                    }'
)

gui = gui.replace(
    '                match session.complete_pairing(',
    '                gui_log("Calling complete_pairing...");\n                match session.complete_pairing('
)

gui = gui.replace(
    '                    Ok(_device) => {\n                        let _ = adapter.disconnect(&conn).await;\n                        Ok(())\n                    }',
    '                    Ok(_device) => {\n                        gui_log("complete_pairing OK");\n                        let _ = adapter.disconnect(&conn).await;\n                        Ok(())\n                    }'
)

gui = gui.replace(
    '                    Err(e) => {\n                        let _ = adapter.disconnect(&conn).await;\n                        Err(format!("Pairing verification failed: {}", e))\n                    }',
    '                    Err(e) => {\n                        gui_log(&format!("complete_pairing FAILED: {}", e));\n                        let _ = adapter.disconnect(&conn).await;\n                        Err(format!("Pairing verification failed: {}", e))\n                    }'
)

# 5. Лог в start_scan
gui = gui.replace(
    '    fn start_scan(&mut self) {\n        self.discovered.clear();',
    '    fn start_scan(&mut self) {\n        gui_log("=== start_scan ===");\n        self.discovered.clear();'
)

with open('desktop/crates/daemon/src/gui.rs', 'w') as f:
    f.write(gui)

print("✅ gui.rs patched")
