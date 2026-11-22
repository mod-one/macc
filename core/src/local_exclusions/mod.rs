//! Local Git visibility derived from creation provenance, shared safely by worktrees.
mod matcher;
#[cfg(test)]
mod tests;
use crate::{project_ownership, MaccError, ProjectPaths, Result};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
pub(super) const BEGIN: &str = "# BEGIN MACC local tool exclusions";
pub(super) const END: &str = "# END MACC local tool exclusions";

/// Keep successfully written assets private even when a later Apply operation fails.
pub(crate) struct AfterWrites<'a> {
    paths: &'a ProjectPaths,
    finished: bool,
}
impl<'a> AfterWrites<'a> {
    pub(crate) fn new(paths: &'a ProjectPaths) -> Result<Self> {
        // Reject broken local policy/provenance before mutating project assets.
        reconcile(paths)?;
        Ok(Self {
            paths,
            finished: false,
        })
    }
    pub(crate) fn finish(mut self) -> Result<Vec<String>> {
        let result = reconcile(self.paths);
        self.finished = result.is_ok();
        result
    }
}
impl Drop for AfterWrites<'_> {
    fn drop(&mut self) {
        if !self.finished {
            if let Err(error) = reconcile(self.paths) {
                tracing::error!(%error, "Cannot restore local Git exclusions after incomplete Apply");
            }
        }
    }
}

/// Strip only complete owned blocks. Unmatched markers and all user bytes survive.
pub(super) fn without_block(text: &str, begin: &str, end: &str) -> String {
    split_owned(text, begin, end).0
}

fn split_owned(text: &str, begin: &str, end: &str) -> (String, Vec<String>) {
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    let mut result = String::new();
    let mut owned = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].trim_end_matches(['\r', '\n']) == begin {
            if let Some(next) = (index + 1..lines.len()).find(|&n| {
                let line = lines[n].trim_end_matches(['\r', '\n']);
                line == begin || line == end
            }) {
                if lines[next].trim_end_matches(['\r', '\n']) == end {
                    owned.extend(
                        lines[index + 1..next]
                            .iter()
                            .map(|line| line.trim_end_matches(['\r', '\n']).to_owned()),
                    );
                    index = next + 1;
                    continue;
                }
            }
        }
        result.push_str(lines[index]);
        index += 1;
    }
    (result, owned)
}

pub(super) fn rewrite(before: &str, rules: &BTreeSet<String>, exceptions: &[&str]) -> String {
    // User patterns in the same file take precedence, including explicit negations.
    let mut text = String::from(BEGIN);
    text.push('\n');
    for rule in std::iter::once("/.macc/*")
        .chain(exceptions.iter().copied())
        .chain(rules.iter().map(String::as_str))
    {
        text.push_str(rule);
        text.push('\n');
    }
    text.push_str(END);
    text.push('\n');
    text.push_str(&without_block(before, BEGIN, END));
    text
}

fn read(path: &Path) -> Result<String> {
    crate::persistent::no_symlink(path)?;
    match fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(crate::persistent::error(e)),
    }
}

fn checkout_roots(root: &Path) -> Result<BTreeSet<PathBuf>> {
    let output = matcher::git(root)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .map_err(crate::persistent::error)?;
    if !output.status.success() {
        return Err(MaccError::Validation(
            "Cannot inspect worktrees for local Git exclusions".into(),
        ));
    }
    let mut roots = BTreeSet::new();
    roots.insert(root.canonicalize().map_err(crate::persistent::error)?);
    for field in output.stdout.split(|&b| b == 0) {
        if let Some(path) = field.strip_prefix(b"worktree ") {
            let path = std::str::from_utf8(path).map_err(crate::persistent::error)?;
            if Path::new(path).is_dir() {
                roots.insert(
                    Path::new(path)
                        .canonicalize()
                        .map_err(crate::persistent::error)?,
                );
            }
        }
    }
    Ok(roots)
}

