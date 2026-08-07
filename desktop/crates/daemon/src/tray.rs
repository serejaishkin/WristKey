//! Cross-platform system tray (requires `tray` feature).

#[allow(dead_code)]
pub enum TrayCommand {
    Quit,
    ResetPairing,
    OpenLogs,
}

#[cfg(feature = "tray")]
pub fn run_tray(cmd_tx: std::sync::mpsc::Sender<TrayCommand>) {
    use tray_icon::{
        Icon,
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        TrayIconBuilder,
    };
    use winit::application::ApplicationHandler;
    use winit::event::StartCause;
    use winit::event_loop::{ControlFlow, EventLoop};

    struct TrayApp {
        _tray_icon: tray_icon::TrayIcon,
        cmd_tx: std::sync::mpsc::Sender<TrayCommand>,
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
                if event.id == quit_id() {
                    tracing::info!("Quit selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::Quit);
                    event_loop.exit();
                } else if event.id == reset_id() {
                    tracing::info!("Reset pairing selected from tray");
                    let _ = self.cmd_tx.send(TrayCommand::ResetPairing);
                } else if event.id == logs_id() {
                    let _ = self.cmd_tx.send(TrayCommand::OpenLogs);
                }
            }
        }
    }

    fn quit_id() -> tray_icon::menu::MenuId {
        use std::sync::OnceLock;
        static ID: OnceLock<tray_icon::menu::MenuId> = OnceLock::new();
        ID.get_or_init(|| MenuItem::new("Quit", true, None).id().clone()).clone()
    }

    fn logs_id() -> tray_icon::menu::MenuId {
        use std::sync::OnceLock;
        static ID: OnceLock<tray_icon::menu::MenuId> = OnceLock::new();
        ID.get_or_init(|| MenuItem::new("Open Logs Folder", true, None).id().clone()).clone()
    }

    fn reset_id() -> tray_icon::menu::MenuId {
        use std::sync::OnceLock;
        static ID: OnceLock<tray_icon::menu::MenuId> = OnceLock::new();
        ID.get_or_init(|| MenuItem::new("Reset Pairing", true, None).id().clone()).clone()
    }

    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let menu = Menu::new();
    let _status_i = MenuItem::new("Status: Waiting for watch…", false, None);
    let _sep1 = PredefinedMenuItem::separator();
    let _devices_i = MenuItem::new("Paired Devices", true, None);
    let _settings_i = MenuItem::new("Settings", true, None);
    let _logs_i = MenuItem::new("Open Logs Folder", true, None);
    let _reset_i = MenuItem::new("Reset Pairing", true, None);
    let _sep2 = PredefinedMenuItem::separator();
    let _quit_i = MenuItem::new("Quit", true, None);

    menu.append(&_status_i).unwrap();
    menu.append(&_sep1).unwrap();
    menu.append(&_devices_i).unwrap();
    menu.append(&_settings_i).unwrap();
    menu.append(&_logs_i).unwrap();
    menu.append(&_reset_i).unwrap();
    menu.append(&_sep2).unwrap();
    menu.append(&_quit_i).unwrap();

    let icon = load_icon();

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("WristKey — PC unlock via Wear OS")
        .with_icon(icon)
        .build()
        .expect("create tray icon");

    let mut app = TrayApp { _tray_icon: tray_icon, cmd_tx };
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
