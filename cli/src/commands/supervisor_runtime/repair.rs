//! Isolated repair, independent verification, and integration against an exact base.
use super::tool::Agent;
use macc_core::supervisor::{incident::Incident, SupervisorConfig};
use macc_core::{MaccError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    pub summary: String,
    pub repairable: bool,
    #[serde(default)]
    pub requires_human: bool,
    #[serde(default)]
    pub task_ids: Vec<String>,
    #[serde(default)]
    pub macc_findings: Vec<String>,
}
#[derive(Debug, Deserialize)]
struct Verification {
    verified: bool,
    explanation: String,
}

pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = macc_core::git::run_git_output_mapped(root, args, "supervisor git operation")?;
    if !output.status.success() {
        return Err(MaccError::Validation(format!(
            "Supervisor git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
pub fn clean(root: &Path) -> Result<()> {
    if !git(root, &["status", "--porcelain"])?.is_empty() {
        return Err(MaccError::Validation(
            "Supervisor integration requires a clean project; user changes are preserved".into(),
        ));
    }
    Ok(())
}
pub fn prepare(root: &Path, dir: &Path) -> Result<(PathBuf, String)> {
    clean(root)?;
    let base = git(root, &["rev-parse", "HEAD"])?;
    let worktree = dir.join("worktree");
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            worktree
                .to_str()
                .ok_or_else(|| MaccError::Validation("Invalid repair worktree path".into()))?,
            &base,
        ],
    )?;
    std::fs::create_dir_all(worktree.join(".macc")).map_err(|e| io(&worktree, e))?;
    let config = root.join(".macc/macc.yaml");
    if config.exists() {
        std::fs::copy(config, worktree.join(".macc/macc.yaml")).map_err(|e| io(&worktree, e))?;
    }
    // Keep incident response files local and out of commits, even in projects with no MACC ignore entries.
    let exclude = git(&worktree, &["rev-parse", "--git-path", "info/exclude"])?;
    let exclude = PathBuf::from(exclude);
    let exclude = if exclude.is_absolute() {
        exclude
    } else {
        worktree.join(exclude)
    };
    if let Some(parent) = exclude.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    }
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude)
        .map_err(|e| io(&exclude, e))?
        .write_all(b"\n.macc/\n")
        .map_err(|e| io(&exclude, e))?;
    Ok((worktree, base))
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|e| io(path, e))?;
    serde_json::from_str(&text).map_err(|e| {
        MaccError::Validation(format!(
            "Supervisor response {} is invalid: {e}",
            path.display()
        ))
    })
}
pub async fn diagnose(
    agent: &Agent,
    root: &Path,
    worktree: &Path,
    dir: &Path,
    incident: &Incident,
) -> Result<Diagnosis> {
    let evidence =
        serde_json::to_string_pretty(incident).map_err(|e| MaccError::Validation(e.to_string()))?;
    let prompt=format!("You are the MACC supervisor diagnosing a coordinator incident. Project: {}. Isolated analysis worktree: {}. Read the original coordinator and performer logs under {}/.macc/log, the task requirements and source. Identify root causes, distinguishing project defects, missing migrations, external conditions, and MACC defects. Prefer recorded unmet_preconditions/result_explanation over normalized error fragments. Do not edit source, reset files, invoke a coordinator, change task state, or approve human gates. Write exactly one JSON object to .macc/supervisor-diagnosis.json: {{\"summary\":\"evidence-based diagnosis\",\"repairable\":true,\"requires_human\":false,\"task_ids\":[\"root task ids verified by evidence\"],\"macc_findings\":[\"MACC dysfunction and concrete improvement suggestions\"]}}. A missing human decision, external credential or explicit operator gate requires_human=true. Source/schema fixes are repairable by this authorized supervisor even when outside the original task's writable scope. Diagnose first; do not repair in this phase.\nIncident:\n{evidence}",root.display(),worktree.display(),root.display());
    agent.run(worktree, &prompt, dir, "diagnosis").await?;
    let diagnosis: Diagnosis = read_json(&worktree.join(".macc/supervisor-diagnosis.json"))?;
    if diagnosis
        .task_ids
        .iter()
        .any(|id| !incident.tasks.iter().any(|t| t.id == *id))
    {
        return Err(MaccError::Validation(
            "AI diagnosis selected a task outside this incident".into(),
        ));
    }
    if !git(worktree, &["status", "--porcelain"])?.is_empty() {
        return Err(MaccError::Validation(
            "Diagnosis modified source; analysis worktree retained for inspection".into(),
        ));
    }
    Ok(diagnosis)
}
pub fn validation_commands(root: &Path, config: &SupervisorConfig) -> Result<Vec<String>> {
    if !config.validation_commands.is_empty() {
        return Ok(config.validation_commands.clone());
    }
    if root.join("Makefile").exists()
        && std::fs::read_to_string(root.join("Makefile"))
            .map_err(|e| io(root, e))?
            .lines()
            .any(|l| l.starts_with("check:"))
    {
        return Ok(vec!["make check".into()]);
    }
    if root.join("Cargo.toml").exists() {
        return Ok(vec!["cargo test --workspace --locked".into()]);
    }
    let package = root.join("package.json");
    if package.exists() {
        let v: serde_json::Value = read_json(&package)?;
        let manager = if root.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else {
            "npm"
        };
        let scripts = v
            .get("scripts")
            .and_then(|s| s.as_object())
            .ok_or_else(|| MaccError::Validation("No project validation scripts".into()))?;
        if !scripts.contains_key("test") && !scripts.contains_key("test:e2e") {
            return Err(MaccError::Validation(
                "Supervisor requires a test command; configure validation_commands".into(),
            ));
        }
        return Ok([
            "lint",
            "typecheck",
            "build",
            "test",
            "test:e2e",
            "contracts:check",
        ]
        .iter()
        .filter(|s| scripts.contains_key(**s))
        .map(|s| format!("{manager} run {s}"))
        .collect());
    }
    Err(MaccError::Validation(
        "Supervisor requires validation_commands for this project".into(),
    ))
}
pub async fn validate(
    worktree: &Path,
    commands: &[String],
    dir: &Path,
    timeout: u64,
) -> Result<()> {
    for (i, command) in commands.iter().enumerate() {
        let file = std::fs::File::create(dir.join(format!("validation-{i}.log")))
            .map_err(|e| io(dir, e))?;
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c")
            .arg(command)
            .current_dir(worktree)
            .stdin(Stdio::null())
            .stdout(Stdio::from(file.try_clone().map_err(|e| io(dir, e))?))
            .stderr(Stdio::from(file))
            .kill_on_drop(true);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.as_std_mut().process_group(0);
        }
        let mut child = cmd.spawn().map_err(|e| io(worktree, e))?;
        let _group = super::tool::ProcessGroup(child.id());
        let result = tokio::time::timeout(Duration::from_secs(timeout.max(1)), child.wait()).await;
        let status = result
            .map_err(|_| MaccError::Validation(format!("Validation timed out: {command}")))?
            .map_err(|e| io(worktree, e))?;
        if !status.success() {
            return Err(MaccError::Validation(format!(
                "Validation failed: {command}; see {}",
                dir.display()
            )));
        }
    }
    Ok(())
}
pub async fn fix(
    agent: &Agent,
    root: &Path,
    worktree: &Path,
    dir: &Path,
    diagnosis: &Diagnosis,
    commands: &[String],
    timeout: u64,
) -> Result<String> {
    let prompt=format!("You are the authorized MACC supervisor repair agent in an isolated worktree. Repair these diagnosed root causes: {}. Source and database migration changes needed for these roots are authorized even outside the original performer scope. Preserve existing behavior and regression assertions; never bypass MFA or security, weaken tests, fabricate approval, change PRD/state/.macc metadata, or restart the coordinator. Add targeted regression tests. Install project dependencies if needed. Do not commit: the supervisor owns commits. Record intervention evidence and MACC improvement recommendations in .macc/repair-notes.md. Read original logs at {}. Validation commands are {:?}. Report missing external authority without guessing it.",diagnosis.summary,root.join(".macc/log").display(),commands);
    agent.run(worktree, &prompt, dir, "repair").await?;
    validate(worktree, commands, dir, timeout).await?;
    git(worktree, &["add", "--all"])?;
    let validated_tree = git(worktree, &["write-tree"])?;
    let validated_head = git(worktree, &["rev-parse", "HEAD"])?;
    let prompt=format!("Independently verify the supervisor repair. Diagnosis: {}. Examine source changes, existing and added regression tests and validation logs at {}. Do not edit files. Confirm the original root causes are corrected; no security checks, assertions, validation scripts or human gates were weakened or bypassed. Write .macc/supervisor-verification.json as {{\"verified\":true,\"explanation\":\"concrete evidence\"}}; use false for unresolved causes, unjustified test changes, or missing proof. Task ids: {:?}.",diagnosis.summary,dir.display(),diagnosis.task_ids);
    agent.run(worktree, &prompt, dir, "verification").await?;
    git(worktree, &["add", "--all"])?;
    if git(worktree, &["write-tree"])? != validated_tree
        || git(worktree, &["rev-parse", "HEAD"])? != validated_head
    {
        return Err(MaccError::Validation(
            "Verification modified validated source; integration refused".into(),
        ));
    }
    let verified: Verification = read_json(&worktree.join(".macc/supervisor-verification.json"))?;
    if !verified.verified {
        return Err(MaccError::Validation(format!(
            "Repair verification rejected: {}",
            verified.explanation
        )));
    }
    clean(root)?;
    // Caller checks exact base before integrating, and keeps failed worktrees for evidence.
    Ok(verified.explanation)
}
pub fn integrate(root: &Path, worktree: &Path, base: &str) -> Result<String> {
    clean(root)?;
    if git(root, &["rev-parse", "HEAD"])? != base {
        return Err(MaccError::Validation(
            "Project HEAD changed during intervention; repair retained, integration deferred"
                .into(),
        ));
    }
    if git(worktree, &["rev-parse", "HEAD"])? != base {
        return Err(MaccError::Validation(
            "Repair agent created commits outside the intervention transaction".into(),
        ));
    }
    git(worktree, &["add", "--all"])?;
    let changed = git(worktree, &["diff", "--cached", "--name-only"])?;
    if changed.lines().any(|p| {
        p == "prd.json"
            || p.ends_with(".prd.json")
            || p.starts_with(".macc/")
            || p == ".env"
            || p.starts_with(".env.")
    }) {
        return Err(MaccError::Validation(
            "Repair changed protected task/state/secret files".into(),
        ));
    }
    if changed.is_empty() {
        return Ok(base.into());
    }
    git(
        worktree,
        &[
            "commit",
            "-m",
            "fix: resolve coordinator incident through supervisor intervention",
        ],
    )?;
    let commit = git(worktree, &["rev-parse", "HEAD"])?;
    git(root, &["merge", "--ff-only", &commit])?;
    Ok(commit)
}
fn io(path: &Path, source: std::io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "supervisor repair".into(),
        source,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_requires_tests() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("package.json"),
            r#"{"scripts":{"lint":"lint"}}"#,
        )
        .unwrap();
        assert!(validation_commands(d.path(), &SupervisorConfig::default()).is_err());
    }
    #[test]
    fn detects_rust_validation() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(
            validation_commands(d.path(), &SupervisorConfig::default()).unwrap(),
            vec!["cargo test --workspace --locked"]
        );
    }
}
