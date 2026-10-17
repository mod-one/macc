//! Verification feedback loop: a rejection is handed back to the repair agent.
use super::*;

fn settled(root: &Path, status: &str) -> Value {
    wait_for(|| report(root).is_some_and(|v| v["intervention"]["status"] == status));
    report(root).unwrap()
}
fn calls(root: &Path, phase: &str) -> usize {
    std::fs::read_to_string(root.join(".macc/calls.log"))
        .unwrap_or_default()
        .lines()
        .filter(|line| *line == phase)
        .count()
}

#[test]
fn rejected_verification_is_fed_back_and_the_next_round_is_integrated() {
    let temp = fixture(false);
    let root = temp.path();
    std::fs::write(root.join(".macc/fake-agent.sh"),r#"#!/bin/sh
set -eu
log="$(dirname "$0")/calls.log"
case "$1" in
 *"Diagnose first"*) printf '%s' '{"summary":"defect.txt is broken; repair it","repairable":true,"task_ids":["ROOT"]}' > .macc/supervisor-diagnosis.json ;;
 *"Independently verify"*) printf 'verification\n' >> "$log"
   if test "$(cat defect.txt)" = fixed; then
     printf '%s' '{"verified":true,"explanation":"defect corrected"}' > .macc/supervisor-verification.json
   else
     printf '%s' '{"verified":false,"explanation":"defect.txt is only partial; write fixed"}' > .macc/supervisor-verification.json
   fi ;;
 *"authorized MACC supervisor repair agent"*) printf 'repair\n' >> "$log"
   case "$1" in
    *"defect.txt is only partial; write fixed"*) test "$(cat defect.txt)" = partial; printf 'fixed\n' > defect.txt ;;
    *) printf 'partial\n' > defect.txt ;;
   esac ;;
 *) exit 9 ;;
esac
"#).unwrap();
    let supervisor = start(root);
    let r = settled(root, "recovered");
    assert_eq!(r["intervention"]["attempt"], 1);
    assert_eq!(r["intervention"]["repair_rounds"], 2);
    assert_eq!(calls(root, "repair"), 2);
    assert_eq!(calls(root, "verification"), 2);
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(
        std::fs::read_to_string(dir.join("repair-round-2-prompt.txt"))
            .unwrap()
            .contains("defect.txt is only partial; write fixed")
    );
    assert!(dir.join("verification-round-2-prompt.txt").exists());
    // The rejected round's logs are kept apart from the accepted round's.
    assert!(dir.join("round-1/validation-0.log").exists());
    assert!(dir.join("round-1/baseline-0.log").exists());
    assert!(dir.join("validation-0.log").exists());
    assert!(!dir.join("baseline-0.log").exists());
    drop(supervisor);
}

#[test]
fn verification_needing_a_human_decision_escalates_without_another_round() {
    let temp = fixture(false);
    let root = temp.path();
    std::fs::write(root.join(".macc/fake-agent.sh"),r#"#!/bin/sh
set -eu
log="$(dirname "$0")/calls.log"
case "$1" in
 *"Diagnose first"*) printf '%s' '{"summary":"defect.txt is broken; repair it","repairable":true,"task_ids":["ROOT"]}' > .macc/supervisor-diagnosis.json ;;
 *"Independently verify"*) printf '%s' '{"verified":false,"requires_human":true,"explanation":"the repair reverses a deliberate access rule"}' > .macc/supervisor-verification.json ;;
 *"authorized MACC supervisor repair agent"*) printf 'repair\n' >> "$log"; printf 'fixed\n' > defect.txt ;;
 *) exit 9 ;;
esac
"#).unwrap();
    let supervisor = start(root);
    let r = settled(root, "escalated");
    wait_for(|| report(root).is_some_and(|v| v["intervention"]["next_action"].is_string()));
    let r2 = report(root).unwrap();
    assert_eq!(calls(root, "repair"), 1);
    assert_eq!(r["intervention"]["repair_rounds"], 1);
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("Repair needs a human decision: the repair reverses a deliberate access rule"));
    assert!(r2["intervention"]["next_action"]
        .as_str()
        .unwrap()
        .starts_with("Make the human decision named in the reason"));
    assert!(r["intervention"]["restart_pid"].is_null());
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
    drop(supervisor);
}

#[test]
fn a_single_round_budget_keeps_the_former_behaviour() {
    let temp = fixture(true);
    let root = temp.path();
    let config = root.join(".macc/macc.yaml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        text.replace(
            "    max_restart_attempts: 1\n",
            "    max_restart_attempts: 1\n    max_repair_rounds: 1\n",
        ),
    )
    .unwrap();
    let supervisor = start(root);
    let r = settled(root, "escalated");
    assert_eq!(r["intervention"]["repair_rounds"], 1);
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(!dir.join("repair-round-2-prompt.txt").exists());
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("Repair verification rejected after 1 repair round(s)"));
    drop(supervisor);
}
