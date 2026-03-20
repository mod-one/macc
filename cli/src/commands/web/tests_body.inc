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
async fn doctor_endpoint_returns_empty_report_when_all_checks_pass() {
    let root = temp_root("doctor-report-empty");
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
                .uri("/api/v1/doctor")
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
    assert_eq!(payload["healthScore"], 100);
    assert!(
        payload["issues"].is_null()
            || payload["issues"] == serde_json::json!([])
    );
}

#[tokio::test]
async fn doctor_endpoint_returns_structured_issues() {
    let root = temp_root("doctor-report-issues");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_doctor_snapshots(
            vec![vec![
                doctor_check(
                    "Git",
                    None,
                    "git",
                    DoctorCheckKind::Which,
                    CheckSeverity::Error,
                    ToolStatus::Missing,
                ),
                doctor_check(
                    "Codex",
                    Some("codex"),
                    "/tmp/codex",
                    DoctorCheckKind::PathExists,
                    CheckSeverity::Warning,
                    ToolStatus::Missing,
                ),
            ]],
            Ok(()),
        )),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/doctor")
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
    let issues = payload["issues"].as_array().expect("issues");
    assert_eq!(payload["issuesBySeverity"]["error"], 1);
    assert_eq!(payload["issuesBySeverity"]["warning"], 1);
    assert_eq!(payload["issuesByCategory"]["tooling"], 1);
    assert_eq!(payload["issuesByCategory"]["filesystem"], 1);
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0]["code"], "doctor.git.which.git");
    assert_eq!(issues[0]["description"], "Git requires 'git' to be available in PATH.");
    assert_eq!(issues[0]["currentState"], "missing");
    assert_eq!(issues[0]["expectedState"], "binary is available in PATH");
    assert_eq!(issues[0]["fixAvailable"], true);
}

#[tokio::test]
async fn doctor_fix_endpoint_returns_success_when_safe_issue_is_resolved() {
    let root = temp_root("doctor-fix-success");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_doctor_snapshots(
            vec![
                vec![doctor_check(
                    "Git",
                    None,
                    "git",
                    DoctorCheckKind::Which,
                    CheckSeverity::Error,
                    ToolStatus::Missing,
                )],
                Vec::new(),
            ],
            Ok(()),
        )),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/doctor/fix")
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
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["attemptedCount"], 1);
    assert_eq!(payload["fixedCount"], 1);
    assert_eq!(payload["failedCount"], 0);
    assert!(
        payload["report"]["issues"].is_null()
            || payload["report"]["issues"] == serde_json::json!([])
    );
    assert_eq!(payload["results"][0]["code"], "doctor.git.which.git");
    assert_eq!(payload["results"][0]["fixed"], true);
}

#[tokio::test]
async fn doctor_fix_endpoint_rejects_selective_issue_targeting() {
    let root = temp_root("doctor-fix-selective");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_doctor_snapshots(
            vec![vec![doctor_check(
                "Git",
                None,
                "git",
                DoctorCheckKind::Which,
                CheckSeverity::Error,
                ToolStatus::Missing,
            )]],
            Ok(()),
        )),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/doctor/fix")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "issueCodes": ["doctor.git.which.git"]
                    }))
                    .expect("json body"),
                ))
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
    assert_eq!(
        payload["error"]["message"],
        "Selective doctor fixes are not supported; omit issueCodes to run all safe fixes."
    );
}

#[tokio::test]
async fn doctor_fix_endpoint_ignores_non_fixable_issues_in_bulk_mode() {
    let root = temp_root("doctor-fix-non-fixable");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_doctor_snapshots(
            vec![vec![doctor_check(
                "Custom Tool",
                Some("custom-tool"),
                "custom-check",
                DoctorCheckKind::Custom,
                CheckSeverity::Warning,
                ToolStatus::Error("unsupported".to_string()),
            )]],
            Ok(()),
        )),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/doctor/fix")
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
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["attemptedCount"], 0);
    assert_eq!(payload["fixedCount"], 0);
    assert_eq!(payload["failedCount"], 0);
    assert_eq!(
        payload["message"],
        "No doctor issues support automatic fixes."
    );
    assert!(
        payload["results"].is_null() || payload["results"] == serde_json::json!([])
    );
    assert_eq!(payload["report"]["issues"].as_array().expect("issues").len(), 1);
}

#[tokio::test]
async fn doctor_fix_endpoint_returns_partial_failure_when_issue_remains() {
    let root = temp_root("doctor-fix-partial");
    fs::create_dir_all(&root).expect("create root");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_doctor_snapshots(
            vec![
                vec![
                    doctor_check(
                        "Git",
                        None,
                        "git",
                        DoctorCheckKind::Which,
                        CheckSeverity::Error,
                        ToolStatus::Missing,
                    ),
                    doctor_check(
                        "Codex",
                        Some("codex"),
                        "/tmp/codex",
                        DoctorCheckKind::PathExists,
                        CheckSeverity::Warning,
                        ToolStatus::Missing,
                    ),
                ],
                vec![doctor_check(
                    "Codex",
                    Some("codex"),
                    "/tmp/codex",
                    DoctorCheckKind::PathExists,
                    CheckSeverity::Warning,
                    ToolStatus::Missing,
                )],
            ],
            Err(MaccError::Validation("Doctor checks failed.".to_string())),
        )),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/doctor/fix")
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
    assert_eq!(payload["status"], "partial");
    assert_eq!(payload["attemptedCount"], 2);
    assert_eq!(payload["fixedCount"], 1);
    assert_eq!(payload["failedCount"], 1);
    assert_eq!(payload["report"]["issues"].as_array().expect("issues").len(), 1);
    let failed_result = payload["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|result| result["fixed"] == serde_json::json!(false))
        .expect("failed result");
    assert_eq!(failed_result["code"], "doctor.codex.path_exists.tmp-codex");
}

