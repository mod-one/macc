//! Isolated repair, independent verification, and integration against an exact base.
use super::report::{Intervention, Progress};
use super::tool::Agent;
use macc_core::supervisor::incident::Incident;
#[cfg(test)]
use macc_core::supervisor::SupervisorConfig;
use macc_core::{MaccError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnosis {
    /// Optional absolute provider reset time, copied into the durable wait policy.
    #[serde(default)]
    pub retry_at: Option<String>,
    #[serde(default)]
    pub unavailable_tool: Option<String>,
    pub summary: String,
    pub repairable: bool,
    #[serde(default)]
    pub requires_human: bool,
    #[serde(default)]
    pub task_ids: Vec<String>,
    #[serde(default)]
    pub macc_findings: Vec<String>,
    /// No source repair is needed: restarting the coordinator resumes the
    /// work (the engine stopped on a MACC error or transient condition).
    #[serde(default)]
    pub restart_only: bool,
}
#[derive(Debug, Deserialize)]
struct Verification {
    verified: bool,
    explanation: String,
    /// Acceptance needs a decision the repair agent has no authority to make.
    #[serde(default)]
    requires_human: bool,
}
const VERIFICATION_FILE: &str = ".macc/supervisor-verification.json";
/// Result of one repair round.
enum Round {
    Accepted(String),
    /// What the repair agent must still address, and the error if no round is left.
    Rework(String, MaccError),
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
    let prompt=format!("You are the MACC supervisor diagnosing a coordinator incident. Project: {}. Isolated analysis worktree: {}. Read the original coordinator and performer logs under {}/.macc/log, the task requirements and source. Identify root causes, distinguishing project defects, missing migrations, external conditions, and MACC defects. Prefer recorded unmet_preconditions/result_explanation over normalized error fragments. Do not edit source, reset files, invoke a coordinator, change task state, or approve human gates. Write exactly one JSON object to .macc/supervisor-diagnosis.json: {{\"summary\":\"evidence-based diagnosis\",\"repairable\":true,\"requires_human\":false,\"restart_only\":false,\"task_ids\":[\"root task ids verified by evidence\"],\"macc_findings\":[\"MACC dysfunction and concrete improvement suggestions\"]}}. Set restart_only=true when no source change is needed and restarting the coordinator resumes the work: the coordinator stopped on a MACC engine error or a transient condition while the root tasks were intact (for example between phases: testing, reviewing, changes_requested), and their pending phase (test, review, fix, merge) will handle any project finding such as a changes-requested review. A missing human decision, external credential or explicit operator gate requires_human=true. Source/schema fixes are repairable by this authorized supervisor even when outside the original task's writable scope; a task with out_of_scope_fix names the paths, owned by another module, where its precondition must be fixed (for example an API route whose declared audience or contract does not serve the consuming web task). For an external quota, also include retry_at as an absolute RFC3339 provider reset time and unavailable_tool as its tool ID when supported by evidence; omit them when unknown. Diagnose first; do not repair in this phase.\nIncident:\n{evidence}",root.display(),worktree.display(),root.display());
    remove_response(&worktree.join(".macc/supervisor-diagnosis.json"))?;
    // A retried incident diagnoses in a slot that already holds repair work.
    let before = super::workspace::snapshot(worktree)?;
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
    if super::workspace::snapshot(worktree)? != before {
        return Err(MaccError::Validation(
            "Diagnosis modified source; analysis worktree retained for inspection".into(),
        ));
    }
    Ok(diagnosis)
}
pub use macc_core::supervisor::validation::commands as validation_commands;
const VALIDATION_REGRESSION: &str = "Validation failed";
/// Detail prefix of a repair the verification referred to a human.
pub const HUMAN_DECISION: &str = "Repair needs a human decision";
/// Outcome of one validation command, bounded by the intervention timeout.
async fn run_validation(worktree: &Path, command: &str, log: &Path, timeout: u64) -> Result<bool> {
    let file = std::fs::File::create(log).map_err(|e| io(log, e))?;
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .current_dir(worktree)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file.try_clone().map_err(|e| io(log, e))?))
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
        .map_err(|_| MaccError::Coordinator {
            code: "E101",
            message: format!("Validation timed out: {command}"),
        })?
        .map_err(|e| io(worktree, e))?;
    Ok(status.success())
}

