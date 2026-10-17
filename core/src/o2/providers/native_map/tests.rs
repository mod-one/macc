use super::*;
#[test]
fn capped_symbol_queries_disclose_omitted_alternatives_without_false_warnings() {
    let t = root();
    let r = t.path();
    let view = crate::o2::policy::inspect(r).unwrap();
    let mut p = view.policy;
    p.code_intelligence.query_limit = 1;
    crate::o2::policy::update(r, &view.revision, Some(p)).unwrap();
    let result = query_at(r, "send", None);
    assert_eq!(result.hits.len(), 1);
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.contains("Output bounded")));
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.contains("map_truncated=false")));
    let unique = query_at(r, "send", Some("sample.py"));
    assert_eq!(unique.hits.len(), 1);
    assert!(!unique
        .diagnostics
        .iter()
        .any(|d| d.contains("Output bounded")));
    assert!(!query_at(r, "missing", None)
        .diagnostics
        .iter()
        .any(|d| d.contains("Output bounded")));
}
fn root() -> tempfile::TempDir {
    let t = crate::o2::tests::root();
    for (path, source) in [
        (
            "sample.rs",
            "pub struct User {}\nimpl User { pub fn send(&self, value:u32)->u32 { value } }\n",
        ),
        (
            "sample.py",
            "class User:\n    def send(self, value: str) -> str:\n        return value\n",
        ),
        (
            "sample.ts",
            "export class User { send(value: string): string { return value; } }\n",
        ),
        (
            "sample.tsx",
            "export const App = (props: {name: string}) => <div>{props.name}</div>;\n",
        ),
    ] {
        std::fs::write(t.path().join(path), source).unwrap();
    }
    crate::git::run_git_output_mapped(
        t.path(),
        &["add", "sample.rs", "sample.py", "sample.ts", "sample.tsx"],
        "Track grammar fixtures",
    )
    .unwrap();
    let view = crate::o2::policy::inspect(t.path()).unwrap();
    let mut p = view.policy;
    p.code_intelligence.native_map_enabled = true;
    crate::o2::policy::update(t.path(), &view.revision, Some(p)).unwrap();
    t
}
fn policy(root: &Path) -> crate::o2::policy::CodePolicy {
    crate::o2::policy::load(root).unwrap().code_intelligence
}
fn operation(root: &Path, action: &str) -> Receipt {
    let Capability::Supported(plan) = super::super::plan(root, "native", action).unwrap() else {
        panic!("plan")
    };
    super::super::apply(
        root,
        &plan.id,
        &digest(serde_json::to_string(&plan).unwrap().as_bytes()),
    )
    .unwrap()
}
fn query_at(root: &Path, text: &str, path: Option<&str>) -> QueryResult {
    let Capability::Supported(r) = super::super::query(
        root,
        &Query {
            text: text.into(),
            kind: "symbol_signature".into(),
            path: path.map(str::to_string),
            worktree: None,
        },
    )
    .unwrap() else {
        panic!("query")
    };
    r
}
#[test]
fn grammars_extract_exact_qualified_declarations_with_locations_and_signatures() {
    let t = root();
    let r = t.path();
    for path in ["sample.rs", "sample.py", "sample.ts"] {
        let result = query_at(r, "User/send", Some(path));
        assert_eq!(result.hits.len(), 1, "{path}: {:?}", result.diagnostics);
        let h = &result.hits[0];
        assert_eq!(h.path, path);
        assert!(h.line > 0);
        assert!(h.text.contains("value"));
        assert!(h.provenance.contains("EXTRACTED"));
    }
    assert_eq!(query_at(r, "send", None).hits.len(), 3);
    assert_eq!(query_at(r, "/send", None).hits.len(), 0);
    assert_eq!(query_at(r, "App", Some("sample.tsx")).hits.len(), 1);
    assert_eq!(query_at(r, "missing", None).hits.len(), 0);
}
#[test]
fn incremental_reuse_deletions_and_renames_are_deterministic() {
    let t = root();
    let r = t.path();
    let p = policy(r);
    let first = build(r, &p, None).unwrap();
    assert_eq!(first.stats.parsed_files, 5);
    let next = build(r, &p, Some(&first)).unwrap();
    assert_eq!(next.stats.parsed_files, 0);
    assert_eq!(next.stats.reused_files, 5);
    assert_eq!(
        serde_json::to_value(&first.entries).unwrap(),
        serde_json::to_value(&next.entries).unwrap()
    );
    std::fs::write(
        r.join("sample.py"),
        "def renamed(value: int) -> int:\n    return value\n",
    )
    .unwrap();
    std::fs::remove_file(r.join("sample.ts")).unwrap();
    let delta = build(r, &p, Some(&first)).unwrap();
    assert_eq!(delta.stats.parsed_files, 1);
    assert_eq!(delta.stats.reused_files, 3);
    assert_eq!(delta.stats.removed_files, 1);
    assert!(!delta.entries.contains_key("sample.ts"));
    assert_eq!(delta.entries["sample.py"].symbols[0].name, "renamed");
}
#[test]
fn lifecycle_publishes_incremental_receipts_and_overlay_never_writes_base() {
    let t = root();
    let r = t.path();
    operation(r, "deploy");
    let first = operation(r, "index");
    let first_path = r.join(first.index.as_ref().unwrap());
    let raw = std::fs::read(&first_path).unwrap();
    let second = operation(r, "update");
    assert_eq!(
        second.metadata.as_ref().unwrap()["stats"]["parsed_files"],
        0
    );
    assert_eq!(
        second.metadata.as_ref().unwrap()["stats"]["reused_files"],
        5
    );
    std::fs::write(r.join("sample.py"), "def other():\n    return 1\n").unwrap();
    assert_eq!(query_at(r, "User/send", Some("sample.py")).hits.len(), 0);
    assert_eq!(query_at(r, "other", None).hits.len(), 1);
    assert_eq!(std::fs::read(&first_path).unwrap(), raw);
    let (db, _) = readonly_database(r).unwrap();
    assert_eq!(
        super::super::receipt(&db, "native").unwrap().unwrap().index,
        second.index
    );
    drop(db);
    let third = operation(r, "update");
    assert_eq!(third.metadata.as_ref().unwrap()["stats"]["parsed_files"], 1);
    operation(r, "rollback");
    assert_eq!(
        query_at(r, "other", None).hits.len(),
        1,
        "rollback must not return stale base symbols"
    );
    operation(r, "uninstall");
    assert!(!first_path.exists());
    assert!(r.join("sample.py").exists());
}
#[test]
fn cache_corruption_and_legacy_artifacts_rebuild_without_stale_hits() {
    let t = root();
    let r = t.path();
    operation(r, "deploy");
    let receipt = operation(r, "index");
    std::fs::write(r.join(receipt.index.as_ref().unwrap()), "corrupt").unwrap();
    let result = query_at(r, "send", None);
    assert_eq!(result.hits.len(), 3);
    assert!(result.diagnostics.iter().any(|d| d.contains("checksum")));
    operation(r, "update");
    assert_eq!(query_at(r, "send", None).hits.len(), 3);
}
#[test]
fn parse_errors_limits_and_language_flags_never_claim_partial_success() {
    let t = root();
    let r = t.path();
    let mut p = policy(r);
    std::fs::write(r.join("sample.py"), "def broken(:\n").unwrap();
    let map = build(r, &p, None).unwrap();
    assert!(map.entries["sample.py"].symbols.is_empty());
    assert!(map.entries["sample.py"].diagnostic.is_some());
    p.native_symbols_per_file = 1;
    let map = build(r, &p, None).unwrap();
    assert!(map.entries["sample.rs"].symbols.is_empty());
    assert!(map.stats.failed_files >= 2);
    p.native_rust = false;
    p.native_python = false;
    p.native_typescript = false;
    p.native_tsx = false;
    assert!(build(r, &p, None).unwrap().entries.is_empty());
    p.native_map_enabled = false;
    assert!(matches!(
        query(
            r,
            &Query {
                text: "send".into(),
                kind: "symbol_definition".into(),
                path: None,
                worktree: None
            },
            &p
        )
        .unwrap(),
        Capability::Unsupported { .. }
    ));
}
#[test]
fn configuration_catalog_covers_all_native_controls_and_binds_cached_settings() {
    let t = root();
    let r = t.path();
    let mut p = policy(r);
    operation(r, "deploy");
    let receipt = operation(r, "index");
    assert!(load(r, Some(&receipt), &p).unwrap().is_some());
    p.native_python = false;
    assert!(load(r, Some(&receipt), &p).unwrap().is_none());
    let all = crate::o2::policy::fields();
    for key in [
        "native_map_enabled",
        "native_incremental",
        "native_rust",
        "native_python",
        "native_typescript",
        "native_tsx",
        "native_corpus_max_bytes",
        "native_index_max_bytes",
        "native_symbols_per_file",
        "native_parse_timeout_ms",
        "native_build_timeout_ms",
    ] {
        assert!(all
            .iter()
            .any(|f| f.path == format!("code_intelligence.{key}")));
    }
    let mut invalid = crate::o2::policy::Policy::default();
    invalid.code_intelligence.native_symbols_per_file = 0;
    assert!(invalid.validate().is_err());
}
#[test]
fn private_untracked_symlink_and_foreign_worktrees_are_excluded() {
    let t = root();
    let r = t.path();
    std::fs::write(r.join("untracked.py"), "def hidden(): pass\n").unwrap();
    std::fs::write(r.join(".env.py"), "def hidden(): pass\n").unwrap();
    crate::git::run_git_output_mapped(r, &["add", "-f", ".env.py"], "Track restricted fixture")
        .unwrap();
    let map = build(r, &policy(r), None).unwrap();
    assert!(!map.entries.contains_key("untracked.py"));
    assert!(!map.entries.contains_key(".env.py"));
    let other = crate::o2::tests::root();
    assert!(super::super::query(
        r,
        &Query {
            text: "send".into(),
            kind: "symbol_definition".into(),
            path: None,
            worktree: Some(other.path().display().to_string())
        }
    )
    .is_err());
}