#[tokio::test]
async fn logs_endpoint_lists_categorized_log_files() {
    let root = temp_root("logs-list");
    let coordinator_dir = root.join(".macc/log/coordinator");
    let performer_dir = root.join(".macc/log/performer");
    fs::create_dir_all(&coordinator_dir).expect("create coordinator dir");
    fs::create_dir_all(&performer_dir).expect("create performer dir");
    fs::write(coordinator_dir.join("events.jsonl"), "{\"type\":\"started\"}\n").expect("write coordinator log");
    fs::write(performer_dir.join("task.md"), "# performer\n").expect("write performer log");
    fs::write(root.join(".macc/log/ops.jsonl"), "{\"action\":\"noop\"}\n").expect("write ops log");

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs")
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
    let files = payload.as_array().expect("array");
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["path"], "coordinator/events.jsonl");
    assert_eq!(files[0]["category"], "coordinator");
    assert!(files[0]["size"].as_u64().expect("size") > 0);
    assert!(files[0]["modified"].as_str().is_some());
    assert_eq!(files[1]["path"], "performer/task.md");
    assert_eq!(files[1]["category"], "performer");
}

#[tokio::test]
async fn log_read_endpoint_returns_full_content() {
    let root = temp_root("logs-read");
    let coordinator_dir = root.join(".macc/log/coordinator");
    fs::create_dir_all(&coordinator_dir).expect("create coordinator dir");
    fs::write(coordinator_dir.join("events.jsonl"), "line-1\nline-2\nline-3\n").expect("write log");

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs/coordinator%2Fevents.jsonl")
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
    assert_eq!(payload["path"], "coordinator/events.jsonl");
    assert_eq!(payload["total"], 3);
    assert_eq!(payload["hasMore"], false);
    assert_eq!(
        payload["lines"],
        serde_json::json!(["line-1", "line-2", "line-3"])
    );
}

#[tokio::test]
async fn log_read_endpoint_supports_pagination() {
    let root = temp_root("logs-pagination");
    let coordinator_dir = root.join(".macc/log/coordinator");
    fs::create_dir_all(&coordinator_dir).expect("create coordinator dir");
    fs::write(
        coordinator_dir.join("events.jsonl"),
        "line-1\nline-2\nline-3\nline-4\n",
    )
    .expect("write log");

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs/coordinator%2Fevents.jsonl?offset=1&limit=2")
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
    assert_eq!(payload["total"], 4);
    assert_eq!(payload["hasMore"], true);
    assert_eq!(payload["lines"], serde_json::json!(["line-2", "line-3"]));
}

#[tokio::test]
async fn log_read_endpoint_supports_search_filtering() {
    let root = temp_root("logs-search");
    let performer_dir = root.join(".macc/log/performer");
    fs::create_dir_all(&performer_dir).expect("create performer dir");
    fs::write(
        performer_dir.join("task.md"),
        "INFO start\nERROR failed\nINFO retry\nERROR recovered\n",
    )
    .expect("write log");

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs/performer%2Ftask.md?search=ERROR")
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
    assert_eq!(payload["total"], 2);
    assert_eq!(payload["hasMore"], false);
    assert_eq!(
        payload["lines"],
        serde_json::json!(["ERROR failed", "ERROR recovered"])
    );
}

#[tokio::test]
async fn log_read_endpoint_rejects_traversal_paths() {
    let root = temp_root("logs-traversal");
    fs::create_dir_all(root.join(".macc/log/coordinator")).expect("create log dir");

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs/..%2F..%2Fetc%2Fpasswd")
                .method("GET")
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
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_LOG_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
}

#[cfg(unix)]
#[tokio::test]
async fn logs_endpoint_skips_symlinked_directories_outside_log_root() {
    let root = temp_root("logs-symlink-dir");
    let coordinator_dir = root.join(".macc/log/coordinator");
    let external_dir = root.join("external-logs");
    fs::create_dir_all(&coordinator_dir).expect("create coordinator dir");
    fs::create_dir_all(&external_dir).expect("create external dir");
    fs::write(external_dir.join("outside.log"), "outside\n").expect("write external log");
    std::os::unix::fs::symlink(&external_dir, coordinator_dir.join("linked"))
        .expect("create symlink");

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs")
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
    let files = payload.as_array().expect("array");
    assert!(files.is_empty());
}

#[tokio::test]
async fn audit_log_skips_get_requests() {
    let root = temp_root("audit-skip-get");
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
    assert!(read_ops_log_entries(&root).is_empty());
}

#[tokio::test]
async fn audit_log_appends_entry_for_post_request() {
    let root = temp_root("audit-post");
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
    let entries = read_ops_log_entries(&root);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["action"], "POST /api/v1/coordinator/stop");
    assert_eq!(entries[0]["result"]["status_code"], 200);
    assert!(entries[0]["duration_ms"].is_number());
}

#[tokio::test]
async fn audit_log_entry_includes_expected_fields_for_put_request() {
    let root = temp_root("audit-put");
    fs::create_dir_all(&root).expect("create root");
    write_test_config(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
                .method("PUT")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "webPort": 4567,
                        "quiet": true
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let entries = read_ops_log_entries(&root);
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["method"], "PUT");
    assert_eq!(entry["path"], "/api/v1/config");
    assert!(entry["timestamp"].as_str().is_some());
    assert!(entry["actor"].as_str().is_some());
    assert_eq!(entry["log_path"], ".macc/log/ops.jsonl");
    assert_eq!(entry["inputs_summary"]["content_type"], "application/json");
    assert_eq!(entry["inputs_summary"]["body"]["kind"], "json-object");
    assert_eq!(
        entry["inputs_summary"]["body"]["keys"],
        serde_json::json!(["quiet", "webPort"])
    );
}

