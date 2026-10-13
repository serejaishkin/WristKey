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
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::info;

pub const HEXCH: &[u8; 16] = b"0123456789abcdef";
pub const REGISTER_DOMAIN: &[u8] = b"wristkey-msa/register/v1";
pub const CHALLENGE_TTL: Duration = Duration::from_secs(300);

/// Domain-separated from `REGISTER_DOMAIN` so a registration signature can
/// never be replayed as a live unlock confirmation or vice versa. Must match
/// the constant the watch app and the desktop `MsaClient` sign/verify with.
pub const UNLOCK_DOMAIN: &[u8] = b"wristkey-msa/unlock/v1";
/// Short-lived: this proves the watch is live and in reach of the server
/// *right now*, unlike `CHALLENGE_TTL` which only needs to survive a single
/// human-paced registration round trip.
pub const UNLOCK_CHALLENGE_TTL: Duration = Duration::from_secs(30);

pub fn unlock_message(nonce: &[u8]) -> Vec<u8> {
    let mut msg = Vec::with_capacity(UNLOCK_DOMAIN.len() + nonce.len());
    msg.extend_from_slice(UNLOCK_DOMAIN);
    msg.extend_from_slice(nonce);
    msg
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WatchBinding {
    pub wristkey_id: String,
    pub pc_name: String,
    pub watch_pubkey_b64: String,
    pub msa_account: String,
    pub linked_at: String,
}

#[derive(Clone, Debug)]
struct PendingUnlock {
    nonce_b64: String,
    issued_at: Instant,
}

#[derive(Clone, Debug)]
struct UnlockResult {
    nonce_b64: String,
    signature_b64: String,
    user_present: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ServerState {
    bindings: Arc<std::sync::RwLock<HashMap<String, WatchBinding>>>,
    challenges: Arc<std::sync::RwLock<HashMap<String, Instant>>>,
    /// At most one pending unlock challenge per watch at a time, keyed by
    /// `wristkey_id`. Issuing a new one replaces any unanswered previous one.
    pending_unlocks: Arc<std::sync::RwLock<HashMap<String, PendingUnlock>>>,
    /// Verified, not-yet-collected unlock confirmations, keyed by `wristkey_id`.
    unlock_results: Arc<std::sync::RwLock<HashMap<String, UnlockResult>>>,
}

impl ServerState {
    pub fn new() -> Self { Self::default() }

    pub fn lookup(&self, wristkey_id: &str) -> Option<WatchBinding> {
        self.bindings.read().ok()?.get(wristkey_id).cloned()
    }

    /// Newest-first bindings for one MSA account. A PC without Bluetooth never
    /// learns its own wristkey_id (the server mints it at register time), so it
    /// resolves the binding by the account it authenticates with instead.
    pub fn lookup_by_account(&self, msa_account: &str) -> Vec<WatchBinding> {
        let guard = match self.bindings.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut found: Vec<WatchBinding> = guard
            .values()
            .filter(|b| b.msa_account == msa_account)
            .cloned()
            .collect();
        found.sort_by(|a, b| b.linked_at.cmp(&a.linked_at));
        found
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

    /// Issue a fresh live-unlock challenge for an already-registered watch.
    /// Replaces any previous unanswered challenge and clears any stale
    /// result, so only the most recently issued challenge can ever be
    /// answered -- a slow response to an earlier request can't be mistaken
    /// for a response to this one.
    pub fn issue_unlock_challenge(&self, wristkey_id: &str) -> Option<(String, u64)> {
        self.lookup(wristkey_id)?;
        let mut b = [0u8; 16];
        OsRng.fill_bytes(&mut b);
        let nonce_b64 = B64.encode(b);
        self.pending_unlocks.write().expect("wristkey-msa pending-unlock lock")
            .insert(wristkey_id.to_string(), PendingUnlock { nonce_b64: nonce_b64.clone(), issued_at: Instant::now() });
        self.unlock_results.write().expect("wristkey-msa unlock-result lock").remove(wristkey_id);
        Some((nonce_b64, UNLOCK_CHALLENGE_TTL.as_secs()))
    }

    /// What the watch polls: the live nonce it needs to sign, if one is
    /// still outstanding and not expired.
    pub fn peek_pending_unlock(&self, wristkey_id: &str) -> Option<String> {
        let pu = self.pending_unlocks.read().ok()?;
        let pending = pu.get(wristkey_id)?;
        if Instant::now().duration_since(pending.issued_at) >= UNLOCK_CHALLENGE_TTL {
            return None;
        }
        Some(pending.nonce_b64.clone())
    }

    /// Verify the watch's signed answer to its own pending challenge and
    /// record the result. Verified here (so a forged or stale answer is
    /// rejected before ever reaching a PC) and independently re-verified by
    /// the requesting PC once it collects the result -- this server is
    /// relaying a signature it cannot forge, not a trusted attestor.
    pub fn submit_unlock_response(&self, wristkey_id: &str, nonce_b64: &str, signature_b64: &str, user_present: bool) -> Result<(), String> {
        let binding = self.lookup(wristkey_id).ok_or_else(|| "unknown wristkey_id".to_string())?;
        let pending = {
            let pu = self.pending_unlocks.read().map_err(|_| "pending-unlock lock poisoned".to_string())?;
            pu.get(wristkey_id).cloned().ok_or_else(|| "no pending unlock challenge for this watch".to_string())?
        };
        if Instant::now().duration_since(pending.issued_at) >= UNLOCK_CHALLENGE_TTL {
            return Err("unlock challenge expired".into());
        }
        if pending.nonce_b64 != nonce_b64 {
            return Err("nonce does not match the current pending challenge".into());
        }
        let vk = decode_pub(&binding.watch_pubkey_b64)?;
        let nonce = B64.decode(nonce_b64).map_err(|e| format!("bad nonce b64: {e}"))?;
        let sig_bytes = B64.decode(signature_b64).map_err(|e| format!("bad signature b64: {e}"))?;
        let sig = Signature::from_slice(&sig_bytes).map_err(|e| format!("bad signature: {e}"))?;
        vk.verify(&unlock_message(&nonce), &sig).map_err(|_| "signature verification failed".to_string())?;

        // Single-use: a consumed challenge can't be answered twice.
        self.pending_unlocks.write().map_err(|_| "pending-unlock lock poisoned".to_string())?.remove(wristkey_id);
        self.unlock_results.write().map_err(|_| "unlock-result lock poisoned".to_string())?
            .insert(wristkey_id.to_string(), UnlockResult { nonce_b64: nonce_b64.to_string(), signature_b64: signature_b64.to_string(), user_present });
        Ok(())
    }

    /// What the PC polls: the verified result for the specific nonce it
    /// requested, consumed on read so it can't be replayed to a second poller.
    pub fn take_unlock_result(&self, wristkey_id: &str, nonce_b64: &str) -> Option<(String, bool)> {
        let mut ur = self.unlock_results.write().ok()?;
        if ur.get(wristkey_id)?.nonce_b64 != nonce_b64 {
            return None;
        }
        let result = ur.remove(wristkey_id)?;
        Some((result.signature_b64, result.user_present))
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

#[derive(Serialize)]
pub struct AccountBindingsResp {
    pub msa_account: String,
    pub bindings: Vec<StatusResp>,
}

#[derive(Serialize)]
pub struct UnlockChallengeResp {
    pub nonce_b64: String,
    pub ttl_secs: u64,
}

#[derive(Serialize)]
pub struct UnlockPendingResp {
    pub nonce_b64: String,
}

#[derive(Deserialize)]
pub struct UnlockResponseReq {
    pub nonce_b64: String,
    pub signature_b64: String,
    pub user_present: bool,
}

#[derive(Serialize)]
pub struct UnlockResultResp {
    pub nonce_b64: String,
    pub signature_b64: String,
    pub user_present: bool,
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
    // SEC1 uncompressed point: 04 || X(32) || Y(32), same as watch's KeyStoreManager.getPublicKey()
    let pubkey = p256::PublicKey::from_sec1_bytes(&bytes).map_err(|e| format!("bad P-256 pubkey: {e}"))?;
    Ok(VerifyingKey::from(pubkey))
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
    let sig = Signature::from_slice(&sig_bytes).map_err(|e| (axum::http::StatusCode::UNAUTHORIZED, format!("bad signature: {e}")))?;
    vk.verify(&register_message(&nonce), &sig)
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

/// Resolve bindings by MSA account so a PC without Bluetooth can learn the
/// wristkey_id the server minted for it. Always 200 with an (possibly empty)
/// list: "no binding yet" is a normal state, not an error.
async fn account_bindings(State(s): State<Arc<ServerState>>, axum::extract::Path(account): axum::extract::Path<String>) -> Json<AccountBindingsResp> {
    let bindings: Vec<StatusResp> = s.lookup_by_account(&account).into_iter().map(status_resp).collect();
    Json(AccountBindingsResp { msa_account: account, bindings })
}

/// A PC without Bluetooth calls this to start a live presence check: "is the
/// watch actually here right now", not just "did it ever register".
async fn unlock_challenge(State(s): State<Arc<ServerState>>, axum::extract::Path(wristkey_id): axum::extract::Path<String>) -> Result<Json<UnlockChallengeResp>, HandlerErr> {
    s.issue_unlock_challenge(&wristkey_id)
        .map(|(nonce_b64, ttl_secs)| Json(UnlockChallengeResp { nonce_b64, ttl_secs }))
        .ok_or((StatusCode::NOT_FOUND, "no such binding".into()))
}

/// The watch app polls this to learn whether a PC is waiting on it.
async fn unlock_pending(State(s): State<Arc<ServerState>>, axum::extract::Path(wristkey_id): axum::extract::Path<String>) -> Result<Json<UnlockPendingResp>, HandlerErr> {
    s.peek_pending_unlock(&wristkey_id)
        .map(|nonce_b64| Json(UnlockPendingResp { nonce_b64 }))
        .ok_or((StatusCode::NOT_FOUND, "no pending unlock challenge".into()))
}

/// The watch app posts its signed answer here, after the on-watch
/// user-presence confirmation the same as the BLE path requires.
async fn unlock_response(State(s): State<Arc<ServerState>>, axum::extract::Path(wristkey_id): axum::extract::Path<String>, Json(req): Json<UnlockResponseReq>) -> Result<StatusCode, HandlerErr> {
    s.submit_unlock_response(&wristkey_id, &req.nonce_b64, &req.signature_b64, req.user_present)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| (StatusCode::UNAUTHORIZED, e))
}

/// The waiting PC polls this for the verified result of the nonce it
/// requested; it independently re-verifies the signature itself rather than
/// trusting this 200 OK alone.
async fn unlock_result(State(s): State<Arc<ServerState>>, axum::extract::Path(wristkey_id): axum::extract::Path<String>, axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>) -> Result<Json<UnlockResultResp>, HandlerErr> {
    let nonce_b64 = params.get("nonce_b64").cloned().ok_or((StatusCode::BAD_REQUEST, "missing nonce_b64 query param".into()))?;
    s.take_unlock_result(&wristkey_id, &nonce_b64)
        .map(|(signature_b64, user_present)| Json(UnlockResultResp { nonce_b64: nonce_b64.clone(), signature_b64, user_present }))
        .ok_or((StatusCode::NOT_FOUND, "no unlock response yet".into()))
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
        .route("/api/v1/account/{msa_account}/bindings", get(account_bindings))
        .route("/api/v1/unlock/{wristkey_id}/challenge", post(unlock_challenge))
        .route("/api/v1/unlock/{wristkey_id}/pending", get(unlock_pending))
        .route("/api/v1/unlock/{wristkey_id}/response", post(unlock_response))
        .route("/api/v1/unlock/{wristkey_id}/result", get(unlock_result))
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
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{SigningKey, Signature as EcdsaSignature};

    fn test_key(byte: u8) -> SigningKey {
        SigningKey::from_bytes((&[byte; 32]).into()).unwrap()
    }

    fn signed_register(sk: &SigningKey, nonce_b64: &str) -> RegisterReq {
        let nonce = B64.decode(nonce_b64).unwrap();
        let sig: EcdsaSignature = sk.sign(&register_message(&nonce));
        RegisterReq {
            pc_name: "DESK-MSA1".into(),
            watch_pubkey_b64: B64.encode(sk.verifying_key().to_encoded_point(false).as_bytes()),
            msa_account: "user@outlook.com".into(),
            nonce_b64: nonce_b64.into(),
            signature_b64: B64.encode(sig.to_bytes()),
        }
    }

    #[test]
    fn register_then_lookup_then_status() {
        let state = ServerState::new();
        let sk = test_key(7);
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
        let sk = test_key(7);
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
        let sk = test_key(7);
        let other = test_key(9);
        let nonce = state.issue_challenge();
        let mut req = signed_register(&sk, &nonce);
        let nonce_raw = B64.decode(&nonce).unwrap();
        let sig: EcdsaSignature = other.sign(&register_message(&nonce_raw));
        req.signature_b64 = B64.encode(sig.to_bytes());
        let err = do_register(&state, req).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn register_rejects_reused_nonce() {
        let state = ServerState::new();
        let sk = test_key(7);
        let nonce = state.issue_challenge();
        do_register(&state, signed_register(&sk, &nonce)).expect("first register ok");
        let err = do_register(&state, signed_register(&sk, &nonce)).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::UNAUTHORIZED);
        assert_eq!(state.count(), 1);
    }

    #[test]
    fn register_rejects_missing_challenge() {
        let state = ServerState::new();
        let sk = test_key(7);
        let never_issued = B64.encode([1u8; 16]);
        let err = do_register(&state, signed_register(&sk, &never_issued)).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn lookup_by_account_returns_only_matching_newest_first() {
        let state = ServerState::new();
        let sk = test_key(7);
        let n1 = state.issue_challenge();
        let first = do_register(&state, signed_register(&sk, &n1)).expect("first register ok");
        let n2 = state.issue_challenge();
        let second = do_register(&state, signed_register(&sk, &n2)).expect("second register ok");

        let found = state.lookup_by_account("user@outlook.com");
        assert_eq!(found.len(), 2, "both registers belong to the same account");
        assert!(found.iter().all(|b| b.msa_account == "user@outlook.com"));
        assert_eq!(found[0].wristkey_id, second.wristkey_id, "newest binding first");
        assert_eq!(found[1].wristkey_id, first.wristkey_id);
        assert_ne!(first.wristkey_id, second.wristkey_id);

        assert!(state.lookup_by_account("other@outlook.com").is_empty(), "account isolation");
        assert!(state.lookup_by_account("").is_empty());
    }

    fn registered_watch(state: &ServerState, byte: u8) -> (SigningKey, WatchBinding) {
        let sk = test_key(byte);
        let nonce = state.issue_challenge();
        let binding = do_register(state, signed_register(&sk, &nonce)).expect("register ok");
        (sk, binding)
    }

    #[test]
    fn unlock_round_trip_succeeds_with_valid_signature() {
        let state = ServerState::new();
        let (sk, binding) = registered_watch(&state, 11);

        let (nonce_b64, ttl) = state.issue_unlock_challenge(&binding.wristkey_id).expect("issue ok");
        assert_eq!(ttl, UNLOCK_CHALLENGE_TTL.as_secs());
        assert_eq!(state.peek_pending_unlock(&binding.wristkey_id).as_deref(), Some(nonce_b64.as_str()));

        let nonce = B64.decode(&nonce_b64).unwrap();
        let sig: EcdsaSignature = sk.sign(&unlock_message(&nonce));
        let sig_b64 = B64.encode(sig.to_bytes());
        state.submit_unlock_response(&binding.wristkey_id, &nonce_b64, &sig_b64, true).expect("submit ok");

        // Consumed: a second poll for the same challenge finds nothing pending.
        assert!(state.peek_pending_unlock(&binding.wristkey_id).is_none());

        let (returned_sig_b64, user_present) = state.take_unlock_result(&binding.wristkey_id, &nonce_b64).expect("result present");
        assert_eq!(returned_sig_b64, sig_b64);
        assert!(user_present);

        // Single-use: the result can't be collected twice.
        assert!(state.take_unlock_result(&binding.wristkey_id, &nonce_b64).is_none());
    }

    #[test]
    fn unlock_rejects_wrong_key_signature() {
        let state = ServerState::new();
        let (_sk, binding) = registered_watch(&state, 11);
        let impostor = test_key(12);

        let (nonce_b64, _) = state.issue_unlock_challenge(&binding.wristkey_id).unwrap();
        let nonce = B64.decode(&nonce_b64).unwrap();
        let sig: EcdsaSignature = impostor.sign(&unlock_message(&nonce));
        let sig_b64 = B64.encode(sig.to_bytes());

        let err = state.submit_unlock_response(&binding.wristkey_id, &nonce_b64, &sig_b64, true).unwrap_err();
        assert!(err.contains("signature verification failed"), "unexpected error: {err}");
        assert!(state.take_unlock_result(&binding.wristkey_id, &nonce_b64).is_none());
    }

    #[test]
    fn unlock_rejects_stale_nonce_from_a_replaced_challenge() {
        let state = ServerState::new();
        let (sk, binding) = registered_watch(&state, 11);

        let (old_nonce_b64, _) = state.issue_unlock_challenge(&binding.wristkey_id).unwrap();
        // A PC retries and asks for a fresh challenge before the watch answers.
        let (new_nonce_b64, _) = state.issue_unlock_challenge(&binding.wristkey_id).unwrap();
        assert_ne!(old_nonce_b64, new_nonce_b64);

        let old_nonce = B64.decode(&old_nonce_b64).unwrap();
        let sig: EcdsaSignature = sk.sign(&unlock_message(&old_nonce));
        let sig_b64 = B64.encode(sig.to_bytes());

        let err = state.submit_unlock_response(&binding.wristkey_id, &old_nonce_b64, &sig_b64, true).unwrap_err();
        assert!(err.contains("does not match"), "unexpected error: {err}");
    }

    #[test]
    fn unlock_response_cannot_be_replayed_as_a_registration() {
        // The register and unlock domains must not collide: a signature
        // valid for one must never verify under the other's message.
        let state = ServerState::new();
        let (sk, binding) = registered_watch(&state, 11);
        let (nonce_b64, _) = state.issue_unlock_challenge(&binding.wristkey_id).unwrap();
        let nonce = B64.decode(&nonce_b64).unwrap();

        let unlock_sig: EcdsaSignature = sk.sign(&unlock_message(&nonce));
        let register_sig: EcdsaSignature = sk.sign(&register_message(&nonce));
        assert_ne!(unlock_sig.to_bytes(), register_sig.to_bytes());
    }

    #[test]
    fn unlock_challenge_requires_a_known_binding() {
        let state = ServerState::new();
        assert!(state.issue_unlock_challenge("wk-does-not-exist").is_none());
    }

    #[test]
    fn lookup_by_account_isolates_accounts_per_key() {
        let state = ServerState::new();

        let a = state.issue_challenge();
        let mut first = signed_register(&test_key(7), &a);
        first.msa_account = "alice@outlook.com".into();
        let first_binding = do_register(&state, first).expect("register 7 ok");

        let b = state.issue_challenge();
        let mut second = signed_register(&test_key(9), &b);
        second.msa_account = "bob@outlook.com".into();
        let second_binding = do_register(&state, second).expect("register 9 ok");

        let alice = state.lookup_by_account("alice@outlook.com");
        assert_eq!(alice.len(), 1, "alice must see only her own binding");
        assert_eq!(alice[0].wristkey_id, first_binding.wristkey_id);
        // The stored pubkey must be the key that actually signed for that account.
        assert_eq!(
            alice[0].watch_pubkey_b64,
            B64.encode(test_key(7).verifying_key().to_encoded_point(false).as_bytes())
        );

        let bob = state.lookup_by_account("bob@outlook.com");
        assert_eq!(bob.len(), 1);
        assert_eq!(bob[0].wristkey_id, second_binding.wristkey_id);
        assert_eq!(
            bob[0].watch_pubkey_b64,
            B64.encode(test_key(9).verifying_key().to_encoded_point(false).as_bytes())
        );

        assert_eq!(state.count(), 2);
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