#[test]
fn incremental_gold_corpus_preserves_answers_without_reparsing_unchanged_files() {
    let t = root();
    let r = t.path();
    for i in 0..128 {
        let path = format!("gold_{i:03}.py");
        std::fs::write(r.join(&path),format!("# fake declaration target_{i}\ndef target_{i}(value: int) -> int:\n    return value\n")).unwrap();
        crate::git::run_git_output_mapped(r, &["add", &path], "Track accuracy corpus").unwrap();
    }
    let p = policy(r);
    let full = build(r, &p, None).unwrap();
    let cached = build(r, &p, Some(&full)).unwrap();
    assert_eq!(full.entries.len(), 133);
    assert_eq!(cached.stats.reused_files, 133);
    assert_eq!(cached.stats.parsed_files, 0);
    assert_eq!(
        serde_json::to_value(&full.entries).unwrap(),
        serde_json::to_value(&cached.entries).unwrap()
    );
    for i in 0..128 {
        let file = &full.entries[&format!("gold_{i:03}.py")];
        assert_eq!(file.symbols.len(), 1);
        assert_eq!(file.symbols[0].name, format!("target_{i}"));
        assert_eq!(file.symbols[0].line, 2);
    }
    tracing::info!("P3-B deterministic gold corpus: 128/128 exact definitions at expected locations; full_parsed={}, incremental_parsed={}, reused={}, full_ms={}, incremental_ms={}",full.stats.parsed_files,cached.stats.parsed_files,cached.stats.reused_files,full.stats.elapsed_ms,cached.stats.elapsed_ms);
}
#[test]
fn syntax_queries_do_not_claim_calls_comments_or_string_literals_as_definitions() {
    let t = root();
    let r = t.path();
    std::fs::write(
        r.join("sample.py"),
        "# def imaginary():\ntext = 'def imaginary():'\ndef actual():\n    imaginary()\n",
    )
    .unwrap();
    assert!(query_at(r, "imaginary", None).hits.is_empty());
    assert_eq!(query_at(r, "actual", None).hits.len(), 1);
    let mut p = policy(r);
    p.native_corpus_max_bytes = 1;
    let map = build(r, &p, None).unwrap();
    assert!(map.symbols_truncated);
    assert!(map.entries.is_empty());
}
#[test]
#[cfg(unix)]
fn tracked_source_and_cache_symlinks_fail_closed() {
    let t = root();
    let r = t.path();
    std::os::unix::fs::symlink(r.join("sample.py"), r.join("alias.py")).unwrap();
    crate::git::run_git_output_mapped(r, &["add", "alias.py"], "Track symlink fixture").unwrap();
    assert!(!build(r, &policy(r), None)
        .unwrap()
        .entries
        .contains_key("alias.py"));
    operation(r, "deploy");
    let receipt = operation(r, "index");
    let path = r.join(receipt.index.as_ref().unwrap());
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(r.join("sample.py"), &path).unwrap();
    assert!(load(r, Some(&receipt), &policy(r)).is_err());
    assert_eq!(query_at(r, "send", None).hits.len(), 3);
}

