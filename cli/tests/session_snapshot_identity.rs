//! Exercise the public JSON snapshot commands with an isolated user home.
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

fn sessions(home: &Path, root: &Path, args: &[&str], success: bool) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_macc"))
        .env("HOME", home)
        .env("USERPROFILE", home)
        .arg("--cwd")
        .arg(root)
        .args(["coordinator", "sessions"])
        .args(args)
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.success(), success, "{args:?}: {text}");
    text
}

#[test]
fn same_named_checkouts_keep_separate_snapshots_and_validate_legacy_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let first = temp.path().join("first/service");
    let second = temp.path().join("second/service");
    let live = ".macc/state/tool-sessions.json";
    for (root, id) in [(&first, "first-session"), (&second, "second-session")] {
        fs::create_dir_all(root.join(".macc/state")).unwrap();
        fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
        let data = json!({"tools":{"fixture":{"sessions":{id:{"status":"available"}}}}});
        fs::write(root.join(live), data.to_string()).unwrap();
        sessions(&home, root, &["save", "shared"], true);
    }
    for (root, id) in [(&first, "first-session"), (&second, "second-session")] {
        assert!(sessions(&home, root, &["list"], true).contains("shared"));
        fs::remove_file(root.join(live)).unwrap();
        sessions(&home, root, &["restore", "shared"], true);
        let restored: Value = serde_json::from_slice(&fs::read(root.join(live)).unwrap()).unwrap();
        let entries = restored["tools"]["fixture"]["sessions"]
            .as_object()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries.contains_key(id));
    }

    let base = home.join(".macc/sessions");
    let legacy = base.join("service/old.json");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    let old = json!({"name":"old", "project_root":first.canonicalize().unwrap(), "saved_at":"2026-01-01","tools":{}});
    fs::write(&legacy, old.to_string()).unwrap();
    assert!(sessions(&home, &first, &["list"], true).contains("old"));
    assert!(!sessions(&home, &second, &["list"], true).contains("old"));
    sessions(&home, &first, &["restore", "old", "--dry-run"], true);
    assert!(sessions(&home, &first, &["delete", "old"], false).contains("read-only"));
    let before = fs::read(second.join(live)).unwrap();
    for args in [
        vec!["restore", "old"],
        vec!["restore", "old", "--dry-run"],
        vec!["delete", "old"],
    ] {
        sessions(&home, &second, &args, false);
    }
    assert_eq!(before, fs::read(second.join(live)).unwrap());
    assert_eq!(old.to_string(), fs::read_to_string(&legacy).unwrap());

    let snapshots: Vec<Value> = fs::read_dir(base.join("by-root"))
        .unwrap()
        .map(|entry| {
            serde_json::from_slice(&fs::read(entry.unwrap().path().join("shared.json")).unwrap())
                .unwrap()
        })
        .collect();
    assert_eq!(snapshots.len(), 2);
    assert_ne!(snapshots[0]["project_id"], snapshots[1]["project_id"]);
    let foreign = snapshots
        .iter()
        .find(|s| s["project_root"] == json!(first.canonicalize().unwrap()))
        .unwrap();
    sessions(&home, &first, &["delete", "shared"], true);
    assert!(!sessions(&home, &first, &["list"], true).contains("shared"));
    sessions(&home, &second, &["restore", "shared", "--dry-run"], true);

    // A misplaced file in the new layout must also fail the ownership check.
    let destination = fs::read_dir(base.join("by-root"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("shared.json"))
        .find(|path| path.exists())
        .unwrap();
    fs::write(&destination, foreign.to_string()).unwrap();
    for action in ["restore", "delete", "save"] {
        sessions(&home, &second, &[action, "shared"], false);
        sessions(&home, &second, &[action, "../service/old"], false);
    }
    assert_eq!(
        fs::read_to_string(destination).unwrap(),
        foreign.to_string()
    );
}
