#[tokio::test]
async fn root_serves_spa_index() {
    let root = temp_root("root");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .expect("content type"),
        "text/html"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload = String::from_utf8(bytes.to_vec()).expect("utf8");
    assert!(payload.contains("<!doctype html") || payload.contains("<!DOCTYPE html"));
}

#[tokio::test]
async fn client_side_route_serves_spa_index() {
    let root = temp_root("spa-route");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/runs/active")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload = String::from_utf8(bytes.to_vec()).expect("utf8");
    assert!(payload.contains("<!doctype html") || payload.contains("<!DOCTYPE html"));
}

#[tokio::test]
async fn static_asset_request_serves_embedded_asset() {
    let root = temp_root("asset");
    let asset_path = assets::web_asset_paths()
        .find(|path| path.starts_with("assets/"))
        .expect("embedded asset path");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/{}", asset_path))
                .method(axum::http::Method::GET.as_str())
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert!(response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .is_some());
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    assert!(!bytes.is_empty());
}

#[tokio::test]
async fn dist_asset_mode_serves_files_from_disk() {
    let root = temp_root("dist-asset");
    write_test_dist_assets(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Dist,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/assets/app.js")
                .method(axum::http::Method::GET.as_str())
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert!(response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .is_some());
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    assert_eq!(
        String::from_utf8(bytes.to_vec()).expect("utf8"),
        "console.log('dist');"
    );
}

#[tokio::test]
async fn dist_asset_mode_falls_back_to_dist_index_for_client_routes() {
    let root = temp_root("dist-spa");
    write_test_dist_assets(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Dist,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/runs/active")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload = String::from_utf8(bytes.to_vec()).expect("utf8");
    assert!(payload.contains("dist web"));
}

#[tokio::test]
async fn health_endpoint_returns_ok_status() {
    let root = temp_root("health");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["status"], "ok");
}

#[tokio::test]
async fn status_endpoint_returns_status_payload() {
    let root = temp_root("ok");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    for key in [
        "total",
        "todo",
        "active",
        "blocked",
        "merged",
        "paused",
        "pause_reason",
        "pause_task_id",
        "pause_phase",
        "latest_error",
        "failure_report",
    ] {
        assert!(payload.get(key).is_some(), "missing {}", key);
    }
}

#[tokio::test]
async fn status_endpoint_maps_engine_errors() {
    let root = temp_root("error");
    let registry_path = root
        .join(".macc")
        .join("automation")
        .join("task")
        .join("task_registry.json");
    fs::create_dir_all(registry_path.parent().expect("parent")).expect("mkdir");
    fs::write(&registry_path, "{not-json").expect("write");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/status")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(
        response.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_STORAGE);
    assert_eq!(payload["error"]["category"], "Internal");
    assert!(payload["error"]["message"].is_string());
    assert_eq!(payload["error"]["context"]["backend"], "json");
}

#[tokio::test]
async fn events_endpoint_streams_coordinator_events() {
    let root = temp_root("events-stream");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_event_snapshots(vec![vec![
            coordinator_event(42, "evt-42", "task_transition"),
        ]])),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/events")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .expect("content type"),
        "text/event-stream"
    );

    let mut body = response.into_body();
    let first_frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .expect("frame timeout")
        .expect("frame")
        .expect("body frame");
    let chunk = first_frame.into_data().expect("data frame");
    let text = String::from_utf8(chunk.to_vec()).expect("utf8");

    assert!(text.contains("event: coordinator_event"), "{text}");
    assert!(text.contains("id: evt-42"), "{text}");
    assert!(text.contains("\"type\":\"task_transition\""), "{text}");
}

