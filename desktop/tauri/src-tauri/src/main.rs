#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::watch;
use tauri::{Manager, State, RunEvent};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tracing::{info, error, warn};
use wristkey_core::{Config, SessionManager, EcdsaP256Crypto, SqliteStorage, PlatformSecurity, Response, TouchPoint, PC_TOUCH_TOLERANCE};
use wristkey_daemon::{Daemon, ConnectionManager};
use wristkey_ble::{BleAdapter, BtleplugAdapter, NullBleAdapter, PeripheralInfo};
#[cfg(target_os = "windows")]
use wristkey_platform_win::WindowsSecurity;
#[cfg(target_os = "linux")]
use wristkey_platform_linux::LinuxSecurity;
#[cfg(target_os = "macos")]
use wristkey_platform_macos::MacosSecurity;
static LOG_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
const PUBLIC_KEY_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567893";
const TRAINING_CONTROL_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567899";
fn get_pc_name() -> String { #[cfg(target_os = "windows")] { std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Unknown PC".to_string()) } #[cfg(not(target_os = "windows"))] { std::env::var("HOSTNAME").unwrap_or_else(|_| "Unknown PC".to_string()) } }
#[derive(serde::Serialize)] struct StatusDto { state: String, detail: String, device_count: usize, daemon_enabled: bool, #[cfg(target_os = "windows")] cp_registered: bool, storage_type: String }
#[derive(serde::Serialize)] struct DeviceDto { id: String, name: String, address: String, baseline_rssi: i32 }
#[derive(serde::Serialize)] struct ScanResultDto { id: String, name: String, rssi: i32, address: String }
#[derive(serde::Serialize)] struct CalibrationResultDto { avg: i32, threshold: i32, samples: usize }
#[derive(serde::Deserialize)] struct PairRequest { id: String, name: String, rssi: i32, address: String }
#[derive(Default)] struct ProximityDiagnostics { filtered_rssi: Option<f64>, last_rssi: Option<i16>, state: String }
#[derive(serde::Serialize)] struct ProximityStatusDto { connected: bool, watch: String, address: String, raw_rssi: Option<i32>, filtered_rssi: Option<f64>, baseline_rssi: Option<i32>, delta_db: Option<f64>, proximity_state: String }
struct AppState { session: Arc<SessionManager>, config: Arc<Mutex<Config>>, daemon: Arc<Mutex<Option<(tokio::task::JoinHandle<()>, watch::Sender<()>)>>>, platform: Arc<dyn PlatformSecurity>, ble: Arc<dyn BleAdapter>, conn_mgr: Arc<ConnectionManager>, proximity: Arc<Mutex<ProximityDiagnostics>> }
fn create_platform_adapter(session: Arc<SessionManager>) -> Arc<dyn PlatformSecurity> { #[cfg(target_os = "windows")] { let mut win = WindowsSecurity::new(); win.set_session(session.clone()); Arc::new(win) } #[cfg(target_os = "linux")] { let mut linux = LinuxSecurity::new(); linux.set_session(session.clone()); Arc::new(linux) } #[cfg(target_os = "macos")] { let mut mac = MacosSecurity::new(); mac.set_session(session.clone()); Arc::new(mac) } }
#[tauri::command] async fn lock_screen(state: State<'_, Arc<AppState>>)->Result<(),String>{state.platform.lock_screen().await.map_err(|e|e.to_string())}
#[tauri::command] async fn get_log_dir() -> Result<String, String> { LOG_DIR.get().map(|p| p.display().to_string()).ok_or_else(|| "log directory not initialized".to_string()) }
#[tauri::command] async fn get_status(state: State<'_, Arc<AppState>>) -> Result<StatusDto, String> { let session_state=state.session.state().await; let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?; let daemon_enabled=state.daemon.lock().unwrap().is_some(); #[cfg(target_os = "windows")] let cp_registered=WindowsSecurity::is_credential_provider_registered(); #[cfg(not(target_os = "windows"))] let cp_registered=false; #[cfg(target_os = "windows")] let storage_type=WindowsSecurity::storage_type_description().to_string(); #[cfg(target_os = "linux")] let storage_type=LinuxSecurity::storage_type_description().to_string(); #[cfg(target_os = "macos")] let storage_type=MacosSecurity::storage_type_description().to_string(); let device_count=devices.len(); let connected=match devices.first(){Some(device)=>state.conn_mgr.get_cached(&device.address).await.is_some(),None=>false}; let state_str=if connected{"connected"}else if session_state.is_authenticated(){"authenticated"}else{"disconnected"}; let detail=if connected{format!("Connected to {}",devices.first().map(|d|d.name.clone()).unwrap_or_else(||"unknown".to_string()))}else if session_state.is_authenticated(){format!("Authenticated with {}",devices.first().map(|d|d.name.clone()).unwrap_or_else(||"unknown".to_string()))}else if device_count>0{format!("{} device(s) paired, reconnecting...",device_count)}else{"No paired devices".to_string()}; Ok(StatusDto{state:state_str.to_string(),detail,device_count,daemon_enabled:true,#[cfg(target_os = "windows")] cp_registered,storage_type}) }
#[tauri::command] async fn get_paired_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<DeviceDto>, String> { let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?; Ok(devices.into_iter().map(|d|DeviceDto{id:d.id.to_string(),name:d.name,address:d.address,baseline_rssi:d.baseline_rssi as i32}).collect()) }
#[tauri::command] async fn get_proximity_status(state: State<'_, Arc<AppState>>) -> Result<ProximityStatusDto, String> { let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?; let device=match devices.first(){Some(d)=>d,None=>return Ok(ProximityStatusDto{connected:false,watch:"—".into(),address:"—".into(),raw_rssi:None,filtered_rssi:None,baseline_rssi:None,delta_db:None,proximity_state:"UNKNOWN".into()})}; let service_uuid=uuid::Uuid::parse_str(SERVICE_UUID).map_err(|e|e.to_string())?; let info=PeripheralInfo{id:device.address.clone(),name:Some(device.name.clone()),pin:None,device_id:device.device_id.as_ref().and_then(|v|String::from_utf8(v.clone()).ok()),rssi:None,service_uuids:vec![service_uuid],raw_manufacturer_data:None}; let conn=match state.conn_mgr.get_or_connect(&state.ble,&info).await{Ok(c)=>c,Err(_)=>{let mut d=state.proximity.lock().unwrap();d.filtered_rssi=None;d.last_rssi=None;d.state="AWAY".into();return Ok(ProximityStatusDto{connected:false,watch:device.name.clone(),address:device.address.clone(),raw_rssi:None,filtered_rssi:None,baseline_rssi:Some(device.baseline_rssi as i32),delta_db:None,proximity_state:"AWAY".into()})}}; let raw=state.ble.read_rssi(&conn).await.map_err(|e|e.to_string())?; let baseline=device.baseline_rssi as f64; let mut d=state.proximity.lock().unwrap(); let filtered=d.filtered_rssi.map(|p|p*0.75+raw as f64*0.25).unwrap_or(raw as f64); let delta=filtered-baseline; let proximity_state=if delta>=-5.0{"PRESENT"}else if delta>=-15.0{"NEAR"}else if delta>=-25.0{"SUSPECTED_AWAY"}else{"AWAY"}; d.filtered_rssi=Some(filtered);d.last_rssi=Some(raw);d.state=proximity_state.into(); Ok(ProximityStatusDto{connected:true,watch:device.name.clone(),address:device.address.clone(),raw_rssi:Some(raw as i32),filtered_rssi:Some(filtered),baseline_rssi:Some(device.baseline_rssi as i32),delta_db:Some(delta),proximity_state:proximity_state.into()}) }
#[tauri::command] async fn scan_devices(state: State<'_, Arc<AppState>>) -> Result<Vec<ScanResultDto>, String> { let service_uuid=uuid::Uuid::parse_str(SERVICE_UUID).unwrap(); let mut rx=state.ble.scan(service_uuid).await.map_err(|e|e.to_string())?; let deadline=tokio::time::Instant::now()+std::time::Duration::from_secs(5); let mut found=Vec::new(); while tokio::time::Instant::now()<deadline{let remaining=deadline.saturating_duration_since(tokio::time::Instant::now());if remaining.is_zero(){break;}match tokio::time::timeout(remaining,rx.recv()).await{Ok(Some(info))=>found.push(ScanResultDto{id:info.id.clone(),name:info.name.unwrap_or_else(||"Unknown".to_string()),rssi:info.rssi.unwrap_or(-100) as i32,address:info.id}),Ok(None)|Err(_)=>break}} let _=state.ble.stop_scan().await;Ok(found) }
#[tauri::command] async fn pair_device(state: State<'_, Arc<AppState>>, req: PairRequest) -> Result<(), String> { let service_uuid=uuid::Uuid::parse_str(SERVICE_UUID).unwrap();let challenge_char=uuid::Uuid::parse_str(CHALLENGE_CHAR).unwrap();let response_char=uuid::Uuid::parse_str(RESPONSE_CHAR).unwrap();let public_key_char=uuid::Uuid::parse_str(PUBLIC_KEY_CHAR).unwrap();let pc_name_char=uuid::Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567898").unwrap();let info=PeripheralInfo{id:req.address.clone(),name:Some(req.name.clone()),pin:None,device_id:Some(req.id.clone()),rssi:Some(req.rssi as i16),service_uuids:vec![service_uuid],raw_manufacturer_data:None};info!("Pairing: connecting to {} ({})",req.name,req.address);let conn=state.ble.connect(&info).await.map_err(|e|e.to_string())?;tokio::time::sleep(std::time::Duration::from_millis(500)).await;let pc_name=get_pc_name();state.ble.write(&conn,pc_name_char,pc_name.as_bytes()).await.map_err(|e|e.to_string())?;let mut rx=state.ble.notify(&conn,response_char).await.map_err(|e|e.to_string())?;let challenge=state.session.begin_pairing().await.map_err(|e|e.to_string())?;state.ble.write(&conn,challenge_char,&challenge.to_bytes()).await.map_err(|e|e.to_string())?;let response_data=match tokio::time::timeout(std::time::Duration::from_secs(30),rx.recv()).await{Ok(Some(d))=>d,_=>{let _=state.ble.disconnect(&conn).await;return Err("Pairing response timeout".into())}};if response_data.len()<65{let _=state.ble.disconnect(&conn).await;return Err(format!("Pairing response too short: {} bytes",response_data.len()));}let signature=response_data[..64].to_vec();let user_present=response_data[64]!=0;let public_key=state.ble.read(&conn,public_key_char).await.map_err(|e|format!("Failed to read public key: {}",e))?;let response=Response{signature,user_present,timestamp:chrono::Utc::now()};state.session.complete_pairing(req.name,public_key,Some(req.id.into_bytes()),&response,req.rssi as i16,req.address.clone()).await.map_err(|e|e.to_string())?;#[cfg(target_os="windows")] {use wristkey_platform_win::WindowsVault;let vault=WindowsVault::new();let device=state.session.list_paired_devices().await.map_err(|e|e.to_string())?.into_iter().find(|d|d.address==req.address).ok_or("Paired device not found")?;let pairing_key=vault.ensure_device(&device.id.to_string(),&device.name,&device.address).map_err(|e|e.to_string())?;let pairing_key_char=uuid::Uuid::parse_str("a1b2c3d4-e5f6-7890-abcd-ef1234567897").unwrap();state.ble.write(&conn,pairing_key_char,&pairing_key).await.map_err(|e|format!("Failed to send pairing key: {}",e))?;}let _=state.ble.disconnect(&conn).await;Ok(()) }
#[tauri::command] async fn forget_device(state: State<'_, Arc<AppState>>, id:String)->Result<(),String>{state.session.forget_device(&id).await.map_err(|e|e.to_string())}
#[tauri::command] async fn calibrate_device(state: State<'_, Arc<AppState>>, id:String)->Result<CalibrationResultDto,String>{let uuid=uuid::Uuid::parse_str(&id).map_err(|e|format!("Invalid device id: {}",e))?;let device=state.session.load_device(uuid).await.map_err(|e|e.to_string())?.ok_or_else(||"Paired device not found".to_string())?;let service_uuid=uuid::Uuid::parse_str(SERVICE_UUID).unwrap();let info=PeripheralInfo{id:device.address.clone(),name:Some(device.name.clone()),pin:None,device_id:device.device_id.as_ref().and_then(|v|String::from_utf8(v.clone()).ok()),rssi:None,service_uuids:vec![service_uuid],raw_manufacturer_data:None};let conn=state.conn_mgr.get_or_connect(&state.ble,&info).await.map_err(|e|format!("Calibration BLE connect failed: {}",e))?;let mut samples=Vec::<i16>::with_capacity(20);for _ in 0..20{match state.ble.read_rssi(&conn).await{Ok(rssi) if (-127..=0).contains(&rssi)=>samples.push(rssi),Ok(rssi)=>warn!("Ignoring invalid calibration RSSI {}",rssi),Err(e)=>warn!("Calibration RSSI read failed: {}",e)}tokio::time::sleep(std::time::Duration::from_millis(300)).await;}if samples.len()<10{return Err(format!("Calibration failed: only {} valid RSSI samples",samples.len()));}let sum:i32=samples.iter().map(|v|*v as i32).sum();let avg=sum/(samples.len() as i32);let threshold=avg-15;state.session.update_baseline_rssi(uuid,avg as i16).await.map_err(|e|e.to_string())?;info!("Proximity calibration complete: device={} avg={} threshold={} samples={}",device.name,avg,threshold,samples.len());Ok(CalibrationResultDto{avg,threshold,samples:samples.len()})}
#[tauri::command] async fn start_daemon(state: State<'_, Arc<AppState>>)->Result<(),String>{let mut g=state.daemon.lock().unwrap();if g.is_some(){return Err("Daemon already running".into())}let (shutdown_tx,shutdown_rx)=watch::channel(());let daemon=Daemon::new(state.session.clone(),state.ble.clone(),state.platform.clone(),state.conn_mgr.clone());let h=tokio::spawn(async move{if let Err(e)=daemon.run(shutdown_rx).await{error!("Daemon error: {}",e)}});*g=Some((h,shutdown_tx));info!("Daemon started");Ok(())}
#[tauri::command] async fn stop_daemon(state: State<'_, Arc<AppState>>)->Result<(),String>{let h = { let mut g = state.daemon.lock().unwrap(); g.take().map(|(h, tx)| { let _ = tx.send(()); h }) }; if let Some(h) = h { let _ = h.await; info!("Daemon stopped"); } Ok(())}
#[cfg(target_os="windows")]
#[tauri::command] async fn set_windows_password(state: State<'_, Arc<AppState>>,password:String)->Result<(),String>{use wristkey_platform_win::WindowsVault;let vault=WindowsVault::new();let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?;if let Some(device)=devices.first(){vault.set_password(&device.id.to_string(),&password).map_err(|e|e.to_string())?;Ok(())}else{Err("No paired device found. Pair a watch first.".into())}}
#[cfg(not(target_os="windows"))]
#[tauri::command] async fn set_windows_password(_state: State<'_, Arc<AppState>>,_password:String)->Result<(),String>{Err("Windows password storage is only available on Windows".into())}
#[tauri::command] async fn get_config(state: State<'_, Arc<AppState>>)->Result<Config,String>{Ok(state.config.lock().unwrap().clone())}
#[cfg(target_os="windows")]
#[tauri::command] async fn register_credential_provider(app: tauri::AppHandle)->Result<(),String>{
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(exe)=std::env::current_exe(){if let Some(d)=exe.parent(){candidates.push(d.join("WristKeyCredentialProvider.dll"));}}
    if let Ok(rd)=app.path().resource_dir(){candidates.push(rd.join("_up_").join("_up_").join("crates").join("credential-provider").join("bin").join("Release").join("net48").join("WristKeyCredentialProvider.dll"));}
    candidates.push(std::path::PathBuf::from(r"C:\Program Files\WristKey\WristKeyCredentialProvider.dll"));
    let dll=candidates.iter().find(|p|p.exists()).ok_or_else(||"Credential Provider DLL not found. Build it: dotnet build -c Release in desktop/crates/credential-provider".to_string())?;
    WindowsSecurity::register_credential_provider(dll.to_string_lossy().as_ref())
}
#[cfg(not(target_os="windows"))]
#[tauri::command] async fn register_credential_provider()->Result<(),String>{Err("Credential Provider is only available on Windows".into())}
#[cfg(target_os="windows")]
#[tauri::command] async fn unregister_credential_provider()->Result<(),String>{WindowsSecurity::unregister_credential_provider()}
#[cfg(not(target_os="windows"))]
#[tauri::command] async fn unregister_credential_provider()->Result<(),String>{Err("Credential Provider is only available on Windows".into())}
#[tauri::command] async fn update_config(state: State<'_, Arc<AppState>>,new_config:Config)->Result<(),String>{*state.config.lock().unwrap()=new_config;Ok(())}
#[tauri::command] async fn set_config(state: State<'_, Arc<AppState>>,config:Config)->Result<(),String>{update_config(state,config).await}
#[tauri::command] async fn get_logs()->Result<Vec<String>,String>{use std::fs;let log_dir=LOG_DIR.get().ok_or("log dir not initialized")?;let mut lines=Vec::new();for i in (0..=5).rev(){let path=if i==0{log_dir.join("wristkey.log")}else{log_dir.join(format!("wristkey.log.{}",i))};if path.exists(){if let Ok(content)=fs::read_to_string(&path){for line in content.lines().rev(){lines.push(line.to_string());}}}}Ok(lines)}
#[derive(serde::Deserialize)] struct TrainTouchRequest { x: f64, y: f64 }
#[derive(serde::Serialize)] struct TouchStatusDto { trained: bool, x: Option<f64>, y: Option<f64> }
#[derive(serde::Serialize, Clone)] struct TrainingStatusDto { state: String, countdown: Option<i32>, samples: Option<i32>, x: Option<f64>, y: Option<f64> }
#[tauri::command] async fn train_pc_touch(state: State<'_, Arc<AppState>>, req: TrainTouchRequest)->Result<(),String>{let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?;let device_id=devices.first().map(|d|d.id.to_string()).unwrap_or_else(||"default".to_string());state.session.save_touch_point(&device_id,&TouchPoint{x:req.x,y:req.y}).await.map_err(|e|e.to_string())?;info!("PC touch point trained: ({},{}) for device {}",req.x,req.y,device_id);Ok(())}
#[tauri::command] async fn get_pc_touch_status(state: State<'_, Arc<AppState>>)->Result<TouchStatusDto,String>{let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?;let device_id=devices.first().map(|d|d.id.to_string()).unwrap_or_else(||"default".to_string());match state.session.load_touch_point(&device_id).await.map_err(|e|e.to_string())?{Some(p)=>Ok(TouchStatusDto{trained:true,x:Some(p.x),y:Some(p.y)}),None=>Ok(TouchStatusDto{trained:false,x:None,y:None})}}
#[tauri::command] async fn clear_pc_touch(state: State<'_, Arc<AppState>>)->Result<(),String>{let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?;let device_id=devices.first().map(|d|d.id.to_string()).unwrap_or_else(||"default".to_string());state.session.delete_touch_point(&device_id).await.map_err(|e|e.to_string())?;info!("PC touch point cleared for device {}",device_id);Ok(())}
#[tauri::command] async fn verify_pc_touch(state: State<'_, Arc<AppState>>, x:f64, y:f64)->Result<bool,String>{let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?;let device_id=devices.first().map(|d|d.id.to_string()).unwrap_or_else(||"default".to_string());match state.session.load_touch_point(&device_id).await.map_err(|e|e.to_string())?{Some(trained)=>Ok(SessionManager::verify_touch_point(&trained,&TouchPoint{x,y},PC_TOUCH_TOLERANCE)),None=>Ok(true)}}

static TRAINING_STATE: std::sync::OnceLock<std::sync::Mutex<TrainingStatusDto>> = std::sync::OnceLock::new();

#[tauri::command] async fn start_watch_training(state: State<'_, Arc<AppState>>)->Result<(),String>{let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?;let device=devices.first().ok_or("No paired device")?;let service_uuid=uuid::Uuid::parse_str(SERVICE_UUID).map_err(|e|e.to_string())?;let training_uuid=uuid::Uuid::parse_str(TRAINING_CONTROL_CHAR).map_err(|e|e.to_string())?;let info=PeripheralInfo{id:device.address.clone(),name:Some(device.name.clone()),pin:None,device_id:device.device_id.as_ref().and_then(|v|String::from_utf8(v.clone()).ok()),rssi:None,service_uuids:vec![service_uuid],raw_manufacturer_data:None};let conn=state.conn_mgr.get_or_connect(&state.ble,&info).await.map_err(|e|format!("BLE connect failed: {}",e))?;state.ble.write(&conn,training_uuid,b"{\"action\":\"start_training\"}").await.map_err(|e|format!("BLE write failed: {}",e))?;info!("Sent start_training to watch");Ok(())}

#[tauri::command] async fn subscribe_watch_training(state: State<'_, Arc<AppState>>)->Result<(),String>{let devices=state.session.list_paired_devices().await.map_err(|e|e.to_string())?;let device=devices.first().ok_or("No paired device")?;let service_uuid=uuid::Uuid::parse_str(SERVICE_UUID).map_err(|e|e.to_string())?;let training_uuid=uuid::Uuid::parse_str(TRAINING_CONTROL_CHAR).map_err(|e|e.to_string())?;let info=PeripheralInfo{id:device.address.clone(),name:Some(device.name.clone()),pin:None,device_id:device.device_id.as_ref().and_then(|v|String::from_utf8(v.clone()).ok()),rssi:None,service_uuids:vec![service_uuid],raw_manufacturer_data:None};let conn=state.conn_mgr.get_or_connect(&state.ble,&info).await.map_err(|e|format!("BLE connect failed: {}",e))?;let mut rx=state.ble.notify(&conn,training_uuid).await.map_err(|e|format!("BLE subscribe failed: {}",e))?;let state_store=TRAINING_STATE.get_or_init(||std::sync::Mutex::new(TrainingStatusDto{state:"idle".into(),countdown:None,samples:None,x:None,y:None}));tokio::spawn(async move{while let Some(data)=rx.recv().await{if let Ok(json)=serde_json::from_slice::<serde_json::Value>(&data){let mut s=state_store.lock().unwrap();s.state=json.get("state").and_then(|v|v.as_str()).unwrap_or("unknown").to_string();s.countdown=json.get("countdown").and_then(|v|v.as_i64()).map(|v|v as i32);s.samples=json.get("samples").and_then(|v|v.as_i64()).map(|v|v as i32);s.x=json.get("x").and_then(|v|v.as_f64());s.y=json.get("y").and_then(|v|v.as_f64());info!("Watch training state: {} countdown={:?} samples={:?} x={:?} y={:?}",s.state,s.countdown,s.samples,s.x,s.y)}}});Ok(())}

#[tauri::command] async fn get_watch_training_status()->Result<TrainingStatusDto,String>{let store=TRAINING_STATE.get_or_init(||std::sync::Mutex::new(TrainingStatusDto{state:"idle".into(),countdown:None,samples:None,x:None,y:None}));Ok(store.lock().unwrap().clone())} 
mod log_rolling;
#[cfg(windows)] mod service;

#[cfg(windows)]
#[tauri::command]
async fn install_windows_service() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    service::win_service::install_service(exe.to_string_lossy().as_ref())?;
    Ok("Service installed successfully".into())
}