#[test]
fn same_project_worktree_overlays_do_not_leak_base_only_symbols_or_write_receipts() {
    let t = root();
    let r = t.path();
    operation(r, "deploy");
    let receipt = operation(r, "index");
    let dir = tempfile::tempdir().unwrap();
    let worktree = dir.path().join("worker");
    crate::git::run_git_output_mapped(
        r,
        &[
            "worktree",
            "add",
            "--detach",
            worktree.to_str().unwrap(),
            "HEAD",
        ],
        "Create native overlay fixture",
    )
    .unwrap();
    std::fs::write(
        worktree.join("sample.py"),
        "def branch_only():\n    return 7\n",
    )
    .unwrap();
    crate::git::run_git_output_mapped(&worktree, &["add", "sample.py"], "Track overlay source")
        .unwrap();
    let run = |text: &str| {
        let Capability::Supported(value) = super::super::query(
            r,
            &Query {
                text: text.into(),
                kind: "symbol_definition".into(),
                path: None,
                worktree: Some(worktree.display().to_string()),
            },
        )
        .unwrap() else {
            panic!("overlay query")
        };
        value
    };
    assert_eq!(run("branch_only").hits.len(), 1);
    assert!(run("send").hits.is_empty());
    assert_eq!(query_at(r, "send", None).hits.len(), 3);
    let (db, _) = readonly_database(r).unwrap();
    let current = super::super::receipt(&db, "native").unwrap();
    assert_eq!(
        super::super::checksum(&current).unwrap(),
        super::super::checksum(&Some(receipt)).unwrap()
    );
}

#[test]
fn generic_rust_implementation_uses_the_declared_type_name() {
    let t = root();
    let r = t.path();
    std::fs::write(
        r.join("sample.rs"),
        "pub struct Boxed<T> {value:T}\nimpl<T> Boxed<T> {pub fn get(&self)->&T{&self.value}}\n",
    )
    .unwrap();
    let result = query_at(r, "Boxed/get", Some("sample.rs"));
    assert_eq!(result.hits.len(), 1);
    assert!(result.hits[0].text.contains("get(&self)"));
}
