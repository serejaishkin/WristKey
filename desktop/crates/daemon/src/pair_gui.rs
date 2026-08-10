//! Standalone pairing GUI — scan for watches and pair with ECDSA verify

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::runtime::Runtime;
use wristkey_ble::{BleAdapter, BtleplugAdapter, PeripheralInfo};
use wristkey_core::CryptoEngine;
use uuid::Uuid;

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
const PUBKEY_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567894";

fn main() {
    let rt = Runtime::new().expect("tokio runtime");
    tracing_subscriber::fmt::init();

    let app = PairApp {
        devices: Arc::new(Mutex::new(Vec::new())),
        scanning: Arc::new(Mutex::new(false)),
        scan_active: Arc::new(AtomicBool::new(false)),
        status: Arc::new(Mutex::new("Click Scan to find watches".to_string())),
        selected: Arc::new(Mutex::new(None)),
        pairing: Arc::new(Mutex::new(false)),
        rt,
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([480.0, 600.0])
            .with_min_inner_size([320.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "WristKey — Pair Device",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    ).unwrap();
}

struct PairApp {
    devices: Arc<Mutex<Vec<PeripheralInfo>>>,
    scanning: Arc<Mutex<bool>>,
    scan_active: Arc<AtomicBool>,
    status: Arc<Mutex<String>>,
    selected: Arc<Mutex<Option<usize>>>,
    pairing: Arc<Mutex<bool>>,
    rt: Runtime,
}

impl eframe::App for PairApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🔷 WristKey Pairing");
            ui.separator();

            let scanning = *self.scanning.lock().unwrap();
            let pairing = *self.pairing.lock().unwrap();
            let devices = self.devices.lock().unwrap().clone();
            let status = self.status.lock().unwrap().clone();

            ui.horizontal(|ui| {
                if ui.add_sized([100.0, 32.0], eframe::egui::Button::new("🔍 Scan")).clicked() && !scanning && !pairing {
                    self.start_scan(ctx.clone());
                }

                if scanning {
                    if ui.add_sized([100.0, 32.0], eframe::egui::Button::new("⏹ Stop")).clicked() {
                        self.scan_active.store(false, Ordering::Relaxed);
                        *self.scanning.lock().unwrap() = false;
                        *self.status.lock().unwrap() = "Scan stopped".to_string();
                    }
                    ui.spinner();
                    ui.label("Scanning…");
                }

                if !scanning && !devices.is_empty() {
                    if ui.add_sized([100.0, 32.0], eframe::egui::Button::new("🗑 Clear")).clicked() {
                        self.devices.lock().unwrap().clear();
                        *self.selected.lock().unwrap() = None;
                        *self.status.lock().unwrap() = "List cleared".to_string();
                    }
                }
            });

            ui.separator();
            ui.label(&status);

            ui.separator();
            ui.heading("Found devices");

            eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                if devices.is_empty() && !scanning {
                    ui.label("No devices found. Click Scan.");
                } else {
                    for (idx, dev) in devices.iter().enumerate() {
                        let is_selected = self.selected.lock().unwrap().map(|s| s == idx).unwrap_or(false);

                        let response = ui.selectable_label(is_selected, format!(
                            "📱 {} | {} | {} dBm",
                            dev.pin.as_deref().or(dev.name.as_deref()).unwrap_or("Unknown"),
                            dev.id,
                            dev.rssi.map(|r| r.to_string()).unwrap_or_else(|| "N/A".into())
                        ));

                        if response.clicked() {
                            *self.selected.lock().unwrap() = Some(idx);
                        }

                        if is_selected && !pairing {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if ui.button("🔗 Pair with this device").clicked() {
                                    self.start_pairing(idx, ctx.clone());
                                }
                            });
                        }
                    }
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(eframe::egui::Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        std::process::exit(0);
                    }
                });
            });
        });
    }
}

