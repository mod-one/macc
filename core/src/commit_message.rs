//! Unified commit message formatting, parsing, and validation for MACC.
//!
//! This module is the **single source of truth** for commit message conventions
//! used by performers, reviewers, coordinators, and merge workers.
//!
//! # Format specification
//!
//! ```text
//! <type>: <task_id> - <title>
//!
//! [macc:task <task_id>]
//! [macc:phase <phase>]
//! ```
//!
//! - **Subject line**: `<type>: <task_id>[ - <title>]`
//! - **Trailer block** (after blank line): zero or more `[macc:<key> <value>]` tags.
//! - The `[macc:task <id>]` trailer is **required** for reconciliation.
//!
//! # Merge commits
//!
//! ```text
//! macc: merge task <task_id>
//!
//! [macc:task <task_id>]
//! [macc:merge true]
//! ```

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Commit type
// ---------------------------------------------------------------------------

/// Conventional commit type prefixes used across MACC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitType {
    Feat,
    Fix,
    Refactor,
    Docs,
    Test,
    Chore,
    /// Internal MACC operations (merge, reconcile, maintenance).
    Macc,
}

impl CommitType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Feat => "feat",
            Self::Fix => "fix",
            Self::Refactor => "refactor",
            Self::Docs => "docs",
            Self::Test => "test",
            Self::Chore => "chore",
            Self::Macc => "macc",
        }
    }

    /// Parse from a string prefix (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "feat" => Some(Self::Feat),
            "fix" => Some(Self::Fix),
            "refactor" => Some(Self::Refactor),
            "docs" => Some(Self::Docs),
            "test" => Some(Self::Test),
            "chore" => Some(Self::Chore),
            "macc" => Some(Self::Macc),
            _ => None,
        }
    }
}

impl fmt::Display for CommitType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// MACC tags (trailers)
// ---------------------------------------------------------------------------

/// Well-known MACC trailer tag keys.
pub const TAG_TASK: &str = "task";
pub const TAG_PHASE: &str = "phase";
pub const TAG_MERGE: &str = "merge";
pub const TAG_TOOL: &str = "tool";
pub const TAG_RUN_ID: &str = "run_id";

/// A single `[macc:<key> <value>]` trailer tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaccTag {
    pub key: String,
    pub value: String,
}

impl MaccTag {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    /// Format as `[macc:<key> <value>]`.
    pub fn format(&self) -> String {
        format!("[macc:{} {}]", self.key, self.value)
    }
}

impl fmt::Display for MaccTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[macc:{} {}]", self.key, self.value)
    }
}

// ---------------------------------------------------------------------------
// CommitMessage
// ---------------------------------------------------------------------------

/// Structured representation of a MACC commit message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitMessage {
    /// Conventional commit type prefix.
    pub commit_type: CommitType,
    /// Task identifier (e.g. `WEB-FRONTEND-006`).
    pub task_id: String,
    /// Optional human-readable title/description.
    pub title: Option<String>,
    /// MACC trailer tags.
    pub tags: Vec<MaccTag>,
}

impl CommitMessage {
    /// Create a new task commit message.
    pub fn task(commit_type: CommitType, task_id: impl Into<String>) -> Self {
        let task_id = task_id.into();
        Self {
            commit_type,
            tags: vec![MaccTag::new(TAG_TASK, &task_id)],
            task_id,
            title: None,
        }
    }

    /// Set the title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Add a phase tag.
    pub fn with_phase(mut self, phase: impl Into<String>) -> Self {
        self.tags.push(MaccTag::new(TAG_PHASE, phase));
        self
    }

