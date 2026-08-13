//! Unified WristKey GUI — launched via `wristkeyd.exe --gui` (or `--pair`, kept
//! as an alias for backward compatibility). Three tabs:
//! 1. Status — live connection/session state
//! 2. Devices — paired devices (with Forget) + "scan for new" sub-section
//! 3. Settings — sync / unlock configuration
//!
//! Runs on the caller's thread (must be the true process main thread — this is
//! a hard winit requirement, see main.rs). BLE and storage operations run in
//! background threads with their own tokio runtime, communicating back to the
//! GUI via channels / shared state, exactly like the original pairing-only GUI
//! this file replaces.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use std::fs::OpenOptions;
use std::io::Write;
use eframe::egui;
use tokio::time::timeout;
use uuid::Uuid;
use chrono::Utc;
use wristkey_core::{SessionManager, SessionState, Storage, Config, PairedDevice, Response};
use wristkey_ble::{BtleplugAdapter, BleAdapter, PeripheralInfo};

// WristKey custom UUIDs (universal, any Wear OS watch)
const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
const CONFIG_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567894";

fn gui_log(msg: &str) {
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let line = format!("[{}] {}\n", ts, msg);
    let _ = OpenOptions::new().create(true).append(true).open("wristkey_gui.log")
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Fallback display name: use advertised name, or short ID, or "Unknown"
fn display_name(info: &PeripheralInfo) -> String {
    info.name.clone()
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            let id = &info.id;
            if id.len() > 4 {
                format!("Watch {}", &id[id.len()-4..])
            } else {
                "Unknown Watch".to_string()
            }
        })
}

pub fn run_app(session: Arc<SessionManager>, storage: Arc<dyn Storage>) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([560.0, 680.0]),
        ..Default::default()
    };
    let app = WristKeyApp::new(session, storage);
    eframe::run_native("WristKey", options, Box::new(|_cc| Ok(Box::new(app)))).expect("eframe");
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Status,
    Devices,
    Settings,
}

#[derive(Clone)]
struct StatusSnapshot {
    state_label: String,
    detail: String,
}

pub struct WristKeyApp {
    session: Arc<SessionManager>,
    storage: Arc<dyn Storage>,
    tab: Tab,
    status: Arc<Mutex<StatusSnapshot>>,
    paired_devices: Vec<PairedDevice>,
    devices_dirty: bool,
    pending_forget: Option<uuid::Uuid>,
    scan_state: ScanState,
    discovered: Vec<PeripheralInfo>,
    scan_rx: Option<mpsc::Receiver<Vec<PeripheralInfo>>>,
    pairing_status: String,
    pairing_result_rx: Option<mpsc::Receiver<Result<(), String>>>,
    settings_form: SettingsForm,
    settings_loaded: bool,
    settings_status: String,
}

#[derive(PartialEq)]
enum ScanState {
    Idle,
    Scanning,
    Pairing,
    Failed(String),
    Paired,
}

struct SettingsForm {
    auto_lock_timeout_sec: String,
    rssi_threshold_offset_dbm: String,
    challenge_timeout_sec: String,
}

impl Default for SettingsForm {
    fn default() -> Self {
        let d = Config::default();
        Self {
            auto_lock_timeout_sec: d.auto_lock_timeout_sec.to_string(),
            rssi_threshold_offset_dbm: d.rssi_threshold_offset_dbm.to_string(),
            challenge_timeout_sec: d.challenge_timeout_sec.to_string(),
        }
    }
}

impl WristKeyApp {
    fn new(session: Arc<SessionManager>, storage: Arc<dyn Storage>) -> Self {
        let status = Arc::new(Mutex::new(StatusSnapshot {
            state_label: "Loading…".into(),
            detail: String::new(),
        }));

        {
            let session = session.clone();
            let status = status.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                rt.block_on(async {
                    loop {
                        let state = session.state().await;
                        let (label, detail) = describe_state(&state);
                        if let Ok(mut s) = status.lock() {
                            s.state_label = label;
                            s.detail = detail;
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                });
            });
        }

        let mut app = Self {
            session,
            storage,
            tab: Tab::Status,
            status,
            paired_devices: Vec::new(),
            devices_dirty: true,
            pending_forget: None,
            scan_state: ScanState::Idle,
            discovered: Vec::new(),
            scan_rx: None,
            pairing_status: String::new(),
            pairing_result_rx: None,
            settings_form: SettingsForm::default(),
            settings_loaded: false,
            settings_status: String::new(),
        };
        app.reload_devices();
        app.load_settings();
        app
    }

