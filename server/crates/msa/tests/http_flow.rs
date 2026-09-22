use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signer, SigningKey};
use tower::ServiceExt;

use wristkey_msa::{register_message, router, ServerState};

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn post_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test]
async fn challenge_register_status_over_router() {
    let app = router(std::sync::Arc::new(ServerState::new()));

    let resp = app.clone().oneshot(post_json("/api/v1/challenge", "{}")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let challenge = body_json(resp).await;
    let nonce_b64 = challenge["nonce_b64"].as_str().unwrap().to_owned();
    assert_eq!(challenge["ttl_secs"], 300);

    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let nonce = B64.decode(&nonce_b64).unwrap();
    let sig = sk.sign(&register_message(&nonce));
    let reg = serde_json::json!({
        "pc_name": "DESK-MSA1",
        "watch_pubkey_b64": B64.encode(sk.verifying_key().to_bytes()),
        "msa_account": "user@outlook.com",
        "nonce_b64": nonce_b64,
        "signature_b64": B64.encode(sig.to_bytes()),
    });
    let resp = app.clone().oneshot(post_json("/api/v1/register", &reg.to_string())).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let registered = body_json(resp).await;
    let wristkey_id = registered["wristkey_id"].as_str().unwrap().to_owned();
    assert!(wristkey_id.starts_with("wk-"));
    assert_eq!(registered["msa_account"], "user@outlook.com");

    let resp = app
        .clone()
        .oneshot(Request::builder().uri(format!("/api/v1/status/{wristkey_id}")).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let status = body_json(resp).await;
    assert_eq!(status["wristkey_id"], wristkey_id.as_str());

    let resp = app.clone().oneshot(post_json("/api/v1/register", &reg.to_string())).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/api/v1/status/wk-missing").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
