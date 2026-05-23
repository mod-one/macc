use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../adapters");
    println!("cargo:rerun-if-changed=../registry/tools.d");

    let manifest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR missing"));
    let repo_root = manifest_dir
        .parent()
        .expect("core crate must be inside repository root");
    let adapters_dir = repo_root.join("adapters");

    let mut runner_paths = collect_runner_paths(&adapters_dir)
        .into_iter()
        .filter_map(|p| p.strip_prefix(repo_root).ok().map(path_to_forward_slashes))
        .collect::<Vec<_>>();
    runner_paths.sort();
    runner_paths.dedup();

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR missing"));
    let dest_path = out_dir.join("embedded_automation_runners.rs");

    let mut generated = String::from("pub const EMBEDDED_RUNNERS: &[(&str, &str)] = &[\n");
    for rel in runner_paths {
        let rel_lit = format!("{:?}", rel);
        generated.push_str("    (");
        generated.push_str(&rel_lit);
        generated.push_str(", include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../");
        generated.push_str(&rel);
        generated.push_str("\"))),\n");
    }
    generated.push_str("];\n");

    fs::write(dest_path, generated).expect("failed to write embedded_automation_runners.rs");

    // Dynamically embed tool specifications from registry/tools.d/
    let tools_d_dir = repo_root.join("registry").join("tools.d");
    let mut spec_paths = Vec::new();
    if let Ok(entries) = fs::read_dir(&tools_d_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.ends_with(".tool.yaml") || name.ends_with(".tool.json") {
                    spec_paths.push(path);
                }
            }
        }
    }
    spec_paths.sort();

    let mut specs_gen = String::from("pub const EMBEDDED_TOOL_SPECS: &[(&str, &str)] = &[\n");
    for path in spec_paths {
        let rel = path.strip_prefix(repo_root).unwrap();
        let rel_str = path_to_forward_slashes(rel);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap();
        let name_lit = format!("embedded:{}", file_name);

        specs_gen.push_str(&format!(
            "    ({:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../{}\"))),\n",
            name_lit, rel_str
        ));
    }
    specs_gen.push_str("];\n");

    let dest_specs_path = out_dir.join("embedded_tool_specs.rs");
    fs::write(dest_specs_path, specs_gen).expect("failed to write embedded_tool_specs.rs");
}

fn collect_runner_paths(adapters_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let Ok(entries) = fs::read_dir(adapters_dir) else {
        return paths;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let is_shared = path.file_name().map(|n| n == "shared").unwrap_or(false);
        let Ok(sub_entries) = fs::read_dir(&path) else {
            continue;
        };
        for sub in sub_entries.flatten() {
            let sub_path = sub.path();
            let Some(name) = sub_path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if is_shared && name.ends_with(".sh") {
                // Embed all shared library scripts so adapter performers can source them.
                paths.push(sub_path);
            } else if name.ends_with(".performer.sh") {
                paths.push(sub_path);
            }
        }
    }

    paths
}

fn path_to_forward_slashes(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}
