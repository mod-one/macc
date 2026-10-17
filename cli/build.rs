use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR missing"));
    let repo_root = manifest_dir
        .parent()
        .expect("cli crate must be inside repository root");

    println!("cargo:rerun-if-env-changed=MACC_BUILD_SHA");
    register_git_rerun_triggers(repo_root);

    let build_sha = std::env::var("MACC_BUILD_SHA")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| resolve_git_build_sha(repo_root));
    println!("cargo:rustc-env=MACC_BUILD_SHA={}", build_sha);
}

fn register_git_rerun_triggers(repo_root: &Path) {
    let git_dir = std::process::Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| {
            let p = PathBuf::from(s.trim());
            if p.is_absolute() {
                p
            } else {
                repo_root.join(p)
            }
        });

    let Some(git_dir) = git_dir else {
        return;
    };

    for rel in ["HEAD", "index", "packed-refs"] {
        let candidate = git_dir.join(rel);
        if candidate.exists() {
            println!("cargo:rerun-if-changed={}", candidate.display());
        }
    }

    if let Ok(head_contents) = fs::read_to_string(git_dir.join("HEAD")) {
        if let Some(ref_rel) = head_contents.trim().strip_prefix("ref: ") {
            let ref_path = git_dir.join(ref_rel.trim());
            if ref_path.exists() {
                println!("cargo:rerun-if-changed={}", ref_path.display());
            }
        }
    }
}

fn resolve_git_build_sha(repo_root: &Path) -> String {
    let sha = std::process::Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let Some(sha) = sha else {
        return "unknown".to_string();
    };

    let is_clean = std::process::Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "--quiet", "HEAD", "--"])
        .status()
        .map(|status| status.success())
        .unwrap_or(true);

    if is_clean {
        sha
    } else {
        format!("{}-dirty", sha)
    }
}