#[tokio::test]
async fn plan_endpoint_returns_structured_preview_with_diffs() {
    let root = temp_root("plan-default");
    fs::create_dir_all(&root).expect("create root");
    let paths = ProjectPaths::from_root(&root);
    let engine = Arc::new(WebTestEngine::with_fixture_plan_from_overrides());
    let tool_id = engine.inner.fixture_ids()[0].to_string();
    write_test_config_with_tools(&root, vec![tool_id.clone()]);
    let state = test_web_state(&root, engine, WebAssetsMode::Embedded);
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/plan")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "includeDiff": true,
                        "explain": true
                    }))
                    .expect("json body"),
                ))
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
    let files = payload["files"].as_array().expect("files");
    let target = files
        .iter()
        .find(|item| item["path"] == format!("{tool_id}-output.txt"))
        .expect("target file");
    assert_eq!(payload["summary"]["backupPath"], paths.backups_dir.display().to_string());
    assert!(payload["summary"]["totalActions"].as_u64().expect("total actions") >= 1);
    assert!(!files.is_empty());
    assert_eq!(target["riskLevel"], "safe");
    assert!(target["contentPreview"]
        .as_str()
        .expect("content preview")
        .contains("fixture content for"));
    assert!(payload["diffs"]
        .as_array()
        .expect("diffs")
        .iter()
        .any(|item| item["path"] == format!("{tool_id}-output.txt")
            && item["diffKind"] == "text"
            && item["diff"]
                .as_str()
                .expect("diff")
                .contains(&format!("{tool_id}-output.txt"))));
    assert_eq!(payload["risks"][0]["level"], "safe");
}

#[tokio::test]
async fn plan_endpoint_filters_tools_and_accepts_offline_scope_fields() {
    let root = temp_root("plan-filtered");
    fs::create_dir_all(&root).expect("create root");
    let engine = Arc::new(WebTestEngine::with_fixture_plan_from_overrides());
    let tool_one = engine.inner.fixture_ids()[0].to_string();
    let tool_two = engine.inner.fixture_ids()[1].to_string();
    write_test_config_with_tools(&root, vec![tool_one.clone(), tool_two.clone()]);
    let state = test_web_state(&root, engine, WebAssetsMode::Embedded);
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/plan")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "tools": [tool_one],
                        "worktrees": ["wt-1"],
                        "offline": true
                    }))
                    .expect("json body"),
                ))
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
    let files = payload["files"].as_array().expect("files");
    assert!(files
        .iter()
        .any(|item| item["path"] == format!("{tool_one}-output.txt")));
    assert!(!files
        .iter()
        .any(|item| item["path"] == format!("{tool_two}-output.txt")));
}

#[tokio::test]
async fn plan_endpoint_maps_missing_config_to_api_error() {
    let root = temp_root("plan-missing-config");
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
                .uri("/api/v1/plan")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({})).expect("json body"),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_GATEWAY);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], "MACC-WEB-4000");
    assert_eq!(payload["error"]["category"], "Dependency");
}

#[tokio::test]
async fn plan_endpoint_rejects_user_scope_without_explicit_opt_in() {
    let root = temp_root("plan-user-scope-rejected");
    fs::create_dir_all(&root).expect("create root");
    write_test_config(&root);
    let mut builder = macc_core::plan::ActionPlan::builder(macc_core::plan::Scope::User);
    let plan = builder
        .write_text(".codex/config.toml", "model = \"gpt-5.4\"\n")
        .expect("user-scope write")
        .build();
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_plan_result(Ok(plan))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/plan")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({})).expect("json body"),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], "MACC-WEB-3000");
    assert_eq!(payload["error"]["category"], "Auth");
}

#[tokio::test]
async fn apply_endpoint_dry_run_returns_preview_without_writing() {
    let _guard = apply_test_guard();
    let root = temp_root("apply-dry-run");
    fs::create_dir_all(&root).expect("create root");
    let paths = ProjectPaths::from_root(&root);
    let engine = Arc::new(TestEngine::with_fixtures());
    let tool_id = engine.fixture_ids()[0].to_string();
    let output_path = root.join(format!("{tool_id}-output.txt"));
    write_test_config_with_tools(&root, vec![tool_id.clone()]);
    let state = test_web_state(&root, engine, WebAssetsMode::Embedded);
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/apply")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "dryRun": true,
                        "tools": [tool_id]
                    }))
                    .expect("json body"),
                ))
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
    assert!(payload.get("summary").is_some());
    assert!(payload["files"].as_array().expect("files").len() > 0);
    assert_eq!(payload["summary"]["backupPath"], paths.backups_dir.display().to_string());
    assert!(!output_path.exists());
}

#[tokio::test]
async fn apply_endpoint_rejects_missing_confirmation() {
    let _guard = apply_test_guard();
    let root = temp_root("apply-missing-confirm");
    fs::create_dir_all(&root).expect("create root");
    let engine = Arc::new(TestEngine::with_fixtures());
    let tool_id = engine.fixture_ids()[0].to_string();
    let output_path = root.join(format!("{tool_id}-output.txt"));
    write_test_config_with_tools(&root, vec![tool_id.clone()]);
    let state = test_web_state(&root, engine, WebAssetsMode::Embedded);
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/apply")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "dryRun": false,
                        "tools": [tool_id]
                    }))
                    .expect("json body"),
                ))
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
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_CONFIRMATION_REQUIRED);
    assert_eq!(payload["error"]["message"], "Confirmation required before apply can write changes.");
    assert!(!output_path.exists());
}

#[tokio::test]
async fn apply_endpoint_rejects_false_confirmation() {
    let _guard = apply_test_guard();
    let root = temp_root("apply-false-confirm");
    fs::create_dir_all(&root).expect("create root");
    let engine = Arc::new(TestEngine::with_fixtures());
    let tool_id = engine.fixture_ids()[0].to_string();
    let output_path = root.join(format!("{tool_id}-output.txt"));
    write_test_config_with_tools(&root, vec![tool_id.clone()]);
    let state = test_web_state(&root, engine, WebAssetsMode::Embedded);
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/apply")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "dryRun": false,
                        "confirmed": false,
                        "tools": [tool_id]
                    }))
                    .expect("json body"),
                ))
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
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_CONFIRMATION_REQUIRED);
    assert!(!output_path.exists());
}

