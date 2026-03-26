use crate::tool::spec::{CheckSeverity, DoctorCheckKind, ToolSpec};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    Installed,
    Missing,
    Error(String),
}

pub trait CheckRunner {
    fn which(&self, binary: &str) -> bool;
    fn path_exists(&self, path: &str) -> bool;
    fn git_config_key(&self, key: &str) -> bool;
}

pub struct SystemRunner;

impl CheckRunner for SystemRunner {
    fn which(&self, binary: &str) -> bool {
        let cmd = if cfg!(windows) { "where" } else { "which" };
        let output = Command::new(cmd).arg(binary).output();
        matches!(output, Ok(out) if out.status.success())
    }

    fn path_exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    fn git_config_key(&self, key: &str) -> bool {
        matches!(
            Command::new("git").args(["config", key]).output(),
            Ok(out) if out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty()
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCheck {
    pub name: String,
    pub tool_id: Option<String>,
    pub check_target: String,
    pub kind: DoctorCheckKind,
    pub status: ToolStatus,
    pub severity: CheckSeverity,
    /// Optional human-readable fix hint shown when the check fails.
    pub fix_hint: Option<String>,
}

pub fn check_tool(runner: &dyn CheckRunner, kind: &DoctorCheckKind, value: &str) -> ToolStatus {
    match kind {
        DoctorCheckKind::Which => {
            if runner.which(value) {
                ToolStatus::Installed
            } else {
                ToolStatus::Missing
            }
        }
        DoctorCheckKind::PathExists => {
            if runner.path_exists(value) {
                ToolStatus::Installed
            } else {
                ToolStatus::Missing
            }
        }
        DoctorCheckKind::GitConfigKey => {
            if runner.git_config_key(value) {
                ToolStatus::Installed
            } else {
                ToolStatus::Missing
            }
        }
        DoctorCheckKind::Custom => ToolStatus::Error("Custom checks not supported yet".to_string()),
    }
}

pub fn checks_for_enabled_tools(specs: &[ToolSpec]) -> Vec<ToolCheck> {
    let mut checks = Vec::new();

    // Baseline checks
    checks.push(ToolCheck {
        name: "Git".to_string(),
        tool_id: None,
        check_target: "git".to_string(),
        kind: DoctorCheckKind::Which,
        status: ToolStatus::Missing,
        severity: CheckSeverity::Error,
        fix_hint: None,
    });

    // Git identity checks — required for commits to succeed.
    checks.push(ToolCheck {
        name: "Git user.email".to_string(),
        tool_id: None,
        check_target: "user.email".to_string(),
        kind: DoctorCheckKind::GitConfigKey,
        status: ToolStatus::Missing,
        severity: CheckSeverity::Error,
        fix_hint: Some("git config --global user.email \"you@example.com\"".to_string()),
    });
    checks.push(ToolCheck {
        name: "Git user.name".to_string(),
        tool_id: None,
        check_target: "user.name".to_string(),
        kind: DoctorCheckKind::GitConfigKey,
        status: ToolStatus::Missing,
        severity: CheckSeverity::Error,
        fix_hint: Some("git config --global user.name \"Your Name\"".to_string()),
    });

    for spec in specs {
        if let Some(doctor_specs) = &spec.doctor {
            for check_spec in doctor_specs {
                checks.push(ToolCheck {
                    name: spec.display_name.clone(),
                    tool_id: Some(spec.id.clone()),
                    check_target: check_spec.value.clone(),
                    kind: check_spec.kind.clone(),
                    status: ToolStatus::Missing,
                    severity: check_spec.severity.clone(),
                    fix_hint: None,
                });
            }
        } else {
            // Heuristic fallback: check for binary with same ID
            checks.push(ToolCheck {
                name: spec.display_name.clone(),
                tool_id: Some(spec.id.clone()),
                check_target: spec.id.clone(),
                kind: DoctorCheckKind::Which,
                status: ToolStatus::Missing,
                severity: CheckSeverity::Warning,
                fix_hint: None,
            });
        }
    }

    checks
}

pub fn run_checks(checks: &mut [ToolCheck]) {
    let runner = SystemRunner;
    for check in checks {
        check.status = check_tool(&runner, &check.kind, &check.check_target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockRunner {
        installed: Vec<String>,
        paths: Vec<String>,
    }

    impl CheckRunner for MockRunner {
        fn which(&self, binary: &str) -> bool {
            self.installed.contains(&binary.to_string())
        }
        fn path_exists(&self, path: &str) -> bool {
            self.paths.contains(&path.to_string())
        }
        fn git_config_key(&self, key: &str) -> bool {
            self.installed.contains(&key.to_string())
        }
    }

    #[test]
    fn test_check_tool_availability_with_mock() {
        let tool_id = format!("tool-{}", uuid_v4_like());
        let runner = MockRunner {
            installed: vec![tool_id.clone()],
            paths: vec!["/tmp/foo".to_string()],
        };

        assert_eq!(
            check_tool(&runner, &DoctorCheckKind::Which, &tool_id),
            ToolStatus::Installed
        );
        assert_eq!(
            check_tool(&runner, &DoctorCheckKind::Which, "missing-tool"),
            ToolStatus::Missing
        );
        assert_eq!(
            check_tool(&runner, &DoctorCheckKind::PathExists, "/tmp/foo"),
            ToolStatus::Installed
        );
    }

    #[test]
    fn test_checks_generation() {
        let spec = ToolSpec {
            api_version: "v1".to_string(),
            id: format!("tool-{}", uuid_v4_like()),
            display_name: "Test Tool".to_string(),
            description: None,
            capabilities: vec![],
            fields: vec![],
            doctor: Some(vec![crate::tool::spec::DoctorCheckSpec {
                kind: DoctorCheckKind::Which,
                value: "test-bin".to_string(),
                severity: CheckSeverity::Error,
            }]),
            gitignore: Vec::new(),
            performer: None,
            install: None,
            update: None,
            version_check: None,
            defaults: None,
        };

        let checks = checks_for_enabled_tools(&[spec]);
        // Git + Git user.email + Git user.name + Test Tool
        assert_eq!(checks.len(), 4);
        assert_eq!(checks[1].check_target, "user.email");
        assert_eq!(checks[1].kind, DoctorCheckKind::GitConfigKey);
        assert!(checks[1].fix_hint.is_some());
        assert_eq!(checks[2].check_target, "user.name");
        assert_eq!(checks[2].kind, DoctorCheckKind::GitConfigKey);
        assert!(checks[2].fix_hint.is_some());
        assert_eq!(checks[3].check_target, "test-bin");
        assert_eq!(checks[3].kind, DoctorCheckKind::Which);
    }

    fn uuid_v4_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{:x}", nanos)
    }
}
