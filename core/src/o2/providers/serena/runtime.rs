//! Project-isolated wheel-only SolidLSP runtime; language dependencies are explicit.
use super::*;
pub(super) const VERSION: &str = "2.0.0";
pub(super) const LOCK: &str = include_str!("requirements.lock");
pub(super) const BRIDGE: &str = include_str!("bridge.py");
pub(super) fn owned(root: &Path, path: &str) -> Result<std::path::PathBuf> {
    super::super::generation::owned(root, "serena", path)
}
pub(super) fn tools(root: &Path, p: &CodePolicy) -> Result<Value> {
    let python = super::super::process::executable(&p.serena_python)?;
    let uv = super::super::process::executable(&p.serena_uv)?;
    let work = owned(
        root,
        &format!(
            ".macc/code-intelligence/serena/previews/{}",
            uuid::Uuid::new_v4()
        ),
    )?;
    let result = (|| {
        let run = |exe: &Path| {
            super::super::process::run(
                exe,
                &["--version".into()],
                &work,
                5,
                p.serena_memory_mb,
                65536,
            )
        };
        let py = run(&python)?;
        let uv_version = run(&uv)?;
        if !py.trim().starts_with("Python 3.12.") {
            return Err(error("The managed Serena lock requires Python 3.12"));
        }
        Ok(
            json!({"python":python,"python_sha256":digest(&store::bytes(&python)?),"python_version":py.trim(),"uv":uv,"uv_sha256":digest(&store::bytes(&uv)?),"uv_version":uv_version.trim()}),
        )
    })();
    let _ = std::fs::remove_dir_all(work);
    result
}
pub(super) fn verify(root: &Path, meta: &Value) -> Result<()> {
    if meta["lock_sha256"] != digest(LOCK.as_bytes())
        || meta["bridge_sha256"] != digest(BRIDGE.as_bytes())
    {
        return Err(error(
            "Serena lock or bridge changed; review a new installation",
        ));
    }
    super::super::generation::verify(
        root,
        "serena",
        field(meta, "runtime")?,
        field(meta, "runtime_manifest")?,
    )?;
    if meta["base_python_sha256"] != digest(&store::bytes(Path::new(field(meta, "base_python")?))?)
    {
        return Err(error("Serena base Python changed; reinstall explicitly"));
    }
    Ok(())
}
pub(super) fn field<'a>(meta: &'a Value, key: &str) -> Result<&'a str> {
    meta[key]
        .as_str()
        .ok_or_else(|| error(format!("Missing Serena runtime {key}")))
}
pub(super) fn command(root: &Path, meta: &Value, work: &Path, p: &CodePolicy) -> Result<Value> {
    let base = owned(root, field(meta, "runtime")?)?;
    super::super::process::run(
        &base.join("venv/bin/python"),
        &[
            "-B".into(),
            "-I".into(),
            base.join("bridge.py").to_string_lossy().into_owned(),
        ],
        work,
        p.serena_timeout_seconds,
        p.serena_memory_mb,
        1048576,
    )?;
    if std::fs::metadata(work.join("response.json"))
        .map_err(|e| error(e.to_string()))?
        .len()
        > 1048576
    {
        return Err(error("Serena response exceeded byte limit"));
    }
    use std::io::Read;
    let mut raw = Vec::new();
    std::fs::File::open(work.join("response.json"))
        .map_err(|e| error(e.to_string()))?
        .take(1048577)
        .read_to_end(&mut raw)
        .map_err(|e| error(e.to_string()))?;
    if raw.len() > 1048576 {
        return Err(error("Serena response exceeded byte limit"));
    }
    let result: Value = store::json(&raw)?;
    if let Some(message) = result["error"].as_str() {
        return Err(error(format!("Serena: {message}")));
    }
    Ok(result)
}
pub(super) fn smoke(root: &Path, meta: &Value, p: &CodePolicy) -> Result<()> {
    let work = owned(
        root,
        &format!(
            ".macc/code-intelligence/serena/queries/{}",
            uuid::Uuid::new_v4()
        ),
    )?;
    let result = (|| {
        store::write(
            root,
            &work.join("sources/smoke.py"),
            b"def macc_smoke():\n    return 42\n\ndef caller():\n    return macc_smoke()\n",
        )?;
        let request = json!({"files":["smoke.py"],"text":"macc_smoke","kind":"symbol_definition","path":null,"limit":2,"timeout":p.serena_timeout_seconds});
        store::write(
            root,
            &work.join("request.json"),
            &serde_json::to_vec(&request).map_err(|e| error(e.to_string()))?,
        )?;
        let result = command(root, meta, &work, p)?;
        if result["hits"][0]["path"] != "smoke.py" || result["hits"][0]["line"] != 1 {
            return Err(error("Serena Python/Jedi definition smoke failed"));
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(work);
    result
}
pub(super) fn install(root: &Path, plan: &Plan, p: &CodePolicy) -> Result<Value> {
    let expected = &plan
        .details
        .as_ref()
        .ok_or_else(|| error("Missing reviewed Serena plan details"))?["adapter"]["tools"];
    if tools(root, p)? != *expected {
        return Err(error("Serena Python/uv changed since preview"));
    }
    let generation = super::super::generation::attempt(root, plan)?;
    let base = owned(root, &generation)?;
    store::write(root, &base.join("requirements.lock"), LOCK.as_bytes())?;
    store::write(root, &base.join("bridge.py"), BRIDGE.as_bytes())?;
    let work = base.join("work");
    let run = |args: Vec<String>| {
        super::super::process::run(
            Path::new(field(expected, "uv")?),
            &args,
            &work,
            p.serena_install_timeout_seconds,
            p.serena_memory_mb,
            1048576,
        )
    };
    run(vec![
        "venv".into(),
        "--python".into(),
        field(expected, "python")?.into(),
        "--no-python-downloads".into(),
        base.join("venv").to_string_lossy().into_owned(),
    ])?;
    // Explicitly install the tested SolidLSP dependency subset, excluding Serena UI/AI/MCP extras.
    run(vec![
        "pip".into(),
        "install".into(),
        "--python".into(),
        base.join("venv/bin/python").to_string_lossy().into_owned(),
        "--no-deps".into(),
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
    let mut meta = json!({"runtime":generation,"base_python":expected["python"],"base_python_sha256":expected["python_sha256"],"lock_sha256":digest(LOCK.as_bytes()),"bridge_sha256":digest(BRIDGE.as_bytes()),"language":"python","language_server":"jedi-language-server==0.46.0"});
    smoke(root, &meta, p)?;
    std::fs::remove_dir_all(work).map_err(|e| error(e.to_string()))?;
    meta["runtime_manifest"] = json!(super::super::generation::seal(root, "serena", &generation)?);
    Ok(meta)
}
