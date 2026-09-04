#[cfg(windows)]
pub mod win_service {
    use std::ffi::OsString;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::Duration;
    use tokio::sync::watch;
    use tracing::{info, error};
    use windows_service::{
        define_windows_service,
        service::{
            ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode, ServiceInfo,
            ServiceStartType, ServiceState, ServiceStatus, ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    use wristkey_core::{SessionManager, EcdsaP256Crypto, SqliteStorage, PlatformSecurity};
    use wristkey_ble::{BleAdapter, BtleplugAdapter};
    use wristkey_daemon::{Daemon, ConnectionManager};
    use wristkey_platform_win::WindowsSecurity;

    const SERVICE_NAME: &str = "WristKey";
    const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

    define_windows_service!(ffi_service_main, service_main_entry);

    fn service_main_entry(_arguments: Vec<OsString>) {
        if let Err(e) = run_service() {
            error!("Service error: {:?}", e);
        }
    }

    fn run_service() -> windows_service::Result<()> {
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

        let event_handler = move |control_event: ServiceControl| -> ServiceControlHandlerResult {
            match control_event {
                ServiceControl::Stop => {
                    info!("Service stop requested");
                    let _ = shutdown_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::UserEvent(code) => {
                    if code.to_raw() == 130 {
                        let _ = shutdown_tx.send(());
                    }
                    ServiceControlHandlerResult::NoError
                }
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };

        let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;

        status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::StartPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::from_secs(10),
            process_id: None,
        })?;

        info!("WristKey service starting...");

        // Init logging
        let log_dir = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("WristKey/logs");
        std::fs::create_dir_all(&log_dir).ok();

        status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        // Block on tokio runtime for the daemon
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

        let storage: Arc<dyn wristkey_core::Storage> = match SqliteStorage::open_default() {
            Ok(s) => { info!("SQLite storage opened"); Arc::new(s) }
            Err(e) => { error!("SQLite open failed: {}", e); return Ok(()); }
        };

        let crypto = Arc::new(EcdsaP256Crypto);
        let session = Arc::new(SessionManager::new(crypto, storage));

        let mut win_sec = WindowsSecurity::new();
        win_sec.set_session(session.clone());
        let platform: Arc<dyn PlatformSecurity> = Arc::new(win_sec);

        let ble: Arc<dyn BleAdapter> = match rt.block_on(BtleplugAdapter::new()) {
            Ok(a) => Arc::new(a),
            Err(e) => { error!("BLE adapter unavailable: {}", e); return Ok(()); }
        };

        let conn_mgr = Arc::new(ConnectionManager::new());
        let daemon = Daemon::new(session, ble, platform, conn_mgr);

        let (tokio_shutdown_tx, tokio_shutdown_rx) = watch::channel(());

        // Spawn the daemon on the tokio runtime
        let daemon_handle = rt.spawn(async move {
            if let Err(e) = daemon.run(tokio_shutdown_rx).await {
                error!("Daemon error: {}", e);
            }
        });

        // Block on the service control channel
        loop {
            match shutdown_rx.recv_timeout(Duration::from_secs(1)) {
                Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => (),
            }
        }

        // Signal tokio daemon to shut down
        let _ = tokio_shutdown_tx.send(());
        rt.block_on(async { let _ = daemon_handle.await; });

        status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        info!("WristKey service stopped");
        Ok(())
    }

    pub fn start_service_dispatcher() {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
            .expect("Failed to start service dispatcher");
    }

    pub fn install_service(exe_path: &str) -> Result<(), String> {
        let manager_access = ServiceManagerAccess::CONNECT
            | ServiceManagerAccess::CREATE_SERVICE
            | ServiceManagerAccess::ENUMERATE_SERVICE;
        let manager = ServiceManager::local_computer(None::<&str>, manager_access)
            .map_err(|e| format!("Service manager open failed: {:?}", e))?;

        // If service already exists, stop and delete it first
        if let Ok(existing) = manager.open_service(
            SERVICE_NAME,
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        ) {
            if let Ok(status) = existing.query_status() {
                if status.current_state != ServiceState::Stopped {
                    let _ = existing.stop();
                    // Wait briefly for stop
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
            let _ = existing.delete();
            // Wait briefly for deletion
            std::thread::sleep(std::time::Duration::from_secs(1));
        }

        let service_info = ServiceInfo {
            name: OsString::from(SERVICE_NAME),
            display_name: OsString::from("WristKey BLE Unlock"),
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: std::path::PathBuf::from(exe_path),
            launch_arguments: vec![OsString::from("--service")],
            dependencies: vec![],
            account_name: None,
            account_password: None,
        };

        let service = manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)
            .map_err(|e| format!("Create service failed: {:?}", e))?;

        service.set_description("WristKey BLE PC Unlock Service")
            .map_err(|e| format!("Set description failed: {:?}", e))?;

        info!("Service installed");
        Ok(())
    }

    pub fn uninstall_service() -> Result<(), String> {
        let manager_access = ServiceManagerAccess::CONNECT;
        let manager = ServiceManager::local_computer(None::<&str>, manager_access)
            .map_err(|e| format!("Service manager open failed: {:?}", e))?;

        let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
        let service = manager.open_service(SERVICE_NAME, service_access)
            .map_err(|e| format!("Open service failed: {:?}", e))?;

        if service.query_status().map(|s| s.current_state).unwrap_or(ServiceState::Stopped) != ServiceState::Stopped {
            let _ = service.stop();
        }

        service.delete().map_err(|e| format!("Delete service failed: {:?}", e))?;
        info!("Service uninstalled");
        Ok(())
    }

    pub fn get_service_status() -> Result<String, String> {
        let manager_access = ServiceManagerAccess::CONNECT;
        let manager = ServiceManager::local_computer(None::<&str>, manager_access)
            .map_err(|e| format!("Service manager open failed: {:?}", e))?;

        let service = manager.open_service(SERVICE_NAME, ServiceAccess::QUERY_STATUS)
            .map_err(|_| "Service not installed".to_string())?;

        let status = service.query_status()
            .map_err(|e| format!("Query status failed: {:?}", e))?;
        Ok(format!("{:?}", status.current_state))
    }
}
