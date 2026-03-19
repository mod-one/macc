fn main() {
    // Embed build SHA for traceability (set by install.sh or fallback to git).
    if std::env::var("MACC_BUILD_SHA").is_ok() {
        println!("cargo:rerun-if-env-changed=MACC_BUILD_SHA");
    }
    let build_sha = std::env::var("MACC_BUILD_SHA").unwrap_or_else(|_| {
        std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    });
    println!("cargo:rustc-env=MACC_BUILD_SHA={}", build_sha);
}
