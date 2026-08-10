//! Pairing GUI — launched via `wristkeyd.exe --pair` or tray menu
//!
//! Runs GUI on caller thread, BLE operations in background threads with own tokio runtime.

use std::sync::Arc;
use std::time::Duration;
use eframe::egui;
use tokio::time::timeout;
use uuid::Uuid;
use chrono::Utc;
use wristkey_core::{SessionManager, Storage, PairedDevice, Response};
use wristkey_ble::{BtleplugAdapter, BleAdapter, PeripheralInfo};

pub fn run_pairing_gui(session: Arc<SessionManager>, storage: Arc<dyn Storage>) {
    let paired_devices = {
        let storage = storage.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                storage.list_devices().await.unwrap_or_default()
            })
        }).join().expect("list devices thread")
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 640.0]),
        ..Default::default()
    };

    let app = PairingApp::new(session, storage, paired_devices);

    eframe::run_native(
        "WristKey — Pairing",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    ).expect("eframe");
}

struct PairingApp {
    session: Arc<SessionManager>,
    storage: Arc<dyn Storage>,
    state: AppState,
    status: String,
    discovered: Vec<PeripheralInfo>,
    paired_devices: Vec<PairedDevice>,
}

#[derive(PartialEq)]
enum AppState {
    Scanning,
    Discovered,
    Pairing,
    Paired,
    Failed(String),
}

impl PairingApp {
    fn new(
        session: Arc<SessionManager>,
        storage: Arc<dyn Storage>,
        paired_devices: Vec<PairedDevice>,
    ) -> Self {
        let (scan_tx, scan_rx) = std::sync::mpsc::channel::<Vec<PeripheralInfo>>();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                let adapter = match BtleplugAdapter::new().await {
                    Ok(a) => a,
                    Err(e) => { eprintln!("BLE adapter error: {}", e); return; }
                };

                let service_uuid = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
                let mut rx_scan = match adapter.scan(service_uuid).await {
                    Ok(r) => r,
                    Err(e) => { eprintln!("Scan error: {}", e); return; }
                };

                let mut devices = Vec::new();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

                while tokio::time::Instant::now() < deadline {
                    match timeout(Duration::from_secs(1), rx_scan.recv()).await {
                        Ok(Some(info)) => {
                            if !devices.iter().any(|d: &PeripheralInfo| d.id == info.id) {
                                devices.push(info);
                                let _ = scan_tx.send(devices.clone());
                            }
                        }
                        _ => {}
                    }
                }
            });
        });

        let mut app = Self {
            session,
            storage,
            state: AppState::Scanning,
            status: "🔍 Scanning for WristKey devices…".into(),
            discovered: Vec::new(),
            paired_devices,
        };

        // Drain scan channel in background
        std::thread::spawn(move || {
            while let Ok(devs) = scan_rx.recv() {
                if devs.is_empty() { break; }
            }
        });

        app
    }

    fn do_pairing(&mut self, info: PeripheralInfo) {
        self.state = AppState::Pairing;
        self.status = format!("🖐️ Pairing with {}…\nMove your wrist to confirm", info.name.as_deref().unwrap_or("Unknown"));

        let session = self.session.clone();
        let storage = self.storage.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                let adapter = match BtleplugAdapter::new().await {
                    Ok(a) => a,
                    Err(e) => { eprintln!("BLE error: {}", e); return; }
                };

                let conn = match adapter.connect(&info).await {
                    Ok(c) => c,
                    Err(e) => { eprintln!("Connect error: {}", e); return; }
                };

                let challenge = match session.begin_pairing().await {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("Session error: {}", e);
                        let _ = adapter.disconnect(&conn).await;
                        return;
                    }
                };

                let challenge_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567891").unwrap();
                let response_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567892").unwrap();

                let mut write_ok = false;
                for attempt in 1..=3 {
                    match adapter.write(&conn, challenge_char, &challenge.to_bytes()).await {
                        Ok(_) => { write_ok = true; break; }
                        Err(e) => eprintln!("Write attempt {} failed: {}", attempt, e),
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }

                if !write_ok {
                    eprintln!("Write challenge failed after 3 attempts");
                    let _ = adapter.disconnect(&conn).await;
                    return;
                }

                eprintln!("🖐️ Move your wrist on the watch to confirm pairing");

                let mut rx = match adapter.notify(&conn, response_char).await {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("Notify error: {}", e);
                        let _ = adapter.disconnect(&conn).await;
                        return;
                    }
                };

                let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
                    Ok(Some(d)) => d,
                    _ => {
                        eprintln!("Pairing timeout");
                        let _ = adapter.disconnect(&conn).await;
                        return;
                    }
                };

                if response_data.len() < 66 {
                    eprintln!("Response too short: {}", response_data.len());
                    let _ = adapter.disconnect(&conn).await;
                    return;
                }

                let pubkey_len = 65;
                let sig_len = response_data.len() - pubkey_len - 1;
                let signature = response_data[..sig_len].to_vec();
                let user_present = response_data[sig_len] != 0;
                let public_key = response_data[sig_len + 1..].to_vec();

                let response = Response {
                    signature,
                    user_present,
                    timestamp: Utc::now(),
                };

                match session.complete_pairing(
                    info.name.as_deref().unwrap_or("WristKey").to_string(),
                    public_key,
                    info.device_id,
                    &response,
                    info.rssi.unwrap_or(-50),
                ).await {
                    Ok(device) => {
                        let _ = storage.save_device(&device).await;
                        eprintln!("✅ Paired: {} (ID: {})", device.name, device.id);
                    }
                    Err(e) => eprintln!("❌ Verify error: {}", e),
                }

                let _ = adapter.disconnect(&conn).await;
            });
        });
    }
}

impl eframe::App for PairingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("WristKey Pairing");
            ui.separator();

            ui.collapsing("Already Paired Devices", |ui| {
                if self.paired_devices.is_empty() {
                    ui.label("No paired devices yet.");
                } else {
                    for d in &self.paired_devices {
                        ui.label(format!("📱 {} (ID: {}, RSSI: {} dBm)", d.name, d.id, d.baseline_rssi));
                    }
                }
            });

            ui.separator();
            ui.label(&self.status);

            if !self.discovered.is_empty() && self.state == AppState::Scanning {
                self.state = AppState::Discovered;
                self.status = format!("Found {} device(s). Select one to pair.", self.discovered.len());
            }

            if self.state == AppState::Discovered || self.state == AppState::Scanning {
                ui.label("Discovered devices:");
                let mut clicked = None;
                for info in &self.discovered {
                    let name = info.name.as_deref().unwrap_or("Unknown");
                    if ui.button(format!("🔗 Pair with {} ({})", name, info.id)).clicked() {
                        clicked = Some(info.clone());
                    }
                }
                if let Some(info) = clicked {
                    self.do_pairing(info);
                }
            }

            if self.state == AppState::Pairing {
                ui.spinner();
                ui.label("Waiting for watch confirmation…");
            }

            if let AppState::Failed(ref msg) = self.state {
                ui.colored_label(egui::Color32::RED, format!("❌ {}", msg));
            }

            if self.state == AppState::Paired {
                ui.colored_label(egui::Color32::GREEN, "✅ Successfully paired!");
            }

            ui.separator();
            if ui.button("🔄 Rescan").clicked() {
                self.discovered.clear();
                self.state = AppState::Scanning;
                self.status = "🔍 Scanning…".into();
            }
        });
    }
}
