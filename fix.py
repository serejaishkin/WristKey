import os

REPO = "/workspaces/WristKey"

def rf(path):
    with open(os.path.join(REPO, path), "r") as f:
        return f.read()

def wf(path, content):
    with open(os.path.join(REPO, path), "w") as f:
        f.write(content)

# =====================================================================
# 1. ble/src/lib.rs — scan() must include device_id in PeripheralInfo
# =====================================================================
print("[1] ble/src/lib.rs")
c = rf("desktop/crates/ble/src/lib.rs")

# Find the exact block and replace
old_block = """                        let pin = props.manufacturer_data.get(&0xFFFF)
                            .and_then(|v| String::from_utf8(v.clone()).ok());
                        let info = PeripheralInfo {
                            pin,
                            id: peripheral.address().to_string(),"""

new_block = """                        let manufacturer_data = props.manufacturer_data.get(&0xFFFF).cloned().unwrap_or_default();
                        let pin = String::from_utf8(manufacturer_data.clone()).ok();
                        let device_id = if manufacturer_data.len() > 4 {
                            Some(hex::encode(&manufacturer_data[manufacturer_data.len()-4..]))
                        } else { None };
                        let info = PeripheralInfo {
                            pin,
                            device_id,
                            id: peripheral.address().to_string(),"""

if old_block in c:
    c = c.replace(old_block, new_block)
    wf("desktop/crates/ble/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP (not found)")

# =====================================================================
# 2. daemon/src/lib.rs — perform_unlock uses write_with_retry
# =====================================================================
print("[2] daemon perform_unlock")
c = rf("desktop/crates/daemon/src/lib.rs")
if "self.ble.write(conn, self.challenge_char, &challenge.to_bytes()).await?;" in c:
    c = c.replace(
        "self.ble.write(conn, self.challenge_char, &challenge.to_bytes()).await?;",
        "self.write_with_retry(conn, self.challenge_char, &challenge.to_bytes()).await?;"
    )
    wf("desktop/crates/daemon/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

# =====================================================================
# 3. daemon/src/lib.rs — perform_pairing saves device_id
# =====================================================================
print("[3] daemon perform_pairing device_id")
c = rf("desktop/crates/daemon/src/lib.rs")

old_p = """    let baseline_rssi = self.ble.read_rssi(conn).await?;
        self.session.complete_pairing(
            info.name.unwrap_or_else(|| "Unknown Watch".into()),
            public_key,
            &response,
            baseline_rssi,
        ).await"""

new_p = """    let baseline_rssi = self.ble.read_rssi(conn).await?;
        let device_id = response_data.get(130..134).map(|b| hex::encode(b));
        self.session.complete_pairing(
            info.name.unwrap_or_else(|| "Unknown Watch".into()),
            public_key,
            device_id,
            &response,
            baseline_rssi,
        ).await"""

if old_p in c:
    c = c.replace(old_p, new_p)
    wf("desktop/crates/daemon/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

# =====================================================================
# 4. core/src/lib.rs — complete_pairing signature + PairedDevice
# =====================================================================
print("[4] core complete_pairing")
c = rf("desktop/crates/core/src/lib.rs")

if "pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, response: &Response, baseline_rssi: i16)" in c:
    c = c.replace(
        "pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, response: &Response, baseline_rssi: i16) -> Result<PairedDevice> {",
        "pub async fn complete_pairing(&self, device_name: String, public_key: Vec<u8>, device_id: Option<String>, response: &Response, baseline_rssi: i16) -> Result<PairedDevice> {"
    )
    c = c.replace(
        "let device = PairedDevice { id: Uuid::new_v4(), name: device_name, public_key, device_id: None, paired_at: Utc::now(), baseline_rssi };",
        "let device = PairedDevice { id: Uuid::new_v4(), name: device_name, public_key, device_id, paired_at: Utc::now(), baseline_rssi };"
    )
    wf("desktop/crates/core/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

# =====================================================================
# 5. core/src/lib.rs — test_sled_storage_persistence
# =====================================================================
print("[5] core test")
c = rf("desktop/crates/core/src/lib.rs")

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
    wf("desktop/crates/core/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

# =====================================================================
# 6. daemon/src/lib.rs — tests
# =====================================================================
print("[6] daemon tests")
c = rf("desktop/crates/daemon/src/lib.rs")

if 'complete_pairing("Mock Watch".into(), pub_key, &response, -50)' in c:
    c = c.replace(
        'complete_pairing("Mock Watch".into(), pub_key, &response, -50)',
        'complete_pairing("Mock Watch".into(), pub_key, None, &response, -50)'
    )
    wf("desktop/crates/daemon/src/lib.rs", c)
    print("  OK")
else:
    print("  SKIP")

print("\nDone.")
