//! Exercise PRD-only bundles through the public CLI in isolated projects.
use macc_core::save::{compute_file_sha256, SaveBundleManifest};
use std::{fs, path::Path, process::Command};

#[test]
fn prd_queue_round_trips_exact_bytes_and_rejects_an_incomplete_queue() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    for root in [&source, &target] {
        fs::create_dir_all(root).unwrap();
        assert!(Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
    }
    fs::create_dir(target.join(".macc")).unwrap();
    let target_config = "tools:\n  enabled: []\n";
    fs::write(target.join(".macc/macc.yaml"), target_config).unwrap();
    let prds = [
        (
            "docs/prd/z/spec.json",
            "{ \"tasks\": [{\"id\":\"BASE\"}] }\r\n",
        ),
        (
            "docs/prd/a  b/spec.json",
            "{\"tasks\":[{\"id\":\"NEXT\",\"dependencies\":[\"BASE\"]}]}\n",
        ),
    ];
    for (path, bytes) in prds {
        let file = source.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, bytes).unwrap();
    }
    fs::create_dir_all(source.join(".macc/automation/task")).unwrap();
    fs::write(source.join(".macc/automation/task/decoy.json"), "{}").unwrap();
    let source_config = format!(
        "tools:\n  enabled: []\nautomation:\n  coordinator:\n    prd_files:\n      - {}\n      - {}\n",
        prds[0].0, prds[1].0
    );
    fs::write(source.join(".macc/macc.yaml"), source_config).unwrap();
    let run = |root: &Path, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .current_dir(root)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .args(args)
            .output()
            .unwrap()
    };
    let save = ["save", "queue", "--only", "prd", "--offline", "--overwrite"];
    let saved = run(&source, &save);
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    let bundle = home.join(".macc/saves/queue");
    let manifest: SaveBundleManifest =
        serde_yaml::from_slice(&fs::read(bundle.join("manifest.yaml")).unwrap()).unwrap();
    assert!(manifest.includes.prd);
    assert!(!manifest.includes.config && !manifest.includes.automation_state);
    assert_eq!(manifest.prd_files.len(), prds.len());
    for (file, (path, _)) in manifest.prd_files.iter().zip(prds) {
        assert_eq!(file.path, path);
        assert_eq!(
            file.sha256,
            compute_file_sha256(&source.join(path)).unwrap()
        );
    }
    fs::remove_file(source.join(prds[1].0)).unwrap();
    let incomplete = run(&source, &save);
    assert!(!incomplete.status.success());
    assert!(String::from_utf8_lossy(&incomplete.stderr).contains(prds[1].0));
    let restored = run(&target, &["restore", "queue", "--yes", "--offline"]);
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    for (path, bytes) in prds {
        assert_eq!(fs::read(target.join(path)).unwrap(), bytes.as_bytes());
    }
    assert!(!target.join(".macc/automation/task/decoy.json").exists());
    assert_eq!(
        fs::read_to_string(target.join(".macc/macc.yaml")).unwrap(),
        target_config
    );
}
