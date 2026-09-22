use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::VerifyingKey;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::info;

pub const HEXCH: &[u8; 16] = b"0123456789abcdef";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WatchBinding {
    pub wristkey_id: String,
    pub pc_name: String,
    pub watch_pubkey_b64: String,
    pub msa_account: String,
    pub linked_at: String,
}

#[derive(Clone, Debug, Default)]
pub struct ServerState {
    bindings: Arc<std::sync::RwLock<HashMap<String, WatchBinding>>>,
}

impl ServerState {
    pub fn new() -> Self { Self::default() }
    pub fn lookup(&self, wristkey_id: &str) -> Option<WatchBinding> {
        self.bindings.read().ok()?.get(wristkey_id).cloned()
    }
    pub fn insert(&self, b: WatchBinding) {
        self.bindings.write().expect("wristkey-msa state lock").insert(b.wristkey_id.clone(), b);
    }
    pub fn count(&self) -> usize {
        self.bindings.read().map(|g| g.len()).unwrap_or(0)
    }
}

#[derive(Deserialize)]
pub struct RegisterReq {
    pub pc_name: String,
    pub watch_pubkey_b64: String,
    pub msa_account: String,
    pub signature_b64: String,
}

#[derive(Deserialize)]
pub struct LinkReq {
    pub wristkey_id: String,
    pub pc_name: String,
    pub signature_b64: String,
}

#[derive(Serialize)]
pub struct StatusResp {
    pub wristkey_id: String,
    pub pc_name: String,
    pub msa_account: String,
    pub watch_pubkey_b64: String,
    pub linked_at: String,
}

pub fn now_iso() -> String { chrono::Utc::now().to_rfc3339() }

pub fn uuid_v4() -> String { make_wristkey_id() }

fn decode_pub(b64: &str) -> Result<VerifyingKey, String> {
    let bytes = B64.decode(b64).map_err(|e| format!("bad pubkey b64: {e}"))?;
    let arr: [u8; 32] = bytes.try_into().map_err(|v: Vec<u8>| format!("pubkey must be 32 bytes, got {}", v.len()))?;
    VerifyingKey::from_bytes(&arr).map_err(|e| format!("bad ed25519 pubkey: {e}"))
}

fn make_wristkey_id() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
    let mut out = String::with_capacity(36);
    for i in 0..16 {
        if i == 4 || i == 6 || i == 8 || i == 10 { out.push('-'); }
        out.push(HEXCH[(b[i] >> 4) as usize] as char);
        out.push(HEXCH[(b[i] & 0x0f) as usize] as char);
    }
    out
}

async fn register(State(s): State<Arc<ServerState>>, Json(req): Json<RegisterReq>) -> Result<Json<StatusResp>, (axum::http::StatusCode, String)> {
    decode_pub(&req.watch_pubkey_b64).map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e))?;
    let mut wristkey_id = format!("wk-{}", make_wristkey_id());
    while s.lookup(&wristkey_id).is_some() { wristkey_id = format!("wk-{}", make_wristkey_id()); }
    let b = WatchBinding {
        wristkey_id: wristkey_id.clone(),
        pc_name: req.pc_name.clone(),
        watch_pubkey_b64: req.watch_pubkey_b64.clone(),
        msa_account: req.msa_account.clone(),
        linked_at: now_iso(),
    };
    s.insert(b.clone());
    info!(wristkey_id = %wristkey_id, pc = %req.pc_name, msa = %req.msa_account, "watch registered to MSA");
    Ok(Json(StatusResp {
        wristkey_id,
        pc_name: b.pc_name,
        watch_pubkey_b64: b.watch_pubkey_b64,
        msa_account: b.msa_account,
        linked_at: b.linked_at,
    }))
}

async fn status(State(s): State<Arc<ServerState>>, axum::extract::Path(id): axum::extract::Path<String>) -> Result<Json<StatusResp>, (axum::http::StatusCode, String)> {
    s.lookup(&id).map(|b| Json(StatusResp {
        wristkey_id: b.wristkey_id,
        pc_name: b.pc_name,
        watch_pubkey_b64: b.watch_pubkey_b64,
        msa_account: b.msa_account,
        linked_at: b.linked_at,
    })).ok_or((axum::http::StatusCode::NOT_FOUND, "no such binding".into()))
}

pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/api/v1/register", post(register))
        .route("/api/v1/status/:wristkey_id", get(status))
        .with_state(state)
}

pub async fn run(listen: &str) -> std::io::Result<()> {
    let state = Arc::new(ServerState::new());
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    info!("wristkey-msa listening on {listen}");
    axum::serve(listener, app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_lookup_then_status() {
        let _ = tracing::info_span!("test");
        let state = Arc::new(ServerState::new());
        let req = RegisterReq {
            pc_name: "DESK-MSA1".into(),
            watch_pubkey_b64: "b64pub".into(),
            msa_account: "user@outlook.com".into(),
            signature_b64: "sig".into(),
        };
        let wristkey_id = format!("wk-{}", make_wristkey_id());
        let b = WatchBinding {
            wristkey_id: wristkey_id.clone(),
            pc_name: req.pc_name.clone(),
            watch_pubkey_b64: req.watch_pubkey_b64.clone(),
            msa_account: req.msa_account.clone(),
            linked_at: now_iso(),
        };
        state.insert(b.clone());
        assert_eq!(state.count(), 1);
        assert_eq!(state.lookup(&wristkey_id).unwrap().msa_account, "user@outlook.com");
    }
}
