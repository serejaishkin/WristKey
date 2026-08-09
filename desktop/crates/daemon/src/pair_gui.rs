//! Standalone pairing GUI — scan for watches and pair

use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use wristkey_ble::{BleAdapter, BtleplugAdapter, PeripheralInfo};
use uuid::Uuid;

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";

fn main() {
    let rt = Runtime::new().expect("tokio runtime");
    
    let devices: Arc<Mutex<Vec<PeripheralInfo>>> = Arc::new(Mutex::new(Vec::new()));
    let scanning: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let status: Arc<Mutex<String>> = Arc::new(Mutex::new("Click Scan to find watches".to_string()));
    let selected: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None::<usize>));
    let pairing: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    let app = PairApp {
        devices,
        scanning,
        status,
        selected,
        pairing,
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

            // Scan button
            ui.horizontal(|ui| {
                if ui.add_sized([120.0, 36.0], eframe::egui::Button::new("🔍 Scan")).clicked() && !scanning && !pairing {
                    *self.scanning.lock().unwrap() = true;
                    *self.status.lock().unwrap() = "Scanning for 30 seconds…".to_string();
                    self.devices.lock().unwrap().clear();
                    *self.selected.lock().unwrap() = None;
                    
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
                        
                        if is_selected && !pairing {
                            ui.horizontal(|ui| {
                                ui.add_space(24.0);
                                if ui.button("🔗 Pair with this device").clicked() {
                                    *self.pairing.lock().unwrap() = true;
                                    *self.status.lock().unwrap() = format!("Pairing with {}…", dev.name.as_deref().unwrap_or("Unknown"));
                                    
                                    let devices = self.devices.clone();
                                    let status = self.status.clone();
                                    let pairing = self.pairing.clone();
                                    let selected = self.selected.clone();
                                    let ctx = ctx.clone();
                                    
                                    self.rt.spawn(async move {
                                        let idx = selected.lock().unwrap().unwrap_or(0);
                                        let dev = {
                                            let d = devices.lock().unwrap();
                                            d.get(idx).cloned()
                                        };
                                        
                                        if let Some(dev) = dev {
                                            match do_pairing(dev).await {
                                                Ok(_) => {
                                                    *status.lock().unwrap() = "✅ Paired successfully!".to_string();
                                                }
                                                Err(e) => {
                                                    *status.lock().unwrap() = format!("❌ Pairing failed: {}", e);
                                                }
                                            }
                                        } else {
                                            *status.lock().unwrap() = "Device not found".to_string();
                                        }
                                        *pairing.lock().unwrap() = false;
                                        ctx.request_repaint();
                                    });
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

async fn do_pairing(dev: PeripheralInfo) -> Result<(), Box<dyn std::error::Error>> {
    let adapter = BtleplugAdapter::new().await.map_err(|e| format!("adapter: {}", e))?;
    let conn = adapter.connect(&dev).await.map_err(|e| format!("connect: {}", e))?;
    
    // TODO: send challenge, verify response, save public key
    // For now just connect and disconnect to test
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    adapter.disconnect(&conn).await.map_err(|e| format!("disconnect: {}", e))?;
    Ok(())
}