fn portable_exceptions(roots: &BTreeSet<PathBuf>) -> Result<Vec<&'static str>> {
    let mut exceptions: Vec<_> = crate::gitignore_policy::BASELINE
        .iter()
        .copied()
        .filter_map(|rule| {
            let path = rule.strip_prefix("!/.macc/")?;
            if path.is_empty() {
                return None;
            }
            Some((
                rule,
                (
                    format!(".macc/{}", path.trim_end_matches('/')),
                    path.ends_with('/'),
                ),
            ))
        })
        .collect();
    for root in roots {
        let candidates = exceptions
            .iter()
            .map(|(_, path)| path.clone())
            .collect::<Vec<_>>();
        let hidden = matcher::global_only(root, &candidates)?;
        exceptions.retain(|(_, path)| !hidden.contains(path));
    }
    Ok(exceptions.into_iter().map(|(rule, _)| rule).collect())
}

/// Migration needs privacy guards before portable configuration exists. No provenance
/// reads here: preparing persistent storage must not recursively prepare itself.
pub(crate) fn bootstrap(root: &Path) -> Result<()> {
    let common = crate::git::git_common_dir(root)?;
    crate::persistent::no_symlink(&common.join("info"))?;
    crate::persistent::no_symlink(&common.join("macc-exclusions.lock"))?;
    let _guard = crate::fs_lock::AdvisoryLock::acquire(
        &common.join("macc-exclusions.lock"),
        Duration::from_secs(5),
        "local Git exclusions",
    )?;
    let path = common.join("info/exclude");
    let before = read(&path)?;
    // Keep existing tool rules until full Apply/Clear can reconcile provenance.
    let (_, owned) = split_owned(&before, BEGIN, END);
    let rules = owned
        .into_iter()
        .filter(|line| line.starts_with('/') && !line.starts_with("/.macc/"))
        .collect();
    let exceptions = portable_exceptions(&checkout_roots(root)?)?;
    let after = rewrite(&before, &rules, &exceptions);
    if after != before {
        fs::create_dir_all(common.join("info")).map_err(crate::persistent::error)?;
        let temp = tempfile::NamedTempFile::new_in(common.join("info"))
            .map_err(crate::persistent::error)?;
        fs::write(temp.path(), after).map_err(crate::persistent::error)?;
        temp.persist(&path).map_err(crate::persistent::error)?;
    }
    Ok(())
}

