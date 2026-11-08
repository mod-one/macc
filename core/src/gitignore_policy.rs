//! Ordered managed Git policy; portable configuration is distinct from local authority.
use crate::Result;
use std::{collections::BTreeSet, path::Path};
pub(crate) const BEGIN: &str = "# BEGIN MACC project visibility";
pub(crate) const END: &str = "# END MACC project visibility";
pub const BASELINE: &[&str] = &[
    "!/.macc/",
    "/.macc/*",
    "!/.macc/macc.yaml",
    "!/.macc/catalog/",
    "!/.macc/skills.lock.json",
];
pub fn rewrite(existing: &str, requested: &BTreeSet<String>) -> String {
    let managed =
        requested.contains("/.macc/*") || requested.contains(".macc/") || existing.contains(BEGIN);
    let source: Vec<_> = existing.lines().collect();
    let mut lines = Vec::new();
    let mut index = 0;
    while index < source.len() {
        if source[index] == BEGIN {
            let next = source[index + 1..]
                .iter()
                .position(|line| *line == BEGIN || *line == END)
                .map(|n| index + 1 + n);
            if let Some(end) = next.filter(|end| source[*end] == END) {
                index = end + 1;
                continue;
            }
        }
        lines.push(source[index].to_string());
        index += 1;
    }
    while lines.last().is_some_and(|s| s.is_empty()) {
        lines.pop();
    }
    if managed {
        lines.push(BEGIN.into());
        lines.extend(BASELINE.iter().map(|s| s.to_string()));
        lines.push(END.into());
    }
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}
/// Local defense in depth, never an allow-rule for config: exclusions only.
pub(crate) fn ensure_local_exclusions(common: &Path) -> Result<()> {
    crate::local_exclusions::bootstrap(common)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_managed_markers_do_not_erase_user_rules() {
        let requested = BASELINE.iter().map(|s| s.to_string()).collect();
        let original = format!("{BEGIN}\n*.private\n# retained comment\n");
        let after = rewrite(&original, &requested);
        assert!(after.starts_with(&original));
        assert_eq!(rewrite(&after, &requested), after);
    }
    #[test]
    fn ordered_policy_is_idempotent_and_preserves_user_patterns() {
        let requested = BASELINE.iter().map(|s| s.to_string()).collect();
        let before = "# user rule\n.macc/\n*.secret\n";
        let after = rewrite(before, &requested);
        assert!(after.starts_with(before));
        assert_eq!(rewrite(&after, &requested), after);
        assert!(after.find("!/.macc/").unwrap() < after.find("/.macc/*").unwrap());
    }
}

#[cfg(test)]
mod git_tests {
    use super::*;
    #[test]
    fn git_sees_portable_files_and_ignores_runtime_even_after_a_legacy_blanket_rule() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(tmp.path())
            .status()
            .unwrap()
            .success());
        let policy = rewrite(
            "# user\n.macc/\n*.secret\n",
            &BASELINE.iter().map(|s| s.to_string()).collect(),
        );
        std::fs::write(tmp.path().join(".gitignore"), policy).unwrap();
        for (path, ignored) in [
            (".macc/macc.yaml", false),
            (".macc/catalog/nested/a.json", false),
            (".macc/catalog/private.secret", true),
            (".macc/skills.lock.json", false),
            (".macc/persistent/control.sqlite", true),
            (".macc/log/a.txt", true),
            (".macc/worktree/integration/file", true),
            (".macc/memory.db", true),
        ] {
            let file = tmp.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "test").unwrap();
            let status = std::process::Command::new("git")
                .args(["check-ignore", "-q", "--", path])
                .current_dir(tmp.path())
                .status()
                .unwrap();
            assert_eq!(status.success(), ignored, "{path}");
        }
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;
    #[test]
    fn tool_rules_stay_inside_managed_section_without_moving_user_rules() {
        let requested = BASELINE
            .iter()
            .copied()
            .chain(["/.codex/", "/AGENTS.md"])
            .map(str::to_owned)
            .collect();
        let original = "# user\n.claude/\n*.private\n";
        let result = rewrite(original, &requested);
        assert!(result.starts_with(original));
        let block = result.split(BEGIN).nth(1).unwrap();
        assert!(!block.contains("/.codex/"));
        assert!(!block.contains("/AGENTS.md"));
        assert!(!block.contains(".claude/"));
        assert_eq!(rewrite(&result, &requested), result);
    }
}

/// Clear releases exclusions for removed generated files, keeping user rules and
/// the runtime/portable setup boundary. Private surviving files remain excluded.
pub(crate) fn after_clear(
    paths: &crate::ProjectPaths,
    _inventory: &crate::project_ownership::Inventory,
    archive: &Path,
) -> Result<()> {
    let path = paths.root.join(".gitignore");
    if !path.is_file() {
        return Ok(());
    }
    let before = std::fs::read_to_string(&path).map_err(crate::persistent::error)?;
    if !before.lines().any(|line| line == BEGIN) || !before.lines().any(|line| line == END) {
        return Ok(());
    }
    let patterns: BTreeSet<String> = BASELINE.iter().map(|s| s.to_string()).collect();
    let after = rewrite(&before, &patterns);
    if after != before {
        crate::atomic_write(
            paths,
            &archive.join("gitignore-before.txt"),
            before.as_bytes(),
        )?;
        crate::atomic_write(paths, &path, after.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod path_line_tests {
    use super::*;
    #[test]
    fn generated_path_cannot_inject_an_allow_rule_for_private_runtime() {
        let mut patterns: BTreeSet<String> = BASELINE.iter().map(|s| s.to_string()).collect();
        patterns.insert("/assets/malicious\n!/.macc/memory.db".into());
        let text = rewrite("", &patterns);
        assert!(!text.contains("!/.macc/memory.db"));
        assert!(text.contains("/.macc/*"));
    }
}
