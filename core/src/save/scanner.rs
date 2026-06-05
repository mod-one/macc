pub fn redact_secrets_in_text(text: &str) -> String {
    let mut redacted = text.to_string();
    let regexes = [
        regex::Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
        regex::Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(),
        regex::Regex::new(r"ghp_[a-zA-Z0-9]{36}").unwrap(),
    ];
    for re in &regexes {
        redacted = re.replace_all(&redacted, "[REDACTED]").into_owned();
    }
    redacted
}