#[tokio::test]
async fn apply_endpoint_with_confirmation_writes_and_returns_backup_location() {
    let _guard = apply_test_guard();
    let root = temp_root("apply-confirmed");
    fs::create_dir_all(&root).expect("create root");
    let engine = Arc::new(TestEngine::with_fixtures());
    let tool_id = engine.fixture_ids()[0].to_string();
    let output_name = format!("{tool_id}-output.txt");
    write_test_config_with_tools(&root, vec![tool_id.clone()]);
    fs::write(root.join(&output_name), "manual content\n").expect("seed file");
    let state = test_web_state(&root, engine, WebAssetsMode::Embedded);
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/apply")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "dryRun": false,
                        "confirmed": true,
                        "tools": [tool_id]
                    }))
                    .expect("json body"),
                ))
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
    assert_eq!(payload["dryRun"], false);
    assert!(payload["changedFiles"].as_u64().expect("changed files") > 0);
    let backup_locations = payload["backupLocations"].as_array().expect("backup locations");
    assert!(!backup_locations.is_empty());
    let backup_root = backup_locations[0].as_str().expect("backup root");
    assert!(std::path::Path::new(backup_root).join(&output_name).exists());
    assert_eq!(
        fs::read_to_string(std::path::Path::new(backup_root).join(&output_name))
            .expect("backup contents"),
        "manual content\n"
    );
    assert!(fs::read_to_string(root.join(&output_name))
        .expect("generated file")
        .contains("fixture content for"));
    assert!(payload["results"]
        .as_array()
        .expect("results")
        .iter()
        .any(|item| item["path"] == output_name));
}

