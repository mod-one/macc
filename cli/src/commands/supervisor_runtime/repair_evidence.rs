//! Bind supervisor repair/verification to their actual worktree and exact trees.
use super::{repair::git, report::write_json};
use macc_core::{supervisor::incident::Incident, MaccError, Result};
use serde_json::{json, Value};
use std::path::Path;

pub(super) const EMPTY: &str = "MACC-SUPERVISOR-EMPTY-REPAIR";
pub(super) const OUTSIDE: &str = "MACC-SUPERVISOR-REPAIR-LOCATION";

pub(super) fn snapshot(path: &Path) -> Result<Value> {
    Ok(json!({
        "worktree":path.canonicalize().map_err(|e|MaccError::Validation(e.to_string()))?,
        "head":git(path,&["rev-parse","HEAD"])?,
        "branch":git(path,&["branch","--show-current"])?,
        "index_tree":git(path,&["write-tree"])?,
        "content_tree":macc_core::git::worktree_content_tree(path)?,
        "changed_paths":git(path,&["diff","HEAD","--name-only","-z"])?.split('\0').filter(|p|!p.is_empty()).collect::<Vec<_>>()
    }))
}

pub(super) fn retained(root: &Path, incident: &Incident) -> Result<Vec<Value>> {
    let mut paths = std::collections::BTreeSet::from([root.to_path_buf()]);
    for task in &incident.tasks {
        if let Some(path) = task.worktree_path().map(Path::new).filter(|p| p.exists()) {
            paths.insert(path.to_path_buf());
        }
    }
    paths.into_iter().map(|p| snapshot(&p)).collect()
}

pub(super) fn unchanged(
    root: &Path,
    incident: &Incident,
    before: &[Value],
    dir: &Path,
    phase: &str,
) -> Result<()> {
    let after = retained(root, incident)?;
    write_json(
        &dir.join(format!("{phase}-retained-work.json")),
        &json!({"before":before,"after":after,"unchanged":before==after,"work_preserved":true}),
    )?;
    if before != after {
        return Err(MaccError::Validation(format!("{OUTSIDE}: source or index changed outside the isolated repair worktree; see {phase}-retained-work.json. Original files are preserved. Resume unfinished task corrections in their original workers through the coordinator fix/review cycle; no repair commit or recovery is inferred.")));
    }
    Ok(())
}

pub(super) fn require_changes(worktree: &Path) -> Result<()> {
    if git(worktree, &["diff", "--cached", "--name-only"])?.is_empty() {
        return Err(MaccError::Validation(format!("{EMPTY}: no source repair exists in {}. An unchanged base is not a repair commit. Retained task delivery must resume in its original worker through the native fix/review cycle; do not import or discard it.", worktree.display())));
    }
    Ok(())
}

