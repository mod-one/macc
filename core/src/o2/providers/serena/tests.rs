use super::*;
fn query(text: &str, kind: &str) -> Query {
    Query {
        text: text.into(),
        kind: kind.into(),
        path: None,
        worktree: None,
    }
}
#[test]
fn defaults_catalog_and_live_routing_contract() {
    let p = CodePolicy::default();
    assert!(!p.serena_enabled);
    assert!(!Serena.requires_cached_snapshot());
    assert!(
        Serena.priority(&query("a", "references"))
            < super::super::graphify::Graphify.priority(&query("a", "references"))
    );
    assert!(
        Serena.priority(&query("a", "file_search"))
            > super::super::native::Native.priority(&query("a", "file_search"))
    );
    let defaults = serde_json::to_value(super::super::super::policy::Policy::default()).unwrap();
    for key in [
        "serena_enabled",
        "serena_python",
        "serena_uv",
        "serena_timeout_seconds",
        "serena_install_timeout_seconds",
        "serena_memory_mb",
        "serena_corpus_max_bytes",
    ] {
        assert!(super::super::super::policy::fields().iter().any(|f| f.path
            == format!("code_intelligence.{key}")
            && defaults
                .pointer(&format!("/code_intelligence/{key}"))
                .is_some()));
    }
}
#[test]
fn legacy_queries_remain_compatible_and_unknown_fields_fail() {
    let q: Query = serde_json::from_value(json!({"text":"target","worktree":null})).unwrap();
    assert!(q.path.is_none());
    assert_eq!(q.kind, "file_search");
    assert!(serde_json::from_value::<Query>(
        json!({"text":"target","worktree":null,"command":"unsafe"})
    )
    .is_err());
}
#[test]
fn citations_reject_unexported_changed_and_invalid_sources() {
    let t = crate::o2::tests::root();
    let root = t.path();
    std::fs::write(root.join("sample.py"), "def target():\n    return 1\n").unwrap();
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "sample.py"])
        .output()
        .unwrap();
    let p = CodePolicy::default();
    let snap = source_snapshot(root, &p).unwrap();
    let mut v = json!({"schema":1,"language":"python","backend":"jedi","hits":[{"path":"sample.py","line":1}]});
    let hits = query::normalize(root, &snap, &v, &p).unwrap();
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].provenance, "serena:python:jedi:LSP");
    v["hits"][0]["line"] = json!(900);
    assert!(query::normalize(root, &snap, &v, &p).is_err());
    v["hits"][0]["path"] = json!("../secret.py");
    assert!(query::normalize(root, &snap, &v, &p).is_err());
    v["hits"][0] = json!({"path":"sample.py","line":1});
    std::fs::write(root.join("sample.py"), "changed\n").unwrap();
    assert!(query::normalize(root, &snap, &v, &p).is_err());
}
#[test]
fn unsupported_capabilities_and_missing_runtime_fall_back() {
    let t = crate::o2::tests::root();
    let root = t.path();
    let p = CodePolicy::default();
    assert!(matches!(
        Serena
            .query(root, &query("target", "architecture_summary"), &p)
            .unwrap(),
        Capability::Unsupported { .. }
    ));
    assert_eq!(Serena.detect(root).unwrap().status, "disabled");
    assert!(matches!(
        super::super::plan(root, "serena", "index").unwrap(),
        Capability::Unsupported { .. }
    ));
    let result = super::super::query(root, &query("preserve_contract", "references")).unwrap();
    assert!(
        matches!(result,Capability::Supported(QueryResult{provider_id,..}) if provider_id=="native")
    );
}
fn setup(fake: bool) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let t = crate::o2::tests::root();
    let root = t.path();
    let view = super::super::super::policy::inspect(root).unwrap();
    let mut policy = view.policy;
    policy.code_intelligence.serena_enabled = true;
    policy.code_intelligence.routing = "adaptive".into();
    policy.code_intelligence.serena_python = "/usr/bin/python3".into();
    if fake {
        let dir = root.join(".macc/test-serena");
        std::fs::create_dir_all(&dir).unwrap();
        let fake_python = dir.join("python");
        std::fs::write(&fake_python,r#"#!/usr/bin/python3
import json, pathlib
w=pathlib.Path.cwd(); r=json.loads((w/'request.json').read_text())
project=next(p for p in pathlib.Path(__file__).parents if (p/'.macc/memory.db').exists())
if (project/'.macc/mutate-source').exists():
    (project/'contract.rs').write_text('changed during LSP query')
hits=[{'path':'smoke.py','line':1,'name':'macc_smoke'}] if r['text']=='macc_smoke' else [{'path':'sample.py','line':1,'name':'target'}]
(w/'response.json').write_text(json.dumps({'schema':1,'language':'python','backend':'jedi','hits':hits}))
"#).unwrap();
        let uv = dir.join("uv");
        std::fs::write(&uv,r#"#!/usr/bin/python3
import pathlib,sys,shutil
if '--version' in sys.argv: print('uv fixture 1')
elif sys.argv[1]=='venv':
 b=pathlib.Path(sys.argv[-1])/'bin';b.mkdir(parents=True);shutil.copy2(pathlib.Path(__file__).with_name('python'),b/'python')
elif sys.argv[1:3]!=['pip','install']: sys.exit(2)
"#).unwrap();
        for path in [&uv, &fake_python] {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        policy.code_intelligence.serena_uv = uv.display().to_string();
    }
    super::super::super::policy::update(root, &view.revision, Some(policy)).unwrap();
    std::fs::write(root.join("sample.py"),"def target():\n    return 42\n\ndef caller():\n    return target()\n\nclass Other:\n    def target(self):\n        return 0\n\ndef unrelated():\n    return 'target' # target is only text\n").unwrap();
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["add", "sample.py"])
        .status()
        .unwrap();
    assert!(status.success());
    t
}
fn apply(root: &Path, action: &str) -> Receipt {
    let Capability::Supported(plan) = super::super::plan(root, "serena", action).unwrap() else {
        panic!("unsupported {action}")
    };
    let checksum = digest(&serde_json::to_vec(&plan).unwrap());
    super::super::apply(root, &plan.id, &checksum).unwrap()
}
#[test]
fn live_queries_survive_source_changes_and_removal_preserves_user_files() {
    let t = setup(true);
    let root = t.path();
    assert_eq!(apply(root, "install").state, "installed");
    let r = apply(root, "deploy");
    assert_eq!(r.state, "healthy");
    assert!(r.index.is_none());
    let mut q = query("target", "symbol_definition");
    q.path = Some("sample.py".into());
    assert!(
        matches!(super::super::query(root,&q).unwrap(),Capability::Supported(QueryResult{provider_id,..}) if provider_id=="serena")
    );
    std::fs::write(root.join("sample.py"), "def target():\n    return 99\n").unwrap();
    assert!(
        matches!(super::super::query(root,&q).unwrap(),Capability::Supported(QueryResult{provider_id,..}) if provider_id=="serena")
    );
    assert_eq!(apply(root, "disable").state, "disabled");
    // Disabling Serena retains the default native symbol-query fallback.
    assert!(matches!(
        super::super::query(root, &q).unwrap(),
        Capability::Supported(QueryResult { provider_id, .. }) if provider_id == "native"
    ));
    assert_eq!(apply(root, "deploy").state, "healthy");
    let runtime = runtime::owned(
        root,
        runtime::field(r.metadata.as_ref().unwrap(), "runtime").unwrap(),
    )
    .unwrap();
    std::fs::write(runtime.join("user-note.txt"), "preserve me").unwrap();
    assert!(runtime::verify(root, r.metadata.as_ref().unwrap()).is_err());
    let removed = apply(root, "uninstall");
    assert_eq!(removed.state, "missing");
    assert!(runtime.join("user-note.txt").exists());
    assert!(!runtime.join("bridge.py").exists());
    assert!(!removed.owned_paths.is_empty());
}
#[test]
#[ignore = "Opt-in public PyPI installation of pinned Serena/Jedi into a temporary project"]
fn real_serena_symbol_accuracy_and_lifecycle() {
    let t = setup(false);
    let root = t.path();
    apply(root, "install");
    let r = apply(root, "deploy");
    let mut q = query("Other/target", "symbol_definition");
    q.path = Some("sample.py".into());
    let Capability::Supported(result) = super::super::query(root, &q).unwrap() else {
        panic!("LSP unavailable")
    };
    assert_eq!(result.provider_id, "serena");
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].line, 8);
    q.text = "/target".into();
    q.kind = "references".into();
    let Capability::Supported(result) = super::super::query(root, &q).unwrap() else {
        panic!()
    };
    assert_eq!(result.provider_id, "serena");
    assert!(result.hits.iter().any(|h| h.line == 5));
    assert!(result.hits.iter().all(|h| h.line != 8 && h.line != 12));
    q.text = "absent_symbol".into();
    let Capability::Supported(result) = super::super::query(root, &q).unwrap() else {
        panic!()
    };
    assert_eq!(result.provider_id, "serena");
    assert!(result.hits.is_empty());
    q.text = "target".into();
    let Capability::Supported(result) = super::super::query(root, &q).unwrap() else {
        panic!()
    };
    assert_eq!(result.provider_id, "native");
    assert!(result.diagnostics.iter().any(|d| d.contains("Ambiguous")));
    let runtime = runtime::owned(
        root,
        runtime::field(r.metadata.as_ref().unwrap(), "runtime").unwrap(),
    )
    .unwrap();
    apply(root, "uninstall");
    assert!(!runtime.exists());
}

