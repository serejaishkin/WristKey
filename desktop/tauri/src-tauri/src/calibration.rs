use std::sync::Arc;
use serde::Serialize;
use tauri::Emitter;
use wristkey_core::SessionManager;
use wristkey_ble::BleAdapter;

pub const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";

#[derive(Clone, Serialize)]
pub struct CalibrationProgress { pub device_id: String, pub samples: usize, pub target: usize, pub rssi: i32 }
#[derive(Clone, Serialize)]
pub struct CalibrationDone { pub device_id: String, pub samples: usize, pub median: i32, pub p10: i32, pub p90: i32, pub threshold: i32 }

fn percentile(values: &[i16], q: f64) -> i16 {
    let pos = q.clamp(0.0, 1.0) * (values.len().saturating_sub(1)) as f64;
    values[pos.round() as usize]
}

pub async fn run<R: tauri::Runtime>(app: &tauri::AppHandle<R>, session: Arc<SessionManager>, ble: Arc<dyn BleAdapter>, device_id: String, address: String, target: usize, threshold_offset: i16) -> Result<CalibrationDone, String> {
    let target = target.clamp(30, 60);
    let service = uuid::Uuid::parse_str(SERVICE_UUID).map_err(|e| e.to_string())?;
    let mut rx = ble.scan(service).await.map_err(|e| e.to_string())?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let mut values = Vec::<i16>::with_capacity(target);
    while values.len() < target && tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(info)) => {
                let same = info.id == address || info.device_id.as_deref() == Some(device_id.as_str());
                let rssi = info.rssi.unwrap_or(-127);
                if same && (-127..=0).contains(&rssi) {
                    values.push(rssi);
                    let _ = app.emit("calibration-progress", CalibrationProgress { device_id: device_id.clone(), samples: values.len(), target, rssi: rssi as i32 });
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
    let _ = ble.stop_scan().await;
    if values.len() < 30 { return Err(format!("Not enough RSSI samples: {}/30", values.len())); }
    values.sort_unstable();
    let median = percentile(&values, 0.50);
    let p10 = percentile(&values, 0.10);
    let p90 = percentile(&values, 0.90);
    let threshold = median.saturating_sub(threshold_offset.max(1));
    session.update_baseline_rssi(&device_id, median).await.map_err(|e| e.to_string())?;
    let done = CalibrationDone { device_id, samples: values.len(), median: median as i32, p10: p10 as i32, p90: p90 as i32, threshold: threshold as i32 };
    let _ = app.emit("calibration-done", done.clone());
    Ok(done)
}