#[tokio::test]
async fn events_endpoint_respects_last_event_id_cursor() {
    let root = temp_root("events-cursor");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_event_snapshots(vec![vec![
            coordinator_event(41, "evt-41", "task_transition"),
            coordinator_event(42, "evt-42", "task_transition"),
        ]])),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/events")
                .method("GET")
                .header("Last-Event-ID", "evt-41")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    let mut body = response.into_body();
    let first_frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .expect("frame timeout")
        .expect("frame")
        .expect("body frame");
    let chunk = first_frame.into_data().expect("data frame");
    let text = String::from_utf8(chunk.to_vec()).expect("utf8");

    assert!(text.contains("id: evt-42"), "{text}");
    assert!(!text.contains("evt-41"), "{text}");
}

#[tokio::test]
async fn coordinator_event_stream_delivers_new_events_after_connect() {
    let root = temp_root("events-live");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_event_snapshots(vec![
            Vec::new(),
            vec![coordinator_event(43, "evt-43", "task_transition")],
        ])),
        WebAssetsMode::Embedded,
    );
    let response = axum::response::sse::Sse::new(sse::coordinator_event_stream(
        state,
        Vec::new(),
        None,
        Duration::from_millis(10),
        Duration::from_millis(100),
    ))
    .into_response();
    let mut body = response.into_body();

    let frame = tokio::time::timeout(Duration::from_millis(100), body.frame())
        .await
        .expect("live event timeout")
        .expect("frame")
        .expect("body frame");
    let data = String::from_utf8(frame.into_data().expect("data frame").to_vec()).expect("utf8");

    assert!(data.contains("event: coordinator_event"), "{data}");
    assert!(data.contains("id: evt-43"), "{data}");
}

#[tokio::test]
async fn coordinator_event_stream_emits_heartbeat_records() {
    let root = temp_root("events-heartbeat");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_event_snapshots(vec![Vec::new()])),
        WebAssetsMode::Embedded,
    );
    let response = axum::response::sse::Sse::new(sse::coordinator_event_stream(
        state,
        Vec::new(),
        None,
        Duration::from_millis(50),
        Duration::from_millis(10),
    ))
    .into_response();
    let mut body = response.into_body();

    let frame = tokio::time::timeout(Duration::from_millis(100), body.frame())
        .await
        .expect("heartbeat timeout")
        .expect("frame")
        .expect("body frame");
    let data = String::from_utf8(frame.into_data().expect("data frame").to_vec()).expect("utf8");

    assert!(data.contains("event: heartbeat"), "{data}");
    assert!(data.contains("\"type\":\"heartbeat\""), "{data}");
    assert!(data.contains("\"status\":\"ok\""), "{data}");
}

#[test]
fn server_config_defaults_to_localhost_and_configured_port() {
    let root = temp_root("bind-default");
    fs::create_dir_all(&root).expect("create root");
    write_test_config_with_port(&root, 4567);
    let command = WebCommand::new(
        AppContext::new(
            root.clone(),
            Arc::new(TestEngine::with_fixtures()),
            CliOverrides::default(),
        ),
        Ipv4Addr::LOCALHOST.to_string(),
        None,
        None,
    );

    let config = command.server_config().expect("server config");

    assert_eq!(
        config,
        WebServerConfig {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 4567,
            assets_mode: WebAssetsMode::Dist,
        }
    );
    assert_eq!(
        config.bind_addr(),
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4567)
    );
}

#[test]
fn server_config_uses_cli_overrides() {
    let root = temp_root("bind-override");
    fs::create_dir_all(&root).expect("create root");
    write_test_config_with_port(&root, 4567);
    write_test_config_with_assets_mode(&root, WebAssetsMode::Dist);
    let command = WebCommand::new(
        AppContext::new(
            root.clone(),
            Arc::new(TestEngine::with_fixtures()),
            CliOverrides::default(),
        ),
        "0.0.0.0".to_string(),
        Some(8080),
        Some(WebAssetsMode::Embedded),
    );

    let config = command.server_config().expect("server config");

    assert_eq!(
        config,
        WebServerConfig {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 8080,
            assets_mode: WebAssetsMode::Embedded,
        }
    );
}

