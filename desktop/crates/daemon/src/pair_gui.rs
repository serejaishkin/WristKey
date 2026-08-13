//! Pairing GUI — launched via `wristkeyd.exe --pair` or tray menu

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
    scan_thread: Option<std::thread::JoinHandle<()>>,
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

        let scan_thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                let adapter = match BtleplugAdapter::new().await {
                    Ok(a) => a,
                    Err(e) => { eprintln!("BLE adapter error: {}", e); let _ = scan_tx.send(vec![]); return; }
                };
                let service_uuid = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
                let mut rx_scan = match adapter.scan(service_uuid).await {
                    Ok(r) => r,
                    Err(e) => { eprintln!("Scan error: {}", e); let _ = scan_tx.send(vec![]); return; }
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
            session, storage, state: AppState::Scanning,
            status: "🔍 Scanning for WristKey devices…".into(),
            discovered: Vec::new(), paired_devices,
            scan_thread: Some(scan_thread),
        };
        std::thread::spawn(move || {
            while let Ok(devs) = scan_rx.recv() { if devs.is_empty() { break; } }
        });
        app
    }

    fn stop_scan(&mut self) {
        if let Some(handle) = self.scan_thread.take() {
            // Note: we can't truly abort the thread, but dropping the receiver
            // and ignoring results effectively stops the UI updates.
            self.status = "⏹️ Scan stopped".into();
            self.state = AppState::Discovered;
        }
    }

    fn clear_list(&mut self) {
        self.discovered.clear();
        self.state = AppState::Scanning;
        self.status = "🗑️ List cleared".into();
    }

    fn do_pairing(&mut self, info: PeripheralInfo) {
        self.state = AppState::Pairing;
        self.status = format!("🖐️ Pairing with {}…", info.name.as_deref().unwrap_or("Unknown"));
        let session = self.session.clone();
        let storage = self.storage.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio");
            rt.block_on(async {
                let adapter = match BtleplugAdapter::new().await { Ok(a) => a, Err(_) => return };
                let conn = match adapter.connect(&info).await { Ok(c) => c, Err(_) => return };
                let challenge = match session.begin_pairing().await { Ok(c) => c, Err(_) => { let _ = adapter.disconnect(&conn).await; return; } };
                let challenge_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567891").unwrap();
                let response_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567892").unwrap();
                let mut write_ok = false;
                for _attempt in 1..=3 {
                    if adapter.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() { write_ok = true; break; }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                if !write_ok { let _ = adapter.disconnect(&conn).await; return; }
                let mut rx = match adapter.notify(&conn, response_char).await { Ok(r) => r, Err(_) => { let _ = adapter.disconnect(&conn).await; return; } };
                let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
                    Ok(Some(d)) => d, _ => { let _ = adapter.disconnect(&conn).await; return; }
                };
                if response_data.len() < 66 { let _ = adapter.disconnect(&conn).await; return; }
                let sig_len = response_data.len() - 66;
                let signature = response_data[..sig_len].to_vec();
                let user_present = response_data[sig_len] != 0;
                let public_key = response_data[sig_len + 1..].to_vec();
                let response = Response { signature, user_present, timestamp: Utc::now() };
                let _ = session.complete_pairing(
                    info.name.as_deref().unwrap_or("WristKey").to_string(),
                    public_key.clone(), info.device_id.clone(), &response, info.rssi.unwrap_or(-50),
                    info.id.clone(),
                ).await;
                let _ = storage.save_device(&PairedDevice {
                    id: uuid::Uuid::new_v4(),
                    name: info.name.clone().unwrap_or_else(|| "WristKey".to_string()),
                    public_key,
                    device_id: info.device_id.clone(),
                    paired_at: Utc::now(),
                    baseline_rssi: info.rssi.unwrap_or(-50),
                    address: info.id.clone(),
                    windows_password: None,
                }).await;
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
                if self.paired_devices.is_empty() { ui.label("No paired devices yet."); }
                else { for d in &self.paired_devices { ui.label(format!("📱 {} (ID: {})", d.name, d.id)); } }
            });

            ui.separator();

            // Control buttons row
            ui.horizontal(|ui| {
                if ui.button("⏹️ Stop Scan").clicked() {
                    self.stop_scan();
                }
                if ui.button("🗑️ Clear List").clicked() {
                    self.clear_list();
                }
                if ui.button("🔄 Rescan").clicked() {
                    self.discovered.clear();
                    self.state = AppState::Scanning;
                    self.status = "🔍 Scanning…".into();
                }
            });

            ui.separator();
            ui.label(&self.status);

            if !self.discovered.is_empty() && self.state == AppState::Scanning {
                self.state = AppState::Discovered;
                self.status = format!("Found {} device(s).", self.discovered.len());
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
                if let Some(info) = clicked { self.do_pairing(info); }
            }

            if self.state == AppState::Pairing {
                ui.spinner();
                ui.label("Waiting for watch…");
            }
        });
    }
}
