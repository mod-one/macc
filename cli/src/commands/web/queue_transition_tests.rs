use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;
#[tokio::test]
async fn queue_transition_api_requires_owner_confirmation_and_a_current_revision() {
    use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_test_config(root);
    let paths = ProjectPaths::from_root(root);
    let mut cfg = macc_core::load_canonical_config(&paths.config_path).unwrap();
    let coordinator = cfg
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    coordinator.prd_files = vec!["new.json".into()];
    coordinator.storage_mode = Some("json".into());
    macc_core::save_canonical_config(&paths, &cfg).unwrap();
    std::fs::write(root.join("new.json"), r#"{"tasks":[{"id":"NEW"}]}"#).unwrap();
    let handle = ProcessHandle {
        kind: ProcessKind::Project,
        project_root: root.into(),
        pid: None,
    };
    macc_core::service::process_ownership::register_process(root, handle.clone()).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    macc_core::service::process_ownership::claim_owner(
        root,
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
        root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    ));
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/prd-queue")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let view: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let request = |owner: &str, confirmed: bool, revision: &str| {
        Request::builder()
            .method("POST")
            .uri("/api/v1/coordinator/prd-queue/accept")
            .header("Content-Type", "application/json")
            .header("X-Macc-Client-Id", owner)
            .body(Body::from(
                serde_json::json!({"confirmed":confirmed,"revision":revision}).to_string(),
            ))
            .unwrap()
    };
    let revision = view["revision"].as_str().unwrap();
    assert_eq!(
        app.clone()
            .oneshot(request("viewer", true, revision))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert!(!app
        .clone()
        .oneshot(request("owner", false, revision))
        .await
        .unwrap()
        .status()
        .is_success());
    assert!(!app
        .clone()
        .oneshot(request("owner", true, "stale"))
        .await
        .unwrap()
        .status()
        .is_success());
    assert_eq!(
        app.oneshot(request("owner", true, revision))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}
