use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;
#[tokio::test]
async fn hooks_api_shares_claims_and_revision_conflicts_with_cli_and_enforces_owner() {
    use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
    let root = tempfile::tempdir().unwrap();
    write_test_config(root.path());
    let handle = ProcessHandle {
        kind: ProcessKind::Project,
        project_root: root.path().into(),
        pid: None,
    };
    macc_core::service::process_ownership::register_process(root.path(), handle.clone()).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    macc_core::service::process_ownership::claim_owner(
        root.path(),
        &handle,
        ClientIdentity {
            client_id: "owner".into(),
            kind: ClientKind::Web,
            connected_at: now.clone(),
            last_heartbeat: now,
        },
    )
    .unwrap();
    let app = build_web_router(test_web_state(
        root.path(),
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    ));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/hooks")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let view: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view["hooks"].as_array().unwrap().len(), 7);
    let payload = serde_json::json!({"action":"set","id":"lint-errors-only","enabled":false,"revision":view["revision"]});
    let request = |client: &str, payload: &serde_json::Value| {
        Request::builder()
            .method("POST")
            .uri("/api/v1/hooks/action")
            .header("X-Macc-Client-Id", client)
            .body(Body::from(payload.to_string()))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(request("viewer", &payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = app
        .clone()
        .oneshot(request("owner", &payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let stale = app
        .clone()
        .oneshot(request("owner", &payload))
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let current = macc_core::hooks::execute(root.path(), macc_core::hooks::Action::Status).unwrap();
    let payload = serde_json::json!({"action":"set","id":"performer-log-summary","enabled":false,"revision":current.revision});
    let blocked = app.oneshot(request("owner", &payload)).await.unwrap();
    assert_eq!(blocked.status(), StatusCode::BAD_REQUEST);
    let text = String::from_utf8(
        blocked
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains("O2 context engine"));
}
