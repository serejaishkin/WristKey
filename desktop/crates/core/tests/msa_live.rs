//! Integration test for the PC-side LAN flow: the real `wristkey-msa` binary is
//! started in `lan` mode, a binding is registered for an account (playing the
//! watch), and `MsaClient` must resolve the server-minted `wristkey_id`.
//!
//! Ignored unless `WRISTKEY_MSA_EXE` points at the built binary:
//! ```text
//! cargo test -p wristkey-core --test msa_live -- --ignored
//! ```

use std::process::{Child, Command, Stdio};

use wristkey_core::{MsaClient, MsaError};

const TOKEN: &str = "live-test-token-987654";

fn exe() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("WRISTKEY_MSA_EXE")
            .unwrap_or_else(|_| "D:\\GitHub\\WristKey\\server\\target\\release\\wristkey-msa.exe".into()),
    )
}

struct Server(Child);

impl Server {
    fn start(listen: &str) -> Self {
        let mut child = Command::new(exe())
            .args(["--mode", "lan", "--listen", listen, "--token", TOKEN])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn wristkey-msa; build it via server/build-server.cmd build");
        let client = MsaClient::new(format!("http://{listen}"), None);
        for _ in 0..50 {
            if let Ok(()) = futures_lite_block_on(client.health()) {
                return Self(child);
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let _ = child.kill();
        panic!("wristkey-msa did not become healthy on {listen}");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

/// Tiny blocking bridge: the client is async but these tests only need to await
/// a single request, so a current-thread runtime per call is enough.
fn futures_lite_block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(fut)
}

fn post(base: &str, path: &str, body: serde_json::Value) -> reqwest::Response {
    futures_lite_block_on(
        reqwest::Client::new()
            .post(format!("{base}{path}"))
            .bearer_auth(TOKEN)
            .json(&body)
            .send(),
    )
    .expect("post failed")
}

fn body_json(resp: reqwest::Response) -> serde_json::Value {
    futures_lite_block_on(resp.json()).expect("response must be JSON")
}

fn challenge_nonce(base: &str) -> String {
    let resp = post(base, "/api/v1/challenge", serde_json::json!({}));
    assert_eq!(resp.status(), 200);
    body_json(resp)["nonce_b64"].as_str().unwrap().to_owned()
}

/// Register a binding the same way the watch does: P-256 signature over
/// `domain || nonce`.
fn register_watch(base: &str, account: &str) -> serde_json::Value {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::{Signature as EcdsaSignature, SigningKey};

    let sk = SigningKey::from_bytes((&[3u8; 32]).into()).unwrap();
    let nonce_b64 = challenge_nonce(base);
    let nonce = B64.decode(&nonce_b64).unwrap();
    let mut message = b"wristkey-msa/register/v1".to_vec();
    message.extend_from_slice(&nonce);
    let sig: EcdsaSignature = sk.sign(&message);

    let resp = post(
        base,
        "/api/v1/register",
        serde_json::json!({
            "pc_name": "DESK-LIVE",
            "watch_pubkey_b64": B64.encode(sk.verifying_key().to_encoded_point(false).as_bytes()),
            "msa_account": account,
            "nonce_b64": nonce_b64,
            "signature_b64": B64.encode(sig.to_bytes()),
        }),
    );
    assert_eq!(resp.status(), 200, "watch register must succeed");
    let json = body_json(resp);
    assert!(json["wristkey_id"].as_str().unwrap().starts_with("wk-"));
    json
}

#[test]
#[ignore = "needs the wristkey-msa binary; set WRISTKEY_MSA_EXE"]
fn pc_resolves_its_binding_by_account() {
    let listen = "127.0.0.1:18791";
    let _server = Server::start(listen);
    let base = format!("http://{listen}");
    let account = "live@wristkey.local";

    // Before the watch registers, resolution reports "no binding" rather than failing.
    let client = MsaClient::new(&base, Some(TOKEN.into()));
    assert_eq!(futures_lite_block_on(client.resolve_own_binding(account)).unwrap(), None);

    let registered = register_watch(&base, account);
    let expected_id = registered["wristkey_id"].as_str().unwrap();

    // This is the point of the whole flow: the PC learns the id the server minted.
    let resolved = futures_lite_block_on(client.resolve_own_binding(account))
        .expect("resolve must succeed")
        .expect("binding must exist after the watch registered");
    assert_eq!(resolved.wristkey_id, expected_id);
    assert_eq!(resolved.msa_account, account);
    assert_eq!(resolved.pc_name, "DESK-LIVE");
    assert_eq!(
        resolved.watch_pubkey_b64,
        registered["watch_pubkey_b64"].as_str().unwrap(),
        "client must return the pubkey the watch signed with"
    );

    // Same binding via the id-based lookup.
    let by_id = futures_lite_block_on(client.binding_by_id(expected_id)).unwrap();
    assert_eq!(by_id.unwrap().wristkey_id, expected_id);
    assert!(futures_lite_block_on(client.binding_by_id("wk-does-not-exist")).unwrap().is_none());

    // Account isolation: another account must not see it.
    assert!(futures_lite_block_on(client.resolve_own_binding("other@wristkey.local")).unwrap().is_none());

    // A second register for the same account becomes the newest binding.
    let second = register_watch(&base, account);
    let newest = futures_lite_block_on(client.resolve_own_binding(account)).unwrap().unwrap();
    assert_eq!(newest.wristkey_id, second["wristkey_id"].as_str().unwrap());
    assert_ne!(newest.wristkey_id, expected_id);
}

#[test]
#[ignore = "needs the wristkey-msa binary; set WRISTKEY_MSA_EXE"]
fn lan_mode_rejects_missing_and_wrong_token() {
    let listen = "127.0.0.1:18792";
    let _server = Server::start(listen);
    let base = format!("http://{listen}");
    register_watch(&base, "auth@wristkey.local");

    // No token -> Unauthorized, reported distinctly so the CLI can say so.
    match futures_lite_block_on(MsaClient::new(&base, None).resolve_own_binding("auth@wristkey.local")) {
        Err(MsaError::Unauthorized(_)) => {}
        other => panic!("expected Unauthorized without token, got {other:?}"),
    }

    // Wrong token -> same error.
    match futures_lite_block_on(
        MsaClient::new(&base, Some("nope".into())).resolve_own_binding("auth@wristkey.local"),
    ) {
        Err(MsaError::Unauthorized(_)) => {}
        other => panic!("expected Unauthorized with wrong token, got {other:?}"),
    }

    // Correct token works, and health stays public.
    let ok = futures_lite_block_on(MsaClient::new(&base, Some(TOKEN.into())).resolve_own_binding("auth@wristkey.local"));
    assert!(ok.unwrap().is_some());
    futures_lite_block_on(MsaClient::new(&base, None).health()).expect("health must be public");
}