use super::*;
pub(crate) fn source(root: &Path, path: &str, max: u64) -> Result<(std::path::PathBuf, Vec<u8>)> {
    let relative = Path::new(path);
    if relative
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || path.is_empty()
    {
        return Err(error("Source must be a relative repository path"));
    }
    if relative.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s == ".git"
            || s == ".macc"
            || s == "node_modules"
            || s == "target"
            || s == "dist"
            || s.starts_with(".env")
            || s == "secrets"
            || s.ends_with(".pem")
            || s.ends_with(".key")
    }) {
        return Err(error("Restricted/generated source path"));
    }
    let root = root.canonicalize().map_err(|e| error(e.to_string()))?;
    let mut current = root.clone();
    for c in relative.components() {
        current.push(c);
        if std::fs::symlink_metadata(&current)
            .map_err(|e| error(e.to_string()))?
            .file_type()
            .is_symlink()
        {
            return Err(error("Source symlinks are not allowed"));
        }
    }
    if !current.is_file()
        || std::fs::metadata(&current)
            .map_err(|e| error(e.to_string()))?
            .len()
            > max
    {
        return Err(error("Source is not a bounded regular file"));
    }
    use std::io::Read;
    let mut data = Vec::new();
    std::fs::File::open(&current)
        .map_err(|e| error(e.to_string()))?
        .take(max + 1)
        .read_to_end(&mut data)
        .map_err(|e| error(e.to_string()))?;
    if data.len() as u64 > max {
        return Err(error("Source grew beyond its bound"));
    }
    Ok((current, data))
}
pub(crate) fn safe_text(text: &str) -> Result<()> {
    if text.contains('\0') || !crate::security::scan_bytes("o2", text.as_bytes()).is_empty() {
        return Err(error(
            "Memory/context contains credentials or binary text; use a safe reference",
        ));
    }
    Ok(())
}
