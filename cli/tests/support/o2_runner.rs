use super::*;
#[test]
fn real_runner_injects_reviewed_scoped_memory_and_keeps_mandatory_prd() {
    use macc_core::o2::{memory, policy};
    let (temp, lib) = setup();
    let root = temp.path();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.memory.enabled = true;
    p.context_engine.enabled = true;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let evidence: memory::Evidence = serde_json::from_value(
        macc_core::o2::actions::execute(
            root,
            macc_core::o2::actions::Action::MemoryEvidence {
                path: ".gitignore".into(),
                worktree: None,
            },
        )
        .unwrap(),
    )
    .unwrap();
    let proposal = memory::Proposal {
        kind: "constraint".into(),
        title: "Keep work".into(),
        body: "Keep work: preserve the existing repository ignore contract.".into(),
        scope: "task".into(),
        scope_ref: Some("A".into()),
        confidence: 0.9,
        evidence: vec![evidence],
        tags: vec![],
        source_task: Some("A".into()),
        source_revision: None,
    };
    let proposed = memory::propose(root, &proposal).unwrap();
    let access = memory::Access {
        task_id: Some("A".into()),
        ..Default::default()
    };
    let hit = memory::get(root, &proposed.id, &access).unwrap();
    memory::review(
        root,
        &memory::Review {
            id: proposed.id,
            status: "active".into(),
            reason: "Checked actual source".into(),
            confirmed: true,
            expected_checksum: hit.checksum,
            evidence: None,
        },
    )
    .unwrap();
    let fake = root.join("fake-ai.sh");
    let script = std::fs::read_to_string(&fake).unwrap();
    std::fs::write(
        &fake,
        script.replace(
            "call_arg=",
            "printf '%s' \"${@: -1}\" > \"$PROMPT_CAPTURE\"\ncall_arg=",
        ),
    )
    .unwrap();
    let captured = root.join(".macc/prompt-capture.txt");
    let out = runner(root, &lib, "A", "performer")
        .env("PROMPT_CAPTURE", &captured)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let prompt = std::fs::read_to_string(&captured).unwrap();
    assert!(prompt.contains("acceptance_criteria"));
    assert!(prompt.contains("Keep work"));
    assert!(prompt.contains("preserve the existing repository ignore contract"));
    let usage = macc_core::session_store::usage::inspect(root).unwrap();
    assert!(usage.records[0].context_bundle_id.is_some());
    runner(root, &lib, "B", "performer")
        .env("PROMPT_CAPTURE", &captured)
        .output()
        .unwrap();
    assert!(!std::fs::read_to_string(captured)
        .unwrap()
        .contains("preserve the existing repository ignore contract"));
}