    fn reload_devices(&mut self) {
        let storage = self.storage.clone();
        self.paired_devices = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(async { storage.list_devices().await.unwrap_or_default() })
        }).join().unwrap_or_default();
        self.devices_dirty = false;
    }

    fn load_settings(&mut self) {
        let storage = self.storage.clone();
        let config = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(async { storage.load_config().await.unwrap_or_default() })
        }).join().unwrap_or_default();
        self.settings_form = SettingsForm {
            auto_lock_timeout_sec: config.auto_lock_timeout_sec.to_string(),
            rssi_threshold_offset_dbm: config.rssi_threshold_offset_dbm.to_string(),
            challenge_timeout_sec: config.challenge_timeout_sec.to_string(),
        };
        self.settings_loaded = true;
    }

    fn save_settings(&mut self) {
        let auto_lock = self.settings_form.auto_lock_timeout_sec.trim().parse::<u64>();
        let rssi = self.settings_form.rssi_threshold_offset_dbm.trim().parse::<i16>();
        let challenge = self.settings_form.challenge_timeout_sec.trim().parse::<u64>();

        match (auto_lock, rssi, challenge) {
            (Ok(auto_lock_timeout_sec), Ok(rssi_threshold_offset_dbm), Ok(challenge_timeout_sec)) => {
                let config = Config { auto_lock_timeout_sec, rssi_threshold_offset_dbm, challenge_timeout_sec };
                let storage = self.storage.clone();
                let result = std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
                    rt.block_on(async { storage.save_config(&config).await })
                }).join().unwrap_or_else(|_| Err(wristkey_core::WristKeyError::Storage("save thread panicked".into())));

                match result {
                    Ok(()) => self.settings_status = "✅ Saved. Takes effect on next unlock/pairing check.".into(),
                    Err(e) => self.settings_status = format!("❌ Failed to save: {}", e),
                }
            }
            _ => {
                self.settings_status = "❌ All fields must be whole numbers.".into();
            }
        }
    }

    fn start_scan(&mut self) {
        gui_log("=== start_scan ===");
        self.discovered.clear();
        self.scan_state = ScanState::Scanning;
        self.pairing_status.clear();
        self.pairing_result_rx = None;

        let (tx, rx) = mpsc::channel::<Vec<PeripheralInfo>>();
        self.scan_rx = Some(rx);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(async {
                let adapter = match BtleplugAdapter::new().await {
                    Ok(a) => a,
                    Err(e) => { gui_log(&format!("BLE adapter error: {}", e)); return; }
                };
                let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
                let mut rx_scan = match adapter.scan(service_uuid).await {
                    Ok(r) => r,
                    Err(e) => { gui_log(&format!("Scan error: {}", e)); return; }
                };
                let mut devices = Vec::new();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                while tokio::time::Instant::now() < deadline {
                    match timeout(Duration::from_secs(1), rx_scan.recv()).await {
                        Ok(Some(info)) => {
                            if !devices.iter().any(|d: &PeripheralInfo| d.id == info.id) {
                                let name = display_name(&info);
                                gui_log(&format!("Discovered device: {} (id={}, rssi={:?})", name, info.id, info.rssi));
                                devices.push(info);
                                let _ = tx.send(devices.clone());
                            }
                        }
                        _ => {}
                    }
                }
            });
        });
    }

    fn do_pairing(&mut self, info: PeripheralInfo) {
        let name = display_name(&info);
        self.scan_state = ScanState::Pairing;
        self.pairing_status = format!(
            "🖐️ Pairing with {}…\nPress the button on the watch to confirm",
            name
        );

        let session = self.session.clone();
        let (result_tx, result_rx) = mpsc::channel::<Result<(), String>>();
        self.pairing_result_rx = Some(result_rx);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            let result = rt.block_on(async {
                let adapter = match BtleplugAdapter::new().await {
                    Ok(a) => a,
                    Err(e) => return Err(format!("BLE adapter: {}", e)),
                };
                let conn = match adapter.connect(&info).await {
                    Ok(c) => {
                        gui_log("Connected to device");
                        c
                    }
                    Err(e) => return Err(format!("Connect failed: {}", e)),
                };

                // Check if WristKey custom service exists on the watch
                let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
                let services = adapter.btleplug_adapter()
                    .and_then(|a| {
                        // We can't easily list services from adapter, but connect() already logged them
                        // Fallback: try to read from challenge char — if it fails, service is missing
                        Some(())
                    });
                gui_log(&format!("Service check placeholder: {:?}", services));

                let challenge = match session.begin_pairing().await {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = adapter.disconnect(&conn).await;
                        return Err(format!("Begin pairing failed: {}", e));
                    }
                };
                let challenge_char = Uuid::parse_str(CHALLENGE_CHAR).unwrap();
                let response_char = Uuid::parse_str(RESPONSE_CHAR).unwrap();

                let mut rx = match adapter.notify(&conn, response_char).await {
                    Ok(r) => {
                        gui_log("Subscribed to response characteristic (notify)");
                        r
                    }
                    Err(e) => {
                        let _ = adapter.disconnect(&conn).await;
                        return Err(format!("Notify subscribe failed: {}. Make sure the WristKey app is running on the watch and the watch supports custom BLE GATT services.", e));
                    }
                };

                let mut write_ok = false;
                for attempt in 1..=3 {
                    match adapter.write(&conn, challenge_char, &challenge.to_bytes()).await {
                        Ok(_) => {
                            gui_log(&format!("Challenge written ({} bytes), attempt {}", challenge.to_bytes().len(), attempt));
                            write_ok = true; break;
                        }
                        Err(e) => gui_log(&format!("Write attempt {} failed: {}", attempt, e)),
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                if !write_ok {
                    let _ = adapter.disconnect(&conn).await;
                    return Err("Failed to write challenge after 3 attempts. The WristKey service may not be running on the watch.".into());
                }

                gui_log("Waiting for response (10s timeout)...");
                let response_data = match timeout(Duration::from_secs(10), rx.recv()).await {
                    Ok(Some(d)) => {
                        gui_log(&format!("Response received: {} bytes", d.len()));
                        d
                    }
                    Ok(None) => {
                        gui_log("Watch disconnected before responding");
                        let _ = adapter.disconnect(&conn).await;
                        return Err("Watch disconnected before responding".into());
                    }
                    Err(_) => {
                        gui_log("TIMEOUT: no response from watch in 10s");
                        let _ = adapter.disconnect(&conn).await;
                        return Err("Timeout waiting for watch response (10s). Make sure you shook your wrist or pressed the button on the watch.".into());
                    }
                };

                // Expected format: [signature 64][user_present 1][public_key 65] = 130 bytes
                if response_data.len() != 130 {
                    let _ = adapter.disconnect(&conn).await;
                    return Err(format!(
                        "Invalid response length: {} bytes (expected exactly 130: 64 sig + 1 user_present + 65 pubkey).                          Make sure the WristKey app on the watch is up to date.",
                        response_data.len()
                    ));
                }

                let signature = response_data[..64].to_vec();
                let user_present = response_data[64] != 0;
                let public_key = response_data[65..].to_vec();

                gui_log(&format!("sig_len={} user_present={} pub_key_len={}", signature.len(), user_present, public_key.len()));
                let response = Response { signature, user_present, timestamp: Utc::now() };

                gui_log("Calling complete_pairing...");
                match session.complete_pairing(
                    info.name.clone().unwrap_or_else(|| info.id.clone()),
                    public_key.clone(),
                    info.device_id.clone(),
                    &response,
                    info.rssi.unwrap_or(-50),
                    info.id.clone(),
                ).await {
                    Ok(_device) => {
                        gui_log("complete_pairing OK — device saved");
                        let _ = adapter.disconnect(&conn).await;
                        Ok(())
                    }
                    Err(e) => {
                        gui_log(&format!("complete_pairing FAILED: {}", e));
                        let _ = adapter.disconnect(&conn).await;
                        Err(format!("Pairing verification failed: {}", e))
                    }
                }
            });
            let _ = result_tx.send(result);
        });
    }

    fn forget_device(&mut self, id: Uuid) {
        let storage = self.storage.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(async { let _ = storage.delete_device(id).await; });
        }).join().ok();
        self.devices_dirty = true;
    }
}

