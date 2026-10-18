use super::*;
use crate::{
    catalog::Agent,
    config::CanonicalConfig,
    doctor::ToolCheck,
    plan::{ActionPlan, PlannedOp},
    resolve::{CliOverrides, MaterializedFetchUnit},
    tool::{ToolDescriptor, ToolDiagnostic, ToolRegistry},
    ApplyReport, MaccEngine,
};
use std::cell::Cell;
struct RecordingEngine {
    inner: MaccEngine,
    starts: Cell<usize>,
    supervisors: Cell<usize>,
    supervisor_fails: Cell<bool>,
    stops: Cell<usize>,
    startup_fails: Cell<bool>,
    dirty_allowed: Cell<bool>,
}
impl Engine for RecordingEngine {
    fn list_tools(&self, p: &ProjectPaths) -> (Vec<ToolDescriptor>, Vec<ToolDiagnostic>) {
        self.inner.list_tools(p)
    }
    fn doctor(&self, p: &ProjectPaths) -> Vec<ToolCheck> {
        self.inner.doctor(p)
    }
    fn plan(
        &self,
        p: &ProjectPaths,
        c: &CanonicalConfig,
        u: &[MaterializedFetchUnit],
        o: &CliOverrides,
    ) -> Result<ActionPlan> {
        self.inner.plan(p, c, u, o)
    }
    fn plan_operations(&self, p: &ProjectPaths, a: &ActionPlan) -> Vec<PlannedOp> {
        self.inner.plan_operations(p, a)
    }
    fn apply(&self, p: &ProjectPaths, a: &mut ActionPlan, u: bool) -> Result<ApplyReport> {
        self.inner.apply(p, a, u)
    }
    fn builtin_agents(&self) -> Vec<Agent> {
        self.inner.builtin_agents()
    }
    fn coordinator_start_managed_command_process(
        &self,
        paths: &ProjectPaths,
        _: &crate::service::coordinator_workflow::CoordinatorCommand,
        cfg: Option<&crate::config::CoordinatorConfig>,
        _: Option<&str>,
    ) -> Result<()> {
        self.starts.set(self.starts.get() + 1);
        self.dirty_allowed.set(
            CoordinatorConfigResolved::resolve(cfg)
                .reference_branch_preflight
                .dirty_policy
                == DirtyReferencePolicy::Allow,
        );
        if self.startup_fails.get() {
            let pid = std::process::id() as i32;
            crate::coordinator::managed_command_registry::upsert_managed_command_with_result(
                paths, "run", pid,
            )?;
            let result = crate::coordinator::managed_command_registry::managed_command_result_path(
                &paths.root,
                "run",
                pid,
            );
            std::fs::create_dir_all(result.parent().unwrap()).unwrap();
            std::fs::write(result, "1").unwrap();
        }
        Ok(())
    }
    fn coordinator_attach_supervisor(&self, _: &ProjectPaths) -> Result<()> {
        self.supervisors.set(self.supervisors.get() + 1);
        if self.supervisor_fails.get() {
            return Err(MaccError::Validation("fixture attachment failure".into()));
        }
        Ok(())
    }
    fn coordinator_stop_managed_command_process(
        &self,
        _: &ProjectPaths,
        _: bool,
    ) -> Result<crate::service::coordinator::CoordinatorStopResult> {
        self.stops.set(self.stops.get() + 1);
        Ok(crate::service::coordinator::CoordinatorStopResult {
            targets: 1,
            used_group: false,
        })
    }
}
fn git(root: &std::path::Path, args: &[&str]) {
    let output = crate::git::run_git_output_mapped(root, args, "launch test fixture").unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn fixture(reference: &str) -> (tempfile::TempDir, ProjectPaths, RecordingEngine) {
    let root = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(root.path());
    std::fs::create_dir_all(&paths.macc_dir).unwrap();
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["config", "user.name", "MACC Tests"]);
    git(
        root.path(),
        &["config", "user.email", "tests@example.invalid"],
    );
    let mut config = CanonicalConfig::default();
    config.tools.enabled = vec!["future-tool".into()];
    config.automation.coordinator = Some(crate::config::CoordinatorConfig {
        reference_branch: Some(reference.into()),
        prd_file: Some("tasks.json".into()),
        ..Default::default()
    });
    std::fs::write(&paths.config_path, serde_yaml::to_string(&config).unwrap()).unwrap();
    std::fs::write(root.path().join("tasks.json"), r#"{"tasks":[{"id":"A"}]}"#).unwrap();
    std::fs::write(root.path().join("README.md"), "seed\n").unwrap();
    std::fs::write(root.path().join(".gitignore"), ".macc/state/\n.macc/log/\n").unwrap();
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-m", "seed"]);
    let engine = RecordingEngine {
        inner: MaccEngine::new(ToolRegistry::new()),
        starts: Cell::new(0),
        supervisors: Cell::new(0),
        supervisor_fails: Cell::new(false),
        stops: Cell::new(0),
        startup_fails: Cell::new(false),
        dirty_allowed: Cell::new(false),
    };
    (root, paths, engine)
}
fn decision(review: &LaunchReview) -> LaunchDecision {
    LaunchDecision {
        confirmed: true,
        revision: review.revision.clone(),
        supervisor: true,
        create_from: None,
        allow_dirty_reference: false,
    }
}
#[test]
fn confirmed_launch_preserves_the_supervisor_choice() {
    let (_root, paths, engine) = fixture("main");
    let review = engine.coordinator_launch_review(&paths).unwrap();
    assert!(review.blockers.is_empty(), "{:?}", review.blockers);
    let mut request = decision(&review);
    request.supervisor = false;
    engine
        .coordinator_launch_confirmed(&paths, &request, None)
        .unwrap();
    assert_eq!(engine.starts.get(), 1);
    assert_eq!(engine.supervisors.get(), 0);
    request.supervisor = true;
    engine
        .coordinator_launch_confirmed(&paths, &request, None)
        .unwrap();
    assert_eq!(engine.supervisors.get(), 1);
}
#[test]
fn no_confirmation_or_changed_configuration_starts_nothing() {
    let (_root, paths, engine) = fixture("main");
    let review = engine.coordinator_launch_review(&paths).unwrap();
    let mut request = decision(&review);
    request.confirmed = false;
    assert!(engine
        .coordinator_launch_confirmed(&paths, &request, None)
        .is_err());
    request.confirmed = true;
    let mut cfg = engine.load_canonical_config(&paths).unwrap();
    cfg.settings.offline = true;
    std::fs::write(&paths.config_path, serde_yaml::to_string(&cfg).unwrap()).unwrap();
    assert!(engine
        .coordinator_launch_confirmed(&paths, &request, None)
        .unwrap_err()
        .to_string()
        .contains("conditions changed"));
    assert_eq!(engine.starts.get(), 0);
}
#[test]
fn missing_branch_requires_a_reviewed_source_and_creates_it_only_after_confirmation() {
    let (_root, paths, engine) = fixture("reference");
    let review = engine.coordinator_launch_review(&paths).unwrap();
    assert!(review.needs_branch_creation);
    let mut request = decision(&review);
    assert!(engine
        .coordinator_launch_confirmed(&paths, &request, None)
        .is_err());
    assert!(!crate::git::run_git_output_mapped(
        &paths.root,
        &["show-ref", "--verify", "refs/heads/reference"],
        "inspect branch"
    )
    .unwrap()
    .status
    .success());
    request.create_from = Some("HEAD".into());
    request.supervisor = false;
    engine
        .coordinator_launch_confirmed(&paths, &request, None)
        .unwrap();
    assert_eq!(engine.starts.get(), 1);
    git(
        &paths.root,
        &["show-ref", "--verify", "refs/heads/reference"],
    );
}
#[test]
fn dirty_reference_requires_its_own_acknowledgement() {
    let (_root, paths, engine) = fixture("main");
    std::fs::write(paths.root.join("README.md"), "unfinished work\n").unwrap();
    let review = engine.coordinator_launch_review(&paths).unwrap();
    assert!(review.dirty_reference);
    let mut request = decision(&review);
    assert!(engine
        .coordinator_launch_confirmed(&paths, &request, None)
        .is_err());
    assert_eq!(engine.starts.get(), 0);
    request.allow_dirty_reference = true;
    request.supervisor = false;
    engine
        .coordinator_launch_confirmed(&paths, &request, None)
        .unwrap();
    assert_eq!(engine.starts.get(), 1);
    assert!(engine.dirty_allowed.get());
    assert_eq!(
        CoordinatorConfigResolved::resolve(
            engine
                .load_canonical_config(&paths)
                .unwrap()
                .automation
                .coordinator
                .as_ref()
        )
        .reference_branch_preflight
        .dirty_policy,
        DirtyReferencePolicy::Block,
        "One-run approval must not relax the saved policy"
    );
    assert_eq!(
        std::fs::read_to_string(paths.root.join("README.md")).unwrap(),
        "unfinished work\n"
    );
}

#[test]
fn failed_supervisor_attachment_rolls_back_the_new_start() {
    let (_root, paths, engine) = fixture("main");
    engine.supervisor_fails.set(true);
    let review = engine.coordinator_launch_review(&paths).unwrap();
    let error = engine
        .coordinator_launch_confirmed(&paths, &decision(&review), None)
        .unwrap_err();
    assert!(error.to_string().contains("Coordinator rollback: stopped"));
    assert_eq!(engine.starts.get(), 1);
    assert_eq!(engine.stops.get(), 1);
}

#[test]
fn failed_runtime_readiness_rolls_back_before_attaching_supervision() {
    let (_root, paths, engine) = fixture("main");
    engine.startup_fails.set(true);
    let review = engine.coordinator_launch_review(&paths).unwrap();
    let error = engine
        .coordinator_launch_confirmed(&paths, &decision(&review), None)
        .unwrap_err();
    assert!(error.to_string().contains("Coordinator rollback: stopped"));
    assert_eq!(engine.starts.get(), 1);
    assert_eq!(engine.stops.get(), 1);
    assert_eq!(engine.supervisors.get(), 0);
}
