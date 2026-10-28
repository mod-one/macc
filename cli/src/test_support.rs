pub(crate) mod http;
#[cfg(test)]
pub(crate) fn run_git_ok(dir: &std::path::Path, args: &[&str]) {
    let output = macc_core::git::run_git_output_mapped(dir, args, "run test git command")
        .unwrap_or_else(|e| panic!("git command failed {:?}: {}", args, e));
    if !output.status.success() {
        panic!(
            "git command failed: {:?} -> {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(test)]
pub(crate) fn run_git_ok_with_env(dir: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) {
    let output =
        macc_core::git::run_git_output_with_env_mapped(dir, args, envs, "run test git command")
            .unwrap_or_else(|e| panic!("git command failed {:?}: {}", args, e));
    if !output.status.success() {
        panic!(
            "git command failed: {:?} -> {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(test)]
pub(crate) fn isolate_fixture_project(root: &std::path::Path, tools: &[String]) {
    let paths = macc_core::ProjectPaths::from_root(root);
    let mut config = macc_core::load_canonical_config(&paths.config_path).unwrap();
    config.settings.offline = true;
    config.tools.enabled = tools.to_vec();
    let mut intelligence = macc_core::o2::policy::load(root).unwrap().code_intelligence;
    intelligence.enabled = false;
    config.code_intelligence = Some(intelligence);
    let mut context = macc_core::o2::policy::load(root).unwrap().context_engine;
    context.enabled = false;
    config.context_engine = Some(context);
    macc_core::save_canonical_config(&paths, &config).unwrap();
}