impl PairApp {
    fn start_scan(&self, ctx: eframe::egui::Context) {
        *self.scanning.lock().unwrap() = true;
        self.scan_active.store(true, Ordering::Relaxed);
        *self.status.lock().unwrap() = "Scanning for 30 seconds…".to_string();
        self.devices.lock().unwrap().clear();
        *self.selected.lock().unwrap() = None;

        let devices = self.devices.clone();
        let scanning = self.scanning.clone();
        let scan_active = self.scan_active.clone();
        let status = self.status.clone();
        let ctx = ctx.clone();

        self.rt.spawn(async move {
            match BtleplugAdapter::new().await {
                Ok(adapter) => {
                    let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
                    match adapter.scan(service_uuid).await {
                        Ok(mut rx) => {
                            let mut found = 0;
                            while let Some(info) = rx.recv().await {
                                if !scan_active.load(Ordering::Relaxed) {
                                    break;
                                }
                                found += 1;
                                devices.lock().unwrap().push(info);
                                *status.lock().unwrap() = format!("Found {} device(s)", found);
                                ctx.request_repaint();
                            }
                        }
                        Err(e) => *status.lock().unwrap() = format!("Scan error: {}", e),
                    }
                }
                Err(e) => *status.lock().unwrap() = format!("BLE adapter error: {}", e),
            }
            *scanning.lock().unwrap() = false;
            ctx.request_repaint();
        });
    }

    fn start_pairing(&self, idx: usize, ctx: eframe::egui::Context) {
        *self.pairing.lock().unwrap() = true;
        *self.status.lock().unwrap() = "Pairing…".to_string();

        let devices = self.devices.clone();
        let status = self.status.clone();
        let pairing = self.pairing.clone();
        let ctx = ctx.clone();

        self.rt.spawn(async move {
            let dev = {
                let d = devices.lock().unwrap();
                d.get(idx).cloned()
            };

            if let Some(dev) = dev {
                match do_pairing(dev).await {
                    Ok(_) => *status.lock().unwrap() = "✅ Paired successfully!".to_string(),
                    Err(e) => *status.lock().unwrap() = format!("❌ Pairing failed: {}", e),
                }
            } else {
                *status.lock().unwrap() = "Device not found".to_string();
            }
            *pairing.lock().unwrap() = false;
            ctx.request_repaint();
        });
    }
}

async fn do_pairing(dev: PeripheralInfo) -> Result<(), Box<dyn std::error::Error>> {
    let adapter = BtleplugAdapter::new().await.map_err(|e| format!("adapter: {}", e))?;

    let conn = adapter.connect(&dev).await.map_err(|e| format!("connect: {}", e))?;

    // Windows BLE needs time after connect+discover before subscribing
    tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;

    let response_uuid = Uuid::parse_str(RESPONSE_CHAR).unwrap();
    let mut rx = adapter.notify(&conn, response_uuid).await
        .map_err(|e| format!("notify: {}", e))?;

    let mut challenge = vec![0u8; 24];
    let nonce = uuid::Uuid::new_v4().as_bytes()[..16].to_vec();
    challenge[..16].copy_from_slice(&nonce);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    challenge[16..24].copy_from_slice(&timestamp.to_le_bytes());

    let challenge_uuid = Uuid::parse_str(CHALLENGE_CHAR).unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let mut write_ok = false;
    for attempt in 1..=3 {
        match adapter.write(&conn, challenge_uuid, &challenge).await {
            Ok(()) => { write_ok = true; break; }
            Err(e) if attempt < 3 => {
                eprintln!("write attempt {} failed: {}, retrying...", attempt, e);
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
            Err(e) => return Err(format!("write challenge: {}", e).into()),
        }
    }
    if !write_ok {
        return Err("write challenge failed after 3 attempts".into());
    }

    let response = tokio::time::timeout(
        tokio::time::Duration::from_secs(10),
        rx.recv()
    ).await
    .map_err(|_| "timeout waiting for response")?
    .ok_or("no response received")?;

    if response.len() < 66 {
        return Err("response too short".into());
    }

    let signature = &response[0..64];
    let user_present = response[64] == 1;
    let public_key = response[65..].to_vec();

    if !user_present {
        return Err("User not present on watch".into());
    }

    if public_key.is_empty() {
        return Err("No public key in response".into());
    }

    let mut payload = challenge.clone();
    payload.push(1);

    if let Err(e) = wristkey_core::EcdsaP256Crypto.verify(&public_key, &payload, signature).await {
        return Err(format!("Signature verification failed: {}", e).into());
    }
    println!("✅ Signature verified!");

    adapter.disconnect(&conn).await.map_err(|e| format!("disconnect: {}", e))?;
    Ok(())
}
