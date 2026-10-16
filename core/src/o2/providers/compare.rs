//! Explicit A/B query comparison, with no model calls or token-savings claims.
use super::*;
pub fn compare(root: &Path, id: &str, q: &Query) -> Result<serde_json::Value> {
    let run = |provider: &str| -> Result<serde_json::Value> {
        let start = std::time::Instant::now();
        let result = query_selected(root, q, Some(provider))?;
        Ok(
            serde_json::json!({"provider":provider,"elapsed_ms":start.elapsed().as_millis(),"result":result}),
        )
    };
    Ok(
        serde_json::json!({"baseline":run("native")?,"candidate":run(id)?,"quality":"local deterministic comparison; no provider billing or quality guarantee"}),
    )
}
