//! Cross-platform system tray (requires `tray` feature).

#[cfg(feature = "tray")]
pub fn run_tray() {
    use tray_icon::{
        icon::Icon,
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        TrayIconBuilder,
    };
    use winit::event::Event;
    use winit::event_loop::{ControlFlow, EventLoop};

    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Wait);

    let menu = Menu::new();
    let status_i = MenuItem::new("Status: Waiting for watch…", false, None);
    let sep1 = PredefinedMenuItem::separator();
    let devices_i = MenuItem::new("Paired Devices", true, None);
    let settings_i = MenuItem::new("Settings", true, None);
    let logs_i = MenuItem::new("Open Logs Folder", true, None);
    let sep2 = PredefinedMenuItem::separator();
    let quit_i = MenuItem::new("Quit", true, None);

    menu.append(&status_i).unwrap();
    menu.append(&sep1).unwrap();
    menu.append(&devices_i).unwrap();
    menu.append(&settings_i).unwrap();
    menu.append(&logs_i).unwrap();
    menu.append(&sep2).unwrap();
    menu.append(&quit_i).unwrap();

    let icon = load_icon();

    let mut tray_icon = None;

    event_loop
        .run(move |event, elwt| {
            if let Event::NewEvents(winit::event::StartCause::Init) = event {
                tray_icon = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(menu.clone()))
                        .with_tooltip("WristKey — PC unlock via Wear OS")
                        .with_icon(icon.clone())
                        .build()
                        .expect("create tray icon"),
                );
            }

            if let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == quit_i.id() {
                    tracing::info!("Quit selected from tray");
                    elwt.exit();
                } else if event.id == logs_i.id() {
                    let log_dir = directories::ProjectDirs::from("", "", "WristKey")
                        .map(|d| d.data_dir().join("logs"))
                        .unwrap_or_else(|| std::path::PathBuf::from("/tmp/wristkey/logs"));
                    let _ = std::process::Command::new("xdg-open")
                        .arg(&log_dir)
                        .spawn();
                }
            }
        })
        .expect("event loop");
}

#[cfg(feature = "tray")]
fn load_icon() -> tray_icon::icon::Icon {
    let (w, h) = (32, 32);
    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.chunks_exact_mut(4) {
        px[0] = 66;
        px[1] = 133;
        px[2] = 244;
        px[3] = 255;
    }
    tray_icon::icon::Icon::from_rgba(rgba, w as u32, h as u32).expect("valid icon")
}

#[cfg(not(feature = "tray"))]
pub fn run_tray() {
    tracing::warn!("Tray feature not enabled. Running in headless mode.");
    // Block forever so tokio tasks keep running
    std::thread::park();
}