#[test]
fn agent_cli_budgets_cover_stdout_serialization_and_all_memory_fragments() {
    use macc_core::context::estimate_tokens;
    use macc_core::o2::{
        actions::{execute, Action},
        memory, policy,
    };
    let (temp, _) = setup();
    let root = temp.path();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.memory.enabled = true;
    p.context_engine.enabled = true;
    p.context_engine.pull_limit_tokens = 100;
    p.code_intelligence.enabled = true;
    p.code_intelligence.output_tokens = 100;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let evidence = serde_json::from_value(
        execute(
            root,
            Action::MemoryEvidence {
                path: ".gitignore".into(),
                worktree: None,
            },
        )
        .unwrap(),
    )
    .unwrap();
    let body = "contract proof € \"value\"\\path\n".repeat(25);
    let fact = memory::propose(
        root,
        &memory::Proposal {
            kind: "constraint".into(),
            title: "Contract".into(),
            body: body.clone(),
            scope: "project".into(),
            scope_ref: None,
            confidence: 0.9,
            evidence: vec![evidence],
            tags: vec![],
            source_task: None,
            source_revision: None,
        },
    )
    .unwrap();
    let inspected = memory::get(root, &fact.id, &Default::default()).unwrap();
    memory::review(
        root,
        &memory::Review {
            id: fact.id.clone(),
            status: "active".into(),
            reason: "Reviewed exact source".into(),
            confirmed: true,
            expected_checksum: inspected.checksum,
            evidence: None,
        },
    )
    .unwrap();
    let invoke = |args: &[&str]| -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_macc"))
            .arg("--cwd")
            .arg(root)
            .arg("o2")
            .args(args)
            .env("MACC_INTERNAL_INVOCATION", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = std::str::from_utf8(&out.stdout).unwrap();
        let actual = estimate_tokens(stdout);
        let v: Value = serde_json::from_str(stdout).unwrap();
        assert!(actual <= 100, "{actual}: {stdout}");
        let reported = if v["outcome"] == "supported" {
            &v["data"]["estimated_tokens"]
        } else {
            &v["estimated_tokens"]
        };
        assert_eq!(reported.as_u64(), Some(actual as u64));
        v
    };
    let search = invoke(&["memory-search", "contract"]);
    assert_eq!(search["references"][0], fact.id);
    let mut content = String::new();
    let mut offset = 0;
    loop {
        let page = invoke(&["memory-get", &fact.id, "--offset", &offset.to_string()]);
        content.push_str(page["fragment"].as_str().unwrap());
        if let Some(next) = page["next_offset"].as_u64() {
            assert!(next > offset);
            offset = next;
        } else {
            break;
        }
    }
    let full: Value = serde_json::from_str(&content).unwrap();
    assert_eq!(full["memory"]["proposal"]["body"], body);
    std::fs::write(
        root.join("sample.rs"),
        format!("needle {}", "plain words ".repeat(20)),
    )
    .unwrap();
    for args in [
        vec!["add", "sample.rs"],
        vec!["commit", "-m", "query fixture"],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    let result = invoke(&["code-query", "needle"]);
    assert!(result["data"]["hits"].as_array().unwrap().is_empty());
    assert!(result["data"]["diagnostics"]
        .to_string()
        .contains("Output bounded"));
}

#[test]
fn scoped_mcp_flows_through_generic_runner_and_real_cli_stdio() {
    use macc_core::o2::policy;
    let (temp, lib) = setup();
    let root = temp.path();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.memory.enabled = true;
    p.memory.mcp_enabled = true;
    p.memory.phase_extraction_enabled = true;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let frames=concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"context.get_checkpoint\",\"arguments\":{}}}\n"
    );
    std::fs::write(root.join(".macc/mcp-input"), frames).unwrap();
    let fake = root.join("fake-ai.sh");
    let script = std::fs::read_to_string(&fake).unwrap();
    let extra = r#"
"$MACC_SESSION_SERVICE_BIN" --verbose o2 mcp < "$MCP_INPUT" > "$MCP_OUTPUT" || exit 43
jq -nc '{session_id:env.MACC_O2_MCP_SESSION,generation:(env.MACC_O2_MCP_GENERATION|tonumber),token:env.MACC_O2_MCP_TOKEN}' > "$MCP_CAPTURE"
echo 'REVIEW_VERDICT: CHANGES_REQUESTED'
"#;
    std::fs::write(
        &fake,
        script.replace("call_arg=", &format!("{extra}\ncall_arg=")),
    )
    .unwrap();
    let out = runner(root, &lib, "A", "reviewer")
        .env("MCP_INPUT", root.join(".macc/mcp-input"))
        .env("MCP_OUTPUT", root.join(".macc/mcp-output"))
        .env("MCP_CAPTURE", root.join(".macc/mcp-capture"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let output = std::fs::read_to_string(root.join(".macc/mcp-output")).unwrap();
    let replies = output
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "macc-o2");
    let checkpoint: Value =
        serde_json::from_str(replies[1]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(checkpoint["task_id"], "A");
    let credentials =
        serde_json::from_slice(&std::fs::read(root.join(".macc/mcp-capture")).unwrap()).unwrap();
    assert!(macc_core::o2::mcp::Server::new(root, credentials).is_err());
    let usage = macc_core::session_store::usage::inspect(root).unwrap();
    assert_eq!(usage.records[0].outcome.as_deref(), Some("success"));
    let hits = macc_core::o2::memory::search(
        root,
        &macc_core::o2::memory::Search {
            query: "".into(),
            access: macc_core::o2::memory::Access {
                task_id: Some("A".into()),
                ..Default::default()
            },
            include_inactive: true,
        },
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].memory.status, "proposed");
    assert!(hits[0]
        .memory
        .proposal
        .tags
        .contains(&"outcome:changes_requested".into()));
}
