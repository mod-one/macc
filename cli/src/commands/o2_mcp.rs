//! Stdio belongs to the CLI transport; capability scope is verified by core.
use macc_core::{
    o2::{agent::Credentials, mcp},
    MaccError, Result,
};
fn required(name: &str) -> Result<String> {
    std::env::var(name)
        .map_err(|_| MaccError::Validation(format!("Scoped MCP requires runner-provided {name}")))
}
pub(super) fn run() -> Result<()> {
    let root = std::path::PathBuf::from(required("MACC_O2_PROJECT_ROOT")?);
    let credentials = Credentials {
        session_id: required("MACC_O2_MCP_SESSION")?,
        generation: required("MACC_O2_MCP_GENERATION")?
            .parse()
            .map_err(|_| MaccError::Validation("Invalid MCP capability generation".into()))?,
        token: required("MACC_O2_MCP_TOKEN")?,
    };
    mcp::serve(
        &root,
        credentials,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
}
