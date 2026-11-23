//! Round-trip the configured coordinator registry through the public save/restore commands.
use macc_core::coordinator::state::{
    coordinator_state_registry_load, coordinator_state_registry_save,
};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, process::Command};

#[test]
fn state_only_bundle_restores_tasks_and_dependencies_in_a_fresh_project() {
    for (source_mode, target_mode) in [("sqlite", "sqlite"), ("sqlite", "json"), ("json", "sqlite")]
    {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let home = temp.path().join("home");
        for (root, mode) in [(&source, source_mode), (&target, target_mode)] {
            fs::create_dir_all(root.join(".macc")).unwrap();
            assert!(Command::new("git")
                .current_dir(root)
                .args(["init", "-q"])
                .status()
                .unwrap()
                .success());
            fs::write(root.join(".macc/macc.yaml"), format!("tools:\n  enabled: []\nautomation:\n  coordinator:\n    storage_mode: {mode}\n")).unwrap();
        }
        let args = |mode: &str| BTreeMap::from([("storage-mode".to_string(), mode.to_string())]);
        let registry = json!({"resource_locks":{"shared":{"task_id":"NEXT","worktree_path":null}}, "tasks":[{"id":"BASE","state":"merged"},{"id":"NEXT","state":"todo","dependencies":["BASE"]}]});
        coordinator_state_registry_save(&source, &args(source_mode), &registry).unwrap();
        let mirror = source.join(".macc/automation/task/task_registry.json");
        let run = |root: &Path, args: &[&str]| {
            let out = Command::new(env!("CARGO_BIN_EXE_macc"))
                .current_dir(root)
                .env("HOME", &home)
                .env("USERPROFILE", &home)
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        if source_mode == "sqlite" {
            assert!(!mirror.exists());
            if target_mode == "json" {
                fs::create_dir_all(mirror.parent().unwrap()).unwrap();
                fs::write(&mirror, r#"{"tasks":[]}"#).unwrap();
            }
        }
        run(
            &source,
            &[
                "save",
                "registry",
                "--only",
                "automation_state",
                "--no-sessions",
                "--offline",
            ],
        );
        let bundle = home.join(".macc/saves/registry");
        let manifest: macc_core::save::SaveBundleManifest =
            serde_yaml::from_slice(&fs::read(bundle.join("manifest.yaml")).unwrap()).unwrap();
        assert_eq!(manifest.version, 4);
        assert!(manifest.includes.automation_state);
        assert!(!manifest.includes.coordinator_sessions);
        let payload = bundle.join(manifest.paths.task_registry.unwrap());
        assert_eq!(
            macc_core::save::compute_file_sha256(&payload).unwrap(),
            manifest.hashes.task_registry.unwrap()
        );
        run(&target, &["restore", "registry", "--yes", "--offline"]);
        let restored = coordinator_state_registry_load(&target, &args(target_mode)).unwrap();
        let tasks = restored["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(restored["resource_locks"]["shared"]["task_id"], "NEXT");
        for task in registry["tasks"].as_array().unwrap() {
            let actual = tasks.iter().find(|t| t["id"] == task["id"]).unwrap();
            assert_eq!(actual["state"], task["state"]);
            if task["id"] == "NEXT" {
                assert_eq!(actual["dependencies"], task["dependencies"]);
            }
        }
        if target_mode == "sqlite" {
            assert!(!target
                .join(".macc/automation/task/task_registry.json")
                .exists());
        }
    }
}