fn describe_state(state: &SessionState) -> (String, String) {
    match state {
        SessionState::Disconnected => ("🔴 Disconnected".into(), "No watch connected.".into()),
        SessionState::Pairing { .. } => ("🟡 Pairing".into(), "Waiting for watch confirmation…".into()),
        SessionState::Verifying { .. } => ("🟡 Verifying".into(), "Checking watch signature…".into()),
        SessionState::Authenticated { device_id, last_rssi, last_seen } => (
            "🟢 Authenticated".into(),
            format!("Device: {}\nLast RSSI: {} dBm\nLast seen: {}", device_id, last_rssi, last_seen.format("%H:%M:%S")),
        ),
        SessionState::Locked => ("🔒 Locked".into(), "Screen locked, watch out of range.".into()),
    }
}

impl eframe::App for WristKeyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(500));

        if let Some(rx) = &self.scan_rx {
            while let Ok(devices) = rx.try_recv() {
                self.discovered = devices;
            }
        }

        if let Some(rx) = &self.pairing_result_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(()) => {
                        self.scan_state = ScanState::Paired;
                        self.pairing_status = "✅ Paired successfully! You can close this window and start the daemon.".into();
                        self.devices_dirty = true;
                    }
                    Err(msg) => {
                        self.scan_state = ScanState::Failed(msg.clone());
                        self.pairing_status = format!("❌ {}", msg);
                    }
                }
                self.pairing_result_rx = None;
                ctx.request_repaint();
            }
        }

        if self.devices_dirty {
            self.reload_devices();
        }

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Status, "📶 Status");
                ui.selectable_value(&mut self.tab, Tab::Devices, "⌚ Devices");
                ui.selectable_value(&mut self.tab, Tab::Settings, "⚙ Settings");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Status => self.ui_status(ui),
            Tab::Devices => self.ui_devices(ui),
            Tab::Settings => self.ui_settings(ui),
        });
    }
}

