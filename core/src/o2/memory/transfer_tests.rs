use super::*;
use serde_json::json;
use transfer::*;
fn setup() -> (tempfile::TempDir, Memory, ExportRequest) {
    let root = crate::o2::tests::root();
    let memory = propose(root.path(), &crate::o2::tests::proposal(root.path(), None)).unwrap();
    crate::o2::tests::activate(root.path(), &memory, None);
    let request = ExportRequest {
        ids: vec![memory.id.clone()],
        access: Access::default(),
        format: Format::Jsonl,
    };
    (root, memory, request)
}
#[test]
fn transfer_round_trips_both_formats_without_trusting_or_overwriting_source_reviews() {
    for format in [Format::Jsonl, Format::Markdown] {
        let (source, m, mut request) = setup();
        request.format = format;
        let plan = export_preview(source.path(), &request).unwrap();
        assert!(export(
            source.path(),
            &request,
            plan["checksum"].as_str().unwrap(),
            false
        )
        .is_err());
        let bundle = export(
            source.path(),
            &request,
            plan["checksum"].as_str().unwrap(),
            true,
        )
        .unwrap();
        let target = crate::o2::tests::root();
        let input = ImportRequest {
            document: bundle["document"].as_str().unwrap().into(),
            format,
            access: Access::default(),
        };
        let plan = import_preview(target.path(), &input).unwrap();
        assert!(import(target.path(), &input, "wrong", true).is_err());
        assert!(import(
            target.path(),
            &input,
            plan["checksum"].as_str().unwrap(),
            false
        )
        .is_err());
        let result = import(
            target.path(),
            &input,
            plan["checksum"].as_str().unwrap(),
            true,
        )
        .unwrap();
        let id = result["ids"][0].as_str().unwrap();
        let imported = get(target.path(), id, &Access::default()).unwrap();
        assert_eq!(imported.memory.status, "proposed");
        assert_eq!(imported.memory.proposal.body, m.proposal.body);
        crate::o2::tests::activate(target.path(), &imported.memory, None);
        // An old preview must not hide a local review; renewed preview/replay never resets it.
        assert!(import(
            target.path(),
            &input,
            plan["checksum"].as_str().unwrap(),
            true
        )
        .is_err());
        let plan = import_preview(target.path(), &input).unwrap();
        import(
            target.path(),
            &input,
            plan["checksum"].as_str().unwrap(),
            true,
        )
        .unwrap();
        assert_eq!(
            get(target.path(), id, &Access::default())
                .unwrap()
                .memory
                .status,
            "active"
        );
    }
}
#[test]
fn transfer_rejects_scope_expansion_tampering_secrets_and_stale_export() {
    let (root, m, request) = setup();
    let plan = export_preview(root.path(), &request).unwrap();
    let mut row: serde_json::Value =
        serde_json::from_str(plan["document"].as_str().unwrap().trim()).unwrap();
    row["memory"]["proposal"]["scope"] = json!("task");
    row["memory"]["proposal"]["scope_ref"] = json!("OTHER");
    let altered: Memory = serde_json::from_value(row["memory"].clone()).unwrap();
    row["checksum"] = json!(hit(altered, 0, "").unwrap().checksum);
    let mut input = ImportRequest {
        document: row.to_string(),
        access: Access::default(),
        format: Format::Jsonl,
    };
    assert!(import_preview(root.path(), &input).is_err());
    input.access.task_id = Some("OTHER".into());
    assert!(import_preview(root.path(), &input).is_ok());
    row["memory"]["proposal"]["body"] = json!("Tampered");
    input.document = row.to_string();
    assert!(import_preview(root.path(), &input).is_err());
    row["memory"]["proposal"]["body"] = json!(format!("{}{}", "sk-proj-", "A".repeat(80)));
    let altered: Memory = serde_json::from_value(row["memory"].clone()).unwrap();
    row["checksum"] = json!(hit(altered, 0, "").unwrap().checksum);
    input.document = row.to_string();
    assert!(import_preview(root.path(), &input).is_err());
    let current = get(root.path(), &m.id, &Access::default()).unwrap();
    review(
        root.path(),
        &Review {
            id: m.id,
            status: "stale".into(),
            reason: "Changed".into(),
            confirmed: true,
            expected_checksum: current.checksum,
            evidence: None,
        },
    )
    .unwrap();
    assert!(export(
        root.path(),
        &request,
        plan["checksum"].as_str().unwrap(),
        true
    )
    .is_err());
}
#[test]
fn transfer_preview_is_read_only_and_import_batch_is_atomic_and_bounded() {
    let (root, _, request) = setup();
    let before = std::fs::read(root.path().join(".macc/memory.db")).unwrap();
    let plan = export_preview(root.path(), &request).unwrap();
    let input = ImportRequest {
        document: plan["document"].as_str().unwrap().into(),
        access: Access::default(),
        format: Format::Jsonl,
    };
    import_preview(root.path(), &input).unwrap();
    assert_eq!(
        before,
        std::fs::read(root.path().join(".macc/memory.db")).unwrap()
    );
    let invalid = ImportRequest {
        document: format!("{}{{}}\n", input.document),
        ..input.clone()
    };
    assert!(import_preview(root.path(), &invalid).is_err());
    let duplicate = ImportRequest {
        document: input.document.repeat(2),
        ..input.clone()
    };
    assert!(import_preview(root.path(), &duplicate).is_err());
    let huge = ImportRequest {
        document: " ".repeat(2 * 1024 * 1024 + 1),
        ..input
    };
    assert!(import_preview(root.path(), &huge).is_err());
}
