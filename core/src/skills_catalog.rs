/// Skills & Catalog lifecycle layer (spec §3, §5, §7, §8–§13).
///
/// Provides the four-state model: available → selected → installed → locked.
/// All types here are data-only; no network I/O is performed.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

// ── State model (spec §3) ─────────────────────────────────────────────────────

/// A skill as declared in a catalog (available state).
/// Extends the basic `SkillEntry` with lifecycle/install metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogSkill {
    /// Stable identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Short description shown in TUI/Web.
    pub description: String,
    /// Tool adapters that support this skill.
    pub tools: Vec<String>,
    /// Catalog source ID this skill came from.
    pub source: String,
    /// Path inside the source repository.
    pub subpath: String,
    /// Recommended ref (tag/branch/SHA).
    pub recommended_ref: Option<String>,
    /// User-facing tags for filtering.
    pub tags: Vec<String>,
    /// Install risk level: `low`, `medium`, `high`.
    pub risk: Option<String>,
    /// Whether this skill requires an MCP server to work.
    pub requires_mcp: bool,
    /// Whether this skill writes user-level config outside the project.
    pub writes_user_level_config: bool,
    /// Preview of install targets per tool.
    pub targets: BTreeMap<String, Vec<String>>,
    /// Optional category, e.g. `hook-bundle`.
    pub category: Option<String>,
    /// MACC/tool compatibility constraints.
    pub compatibility: Option<serde_json::Value>,
}

/// User's intent: a skill selected for a specific tool in `macc.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillSelection {
    /// Skill identifier.
    pub id: String,
    /// Target tool adapter.
    pub tool: String,
    /// Catalog source (or `None` for direct URL installs).
    pub source: Option<String>,
    /// Requested ref (branch, tag, or SHA).
    pub reference: Option<String>,
    /// Whether the resolved SHA is pinned.
    pub pin: bool,
    /// Optional install alias (overrides destination path component).
    pub alias: Option<String>,
    /// Optional category, e.g. `hook-bundle`.
    pub category: Option<String>,
}

// ── Lockfile types (spec §5.2) ────────────────────────────────────────────────

/// A single installed skill entry in `skills.lock.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillLockEntry {
    pub id: String,
    pub tool: String,
    pub source: LockedSource,
    pub package: LockedPackage,
    pub cache: CacheRef,
    pub installed: InstalledTargets,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedSource {
    pub kind: String,
    pub url: Option<String>,
    pub requested_ref: Option<String>,
    pub resolved_ref: Option<String>,
    pub checksum: Option<String>,
    pub subpath: String,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockedPackage {
    pub manifest_path: Option<String>,
    pub manifest_digest: Option<String>,
    pub id: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheRef {
    pub cache_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledTargets {
    pub at: String,
    pub targets: Vec<InstalledTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledTarget {
    pub src: String,
    pub dest: String,
    pub digest: Option<String>,
    pub owner: String,
}

/// The full skills lockfile: `.macc/skills.lock.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SkillsLockFile {
    pub version: u32,
    pub generated_by: String,
    pub generated_at: String,
    pub skills: Vec<SkillLockEntry>,
}

impl SkillsLockFile {
    pub fn load(path: &Path) -> crate::Result<Self> {
        if !path.exists() {
            return Ok(Self {
                version: 1,
                generated_by: format!("macc {}", env!("CARGO_PKG_VERSION")),
                generated_at: chrono::Utc::now().to_rfc3339(),
                skills: Vec::new(),
            });
        }
        let content = std::fs::read_to_string(path).map_err(|e| crate::MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "read skills lockfile".into(),
            source: e,
        })?;
        serde_json::from_str(&content).map_err(|e| {
            crate::MaccError::Validation(format!(
                "Failed to parse skills lockfile {}: {}",
                path.display(),
                e
            ))
        })
    }

    pub fn save(&self, path: &Path) -> crate::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| crate::MaccError::Io {
                path: parent.to_string_lossy().into(),
                action: "create lockfile parent dir".into(),
                source: e,
            })?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|e| {
            crate::MaccError::Validation(format!("Failed to serialize skills lockfile: {}", e))
        })?;
        std::fs::write(path, content).map_err(|e| crate::MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "write skills lockfile".into(),
            source: e,
        })
    }

    pub fn find(&self, id: &str, tool: &str) -> Option<&SkillLockEntry> {
        self.skills.iter().find(|e| e.id == id && e.tool == tool)
    }

    pub fn upsert(&mut self, entry: SkillLockEntry) {
        if let Some(pos) = self
            .skills
            .iter()
            .position(|e| e.id == entry.id && e.tool == entry.tool)
        {
            self.skills[pos] = entry;
        } else {
            self.skills.push(entry);
        }
        self.generated_at = chrono::Utc::now().to_rfc3339();
    }

    pub fn remove(&mut self, id: &str, tool: &str) -> bool {
        let before = self.skills.len();
        self.skills.retain(|e| !(e.id == id && e.tool == tool));
        self.skills.len() < before
    }
}

