use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::info;

pub const HEXCH: &[u8; 16] = b"0123456789abcdef";
pub const REGISTER_DOMAIN: &[u8] = b"wristkey-msa/register/v1";
pub const CHALLENGE_TTL: Duration = Duration::from_secs(300);

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
    challenges: Arc<std::sync::RwLock<HashMap<String, Instant>>>,
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

    pub fn issue_challenge(&self) -> String {
        let mut b = [0u8; 16];
        OsRng.fill_bytes(&mut b);
        let nonce_b64 = B64.encode(b);
        let mut ch = self.challenges.write().expect("wristkey-msa challenge lock");
        let now = Instant::now();
        ch.retain(|_, issued| now.duration_since(*issued) < CHALLENGE_TTL);
        ch.insert(nonce_b64.clone(), now);
        nonce_b64
    }

    pub fn consume_challenge(&self, nonce_b64: &str) -> Result<Vec<u8>, String> {
        let mut ch = self.challenges.write().expect("wristkey-msa challenge lock");
        match ch.remove(nonce_b64) {
            None => Err("challenge unknown, expired, or already used".into()),
            Some(issued) if Instant::now().duration_since(issued) >= CHALLENGE_TTL => Err("challenge expired".into()),
            Some(_) => B64.decode(nonce_b64).map_err(|e| format!("bad nonce b64: {e}")),
        }
    }
}

#[derive(Deserialize)]
pub struct RegisterReq {
    pub pc_name: String,
    pub watch_pubkey_b64: String,
    pub msa_account: String,
    pub nonce_b64: String,
    pub signature_b64: String,
}

#[derive(Serialize)]
pub struct ChallengeResp {
    pub nonce_b64: String,
    pub ttl_secs: u64,
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

pub fn register_message(nonce: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(REGISTER_DOMAIN.len() + nonce.len());
    msg.extend_from_slice(REGISTER_DOMAIN);
    msg.extend_from_slice(nonce);
    msg
}

fn decode_pub(b64: &str) -> Result<VerifyingKey, String> {
    let bytes = B64.decode(b64).map_err(|e| format!("bad pubkey b64: {e}"))?;
    let arr: [u8; 32] = bytes.try_into().map_err(|v: Vec<u8>| format!("pubkey must be 32 bytes, got {}", v.len()))?;
    VerifyingKey::from_bytes(&arr).map_err(|e| format!("bad ed25519 pubkey: {e}"))
}

fn make_wristkey_id() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let mut out = String::with_capacity(36);
    for i in 0..16 {
        if i == 4 || i == 6 || i == 8 || i == 10 { out.push('-'); }
        out.push(HEXCH[(b[i] >> 4) as usize] as char);
        out.push(HEXCH[(b[i] & 0x0f) as usize] as char);
    }
    out
}

fn status_resp(b: WatchBinding) -> StatusResp {
    StatusResp {
        wristkey_id: b.wristkey_id,
        pc_name: b.pc_name,
        msa_account: b.msa_account,
        watch_pubkey_b64: b.watch_pubkey_b64,
        linked_at: b.linked_at,
    }
}

type HandlerErr = (axum::http::StatusCode, String);

pub fn do_register(s: &ServerState, req: RegisterReq) -> Result<WatchBinding, HandlerErr> {
    let vk = decode_pub(&req.watch_pubkey_b64).map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e))?;
    let nonce = s.consume_challenge(&req.nonce_b64).map_err(|e| (axum::http::StatusCode::UNAUTHORIZED, e))?;
    let sig_bytes = B64.decode(&req.signature_b64).map_err(|e| (axum::http::StatusCode::BAD_REQUEST, format!("bad signature b64: {e}")))?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| (axum::http::StatusCode::BAD_REQUEST, format!("bad signature: {e}")))?;
    vk.verify_strict(&register_message(&nonce), &sig)
        .map_err(|e| (axum::http::StatusCode::UNAUTHORIZED, format!("signature verification failed: {e}")))?;
    let mut wristkey_id = format!("wk-{}", make_wristkey_id());
    while s.lookup(&wristkey_id).is_some() {
        wristkey_id = format!("wk-{}", make_wristkey_id());
    }
    let b = WatchBinding {
        wristkey_id,
        pc_name: req.pc_name,
        watch_pubkey_b64: req.watch_pubkey_b64,
        msa_account: req.msa_account,
        linked_at: now_iso(),
    };
    s.insert(b.clone());
    info!(wristkey_id = %b.wristkey_id, pc = %b.pc_name, msa = %b.msa_account, "watch registered to MSA");
    Ok(b)
}

async fn challenge(State(s): State<Arc<ServerState>>) -> Json<ChallengeResp> {
    Json(ChallengeResp {
        nonce_b64: s.issue_challenge(),
        ttl_secs: CHALLENGE_TTL.as_secs(),
    })
}