/// Git metacharacters in literal owned paths must never become wildcard exclusions.
fn pattern(relative: &str, directory: bool) -> String {
    let mut escaped = String::from("/");
    for ch in relative.chars() {
        if "\\*?[] !#".contains(ch) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    if directory {
        escaped.push('/');
    }
    escaped
}

/// Runtime writes also happen outside Apply. Reused assets normally already have
/// effective rules, so avoid rescanning every linked checkout on each dispatch.
pub(crate) fn after_runtime_write(
    paths: &ProjectPaths,
    relative: &str,
    status: crate::plan::ActionStatus,
) -> Result<()> {
    if status != crate::plan::ActionStatus::Created
        && matcher::git(&paths.root)
            .args(["check-ignore", "-q", "--", relative])
            .output()
            .is_ok_and(|result| result.status.success())
    {
        return Ok(());
    }
    refresh_runtime(paths)
}

pub(crate) fn refresh_runtime(paths: &ProjectPaths) -> Result<()> {
    for warning in reconcile(paths)? {
        tracing::warn!("{warning}");
    }
    Ok(())
}

/// Rebuild the owned block from all registered checkout inventories, never declarations.
/// No Git repository means there is no local exclusion file to manage yet.
pub(crate) fn reconcile(paths: &ProjectPaths) -> Result<Vec<String>> {
    let Ok(common) = crate::git::git_common_dir(&paths.root) else {
        return Ok(vec![]);
    };
    crate::persistent::no_symlink(&common.join("info"))?;
    crate::persistent::no_symlink(&common.join("macc-exclusions.lock"))?;
    let _guard = crate::fs_lock::AdvisoryLock::acquire(
        &common.join("macc-exclusions.lock"),
        Duration::from_secs(5),
        "local Git exclusions",
    )?;
    let exclude = common.join("info/exclude");
    let before = read(&exclude)?;
    let user_exclude = without_block(&before, BEGIN, END);
    let roots = checkout_roots(&paths.root)?;
    let exceptions = portable_exceptions(&roots)?;
    let scopes = roots
        .into_iter()
        .map(|root| {
            let paths = ProjectPaths::from_root(&root);
            project_ownership::read(&paths).map(|inventory| (paths, inventory))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut candidates = BTreeSet::new();
    for (_, inventory) in &scopes {
        candidates.extend(inventory.directories.iter().map(|p| (p.clone(), true)));
        candidates.extend(inventory.files.keys().map(|p| (p.clone(), false)));
    }
    candidates.retain(|(p, _)| {
        !p.chars().any(char::is_control) && p != ".gitignore" && !p.starts_with(".macc/")
    });
    let mut warnings = vec![];
    let mut selected: Vec<(String, bool)> = vec![];
    for (relative, directory) in candidates {
        if selected
            .iter()
            .any(|(p, dir)| *dir && relative.starts_with(&format!("{p}/")))
        {
            continue;
        }
        let safe =
            scopes.iter().all(
                |(scope, inv)| match scope.root.join(&relative).symlink_metadata() {
                    Err(error) => error.kind() == std::io::ErrorKind::NotFound,
                    Ok(metadata) if directory => {
                        metadata.is_dir() && inv.directories.contains(&relative)
                    }
                    Ok(metadata) => metadata.is_file() && inv.files.contains_key(&relative),
                },
            );
        if safe {
            selected.push((relative, directory));
        } else if !directory {
            warnings.push(format!("MACC-EXCLUDE-001: {relative} is user-owned in another worktree; no shared local exclusion was added"));
        }
    }
    // A rule can be omitted only if every checkout is already covered by user policy.
    let mut covered_everywhere: BTreeSet<_> = selected.iter().cloned().collect();
    for (scope, _) in &scopes {
        let covered = matcher::covered(&scope.root, &user_exclude, &selected)?;
        covered_everywhere.retain(|candidate| covered.contains(candidate));
    }
    let rules = selected
        .iter()
        .filter(|p| !covered_everywhere.contains(*p))
        .map(|(p, dir)| pattern(p, *dir))
        .collect();
    let after = rewrite(&before, &rules, &exceptions);
    for (scope, _) in &scopes {
        let effective = matcher::covered(&scope.root, &after, &selected)?;
        for candidate in &selected {
            if !effective.contains(candidate) {
                warnings.push(format!("MACC-EXCLUDE-002: {} remains visible in {}; a higher-priority user Git rule overrides the local exclusion", candidate.0, scope.root.display()));
            }
        }
        let tracked = matcher::git(&scope.root)
            .args(["ls-files", "-z"])
            .output()
            .map_err(crate::persistent::error)?;
        if !tracked.status.success() {
            return Err(crate::persistent::error(
                "Cannot inspect tracked generated assets",
            ));
        }
        for file in tracked.stdout.split(|&b| b == 0).filter(|p| !p.is_empty()) {
            let file = String::from_utf8_lossy(file);
            if selected
                .iter()
                .any(|(p, dir)| file == p.as_str() || (*dir && file.starts_with(&format!("{p}/"))))
            {
                warnings.push(format!("MACC-EXCLUDE-003: {file} is already tracked in {}; local exclusions do not untrack files", scope.root.display()));
            }
        }
    }
    if after != before {
        fs::create_dir_all(common.join("info")).map_err(crate::persistent::error)?;
        crate::atomic_write(paths, &exclude, after.as_bytes())?;
    }
    Ok(warnings)
}