// ── Status model (spec §4.2) ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkillStatusKind {
    Clean,
    Modified,
    MissingFiles,
    CacheMissing,
    Unpinned,
    SourceUnreachable,
    Conflict,
    Orphaned,
    UnsupportedTool,
    ManifestInvalid,
    NotInstalled,
}

impl SkillStatusKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Modified => "modified",
            Self::MissingFiles => "missing-files",
            Self::CacheMissing => "cache-missing",
            Self::Unpinned => "unpinned",
            Self::SourceUnreachable => "source-unreachable",
            Self::Conflict => "conflict",
            Self::Orphaned => "orphaned",
            Self::UnsupportedTool => "unsupported-tool",
            Self::ManifestInvalid => "manifest-invalid",
            Self::NotInstalled => "not-installed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStatus {
    pub id: String,
    pub tool: String,
    pub kind: SkillStatusKind,
    pub source_url: Option<String>,
    pub requested_ref: Option<String>,
    pub resolved_ref: Option<String>,
    pub pinned: bool,
    pub warnings: Vec<String>,
    pub installed_files: Vec<String>,
}

// ── Package manifest (spec §7) ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackageManifest {
    #[serde(rename = "type")]
    pub manifest_type: String,
    pub id: String,
    pub version: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub targets: BTreeMap<String, Vec<ManifestTarget>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestTarget {
    pub src: String,
    pub dest: String,
}

impl PackageManifest {
    /// Validate the manifest per spec §7 safety rules.
    pub fn validate(&self) -> Result<(), String> {
        // type allowlist
        if !matches!(self.manifest_type.as_str(), "skill" | "mcp" | "hook-bundle") {
            return Err(format!("Unknown manifest type '{}'", self.manifest_type));
        }
        // id must be non-empty
        if self.id.is_empty() {
            return Err("Manifest id is empty".to_string());
        }
        // all targets: check for path escapes
        for targets in self.targets.values() {
            for t in targets {
                if t.src.contains("..") || t.dest.contains("..") {
                    return Err(format!(
                        "Path escape detected in manifest target: {} → {}",
                        t.src, t.dest
                    ));
                }
                if std::path::Path::new(&t.src).is_absolute()
                    || std::path::Path::new(&t.dest).is_absolute()
                {
                    return Err(format!(
                        "Absolute path rejected in manifest target: {} → {}",
                        t.src, t.dest
                    ));
                }
            }
        }
        Ok(())
    }
}

// ── Conflict detection (spec §11) ─────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictKind {
    SkillVsSkill { existing_owner: String },
    SkillVsUnmanagedFile,
    SkillVsModifiedManaged,
    PathEscape,
    CaseInsensitiveCollision,
    FileVsDirectory,
    UserLevelWrite,
}

#[derive(Debug, Clone)]
pub struct InstallConflict {
    pub dest: String,
    pub kind: ConflictKind,
    pub message: String,
}

