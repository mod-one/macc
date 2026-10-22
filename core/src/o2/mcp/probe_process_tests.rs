use super::*;

#[test]
fn actual_stdio_transport_detects_startup_timeout_exit_and_bounded_output() {
    let temp = tempfile::tempdir().unwrap();
    let env = json!({"MACC_O2_PROJECT_ROOT":temp.path(),"MACC_O2_MCP_SESSION":"control","MACC_O2_MCP_GENERATION":"1","MACC_O2_MCP_TOKEN":"redacted-control-token"});
    let run = |script: &str, timeout| {
        super::probe_process::run(
            temp.path(),
            &json!({"command":"/bin/sh","args":["-c",script]}),
            &env,
            b"hello\n",
            timeout,
        )
    };
    assert_eq!(
        run("cat", std::time::Duration::from_secs(2)).unwrap(),
        b"hello\n"
    );
    let exit = run("exit 7", std::time::Duration::from_secs(2))
        .unwrap_err()
        .to_string();
    assert!(exit.contains("exit") || exit.contains("input failed"));
    assert!(!exit.contains("redacted-control-token"));
    assert!(run("sleep 5", std::time::Duration::from_millis(30))
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    assert!(super::probe_process::run(
        temp.path(),
        &json!({"command":"/no-such-mcp-command","args":[]}),
        &env,
        b"hello",
        std::time::Duration::from_secs(1)
    )
    .unwrap_err()
    .to_string()
    .contains("startup"));
}
