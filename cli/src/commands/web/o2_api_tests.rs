use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;
#[tokio::test]
async fn o2_policy_has_shared_catalog_and_mutations_require_the_project_owner() {
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
                .uri("/api/v1/o2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let view: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        view["fields"].as_array().unwrap().len(),
        macc_core::o2::policy::fields().len()
    );
    let payload = serde_json::json!({"action":"policy_apply","revision":view["revision"],"policy":view["policy"]});
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/o2/action")
                .header("X-Macc-Client-Id", "viewer")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    for payload in [
        serde_json::json!({"action":"policy_field_get","path":"memory.enabled"}),
        serde_json::json!({"action":"policy_field_preview","path":"memory.enabled","value":"true","revision":null}),
        serde_json::json!({"action":"policy_field_apply","path":"memory.enabled","value":"true","revision":view["revision"],"confirmed":true}),
    ] {
        let expected = if payload["action"] == "policy_field_apply" {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::OK
        };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/o2/action")
                    .header("X-Macc-Client-Id", "viewer")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/o2/action")
                .body(Body::from("{broken"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let error: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(error["error"]["code"]
        .as_str()
        .unwrap()
        .starts_with("MACC-WEB-"));
}
