use super::*;
use crate::coordinator::{
    model::{Task, TaskRegistry},
    state,
    types::CoordinatorEnvConfig,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

const CI: &str = include_str!("../../../../docs/resume-checks/ci.sql");
const ADB: &str = include_str!("../../../../docs/resume-checks/adb.sql");
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(3)
}
fn evaluate(script: &str, input: &Value) -> Outcome {
    verifier::evaluate(script, input, deadline()).unwrap()
}
#[test]
fn novel_scripts_check_identity_freshness_and_transport_state() {
    let mut input = json!({"target":"reserved","observation":[{"serial":"other","state":"device"},{"serial":"reserved","state":"unauthorized"}]});
    assert_eq!(evaluate(ADB, &input).status, Status::Waiting);
    input["observation"][1]["state"] = json!("device");
    assert_eq!(evaluate(ADB, &input).status, Status::Ready);
    let mut ci = json!({"target":"stage","commit":"abc","now_unix":7200,"observation":{"commit":"abc","environment":"stage","finished_at":7200,"result":"passed"}});
    assert_eq!(evaluate(CI, &ci).status, Status::Ready);
    for (key, value) in [
        ("commit", json!("old")),
        ("environment", json!("other")),
        ("finished_at", json!(3599)),
        ("finished_at", json!(7201)),
        ("finished_at", json!("7200")),
        ("result", json!("failed")),
    ] {
        let original = ci["observation"][key].clone();
        ci["observation"][key] = value;
        assert_eq!(evaluate(CI, &ci).status, Status::Waiting, "{key}");
        ci["observation"][key] = original;
    }
}
#[test]
fn invalid_or_unbounded_scripts_do_not_return_ready() {
    for sql in ["SELECT :input; SELECT 1","ATTACH ':memory:' AS other","SELECT load_extension(:input)","SELECT :input UNION ALL SELECT :input","SELECT json_object('status','ready','unknown',:input)","WITH RECURSIVE seq(i) AS (SELECT length(:input) UNION ALL SELECT i+1 FROM seq) SELECT max(i) FROM seq"]{
        assert!(verifier::evaluate(sql,&json!({}),deadline()).is_err(),"{sql}");
    }
    assert!(verifier::evaluate(ADB, &json!({}), Instant::now()).is_err());
}
struct Project {
    dir: tempfile::TempDir,
    proposal: Proposal,
}
impl Project {
    fn new() -> Self {
        Self::with_proposal(Self::ci_proposal())
    }
    fn ci_proposal() -> Proposal {
        Proposal {
            task_id: "CI".into(),
            phase: "dev".into(),
            category: Category::TechnicalCondition,
            target: "staging".into(),
            criterion: "Candidate passes staging CI".into(),
            decision: None,
            verifier: Some(Verifier {
                source: Source::JsonFile {
                    path: "reports/ci.json".into(),
                },
                script: CI.into(),
            }),
        }
    }
    fn with_proposal(proposal: Proposal) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-b", "main"]);
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.org",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        );
        let graph = json!({"tasks":[{"id":proposal.task_id,"state":"todo","blocked_on_external":{"reason":"Waiting on prerequisite","clears_when":proposal.criterion,"tracking_id":"receipt"}},
                                 {"id":"OTHER","state":"blocked"}]});
        fs::write(root.join("prd.json"), graph.to_string()).unwrap();
        let mut reg = TaskRegistry::from_value(&graph).unwrap();
        crate::coordinator::task_selector::reconcile_task_blocks(&mut reg, "2026-01-01T00:00:00Z");
        state::coordinator_state_registry_save(root, &Default::default(), &reg.to_value().unwrap())
            .unwrap();
        Self { dir, proposal }
    }
    fn root(&self) -> &Path {
        self.dir.path()
    }
    fn tasks(&self) -> Vec<Task> {
        TaskRegistry::from_value(
            &state::coordinator_state_registry_load(self.root(), &Default::default()).unwrap(),
        )
        .unwrap()
        .tasks
    }
    fn install(&self) -> String {
        DiagnosisSnapshot::capture(self.root(), None, &self.tasks())
            .register(self.root(), None, std::slice::from_ref(&self.proposal))
            .unwrap()
            .remove(0)
    }
    fn replay(&self) -> Task {
        replay_at_startup(
            self.root(),
            &self.root().join("prd.json"),
            Some(&crate::config::CoordinatorConfig {
                reference_branch: Some("main".into()),
                ..Default::default()
            }),
            &CoordinatorEnvConfig::default(),
        )
        .unwrap();
        self.tasks().remove(0)
    }
    fn report(&self, commit: &str) {
        self.report_at(self.root(), commit)
    }
    fn report_at(&self, worktree: &Path, commit: &str) {
        fs::create_dir_all(worktree.join("reports")).unwrap();
        fs::write(worktree.join("reports/ci.json"),json!({"commit":commit,"environment":"staging","finished_at":chrono::Utc::now().timestamp(),"result":"passed"}).to_string()).unwrap()
    }
}
fn git(root: &Path, args: &[&str]) {
    let result = crate::git::run_git_output_mapped(root, args, "resume check fixture").unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn outcome(task: &Task) -> &Outcome {
    task.task_runtime
        .resume_check
        .as_ref()
        .unwrap()
        .last
        .as_ref()
        .unwrap()
}
#[test]
fn restart_rechecks_only_the_bound_task_and_preserves_audit() {
    let p = Project::new();
    let id = p.install();
    let task = p.replay();
    assert_eq!(task.state, "blocked");
    assert_eq!(outcome(&task).status, Status::Waiting);
    assert!(waiting_check(p.root(), &p.root().join("prd.json"), &task).is_some());
    assert_eq!(p.install(), id);
    p.report("old-commit");
    assert_eq!(outcome(&p.replay()).status, Status::Waiting);
    p.report(&crate::git::head_commit(p.root()).unwrap());
    let ready = p.replay();
    assert_eq!(outcome(&ready).status, Status::Ready);
    assert_eq!(ready.state, "todo");
    assert_eq!(p.tasks()[1].state, "blocked");
    assert!(ready.external_block_is_cleared());
    assert!(waiting_check(p.root(), &p.root().join("prd.json"), &ready).is_none());
    let mut changed = ready.clone();
    changed.blocked_on_external.as_mut().unwrap().clears_when = "Also requires a signature".into();
    assert!(!changed.external_block_is_cleared());
    assert_eq!(
        p.replay().task_runtime.resume_check,
        ready.task_runtime.resume_check
    );
}
#[test]
fn changed_packet_candidate_or_prd_keeps_the_block() {
    for change in ["packet", "candidate", "prd"] {
        let p = Project::new();
        let id = p.install();
        match change {
            "packet" => fs::write(
                p.root().join(format!(
                    ".macc/state/resume-checks/{}.json",
                    id.strip_prefix("sha256:").unwrap()
                )),
                "{}",
            )
            .unwrap(),
            "candidate" => git(
                p.root(),
                &[
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.org",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "new base",
                ],
            ),
            _ => {
                let path = p.root().join("prd.json");
                let mut prd: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                prd["tasks"][0]["title"] = json!("Revised requirement");
                fs::write(path, prd.to_string()).unwrap();
            }
        }
        p.report(&crate::git::head_commit(p.root()).unwrap());
        let task = p.replay();
        assert_eq!(task.state, "blocked", "{change}");
        assert_eq!(outcome(&task).status, Status::Error, "{change}");
    }
}
#[test]
fn passive_classification_does_not_release_or_run_a_script() {
    let mut p = Project::new();
    for category in [
        Category::HumanApproval,
        Category::ReviewCorrection,
        Category::ExternalAction,
    ] {
        p.proposal.category = category;
        assert!(DiagnosisSnapshot::capture(p.root(), None, &p.tasks())
            .register(p.root(), None, std::slice::from_ref(&p.proposal))
            .is_err());
        let verifier = p.proposal.verifier.take();
        p.install();
        let t = p.replay();
        assert_eq!(t.state, "blocked");
        assert!(t.task_runtime.resume_check.unwrap().checked_at.is_none());
        p.proposal.verifier = verifier;
    }
}

#[test]
fn changing_commit_during_diagnosis_prevents_registration() {
    let p = Project::new();
    let snapshot = DiagnosisSnapshot::capture(p.root(), None, &p.tasks());
    git(
        p.root(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.org",
            "commit",
            "--allow-empty",
            "-m",
            "changed during diagnosis",
        ],
    );
    assert!(snapshot
        .register(p.root(), None, &[p.proposal.clone()])
        .is_err());
    assert!(p.tasks()[0].task_runtime.resume_check.is_none());
}

#[test]
fn adb_host_inventory_is_framed_and_checked_on_the_runner() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = server.local_addr().unwrap();
    let responder = std::thread::spawn(move || {
        let (mut stream, _) = server.accept().unwrap();
        let mut request = [0; 18];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"000ehost:devices-l");
        let inventory = "unrelated\tdevice model:Other\nreserved\tunauthorized usb:1-3\n";
        write!(stream, "OKAY{:04x}{inventory}", inventory.len()).unwrap();
    });
    let inventory = adb::inventory(address, deadline()).unwrap();
    responder.join().unwrap();
    assert_eq!(inventory[0]["attributes"]["model"], "Other");
    assert_eq!(
        evaluate(ADB, &json!({"target":"reserved","observation":inventory})).status,
        Status::Waiting
    );
}