async fn register(State(s): State<Arc<ServerState>>, Json(req): Json<RegisterReq>) -> Result<Json<StatusResp>, HandlerErr> {
    do_register(&s, req).map(|b| Json(status_resp(b)))
}

async fn status(State(s): State<Arc<ServerState>>, axum::extract::Path(id): axum::extract::Path<String>) -> Result<Json<StatusResp>, HandlerErr> {
    s.lookup(&id).map(|b| Json(status_resp(b))).ok_or((axum::http::StatusCode::NOT_FOUND, "no such binding".into()))
}

static AUTH_TOKEN: OnceLock<String> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppMode {
    Local,
    Lan,
}

impl AppMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "lan" => Some(Self::Lan),
            _ => None,
        }
    }

    pub fn default_listen(self) -> &'static str {
        match self {
            Self::Local => "127.0.0.1:8787",
            Self::Lan => "0.0.0.0:8787",
        }
    }
}

impl std::fmt::Display for AppMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Local => "local",
            Self::Lan => "lan",
        })
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: AppMode,
    pub listen: String,
    pub token: Option<String>,
}

impl Config {
    pub fn resolve(
        cli_mode: Option<String>,
        cli_listen: Option<String>,
        cli_token: Option<String>,
    ) -> Result<Self, String> {
        let mode = cli_mode
            .or_else(|| std::env::var("WRISTKEY_MSA_MODE").ok())
            .map(|s| {
                AppMode::parse(&s).ok_or_else(|| format!("unknown mode '{s}' (expected 'local' or 'lan')"))
            })
            .transpose()?
            .unwrap_or(AppMode::Local);
        let listen = cli_listen
            .or_else(|| std::env::var("WRISTKEY_MSA_LISTEN").ok())
            .unwrap_or_else(|| mode.default_listen().to_owned());
        let token = cli_token.or_else(|| std::env::var("WRISTKEY_MSA_TOKEN").ok());
        match mode {
            AppMode::Lan if token.is_none() => Err(
                "lan mode requires a Bearer token; pass --token or set WRISTKEY_MSA_TOKEN".into(),
            ),
            _ => Ok(Config { mode, listen, token }),
        }
    }
}

pub fn configure_auth(token: Option<String>) {
    if let Some(t) = token {
        let _ = AUTH_TOKEN.set(t);
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) { diff |= x ^ y; }
    diff == 0
}