/// Detect conflicts before writing (spec §11.2).
pub fn detect_conflicts(
    planned_targets: &[(String, String)], // (src, dest) pairs
    lockfile: &SkillsLockFile,
    project_root: &Path,
) -> Vec<InstallConflict> {
    let mut conflicts = Vec::new();
    let mut dest_map: BTreeMap<String, String> = BTreeMap::new(); // dest → skill_id

    for (_, dest) in planned_targets {
        // Path escape check
        if dest.contains("..") || std::path::Path::new(dest).is_absolute() {
            conflicts.push(InstallConflict {
                dest: dest.clone(),
                kind: ConflictKind::PathEscape,
                message: format!("Destination path '{}' contains escape or is absolute", dest),
            });
            continue;
        }

        // Skill-vs-skill collision in this plan
        if let Some(existing) = dest_map.get(dest.as_str()) {
            conflicts.push(InstallConflict {
                dest: dest.clone(),
                kind: ConflictKind::SkillVsSkill {
                    existing_owner: existing.clone(),
                },
                message: format!(
                    "Destination '{}' already claimed by another skill in this install plan",
                    dest
                ),
            });
            continue;
        }
        dest_map.insert(dest.clone(), "planned".to_string());

        // Lockfile ownership check
        for lock_entry in &lockfile.skills {
            for installed in &lock_entry.installed.targets {
                if normalize_path(&installed.dest) == normalize_path(dest) {
                    conflicts.push(InstallConflict {
                        dest: dest.clone(),
                        kind: ConflictKind::SkillVsSkill {
                            existing_owner: format!("{}@{}", lock_entry.id, lock_entry.tool),
                        },
                        message: format!(
                            "Destination '{}' is already owned by skill '{}' for tool '{}'",
                            dest, lock_entry.id, lock_entry.tool
                        ),
                    });
                }
            }
        }

        // Filesystem: unmanaged file check
        let full_path = project_root.join(dest);
        if full_path.exists() {
            let is_managed = lockfile.skills.iter().any(|e| {
                e.installed
                    .targets
                    .iter()
                    .any(|t| normalize_path(&t.dest) == normalize_path(dest))
            });
            if !is_managed {
                conflicts.push(InstallConflict {
                    dest: dest.clone(),
                    kind: ConflictKind::SkillVsUnmanagedFile,
                    message: format!("Destination '{}' exists and is not MACC-owned", dest),
                });
            }
        }
    }

    conflicts
}

fn normalize_path(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

// ── Cache key generation (spec §8.2) ─────────────────────────────────────────

/// Generate an immutable cache key from source URL and resolved commit SHA.
///
/// Format: `git/<url-hash-6>/<resolved-sha>`
pub fn git_cache_key(url: &str, resolved_sha: &str) -> String {
    let url_hash = hex_hash_6(url);
    format!("git/{}/{}", url_hash, resolved_sha)
}

/// Generate a cache key for HTTP sources (url + checksum).
pub fn http_cache_key(url: &str, checksum: &str) -> String {
    let combined = format!("{}|{}", url, checksum);
    let hash = hex_hash_6(&combined);
    format!("http/{}", hash)
}

fn hex_hash_6(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    result[..3].iter().fold(String::new(), |mut s, b| {
        write!(s, "{:02x}", b).ok();
        s
    })
}

// ── Ownership marker (spec §12) ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipMarker {
    pub owner: String,
    pub skill_id: String,
    pub tool: String,
    pub lockfile_entry: String,
    pub installed_at: String,
}

pub fn write_ownership_marker(dir: &Path, skill_id: &str, tool: &str) -> crate::Result<()> {
    let marker_path = dir.join(".macc-owned.json");
    let marker = OwnershipMarker {
        owner: "macc".to_string(),
        skill_id: skill_id.to_string(),
        tool: tool.to_string(),
        lockfile_entry: format!("{}@{}", skill_id, tool),
        installed_at: chrono::Utc::now().to_rfc3339(),
    };
    let content = serde_json::to_string_pretty(&marker).map_err(|e| {
        crate::MaccError::Validation(format!("Failed to serialize ownership marker: {}", e))
    })?;
    std::fs::write(&marker_path, content).map_err(|e| crate::MaccError::Io {
        path: marker_path.to_string_lossy().into(),
        action: "write ownership marker".into(),
        source: e,
    })
}

// ── File digest (spec §5.2, §13) ─────────────────────────────────────────────

pub fn sha256_digest(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    let hex = result.iter().fold(String::new(), |mut s, b| {
        write!(s, "{:02x}", b).ok();
        s
    });
    format!("sha256:{}", hex)
}

pub fn file_digest(path: &Path) -> crate::Result<String> {
    let content = std::fs::read(path).map_err(|e| crate::MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "read file for digest".into(),
        source: e,
    })?;
    Ok(sha256_digest(&content))
}

// ── Status computation (spec §4.2) ────────────────────────────────────────────