impl WristKeyApp {
    fn ui_status(&mut self, ui: &mut egui::Ui) {
        ui.heading("Connection Status");
        ui.separator();
        let snapshot = self.status.lock().map(|s| s.clone()).unwrap_or(StatusSnapshot {
            state_label: "Unknown".into(),
            detail: String::new(),
        });
        ui.label(egui::RichText::new(&snapshot.state_label).size(20.0));
        ui.add_space(8.0);
        ui.label(&snapshot.detail);
        ui.add_space(16.0);
        ui.label(format!("Paired devices: {}", self.paired_devices.len()));
    }

    fn ui_devices(&mut self, ui: &mut egui::Ui) {
        ui.heading("Registered Devices");
        ui.separator();
        if self.paired_devices.is_empty() {
            ui.label("No paired devices yet — scan for one below.");
        } else {
            let mut to_forget = None;
            for d in &self.paired_devices {
                ui.horizontal(|ui| {
                    ui.label(format!("📱 {} — baseline RSSI {} dBm", d.name, d.baseline_rssi));
                    if ui.button("🗑 Forget").clicked() {
                        to_forget = Some(d.id);
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("📏 Calibrate touch").clicked() {
                        let address = d.address.clone();
                        let name = d.name.clone();
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Builder::new_current_thread()
                                .enable_all().build().unwrap();
                            rt.block_on(async {
                                let adapter = match BtleplugAdapter::new().await {
                                    Ok(a) => a,
                                    Err(e) => { gui_log(&format!("BLE adapter: {}", e)); return; }
                                };
                                let info = PeripheralInfo {
                                    id: address,
                                    name: Some(name),
                                    pin: None,
                                    device_id: None,
                                    rssi: None,
                                    service_uuids: vec![],
                                    raw_manufacturer_data: None,
                                };
                                let conn = match adapter.connect(&info).await {
                                    Ok(c) => c,
                                    Err(e) => { gui_log(&format!("Connect: {}", e)); return; }
                                };
                                let config_char = Uuid::parse_str(CONFIG_CHAR).unwrap();
                                if let Err(e) = adapter.write(&conn, config_char, &[0x01]).await {
                                    gui_log(&format!("Start calibration failed: {}", e));
                                    let _ = adapter.disconnect(&conn).await;
                                    return;
                                }
                                gui_log("Calibration started — hold watch near PC for 10s");
                                let mut samples = Vec::new();
                                let mut ticker = tokio::time::interval(Duration::from_millis(500));
                                let start = std::time::Instant::now();
                                while start.elapsed() < Duration::from_secs(10) {
                                    ticker.tick().await;
                                    match adapter.read_rssi(&conn).await {
                                        Ok(rssi) => {
                                            samples.push(rssi);
                                            gui_log(&format!("RSSI sample: {} dBm", rssi));
                                        }
                                        Err(e) => gui_log(&format!("RSSI error: {}", e)),
                                    }
                                }
                                if samples.is_empty() {
                                    gui_log("No RSSI samples collected");
                                    let _ = adapter.write(&conn, config_char, &[0x03]).await;
                                    let _ = adapter.disconnect(&conn).await;
                                    return;
                                }
                                let avg = samples.iter().sum::<i16>() / samples.len() as i16;
                                let threshold = avg.saturating_add(5).min(-20).max(-90);
                                let rssi_byte = threshold as i8;
                                if let Err(e) = adapter.write(&conn, config_char, &[0x02, rssi_byte as u8]).await {
                                    gui_log(&format!("Send result failed: {}", e));
                                } else {
                                    gui_log(&format!("✅ Calibrated: avg={} dBm, threshold={} dBm", avg, threshold));
                                }
                                let _ = adapter.disconnect(&conn).await;
                            });
                        });
                    }
                });
            }
            if let Some(id) = to_forget {
                self.pending_forget = Some(id);
            }
        }

