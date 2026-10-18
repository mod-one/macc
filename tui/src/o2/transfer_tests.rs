use super::*;
use macc_core::{o2::memory, session_store};
fn setup() -> (tempfile::TempDir, Panel, String) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-qm",
            "Fixture",
        ],
    ] {
        assert!(
            macc_core::git::run_git_output_mapped(root, &args, "initialize transfer fixture")
                .unwrap()
                .status
                .success()
        );
    }
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    let plan = session_store::migrate(root, false, None).unwrap();
    session_store::migrate(root, true, Some(&plan.source_sha256)).unwrap();
    let view = policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.memory.enabled = true;
    policy::update(root, &view.revision, Some(p)).unwrap();
    let proposal = memory::Proposal {
        kind: "constraint".into(),
        title: "Transfer claim".into(),
        body: "Review this sourced claim locally".into(),
        scope: "project".into(),
        scope_ref: None,
        confidence: 0.8,
        evidence: vec![],
        tags: vec![],
        source_task: None,
        source_revision: None,
    };
    let record = memory::propose(root, &proposal).unwrap();
    let hit = memory::get(root, &record.id, &memory::Access::default()).unwrap();
    let panel = Panel {
        access: json!({}),
        rows: vec![serde_json::to_value(hit).unwrap()],
        ..Default::default()
    };
    (temp, panel, record.id)
}
fn fill(panel: &mut Panel, root: &Path, values: &[&str]) {
    for value in values {
        for ch in value.chars() {
            panel.key(root, KeyCode::Char(ch)).unwrap();
        }
        panel.key(root, KeyCode::Enter).unwrap();
    }
}
#[test]
fn guided_memory_transfer_previews_confirms_and_preserves_local_decisions() {
    let (temp, mut panel, id) = setup();
    let root = temp.path();
    panel.key(root, KeyCode::Char('E')).unwrap();
    fill(&mut panel, root, &["", "jsonl", "export.jsonl"]);
    assert!(matches!(panel.pending, Some(Action::MemoryExport { .. })));
    assert!(!root.join("export.jsonl").exists());
    assert!(panel.mutation(KeyCode::Char('a')));
    panel.key(root, KeyCode::Char('a')).unwrap();
    assert!(root.join("export.jsonl").exists());
    assert!(panel.pending.is_none());
    panel.key(root, KeyCode::Char('I')).unwrap();
    fill(&mut panel, root, &["export.jsonl", "jsonl"]);
    assert!(matches!(panel.pending, Some(Action::MemoryImport { .. })));
    panel.key(root, KeyCode::Char('a')).unwrap();
    assert_eq!(
        memory::get(root, &id, &memory::Access::default())
            .unwrap()
            .memory
            .status,
        "proposed"
    );
    assert_eq!(
        execute(
            root,
            Action::MemorySearch {
                search: memory::Search {
                    query: String::new(),
                    access: Default::default(),
                    include_inactive: true
                }
            }
        )
        .unwrap()
        .as_array()
        .unwrap()
        .len(),
        2
    );
    panel.key(root, KeyCode::Char('I')).unwrap();
    fill(&mut panel, root, &["export.jsonl", "jsonl"]);
    panel.access = json!({"task_id":"different"});
    assert!(panel.key(root, KeyCode::Char('a')).is_err());
    assert!(panel.pending.is_none());
}
#[test]
fn transfer_can_be_discarded_and_export_never_overwrites_files() {
    let (temp, mut panel, _) = setup();
    let root = temp.path();
    panel.key(root, KeyCode::Char('E')).unwrap();
    fill(&mut panel, root, &["", "markdown", "new.md"]);
    panel.key(root, KeyCode::Char('D')).unwrap();
    assert!(panel.pending.is_none());
    assert!(!root.join("new.md").exists());
    panel.key(root, KeyCode::Char('E')).unwrap();
    fill(&mut panel, root, &["", "markdown", "new.md"]);
    // A file created after preview is protected by create_new at Apply.
    std::fs::write(root.join("new.md"), "existing work").unwrap();
    assert!(panel.key(root, KeyCode::Char('a')).is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("new.md")).unwrap(),
        "existing work"
    );
}
