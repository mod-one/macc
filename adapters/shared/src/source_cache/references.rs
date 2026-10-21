//! An explicit update resolves a small ref advertisement before downloading source objects.
use super::*;
pub(super) fn resolve(source: &Source) -> Result<String> {
    let reference = source
        .reference
        .strip_prefix("refs/remotes/origin/")
        .unwrap_or(&source.reference);
    let requested = if reference.is_empty() {
        "HEAD"
    } else {
        reference
    };
    let branch = if requested.starts_with("refs/heads/") {
        requested.to_string()
    } else {
        format!("refs/heads/{requested}")
    };
    let tag = if requested.starts_with("refs/tags/") {
        requested.to_string()
    } else {
        format!("refs/tags/{requested}")
    };
    let peeled = format!("{tag}^{{}}");
    let output = Command::new("git")
        .args([
            "ls-remote",
            "--exit-code",
            "--",
            &source.url,
            requested,
            &branch,
            &tag,
            &peeled,
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| io(Path::new(&source.url), "resolve explicit source update", e))?;
    if !output.status.success() {
        return Err(MaccError::Validation(format!(
            "Cannot resolve explicit source update ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // Prefer a branch over a tag with the same name; peel annotated tags to commits.
    for wanted in [&branch, &peeled, &tag, &requested.to_string()] {
        if let Some(sha) = text
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .find(|(sha, name)| {
                *name == wanted.as_str() && macc_core::skills_catalog::locking::is_sha(sha)
            })
            .map(|(sha, _)| sha.to_string())
        {
            return Ok(sha);
        }
    }
    Err(MaccError::Validation(format!(
        "No immutable commit found for update ref '{requested}'"
    )))
}
