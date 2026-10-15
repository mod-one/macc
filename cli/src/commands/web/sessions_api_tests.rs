use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;
async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    payload: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn session_policy_api_validates_cas_and_never_returns_lease_secrets() {
    let root = tempfile::tempdir().unwrap();
    write_test_config(root.path());
    let app = build_web_router(test_web_state(
        root.path(),
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    ));
    let (code, initial) = call(
        &app,
        "GET",
        "/api/v1/sessions/store",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(initial["fields"].as_array().unwrap().len(), 3);
    assert!(initial.to_string().find("lease_token").is_none());
    let mut policy = initial["policy"].clone();
    policy["lease"]["ttl_seconds"] = serde_json::json!(60);
    let (code, error) = call(
        &app,
        "PUT",
        "/api/v1/sessions/policy",
        serde_json::json!({"revision":initial["revision"],"policy":policy}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(error["error"]["code"]
        .as_str()
        .unwrap()
        .starts_with("MACC-WEB-"));
    policy["lease"]["heartbeat_seconds"] = serde_json::json!(10);
    let body = serde_json::json!({"revision":initial["revision"],"policy":policy});
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/sessions/policy/preview",
            body.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    let (code, saved) = call(&app, "PUT", "/api/v1/sessions/policy", body.clone()).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(saved["policy_source"], "project");
    let (code, conflict) = call(&app, "PUT", "/api/v1/sessions/policy", body).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "MACC-WEB-3004");
    let (code, reset) = call(
        &app,
        "PUT",
        "/api/v1/sessions/policy",
        serde_json::json!({"revision":saved["revision"],"policy":null}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(reset["policy_source"], "defaults");
}
#[tokio::test]
async fn session_authority_api_requires_reviewed_source_and_explicit_close() {
    let root = tempfile::tempdir().unwrap();
    write_test_config(root.path());
    let app = build_web_router(test_web_state(
        root.path(),
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    ));
    let (code, plan) = call(
        &app,
        "GET",
        "/api/v1/sessions/authority/migrate",
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(plan["applied"], false);
    let (code, _) = call(
        &app,
        "POST",
        "/api/v1/sessions/authority/migrate",
        serde_json::json!({"apply":true,"source_sha256":"stale"}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, result) = call(
        &app,
        "POST",
        "/api/v1/sessions/authority/migrate",
        serde_json::json!({"apply":true,"source_sha256":plan["source_sha256"]}),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(result["applied"], true);
    let (code, _) = call(
        &app,
        "POST",
        "/api/v1/sessions/action",
        serde_json::json!({"action":"close","id":"missing","confirm":false}),
    )
    .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn session_mutations_reject_a_viewer_while_readers_can_inspect() {
    use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
    let root = tempfile::tempdir().unwrap();
    write_test_config(root.path());
    let handle = ProcessHandle {
        kind: ProcessKind::Project,
        project_root: root.path().to_path_buf(),
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
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/sessions/store",
            serde_json::Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/sessions/authority/migrate")
                .header("content-type", "application/json")
                .header("X-Macc-Client-Id", "viewer")
                .body(Body::from(r#"{"apply":true,"source_sha256":"unused"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(!root.path().join(".macc/memory.db").exists());
}