#[tokio::test]
async fn apply_endpoint_maps_apply_errors() {
    let _guard = apply_test_guard();
    let root = temp_root("apply-error");
    let paths = ProjectPaths::from_root(&root);
    fs::create_dir_all(paths.config_path.parent().expect("config dir")).expect("mkdir config");
    fs::write(&paths.config_path, "{not-yaml").expect("write invalid config");
    let engine = Arc::new(TestEngine::with_fixtures());
    let tool_id = engine.fixture_ids()[0].to_string();
    let state = test_web_state(&root, engine, WebAssetsMode::Embedded);
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/apply")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "dryRun": false,
                        "confirmed": true,
                        "tools": [tool_id]
                    }))
                    .expect("json body"),
                ))
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
    assert_eq!(payload["error"]["code"], "MACC-WEB-1003");
    assert_eq!(payload["error"]["category"], "Validation");
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
async fn git_graph_endpoint_returns_commit_graph_payload() {
    let root = temp_root("git-graph");
    build_git_graph_repo(&root);
    write_test_config(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/git/graph")
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
    assert_eq!(payload["head"], "main");
    assert!(payload["branches"]
        .as_array()
        .expect("branches")
        .iter()
        .any(|branch| branch == "main"));
    assert!(payload["branches"]
        .as_array()
        .expect("branches")
        .iter()
        .any(|branch| branch == "feature/web-graph"));
    let commits = payload["commits"].as_array().expect("commits");
    assert_eq!(commits.len(), 4);
    assert_eq!(commits[0]["task_id"], "WEB2-BE-GIT-002");
    assert_eq!(commits[0]["branch_refs"], serde_json::json!(["main"]));
    assert_eq!(commits[0]["parent_shas"].as_array().expect("parents").len(), 2);
    assert_eq!(commits[1]["task_id"], "WEB2-BE-GIT-003");
    assert_eq!(commits[2]["task_id"], "WEB2-BE-GIT-002");
    assert_eq!(commits[2]["branch_refs"], serde_json::json!(["feature/web-graph"]));
    assert_eq!(commits[3]["task_id"], "WEB2-BE-GIT-001");
}

#[tokio::test]
async fn git_graph_endpoint_returns_empty_payload_for_empty_repo() {
    let root = temp_root("git-graph-empty");
    init_git_repo(&root);
    write_test_config(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/git/graph")
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
    assert_eq!(payload["head"], "main");
    assert_eq!(payload["branches"], serde_json::json!([]));
    assert_eq!(payload["commits"], serde_json::json!([]));
}

#[tokio::test]
async fn git_graph_endpoint_honors_limit_query() {
    let root = temp_root("git-graph-limit");
    build_git_graph_repo(&root);
    write_test_config(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/git/graph?limit=2")
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
    assert_eq!(payload["commits"].as_array().expect("commits").len(), 2);
}

#[tokio::test]
async fn git_graph_endpoint_paginates_after_since_sha() {
    let root = temp_root("git-graph-since");
    build_git_graph_repo(&root);
    write_test_config(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let first_page = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/git/graph?limit=2")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let first_page_bytes = first_page
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let first_page_payload: serde_json::Value =
        serde_json::from_slice(&first_page_bytes).expect("json");
    let first_page_commits = first_page_payload["commits"].as_array().expect("commits");
    let since_sha = first_page_commits[1]["sha"].as_str().expect("sha");

    let second_page = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/git/graph?limit=2&since={since_sha}"))
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(second_page.status(), axum::http::StatusCode::OK);
    let second_page_bytes = second_page
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let second_page_payload: serde_json::Value =
        serde_json::from_slice(&second_page_bytes).expect("json");
    let second_page_commits = second_page_payload["commits"].as_array().expect("commits");
    assert_eq!(second_page_commits.len(), 2);
    assert_eq!(second_page_commits[0]["task_id"], "WEB2-BE-GIT-002");
    assert_eq!(second_page_commits[1]["task_id"], "WEB2-BE-GIT-001");
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
async fn registry_tasks_endpoint_returns_registry_metadata() {
    let root = temp_root("registry-list");
    fs::create_dir_all(&root).expect("create root");

    let mut task = registry_task("WEB2-BE-REG-001", "Registry task", "blocked", Some("codex"));
    task.task_runtime.status = Some(RuntimeStatus::Failed.as_str().to_string());
    task.task_runtime.attempt = Some(2);
    task.task_runtime.last_heartbeat = Some("2026-03-20T12:00:00Z".to_string());
    task.task_runtime.delayed_until = Some("2026-03-20T13:00:00Z".to_string());
    task.task_runtime.current_phase = Some("review".to_string());
    task.task_runtime.last_error = Some("tool failed".to_string());
    task.task_runtime.last_error_code = Some("E901".to_string());
    task.updated_at = Some("2026-03-20T12:05:00Z".to_string());
    task.worktree = Some(TaskWorktree {
        worktree_path: Some("/tmp/macc-wt".to_string()),
        branch: Some("feature/registry".to_string()),
        base_branch: Some("main".to_string()),
        last_commit: Some("abc123".to_string()),
        session_id: Some("session-1".to_string()),
        ..TaskWorktree::default()
    });

    write_registry_snapshot(
        &root,
        &CoordinatorSnapshot {
            registry: TaskRegistry {
                tasks: vec![task],
                updated_at: Some("2026-03-20T12:05:00Z".to_string()),
                ..TaskRegistry::default()
            },
            events: vec![registry_event(
                "WEB2-BE-REG-001",
                "evt-reg-1",
                "task_blocked",
            )],
            cursor: None,
        },
    );

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/registry/tasks")
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
    let tasks = payload.as_array().expect("tasks");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0]["id"], "WEB2-BE-REG-001");
    assert_eq!(tasks[0]["title"], "Registry task");
    assert_eq!(tasks[0]["priority"], "P1");
    assert_eq!(tasks[0]["state"], "blocked");
    assert_eq!(tasks[0]["tool"], "codex");
    assert_eq!(tasks[0]["attempts"], 2);
    assert_eq!(tasks[0]["heartbeat"], "2026-03-20T12:00:00Z");
    assert_eq!(tasks[0]["delayedUntil"], "2026-03-20T13:00:00Z");
    assert_eq!(tasks[0]["lastErrorCode"], "E901");
    assert_eq!(tasks[0]["worktree"]["branch"], "feature/registry");
    assert_eq!(tasks[0]["events"][0]["eventType"], "task_blocked");
}

#[tokio::test]
async fn registry_task_action_requeues_failed_task() {
    let root = temp_root("registry-requeue");
    fs::create_dir_all(&root).expect("create root");

    let mut task = registry_task("WEB2-BE-REG-002", "Blocked task", "blocked", Some("codex"));
    task.assignee = Some(serde_json::json!({ "tool": "codex" }));
    task.claimed_at = Some("2026-03-20T11:00:00Z".to_string());
    task.worktree = Some(TaskWorktree {
        worktree_path: Some("/tmp/requeue".to_string()),
        ..TaskWorktree::default()
    });
    task.task_runtime.status = Some(RuntimeStatus::Failed.as_str().to_string());
    task.task_runtime.current_phase = Some("review".to_string());
    task.task_runtime.last_error = Some("boom".to_string());
    task.task_runtime.last_error_code = Some("E901".to_string());
    task.task_runtime.attempt = Some(3);
    task.task_runtime.last_heartbeat = Some("2026-03-20T11:59:00Z".to_string());
    task.task_runtime.delayed_until = Some("2026-03-20T12:30:00Z".to_string());

    write_registry_snapshot(
        &root,
        &CoordinatorSnapshot {
            registry: TaskRegistry {
                tasks: vec![task],
                ..TaskRegistry::default()
            },
            events: Vec::new(),
            cursor: None,
        },
    );

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/registry/tasks/WEB2-BE-REG-002/requeue")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "kind": "requeue",
                        "justification": "operator retry"
                    }))
                    .expect("json body"),
                ))
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
    assert_eq!(payload["state"], "todo");
    assert_eq!(payload["tool"], "codex");
    assert!(payload["assignee"].is_null());
    assert!(payload["worktree"].is_null());
    assert!(payload["attempts"].is_null());
    assert!(payload["heartbeat"].is_null());
    assert!(payload["delayedUntil"].is_null());
    assert!(payload["lastError"].is_null());
    assert!(payload["lastErrorCode"].is_null());

    let project_paths = ProjectPaths::from_root(&root);
    let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
    let snapshot = SqliteStorage::new(storage_paths)
        .load_snapshot()
        .expect("load snapshot");
    let stored = snapshot
        .registry
        .find_task("WEB2-BE-REG-002")
        .expect("stored task");
    assert_eq!(stored.state, "todo");
    assert!(stored.assignee.is_none());
    assert!(stored.worktree.is_none());
    assert_eq!(
        stored.task_runtime.status.as_deref(),
        Some(RuntimeStatus::Idle.as_str())
    );
}

#[tokio::test]
async fn registry_task_action_reassigns_tool() {
    let root = temp_root("registry-reassign");
    fs::create_dir_all(&root).expect("create root");

    let mut task = registry_task("WEB2-BE-REG-003", "Todo task", "blocked", Some("codex"));
    task.assignee = Some(serde_json::json!({ "tool": "codex" }));
    task.claimed_at = Some("2026-03-20T11:00:00Z".to_string());
    task.worktree = Some(TaskWorktree {
        worktree_path: Some("/tmp/reassign".to_string()),
        branch: Some("feature/reassign".to_string()),
        ..TaskWorktree::default()
    });
    write_registry_snapshot(
        &root,
        &CoordinatorSnapshot {
            registry: TaskRegistry {
                tasks: vec![task],
                ..TaskRegistry::default()
            },
            events: Vec::new(),
            cursor: None,
        },
    );

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/registry/tasks/WEB2-BE-REG-003/reassign")
                .method("POST")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "kind": "reassign",
                        "tool": "gemini",
                        "justification": "specialized follow-up"
                    }))
                    .expect("json body"),
                ))
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
    assert_eq!(payload["tool"], "gemini");
    assert_eq!(payload["state"], "blocked");
    assert!(payload["assignee"].is_null());
    assert!(payload["worktree"].is_null());

    let project_paths = ProjectPaths::from_root(&root);
    let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
    let snapshot = SqliteStorage::new(storage_paths)
        .load_snapshot()
        .expect("load snapshot");
    let stored = snapshot
        .registry
        .find_task("WEB2-BE-REG-003")
        .expect("stored task");
    assert_eq!(stored.tool.as_deref(), Some("gemini"));
    assert!(stored.assignee.is_none());
    assert!(stored.worktree.is_none());
}