#[test]
fn restart_checks_the_dataset_and_writer_lock_without_a_domain_handler() {
    use sha2::{Digest, Sha256};
    let data = b"sample dataset\x00\xff";
    let expected = format!("{:x}", Sha256::digest(data));
    let p = Project::with_proposal(Proposal {
        task_id: "IMPORT".into(),
        phase: "dev".into(),
        category: Category::TechnicalCondition,
        criterion: format!("Dataset {expected} is available and its writer has released the lock"),
        target: expected,
        decision: None,
        verifier: Some(Verifier {
            source: serde_json::from_value(json!({"kind":"inputs","fields":{
                "digest":{"kind":"sha256_file","path":"export/dataset.bin"},
                "lock":{"kind":"path_info","path":"export/writer.lock"}
            }}))
            .unwrap(),
            script: include_str!("../../../../docs/resume-checks/import.sql").into(),
        }),
    });
    p.install();
    let dir = p.root().join("export");
    let file = dir.join("dataset.bin");
    let lock = dir.join("writer.lock");
    let missing = p.replay();
    assert_eq!(outcome(&missing).status, Status::Waiting);
    assert!(outcome(&missing).detail.contains("export/dataset.bin"));
    fs::create_dir(&dir).unwrap();
    fs::write(&file, b"incomplete").unwrap();
    assert_eq!(outcome(&p.replay()).status, Status::Waiting);
    fs::write(lock.as_path(), b"").unwrap();
    fs::write(&file, data).unwrap();
    let locked = p.replay();
    assert_eq!(outcome(&locked).status, Status::Waiting);
    assert!(outcome(&locked).detail.contains("lock"));
    fs::remove_file(lock).unwrap();
    let ready = p.replay();
    assert_eq!(outcome(&ready).status, Status::Ready);
    assert_eq!(ready.state, "todo");
    assert!(ready.external_block_is_cleared());
    assert_eq!(p.tasks()[1].state, "blocked");
}