#[test]
fn server_config_assets_mode_defaults_to_dist_in_dev() {
    let root = temp_root("assets-default");
    fs::create_dir_all(&root).expect("create root");
    write_test_config(&root);
    let command = WebCommand::new(
        AppContext::new(
            root.clone(),
            Arc::new(TestEngine::with_fixtures()),
            CliOverrides::default(),
        ),
        Ipv4Addr::LOCALHOST.to_string(),
        None,
        None,
    );

    assert_eq!(
        command.server_config().expect("server config").assets_mode,
        WebAssetsMode::Dist
    );
}

#[test]
fn server_config_assets_mode_uses_config_when_cli_flag_is_absent() {
    let root = temp_root("assets-config");
    fs::create_dir_all(&root).expect("create root");
    write_test_config_with_assets_mode(&root, WebAssetsMode::Embedded);
    let command = WebCommand::new(
        AppContext::new(
            root.clone(),
            Arc::new(TestEngine::with_fixtures()),
            CliOverrides::default(),
        ),
        Ipv4Addr::LOCALHOST.to_string(),
        None,
        None,
    );

    assert_eq!(
        command.server_config().expect("server config").assets_mode,
        WebAssetsMode::Embedded
    );
}

#[test]
fn server_config_assets_mode_cli_flag_overrides_config() {
    let root = temp_root("assets-cli");
    fs::create_dir_all(&root).expect("create root");
    write_test_config_with_assets_mode(&root, WebAssetsMode::Dist);
    let command = WebCommand::new(
        AppContext::new(
            root.clone(),
            Arc::new(TestEngine::with_fixtures()),
            CliOverrides::default(),
        ),
        Ipv4Addr::LOCALHOST.to_string(),
        None,
        Some(WebAssetsMode::Embedded),
    );

    assert_eq!(
        command.server_config().expect("server config").assets_mode,
        WebAssetsMode::Embedded
    );
}

#[tokio::test]
async fn coordinator_run_endpoint_returns_envelope() {
    let root = temp_root("run-ok");
    fs::create_dir_all(&root).expect("create root");
    let result = CoordinatorCommandResult {
        status: Some(CoordinatorStatus {
            total: 5,
            todo: 2,
            active: 1,
            blocked: 1,
            merged: 1,
            paused: false,
            pause_reason: None,
            pause_task_id: None,
            pause_phase: None,
            latest_error: None,
            failure_report: None,
            throttled_tools: vec![],
            effective_max_parallel: None,
        }),
        resumed: Some(false),
        aggregated_performer_logs: Some(2),
        runtime_status: Some("running".to_string()),
        exported_events_path: Some(std::path::PathBuf::from(
            ".macc/log/coordinator/events.jsonl",
        )),
        removed_worktrees: Some(0),
        selected_task: Some(SelectedTask {
            id: "TASK-1".to_string(),
            title: "Example".to_string(),
            tool: "mock".to_string(),
            base_branch: "main".to_string(),
            is_fallback: false,
        }),
        audit_prd_report: None,
    };
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Ok(result))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/run")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    for key in [
        "status",
        "resumed",
        "aggregated_performer_logs",
        "runtime_status",
        "exported_events_path",
        "removed_worktrees",
        "selected_task",
    ] {
        assert!(payload.get(key).is_some(), "missing {}", key);
    }
}

#[tokio::test]
async fn coordinator_run_endpoint_maps_errors() {
    let root = temp_root("run-error");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Err(MaccError::Validation(
            "coordinator failed".to_string(),
        )))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/run")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
    assert_eq!(payload["error"]["message"], "coordinator failed");
    assert_eq!(payload["error"]["retryable"], false);
    assert!(payload["error"]["recommended_action"].is_string());
    assert!(payload["error"].get("cause").is_none());
}

