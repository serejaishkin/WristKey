//! Pairing GUI — launched via `wristkeyd.exe --pair` or tray menu

use std::sync::{Arc, mpsc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use eframe::egui;
use tokio::time::timeout;
use uuid::Uuid;
use chrono::Utc;
use wristkey_core::{SessionManager, Storage, PairedDevice, Response};
use wristkey_ble::{BtleplugAdapter, BleAdapter, PeripheralInfo};

enum ScanUpdate {
    Devices(Vec<PeripheralInfo>),
    Error(String),
}

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
    all_devices_count: usize,
    wristkey_count: usize,
    paired_devices: Vec<PairedDevice>,
    scan_rx: Option<mpsc::Receiver<ScanUpdate>>,
    scan_abort: Arc<AtomicBool>,
    pairing_result_rx: Option<mpsc::Receiver<Result<(), String>>>,
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
        let mut app = Self {
            session,
            storage,
            state: AppState::Scanning,
            status: "🔍 Scanning for WristKey devices…".into(),
            discovered: Vec::new(),
            all_devices_count: 0,
            wristkey_count: 0,
            paired_devices,
            scan_rx: None,
            scan_abort: Arc::new(AtomicBool::new(false)),
            pairing_result_rx: None,
        };
        app.start_scan();
        app
    }

    fn start_scan(&mut self) {
        self.stop_scan_internal();
        let (scan_tx, scan_rx) = mpsc::channel::<ScanUpdate>();
        self.scan_rx = Some(scan_rx);
        self.state = AppState::Scanning;
        self.status = "🔍 Scanning for WristKey devices…".into();
        self.all_devices_count = 0;
        self.wristkey_count = 0;

        let abort = self.scan_abort.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            rt.block_on(async {
                let adapter = match BtleplugAdapter::new().await {
                    Ok(a) => Arc::new(a) as Arc<dyn BleAdapter>,
                    Err(e) => {
                        let _ = scan_tx.send(ScanUpdate::Error(format!("BLE adapter error: {}", e)));
                        return;
                    }
                };
                let service_uuid = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567890").unwrap();
                let mut rx_scan = match adapter.scan(service_uuid).await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = scan_tx.send(ScanUpdate::Error(format!("Scan error: {}", e)));
                        return;
                    }
                };
                let mut devices = Vec::new();
                let mut all_count = 0usize;
                let mut wk_count = 0usize;
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                while tokio::time::Instant::now() < deadline && !abort.load(Ordering::Relaxed) {
                    match timeout(Duration::from_secs(1), rx_scan.recv()).await {
                        Ok(Some(info)) => {
                            all_count += 1;
                            let is_wristkey = info.raw_manufacturer_data.is_some();
                            if is_wristkey {
                                wk_count += 1;
                                if !devices.iter().any(|d: &PeripheralInfo| d.id == info.id) {
                                    devices.push(info);
                                    let _ = scan_tx.send(ScanUpdate::Devices(devices.clone()));
                                }
                            }
                            let _ = scan_tx.send(ScanUpdate::Error(format!("__STATS__:{}:{}", all_count, wk_count)));
                        }
                        _ => {}
                    }
                }
                let _ = adapter.stop_scan().await;
            });
        });
    }

    fn stop_scan_internal(&mut self) {
        self.scan_abort.store(true, Ordering::Relaxed);
        self.scan_rx = None;
    }

    fn stop_scan(&mut self) {
        self.stop_scan_internal();
        self.status = "⏹️ Scan stopped".into();
        self.state = AppState::Discovered;
    }

    fn clear_list(&mut self) {
        self.discovered.clear();
        self.state = AppState::Scanning;
        self.status = "🗑️ List cleared".into();
    }

    fn rescan(&mut self) {
        self.discovered.clear();
        self.scan_abort = Arc::new(AtomicBool::new(false));
        self.start_scan();
    }

    fn do_pairing(&mut self, info: PeripheralInfo) {
        self.state = AppState::Pairing;
        self.status = format!("🖐️ Pairing with {}…", info.name.as_deref().unwrap_or("Unknown"));
        let session = self.session.clone();
        let (result_tx, result_rx) = mpsc::channel::<Result<(), String>>();
        self.pairing_result_rx = Some(result_rx);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio");
            let result = rt.block_on(async {
                let adapter = match BtleplugAdapter::new().await {
                    Ok(a) => a,
                    Err(e) => return Err(format!("BLE adapter: {}", e)),
                };
                let conn = match adapter.connect(&info).await {
                    Ok(c) => c,
                    Err(e) => return Err(format!("Connect failed: {}", e)),
                };
                let challenge = match session.begin_pairing().await {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = adapter.disconnect(&conn).await;
                        return Err(format!("Begin pairing failed: {}", e));
                    }
                };
                let challenge_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567891").unwrap();
                let response_char = Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567892").unwrap();

                let mut rx = match adapter.notify(&conn, response_char).await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = adapter.disconnect(&conn).await;
                        return Err(format!("Notify subscribe failed: {}", e));
                    }
                };

                let mut write_ok = false;
                for _attempt in 1..=3 {
                    if adapter.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() {
                        write_ok = true;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                if !write_ok {
                    let _ = adapter.disconnect(&conn).await;
                    return Err("Failed to write challenge after 3 attempts".into());
                }

                let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
                    Ok(Some(d)) => d,
                    Ok(None) => {
                        let _ = adapter.disconnect(&conn).await;
                        return Err("Watch disconnected before responding".into());
                    }
                    Err(_) => {
                        let _ = adapter.disconnect(&conn).await;
                        return Err("Timeout waiting for watch response (10s). Make sure you shook your wrist or pressed the button on the watch.".into());
                    }
                };

                if response_data.len() < 66 {
                    let _ = adapter.disconnect(&conn).await;
                    return Err(format!("Response too short: {} bytes (expected >= 66)", response_data.len()));
                }

                let sig_len = response_data.len() - 66;
                let signature = response_data[..sig_len].to_vec();
                let user_present = response_data[sig_len] != 0;
                let public_key = response_data[sig_len + 1..].to_vec();
                let response = Response { signature, user_present, timestamp: Utc::now() };

                match session.complete_pairing(
                    info.name.as_deref().unwrap_or("WristKey").to_string(),
                    public_key.clone(), info.device_id.clone(), &response, info.rssi.unwrap_or(-50),
                ).await {
                    Ok(_device) => {
                        let _ = adapter.disconnect(&conn).await;
                        Ok(())
                    }
                    Err(e) => {
                        let _ = adapter.disconnect(&conn).await;
                        Err(format!("Pairing verification failed: {}", e))
                    }
                }
            });
            let _ = result_tx.send(result);
        });
    }
}