#[test]
fn input_groups_validate_names_paths_deadlines_and_total_size() {
    let dir = tempfile::tempdir().unwrap();
    let collect = |fields: Value, until| {
        let source: Source =
            serde_json::from_value(json!({"kind":"inputs","fields":fields})).unwrap();
        observations::collect(&source, dir.path(), until)
    };
    for fields in [
        json!({"a.b":{"kind":"path_info","path":"a"}}),
        json!({"a":{"kind":"inputs","fields":{}}}),
        json!({"a":{"kind":"sha256_file","path":"../data"}}),
        Value::Object(
            (0..9)
                .map(|i| (i.to_string(), json!({"kind":"path_info","path":"f"})))
                .collect(),
        ),
    ] {
        assert!(collect(fields, deadline()).is_err());
    }
    assert!(collect(json!({}), Instant::now()).is_err());
    fs::write(dir.path().join("sample"), "x".repeat(70 * 1024)).unwrap();
    assert!(collect(
        json!({"a":{"kind":"text_file","path":"sample"},"b":{"kind":"text_file","path":"sample"}}),
        deadline()
    )
    .is_err());
    let observations::Reading::Observed(value) = collect(
        json!({
            "empty":{"kind":"path_info","path":"absent"},
            "sample":{"kind":"path_info","path":"sample"}
        }),
        deadline(),
    )
    .unwrap() else {
        panic!("expected metadata");
    };
    assert_eq!(value["empty"], json!({"kind":"missing"}));
    assert_eq!(value["sample"], json!({"kind":"file","bytes":70*1024}));
    fs::write(dir.path().join("invalid"), "[").unwrap();
    assert!(collect(
        json!({"bad":{"kind":"json_file","path":"invalid"}}),
        deadline()
    )
    .is_err());
}