#[tokio::test]
async fn registry_task_action_rejects_invalid_action() {
    let root = temp_root("registry-invalid-action");
    fs::create_dir_all(&root).expect("create root");

    let task = registry_task("WEB2-BE-REG-004", "Todo task", "todo", Some("codex"));
    write_registry_snapshot(
        &root,
        &CoordinatorSnapshot {
            registry: TaskRegistry {
                tasks: vec![task],
                ..TaskRegistry::default()
            },
            events: Vec::new(),
            cursor: None,
        },
    );

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/registry/tasks/WEB2-BE-REG-004/pause")
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
    assert_eq!(payload["error"]["code"], errors::WEB_ERR_REGISTRY_VALIDATION);
    assert_eq!(payload["error"]["category"], "Validation");
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

#[tokio::test]
async fn config_get_endpoint_returns_canonical_config() {
    let root = temp_root("config-get");
    fs::create_dir_all(&root).expect("create root");
    write_test_config_with_port(&root, 4567);
    let state = test_web_state(&root, Arc::new(TestEngine::with_fixtures()), WebAssetsMode::Embedded);
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
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
    assert_eq!(payload["webPort"], 4567);
    assert_eq!(payload["quiet"], false);
    assert_eq!(payload["offline"], false);
}

#[tokio::test]
async fn config_put_endpoint_updates_config_file() {
    let root = temp_root("config-put");
    fs::create_dir_all(&root).expect("create root");
    write_test_config(&root);
    let state = test_web_state(&root, Arc::new(TestEngine::with_fixtures()), WebAssetsMode::Embedded);
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
                .method("PUT")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "webPort": 4567,
                        "quiet": true,
                        "selectedSkills": ["macc-performer"]
                    })
                    .to_string(),
                ))
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
    assert_eq!(payload["webPort"], 4567);
    assert_eq!(payload["quiet"], true);
    assert_eq!(payload["selectedSkills"], serde_json::json!(["macc-performer"]));

    let paths = ProjectPaths::from_root(&root);
    let updated = macc_core::load_canonical_config(&paths.config_path).expect("load config");
    assert_eq!(updated.settings.web_port, Some(4567));
    assert!(updated.settings.quiet);
    assert_eq!(
        updated
            .selections
            .as_ref()
            .expect("selections")
            .skills,
        vec!["macc-performer".to_string()]
    );
}

#[tokio::test]
async fn config_put_endpoint_returns_validation_error_for_invalid_json() {
    let root = temp_root("config-put-invalid");
    fs::create_dir_all(&root).expect("create root");
    write_test_config(&root);
    let state = test_web_state(&root, Arc::new(TestEngine::with_fixtures()), WebAssetsMode::Embedded);
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
                .method("PUT")
                .header("content-type", "application/json")
                .body(Body::from("{"))
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
    assert!(
        payload["error"]["message"]
            .as_str()
            .expect("message")
            .contains("Invalid config request body")
    );
}

#[tokio::test]
async fn config_put_endpoint_creates_backup_before_write() {
    let root = temp_root("config-put-backup");
    fs::create_dir_all(&root).expect("create root");
    write_test_config(&root);
    let paths = ProjectPaths::from_root(&root);
    let original = fs::read_to_string(&paths.config_path).expect("read original");
    let state = test_web_state(&root, Arc::new(TestEngine::with_fixtures()), WebAssetsMode::Embedded);
    let app = build_web_router(state);
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/config")
                .method("PUT")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "webPort": 5678 }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let backups = fs::read_dir(&paths.backups_dir).expect("read backups");
    let mut backup_found = false;
    for entry in backups {
        let entry = entry.expect("entry");
        if !entry.file_type().expect("file type").is_dir() {
            continue;
        }
        let backup_file = entry.path().join(".macc/macc.yaml");
        if backup_file.exists() {
            assert_eq!(fs::read_to_string(backup_file).expect("read backup"), original);
            backup_found = true;
            break;
        }
    }
    assert!(backup_found, "config backup not found");
}

#[tokio::test]
async fn backups_list_endpoint_returns_empty_array_when_no_backups_exist() {
    let root = temp_root("backups-list-empty");
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
                .uri("/api/v1/backups")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload, serde_json::json!([]));
}

#[tokio::test]
async fn backups_list_endpoint_returns_chronological_backup_metadata() {
    let root = temp_root("backups-list");
    fs::create_dir_all(&root).expect("create root");
    let backups_root = root.join(".macc").join("backups");
    fs::create_dir_all(backups_root.join("20260320-120000").join("nested")).expect("mkdir old");
    fs::create_dir_all(backups_root.join("20260320-130000")).expect("mkdir new");
    fs::write(
        backups_root.join("20260320-120000").join("nested").join("a.txt"),
        "abc",
    )
    .expect("write backup file");
    fs::write(backups_root.join("20260320-130000").join("b.txt"), "hello").expect("write backup");

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/backups")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload.as_array().expect("array").len(), 2);
    assert_eq!(payload[0]["id"], "20260320-130000");
    assert_eq!(payload[0]["timestamp"], "20260320-130000");
    assert_eq!(payload[0]["files"], 1);
    assert_eq!(payload[0]["totalSize"], 5);
    assert_eq!(payload[0]["entries"][0]["path"], "b.txt");
    assert_eq!(payload[0]["entries"][0]["size"], 5);
    assert_eq!(payload[1]["id"], "20260320-120000");
    assert_eq!(payload[1]["files"], 1);
    assert_eq!(payload[1]["entries"][0]["path"], "nested/a.txt");
    assert_eq!(payload[1]["entries"][0]["size"], 3);
}