/// Compute the current status of all entries in the lockfile.
pub fn compute_skills_status(
    lockfile: &SkillsLockFile,
    project_root: &Path,
    filter_tool: Option<&str>,
) -> Vec<SkillStatus> {
    let mut statuses = Vec::new();

    for entry in &lockfile.skills {
        if let Some(t) = filter_tool {
            if entry.tool != t {
                continue;
            }
        }

        let mut warnings = Vec::new();
        let mut installed_files = Vec::new();

        // Check if installed files exist and match digests.
        let mut all_present = true;
        let mut any_modified = false;

        for target in &entry.installed.targets {
            let dest_path = project_root.join(&target.dest);
            installed_files.push(target.dest.clone());

            if !dest_path.exists() {
                all_present = false;
                continue;
            }

            if let Some(expected_digest) = &target.digest {
                if let Ok(actual) = file_digest(&dest_path) {
                    if &actual != expected_digest {
                        any_modified = true;
                    }
                }
            }
        }

        // Unpinned warning.
        if !entry.source.pinned
            && (entry.source.requested_ref.as_deref() == Some("main")
                || entry.source.requested_ref.as_deref() == Some("master")
                || entry.source.resolved_ref.is_none())
        {
            warnings.push(format!(
                "{} is installed from mutable ref \"{}\".",
                entry.id,
                entry.source.requested_ref.as_deref().unwrap_or("unknown")
            ));
        }

        let kind = if !all_present {
            SkillStatusKind::MissingFiles
        } else if any_modified {
            warnings.push(format!("{} differs from lockfile digest.", entry.id));
            SkillStatusKind::Modified
        } else if !entry.source.pinned {
            SkillStatusKind::Unpinned
        } else {
            SkillStatusKind::Clean
        };

        statuses.push(SkillStatus {
            id: entry.id.clone(),
            tool: entry.tool.clone(),
            kind,
            source_url: entry.source.url.clone(),
            requested_ref: entry.source.requested_ref.clone(),
            resolved_ref: entry.source.resolved_ref.clone(),
            pinned: entry.source.pinned,
            warnings,
            installed_files,
        });
    }

    statuses
}

// ── Verify (spec §4.5) ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyFinding {
    pub skill_id: String,
    pub tool: String,
    pub kind: String,
    pub message: String,
}

pub fn verify_skills(
    lockfile: &SkillsLockFile,
    project_root: &Path,
    cache_dir: &Path,
) -> Vec<VerifyFinding> {
    let mut findings = Vec::new();

    for entry in &lockfile.skills {
        // Check installed files exist and match digests.
        for target in &entry.installed.targets {
            let dest_path = project_root.join(&target.dest);
            if !dest_path.exists() {
                findings.push(VerifyFinding {
                    skill_id: entry.id.clone(),
                    tool: entry.tool.clone(),
                    kind: "missing-installed-file".to_string(),
                    message: format!("Installed file not found: {}", target.dest),
                });
                continue;
            }
            if let Some(expected) = &target.digest {
                match file_digest(&dest_path) {
                    Ok(actual) if &actual != expected => {
                        findings.push(VerifyFinding {
                            skill_id: entry.id.clone(),
                            tool: entry.tool.clone(),
                            kind: "digest-mismatch".to_string(),
                            message: format!(
                                "File '{}' digest mismatch: expected {}, got {}",
                                target.dest,
                                &expected[..16],
                                &actual[..16]
                            ),
                        });
                    }
                    Err(e) => {
                        findings.push(VerifyFinding {
                            skill_id: entry.id.clone(),
                            tool: entry.tool.clone(),
                            kind: "digest-read-error".to_string(),
                            message: format!("Could not read '{}': {}", target.dest, e),
                        });
                    }
                    _ => {}
                }
            }
        }

        // Check cache entry exists.
        let cache_path = cache_dir.join(&entry.cache.cache_key);
        if !cache_path.exists() {
            findings.push(VerifyFinding {
                skill_id: entry.id.clone(),
                tool: entry.tool.clone(),
                kind: "cache-missing".to_string(),
                message: format!("Cache entry not found: {}", entry.cache.cache_key),
            });
        }

        // Mutable ref warning in strict verification.
        if !entry.source.pinned && entry.source.resolved_ref.is_none() {
            findings.push(VerifyFinding {
                skill_id: entry.id.clone(),
                tool: entry.tool.clone(),
                kind: "unpinned-ref".to_string(),
                message: format!(
                    "Skill '{}' is installed from mutable ref without resolved SHA",
                    entry.id
                ),
            });
        }
    }

    findings
}