#[tokio::test]
async fn coordinator_run_endpoint_maps_internal_coordinator_error_code() {
    let root = temp_root("run-coordinator-error");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Err(MaccError::Coordinator {
            code: "E901",
            message: "coordinator crashed".to_string(),
        }))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/run")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_COORDINATOR);
    assert_eq!(payload["error"]["category"], "Internal");
    assert_eq!(payload["error"]["retryable"], true);
    assert!(payload["error"]["recommended_action"].is_string());
    assert_eq!(payload["error"]["context"]["code"], "E901");
}

#[tokio::test]
async fn coordinator_dispatch_endpoint_returns_envelope() {
    let root = temp_root("dispatch-ok");
    fs::create_dir_all(&root).expect("create root");
    write_test_config(&root);
    let result = CoordinatorCommandResult {
        status: Some(CoordinatorStatus {
            total: 3,
            todo: 1,
            active: 1,
            blocked: 0,
            merged: 1,
            paused: false,
            pause_reason: None,
            pause_task_id: None,
            pause_phase: None,
            latest_error: None,
            failure_report: None,
            throttled_tools: vec![],
            effective_max_parallel: None,
        }),
        resumed: Some(false),
        aggregated_performer_logs: Some(0),
        runtime_status: Some("running".to_string()),
        exported_events_path: None,
        removed_worktrees: Some(0),
        selected_task: Some(SelectedTask {
            id: "TASK-2".to_string(),
            title: "Dispatch".to_string(),
            tool: "mock".to_string(),
            base_branch: "main".to_string(),
            is_fallback: false,
        }),
        audit_prd_report: None,
    };
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Ok(result))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/dispatch")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    for key in [
        "status",
        "resumed",
        "aggregated_performer_logs",
        "runtime_status",
        "exported_events_path",
        "removed_worktrees",
        "selected_task",
    ] {
        assert!(payload.get(key).is_some(), "missing {}", key);
    }
}

#[tokio::test]
async fn coordinator_dispatch_endpoint_maps_errors() {
    let root = temp_root("dispatch-error");
    fs::create_dir_all(&root).expect("create root");
    write_test_config(&root);
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Err(MaccError::Validation(
            "dispatch failed".to_string(),
        )))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/dispatch")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
    assert_eq!(payload["error"]["message"], "dispatch failed");
}

#[tokio::test]
async fn coordinator_stop_endpoint_returns_envelope() {
    let root = temp_root("stop-ok");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_stop_result(Ok(()))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/stop")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    for key in [
        "status",
        "resumed",
        "aggregated_performer_logs",
        "runtime_status",
        "exported_events_path",
        "removed_worktrees",
        "selected_task",
    ] {
        assert!(payload.get(key).is_some(), "missing {}", key);
    }
}

#[tokio::test]
async fn coordinator_stop_endpoint_maps_errors() {
    let root = temp_root("stop-error");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_stop_result(Err(MaccError::Validation(
            "stop failed".to_string(),
        )))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/stop")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
}

#[tokio::test]
async fn coordinator_cleanup_endpoint_returns_envelope() {
    let root = temp_root("cleanup-ok");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_cleanup_result(Ok(()))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/cleanup")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    for key in [
        "status",
        "resumed",
        "aggregated_performer_logs",
        "runtime_status",
        "exported_events_path",
        "removed_worktrees",
        "selected_task",
    ] {
        assert!(payload.get(key).is_some(), "missing {}", key);
    }
}

#[tokio::test]
async fn coordinator_cleanup_endpoint_maps_errors() {
    let root = temp_root("cleanup-error");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_cleanup_result(Err(MaccError::Validation(
            "cleanup failed".to_string(),
        )))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/cleanup")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
    assert_eq!(payload["error"]["message"], "cleanup failed");
}