#[test]
fn updates_rollback_dependency_failure_and_gitignore_are_explicit() {
    let t = setup(true);
    let root = t.path();
    apply(root, "install");
    let first = apply(root, "deploy");
    let second = apply(root, "update");
    assert_ne!(
        first.metadata.as_ref().unwrap()["runtime"],
        second.metadata.as_ref().unwrap()["runtime"]
    );
    let restored = apply(root, "rollback");
    assert_eq!(
        restored.metadata.as_ref().unwrap()["runtime"],
        first.metadata.as_ref().unwrap()["runtime"]
    );
    assert_eq!(restored.owned_paths.len(), 2);
    let mut obsolete_bridge = restored.metadata.clone().unwrap();
    obsolete_bridge["bridge_sha256"] = json!("older-bridge");
    assert!(runtime::verify(root, &obsolete_bridge).is_err());
    std::fs::write(root.join(".gitignore"), "sample.py\n").unwrap();
    let mut q = query("target", "references");
    q.path = Some("sample.py".into());
    let Capability::Supported(result) = super::super::query(root, &q).unwrap() else {
        panic!()
    };
    assert_eq!(result.provider_id, "native");
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.contains("allowed tracked Python")));
    std::fs::remove_file(root.join(".gitignore")).unwrap();
    let runtime = runtime::owned(
        root,
        runtime::field(restored.metadata.as_ref().unwrap(), "runtime").unwrap(),
    )
    .unwrap();
    std::fs::remove_file(runtime.join("venv/bin/python")).unwrap();
    let Capability::Supported(result) = super::super::query(root, &q).unwrap() else {
        panic!()
    };
    assert_eq!(result.provider_id, "native");
    assert!(result.diagnostics.iter().any(|d| d.contains("generation")));
    let p = super::super::super::policy::load(root)
        .unwrap()
        .code_intelligence;
    assert_eq!(
        Serena.health(root, Some(&restored), &p).unwrap().status,
        "degraded"
    );
}