#[tokio::test]
async fn backup_restore_endpoint_requires_confirmation() {
    let root = temp_root("backups-restore-no-confirm");
    fs::create_dir_all(root.join(".macc").join("backups").join("20260320-120000"))
        .expect("create backup dir");
    fs::write(
        root.join(".macc")
            .join("backups")
            .join("20260320-120000")
            .join("file.txt"),
        "restored",
    )
    .expect("write backup file");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/backups/20260320-120000/restore")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "confirmed": false }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], "MACC-WEB-1001");
    assert_eq!(payload["error"]["category"], "Validation");
    assert_eq!(payload["error"]["message"], "Confirmation required");
}

#[tokio::test]
async fn backup_restore_endpoint_creates_pre_restore_backup_and_restores_files() {
    let root = temp_root("backups-restore");
    fs::create_dir_all(&root).expect("create root");
    let paths = ProjectPaths::from_root(&root);
    let selected_backup = paths.backups_dir.join("20260320-120000");
    fs::create_dir_all(selected_backup.join("nested")).expect("create backup dir");
    fs::write(selected_backup.join("file.txt"), "restored value").expect("write backup file");
    fs::write(selected_backup.join("nested").join("child.txt"), "child restore")
        .expect("write nested backup file");
    fs::write(root.join("file.txt"), "current value").expect("write current file");
    fs::create_dir_all(root.join("nested")).expect("create current nested");
    fs::write(root.join("nested").join("child.txt"), "current child")
        .expect("write current nested file");

    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/backups/20260320-120000/restore")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "confirmed": true }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["backupId"], "20260320-120000");
    assert_eq!(payload["restoredFiles"], 2);

    let restore_backup_id = payload["restoreBackupId"]
        .as_str()
        .expect("restore backup id");
    assert_ne!(restore_backup_id, "20260320-120000");
    assert_eq!(
        fs::read_to_string(root.join("file.txt")).expect("read restored root file"),
        "restored value"
    );
    assert_eq!(
        fs::read_to_string(root.join("nested").join("child.txt"))
            .expect("read restored nested file"),
        "child restore"
    );
    assert_eq!(
        fs::read_to_string(paths.backups_dir.join(restore_backup_id).join("file.txt"))
            .expect("read pre restore backup"),
        "current value"
    );
    assert_eq!(
        fs::read_to_string(
            paths.backups_dir
                .join(restore_backup_id)
                .join("nested")
                .join("child.txt")
        )
        .expect("read nested pre restore backup"),
        "current child"
    );
}

fn init_worktree_repo(root: &Path) {
    init_git_repo(root);
    commit_file(
        root,
        "README.md",
        "# test\n",
        "feat: seed repo",
        "",
        "2026-03-20T12:00:00Z",
    );
}

fn create_test_worktree(root: &Path, slug: &str) -> macc_core::WorktreeCreateResult {
    macc_core::create_worktrees(
        root,
        &macc_core::WorktreeCreateSpec {
            slug: slug.to_string(),
            tool: "codex".to_string(),
            count: 1,
            base: "main".to_string(),
            dir: std::path::PathBuf::from(".macc/worktree"),
            scope: Some("project".to_string()),
            feature: Some("web".to_string()),
        },
    )
    .expect("create worktree")
    .into_iter()
    .next()
    .expect("created worktree")
}

#[tokio::test]
async fn worktrees_list_endpoint_returns_empty_array_when_only_root_exists() {
    let root = temp_root("worktrees-list-empty");
    init_worktree_repo(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/worktrees")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload, serde_json::json!([]));
}

#[tokio::test]
async fn worktrees_list_endpoint_returns_managed_worktree_metadata() {
    let root = temp_root("worktrees-list");
    init_worktree_repo(&root);
    let created = create_test_worktree(&root, "list-worktree");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/worktrees")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    let items = payload.as_array().expect("array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], created.id);
    assert_eq!(items[0]["tool"], "codex");
    assert_eq!(items[0]["baseBranch"], "main");
    assert_eq!(items[0]["scope"], "project");
    assert_eq!(items[0]["feature"], "web");
    assert_eq!(items[0]["status"], "dirty");
    assert_eq!(items[0]["path"], created.path.to_string_lossy().to_string());
}

#[tokio::test]
async fn worktrees_list_endpoint_normalizes_unmanaged_branch_names() {
    let root = temp_root("worktrees-list-unmanaged");
    init_worktree_repo(&root);
    let unmanaged_path = root.join(".macc").join("worktree").join("plain-branch");
    run_git_ok(
        &root,
        &[
            "worktree",
            "add",
            unmanaged_path.to_str().expect("worktree path"),
            "-b",
            "plain-branch",
            "main",
        ],
    );
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/worktrees")
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    let items = payload.as_array().expect("array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["branch"], "plain-branch");
    assert_eq!(items[0]["tool"], serde_json::Value::Null);
}

#[tokio::test]
async fn worktrees_create_endpoint_creates_worktree_and_returns_payload() {
    let root = temp_root("worktrees-create");
    init_worktree_repo(&root);
    write_test_config(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/worktrees")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "slug": "api-create",
                        "tool": "codex",
                        "count": 1,
                        "baseBranch": "main",
                        "scope": "project",
                        "feature": "web",
                        "skipApply": true
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    let items = payload.as_array().expect("array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["tool"], "codex");
    assert_eq!(items[0]["baseBranch"], "main");
    assert_eq!(items[0]["scope"], "project");
    assert_eq!(items[0]["feature"], "web");
    assert_eq!(items[0]["status"], "dirty");
    let created_path = std::path::PathBuf::from(items[0]["path"].as_str().expect("path"));
    assert!(created_path.exists());
}

#[tokio::test]
async fn worktrees_create_endpoint_rejects_invalid_params() {
    let root = temp_root("worktrees-create-invalid");
    init_worktree_repo(&root);
    write_test_config(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/worktrees")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "slug": "invalid",
                        "tool": "codex",
                        "count": 0,
                        "base": "main",
                        "skipApply": true
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], "MACC-WEB-1000");
    assert_eq!(payload["error"]["message"], "worktree count must be >= 1");
}

#[tokio::test]
async fn worktrees_delete_endpoint_removes_existing_worktree() {
    let root = temp_root("worktrees-delete");
    init_worktree_repo(&root);
    let created = create_test_worktree(&root, "delete-worktree");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/v1/worktrees/{}?force=true&removeBranch=true&forceConfirmed=true",
                    created.id
                ))
                .method("DELETE")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "confirmed": true }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["status"], "ok");
    assert_eq!(payload["id"], created.id);
    assert!(!created.path.exists());
    assert!(
        !macc_core::git::rev_parse_verify(&root, &format!("refs/heads/{}", created.branch))
            .expect("check branch removed")
    );
}

