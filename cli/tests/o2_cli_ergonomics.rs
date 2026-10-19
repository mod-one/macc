//! Public-client regression of the workflow observed in familytree_test_ai.
use serde_json::{json, Value};
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
fn cli(root: &Path, args: &[&str], input: Option<&Value>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    child.wait_with_output().unwrap()
}
fn value(result: Output) -> Value {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join(".macc/state")).unwrap();
    std::fs::write(
        t.path().join(".macc/macc.yaml"),
        "tools:\n  enabled: []\nsettings:\n  offline: true\n",
    )
    .unwrap();
    std::fs::write(t.path().join(".gitignore"), ".macc/\n").unwrap();
    std::fs::write(t.path().join("contract.rs"), "pub fn contract() {}\n").unwrap();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.name", "O2 Test"],
        vec!["config", "user.email", "o2@example.invalid"],
        vec!["add", "."],
        vec!["commit", "-qm", "Test baseline"],
    ] {
        let r = Command::new("git")
            .arg("-C")
            .arg(t.path())
            .args(args)
            .output()
            .unwrap();
        assert!(r.status.success());
    }
    t
}
#[test]
fn single_field_preview_confirmation_and_revision_work_through_public_cli() {
    let t = fixture();
    let root = t.path();
    let path = root.join(".macc/macc.yaml");
    let bytes = std::fs::read(&path).unwrap();
    let preview = value(cli(
        root,
        &["o2", "policy", "set", "memory.enabled", "true"],
        None,
    ));
    assert_eq!(preview["before"], false);
    assert_eq!(preview["after"], true);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let denied = cli(
        root,
        &["o2", "policy", "set", "memory.enabled", "true", "--apply"],
        None,
    );
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("--confirm --revision"));
    assert!(!cli(
        root,
        &[
            "o2",
            "policy",
            "set",
            "memory.enabled",
            "true",
            "--apply",
            "--confirm"
        ],
        None
    )
    .status
    .success());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let revision = preview["revision"].as_str().unwrap();
    value(cli(
        root,
        &[
            "o2",
            "policy",
            "set",
            "memory.enabled",
            "true",
            "--apply",
            "--confirm",
            "--revision",
            revision,
        ],
        None,
    ));
    let current = value(cli(root, &["o2", "policy", "get", "memory.enabled"], None));
    assert_eq!(current["value"], true);
    assert_ne!(current["revision"], revision);
    assert!(!cli(
        root,
        &[
            "o2",
            "policy",
            "set",
            "memory.enabled",
            "false",
            "--apply",
            "--confirm",
            "--revision",
            revision
        ],
        None
    )
    .status
    .success());
    assert_eq!(
        value(cli(root, &["o2", "policy", "get", "memory.enabled"], None))["value"],
        true
    );
    let list = value(cli(root, &["o2", "policy", "list"], None));
    assert!(list["fields"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["path"] == "memory.enabled"));
}
#[test]
fn malformed_edits_and_ambiguous_search_modes_are_rejected() {
    let t = fixture();
    let root = t.path();
    let bytes = std::fs::read(root.join(".macc/macc.yaml")).unwrap();
    for (field, input) in [
        ("memory.enabled", "yes"),
        ("memory.search_limit", "1.5"),
        ("memory.typo", "true"),
    ] {
        assert!(!cli(root, &["o2", "policy", "set", field, input], None)
            .status
            .success());
    }
    assert!(!cli(
        root,
        &["o2", "memory-search", "contract", "--explain", "--full"],
        None
    )
    .status
    .success());
    assert!(!cli(
        root,
        &[
            "o2",
            "memory-search",
            "contract",
            "--explain",
            "--cursor",
            "1"
        ],
        None
    )
    .status
    .success());
    assert_eq!(std::fs::read(root.join(".macc/macc.yaml")).unwrap(), bytes);
}
#[test]
fn public_search_explains_source_drift_and_respects_task_scope() {
    let t = fixture();
    let root = t.path();
    let migration = value(cli(root, &["session-store", "migrate"], None));
    value(cli(
        root,
        &[
            "session-store",
            "migrate",
            "--apply",
            "--expected-sha256",
            migration["source_sha256"].as_str().unwrap(),
        ],
        None,
    ));
    let preview = value(cli(
        root,
        &["o2", "policy", "set", "memory.enabled", "true"],
        None,
    ));
    value(cli(
        root,
        &[
            "o2",
            "policy",
            "set",
            "memory.enabled",
            "true",
            "--apply",
            "--confirm",
            "--revision",
            preview["revision"].as_str().unwrap(),
        ],
        None,
    ));
    let evidence = value(cli(root, &["o2", "memory-evidence", "contract.rs"], None));
    let proposal = json!({"kind":"constraint","title":"Preserve contract","body":"Keep the contract source compatible.","scope":"task","scope_ref":"TASK-A","confidence":1.0,"evidence":[evidence],"tags":[],"source_task":"TASK-A","source_revision":null});
    let m = value(cli(root, &["o2", "memory-propose"], Some(&proposal)));
    let id = m["id"].as_str().unwrap();
    let proposed = value(cli(
        root,
        &[
            "o2",
            "memory-search",
            "contract",
            "--task",
            "TASK-A",
            "--explain",
        ],
        None,
    ));
    assert!(proposed["exclusions"][0]
        .as_str()
        .unwrap()
        .contains("status=proposed"));
    let inspect = value(cli(
        root,
        &["o2", "memory-get", id, "--task", "TASK-A", "--full"],
        None,
    ));
    value(cli(
        root,
        &["o2", "memory-review"],
        Some(
            &json!({"id":id,"status":"active","reason":"Inspected source contract","confirmed":true,"expected_checksum":inspect["checksum"]}),
        ),
    ));
    let source = root.join("contract.rs");
    let bytes = std::fs::read(&source).unwrap();
    std::fs::write(&source, b"pub fn changed_contract() {}\n").unwrap();
    let normal = value(cli(
        root,
        &["o2", "memory-search", "contract", "--task", "TASK-A"],
        None,
    ));
    assert_eq!(
        normal
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "estimated_tokens",
            "limit_tokens",
            "next_cursor",
            "records",
            "references"
        ]
    );
    assert_eq!(normal["records"], json!([]));
    let explained = value(cli(
        root,
        &[
            "o2",
            "memory-search",
            "contract",
            "--task",
            "TASK-A",
            "--explain",
        ],
        None,
    ));
    assert!(explained["exclusions"].as_array().unwrap().iter().any(|e| {
        let s = e.as_str().unwrap();
        s.contains(id) && s.contains("snapshot_mismatch")
    }));
    let other = value(cli(
        root,
        &[
            "o2",
            "memory-search",
            "contract",
            "--task",
            "TASK-B",
            "--explain",
        ],
        None,
    ));
    assert_eq!(other["exclusions"], json!([]));
    assert_eq!(other["records"], json!([]));
    std::fs::write(&source, bytes).unwrap();
    let restored = value(cli(
        root,
        &[
            "o2",
            "memory-search",
            "contract",
            "--task",
            "TASK-A",
            "--explain",
        ],
        None,
    ));
    assert_eq!(restored["records"][0]["memory"]["id"], id);
    assert_eq!(restored["exclusions"], json!([]));
}