    /// Add a tool tag.
    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tags.push(MaccTag::new(TAG_TOOL, tool));
        self
    }

    /// Add a run_id tag.
    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.tags.push(MaccTag::new(TAG_RUN_ID, run_id));
        self
    }

    /// Add an arbitrary tag.
    pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.push(MaccTag::new(key, value));
        self
    }

    /// Format as a full git commit message string.
    pub fn format(&self) -> String {
        let subject = self.format_subject();
        if self.tags.is_empty() {
            return subject;
        }
        let trailer = self
            .tags
            .iter()
            .map(|t| t.format())
            .collect::<Vec<_>>()
            .join("\n");
        format!("{}\n\n{}", subject, trailer)
    }

    /// Format only the subject line.
    pub fn format_subject(&self) -> String {
        match &self.title {
            Some(title) if !title.is_empty() => {
                format!("{}: {} - {}", self.commit_type, self.task_id, title)
            }
            _ => format!("{}: {}", self.commit_type, self.task_id),
        }
    }

    /// Get the value of a specific tag key (first occurrence).
    pub fn tag_value(&self, key: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|t| t.key == key)
            .map(|t| t.value.as_str())
    }

    /// Check whether this is a merge commit.
    pub fn is_merge(&self) -> bool {
        self.tag_value(TAG_MERGE).is_some()
    }
}

impl fmt::Display for CommitMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

// ---------------------------------------------------------------------------
// Factory helpers
// ---------------------------------------------------------------------------

/// Create a standard task commit (performer).
pub fn task_commit(
    commit_type: CommitType,
    task_id: &str,
    title: Option<&str>,
    phase: Option<&str>,
) -> CommitMessage {
    let mut msg = CommitMessage::task(commit_type, task_id);
    if let Some(t) = title {
        msg = msg.with_title(t);
    }
    if let Some(p) = phase {
        msg = msg.with_phase(p);
    }
    msg
}

/// Create a merge commit message.
pub fn merge_commit(task_id: &str) -> CommitMessage {
    CommitMessage {
        commit_type: CommitType::Macc,
        task_id: task_id.to_string(),
        title: Some(format!("merge task {}", task_id)),
        tags: vec![
            MaccTag::new(TAG_TASK, task_id),
            MaccTag::new(TAG_MERGE, "true"),
        ],
    }
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Lazy-compiled regex for subject line: `<type>: <task_id>[ - <title>]`
fn subject_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(?P<type>[a-zA-Z]+):\s+(?P<task_id>[A-Za-z0-9_-]+)(?:\s+-\s+(?P<title>.+))?$")
            .expect("invalid subject regex")
    })
}

/// Lazy-compiled regex for trailer tags: `[macc:<key> <value>]`
fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\[macc:(?P<key>[a-z_]+)\s+(?P<value>[^\]]+)\]")
            .expect("invalid tag regex")
    })
}

/// Result of parsing a commit message.
#[derive(Debug, Clone)]
pub struct ParsedCommit {
    /// Structured message (if the subject line matched the convention).
    pub message: Option<CommitMessage>,
    /// Task ID extracted from tags or subject (best effort, even for non-standard commits).
    pub task_id: Option<String>,
    /// All MACC tags found in the body.
    pub tags: BTreeMap<String, String>,
    /// The raw subject line.
    pub subject: String,
}

/// Parse a raw commit message string into a `ParsedCommit`.
///
/// This is tolerant: it extracts what it can from both new-format and legacy commits.
pub fn parse(raw: &str) -> ParsedCommit {
    let lines: Vec<&str> = raw.lines().collect();
    let subject = lines.first().map(|s| s.trim()).unwrap_or("").to_string();

    // Extract all tags from the entire message
    let tag_re = tag_regex();
    let mut tags = BTreeMap::new();
    let mut tag_vec = Vec::new();
    for cap in tag_re.captures_iter(raw) {
        let key = cap["key"].to_string();
        let value = cap["value"].to_string();
        tags.insert(key.clone(), value.clone());
        tag_vec.push(MaccTag::new(key, value));
    }

    // Try structured subject parse
    let sub_re = subject_regex();
    let message = sub_re.captures(&subject).and_then(|cap| {
        let type_str = &cap["type"];
        let commit_type = CommitType::parse(type_str)?;
        let task_id = cap["task_id"].to_string();
        let title = cap.name("title").map(|m| m.as_str().to_string());

        // Ensure the task tag is present; add from subject if missing.
        let mut final_tags = tag_vec.clone();
        if !final_tags.iter().any(|t| t.key == TAG_TASK) {
            final_tags.insert(0, MaccTag::new(TAG_TASK, &task_id));
        }

        Some(CommitMessage {
            commit_type,
            task_id,
            title,
            tags: final_tags,
        })
    });

    // Best-effort task_id: prefer tag, then structured subject, then legacy heuristic.
    let task_id = tags
        .get(TAG_TASK)
        .cloned()
        .or_else(|| message.as_ref().map(|m| m.task_id.clone()))
        .or_else(|| extract_task_id_legacy(&subject));

    ParsedCommit {
        message,
        task_id,
        tags,
        subject,
    }
}

