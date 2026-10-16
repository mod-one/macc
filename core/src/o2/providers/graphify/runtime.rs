//! Pinned wheel-only venv installation. System/user Python environments are never modified.
use super::*;
pub(super) const VERSION: &str = "0.9.92";
pub(super) const LOCK: &str = include_str!("requirements.lock");
pub(super) const LAUNCHER: &str = include_str!("offline.py");
pub(super) fn tools(root: &Path, p: &super::super::super::policy::CodePolicy) -> Result<Value> {
    let python = super::super::process::executable(&p.graphify_python)?;
    let uv = super::super::process::executable(&p.graphify_uv)?;
    let scratch = root.join(format!(
        ".macc/cache/provider-preview-{}",
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let py = super::super::process::run(
            &python,
            &["--version".into()],
            &scratch,
            5,
            p.graphify_memory_mb,
            65536,
        )?;
        let uv_version = super::super::process::run(
            &uv,
            &["--version".into()],
            &scratch,
            5,
            p.graphify_memory_mb,
            65536,
        )?;
        if !py.trim().starts_with("Python 3.12.") {
            return Err(error("This Graphify lock requires Python 3.12; select its executable in Code Intelligence policies"));
        }
        Ok(
            json!({"python":python,"python_sha256":digest(&store::bytes(&python)?),"python_version":py.trim(),"uv":uv,"uv_sha256":digest(&store::bytes(&uv)?),"uv_version":uv_version.trim()}),
        )
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    result
}
pub(super) fn python(root: &Path, meta: &Value) -> Result<std::path::PathBuf> {
    let generation = meta["runtime"]
        .as_str()
        .ok_or_else(|| error("Managed Graphify runtime missing"))?;
    let base = storage::owned(root, generation)?;
    Ok(base.join("venv/bin/python"))
}
pub(super) fn verify(root: &Path, meta: &Value) -> Result<()> {
    if meta["lock_sha256"] != digest(LOCK.as_bytes()) {
        return Err(error(
            "Graphify dependency lock changed; review an update before reuse",
        ));
    }
    storage::verify(
        root,
        meta["runtime"]
            .as_str()
            .ok_or_else(|| error("Missing runtime generation"))?,
        meta["runtime_manifest"]
            .as_str()
            .ok_or_else(|| error("Missing runtime fingerprint"))?,
    )?;
    let py = meta["base_python"]
        .as_str()
        .ok_or_else(|| error("Missing base Python provenance"))?;
    if meta["base_python_sha256"] != digest(&store::bytes(Path::new(py))?) {
        return Err(error("Base Python changed; reinstall Graphify explicitly"));
    }
    Ok(())
}
pub(super) fn command(
    root: &Path,
    meta: &Value,
    args: &[String],
    scratch: &Path,
    p: &super::super::super::policy::CodePolicy,
) -> Result<String> {
    let base = storage::owned(
        root,
        meta["runtime"]
            .as_str()
            .ok_or_else(|| error("Missing runtime"))?,
    )?;
    let mut argv = vec![
        "-B".into(),
        "-I".into(),
        base.join("offline.py").to_string_lossy().into_owned(),
    ];
    argv.extend_from_slice(args);
    super::super::process::run(
        &python(root, meta)?,
        &argv,
        scratch,
        p.graphify_timeout_seconds,
        p.graphify_memory_mb,
        1048576,
    )
}
pub(super) fn install(
    root: &Path,
    plan: &Plan,
    generation: &str,
    p: &super::super::super::policy::CodePolicy,
) -> Result<Value> {
    let expected = &plan
        .details
        .as_ref()
        .ok_or_else(|| error("Reviewed installation details missing"))?["adapter"]["tools"];
    if tools(root, p)? != *expected {
        return Err(error("Python/uv changed since preview; create a new plan"));
    }
    let base = storage::owned(root, generation)?;
    store::write(root, &base.join("requirements.lock"), LOCK.as_bytes())?;
    store::write(root, &base.join("offline.py"), LAUNCHER.as_bytes())?;
    let uv = Path::new(
        expected["uv"]
            .as_str()
            .ok_or_else(|| error("Missing uv executable"))?,
    );
    let python = expected["python"]
        .as_str()
        .ok_or_else(|| error("Missing Python executable"))?;
    let work = base.join("work");
    let run = |args: Vec<String>| {
        super::super::process::run(
            uv,
            &args,
            &work,
            p.graphify_install_timeout_seconds,
            p.graphify_memory_mb,
            1048576,
        )
    };
    run(vec![
        "venv".into(),
        "--python".into(),
        python.into(),
        "--no-python-downloads".into(),
        base.join("venv").to_string_lossy().into_owned(),
    ])?;
    run(vec![
        "pip".into(),
        "install".into(),
        "--python".into(),
        base.join("venv/bin/python").to_string_lossy().into_owned(),
        "--require-hashes".into(),
        "--only-binary".into(),
        ":all:".into(),
        "--no-sources".into(),
        "--no-config".into(),
        "--index-url".into(),
        "https://pypi.org/simple".into(),
        "--link-mode".into(),
        "copy".into(),
        "-r".into(),
        base.join("requirements.lock")
            .to_string_lossy()
            .into_owned(),
    ])?;
    let mut meta = json!({"runtime":generation,"base_python":python,"base_python_sha256":expected["python_sha256"],"lock_sha256":digest(LOCK.as_bytes())});
    let version = command(root, &meta, &["--version".into()], &work, p)?;
    if version.split_whitespace().last() != Some(VERSION) {
        return Err(error(
            "Installed Graphify version does not match the pinned release",
        ));
    }
    // Smoke is a local AST extraction and a bounded query, never a model prompt.
    let sample = work.join("sample");
    std::fs::create_dir_all(&sample).map_err(|e| error(e.to_string()))?;
    std::fs::write(
        sample.join("smoke.py"),
        "def macc_smoke():\n    return 42\n",
    )
    .map_err(|e| error(e.to_string()))?;
    let out = work.clone();
    command(root, &meta, &index::arguments(&sample, &out), &work, p)?;
    let graph = out.join("graphify-out/graph.json");
    let text = command(
        root,
        &meta,
        &[
            "query".into(),
            "macc_smoke".into(),
            "--graph".into(),
            graph.to_string_lossy().into_owned(),
            "--budget".into(),
            "400".into(),
        ],
        &work,
        p,
    )?;
    if !text.contains("NODE macc_smoke") {
        return Err(error("Graphify AST/query smoke failed"));
    }
    let _ = std::fs::remove_dir_all(&work);
    meta["runtime_manifest"] = json!(storage::seal(root, generation)?);
    Ok(meta)
}