#[tokio::test]
async fn worktrees_delete_endpoint_returns_not_found_for_unknown_id() {
    let root = temp_root("worktrees-delete-missing");
    init_worktree_repo(&root);
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/worktrees/missing-worktree")
                .method("DELETE")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "confirmed": true }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], "MACC-WEB-2004");
    assert_eq!(payload["error"]["category"], "NotFound");
}

#[tokio::test]
async fn worktrees_delete_endpoint_requires_double_confirmation_for_force() {
    let root = temp_root("worktrees-delete-force");
    init_worktree_repo(&root);
    let created = create_test_worktree(&root, "force-worktree");
    let state = test_web_state(
        &root,
        Arc::new(TestEngine::with_fixtures()),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/worktrees/{}?force=true", created.id))
                .method("DELETE")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({ "confirmed": true }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], "MACC-WEB-1001");
    assert_eq!(
        payload["error"]["message"],
        "Force delete requires a second confirmation"
    );
    assert!(created.path.exists());
}

#[tokio::test]
async fn worktrees_run_endpoint_returns_started_status_and_triggers_engine() {
    let root = temp_root("worktrees-run");
    init_worktree_repo(&root);
    let created = create_test_worktree(&root, "run-worktree");
    let engine = Arc::new(WebTestEngine::with_worktree_run_result(Ok(())));
    let state = test_web_state(&root, engine.clone(), WebAssetsMode::Embedded);
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/worktrees/{}/run", created.id))
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::ACCEPTED);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["status"], "started");
    assert_eq!(payload["worktreeId"], created.id);

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(engine.worktree_run_calls(), vec![created.id]);
}

#[tokio::test]
async fn worktrees_run_endpoint_returns_not_found_for_unknown_worktree() {
    let root = temp_root("worktrees-run-missing");
    init_worktree_repo(&root);
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_worktree_run_result(Ok(()))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/worktrees/missing-worktree/run")
                .method("POST")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    let bytes = response.into_body().collect().await.expect("collect").to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(payload["error"]["code"], "MACC-WEB-2004");
    assert_eq!(payload["error"]["category"], "NotFound");
}

#[tokio::test]
async fn worktree_logs_endpoint_streams_log_lines_as_sse() {
    let root = temp_root("worktrees-logs");
    init_worktree_repo(&root);
    let created = create_test_worktree(&root, "logs-worktree");
    let log_dir = root
        .join(".macc")
        .join("log")
        .join("performer")
        .join(&created.id);
    fs::create_dir_all(&log_dir).expect("create performer log dir");
    fs::write(
        log_dir.join("20260320-120000.md"),
        "2026-03-20T12:00:00Z INFO boot\n2026-03-20T12:00:01Z ERROR failed step\n",
    )
    .expect("write performer log");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_worktree_run_result(Ok(()))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/worktrees/{}/logs", created.id))
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
    let text = String::from_utf8(first_frame.into_data().expect("data frame").to_vec())
        .expect("utf8");

    assert!(text.contains("event: log_line"), "{text}");
    assert!(text.contains("id: 1"), "{text}");
    assert!(text.contains("\"worktree_id\":\""), "{text}");
    assert!(text.contains("\"level\":\"info\""), "{text}");
    assert!(text.contains("\"message\":\"boot\""), "{text}");
}

#[tokio::test]
async fn worktree_logs_endpoint_resumes_after_last_event_id() {
    let root = temp_root("worktrees-logs-resume");
    init_worktree_repo(&root);
    let created = create_test_worktree(&root, "logs-resume-worktree");
    let log_dir = root
        .join(".macc")
        .join("log")
        .join("performer")
        .join(&created.id);
    fs::create_dir_all(&log_dir).expect("create performer log dir");
    fs::write(
        log_dir.join("20260320-120000.md"),
        "2026-03-20T12:00:00Z INFO boot\n2026-03-20T12:00:01Z WARN retrying\n2026-03-20T12:00:02Z ERROR failed step\n",
    )
    .expect("write performer log");
    let state = test_web_state(
        &root,
        Arc::new(WebTestEngine::with_worktree_run_result(Ok(()))),
        WebAssetsMode::Embedded,
    );
    let app = build_web_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/worktrees/{}/logs", created.id))
                .method("GET")
                .header("Last-Event-ID", "2")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let mut body = response.into_body();
    let frame = tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .expect("frame timeout")
        .expect("frame")
        .expect("body frame");
    let text =
        String::from_utf8(frame.into_data().expect("data frame").to_vec()).expect("utf8");

    assert!(text.contains("id: 3"), "{text}");
    assert!(text.contains("event: log_line"), "{text}");
    assert!(text.contains("\"level\":\"error\""), "{text}");
    assert!(text.contains("\"message\":\"failed step\""), "{text}");
}

#[tokio::test]
async fn worktree_log_stream_emits_heartbeat_records() {
    let response = axum::response::sse::Sse::new(worktrees::worktree_log_stream(
        worktrees::WorktreeLogTarget {
            worktree_id: "wt-1".to_string(),
            log_path: temp_root("worktree-log-heartbeat").join("missing.log"),
        },
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