pub(super) fn verified(worktree: &Path, expected: &Value) -> Result<()> {
    if snapshot(worktree)? != *expected {
        return Err(MaccError::Validation("Verified repair source, index, HEAD or worktree changed before integration; original work preserved".into()));
    }
    require_changes(worktree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::coordinator::model::Task;
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Incident) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-q", "-b", "main"]).unwrap();
        git(root, &["config", "user.name", "Test"]).unwrap();
        git(root, &["config", "user.email", "test@example.invalid"]).unwrap();
        std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
        std::fs::write(root.join("source.txt"), "baseline").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-qm", "baseline"]).unwrap();
        std::fs::create_dir_all(root.join(".macc/worktree")).unwrap();
        for name in ["worker", "supervisor"] {
            let p = root.join(".macc/worktree").join(name);
            git(
                root,
                &["worktree", "add", "-b", name, p.to_str().unwrap(), "main"],
            )
            .unwrap();
            std::fs::create_dir_all(p.join(".macc")).unwrap();
        }
        let worker = root.join(".macc/worktree/worker");
        let task:Task=serde_json::from_value(json!({"id":"A","state":"blocked","worktree":{"worktree_path":worker,"branch":"worker","base_branch":"main"}})).unwrap();
        let incident = Incident {
            id: "incident".into(),
            run_id: "run".into(),
            epoch: 1,
            reason: "source defect".into(),
            tasks: vec![task],
            recent_events: vec![],
            run_status: Some("blocked".into()),
        };
        let supervisor = root.join(".macc/worktree/supervisor");
        (tmp, supervisor, incident)
    }
    #[test]
    fn unchanged_supervisor_base_is_never_a_repair_commit() {
        let (tmp, repair, incident) = fixture();
        let root = tmp.path();
        let base = git(root, &["rev-parse", "HEAD"]).unwrap();
        let target = super::super::workspace::Target {
            branch: Some("main".into()),
            base: base.clone(),
        };
        let proof = snapshot(&repair).unwrap();
        let error =
            super::super::repair::integrate(root, &repair, &target, &incident, &proof).unwrap_err();
        assert!(error.to_string().contains(EMPTY));
        assert_eq!(git(root, &["rev-parse", "HEAD"]).unwrap(), base);
    }
    #[test]
    fn edits_in_the_original_worker_are_preserved_and_refuse_the_wrong_transaction() {
        let (tmp, repair, incident) = fixture();
        let root = tmp.path();
        let before = retained(root, &incident).unwrap();
        let worker = Path::new(incident.tasks[0].worktree_path().unwrap());
        std::fs::write(worker.join("source.txt"), "staged correction").unwrap();
        git(worker, &["add", "source.txt"]).unwrap();
        std::fs::write(worker.join("source.txt"), "retained final correction").unwrap();
        std::fs::write(worker.join("proof.txt"), "untracked evidence").unwrap();
        let dir = root.join(".macc/incident");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(unchanged(root, &incident, &before, &dir, "repair")
            .unwrap_err()
            .to_string()
            .contains(OUTSIDE));
        assert_eq!(
            std::fs::read_to_string(worker.join("source.txt")).unwrap(),
            "retained final correction"
        );
        assert_eq!(
            git(worker, &["show", ":source.txt"]).unwrap(),
            "staged correction"
        );
        assert_eq!(
            std::fs::read_to_string(worker.join("proof.txt")).unwrap(),
            "untracked evidence"
        );
        assert_eq!(git(&repair, &["diff", "HEAD"]).unwrap(), "");
        let receipt: Value =
            serde_json::from_slice(&std::fs::read(dir.join("repair-retained-work.json")).unwrap())
                .unwrap();
        assert_eq!(receipt["unchanged"], false);
        assert_eq!(receipt["work_preserved"], true);
    }
    #[test]
    fn integration_is_bound_to_the_verified_files_index_branch_and_head() {
        let (tmp, repair, incident) = fixture();
        let root = tmp.path();
        std::fs::write(repair.join("source.txt"), "actual correction").unwrap();
        super::super::repair_changes::prepare(&repair, &incident).unwrap();
        let proof = snapshot(&repair).unwrap();
        std::fs::write(repair.join("source.txt"), "unreviewed change").unwrap();
        assert!(verified(&repair, &proof).is_err());
        std::fs::write(repair.join("source.txt"), "actual correction").unwrap();
        std::fs::write(repair.join("new.txt"), "unverified untracked source").unwrap();
        assert!(verified(&repair, &proof).is_err());
        std::fs::remove_file(repair.join("new.txt")).unwrap();
        let base = git(root, &["rev-parse", "HEAD"]).unwrap();
        let target = super::super::workspace::Target {
            branch: Some("main".into()),
            base: base.clone(),
        };
        let commit =
            super::super::repair::integrate(root, &repair, &target, &incident, &proof).unwrap();
        assert_ne!(commit, base);
        assert_eq!(git(root, &["rev-parse", "main"]).unwrap(), commit);
        assert_eq!(
            git(root, &["show", "main:source.txt"]).unwrap(),
            "actual correction"
        );
        assert_eq!(
            git(
                Path::new(incident.tasks[0].worktree_path().unwrap()),
                &["show", "HEAD:source.txt"]
            )
            .unwrap(),
            "baseline"
        );
    }
}