// ── Diff computation (spec §4.7) ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SkillDiffEntry {
    pub skill_id: String,
    pub tool: String,
    pub path: String,
    pub diff_lines: Vec<String>,
}

pub fn diff_skill(
    entry: &SkillLockEntry,
    project_root: &Path,
    cache_dir: &Path,
) -> Vec<SkillDiffEntry> {
    let mut diffs = Vec::new();

    for target in &entry.installed.targets {
        let installed_path = project_root.join(&target.dest);
        let cache_src_path = cache_dir.join(&entry.cache.cache_key).join(&target.src);

        if !installed_path.exists() {
            diffs.push(SkillDiffEntry {
                skill_id: entry.id.clone(),
                tool: entry.tool.clone(),
                path: target.dest.clone(),
                diff_lines: vec!["<file missing>".to_string()],
            });
            continue;
        }

        // Check digest drift.
        if let (Some(expected_digest), Ok(actual_digest)) =
            (&target.digest, file_digest(&installed_path))
        {
            if expected_digest != &actual_digest {
                // Read both files and produce line-level diff markers.
                let installed_content =
                    std::fs::read_to_string(&installed_path).unwrap_or_default();
                let cache_content = if cache_src_path.exists() {
                    std::fs::read_to_string(&cache_src_path).unwrap_or_default()
                } else {
                    String::new()
                };

                let mut lines = Vec::new();
                lines.push(format!("--- a/{} (cache)", target.src));
                lines.push(format!("+++ b/{} (installed)", target.dest));

                // Simple line diff.
                let cache_lines: Vec<&str> = cache_content.lines().collect();
                let inst_lines: Vec<&str> = installed_content.lines().collect();
                let max = cache_lines.len().max(inst_lines.len());
                for i in 0..max {
                    match (cache_lines.get(i), inst_lines.get(i)) {
                        (Some(c), Some(a)) if c != a => {
                            lines.push(format!("-{}", c));
                            lines.push(format!("+{}", a));
                        }
                        (None, Some(a)) => lines.push(format!("+{}", a)),
                        (Some(c), None) => lines.push(format!("-{}", c)),
                        _ => {}
                    }
                }

                diffs.push(SkillDiffEntry {
                    skill_id: entry.id.clone(),
                    tool: entry.tool.clone(),
                    path: target.dest.clone(),
                    diff_lines: lines,
                });
            }
        }
    }

    diffs
}

// ── Hook bundle model (spec §14) ─────────────────────────────────────────────

pub const DEFAULT_HOOK_BUNDLES: &[(&str, &str, &str)] = &[
    (
        "test-output-failures-only",
        "Test Output Failures Only",
        "Filters test logs so only failures, summaries, and actionable stack traces enter assistant context.",
    ),
    (
        "lint-errors-only",
        "Lint Errors Only",
        "Keeps lint errors and actionable warnings; collapses successful lint output.",
    ),
    (
        "stacktrace-collapse",
        "Stacktrace Collapse",
        "Collapses repetitive stack frames while preserving top error, cause chain, and project frames.",
    ),
    (
        "git-diff-stat-before-full-diff",
        "Git Diff Stat Before Full Diff",
        "Shows diff stat and changed files before exposing the full diff.",
    ),
    (
        "log-grep-error-first",
        "Log Grep Error First",
        "Surfaces error, warn, fatal, panic, exception, and recent context first.",
    ),
    (
        "build-output-summary",
        "Build Output Summary",
        "Keeps build errors and bundle summary; collapses successful compilation logs.",
    ),
    (
        "package-manager-noise-filter",
        "Package Manager Noise Filter",
        "Collapses dependency install progress, audit banners, and repeated network retry logs.",
    ),
    (
        "coordinator-event-summarizer",
        "Coordinator Event Summarizer",
        "Summarizes MACC coordinator event streams by task, phase, and state transition.",
    ),
    (
        "performer-log-summary",
        "Performer Log Summary",
        "Summarizes long performer logs into action/result/error sections.",
    ),
];

// ── Config additions (spec §5.1) ─────────────────────────────────────────────