#[test]
fn a_custom_time_window_can_run_without_an_artifact_or_device() {
    for (offset, expected) in [(86400, Status::Waiting), (-86400, Status::Ready)] {
        let start = chrono::Utc::now().timestamp() + offset;
        let p = Project::with_proposal(Proposal {
            task_id: "MAINTENANCE".into(),
            phase: "dev".into(),
            category: Category::TechnicalCondition,
            target: start.to_string(),
            criterion: format!("The maintenance window opens at Unix timestamp {start}"),
            decision: None,
            verifier: Some(Verifier {
                source: Source::Inputs { fields: Default::default() },
                script: "SELECT CASE WHEN (:input ->> '$.now_unix') >= CAST(:input ->> '$.target' AS INTEGER)
                    THEN json_object('status','ready','detail','The maintenance window is open')
                    ELSE json_object('status','waiting','detail','Wait for the scheduled window') END".into(),
            }),
        });
        p.install();
        assert_eq!(outcome(&p.replay()).status, expected);
        assert_eq!(p.tasks()[1].state, "blocked");
    }
}

// Capture the durable packets at Git's commit point, before control returns to MACC.
#[cfg(unix)]
fn snapshot_packets_at_handoff(root: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let hooks = root.join("fixture-hooks");
    fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("reference-transaction");
    fs::write(
        &hook,
        r#"#!/bin/sh
[ "$1" = committed ] || exit 0
while read -r old new ref; do
    if [ "$ref" = refs/heads/ai/fixture/worker ]; then
        common=$(git rev-parse --path-format=absolute --git-common-dir) || exit 1
        cp -R "$common/../.macc/state/resume-checks" "$common/packets-at-handoff"
    fi
done
"#,
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    git(root, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
}

#[test]
fn reconciliation_rechecks_the_retained_phase_on_its_new_commit() {
    for commit_specific in [false, true] {
        let mut proposal = Project::ci_proposal();
        proposal.phase = "review".into();
        if !commit_specific {
            proposal.criterion = "The maintenance window is open".into();
            proposal.verifier = Some(Verifier {
                source: Source::Inputs { fields: Default::default() },
                script: "SELECT json_object('status','ready','detail','Window open') WHERE :input ->> '$.now_unix' > 0".into(),
            });
        }
        let project = Project::with_proposal(proposal);
        let root = project.root();
        git(root, &["config", "user.name", "Fixture"]);
        git(root, &["config", "user.email", "test@example.invalid"]);
        fs::write(root.join(".gitignore"), ".macc/\nreports/\n").unwrap();
        git(root, &["add", ".gitignore"]);
        git(root, &["commit", "-qm", "ignore local artifacts"]);
        let worker = root.join(".macc/worktree/worker");
        let branch = "ai/fixture/worker";
        git(
            root,
            &["worktree", "add", "-qb", branch, worker.to_str().unwrap()],
        );
        fs::create_dir_all(worker.join(".macc")).unwrap();
        fs::write(
            worker.join(".macc/worktree.json"),
            json!({
                "id":"worker","slug":"worker","tool":"fixture","branch":branch,"base":"main"
            })
            .to_string(),
        )
        .unwrap();
        let args = Default::default();
        let mut registry = state::coordinator_state_registry_load(root, &args).unwrap();
        registry["tasks"][0]["worktree"] =
            json!({"branch":branch,"base_branch":"main","worktree_path":worker});
        registry["tasks"][0]["task_runtime"]["current_phase"] = json!("review");
        state::coordinator_state_registry_save(root, &args, &registry).unwrap();
        let previous = project.install();
        let registered = state::coordinator_state_registry_load(root, &args).unwrap();
        let old_head = crate::git::head_commit(&worker).unwrap();
        if commit_specific {
            project.report_at(&worker, &old_head);
        }
        fs::write(worker.join("unfinished.txt"), "retained work").unwrap();
        fs::write(root.join("independent.txt"), "new reference content").unwrap();
        git(root, &["add", "independent.txt"]);
        git(root, &["commit", "-qm", "Independent task advanced main"]);
        let head = crate::git::head_commit(root).unwrap();
        if !commit_specific {
            // An unwritable successor must stop the handoff before checkout or ref movement.
            let blocked = project.tasks().remove(0);
            let check = blocked.task_runtime.resume_check.as_deref().unwrap();
            let mut packet = packets::load(root, check).unwrap();
            packet.advance_candidate(&previous, &head);
            let id = crate::coordinator::decisions::digest(&packet);
            let file = root
                .join(".macc/state/resume-checks")
                .join(format!("{}.json", id.strip_prefix("sha256:").unwrap()));
            fs::create_dir(&file).unwrap();
            let failed = project.replay();
            assert_eq!(outcome(&failed).status, Status::Error);
            assert_eq!(failed.task_runtime.resume_check.unwrap().id, previous);
            assert_eq!(crate::git::head_commit(&worker).unwrap(), old_head);
            assert!(!worker.join("independent.txt").exists());
            fs::remove_dir(file).unwrap();
        }
        #[cfg(unix)]
        snapshot_packets_at_handoff(root);
        let task = project.replay();
        assert_eq!(
            crate::git::head_commit(&worker).unwrap(),
            head,
            "{:?}",
            outcome(&task)
        );
        let check = task.task_runtime.resume_check.as_deref().unwrap();
        let packet = packets::load(root, check).unwrap();
        assert_eq!(packet.previous_packet.as_deref(), Some(previous.as_str()));
        assert_eq!(packet.binding["commit"], head);
        // Restart from exactly the packets visible when Git moved the branch
        // and the registry still pointed to the previous candidate.
        #[cfg(unix)]
        {
            fs::rename(
                root.join(".macc/state/resume-checks"),
                root.join("after-replay"),
            )
            .unwrap();
            fs::rename(
                root.join(".git/packets-at-handoff"),
                root.join(".macc/state/resume-checks"),
            )
            .unwrap();
        }
        state::coordinator_state_registry_save(root, &args, &registered).unwrap();
        let recovered = project.replay();
        assert_eq!(
            recovered.task_runtime.resume_check.as_ref().unwrap().id,
            check.id
        );
        let resumed = if commit_specific {
            assert_eq!(task.state, "blocked");
            assert_eq!(outcome(&task).status, Status::Waiting);
            assert_eq!(outcome(&recovered).status, Status::Waiting);
            project.report_at(&worker, &head);
            project.replay()
        } else {
            recovered
        };
        assert_eq!(
            outcome(&resumed).status,
            Status::Ready,
            "{:?}",
            outcome(&resumed)
        );
        assert_eq!(
            fs::read_to_string(worker.join("unfinished.txt")).unwrap(),
            "retained work"
        );
        assert_eq!(resumed.state, "testing");
        assert!(resumed.external_block_is_cleared());
    }
}
