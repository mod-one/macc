//! Interactive human approval in the terminal.
//!
//! `macc coordinator approvals` walks every gate waiting for a decision and
//! asks for it; `approve|reject|request-changes` prompt for any value missing
//! from the command line. Both are the default in a terminal; `--no-interactive`
//! (or a non-terminal stdin/stdout) keeps the plain, scriptable behaviour.
//!
//! The prompts only collect input: every decision is recorded through
//! `CoordinatorCommand::GateDecision`, so validation (role, revision, evidence,
//! reason) stays in core and is identical for the CLI, TUI and web.

use macc_core::coordinator::approval::{approval_brief, short_revision, PendingApproval};
use macc_core::coordinator::model::ApprovalDecision;
use macc_core::Result;
use std::io::{BufRead, Write};

/// Flags this module understands; they are removed before core parsing.
pub(crate) const INTERACTIVE_FLAG: &str = "--interactive";
pub(crate) const NO_INTERACTIVE_FLAG: &str = "--no-interactive";

/// Whether the command should prompt, and the arguments without the
/// interactive flags.
pub(crate) fn split_interactive_flags(args: &[String], is_tty: bool) -> (bool, Vec<String>) {
    let mut interactive = is_tty;
    let mut rest = Vec::with_capacity(args.len());
    for arg in args {
        match arg.as_str() {
            NO_INTERACTIVE_FLAG => interactive = false,
            INTERACTIVE_FLAG => interactive = true,
            _ => rest.push(arg.clone()),
        }
    }
    (interactive, rest)
}

pub(crate) fn stdio_is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Values already given on the command line; the prompts ask for the rest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Prefill {
    pub task_id: Option<String>,
    pub decision: Option<ApprovalDecision>,
    pub role: Option<String>,
    pub revision: Option<String>,
    pub evidence: Option<String>,
    pub reason: Option<String>,
    pub actor: Option<String>,
}

impl Prefill {
    /// Parse `[<GATE>] [--task G] [--role R] [--revision S] [--evidence E]
    /// [--reason T] [--as A] [--decision D]`. Unknown flags are an error so a
    /// typo is not silently dropped.
    pub(crate) fn parse(action: &str, args: &[String]) -> Result<Self> {
        let mut out = Prefill {
            decision: match action {
                "approve" => Some(ApprovalDecision::Approved),
                "reject" => Some(ApprovalDecision::Rejected),
                "request-changes" => Some(ApprovalDecision::ChangesRequested),
                _ => None,
            },
            ..Prefill::default()
        };
        let mut index = 0;
        while index < args.len() {
            let name = args[index].as_str();
            if !name.starts_with("--") {
                if out.task_id.is_none() {
                    out.task_id = Some(name.to_string());
                    index += 1;
                    continue;
                }
                return Err(macc_core::MaccError::Validation(format!(
                    "Unexpected argument: {name}"
                )));
            }
            let value = args.get(index + 1).cloned().ok_or_else(|| {
                macc_core::MaccError::Validation(format!("{name} requires a value"))
            })?;
            match name {
                "--task" => out.task_id = Some(value),
                "--role" => out.role = Some(value),
                "--revision" => out.revision = Some(value),
                "--evidence" => out.evidence = Some(value),
                "--reason" => out.reason = Some(value),
                "--as" => out.actor = Some(value),
                "--decision" => {
                    out.decision = Some(
                        value
                            .parse::<ApprovalDecision>()
                            .map_err(macc_core::MaccError::Validation)?,
                    )
                }
                other => {
                    return Err(macc_core::MaccError::Validation(format!(
                        "Unknown argument: {other}"
                    )))
                }
            }
            index += 2;
        }
        Ok(out)
    }

    /// True when the command line already holds everything core requires, so
    /// the decision can be recorded without asking anything.
    pub(crate) fn is_complete_for(&self, gate: Option<&PendingApproval>) -> bool {
        let filled = |v: &Option<String>| v.as_deref().is_some_and(|s| !s.trim().is_empty());
        let Some(decision) = self.decision else {
            return false;
        };
        if self.task_id.is_none() || !filled(&self.role) {
            return false;
        }
        match decision {
            ApprovalDecision::Approved => {
                filled(&self.revision)
                    && (filled(&self.evidence) || !gate.is_some_and(|g| g.evidence_required))
            }
            _ => filled(&self.reason),
        }
    }
}

