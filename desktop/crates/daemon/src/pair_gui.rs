//! Standalone pairing GUI — scan for watches and pair

use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use wristkey_ble::{BleAdapter, BtleplugAdapter, PeripheralInfo};
use uuid::Uuid;

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

fn main() {
    let rt = Runtime::new().expect("tokio runtime");
    
    let devices: Arc<Mutex<Vec<PeripheralInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let scanning: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let status: Arc<Mutex<String>> = Arc::new(Mutex::new("Click Scan to find watches".to_string()));
    let selected: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None::<usize>));

    let app = PairApp {
        devices,
        scanning,
        status,
        selected,
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
    status: Arc<Mutex<String>>,
    selected: Arc<Mutex<Option<usize>>>,
    rt: Runtime,
}

impl eframe::App for PairApp {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        eframe::egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🔷 WristKey Pairing");
            ui.separator();

            let scanning = *self.scanning.lock().unwrap();
            let devices = self.devices.lock().unwrap().clone();
            let status = self.status.lock().unwrap().clone();

            // Scan button
            ui.horizontal(|ui| {
                if ui.add_sized([120.0, 36.0], eframe::egui::Button::new("🔍 Scan")).clicked() && !scanning {
                    *self.scanning.lock().unwrap() = true;
                    *self.status.lock().unwrap() = "Scanning for 30 seconds…".to_string();
                    self.devices.lock().unwrap().clear();
                    
                    let devices = self.devices.clone();
                    let scanning = self.scanning.clone();
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
                                            found += 1;
                                            devices.lock().unwrap().push(info);
                                            *status.lock().unwrap() = format!("Found {} device(s)", found);
                                            ctx.request_repaint();
                                        }
                                    }
                                    Err(e) => {
                                        *status.lock().unwrap() = format!("Scan error: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                *status.lock().unwrap() = format!("BLE adapter error: {}", e);
                            }
                        }
                        *scanning.lock().unwrap() = false;
                        ctx.request_repaint();
                    });
                }
                
                if scanning {
                    ui.spinner();
                    ui.label("Scanning…");
                }
            });

            ui.separator();
            ui.label(&status);

            // Device list
            ui.separator();
            ui.heading("Found devices");
            
            eframe::egui::ScrollArea::vertical().show(ui, |ui| {
                if devices.is_empty() && !scanning {
                    ui.label("No devices found. Click Scan.");
                } else {
                    for (idx, dev) in devices.iter().enumerate() {
                        let is_selected = self.selected.lock().unwrap().map(|s| s == idx).unwrap_or(false);
                        
                        let response = ui.selectable_label(is_selected, format!(
                            "📱 {}  |  {}  |  {} dBm",
                            dev.name.as_deref().unwrap_or("Unknown"),
                            dev.id,
                            dev.rssi.map(|r| r.to_string()).unwrap_or_else(|| "N/A".into())
                        ));
                        
                        if response.clicked() {
                            *self.selected.lock().unwrap() = Some(idx);
                        }
                        
                        if is_selected {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if ui.button("🔗 Pair with this device").clicked() {
                                    *self.status.lock().unwrap() = format!("Pairing with {}…", dev.name.as_deref().unwrap_or("Unknown"));
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