#[tokio::test]
async fn coordinator_advance_endpoint_returns_envelope() {
    let root = temp_root("advance-ok");
    fs::create_dir_all(&root).expect("create root");
    let result = CoordinatorCommandResult {
        status: Some(CoordinatorStatus {
            total: 3,
            todo: 1,
            active: 1,
            blocked: 0,
            merged: 1,
            paused: false,
            pause_reason: None,
            pause_task_id: None,
            pause_phase: None,
            latest_error: None,
            failure_report: None,
            throttled_tools: vec![],
            effective_max_parallel: None,
        }),
        aggregated_performer_logs: Some(1),
        runtime_status: Some("running".to_string()),
        removed_worktrees: Some(0),
        ..CoordinatorCommandResult::default()
    };
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Ok(result))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/advance")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    for key in [
        "status",
        "resumed",
        "aggregated_performer_logs",
        "runtime_status",
        "exported_events_path",
        "removed_worktrees",
        "selected_task",
    ] {
        assert!(payload.get(key).is_some(), "missing {}", key);
    }
}

#[tokio::test]
async fn coordinator_advance_endpoint_maps_errors() {
    let root = temp_root("advance-error");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Err(MaccError::Validation(
            "advance failed".to_string(),
        )))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/advance")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
    assert_eq!(payload["error"]["message"], "advance failed");
    assert!(payload["error"].get("cause").is_none());
}

#[tokio::test]
async fn coordinator_reconcile_endpoint_returns_envelope() {
    let root = temp_root("reconcile-ok");
    fs::create_dir_all(&root).expect("create root");
    let result = CoordinatorCommandResult {
        runtime_status: Some("running".to_string()),
        removed_worktrees: Some(1),
        ..CoordinatorCommandResult::default()
    };
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Ok(result))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/reconcile")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    for key in [
        "status",
        "resumed",
        "aggregated_performer_logs",
        "runtime_status",
        "exported_events_path",
        "removed_worktrees",
        "selected_task",
    ] {
        assert!(payload.get(key).is_some(), "missing {}", key);
    }
    assert_eq!(payload["runtime_status"], "running");
    assert_eq!(payload["removed_worktrees"], 1);
}

#[tokio::test]
async fn coordinator_reconcile_endpoint_maps_errors() {
    let root = temp_root("reconcile-error");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::new(Err(MaccError::Validation(
            "reconcile failed".to_string(),
        )))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/reconcile")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
    assert_eq!(payload["error"]["message"], "reconcile failed");
    assert!(payload["error"].get("cause").is_none());
}

#[tokio::test]
async fn coordinator_resume_endpoint_returns_envelope_with_resumed_true() {
    let root = temp_root("resume-ok");
    fs::create_dir_all(&root).expect("create root");
    macc_core::coordinator::state_runtime::write_coordinator_pause_file(
        &root,
        "global",
        "dev",
        "paused for test",
    )
    .expect("write pause file");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_resume_result(Ok(()))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/resume")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    for key in [
        "status",
        "resumed",
        "aggregated_performer_logs",
        "runtime_status",
        "exported_events_path",
        "removed_worktrees",
        "selected_task",
    ] {
        assert!(payload.get(key).is_some(), "missing {}", key);
    }
    assert_eq!(payload["resumed"], true);
}

#[tokio::test]
async fn coordinator_resume_endpoint_returns_envelope_with_resumed_false_when_not_paused() {
    let root = temp_root("resume-noop");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_resume_result(Ok(()))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/resume")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["resumed"], false);
}

#[tokio::test]
async fn coordinator_resume_endpoint_maps_errors() {
    let root = temp_root("resume-error");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_resume_result(Err(MaccError::Validation(
            "resume failed".to_string(),
        )))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/coordinator/resume")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
    assert_eq!(payload["error"]["message"], "resume failed");
}

