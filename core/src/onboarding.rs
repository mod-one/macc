/// Readiness ladder and onboarding state (spec §9).
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessStatus {
    Done,
    Pending,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessStep {
    pub number: u8,
    pub label: String,
    pub status: ReadinessStatus,
    /// Extra detail shown beside the label (e.g. selected tool name).
    pub detail: Option<String>,
}

impl ReadinessStep {
    pub fn done(number: u8, label: &str, detail: Option<&str>) -> Self {
        Self {
            number,
            label: label.to_string(),
            status: ReadinessStatus::Done,
            detail: detail.map(|s| s.to_string()),
        }
    }

    pub fn pending(number: u8, label: &str) -> Self {
        Self {
            number,
            label: label.to_string(),
            status: ReadinessStatus::Pending,
            detail: None,
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self.status {
            ReadinessStatus::Done => "✅",
            ReadinessStatus::Pending => "❌",
            ReadinessStatus::NotApplicable => " —",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessLadder {
    pub steps: Vec<ReadinessStep>,
    pub blocking_count: usize,
}

impl ReadinessLadder {
    pub fn is_ready(&self) -> bool {
        self.blocking_count == 0
    }
}

/// Persistent onboarding progress saved to `.macc/state/onboarding.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OnboardingState {
    pub quickstart_version: u32,
    pub completed_steps: CompletedSteps,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<LastError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CompletedSteps {
    pub init: bool,
    pub tool_selected: bool,
    pub starter_skill_installed: bool,
    pub starter_task_created: bool,
    pub config_applied: bool,
    pub doctor_passed: bool,
    pub coordinator_started: bool,
    pub first_task_dispatched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastError {
    pub code: String,
    pub message: String,
}

impl OnboardingState {
    pub fn load(macc_dir: &Path) -> Self {
        let path = macc_dir.join("state/onboarding.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self, macc_dir: &Path) -> crate::Result<()> {
        let path = macc_dir.join("state/onboarding.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::MaccError::Io {
                path: parent.to_string_lossy().into(),
                action: "create onboarding state dir".into(),
                source: e,
            })?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            crate::MaccError::Validation(format!("Failed to serialize onboarding state: {}", e))
        })?;
        std::fs::write(&path, json).map_err(|e| crate::MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "write onboarding state".into(),
            source: e,
        })
    }
}

/// Compute the readiness ladder from pre-resolved state.
///
/// Called by `Engine::readiness_ladder()` which supplies the already-loaded
/// `CanonicalConfig` and coordinator liveness flag — no direct YAML/SQLite
/// access happens here.
pub fn compute_readiness_from_state(
    paths: &crate::ProjectPaths,
    canonical: Option<&crate::config::CanonicalConfig>,
    coordinator_running: bool,
) -> ReadinessLadder {
    let onboarding = OnboardingState::load(&paths.macc_dir);

    // Step 1: Project initialized
    let step1 =
        if paths.macc_dir.join("macc.yaml").exists() || paths.macc_dir.join("macc.toml").exists() {
            ReadinessStep::done(1, "Project initialized", None)
        } else {
            ReadinessStep::pending(1, "Project initialized")
        };

    // Step 2: Tool adapter selected — read from the already-loaded canonical config.
    let selected_tool = canonical
        .and_then(|c| c.tools.enabled.first().cloned())
        .or_else(|| {
            // Fall back to onboarding state if config not loaded.
            onboarding.completed_steps.tool_selected.then(String::new)
        });
    let step2 = if let Some(ref tool) = selected_tool {
        if tool.is_empty() {
            ReadinessStep::done(2, "Tool adapter selected", None)
        } else {
            ReadinessStep::done(2, "Tool adapter selected", Some(tool.as_str()))
        }
    } else {
        ReadinessStep::pending(2, "Tool adapter selected")
    };

    // Step 3: Config applied — managed-paths state file written by `macc apply`.
    let config_applied =
        onboarding.completed_steps.config_applied || paths.managed_paths_state_path().exists();
    let step3 = if config_applied {
        ReadinessStep::done(3, "Config applied", None)
    } else {
        ReadinessStep::pending(3, "Config applied")
    };

    // Step 4: PRD/task available
    let prd_path = paths.macc_dir.join("prd.json");
    let task_count = if prd_path.exists() {
        crate::doctor::count_ready_tasks_public(&prd_path)
    } else {
        0
    };
    let step4 = if task_count > 0 {
        ReadinessStep::done(
            4,
            "PRD/task available",
            Some(&format!("{} task(s) ready", task_count)),
        )
    } else if onboarding.completed_steps.starter_task_created {
        ReadinessStep::done(4, "PRD/task available", None)
    } else {
        ReadinessStep::pending(4, "PRD/task available")
    };

    // Step 5: Git identity configured
    let git_id_ok = check_git_identity_ok(&paths.root);
    let step5 = if git_id_ok {
        ReadinessStep::done(5, "Git identity configured", None)
    } else {
        ReadinessStep::pending(5, "Git identity configured")
    };

    // Step 6: Coordinator running — supplied by caller via Engine facade.
    let step6 = if coordinator_running {
        ReadinessStep::done(6, "Coordinator running", None)
    } else {
        ReadinessStep::pending(6, "Coordinator running")
    };

    // Steps 7–8: only mark done when first task has been dispatched.
    let step7 = if onboarding.completed_steps.first_task_dispatched {
        ReadinessStep::done(7, "Performer connected", None)
    } else {
        ReadinessStep {
            number: 7,
            label: "Performer connected".to_string(),
            status: ReadinessStatus::NotApplicable,
            detail: None,
        }
    };

    let step8 = if onboarding.completed_steps.first_task_dispatched {
        ReadinessStep::done(8, "First task dispatched", None)
    } else {
        ReadinessStep {
            number: 8,
            label: "First task dispatched".to_string(),
            status: ReadinessStatus::NotApplicable,
            detail: None,
        }
    };

    let steps = vec![step1, step2, step3, step4, step5, step6, step7, step8];
    let blocking_count = steps
        .iter()
        .filter(|s| matches!(s.status, ReadinessStatus::Pending))
        .count();

    ReadinessLadder {
        steps,
        blocking_count,
    }
}

// ── Internal helpers (core-only, no direct storage I/O) ──────────────────────

fn check_git_identity_ok(root: &Path) -> bool {
    is_git_config_set(root, "user.name") && is_git_config_set(root, "user.email")
}

fn is_git_config_set(root: &Path, key: &str) -> bool {
    let local_ok = std::process::Command::new("git")
        .args(["config", "--local", key])
        .current_dir(root)
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false);
    if local_ok {
        return true;
    }
    std::process::Command::new("git")
        .args(["config", "--global", key])
        .output()
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}
