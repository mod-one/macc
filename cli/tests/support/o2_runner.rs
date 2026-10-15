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