#[tokio::test]
async fn prd_endpoint_read_project_prd() {
    let root = temp_root("prd-read-project");
    fs::create_dir_all(&root).expect("create root");
    let prd_content = serde_json::json!({
        "tasks": [
            { "id": "TASK-1", "title": "First task" }
        ],
        "lot": "Some Lot"
    });
    fs::write(root.join("prd.json"), serde_json::to_string(&prd_content).unwrap()).unwrap();

    let state = test_web_state(&root, Arc::new(WebTestEngine::new(Ok(CoordinatorCommandResult::default()))), WebAssetsMode::Embedded);
    let app = build_web_router(state);
    
    let response = app.oneshot(Request::builder().uri("/api/v1/prd").method("GET").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["tasks"][0]["id"], "TASK-1");
    assert_eq!(payload["metadata"]["lot"], "Some Lot");
}

#[tokio::test]
async fn prd_endpoint_read_worktree_prd() {
    let root = temp_root("prd-read-worktree");
    fs::create_dir_all(root.join("worker-01")).expect("create root");
    let prd_content = serde_json::json!({
        "tasks": [
            { "id": "WT-1", "title": "WT task" }
        ]
    });
    fs::write(root.join("worker-01").join("worktree.prd.json"), serde_json::to_string(&prd_content).unwrap()).unwrap();

    let state = test_web_state(&root, Arc::new(WebTestEngine::new(Ok(CoordinatorCommandResult::default()))), WebAssetsMode::Embedded);
    let app = build_web_router(state);
    
    let response = app.oneshot(Request::builder().uri("/api/v1/prd?path=worker-01").method("GET").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["tasks"][0]["id"], "WT-1");
}

#[tokio::test]
async fn prd_endpoint_write_prd_with_backup() {
    let root = temp_root("prd-write");
    fs::create_dir_all(&root).expect("create root");
    let prd_content = serde_json::json!({
        "tasks": [],
        "lot": "Old Lot"
    });
    fs::write(root.join("prd.json"), serde_json::to_string(&prd_content).unwrap()).unwrap();

    let state = test_web_state(&root, Arc::new(WebTestEngine::new(Ok(CoordinatorCommandResult::default()))), WebAssetsMode::Embedded);
    let app = build_web_router(state);
    
    let update_payload = serde_json::json!({
        "tasks": [
            { "id": "NEW-1", "title": "New task" }
        ],
        "metadata": {
            "lot": "New Lot"
        }
    });

    let response = app.oneshot(Request::builder().uri("/api/v1/prd").method("PUT").header("content-type", "application/json").body(Body::from(serde_json::to_vec(&update_payload).unwrap())).unwrap()).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let backups_dir = root.join(".macc").join("backups");
    let mut backup_found = false;
    for entry in fs::read_dir(&backups_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap().to_string_lossy().starts_with("prd.json.") {
            backup_found = true;
            let backup_content = fs::read_to_string(path).unwrap();
            assert!(backup_content.contains("Old Lot"));
            break;
        }
    }
    assert!(backup_found);

    let new_file_content: serde_json::Value = serde_json::from_str(&fs::read_to_string(root.join("prd.json")).unwrap()).unwrap();
    assert_eq!(new_file_content["tasks"][0]["id"], "NEW-1");
    assert_eq!(new_file_content["lot"], "New Lot");
}

#[tokio::test]
async fn prd_endpoint_write_validation_error() {
    let root = temp_root("prd-write-err");
    fs::create_dir_all(&root).expect("create root");

    let state = test_web_state(&root, Arc::new(WebTestEngine::new(Ok(CoordinatorCommandResult::default()))), WebAssetsMode::Embedded);
    let app = build_web_router(state);
    
    let update_payload = serde_json::json!({
        "tasks": [
            { "id": "DUP-1" },
            { "id": "DUP-1" }
        ],
        "metadata": {}
    });

    let response = app.oneshot(Request::builder().uri("/api/v1/prd").method("PUT").header("content-type", "application/json").body(Body::from(serde_json::to_vec(&update_payload).unwrap())).unwrap()).await.unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}
