import os

REPO = "/workspaces/WristKey"

def read(path):
    with open(os.path.join(REPO, path), "r") as f:
        return f.read()

def write(path, content):
    with open(os.path.join(REPO, path), "w") as f:
        f.write(content)

print("[1] ble/src/lib.rs ...")
c = read("desktop/crates/ble/src/lib.rs")

old = """                        let pin = props.manufacturer_data.get(&0xFFFF)
                            .and_then(|v| String::from_utf8(v.clone()).ok());
                        let info = PeripheralInfo {
                            pin,
                            id: peripheral.address().to_string(),"""

new = """                        let manufacturer_data = props.manufacturer_data.get(&0xFFFF).cloned().unwrap_or_default();
                        let pin = String::from_utf8(manufacturer_data.clone()).ok();
                        let device_id = if manufacturer_data.len() > 4 {
                            Some(hex::encode(&manufacturer_data[manufacturer_data.len()-4..]))
                        } else { None };
                        let info = PeripheralInfo {
                            pin,
                            device_id,
                            id: peripheral.address().to_string(),"""

if old in c:
    c = c.replace(old, new)
    write("desktop/crates/ble/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

print("[2] daemon perform_unlock ...")
c = read("desktop/crates/daemon/src/lib.rs")
if "self.ble.write(conn, self.challenge_char, &challenge.to_bytes()).await?;" in c:
    c = c.replace("self.ble.write(conn, self.challenge_char, &challenge.to_bytes()).await?;", "self.write_with_retry(conn, self.challenge_char, &challenge.to_bytes()).await?;")
    write("desktop/crates/daemon/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

print("[3] daemon perform_pairing device_id ...")
c = read("desktop/crates/daemon/src/lib.rs")

old = """    let baseline_rssi = self.ble.read_rssi(conn).await?;
        self.session.complete_pairing(
            info.name.unwrap_or_else(|| "Unknown Watch".into()),
            public_key,
            &response,
            baseline_rssi,
        ).await"""

new = """    let baseline_rssi = self.ble.read_rssi(conn).await?;
        let device_id = response_data.get(130..134).map(|b| hex::encode(b));
        self.session.complete_pairing(
            info.name.unwrap_or_else(|| "Unknown Watch".into()),
            public_key,
            device_id,
            &response,
            baseline_rssi,
        ).await"""

if old in c:
    c = c.replace(old, new)
    write("desktop/crates/daemon/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

print("[4] core complete_pairing signature ...")
c = read("desktop/crates/core/src/lib.rs")
if "pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, response: &Response, baseline_rssi: i16)" in c:
    c = c.replace("pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, response: &Response, baseline_rssi: i16) -> Result<PairedDevice> {", "pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, device_id: Option<String>, response: &Response, baseline_rssi: i16) -> Result<PairedDevice> {")
    c = c.replace("let device = PairedDevice { id: Uuid::new_v4(), name: device_name, public_key, device_id: None, paired_at: Utc::now(), baseline_rssi };", "let device = PairedDevice { id: Uuid::new_v4(), name: device_name, public_key, device_id, paired_at: Utc::now(), baseline_rssi };")
    write("desktop/crates/core/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

print("[5] core test ...")
c = read("desktop/crates/core/src/lib.rs")
old_test = """let device = PairedDevice {
 id: Uuid::new_v4(),
 name: "Sled Watch".into(),
 public_key: vec![1, 2, 3],
 paired_at: Utc::now(),
 baseline_rssi: -55,
 };"""
new_test = """let device = PairedDevice {
 id: Uuid::new_v4(),
 name: "Sled Watch".into(),
 public_key: vec![1, 2, 3],
 device_id: None,
 paired_at: Utc::now(),
 baseline_rssi: -55,
 };"""
if old_test in c:
    c = c.replace(old_test, new_test)
    write("desktop/crates/core/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

print("[6] daemon tests ...")
c = read("desktop/crates/daemon/src/lib.rs")
if 'complete_pairing("Mock Watch".into(), pub_key, &response, -50)' in c:
    c = c.replace('complete_pairing("Mock Watch".into(), pub_key, &response, -50)', 'complete_pairing("Mock Watch".into(), pub_key, None, &response, -50)')
    write("desktop/crates/daemon/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

print("\nDone. Now run:")
print("  cd /workspaces/WristKey/desktop")
print("  cargo build --release --target x86_64-pc-windows-gnu --bin wristkeyd")