impl eframe::App for PairingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.scan_rx {
            while let Ok(update) = rx.try_recv() {
                match update {
                    ScanUpdate::Devices(devs) => {
                        self.discovered = devs;
                        if !self.discovered.is_empty() && self.state == AppState::Scanning {
                            self.state = AppState::Discovered;
                        }
                        ctx.request_repaint();
                    }
                    ScanUpdate::Error(msg) => {
                        if msg.starts_with("__STATS__") {
                            let parts: Vec<&str> = msg.split(':').collect();
                            if parts.len() == 3 {
                                if let (Ok(a), Ok(w)) = (parts[1].parse(), parts[2].parse()) {
                                    self.all_devices_count = a;
                                    self.wristkey_count = w;
                                    self.status = format!("🔍 Scanning… Found {} BLE devices total ({} WristKey)", a, w);
                                }
                            }
                        } else {
                            self.state = AppState::Failed(msg.clone());
                            self.status = format!("❌ {}", msg);
                        }
                        ctx.request_repaint();
                    }
                }
            }
        }

        if let Some(rx) = &self.pairing_result_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(()) => {
                        self.state = AppState::Paired;
                        self.status = "✅ Paired successfully! You can close this window and start the daemon.".into();
                        self.paired_devices = {
                            let storage = self.storage.clone();
                            std::thread::spawn(move || {
                                let rt = tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                    .expect("tokio");
                                rt.block_on(async { storage.list_devices().await.unwrap_or_default() })
                            }).join().expect("reload")
                        };
                    }
                    Err(msg) => {
                        self.state = AppState::Failed(msg.clone());
                        self.status = format!("❌ {}", msg);
                    }
                }
                self.pairing_result_rx = None;
                ctx.request_repaint();
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("WristKey Pairing");
            ui.separator();

            ui.collapsing("Already Paired Devices", |ui| {
                if self.paired_devices.is_empty() { ui.label("No paired devices yet."); }
                else { for d in &self.paired_devices { ui.label(format!("📱 {} (ID: {})", d.name, d.id)); } }
            });

            ui.separator();

            ui.horizontal(|ui| {
                if ui.button("⏹️ Stop Scan").clicked() {
                    self.stop_scan();
                }
                if ui.button("🗑️ Clear List").clicked() {
                    self.clear_list();
                }
                if ui.button("🔄 Rescan").clicked() {
                    self.rescan();
                }
            });

            ui.separator();
            ui.label(&self.status);

            if self.state == AppState::Scanning {
                ui.label(format!("Total BLE devices seen: {} | WristKey devices: {}", self.all_devices_count, self.wristkey_count));
            }

            if self.state == AppState::Discovered || self.state == AppState::Scanning {
                ui.label("Discovered WristKey devices:");
                let mut clicked = None;
                for info in &self.discovered {
                    let name = info.name.as_deref().unwrap_or("Unknown");
                    let pin = info.pin.as_deref().unwrap_or("????");
                    if ui.button(format!("🔗 Pair with {} (PIN: {}, {})", name, pin, info.id)).clicked() {
                        clicked = Some(info.clone());
                    }
                }
                if let Some(info) = clicked { self.do_pairing(info); }
            }

            if self.state == AppState::Pairing {
                ui.spinner();
                ui.label("Waiting for watch… Shake your wrist or press the button on the watch when it vibrates.");
            }

            if self.state == AppState::Paired {
                ui.colored_label(egui::Color32::from_rgb(0, 180, 0), "✅ Paired!");
            }

            if let AppState::Failed(ref msg) = self.state {
                ui.colored_label(egui::Color32::from_rgb(255, 80, 80), msg);
            }
        });
    }
}