#[cfg(unix)]
#[test]
fn terminal_confirmation_defaults_to_cancel_and_accepts_explicit_yes() {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::{io::Read, sync::mpsc, time::Duration};
    for (answer, expected) in [("\n", false), ("n\n", false), ("yes\n", true)] {
        let t = fixture();
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_macc"));
        command.args([
            "--cwd",
            t.path().to_str().unwrap(),
            "o2",
            "policy",
            "set",
            "memory.enabled",
            "true",
            "--apply",
        ]);
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut all = Vec::new();
            let mut b = [0; 4096];
            loop {
                match reader.read(&mut b) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => all.extend_from_slice(&b[..n]),
                }
            }
            let _ = tx.send(all);
        });
        pair.master
            .take_writer()
            .unwrap()
            .write_all(answer.as_bytes())
            .unwrap();
        let output = match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(v) => v,
            Err(e) => {
                let _ = child.kill();
                panic!("Confirmation did not finish: {e}")
            }
        };
        assert!(child.wait().unwrap().success());
        assert!(String::from_utf8_lossy(&output).contains("Apply this O2 setting? [y/N]"));
        assert_eq!(
            value(cli(
                t.path(),
                &["o2", "policy", "get", "memory.enabled"],
                None
            ))["value"],
            expected
        );
    }
}
