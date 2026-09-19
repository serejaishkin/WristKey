//! Shared desktop unlock flow.
//!
//! Pairing is intentionally not implemented here. This module only owns the
//! post-pairing challenge/response exchange so platform entry points do not
//! duplicate the authentication protocol.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use tracing::warn;
use wristkey_ble::{BleAdapter, PeripheralInfo};
use wristkey_core::{PairedDevice, Response, Result, SessionManager, WristKeyError};

use crate::ConnectionManager;

const SERVICE_UUID: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
const CHALLENGE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567891";
const RESPONSE_CHAR: &str = "a1b2c3d4-e5f6-7890-abcd-ef1234567892";
// Must exceed the watch's 10s confirmation timeout so a negative answer
// (1-byte refusal sent ~10s after the challenge) always reaches the PC.
const RESPONSE_WAIT_SECS: u64 = 25;

/// Perform one authenticated unlock exchange against the first paired watch.
///
/// This deliberately preserves the existing pairing and SessionManager
/// protocol. The BLE response is accepted only after SessionManager verifies
/// the freshly issued challenge.
pub async fn authenticate_watch(
    session: Arc<SessionManager>,
    ble: Arc<dyn BleAdapter>,
    conn_mgr: Arc<ConnectionManager>,
) -> Result<()> {
    let devices = session.list_paired_devices().await?;
    let device = devices
        .first()
        .ok_or_else(|| WristKeyError::Session("no paired devices".into()))?;

    authenticate_device(&session, &ble, &conn_mgr, device).await
}

pub async fn authenticate_device(
    session: &Arc<SessionManager>,
    ble: &Arc<dyn BleAdapter>,
    conn_mgr: &Arc<ConnectionManager>,
    device: &PairedDevice,
) -> Result<()> {
    let service_uuid = Uuid::parse_str(SERVICE_UUID).unwrap();
    let info = PeripheralInfo {
        id: device.address.clone(),
        name: Some(device.name.clone()),
        pin: None,
        device_id: device
            .device_id
            .as_ref()
            .and_then(|v| String::from_utf8(v.clone()).ok()),
        rssi: None,
        service_uuids: vec![service_uuid],
        raw_manufacturer_data: None,
    };

    let conn = conn_mgr.get_or_connect(ble, &info).await?;
    let challenge = session.begin_unlock(device.id).await?;

    let challenge_char = Uuid::parse_str(CHALLENGE_CHAR).unwrap();
    let response_char = Uuid::parse_str(RESPONSE_CHAR).unwrap();

    let mut write_ok = false;
    for attempt in 1..=3 {
        if ble.write(&conn, challenge_char, &challenge.to_bytes()).await.is_ok() {
            write_ok = true;
            break;
        }
        warn!("unlock challenge write attempt {} failed", attempt);
        tokio::time::sleep(Duration::from_millis(300)).await;
    }

    if !write_ok {
        let _ = ble.disconnect(&conn).await;
        return Err(WristKeyError::Ble("unlock challenge write failed".into()));
    }

    let response_data = crate::wait_for_response(ble, &conn, response_char, RESPONSE_WAIT_SECS).await?;

    if response_data.len() != 65 {
        // The watch sends a 1-byte refusal ([0]) when the user declines or
        // the confirmation UI times out; the connection is still healthy, so
        // the caller keeps the cached session for the next attempt.
        warn!("unlock: watch returned {} bytes (user denied?)", response_data.len());
        return Err(WristKeyError::Protocol(format!(
            "invalid unlock response length: {} bytes",
            response_data.len()
        )));
    }

    let response = Response {
        signature: response_data[..64].to_vec(),
        user_present: response_data[64] != 0,
        // SessionManager validates the signature against the freshly issued
        // challenge. Keep the existing protocol timestamp semantics intact.
        timestamp: chrono::Utc::now(),
    };

    session.verify_unlock(&response).await
}
