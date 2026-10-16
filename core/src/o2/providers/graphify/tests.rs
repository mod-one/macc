use super::*;
use std::os::unix::fs::PermissionsExt;
fn script(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}
fn setup(fake: bool) -> tempfile::TempDir {
    let temp = crate::o2::tests::root();
    let root = temp.path();
    let view = super::super::super::policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.code_intelligence.graphify_enabled = true;
    p.code_intelligence.routing = "adaptive".into();
    p.code_intelligence.graphify_python = "/usr/bin/python3".into();
    if fake {
        let dir = root.join(".macc/test-tools");
        std::fs::create_dir_all(&dir).unwrap();
        script(&dir.join("fake-python"), include_str!("test-python.py"));
        script(
            &dir.join("uv"),
            r#"#!/usr/bin/python3
import pathlib, shutil, sys
if '--version' in sys.argv:
    print('uv fake 1.0')
elif sys.argv[1] == 'venv':
    target=pathlib.Path(sys.argv[-1]) / 'bin'
    target.mkdir(parents=True)
    shutil.copy2(pathlib.Path(__file__).with_name('fake-python'), target / 'python')
elif sys.argv[1:3] == ['pip','install']:
    import time
    hold=pathlib.Path(__file__).with_name('hold')
    if hold.exists():
        pathlib.Path(__file__).with_name('active').touch()
        deadline=time.monotonic()+10
        while hold.exists() and time.monotonic()<deadline:
            time.sleep(0.01)
else:
    sys.exit(2)
"#,
        );
        p.code_intelligence.graphify_uv = dir.join("uv").display().to_string();
    } else {
        p.code_intelligence.graphify_uv =
            std::env::var("MACC_GRAPHIFY_TEST_UV").unwrap_or_else(|_| "uv".into());
    }
    super::super::super::policy::update(root, &view.revision, Some(p)).unwrap();
    temp
}
fn planned(root: &Path, action: &str) -> Plan {
    let Capability::Supported(plan) = super::super::plan(root, "graphify", action).unwrap() else {
        panic!()
    };
    plan
}
fn apply_plan(root: &Path, plan: &Plan) -> Result<Receipt> {
    super::super::apply(
        root,
        &plan.id,
        &digest(serde_json::to_string(plan).unwrap().as_bytes()),
    )
}
fn apply(root: &Path, action: &str) -> Receipt {
    apply_plan(root, &planned(root, action)).unwrap()
}
fn query() -> Query {
    Query {
        text: "macc_smoke".into(),
        kind: "references".into(),
        worktree: None,
    }
}
#[test]
fn managed_lifecycle_query_budget_drift_rollback_and_owned_removal() {
    let t = setup(true);
    let root = t.path();
    std::fs::write(root.join("unrelated.rs"), "// stable source\n").unwrap();
    assert!(std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "unrelated.rs"])
        .status()
        .unwrap()
        .success());
    assert!(apply_plan(root, &planned(root, "update")).is_err());
    let first = planned(root, "install");
    assert!(first.network);
    assert!(
        first.details.as_ref().unwrap()["adapter"]["download_bytes_upper_bound"]
            .as_u64()
            .unwrap()
            > 0
    );
    let installed = apply_plan(root, &first).unwrap();
    assert_eq!(installed.state, "installed");
    assert_eq!(
        std::fs::metadata(root.join(&installed.owned_paths[0]))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        apply_plan(root, &first).unwrap().owned_paths,
        installed.owned_paths
    );
    apply(root, "deploy");
    let indexed = apply(root, "index");
    assert_eq!(indexed.state, "healthy");
    let Capability::Supported(result) = super::super::query(root, &query()).unwrap() else {
        panic!()
    };
    assert_eq!(result.provider_id, "graphify");
    assert_eq!(result.hits[0].path, "contract.rs");
    assert!(crate::context::payload::json_tokens(&Capability::Supported(result)).unwrap() <= 2000);
    let comparison = super::super::compare(root, "graphify", &query()).unwrap();
    assert_eq!(
        comparison["candidate"]["result"]["data"]["provider_id"],
        "graphify"
    );
    let p = super::super::super::policy::load(root)
        .unwrap()
        .code_intelligence;
    let graph = index::load(root, indexed.index.as_deref().unwrap(), &p)
        .unwrap()
        .graph_path;
    let graph_before = std::fs::read(root.join(&graph)).unwrap();
    let updated = apply(root, "update");
    assert_ne!(
        updated.metadata.as_ref().unwrap()["runtime"],
        installed.metadata.as_ref().unwrap()["runtime"]
    );
    let restored = apply(root, "rollback");
    assert_eq!(restored.index, indexed.index);
    assert_eq!(std::fs::read(root.join(&graph)).unwrap(), graph_before);
    let mutation = root.join(".macc/test-tools/mutate-query");
    std::fs::write(&mutation, "trigger").unwrap();
    assert!(
        matches!(super::super::query(root,&query()).unwrap(),Capability::Supported(QueryResult{provider_id,..}) if provider_id=="native")
    );
    std::fs::remove_file(mutation).unwrap();
    std::fs::write(root.join("unrelated.rs"), "// stable source\n").unwrap();
    std::fs::write(root.join("contract.rs"), "// dirty source\n").unwrap();
    assert!(apply_plan(root, &planned(root, "deploy")).is_err());
    assert!(matches!(
        super::super::query(root, &query()).unwrap(),
        Capability::Supported(QueryResult{provider_id,..}) if provider_id=="native"
    ));
    let found = super::super::query(
        root,
        &Query {
            kind: "file_search".into(),
            text: "dirty source".into(),
            worktree: None,
        },
    )
    .unwrap();
    assert!(
        matches!(found,Capability::Supported(QueryResult{provider_id,..}) if provider_id=="native")
    );
    std::fs::write(root.join("contract.rs"), "pub fn preserve_contract() {}\n").unwrap();
    let runtime = installed.metadata.as_ref().unwrap()["runtime"]
        .as_str()
        .unwrap();
    std::fs::write(root.join(runtime).join("user-notes.txt"), "preserve me").unwrap();
    let removal = planned(root, "uninstall");
    assert!(removal.affected_paths.contains(&runtime.to_string()));
    let removed = apply_plan(root, &removal).unwrap();
    assert_eq!(removed.state, "missing");
    assert_eq!(
        std::fs::read_to_string(root.join(runtime).join("user-notes.txt")).unwrap(),
        "preserve me"
    );
    assert!(root.join(".macc/memory.db").exists());
    assert_eq!(
        super::super::operation(root, &removal.id).unwrap()["status"],
        "complete"
    );
}
#[test]
fn reviewed_policy_and_source_changes_refuse_activation() {
    let t = setup(true);
    let root = t.path();
    let plan = planned(root, "install");
    let view = super::super::super::policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.code_intelligence.graphify_memory_mb += 1;
    super::super::super::policy::update(root, &view.revision, Some(p)).unwrap();
    assert!(apply_plan(root, &plan).is_err());
    let plan = planned(root, "install");
    std::fs::write(root.join("contract.rs"), "changed").unwrap();
    assert!(apply_plan(root, &plan).is_err());
    assert!(super::super::inspect(root).unwrap().receipts.is_empty());
}
#[test]
fn runtime_corruption_and_busy_provider_retain_native_fallback() {
    let t = setup(true);
    let root = t.path();
    apply(root, "install");
    apply(root, "deploy");
    let receipt = apply(root, "index");
    let lock = storage::owned(root, ".macc/code-intelligence/graphify/operation.lock").unwrap();
    let guard = crate::fs_lock::AdvisoryLock::try_acquire(&lock, "test")
        .unwrap()
        .unwrap();
    assert!(matches!(
        super::super::query(root, &query()).unwrap(),
        Capability::Supported(QueryResult{provider_id,..}) if provider_id=="native"
    ));
    drop(guard);
    let meta = receipt.metadata.unwrap();
    let mut stale = meta.clone();
    stale["lock_sha256"] = json!("old lock");
    assert!(runtime::verify(root, &stale).is_err());
    std::fs::write(runtime::python(root, &meta).unwrap(), "corrupt").unwrap();
    assert!(runtime::verify(root, &meta).is_err());
    assert!(matches!(
        super::super::query(root, &query()).unwrap(),
        Capability::Supported(QueryResult{provider_id,..}) if provider_id=="native"
    ));
    let removed = apply(root, "uninstall");
    assert_eq!(removed.state, "missing");
    assert!(runtime::python(root, &meta).unwrap().exists());
}
#[test]
#[ignore = "Explicit isolated network smoke: installs pinned wheels only inside a disposable /tmp project"]
fn real_pinned_graphify_code_only_smoke() {
    let t = setup(false);
    let kept = t.keep();
    let root = kept.as_path();
    std::fs::write(
        root.join("contract.rs"),
        "pub fn preserve_contract() {}\npub fn caller() { preserve_contract(); }\n",
    )
    .unwrap();
    apply(root, "install");
    apply(root, "deploy");
    let receipt = apply(root, "index");
    let q = Query {
        text: "preserve_contract".into(),
        kind: "references".into(),
        worktree: None,
    };
    let Capability::Supported(result) = super::super::query(root, &q).unwrap() else {
        panic!("real graph unavailable")
    };
    assert_eq!(result.provider_id, "graphify", "{result:?}");
    assert!(result
        .hits
        .iter()
        .any(|h| h.path == "contract.rs" && h.line == 1));
    assert!(result
        .hits
        .iter()
        .any(|h| h.text.contains("--calls [EXTRACTED")));
    let missing = Query {
        text: "macc_symbol_not_present_4821".into(),
        kind: "references".into(),
        worktree: None,
    };
    assert!(
        matches!(super::super::query(root,&missing).unwrap(),Capability::Supported(QueryResult{provider_id,..}) if provider_id=="native")
    );
    let comparison = super::super::compare(root, "graphify", &q).unwrap();
    let report = root.join(".macc/graphify-ab.json");
    std::fs::write(report, serde_json::to_vec_pretty(&comparison).unwrap()).unwrap();
    assert_eq!(receipt.state, "healthy");
    apply(root, "uninstall");
}

