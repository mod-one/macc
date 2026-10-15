use super::*;
use serde_json::json;
use std::path::Path;
fn profile(scope: &str) -> crate::tool::usage::UsageSpec {
    serde_json::from_value(json!({"schema":"future-meter-v1","event_match":{"/type":"result"},"text":["/text"],"token_scope":scope,"cost_scope":scope,"currency":"USD","session_id":"/session_id","fields":{"input_uncached":"/usage/input","cached_read":"/usage/cached","cached_write":"/usage/write","output":"/usage/output","reasoning":"/usage/reasoning","cost":"/cost"}})).unwrap()
}
fn root() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join(".macc/state")).unwrap();
    let p = migrate(t.path(), false, None).unwrap();
    migrate(t.path(), true, Some(&p.source_sha256)).unwrap();
    t
}
fn claim(root: &Path, task: &str, tool: &str, scope: &str) -> LeaseGrant {
    let spec = json!({"id":tool,"performer":{"command":"fake","usage":profile(scope),"session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}});
    let identity = crate::coordinator::session_identity::identity(
        root,
        root,
        task,
        "performer",
        tool,
        &spec,
        &crate::coordinator::session_identity::SessionRouting {
            tier: "".into(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let mut grant = acquire(
        root,
        &AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: false,
            checkpoint_id: None,
        },
    )
    .unwrap();
    bind(root, &grant.key, &grant.key.session_id).unwrap();
    grant.provider_session_id = Some(grant.key.session_id.clone());
    grant
}
fn finish(key: &LeaseKey, input: u64, cost: f64) -> EndRequest {
    EndRequest{key:key.clone(),outcome:"success".into(),raw:json!({"type":"result","session_id":key.session_id,"usage":{"input":input,"cached":10,"write":2,"output":5},"cost":cost,"sensitive":"sk-abcdefghijklmnopqrstuvwxyz"}).to_string(),omitted:false}
}
#[test]
fn cumulative_resume_and_replayed_receipts_never_double_count() {
    let t = root();
    let r = t.path();
    let first = claim(r, "A", "future-tool", "session");
    begin(
        r,
        &BeginRequest {
            key: first.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    let request = finish(&first.key, 100, 0.2);
    let one = end(r, &request).unwrap();
    assert_eq!(one.incremental.input_uncached, Some(100));
    release(r, &first.key, true).unwrap();
    assert_eq!(
        end(r, &request).unwrap().incremental.input_uncached,
        Some(100)
    );
    let second = claim(r, "A", "future-tool", "session");
    assert_eq!(first.key.session_id, second.key.session_id);
    begin(
        r,
        &BeginRequest {
            key: second.key.clone(),
            phase: "fix".into(),
        },
    )
    .unwrap();
    let two = end(r, &finish(&second.key, 140, 0.25)).unwrap();
    assert_eq!(two.incremental.input_uncached, Some(40));
    assert_eq!(two.incremental.cost, Some(50_000_000));
    assert!(end(r, &finish(&second.key, 141, 0.25)).is_err());
    let view = inspect(r).unwrap();
    assert_eq!(view.records.len(), 2);
    assert_eq!(
        view.totals
            .iter()
            .filter_map(|t| t.counters.input_uncached)
            .sum::<u64>(),
        140
    );
    assert!(!serde_json::to_string(&view)
        .unwrap()
        .contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(!serde_json::to_string(&view)
        .unwrap()
        .contains(&first.key.lease_token));
}
#[test]
fn missing_baseline_reset_and_partial_fields_stay_unknown() {
    let current = Counters {
        input_uncached: Some(10),
        cost: Some(20),
        ..Default::default()
    };
    let (delta, notes) = normalization::incremental(&current, None, false, "session", "session");
    assert_eq!(delta.input_uncached, None);
    assert!(!notes.is_empty());
    let prior = Counters {
        input_uncached: Some(50),
        cost: Some(100),
        ..Default::default()
    };
    let (delta, _) =
        normalization::incremental(&current, Some(&prior), false, "session", "session");
    assert_eq!(delta.cost, None);
    let (delta, _) =
        normalization::incremental(&current, Some(&prior), false, "invocation", "invocation");
    assert_eq!(delta.input_uncached, Some(10));
    assert_eq!(delta.cached_write, None);
}
#[test]
fn terminal_stats_are_not_added_to_steps_or_repeated_totals() {
    let p = profile("invocation");
    let raw="{\"type\":\"assistant\",\"usage\":{\"input\":99}}\n{\"type\":\"result\",\"usage\":{\"input\":7,\"cached\":0}}\n{\"type\":\"result\",\"usage\":{\"input\":7,\"cached\":0}}\n";
    let o = parser::observe(raw, Some(&p));
    assert_eq!(o.counters.input_uncached, Some(7));
    assert_eq!(o.counters.cached_read, Some(0));
    assert_eq!(o.counters.output, None);
    assert!(parser::observe("tokens used: 12000", Some(&p))
        .counters
        .input_uncached
        .is_none());
}
#[test]
fn invalid_numbers_and_cache_underflow_never_become_estimates() {
    let mut p = profile("invocation");
    p.input_includes_cache_read = true;
    let o = parser::observe(
        r#"{"type":"result","usage":{"input":2,"cached":4,"write":-3,"output":"10"},"cost":-1}"#,
        Some(&p),
    );
    assert_eq!(o.counters.input_uncached, None);
    assert_eq!(o.counters.cached_write, None);
    assert_eq!(o.counters.output, None);
    assert_eq!(o.counters.cost, None);
}
#[test]
fn unknown_and_oversized_calls_are_visible_without_affecting_tasks() {
    let t = root();
    let r = t.path();
    let lease = claim(r, "A", "future-tool", "invocation");
    let plan = begin(
        r,
        &BeginRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    assert!(!plan.structured);
    let pending = inspect(r).unwrap();
    assert_eq!(pending.records[0].status, "pending");
    assert_eq!(pending.totals[0].counters.input_uncached, None);
    let mut request = finish(&lease.key, 200, 1.);
    request.omitted = true;
    let row = end(r, &request).unwrap();
    assert_eq!(row.token_quality, "unknown");
    assert!(row.notes.contains(&"output_exceeds_capture_bound".into()));
    release(r, &lease.key, true).unwrap();
}
#[test]
fn feature_upgrade_requires_reviewed_source_and_preserves_backup() {
    let t = root();
    let r = t.path();
    let (db, _) = db::active(r).unwrap();
    db.execute_batch("DROP TABLE invocation_usage;DROP TABLE usage_watermarks;DELETE FROM session_meta WHERE key='usage_schema_version'").unwrap();
    drop(db);
    assert!(!inspect(r).unwrap().ready);
    let plan = upgrade(r, false, None).unwrap();
    assert!(upgrade(r, true, Some("stale")).is_err());
    let report = upgrade(r, true, Some(&plan.source_sha256)).unwrap();
    assert!(Path::new(report.backup.as_ref().unwrap()).exists());
    assert!(inspect(r).unwrap().ready);
}
#[test]
fn tasks_tools_and_stale_fencing_remain_isolated() {
    let t = root();
    let r = t.path();
    let a = claim(r, "A", "future-tool", "invocation");
    begin(
        r,
        &BeginRequest {
            key: a.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    end(r, &finish(&a.key, 5, 0.)).unwrap();
    release(r, &a.key, true).unwrap();
    let b = claim(r, "B", "other-tool", "invocation");
    begin(
        r,
        &BeginRequest {
            key: b.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    let mut bad = b.key.clone();
    bad.lease_token = "foreign".into();
    assert!(end(r, &finish(&bad, 50, 0.)).is_err());
    end(r, &finish(&b.key, 50, 0.)).unwrap();
    let v = inspect(r).unwrap();
    assert_eq!(v.totals.len(), 2);
    assert_ne!(v.totals[0].task_id, v.totals[1].task_id);
}
#[test]
fn built_in_adapter_schemas_preserve_the_output_only_contract() {
    for body in [
        include_str!("../../../../registry/tools.d/codex.tool.yaml"),
        include_str!("../../../../registry/tools.d/claude.tool.yaml"),
        include_str!("../../../../registry/tools.d/gemini.tool.yaml"),
    ] {
        let spec: crate::tool::ToolSpec = serde_yaml::from_str(body).unwrap();
        spec.validate().unwrap();
        assert!(spec.performer.unwrap().usage.is_some());
    }
    let mut p = profile("invocation");
    p.append_args = vec!["--dangerously-change-permissions".into()];
    assert!(p.validate().is_err());
    p.append_args = vec!["--output-format".into()];
    assert!(p.validate().is_err());
    p.append_args = vec!["json".into()];
    assert!(p.validate().is_err());
}

#[test]
fn registry_profiles_parse_terminal_counters_and_assistant_text() {
    let bodies = [
        include_str!("../../../../registry/tools.d/codex.tool.yaml"),
        include_str!("../../../../registry/tools.d/claude.tool.yaml"),
        include_str!("../../../../registry/tools.d/gemini.tool.yaml"),
    ];
    let profiles: Vec<_> = bodies
        .iter()
        .map(|body| {
            serde_yaml::from_str::<crate::tool::ToolSpec>(body)
                .unwrap()
                .performer
                .unwrap()
                .usage
                .unwrap()
        })
        .collect();
    let raw = "{\"type\":\"item.completed\",\"item\":{\"type\":\"reasoning\",\"text\":\"private\"}}\n{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"done\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":100,\"cached_input_tokens\":40,\"output_tokens\":10,\"reasoning_output_tokens\":3}}";
    let o = parser::observe(raw, Some(&profiles[0]));
    assert_eq!(o.counters.input_uncached, Some(60));
    assert_eq!(o.counters.reasoning, Some(3));
    assert_eq!(parser::render(raw, &profiles[0]), "done");
    let o = parser::observe(
        r#"{"type":"result","session_id":"native","result":"done","total_cost_usd":0.125,"modelUsage":{"model-a":{"inputTokens":10,"cacheReadInputTokens":20,"cacheCreationInputTokens":5,"outputTokens":7},"model-b":{"inputTokens":3,"cacheReadInputTokens":0,"cacheCreationInputTokens":0,"outputTokens":2}}}"#,
        Some(&profiles[1]),
    );
    assert_eq!(o.counters.input_uncached, Some(13));
    assert_eq!(o.counters.output, Some(9));
    assert_eq!(o.counters.cost, Some(125_000_000));
    let raw="{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"do\"}\n{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"ne\"}\n{\"type\":\"result\",\"stats\":{\"input\":10,\"cached\":5,\"output_tokens\":3}}";
    assert_eq!(parser::render(raw, &profiles[2]), "done");
    assert_eq!(
        parser::observe(raw, Some(&profiles[2]))
            .counters
            .input_uncached,
        Some(10)
    );
    assert!(parser::observe(raw, None)
        .counters
        .values()
        .iter()
        .all(Option::is_none));
}

#[test]
fn concurrent_receipt_replays_record_one_invocation() {
    let t = root();
    let lease = claim(t.path(), "A", "future-tool", "invocation");
    begin(
        t.path(),
        &BeginRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..4)
            .map(|_| s.spawn(|| end(t.path(), &finish(&lease.key, 10, 0.))))
            .collect();
        for h in handles {
            assert_eq!(
                h.join().unwrap().unwrap().incremental.input_uncached,
                Some(10)
            );
        }
    });
    let view = inspect(t.path()).unwrap();
    assert_eq!(view.records.len(), 1);
    assert_eq!(view.totals[0].coverage.input_uncached, Some(1));
    assert_eq!(view.totals[0].counters.input_uncached, Some(10));
}

#[test]
fn mismatched_native_session_is_not_attributed() {
    let t = root();
    let lease = claim(t.path(), "A", "future-tool", "session");
    begin(
        t.path(),
        &BeginRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    let mut request = finish(&lease.key, 10, 1.);
    request.raw = request
        .raw
        .replace(&lease.key.session_id, "foreign-session");
    let record = end(t.path(), &request).unwrap();
    assert_eq!(record.incremental.input_uncached, None);
    assert!(record.notes.contains(&"provider_session_mismatch".into()));
}

#[test]
fn active_capture_plan_is_immutable_until_the_next_invocation() {
    let t = root();
    let lease = claim(t.path(), "A", "future-tool", "invocation");
    let request = BeginRequest {
        key: lease.key.clone(),
        phase: "dev".into(),
    };
    assert!(!begin(t.path(), &request).unwrap().structured);
    let view = super::super::inspect(t.path()).unwrap();
    let mut policy = view.policy;
    policy.usage.capture_mode = 1;
    super::super::update_policy(t.path(), &view.revision, &policy).unwrap();
    assert!(!begin(t.path(), &request).unwrap().structured);
    end(t.path(), &finish(&lease.key, 1, 0.)).unwrap();
    release(t.path(), &lease.key, true).unwrap();
    let next = claim(t.path(), "A", "future-tool", "invocation");
    assert!(
        begin(
            t.path(),
            &BeginRequest {
                key: next.key,
                phase: "fix".into()
            }
        )
        .unwrap()
        .structured
    );
}

#[test]
fn cache_write_only_observations_count_as_measured_calls() {
    let t = root();
    let lease = claim(t.path(), "A", "future-tool", "invocation");
    begin(
        t.path(),
        &BeginRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    end(
        t.path(),
        &EndRequest {
            key: lease.key,
            outcome: "success".into(),
            raw: r#"{"type":"result","usage":{"write":0}}"#.into(),
            omitted: false,
        },
    )
    .unwrap();
    let view = inspect(t.path()).unwrap();
    assert_eq!(view.totals[0].measured_invocations, 1);
    assert_eq!(view.totals[0].coverage.cached_write, Some(1));
    assert_eq!(view.totals[0].counters.cached_write, Some(0));
    assert_eq!(view.totals[0].counters.input_uncached, None);
}

#[test]
fn imported_transcripts_with_zero_legacy_uses_do_not_get_a_zero_baseline() {
    let t = root();
    let lease = claim(t.path(), "A", "future-tool", "session");
    // Imported rows lack a native new-session reservation, even if their first
    // acquisition increments a missing legacy use count from zero to one.
    let (db, _) = db::active(t.path()).unwrap();
    db.execute(
        "DELETE FROM session_events WHERE logical_session_id=?1 AND event_type='session_reserved'",
        [&lease.key.session_id],
    )
    .unwrap();
    db::event(
        &db,
        &lease.key.session_id,
        "session_imported",
        json!({"status":"available"}),
    )
    .unwrap();
    drop(db);
    begin(
        t.path(),
        &BeginRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    let record = end(t.path(), &finish(&lease.key, 100, 1.)).unwrap();
    assert_eq!(record.reported.cost, Some(1_000_000_000));
    assert_eq!(record.incremental.cost, None);
    assert_eq!(record.incremental.input_uncached, None);
}