#[cfg(not(windows))]
#[tauri::command]
async fn install_windows_service() -> Result<String, String> { Err("Windows service is only available on Windows".into()) }

#[cfg(windows)]
#[tauri::command]
async fn uninstall_windows_service() -> Result<String, String> {
    service::win_service::uninstall_service()?;
    Ok("Service uninstalled successfully".into())
}

#[cfg(not(windows))]
#[tauri::command]
async fn uninstall_windows_service() -> Result<String, String> { Err("Windows service is only available on Windows".into()) }

#[cfg(windows)]
#[tauri::command]
async fn get_windows_service_status() -> Result<String, String> {
    service::win_service::get_service_status()
}

#[cfg(not(windows))]
#[tauri::command]
async fn get_windows_service_status() -> Result<String, String> { Ok("N/A (not Windows)".into()) }

#[tokio::main]
async fn main() {
    #[cfg(windows)]
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "--service") {
            tracing_subscriber::fmt().with_ansi(false).with_level(true).init();
            info!("WristKey running as Windows service");
            service::win_service::start_service_dispatcher();
            return;
        }
    }

    let log_dir = std::env::var("WRISTKEY_LOG_DIR")
        .map(|s| std::path::PathBuf::from(s))
        .unwrap_or_else(|_| {
            let mut p = dirs::data_local_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            p.push("WristKey/logs");
            p
        });std::fs::create_dir_all(&log_dir).ok();LOG_DIR.set(log_dir.clone()).ok();let writer=log_rolling::RollingWriter::new(&log_dir.join("wristkey.log"),20*1024*1024,5).expect("failed to open log file");let(non_blocking,_guard)=tracing_appender::non_blocking(writer);tracing_subscriber::fmt().with_writer(non_blocking).with_ansi(false).with_level(true).with_target(true).init();info!("WristKey starting up...");let config=Config::from_file(&dirs::config_dir().unwrap_or_else(||std::path::PathBuf::from(".")).join("WristKey/config.toml")).unwrap_or_default();let storage:Arc<dyn wristkey_core::Storage>=match SqliteStorage::open_default(){Ok(storage)=>{info!("Persistent SQLite storage opened; paired watches will survive desktop restarts");Arc::new(storage)},Err(e)=>{error!("Failed to open persistent SQLite storage: {}",e);return;}};let crypto=Arc::new(EcdsaP256Crypto);let session=Arc::new(SessionManager::new(crypto,storage));let platform=create_platform_adapter(session.clone());let ble:Arc<dyn BleAdapter>=match BtleplugAdapter::new().await{Ok(a)=>Arc::new(a),Err(e)=>{warn!("BLE adapter unavailable, running without BLE: {}",e);Arc::new(NullBleAdapter)}};let conn_mgr=Arc::new(ConnectionManager::new());let app_state=Arc::new(AppState{session:session.clone(),config:Arc::new(Mutex::new(config)),daemon:Arc::new(Mutex::new(None)),platform,ble,conn_mgr:conn_mgr.clone(),proximity:Arc::new(Mutex::new(ProximityDiagnostics::default()))});tauri::Builder::default().manage(app_state).invoke_handler(tauri::generate_handler![get_status,get_paired_devices,get_proximity_status,scan_devices,pair_device,forget_device,calibrate_device,start_daemon,stop_daemon,set_windows_password,get_config,update_config,set_config,get_logs,get_log_dir,lock_screen,register_credential_provider,unregister_credential_provider,train_pc_touch,get_pc_touch_status,clear_pc_touch,verify_pc_touch,start_watch_training,subscribe_watch_training,get_watch_training_status,install_windows_service,uninstall_windows_service,get_windows_service_status]).setup(|app|{let s:tauri::State<Arc<AppState>>=app.state();let session=s.session.clone();let ble=s.ble.clone();let platform=s.platform.clone();let conn_mgr=s.conn_mgr.clone();tauri::async_runtime::spawn(async move{let (_shutdown_tx,shutdown_rx)=watch::channel(());let daemon=Daemon::new(session,ble,platform,conn_mgr);if let Err(e)=daemon.run(shutdown_rx).await{error!("Background daemon error: {}",e)}});let handle=app.handle().clone();let quit_item=MenuItem::with_id(&handle,"quit","Quit",true,None::<&str>)?;let menu=Menu::with_items(&handle,&[&PredefinedMenuItem::separator(&handle)?,&quit_item])?;let _tray=TrayIconBuilder::new().icon(handle.default_window_icon().unwrap().clone()).menu(&menu).on_menu_event(|app,event|{if event.id().as_ref()=="quit"{app.exit(0)}}).on_tray_icon_event(|tray,event|{if matches!(event, TrayIconEvent::DoubleClick { .. }) {let app=tray.app_handle();if let Some(window)=app.get_webview_window("main"){let _=window.show();let _=window.set_focus();}}}).build(&handle)?;Ok(())}).on_window_event(|window,event|{if let tauri::WindowEvent::CloseRequested{api,..}=event{window.hide().ok();api.prevent_close()}}).build(tauri::generate_context!()).expect("error while running tauri application").run(|_app_handle,event|{if let RunEvent::ExitRequested{api,..}=event{api.prevent_exit()}});}