/// Extract a task ID from a legacy subject line that doesn't follow the full convention.
///
/// Handles patterns like:
/// - `feat: WEB-FRONTEND-006 - some title`
/// - `macc: merge task WEB-BACKEND-001`
/// - `WEB-SETUP-001 something`
fn extract_task_id_legacy(subject: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)\b([A-Z][A-Z0-9]*(?:-[A-Z0-9]+)+)\b").expect("invalid legacy id regex")
    });
    re.captures(subject).map(|c| c[1].to_string())
}

// ---------------------------------------------------------------------------
// Shell integration helpers
// ---------------------------------------------------------------------------

/// Return the commit message as arguments suitable for `git commit -m <subject> -m "" -m <trailers>`.
///
/// This helps shell scripts build multi-line commit messages without heredocs.
pub fn shell_commit_args(msg: &CommitMessage) -> Vec<String> {
    let mut args = vec!["-m".to_string(), msg.format_subject()];
    if !msg.tags.is_empty() {
        args.push("-m".to_string());
        args.push(String::new()); // blank line separator
        let trailer = msg
            .tags
            .iter()
            .map(|t| t.format())
            .collect::<Vec<_>>()
            .join("\n");
        args.push("-m".to_string());
        args.push(trailer);
    }
    args
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_task_commit_with_title() {
        let msg = task_commit(CommitType::Feat, "WEB-FRONTEND-006", Some("Integrate Headless UI"), Some("dev"));
        let formatted = msg.format();
        assert!(formatted.starts_with("feat: WEB-FRONTEND-006 - Integrate Headless UI"));
        assert!(formatted.contains("[macc:task WEB-FRONTEND-006]"));
        assert!(formatted.contains("[macc:phase dev]"));
    }

    #[test]
    fn format_task_commit_without_title() {
        let msg = task_commit(CommitType::Fix, "BUG-042", None, None);
        assert_eq!(msg.format_subject(), "fix: BUG-042");
        assert!(msg.format().contains("[macc:task BUG-042]"));
    }

    #[test]
    fn format_merge_commit() {
        let msg = merge_commit("WEB-BACKEND-001");
        let formatted = msg.format();
        assert!(formatted.starts_with("macc: WEB-BACKEND-001 - merge task WEB-BACKEND-001"));
        assert!(formatted.contains("[macc:task WEB-BACKEND-001]"));
        assert!(formatted.contains("[macc:merge true]"));
    }

    #[test]
    fn parse_new_format_with_tags() {
        let raw = "feat: WEB-FRONTEND-006 - Integrate Headless UI\n\n[macc:task WEB-FRONTEND-006]\n[macc:phase dev]";
        let parsed = parse(raw);
        assert_eq!(parsed.task_id, Some("WEB-FRONTEND-006".into()));
        assert!(parsed.message.is_some());
        let msg = parsed.message.unwrap();
        assert_eq!(msg.commit_type, CommitType::Feat);
        assert_eq!(msg.task_id, "WEB-FRONTEND-006");
        assert_eq!(msg.title, Some("Integrate Headless UI".into()));
        assert_eq!(parsed.tags.get("phase"), Some(&"dev".into()));
    }

    #[test]
    fn parse_legacy_performer_format() {
        let raw = "feat: WEB-FRONTEND-006 - Integrate Headless UI";
        let parsed = parse(raw);
        assert_eq!(parsed.task_id, Some("WEB-FRONTEND-006".into()));
        assert!(parsed.message.is_some());
    }

    #[test]
    fn parse_legacy_merge_format() {
        let raw = "macc: merge task WEB-BACKEND-001";
        let parsed = parse(raw);
        // Subject regex: type=macc, task_id=merge — the subject regex won't match well,
        // but legacy extraction should still find the real task ID.
        assert_eq!(parsed.task_id, Some("WEB-BACKEND-001".into()));
    }

    #[test]
    fn parse_unknown_format_extracts_id() {
        let raw = "some random commit mentioning WEB-SETUP-001";
        let parsed = parse(raw);
        assert_eq!(parsed.task_id, Some("WEB-SETUP-001".into()));
        assert!(parsed.message.is_none());
    }

    #[test]
    fn parse_no_task_id() {
        let raw = "initial commit";
        let parsed = parse(raw);
        assert_eq!(parsed.task_id, None);
        assert!(parsed.message.is_none());
    }

    #[test]
    fn roundtrip_format_then_parse() {
        let original = task_commit(CommitType::Refactor, "CORE-010", Some("Split engine trait"), Some("dev"))
            .with_tool("claude");
        let formatted = original.format();
        let parsed = parse(&formatted);
        assert!(parsed.message.is_some());
        let reparsed = parsed.message.unwrap();
        assert_eq!(reparsed.commit_type, original.commit_type);
        assert_eq!(reparsed.task_id, original.task_id);
        assert_eq!(reparsed.title, original.title);
        assert_eq!(parsed.tags.get("tool"), Some(&"claude".into()));
        assert_eq!(parsed.tags.get("phase"), Some(&"dev".into()));
    }

    #[test]
    fn shell_commit_args_multiline() {
        let msg = task_commit(CommitType::Feat, "T-1", Some("title"), None);
        let args = shell_commit_args(&msg);
        // Should produce: -m "feat: T-1 - title" -m "" -m "[macc:task T-1]"
        assert_eq!(args[0], "-m");
        assert_eq!(args[1], "feat: T-1 - title");
        assert_eq!(args[2], "-m");
        assert_eq!(args[3], ""); // blank separator
        assert_eq!(args[4], "-m");
        assert!(args[5].contains("[macc:task T-1]"));
    }

    #[test]
    fn commit_type_parse_case_insensitive() {
        assert_eq!(CommitType::parse("FEAT"), Some(CommitType::Feat));
        assert_eq!(CommitType::parse("Fix"), Some(CommitType::Fix));
        assert_eq!(CommitType::parse("unknown"), None);
    }

    #[test]
    fn merge_commit_is_merge() {
        let msg = merge_commit("X-1");
        assert!(msg.is_merge());
    }

    #[test]
    fn task_commit_is_not_merge() {
        let msg = task_commit(CommitType::Feat, "X-1", None, None);
        assert!(!msg.is_merge());
    }

    #[test]
    fn tag_value_lookup() {
        let msg = task_commit(CommitType::Feat, "X-1", None, Some("review"))
            .with_tool("gemini");
        assert_eq!(msg.tag_value(TAG_PHASE), Some("review"));
        assert_eq!(msg.tag_value(TAG_TOOL), Some("gemini"));
        assert_eq!(msg.tag_value("nonexistent"), None);
    }

    #[test]
    fn parse_merge_with_tags() {
        let msg = merge_commit("WEB-001");
        let formatted = msg.format();
        let parsed = parse(&formatted);
        assert_eq!(parsed.task_id, Some("WEB-001".into()));
        assert_eq!(parsed.tags.get("merge"), Some(&"true".into()));
    }
}