/// Project-level skills policy (`settings.skills` in macc.yaml).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SkillsPolicy {
    /// Require commit-SHA or checksum-resolved installs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_pin: Option<bool>,
    /// Allow branch refs (like `main`) without pinning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_mutable_refs: Option<bool>,
    /// `fail` | `prompt` | `replace-managed`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict_policy: Option<String>,
    /// Offline install uses lockfile + cache only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offline_uses_lockfile_only: Option<bool>,
    /// Write `.macc-owned.json` markers in generated package roots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_ownership_markers: Option<bool>,
}

impl SkillsPolicy {
    pub fn effective_require_pin(&self) -> bool {
        self.require_pin.unwrap_or(false)
    }

    pub fn effective_allow_mutable_refs(&self) -> bool {
        self.allow_mutable_refs.unwrap_or(true)
    }

    pub fn effective_conflict_policy(&self) -> &str {
        self.conflict_policy.as_deref().unwrap_or("fail")
    }

    pub fn effective_write_ownership_markers(&self) -> bool {
        self.write_ownership_markers.unwrap_or(true)
    }
}

// ── Error codes (spec §16) ────────────────────────────────────────────────────

pub const MACC_SKILL_1001: &str = "MACC-SKILL-1001"; // Catalog parse error
pub const MACC_SKILL_1002: &str = "MACC-SKILL-1002"; // Skill not found
pub const MACC_SKILL_1003: &str = "MACC-SKILL-1003"; // Unsupported tool target
pub const MACC_SKILL_2001: &str = "MACC-SKILL-2001"; // Source resolution failed
pub const MACC_SKILL_2002: &str = "MACC-SKILL-2002"; // Mutable ref blocked by policy
pub const MACC_SKILL_2003: &str = "MACC-SKILL-2003"; // Checksum mismatch
pub const MACC_SKILL_3001: &str = "MACC-SKILL-3001"; // Destination conflict
pub const MACC_SKILL_3002: &str = "MACC-SKILL-3002"; // Path escape rejected
pub const MACC_SKILL_3003: &str = "MACC-SKILL-3003"; // Unmanaged file overwrite rejected
pub const MACC_SKILL_4001: &str = "MACC-SKILL-4001"; // Manifest invalid
pub const MACC_SKILL_4002: &str = "MACC-SKILL-4002"; // Cache entry missing
pub const MACC_SKILL_4003: &str = "MACC-SKILL-4003"; // Lockfile drift detected

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_cache_key_format() {
        let key = git_cache_key(
            "https://github.com/brand201/macc-skills",
            "9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911",
        );
        assert!(key.starts_with("git/"), "key should start with git/");
        assert!(key.contains("9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911"));
        let parts: Vec<&str> = key.splitn(3, '/').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[1].len(), 6, "URL hash should be 6 hex chars");
    }

    #[test]
    fn test_git_cache_key_deterministic() {
        let k1 = git_cache_key("https://github.com/brand201/macc-skills", "abc123");
        let k2 = git_cache_key("https://github.com/brand201/macc-skills", "abc123");
        assert_eq!(k1, k2);
    }

    #[test]
    fn test_http_cache_key() {
        let k = http_cache_key("https://example.com/skill.tar.gz", "sha256:abc");
        assert!(k.starts_with("http/"));
    }

    #[test]
    fn test_sha256_digest() {
        let d = sha256_digest(b"hello");
        assert!(d.starts_with("sha256:"));
        assert_eq!(d.len(), 7 + 64); // "sha256:" + 64 hex chars
    }

    #[test]
    fn test_manifest_validation_path_escape() {
        let m = PackageManifest {
            manifest_type: "skill".to_string(),
            id: "test".to_string(),
            version: None,
            title: None,
            description: None,
            category: None,
            targets: {
                let mut t = BTreeMap::new();
                t.insert(
                    "claude".to_string(),
                    vec![ManifestTarget {
                        src: "src/foo.md".to_string(),
                        dest: "../../.ssh/config".to_string(),
                    }],
                );
                t
            },
        };
        assert!(m.validate().is_err(), "path escape should fail validation");
    }

    #[test]
    fn test_manifest_validation_clean() {
        let m = PackageManifest {
            manifest_type: "skill".to_string(),
            id: "nextjs-rsc".to_string(),
            version: Some("0.3.1".to_string()),
            title: Some("Next.js RSC".to_string()),
            description: None,
            category: None,
            targets: {
                let mut t = BTreeMap::new();
                t.insert(
                    "claude".to_string(),
                    vec![ManifestTarget {
                        src: "claude/SKILL.md".to_string(),
                        dest: ".claude/skills/nextjs-rsc/SKILL.md".to_string(),
                    }],
                );
                t
            },
        };
        assert!(m.validate().is_ok());
    }

    #[test]
    fn test_lockfile_roundtrip() {
        let lockfile = SkillsLockFile {
            version: 1,
            generated_by: "macc 0.2.0".to_string(),
            generated_at: "2026-06-01T00:00:00Z".to_string(),
            skills: vec![SkillLockEntry {
                id: "nextjs-rsc".to_string(),
                tool: "claude".to_string(),
                source: LockedSource {
                    kind: "git".to_string(),
                    url: Some("https://github.com/brand201/macc-skills".to_string()),
                    requested_ref: Some("main".to_string()),
                    resolved_ref: Some("9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911".to_string()),
                    checksum: None,
                    subpath: "skills/nextjs-rsc".to_string(),
                    pinned: true,
                },
                package: LockedPackage {
                    manifest_path: Some("skills/nextjs-rsc/macc.package.json".to_string()),
                    manifest_digest: None,
                    id: "nextjs-rsc".to_string(),
                    version: Some("0.3.1".to_string()),
                },
                cache: CacheRef {
                    cache_key: "git/2c92a9/9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911".to_string(),
                },
                installed: InstalledTargets {
                    at: "2026-06-01T00:00:00Z".to_string(),
                    targets: vec![InstalledTarget {
                        src: "claude/SKILL.md".to_string(),
                        dest: ".claude/skills/nextjs-rsc/SKILL.md".to_string(),
                        digest: Some("sha256:abc123".to_string()),
                        owner: "macc".to_string(),
                    }],
                },
            }],
        };
        let json = serde_json::to_string_pretty(&lockfile).unwrap();
        let parsed: SkillsLockFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.skills.len(), 1);
        assert_eq!(parsed.skills[0].id, "nextjs-rsc");
        assert_eq!(parsed.skills[0].source.pinned, true);
    }

    #[test]
    fn test_detect_conflicts_path_escape() {
        let lockfile = SkillsLockFile::default();
        let planned = vec![("src/foo.md".to_string(), "../../.ssh/config".to_string())];
        let conflicts = detect_conflicts(&planned, &lockfile, Path::new("/tmp"));
        assert!(!conflicts.is_empty());
        assert!(matches!(conflicts[0].kind, ConflictKind::PathEscape));
    }

    #[test]
    fn test_skills_policy_defaults() {
        let p = SkillsPolicy::default();
        assert!(!p.effective_require_pin());
        assert!(p.effective_allow_mutable_refs());
        assert_eq!(p.effective_conflict_policy(), "fail");
        assert!(p.effective_write_ownership_markers());
    }

    #[test]
    fn test_lockfile_upsert_and_remove() {
        let mut lockfile = SkillsLockFile::default();
        let entry = SkillLockEntry {
            id: "foo".to_string(),
            tool: "claude".to_string(),
            source: LockedSource {
                kind: "git".to_string(),
                url: None,
                requested_ref: None,
                resolved_ref: None,
                checksum: None,
                subpath: "skills/foo".to_string(),
                pinned: false,
            },
            package: LockedPackage {
                manifest_path: None,
                manifest_digest: None,
                id: "foo".to_string(),
                version: None,
            },
            cache: CacheRef {
                cache_key: "git/aabbcc/def".to_string(),
            },
            installed: InstalledTargets {
                at: "2026-06-01T00:00:00Z".to_string(),
                targets: vec![],
            },
        };
        lockfile.upsert(entry);
        assert_eq!(lockfile.skills.len(), 1);
        let removed = lockfile.remove("foo", "claude");
        assert!(removed);
        assert!(lockfile.skills.is_empty());
    }

    #[test]
    fn test_default_hook_bundles_non_empty() {
        assert!(!DEFAULT_HOOK_BUNDLES.is_empty());
        for (id, title, _) in DEFAULT_HOOK_BUNDLES {
            assert!(!id.is_empty());
            assert!(!title.is_empty());
        }
    }
}