#[test]
fn bridge_rejects_network_arbitrary_children_and_external_writes() {
    let t = tempfile::tempdir().unwrap();
    let prefix = runtime::BRIDGE.split("logging.basicConfig").next().unwrap();
    super::super::filesystem_guard_tests::mutations(prefix);
    let code = format!(
        "__file__='/managed/bridge.py'\n{prefix}\n{}",
        r#"
sys.addaudithook(audit)
import socket, subprocess
blocked=0
for action in [lambda: socket.getaddrinfo('example.com',443), lambda: subprocess.run(['/bin/true']), lambda: open('/tmp/macc-serena-forbidden-write','w')]:
    try: action()
    except PermissionError: blocked+=1
assert blocked==3
assert not is_server
"#
    );
    super::super::process::run(
        Path::new("/usr/bin/python3"),
        &["-B".into(), "-I".into(), "-c".into(), code],
        t.path(),
        5,
        512,
        65536,
    )
    .unwrap();
}

#[test]
fn changes_to_non_returned_sources_during_query_trigger_fallback() {
    let t = setup(true);
    let root = t.path();
    apply(root, "install");
    apply(root, "deploy");
    std::fs::write(root.join(".macc/mutate-source"), "").unwrap();
    let Capability::Supported(result) = super::super::query(
        root,
        &Query {
            path: Some("sample.py".into()),
            ..query("target", "references")
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(result.provider_id, "native");
    assert!(result
        .diagnostics
        .iter()
        .any(|d| d.contains("source changed during query")));
}