#[test]
fn offline_install_and_changed_tool_fingerprint_are_rejected_before_installation() {
    let t = setup(true);
    let root = t.path();
    let plan = planned(root, "install");
    // Use the canonical configuration path, never a provider-specific network bypass.
    let path = crate::ProjectPaths::from_root(root).config_path;
    let original = std::fs::read(&path).unwrap();
    let mut config = crate::load_canonical_config(&path).unwrap();
    config.settings.offline = true;
    std::fs::write(&path, serde_yaml::to_string(&config).unwrap()).unwrap();
    assert!(apply_plan(root, &plan)
        .unwrap_err()
        .to_string()
        .contains("offline"));
    std::fs::write(&path, original).unwrap();
    let uv = root.join(".macc/test-tools/uv");
    let mut script = std::fs::read_to_string(&uv).unwrap();
    script.push_str("\n# changed launcher\n");
    std::fs::write(uv, script).unwrap();
    assert!(apply_plan(root, &plan).is_err());
    assert!(super::super::inspect(root).unwrap().receipts.is_empty());
}

#[test]
fn external_install_does_not_hold_session_authority() {
    let t = setup(true);
    let root = t.path();
    let plan = planned(root, "install");
    let tools = root.join(".macc/test-tools");
    std::fs::write(tools.join("hold"), b"hold").unwrap();
    let project = root.to_path_buf();
    let worker = std::thread::spawn(move || apply_plan(&project, &plan));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !tools.join("active").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let observed = tools.join("active").exists();
    let authority = crate::fs_lock::AdvisoryLock::try_acquire(
        &root.join(".macc/state/tool-sessions.json.lock"),
        "test authority",
    )
    .unwrap();
    let unlocked = authority.is_some();
    drop(authority);
    std::fs::remove_file(tools.join("hold")).unwrap();
    let result = worker.join().unwrap();
    assert!(observed, "Installer phase was never observed");
    assert!(unlocked, "Long install held the session authority lock");
    result.unwrap();
}

