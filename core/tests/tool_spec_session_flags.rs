//! Guardrail: no shipped tool spec may use a *create* flag where a *resume*
//! flag belongs.
//!
//! `claude --session-id <uuid>` opens a conversation under a caller-chosen id.
//! Used as the resume command it succeeds exactly once per id, then fails with
//! `Error: Session ID <uuid> is already in use.` on every later call. Because
//! MACC pools and re-issues session ids (`tool-sessions.json`), a single wrong
//! flag turns every dispatch after the first into an instant
//! `error_without_changes`. One shipped run burned 11 minutes and 30 dispatches
//! on one task this way while 24 others never started.
//!
//! This asserts the property against the specs MACC actually ships, so the
//! regression cannot return via a hand-edited YAML that never round-trips
//! through `ToolSpec::validate`.

use macc_core::tool::spec::{create_only_flag_in, ToolSpec};
use std::path::PathBuf;

fn registry_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core crate should live one level below repo root")
        .join("registry/tools.d")
}

fn shipped_specs() -> Vec<(String, ToolSpec)> {
    let mut specs = Vec::new();
    for entry in std::fs::read_dir(registry_dir()).expect("read registry/tools.d") {
        let path = entry.expect("dir entry").path();
        if !path.to_string_lossy().ends_with(".tool.yaml") {
            continue;
        }
        let name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .into_owned();
        let raw = std::fs::read_to_string(&path).expect("read tool spec");
        let spec: ToolSpec =
            serde_yaml::from_str(&raw).unwrap_or_else(|e| panic!("parse {name}: {e}"));
        specs.push((name, spec));
    }
    assert!(
        !specs.is_empty(),
        "no tool specs found in {}",
        registry_dir().display()
    );
    specs
}

#[test]
fn no_shipped_tool_resumes_with_a_create_only_flag() {
    let mut violations = Vec::new();
    for (file, spec) in shipped_specs() {
        let Some(session) = spec.performer.as_ref().and_then(|p| p.session.as_ref()) else {
            continue;
        };
        let Some(resume) = session.resume.as_ref() else {
            continue;
        };
        if let Some(flag) = create_only_flag_in(&session.create_only_flags, &resume.args) {
            violations.push(format!(
                "{file}: session.resume.args uses create-only flag '{flag}' \
                 (args: {:?}) — move it to session.create and use the tool's resume flag here",
                resume.args
            ));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn no_shipped_tool_retries_with_a_create_only_flag() {
    // Retry args are merged into the resume invocation on attempt > 1, where
    // the session already exists, so the same rule applies.
    let mut violations = Vec::new();
    for (file, spec) in shipped_specs() {
        let Some(performer) = spec.performer.as_ref() else {
            continue;
        };
        let (Some(session), Some(retry)) = (performer.session.as_ref(), performer.retry.as_ref())
        else {
            continue;
        };
        if let Some(flag) = create_only_flag_in(&session.create_only_flags, &retry.args) {
            violations.push(format!(
                "{file}: performer.retry.args uses create-only flag '{flag}' (args: {:?})",
                retry.args
            ));
        }
    }
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn every_shipped_spec_passes_validation() {
    for (file, spec) in shipped_specs() {
        spec.validate()
            .unwrap_or_else(|e| panic!("{file} failed ToolSpec::validate: {e}"));
    }
}

/// The guardrail is only worth having if it actually fires. Re-inject each
/// spec's own declared create-only flag into its resume args and assert the
/// spec is rejected — this is exactly the shipped bug.
#[test]
fn the_guardrail_rejects_the_flag_that_broke_the_run() {
    let mut checked = 0;
    for (file, spec) in shipped_specs() {
        let Some(session) = spec.performer.as_ref().and_then(|p| p.session.as_ref()) else {
            continue;
        };
        if session.resume.is_none() {
            continue;
        }
        for flag in session.create_only_flags.clone() {
            let mut poisoned = spec.clone();
            let args = &mut poisoned
                .performer
                .as_mut()
                .expect("performer")
                .session
                .as_mut()
                .expect("session")
                .resume
                .as_mut()
                .expect("resume")
                .args;
            args.push(flag.clone());
            args.push("{session_id}".to_string());

            let err = poisoned
                .validate()
                .expect_err(&format!("{file}: '{flag}' in resume.args must be rejected"))
                .to_string();
            assert!(
                err.contains(&flag) && err.contains("session.create"),
                "{file}: rejection must name the offending flag and the fix, got: {err}"
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no spec declares create_only_flags — the guardrail is inert"
    );
}

/// A spec that declares a separate `session.create` command is asserting that
/// create and resume are different invocations. It must then also declare which
/// flags are create-only, or the rule above has nothing to enforce.
#[test]
fn specs_with_a_create_command_declare_their_create_only_flags() {
    for (file, spec) in shipped_specs() {
        let Some(session) = spec.performer.as_ref().and_then(|p| p.session.as_ref()) else {
            continue;
        };
        let (Some(create), Some(resume)) = (session.create.as_ref(), session.resume.as_ref())
        else {
            continue;
        };
        assert!(
            !session.create_only_flags.is_empty(),
            "{file}: declares session.create but no session.create_only_flags, so nothing stops \
             the create flag being put back into resume.args"
        );
        // The declaration must describe reality: every declared flag should be
        // in create.args and absent from resume.args.
        for flag in &session.create_only_flags {
            assert!(
                create.args.iter().any(|a| a == flag),
                "{file}: '{flag}' is declared create-only but is not used in session.create.args"
            );
            assert!(
                !resume.args.iter().any(|a| a == flag),
                "{file}: '{flag}' is declared create-only but appears in session.resume.args"
            );
        }
    }
}

/// `--flag=value` must not slip past a check written for `--flag value`.
#[test]
fn equals_spelling_is_also_caught() {
    let declared = vec!["--session-id".to_string()];
    let args = vec!["--model".into(), "sonnet".into(), "--session-id=abc".into()];
    assert_eq!(create_only_flag_in(&declared, &args), Some("--session-id"));
}

/// The rule is scoped to what a spec declares: a flag one tool calls
/// create-only must not be flagged on a tool that declares nothing.
#[test]
fn undeclared_flags_are_not_flagged() {
    let args = vec!["--session-id".to_string(), "{session_id}".to_string()];
    assert_eq!(create_only_flag_in(&[], &args), None);
    assert_eq!(
        create_only_flag_in(&["--session-id".to_string()], &args),
        Some("--session-id")
    );
}

/// The guardrail must not be so broad that it blocks correct resume spellings.
#[test]
fn genuine_resume_flags_pass() {
    let declared = vec!["--session-id".to_string()];
    for args in [
        vec!["-r", "{session_id}"],
        vec!["--resume", "{session_id}"],
        vec!["exec", "resume", "{session_id}"],
        vec!["--conversation", "{session_id}"],
    ] {
        let args: Vec<String> = args.into_iter().map(str::to_string).collect();
        assert_eq!(
            create_only_flag_in(&declared, &args),
            None,
            "resume args wrongly rejected: {args:?}"
        );
    }
}
