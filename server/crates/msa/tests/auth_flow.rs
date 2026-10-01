use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use wristkey_msa::{configure_auth, router, ServerState};

#[tokio::test]
async fn bearer_token_required_when_configured() {
    configure_auth(Some("s3cret".to_owned()));
    let app = router(std::sync::Arc::new(ServerState::new()));
    let uri = "/api/v1/health";

    let resp = app.clone().oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "health must stay public");

    let resp = app
        .clone()
        .oneshot(Request::builder().uri("/api/v1/challenge").method("POST").body(Body::from("{}")).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "missing token must be rejected");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/challenge")
                .method("POST")
                .header("authorization", "Bearer wrong")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "wrong token must be rejected");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/challenge")
                .method("POST")
                .header("authorization", "Bearer s3cret")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "correct token must pass");

    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ttl_secs"], 300);

    // Resolving a binding by account exposes PC identity, so it must be
    // behind the token just like register/status.
    for token in [None, Some("wrong")] {
        let mut builder = Request::builder().uri("/api/v1/account/user@outlook.com/bindings");
        if let Some(t) = token {
            builder = builder.header("authorization", format!("Bearer {t}"));
        }
        let resp = app.clone().oneshot(builder.body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "account bindings must require the token (token={token:?})"
        );
    }

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/account/user@outlook.com/bindings")
                .header("authorization", "Bearer s3cret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "correct token must pass");
}