#[test]
fn worktree_local_ignores_are_part_of_the_snapshot_compatibility_contract() {
    let t = crate::o2::tests::root();
    let root = t.path();
    let p = super::super::super::policy::load(root)
        .unwrap()
        .code_intelligence;
    let wt = root.join(".macc/worktree/sample");
    crate::git::worktree_add_detached(root, &wt, "HEAD").unwrap();
    assert_eq!(
        index::settings(root, &p).unwrap(),
        index::settings(&wt, &p).unwrap()
    );
    std::fs::write(wt.join(".graphifyignore"), "contract.rs\n").unwrap();
    assert_eq!(
        source_snapshot(root, &p).unwrap().digest,
        source_snapshot(&wt, &p).unwrap().digest
    );
    assert_ne!(
        index::settings(root, &p).unwrap(),
        index::settings(&wt, &p).unwrap()
    );
}

#[test]
fn offline_launcher_blocks_exec_fork_network_and_external_writes() {
    let t = tempfile::tempdir().unwrap();
    let prefix = runtime::LAUNCHER
        .split("sys.addaudithook(audit)")
        .next()
        .unwrap();
    let code = format!(
        r#"{prefix}
sys.addaudithook(audit)
checks = [lambda: os.execv('/bin/true', ['/bin/true']), lambda: os.fork(), lambda: __import__('socket').socket(), lambda: open('/tmp/macc-forbidden-provider-write', 'w')]
for check in checks:
    try:
        check()
    except PermissionError:
        continue
    raise AssertionError('Audit boundary escaped')
assert os.environ['PYTHONHASHSEED'] == '0'
print('audit-boundary-ok')
"#
    );
    let text = super::super::process::run(
        Path::new("/usr/bin/python3"),
        &["-B".into(), "-I".into(), "-c".into(), code],
        t.path(),
        5,
        512,
        10000,
    )
    .unwrap();
    assert!(text.contains("audit-boundary-ok"));
}