/// Run project validation against the repair. A failing command is re-run on
/// the unmodified base in the same slot (same dependencies and environment):
/// only a command that passes there and fails with the repair blocks
/// integration. Commands already failing on the base are returned so the
/// independent verification can compare both logs for added failures.
pub async fn validate(
    worktree: &Path,
    _timeout: u64,
    progress: &Progress<'_>,
    report: &mut Intervention,
) -> Result<Vec<String>> {
    let dir = progress.dir;
    let commands = report.validation.clone();
    let mut preexisting = Vec::new();
    for (i, command) in commands.iter().enumerate() {
        let position = format!("{}/{}", i + 1, commands.len());
        progress.step(report, format!("Validation {position}: {command}"))?;
        let log = dir.join(format!("validation-{i}.log"));
        if run_validation(
            worktree,
            command,
            &log,
            progress.config.validation_timeout_seconds,
        )
        .await?
        {
            continue;
        }
        progress.step(
            report,
            format!("Validation {position} failed; re-running on the unmodified base: {command}"),
        )?;
        super::workspace::enter_baseline(worktree)?;
        let baseline = run_validation(
            worktree,
            command,
            &dir.join(format!("baseline-{i}.log")),
            progress.config.validation_timeout_seconds,
        )
        .await;
        super::workspace::restore_repair(worktree)?;
        if baseline? {
            return Err(MaccError::Validation(format!(
                "{VALIDATION_REGRESSION}: {command} passes on the unmodified base and fails with the repair; see {}",
                log.display()
            )));
        }
        preexisting.push(command.clone());
    }
    Ok(preexisting)
}
/// Repair, validate and independently verify, feeding a rejection back to the
/// repair agent for another round in the same worktree until the repair is
/// accepted, a human decision is needed, or `max_repair_rounds` is spent.
pub async fn fix(
    agent: &Agent,
    progress: &Progress<'_>,
    worktree: &Path,
    diagnosis: &Diagnosis,
    timeout: u64,
    report: &mut Intervention,
) -> Result<String> {
    let rounds = progress.config.max_repair_rounds.max(1);
    // A kept slot may hold a repair that an earlier attempt's verification rejected.
    let mut feedback = read_json::<Verification>(&worktree.join(VERIFICATION_FILE))
        .ok()
        .filter(|previous| !previous.verified)
        .map(|previous| previous.explanation);
    let mut round = 1;
    loop {
        report.repair_rounds = round;
        if round > 1 {
            archive_round(progress.dir, round - 1)?;
        }
        let position = (round, rounds);
        let outcome = repair_round(
            agent,
            progress,
            worktree,
            diagnosis,
            timeout,
            report,
            position,
            feedback.as_deref(),
        )
        .await?;
        match outcome {
            Round::Accepted(proof) => return Ok(proof),
            Round::Rework(_, error) if round >= rounds => return Err(error),
            Round::Rework(reason, _) => feedback = Some(reason),
        }
        round += 1;
    }
}
/// Keep the validation logs of a rejected round apart from the next round's.
fn archive_round(dir: &Path, round: u32) -> Result<()> {
    let archive = dir.join(format!("round-{round}"));
    std::fs::create_dir_all(&archive).map_err(|e| io(&archive, e))?;
    for entry in std::fs::read_dir(dir).map_err(|e| io(dir, e))? {
        let entry = entry.map_err(|e| io(dir, e))?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if (text.starts_with("validation-") || text.starts_with("baseline-"))
            && text.ends_with(".log")
        {
            std::fs::rename(entry.path(), archive.join(&name)).map_err(|e| io(&archive, e))?;
        }
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
async fn repair_round(
    agent: &Agent,
    progress: &Progress<'_>,
    worktree: &Path,
    diagnosis: &Diagnosis,
    timeout: u64,
    report: &mut Intervention,
    (round, rounds): (u32, u32),
    feedback: Option<&str>,
) -> Result<Round> {
    let (root, dir) = (progress.paths.root.as_path(), progress.dir);
    let commands = report.validation.clone();
    let phase = |name: &str| {
        if round == 1 {
            name.to_string()
        } else {
            format!("{name}-round-{round}")
        }
    };
    let activity = match feedback {
        Some(_) => format!(
            "AI repair with {}, round {round}/{rounds}: addressing the rejection",
            agent.id
        ),
        None => format!("AI repair with {} in the isolated worktree", agent.id),
    };
    progress.enter(report, "repairing", activity)?;
    let rework = feedback.map(|reason| format!("\nThe previous state of this repair was rejected. Keep the valid work already in the worktree and address every point of the rejection: {reason}\nAn existing assertion may change only when it pins exactly the defective behaviour named in the diagnosis; justify each such change in .macc/repair-notes.md. If a point needs a decision outside your authority, do not guess: state it in .macc/repair-notes.md and leave that point unchanged.")).unwrap_or_default();
    let prompt=format!("You are the authorized MACC supervisor repair agent in an isolated worktree. Continue the existing repair in this same worktree. Inspect and preserve previous staged, unstaged and untracked work; do not reset or clean it. Repair these diagnosed root causes: {}. Source and database migration changes needed for these roots are authorized even outside the original performer scope. Preserve existing behavior and regression assertions; never bypass MFA or security, weaken tests, fabricate approval, change PRD/state/.macc metadata, or restart the coordinator. Integration rejects any change to prd.json, *.prd.json, .macc/, .env or .env.* (including .env.example): document new settings elsewhere. Add targeted regression tests. Install project dependencies if needed. Do not commit: the supervisor owns commits. Record intervention evidence and MACC improvement recommendations in .macc/repair-notes.md. Read original logs at {}. Validation commands are {:?}; a command that already fails on the unmodified base does not block integration, but the repair must not add failures to it. Report missing external authority without guessing it.{rework}",diagnosis.summary,root.join(".macc/log").display(),commands);
    agent.run(worktree, &prompt, dir, &phase("repair")).await?;
    progress.enter(
        report,
        "validating",
        if progress.config.tests_enabled {
            "Running project validation"
        } else {
            "Project tests skipped by configuration"
        },
    )?;
    let preexisting = match validate(worktree, timeout, progress, report).await {
        Ok(preexisting) => preexisting,
        // A regression the repair introduced is the repair agent's to fix.
        Err(MaccError::Validation(reason)) if reason.starts_with(VALIDATION_REGRESSION) => {
            return Ok(Round::Rework(reason.clone(), MaccError::Validation(reason)));
        }
        Err(error) => return Err(error),
    };
    report.preexisting_failures = preexisting.clone();
    if !progress.config.ai_verification_enabled {
        return Ok(Round::Accepted(format!(
            "AI verification disabled by supervisor configuration; tests_enabled={}; preexisting failures={preexisting:?}", progress.config.tests_enabled
        )));
    }
    let baseline_note = if !progress.config.tests_enabled {
        " Project validation was explicitly disabled by the operator; no validation logs are expected. Review the source and regression tests without claiming commands ran.".to_string()
    } else if preexisting.is_empty() {
        String::new()
    } else {
        format!(" These validation commands fail on the unmodified base too and were not used as a gate: {preexisting:?}. For each, compare validation-N.log (with the repair) against baseline-N.log (unmodified base) in that directory, N being the position in {commands:?} counted from 0; use false if the repair adds any failure absent from the baseline, or if the logs cannot establish that it does not.")
    };
    let rework_note = feedback.map(|reason| format!(" This is repair round {round}: an earlier state of the repair was rejected for: {reason} Check that each point is resolved and judge the whole repair again; logs of earlier rounds are under round-N/ in that directory.")).unwrap_or_default();
    git(worktree, &["add", "--all"])?;
    let validated_tree = git(worktree, &["write-tree"])?;
    let validated_head = git(worktree, &["rev-parse", "HEAD"])?;
    let prompt=format!("Independently verify the supervisor repair. Diagnosis: {}. Examine source changes, existing and added regression tests and validation logs at {}. Do not edit files. Confirm the original root causes are corrected; no security checks, assertions, validation scripts or human gates were weakened or bypassed. Write .macc/supervisor-verification.json as {{\"verified\":true,\"explanation\":\"concrete evidence\",\"requires_human\":false}}; use false for unresolved causes, unjustified test changes, or missing proof. When false, the explanation is handed to the repair agent: state exactly what must change. Set requires_human to true only when accepting the repair needs a product, security or access-policy decision that neither the diagnosis nor the project documentation settles.{baseline_note}{rework_note} Task ids: {:?}.",diagnosis.summary,dir.display(),diagnosis.task_ids);
    remove_response(&worktree.join(VERIFICATION_FILE))?;
    progress.enter(
        report,
        "verifying",
        format!("Independent AI verification with {}", agent.id),
    )?;
    agent
        .run(worktree, &prompt, dir, &phase("verification"))
        .await?;
    git(worktree, &["add", "--all"])?;
    if git(worktree, &["write-tree"])? != validated_tree
        || git(worktree, &["rev-parse", "HEAD"])? != validated_head
    {
        return Err(MaccError::Validation(
            "Verification modified validated source; integration refused".into(),
        ));
    }
    let verified: Verification = read_json(&worktree.join(VERIFICATION_FILE))?;
    // A human gate always wins, including contradictory AI output.
    if verified.requires_human {
        return Err(MaccError::Validation(format!(
            "{HUMAN_DECISION}: {}",
            verified.explanation
        )));
    }
    if verified.verified {
        return Ok(Round::Accepted(verified.explanation));
    }
    let error = MaccError::Validation(format!(
        "Repair verification rejected after {round} repair round(s): {}",
        verified.explanation
    ));
    Ok(Round::Rework(verified.explanation, error))
}
/// Commit the verified repair and publish it to the reference branch the way
/// the coordinator publishes task merges. Operator checkouts are never
/// required to be clean: Git refuses a fast-forward that would overwrite
/// local changes, and nothing is modified in that case.
pub fn integrate(
    root: &Path,
    worktree: &Path,
    target: &super::workspace::Target,
) -> Result<String> {
    use macc_core::coordinator::integration::{publish_commit, PublishOutcome};
    let base = target.base.as_str();
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
    let refused = match &target.branch {
        Some(branch) => match publish_commit(root, branch, &commit, base)? {
            PublishOutcome::Published { .. } | PublishOutcome::UpToDate => None,
            PublishOutcome::BaseMoved { .. } => Some(format!(
                "Reference branch '{branch}' moved during intervention; repair retained, integration deferred"
            )),
            PublishOutcome::BlockedByCheckout {
                worktree: holder,
                git_output,
            } => Some(format!(
                "Reference branch '{branch}' is checked out in {} with local changes that overlap the repair; repair retained, integration deferred: {git_output}",
                holder.display()
            )),
        },
        // Detached project without a reference branch: advance its HEAD only.
        None if git(root, &["rev-parse", "HEAD"])? != base => Some(
            "Project HEAD changed during intervention; repair retained, integration deferred"
                .into(),
        ),
        None => git(root, &["merge", "--ff-only", &commit])
            .err()
            .map(|error| error.to_string()),
    };
    if let Some(reason) = refused {
        // Keep the repair staged on the base so a later attempt can resume it.
        git(worktree, &["reset", "--soft", base])?;
        return Err(MaccError::Validation(reason));
    }
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

fn remove_response(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(path, e)),
    }
}