        if let Some(id) = self.pending_forget {
            egui::Window::new("Confirm").collapsible(false).resizable(false).show(ui.ctx(), |ui| {
                ui.label("Remove this device? It will need to be paired again to unlock your PC.");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.pending_forget = None;
                    }
                    if ui.button("Remove").clicked() {
                        self.forget_device(id);
                        self.pending_forget = None;
                    }
                });
            });
        }

        ui.add_space(20.0);
        ui.separator();
        ui.heading("Scan for New Device");
        ui.separator();

        match &self.scan_state {
            ScanState::Idle => {
                if ui.button("🔍 Scan for 30 seconds").clicked() {
                    self.start_scan();
                }
            }
            ScanState::Scanning => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Scanning… open the WristKey app on your watch and keep it nearby.");
                });
                if self.discovered.is_empty() {
                    ui.label("No WristKey devices found yet.");
                } else {
                    let mut clicked = None;
                    for info in &self.discovered {
                        let name = display_name(info);
                        let rssi_str = info.rssi.map(|r| format!("{} dBm", r)).unwrap_or_else(|| "??".into());
                        let btn_text = format!("🔗 Pair with {} ({})", name, rssi_str);
                        if ui.button(btn_text).clicked() {
                            clicked = Some(info.clone());
                        }
                    }
                    if let Some(info) = clicked {
                        self.do_pairing(info);
                    }
                }
                if ui.button("Cancel scan").clicked() {
                    self.scan_state = ScanState::Idle;
                    self.scan_rx = None;
                }
            }
            ScanState::Pairing => {
                ui.spinner();
                ui.label(&self.pairing_status);
            }
            ScanState::Failed(msg) => {
                ui.colored_label(egui::Color32::RED, format!("❌ {}", msg));
                if ui.button("Try again").clicked() {
                    self.scan_state = ScanState::Idle;
                }
            }
            ScanState::Paired => {
                ui.colored_label(egui::Color32::GREEN, "✅ Paired successfully!");
                if ui.button("Done").clicked() {
                    self.scan_state = ScanState::Idle;
                    self.devices_dirty = true;
                }
            }
        }
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.separator();

        ui.label(egui::RichText::new("Sync").strong());
        ui.horizontal(|ui| {
            ui.label("Challenge freshness window (sec):");
            ui.text_edit_singleline(&mut self.settings_form.challenge_timeout_sec);
        });
        ui.label(egui::RichText::new(
            "How long a challenge/response is considered valid. Lower = stricter             replay protection, but less tolerant of slow BLE round-trips."
        ).small().weak());

        ui.add_space(12.0);
        ui.label(egui::RichText::new("Unlock").strong());
        ui.horizontal(|ui| {
            ui.label("Auto-lock timeout (sec):");
            ui.text_edit_singleline(&mut self.settings_form.auto_lock_timeout_sec);
        });
        ui.horizontal(|ui| {
            ui.label("RSSI drop threshold (dB below baseline):");
            ui.text_edit_singleline(&mut self.settings_form.rssi_threshold_offset_dbm);
        });
        ui.label(egui::RichText::new(
            "How far the signal must drop below the baseline recorded at pairing             time before the PC locks — higher = watch needs to move further away."
        ).small().weak());

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui.button("💾 Save").clicked() {
                self.save_settings();
            }
            if ui.button("↺ Reload").clicked() {
                self.load_settings();
                self.settings_status.clear();
            }
        });
        if !self.settings_status.is_empty() {
            ui.add_space(8.0);
            ui.label(&self.settings_status);
        }
    }
}