async fn bearer_auth(req: Request, next: Next) -> Result<axum::response::Response, StatusCode> {
    let Some(expected) = AUTH_TOKEN.get() else {
        return Ok(next.run(req).await);
    };
    let header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let provided = header
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !ct_eq(provided.as_bytes(), expected.as_bytes()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}

async fn health() -> &'static str { "ok" }

pub fn router(state: Arc<ServerState>) -> Router {
    let api = Router::new()
        .route("/api/v1/challenge", post(challenge))
        .route("/api/v1/register", post(register))
        .route("/api/v1/status/{wristkey_id}", get(status))
        .layer(axum::middleware::from_fn(bearer_auth));
    Router::new()
        .route("/api/v1/health", get(health))
        .merge(api)
        .with_state(state)
}

pub async fn run(listen: &str) -> std::io::Result<()> {
    let auth = AUTH_TOKEN.get().is_some();
    let is_lan = listen.starts_with("0.0.0.0") || listen.starts_with("[::") || listen.starts_with("::");
    if is_lan && !auth {
        tracing::warn!("listening on {listen} WITHOUT auth token - anyone on the LAN can call this API. Set WRISTKEY_MSA_TOKEN.");
    }
    let state = Arc::new(ServerState::new());
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    info!("wristkey-msa listening on {listen} (auth: {})", if auth { "Bearer token" } else { "none" });
    axum::serve(listener, app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed_register(sk: &SigningKey, nonce_b64: &str) -> RegisterReq {
        let nonce = B64.decode(nonce_b64).unwrap();
        let sig = sk.sign(&register_message(&nonce));
        RegisterReq {
            pc_name: "DESK-MSA1".into(),
            watch_pubkey_b64: B64.encode(sk.verifying_key().to_bytes()),
            msa_account: "user@outlook.com".into(),
            nonce_b64: nonce_b64.into(),
            signature_b64: B64.encode(sig.to_bytes()),
        }
    }

    #[test]
    fn register_then_lookup_then_status() {
        let state = ServerState::new();
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let nonce = state.issue_challenge();
        let b = do_register(&state, signed_register(&sk, &nonce)).expect("register ok");
        assert_eq!(state.count(), 1);
        assert_eq!(state.lookup(&b.wristkey_id).unwrap().msa_account, "user@outlook.com");
        assert_eq!(b.wristkey_id.len(), 3 + 36);
    }

    #[test]
    fn challenge_is_single_use() {
        let state = ServerState::new();
        let nonce = state.issue_challenge();
        assert!(state.consume_challenge(&nonce).is_ok());
        assert!(state.consume_challenge(&nonce).is_err());
        assert!(state.consume_challenge("no-such-nonce").is_err());
    }

    #[test]
    fn register_rejects_bad_signature() {
        let state = ServerState::new();
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let nonce = state.issue_challenge();
        let mut req = signed_register(&sk, &nonce);
        req.signature_b64 = B64.encode([0u8; 64]);
        let err = do_register(&state, req).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(state.count(), 0);
    }

    #[test]
    fn register_rejects_foreign_key_signature() {
        let state = ServerState::new();
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let other = SigningKey::from_bytes(&[9u8; 32]);
        let nonce = state.issue_challenge();
        let mut req = signed_register(&sk, &nonce);
        let nonce_raw = B64.decode(&nonce).unwrap();
        req.signature_b64 = B64.encode(other.sign(&register_message(&nonce_raw)).to_bytes());
        let err = do_register(&state, req).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn register_rejects_reused_nonce() {
        let state = ServerState::new();
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let nonce = state.issue_challenge();
        do_register(&state, signed_register(&sk, &nonce)).expect("first register ok");
        let err = do_register(&state, signed_register(&sk, &nonce)).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(state.count(), 1);
    }

    #[test]
    fn register_rejects_missing_challenge() {
        let state = ServerState::new();
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let never_issued = B64.encode([1u8; 16]);
        let err = do_register(&state, signed_register(&sk, &never_issued)).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn app_mode_parse_and_defaults() {
        assert_eq!(AppMode::parse("local"), Some(AppMode::Local));
        assert_eq!(AppMode::parse("lan"), Some(AppMode::Lan));
        assert_eq!(AppMode::parse("relay"), None);
        assert_eq!(AppMode::Local.default_listen(), "127.0.0.1:8787");
        assert_eq!(AppMode::Lan.default_listen(), "0.0.0.0:8787");
        assert_eq!(AppMode::Local.to_string(), "local");
        assert_eq!(AppMode::Lan.to_string(), "lan");
    }

    #[test]
    fn config_resolution() {
        let saved_mode = std::env::var("WRISTKEY_MSA_MODE").ok();
        let saved_listen = std::env::var("WRISTKEY_MSA_LISTEN").ok();
        let saved_token = std::env::var("WRISTKEY_MSA_TOKEN").ok();
        std::env::remove_var("WRISTKEY_MSA_MODE");
        std::env::remove_var("WRISTKEY_MSA_LISTEN");
        std::env::remove_var("WRISTKEY_MSA_TOKEN");

        let cfg = Config::resolve(None, None, None).unwrap();
        assert_eq!(cfg.mode, AppMode::Local);
        assert_eq!(cfg.listen, "127.0.0.1:8787");
        assert!(cfg.token.is_none());

        let err = Config::resolve(Some("lan".into()), None, None).unwrap_err();
        assert!(err.contains("token"), "lan without token must fail: {err}");

        let cfg = Config::resolve(Some("lan".into()), None, Some("t".into())).unwrap();
        assert_eq!(cfg.mode, AppMode::Lan);
        assert_eq!(cfg.listen, "0.0.0.0:8787");
        assert_eq!(cfg.token.as_deref(), Some("t"));

        let err = Config::resolve(Some("relay".into()), None, None).unwrap_err();
        assert!(err.contains("unknown mode"), "unexpected: {err}");

        let cfg = Config::resolve(Some("local".into()), Some("10.0.0.5:9999".into()), None).unwrap();
        assert_eq!(cfg.listen, "10.0.0.5:9999");

        std::env::set_var("WRISTKEY_MSA_MODE", "lan");
        std::env::set_var("WRISTKEY_MSA_TOKEN", "envtok");
        let cfg = Config::resolve(None, None, None).unwrap();
        assert_eq!(cfg.mode, AppMode::Lan);
        assert_eq!(cfg.token.as_deref(), Some("envtok"));

        let cfg = Config::resolve(Some("local".into()), Some("127.0.0.1:1".into()), Some("flagtok".into()))
            .unwrap();
        assert_eq!(cfg.mode, AppMode::Local);
        assert_eq!(cfg.token.as_deref(), Some("flagtok"));

        match saved_mode {
            Some(v) => std::env::set_var("WRISTKEY_MSA_MODE", v),
            None => std::env::remove_var("WRISTKEY_MSA_MODE"),
        }
        match saved_listen {
            Some(v) => std::env::set_var("WRISTKEY_MSA_LISTEN", v),
            None => std::env::remove_var("WRISTKEY_MSA_LISTEN"),
        }
        match saved_token {
            Some(v) => std::env::set_var("WRISTKEY_MSA_TOKEN", v),
            None => std::env::remove_var("WRISTKEY_MSA_TOKEN"),
        }
    }
}