/// A decision collected from the operator, ready for `GateDecision`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectedDecision {
    pub task_id: String,
    pub decision: ApprovalDecision,
    pub role: String,
    pub revision: Option<String>,
    pub evidence: Option<String>,
    pub reason: Option<String>,
    pub actor: Option<String>,
}

/// Why no decision came out of the prompts for one gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Skipped,
    Quit,
}

pub(crate) struct Prompter<'a> {
    input: &'a mut dyn BufRead,
    output: &'a mut dyn Write,
}

impl<'a> Prompter<'a> {
    pub(crate) fn new(input: &'a mut dyn BufRead, output: &'a mut dyn Write) -> Self {
        Self { input, output }
    }

    fn say(&mut self, line: impl AsRef<str>) {
        let _ = writeln!(self.output, "{}", line.as_ref());
    }

    /// Read one trimmed line; `None` on end of input (treated as quit).
    fn ask(&mut self, prompt: &str) -> Option<String> {
        let _ = write!(self.output, "{prompt}");
        let _ = self.output.flush();
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim().to_string()),
        }
    }

    /// Ask until the answer is non-empty; an empty answer re-asks, end of
    /// input gives `None`.
    fn ask_required(&mut self, prompt: &str) -> Option<String> {
        loop {
            let answer = self.ask(prompt)?;
            if !answer.is_empty() {
                return Some(answer);
            }
            self.say("  A value is required (Ctrl-D to quit).");
        }
    }

    fn confirm(&mut self, prompt: &str) -> Option<bool> {
        let answer = self.ask(prompt)?;
        Some(matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes"))
    }

    /// Print the full explanation of an approval request.
    pub(crate) fn show_brief(&mut self, p: &PendingApproval) {
        self.say("");
        self.say(format!(
            "━━ {}{}  [{}]",
            p.task_id,
            p.title
                .as_deref()
                .map(|t| format!(" — {t}"))
                .unwrap_or_default(),
            p.status
        ));
        for section in approval_brief(p) {
            self.say(format!("{}:", section.title));
            for line in section.lines {
                self.say(format!("  {line}"));
            }
        }
    }

    fn choose_decision(&mut self) -> Option<Option<ApprovalDecision>> {
        loop {
            let answer =
                self.ask("Decision? [a]pprove, [r]eject, request [c]hanges, [s]kip, [q]uit: ")?;
            match answer.to_ascii_lowercase().as_str() {
                "a" | "approve" => return Some(Some(ApprovalDecision::Approved)),
                "r" | "reject" => return Some(Some(ApprovalDecision::Rejected)),
                "c" | "changes" | "request-changes" => {
                    return Some(Some(ApprovalDecision::ChangesRequested))
                }
                "s" | "skip" | "" => return Some(None),
                "q" | "quit" => return None,
                other => self.say(format!("  Unknown choice '{other}'.")),
            }
        }
    }

    /// Roles still missing come first; the first one is the default.
    fn choose_role(&mut self, p: &PendingApproval) -> Option<String> {
        let mut roles: Vec<String> = p.missing_roles.clone();
        for role in &p.required_roles {
            if !roles.contains(role) {
                roles.push(role.clone());
            }
        }
        if roles.is_empty() {
            return self.ask_required("Your role: ");
        }
        if roles.len() == 1 {
            self.say(format!("Role: {} (the only declared role)", roles[0]));
            return Some(roles[0].clone());
        }
        self.say("You decide as which role?");
        for (index, role) in roles.iter().enumerate() {
            let state = if p.missing_roles.contains(role) {
                "still needed"
            } else {
                "already approved"
            };
            self.say(format!("  {}) {role}  ({state})", index + 1));
        }
        loop {
            let answer = self.ask(&format!("Role [1-{}, default 1]: ", roles.len()))?;
            if answer.is_empty() {
                return Some(roles[0].clone());
            }
            if let Ok(n) = answer.parse::<usize>() {
                if (1..=roles.len()).contains(&n) {
                    return Some(roles[n - 1].clone());
                }
            }
            if let Some(role) = roles.iter().find(|r| r.eq_ignore_ascii_case(&answer)) {
                return Some(role.clone());
            }
            self.say(format!("  '{answer}' is not a declared role."));
        }
    }

    /// Collect one decision for `p`, using `prefill` where it has values.
    /// `None` means the operator skipped or quit (see the returned outcome).
    pub(crate) fn collect(
        &mut self,
        p: &PendingApproval,
        prefill: &Prefill,
        actor_label: &str,
    ) -> std::result::Result<CollectedDecision, Outcome> {
        let decision = match prefill.decision {
            Some(d) => d,
            None => match self.choose_decision() {
                None => return Err(Outcome::Quit),
                Some(None) => return Err(Outcome::Skipped),
                Some(Some(d)) => d,
            },
        };
        let role = match prefill.role.clone().filter(|r| !r.trim().is_empty()) {
            Some(role) => role,
            None => self.choose_role(p).ok_or(Outcome::Quit)?,
        };
        let revision = prefill
            .revision
            .clone()
            .filter(|r| !r.trim().is_empty())
            .or_else(|| p.subject_revision.clone());
        let mut evidence = prefill.evidence.clone().filter(|e| !e.trim().is_empty());
        let mut reason = prefill.reason.clone().filter(|r| !r.trim().is_empty());
        match decision {
            ApprovalDecision::Approved => {
                if revision.is_none() {
                    self.say(format!(
                        "The revision of {} is unknown: name the commit you reviewed.",
                        p.subject_task
                    ));
                }
                if evidence.is_none() {
                    let kind = p.evidence_type.as_deref().unwrap_or("evidence");
                    evidence = if p.evidence_required {
                        Some(
                            self.ask_required(&format!("Evidence ({kind}: link or path): "))
                                .ok_or(Outcome::Quit)?,
                        )
                    } else {
                        Some(
                            self.ask("Evidence (optional, Enter to skip): ")
                                .ok_or(Outcome::Quit)?,
                        )
                        .filter(|e| !e.is_empty())
                    };
                }
            }
            _ => {
                if reason.is_none() {
                    reason = Some(
                        self.ask_required("Reason (what must change, and why): ")
                            .ok_or(Outcome::Quit)?,
                    );
                }
            }
        }
        let revision = match revision {
            Some(r) => Some(r),
            None if decision == ApprovalDecision::Approved => Some(
                self.ask_required("Revision you reviewed (commit SHA): ")
                    .ok_or(Outcome::Quit)?,
            ),
            None => None,
        };
        let actor = prefill.actor.clone();
        self.say("");
        self.say(format!(
            "About to record: {} on {} as {} by {}",
            decision.as_str(),
            p.task_id,
            role,
            actor.as_deref().unwrap_or(actor_label)
        ));
        if let Some(rev) = revision.as_deref() {
            self.say(format!("  Revision: {}", short_revision(rev)));
        }
        if let Some(e) = evidence.as_deref() {
            self.say(format!("  Evidence: {e}"));
        }
        if let Some(r) = reason.as_deref() {
            self.say(format!("  Reason:   {r}"));
        }
        match self.confirm("Record this decision? [y/N]: ") {
            None => return Err(Outcome::Quit),
            Some(false) => {
                self.say("Not recorded.");
                return Err(Outcome::Skipped);
            }
            Some(true) => {}
        }
        Ok(CollectedDecision {
            task_id: p.task_id.clone(),
            decision,
            role,
            revision,
            evidence,
            reason,
            actor,
        })
    }

    /// Walk every gate awaiting a decision. `load` returns the current
    /// pending list; `record` records one decision and returns the summary
    /// line. A gate is shown once per walk unless the operator chooses to
    /// decide for another role right after recording.
    pub(crate) fn walk(
        &mut self,
        load: &mut dyn FnMut() -> Result<Vec<PendingApproval>>,
        record: &mut dyn FnMut(CollectedDecision) -> Result<String>,
        actor_label: &str,
        actor: Option<&str>,
    ) -> Result<usize> {
        let mut seen: Vec<(String, Option<String>)> = Vec::new();
        let mut recorded = 0usize;
        let first = load()?;
        if !first.iter().any(|p| p.awaiting_decision) {
            self.say("No approval gate is waiting for a decision.");
            for p in first.iter() {
                self.say(format!(
                    "  {} [{}]: {}",
                    p.task_id,
                    p.status,
                    p.note.as_deref().unwrap_or("")
                ));
            }
            return Ok(0);
        }
        let mut pending = first;
        while let Some(p) = pending
            .iter()
            .find(|p| {
                p.awaiting_decision
                    && !seen.contains(&(p.task_id.clone(), p.subject_revision.clone()))
            })
            .cloned()
        {
            seen.push((p.task_id.clone(), p.subject_revision.clone()));
            let total = pending.iter().filter(|p| p.awaiting_decision).count();
            self.say(format!("\nApproval request ({total} waiting)"));
            self.show_brief(&p);
            let mut current = p;
            loop {
                let prefill = Prefill {
                    actor: actor.map(str::to_string),
                    ..Prefill::default()
                };
                match self.collect(&current, &prefill, actor_label) {
                    Err(Outcome::Quit) => return Ok(recorded),
                    Err(_) => break,
                    Ok(decision) => match record(decision) {
                        Ok(line) => {
                            recorded += 1;
                            self.say(line);
                        }
                        Err(err) => {
                            self.say(format!("Not recorded: {err}"));
                            continue;
                        }
                    },
                }
                pending = load()?;
                match pending
                    .iter()
                    .find(|q| q.task_id == current.task_id && q.awaiting_decision)
                {
                    Some(next) if !next.missing_roles.is_empty() => {
                        let again = self.confirm(&format!(
                            "{} still needs {}. Decide for another role now? [y/N]: ",
                            next.task_id,
                            next.missing_roles.join(", ")
                        ));
                        match again {
                            None => return Ok(recorded),
                            Some(true) => current = next.clone(),
                            Some(false) => break,
                        }
                    }
                    _ => break,
                }
            }
        }
        self.say(format!(
            "\nDone: {recorded} decision(s) recorded. A running coordinator applies them on its next cycle."
        ));
        Ok(recorded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn gate(missing: &[&str]) -> PendingApproval {
        PendingApproval {
            task_id: "FIN-APP-001".into(),
            title: Some("Approve the FIN-CR-001 normative contract".into()),
            status: "waiting_approval".into(),
            subject_task: "FIN-CR-001".into(),
            subject_revision: Some("abcdef1234567890".into()),
            question: "Do you approve FIN-CR-001 as delivered at revision abcdef123456?".into(),
            awaiting_decision: true,
            required_roles: vec!["FINANCE".into(), "LEAD_TECHNIQUE".into()],
            missing_roles: missing.iter().map(|r| r.to_string()).collect(),
            evidence_type: Some("pull_request_review".into()),
            evidence_required: true,
            quorum: "every listed role must approve".into(),
            required: "FINANCE 0/1, LEAD_TECHNIQUE 0/1".into(),
            ..PendingApproval::default()
        }
    }

    fn run_collect(
        input: &str,
        p: &PendingApproval,
        prefill: &Prefill,
    ) -> (std::result::Result<CollectedDecision, Outcome>, String) {
        let mut reader = Cursor::new(input.as_bytes().to_vec());
        let mut out = Vec::new();
        let result = Prompter::new(&mut reader, &mut out).collect(p, prefill, "Ann <ann@x>");
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn interactive_flags_are_stripped_and_override_the_terminal_default() {
        let args = vec!["--no-interactive".to_string(), "FIN-APP-001".to_string()];
        assert_eq!(
            split_interactive_flags(&args, true),
            (false, vec!["FIN-APP-001".to_string()])
        );
        assert!(split_interactive_flags(&["--interactive".into()], false).0);
        assert!(!split_interactive_flags(&[], false).0);
    }

    #[test]
    fn approve_prompts_for_role_and_evidence_and_binds_the_current_revision() {
        let (result, out) = run_collect(
            "a\n2\nhttps://git/pr/7\ny\n",
            &gate(&["FINANCE", "LEAD_TECHNIQUE"]),
            &Prefill::default(),
        );
        let d = result.unwrap();
        assert_eq!(d.decision, ApprovalDecision::Approved);
        assert_eq!(d.role, "LEAD_TECHNIQUE");
        assert_eq!(d.revision.as_deref(), Some("abcdef1234567890"));
        assert_eq!(d.evidence.as_deref(), Some("https://git/pr/7"));
        assert!(out.contains("1) FINANCE  (still needed)"));
        assert!(out
            .contains("About to record: approved on FIN-APP-001 as LEAD_TECHNIQUE by Ann <ann@x>"));
    }

    #[test]
    fn evidence_is_asked_again_when_left_empty() {
        let (result, out) = run_collect(
            "a\n\n\nhttps://git/pr/7\ny\n",
            &gate(&["FINANCE", "LEAD_TECHNIQUE"]),
            &Prefill::default(),
        );
        assert_eq!(
            result.unwrap().evidence.as_deref(),
            Some("https://git/pr/7")
        );
        assert!(out.contains("A value is required"));
    }

    #[test]
    fn reject_requires_a_reason_and_declining_the_confirmation_records_nothing() {
        let (result, _) = run_collect(
            "r\n1\ncontract misses ETag\ny\n",
            &gate(&["FINANCE"]),
            &Prefill::default(),
        );
        let d = result.unwrap();
        assert_eq!(d.decision, ApprovalDecision::Rejected);
        assert_eq!(d.reason.as_deref(), Some("contract misses ETag"));

        let (result, out) = run_collect(
            "c\n1\nadd ETag\nn\n",
            &gate(&["FINANCE"]),
            &Prefill::default(),
        );
        assert_eq!(result.unwrap_err(), Outcome::Skipped);
        assert!(out.contains("Not recorded."));
    }

    #[test]
    fn prefilled_values_are_not_asked_again() {
        let prefill = Prefill::parse(
            "approve",
            &["FIN-APP-001".into(), "--role".into(), "FINANCE".into()],
        )
        .unwrap();
        assert!(!prefill.is_complete_for(Some(&gate(&["FINANCE"]))));
        let (result, out) = run_collect("https://git/pr/7\ny\n", &gate(&["FINANCE"]), &prefill);
        let d = result.unwrap();
        assert_eq!(d.role, "FINANCE");
        assert!(!out.contains("Decision?"));
        assert!(!out.contains("You decide as which role?"));
    }

    #[test]
    fn a_complete_command_line_needs_no_prompt() {
        let prefill = Prefill::parse(
            "approve",
            &[
                "FIN-APP-001".into(),
                "--role".into(),
                "FINANCE".into(),
                "--revision".into(),
                "abcdef1".into(),
                "--evidence".into(),
                "https://git/pr/7".into(),
            ],
        )
        .unwrap();
        assert!(prefill.is_complete_for(Some(&gate(&["FINANCE"]))));
        let reject = Prefill::parse(
            "reject",
            &["FIN-APP-001".into(), "--role".into(), "FINANCE".into()],
        )
        .unwrap();
        assert!(!reject.is_complete_for(None));
        assert!(Prefill::parse("approve", &["--bogus".into(), "x".into()]).is_err());
    }

    #[test]
    fn walk_shows_the_brief_records_and_offers_the_remaining_role() {
        // Popped from the end: both roles missing, then one, then none left.
        let mut loads = vec![
            vec![],
            vec![gate(&["LEAD_TECHNIQUE"])],
            vec![gate(&["FINANCE", "LEAD_TECHNIQUE"])],
        ];
        let mut recorded = Vec::new();
        let mut reader =
            Cursor::new(b"a\n1\nhttps://git/pr/7\ny\ny\na\n1\nhttps://git/pr/8\ny\n".to_vec());
        let mut out = Vec::new();
        let count = Prompter::new(&mut reader, &mut out)
            .walk(
                &mut || Ok(loads.pop().unwrap_or_default()),
                &mut |d| {
                    recorded.push(d.role.clone());
                    Ok(format!(
                        "Recorded approved for {} as {}.",
                        d.task_id, d.role
                    ))
                },
                "Ann <ann@x>",
                None,
            )
            .unwrap();
        let out = String::from_utf8(out).unwrap();
        assert_eq!(count, 2);
        assert_eq!(recorded, vec!["FINANCE", "LEAD_TECHNIQUE"]);
        assert!(out.contains("Question:"));
        assert!(out.contains("Decide for another role now?"));
        assert!(out.contains("Done: 2 decision(s) recorded."));
    }

    #[test]
    fn walk_reports_when_nothing_awaits_a_decision() {
        let mut rejected = gate(&["FINANCE"]);
        rejected.awaiting_decision = false;
        rejected.status = "rejected".into();
        let mut reader = Cursor::new(Vec::new());
        let mut out = Vec::new();
        let count = Prompter::new(&mut reader, &mut out)
            .walk(
                &mut || Ok(vec![rejected.clone()]),
                &mut |_| unreachable!(),
                "a",
                None,
            )
            .unwrap();
        assert_eq!(count, 0);
        assert!(String::from_utf8(out)
            .unwrap()
            .contains("No approval gate is waiting"));
    }
}
