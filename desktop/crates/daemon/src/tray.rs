//! Cross-platform system tray (requires `tray` feature).

#[allow(dead_code)]
pub enum TrayCommand {
    Quit,
    ResetPairing,
    OpenLogs,
    PairDevice,
    StopScan,
    ClearScanList,
}

#[cfg(feature = "tray")]
pub fn run_tray(cmd_tx: std::sync::mpsc::Sender<TrayCommand>) {
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        TrayIconBuilder,
    };
    use winit::application::ApplicationHandler;
    use winit::event::StartCause;
    use winit::event_loop::{ControlFlow, EventLoop};

    struct TrayApp {
        _tray_icon: tray_icon::TrayIcon,
        cmd_tx: std::sync::mpsc::Sender<TrayCommand>,
        quit_id: tray_icon::menu::MenuId,
        reset_id: tray_icon::menu::MenuId,
        logs_id: tray_icon::menu::MenuId,
        pair_id: tray_icon::menu::MenuId,
        stop_scan_id: tray_icon::menu::MenuId,
        clear_scan_id: tray_icon::menu::MenuId,
    }

    impl ApplicationHandler for TrayApp {
        fn new_events(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, _cause: StartCause) {
            self.process_menu_events(_event_loop);
        }
        fn resumed(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop) {}
        fn window_event(
            &mut self,
            _event_loop: &winit::event_loop::ActiveEventLoop,
            _window_id: winit::window::WindowId,
            _event: winit::event::WindowEvent,
        ) {}
        fn about_to_wait(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
            self.process_menu_events(event_loop);
        }
    }

    impl TrayApp {
        fn process_menu_events(&self, event_loop: &winit::event_loop::ActiveEventLoop) {
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == self.quit_id {
                    tracing::info!("Quit selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::Quit);
                    event_loop.exit();
                } else if event.id == self.reset_id {
                    tracing::info!("Reset pairing selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::ResetPairing);
                } else if event.id == self.logs_id {
                    tracing::info!("Open logs selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::OpenLogs);
                } else if event.id == self.pair_id {
                    tracing::info!("Pair device selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::PairDevice);
                } else if event.id == self.stop_scan_id {
                    tracing::info!("Stop scan selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::StopScan);
                } else if event.id == self.clear_scan_id {
                    tracing::info!("Clear scan list selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::ClearScanList);
                } else if event.id == self.stop_scan_id {
                    tracing::info!("Stop scan selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::StopScan);
                } else if event.id == self.clear_scan_id {
                    tracing::info!("Clear scan list selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::ClearScanList);
                }
            }
        }
    }

    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let menu = Menu::new();
    let _status_i = MenuItem::new("Status: Waiting for watch…", false, None);
    let _sep1 = PredefinedMenuItem::separator();
    let _devices_i = MenuItem::new("Paired Devices", false, None);
    let pair_i = MenuItem::new("Pair New Device", true, None);
    let stop_scan_i = MenuItem::new("Stop Scan", true, None);
    let clear_scan_i = MenuItem::new("Clear Scan List", true, None);
    let _settings_i = MenuItem::new("Settings", false, None);
    let logs_i = MenuItem::new("Open Logs Folder", true, None);
    let reset_i = MenuItem::new("Reset Pairing", true, None);
    let _sep2 = PredefinedMenuItem::separator();
    let quit_i = MenuItem::new("Quit", true, None);

    menu.append(&_status_i).unwrap();
    menu.append(&_sep1).unwrap();
    menu.append(&_devices_i).unwrap();
    menu.append(&pair_i).unwrap();
    menu.append(&stop_scan_i).unwrap();
    menu.append(&clear_scan_i).unwrap();
    menu.append(&_settings_i).unwrap();
    menu.append(&logs_i).unwrap();
    menu.append(&reset_i).unwrap();
    menu.append(&_sep2).unwrap();
    menu.append(&quit_i).unwrap();

    let icon = load_icon();

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("WristKey — PC unlock via Wear OS")
        .with_icon(icon)
        .build()
        .expect("create tray icon");

    let mut app = TrayApp {
        _tray_icon: tray_icon,
        cmd_tx,
        quit_id: quit_i.id().clone(),
        reset_id: reset_i.id().clone(),
        logs_id: logs_i.id().clone(),
        pair_id: pair_i.id().clone(),
        stop_scan_id: stop_scan_i.id().clone(),
        clear_scan_id: clear_scan_i.id().clone(),
    };
    event_loop.run_app(&mut app).expect("event loop");
}

#[cfg(feature = "tray")]
fn load_icon() -> tray_icon::Icon {
    let (w, h) = (32, 32);
    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.chunks_exact_mut(4) {
        px[0] = 66;
        px[1] = 133;
        px[2] = 244;
        px[3] = 255;
    }
    tray_icon::Icon::from_rgba(rgba, w as u32, h as u32).expect("valid icon")
}

#[cfg(not(feature = "tray"))]
pub fn run_tray(_cmd_tx: std::sync::mpsc::Sender<TrayCommand>) {
    tracing::warn!("Tray feature not enabled. Running in headless mode.");
    std::thread::park();
}
