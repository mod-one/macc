//! Cached, non-executing source checks shared by all clients.
mod github;
#[cfg(test)]
mod tests;
use super::{SkillEntry, SourceKind};
use crate::{ProjectPaths, Result};
use serde::{Deserialize, Serialize};
const CONFIRMED_TTL_SECONDS: i64 = 6 * 60 * 60;
const RETRY_SECONDS: i64 = 5 * 60;

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Available,
    Unavailable,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Availability {
    pub state: State,
    pub reason: String,
    pub checked_at: i64,
    #[serde(default)]
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}
impl Availability {
    pub fn disabled(&self) -> bool {
        self.state == State::Unavailable
    }
    pub fn new(state: State, reason: impl Into<String>, now: i64) -> Self {
        Self {
            state,
            reason: reason.into(),
            checked_at: now,
            stale: false,
            last_error: None,
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Cache {
    entries: BTreeMap<String, Record>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    availability: Availability,
    attempted_at: i64,
}
pub trait Checker: Sync {
    fn check(&self, root: &std::path::Path, entry: &SkillEntry, now: i64) -> Availability;
}
fn key(entry: &SkillEntry) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(&entry.id, &entry.source, &entry.selector, &entry.category))
                .unwrap_or_default()
        )
    )
}
fn path(paths: &ProjectPaths) -> PathBuf {
    paths.macc_dir.join("state/catalog-availability.json")
}
fn load(paths: &ProjectPaths) -> Cache {
    std::fs::read(path(paths))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}
fn due(record: &Record, now: i64) -> bool {
    now.saturating_sub(record.attempted_at)
        >= if record.availability.last_error.is_some()
            || record.availability.state == State::Unknown
        {
            RETRY_SECONDS
        } else {
            CONFIRMED_TTL_SECONDS
        }
}
pub fn annotate(paths: &ProjectPaths, entries: &mut [SkillEntry]) {
    let cache = load(paths);
    let now = chrono::Utc::now().timestamp();
    for entry in entries {
        entry.availability = cache.entries.get(&key(entry)).map(|r| {
            let mut a = r.availability.clone();
            a.stale =
                a.last_error.is_some() || now.saturating_sub(a.checked_at) >= CONFIRMED_TTL_SECONDS;
            a
        });
        if entry.source.kind == SourceKind::Local {
            entry.availability = Some(github::Local.check(&paths.root, entry, now));
        }
    }
}
pub fn ensure_selectable(entry: &SkillEntry) -> Result<()> {
    if let Some(a) = entry.availability.as_ref().filter(|a| a.disabled()) {
        return Err(crate::MaccError::Catalog {
            operation: "availability".into(),
            message: format!(
                "Skill '{}' is unavailable: {}. Correct its source and run `macc skills available --refresh`; installed files and saved selections are retained.",
                entry.id, a.reason
            ),
        });
    }
    Ok(())
}
fn offline(paths: &ProjectPaths) -> bool {
    crate::load_canonical_config(&paths.config_path)
        .map(|c| c.settings.offline)
        .unwrap_or(true)
}
pub fn refresh(paths: &ProjectPaths, force: bool) -> Result<()> {
    if offline(paths) {
        return Ok(());
    }
    let entries = super::load_skills_catalog_with_local(paths)?.entries;
    refresh_with(
        paths,
        &entries,
        &github::Github::<github::Curl>::default(),
        force,
        chrono::Utc::now().timestamp(),
    )
}
/// Verify the effective install reference, including an explicit CLI override.
pub fn check_install(paths: &ProjectPaths, entry: &mut SkillEntry) -> Result<()> {
    if !offline(paths) {
        refresh_with(
            paths,
            std::slice::from_ref(entry),
            &github::Github::<github::Curl>::default(),
            false,
            chrono::Utc::now().timestamp(),
        )?;
    }
    annotate(paths, std::slice::from_mut(entry));
    ensure_selectable(entry)
}
pub fn refresh_with(
    paths: &ProjectPaths,
    entries: &[SkillEntry],
    checker: &dyn Checker,
    force: bool,
    now: i64,
) -> Result<()> {
    let mut cache = load(paths);
    let pending: Vec<_> = entries
        .iter()
        .filter(|entry| force || cache.entries.get(&key(entry)).is_none_or(|r| due(r, now)))
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let pending = &pending;
            let next = &next;
            let results = &results;
            scope.spawn(move || loop {
                let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(entry) = pending.get(index) else {
                    break;
                };
                let a = if std::time::Instant::now() < deadline {
                    checker.check(&paths.root, entry, now)
                } else {
                    Availability::new(
                        State::Unknown,
                        "Check deferred: catalog time budget reached",
                        now,
                    )
                };
                results.lock().unwrap().push((key(entry), a));
            });
        }
    });
    // Merge under a short lock; never hold it while making network requests.
    let _lock = crate::fs_lock::AdvisoryLock::acquire(
        &paths.macc_dir.join("state/catalog-availability.lock"),
        std::time::Duration::from_secs(2),
        "catalog availability cache",
    )?;
    cache = load(paths);
    for (key, mut a) in results.into_inner().unwrap() {
        if cache
            .entries
            .get(&key)
            .is_some_and(|record| record.attempted_at > now)
        {
            continue;
        }
        if a.state == State::Unknown {
            if let Some(old) = cache
                .entries
                .get(&key)
                .filter(|r| r.availability.state != State::Unknown)
            {
                let reason = a.reason;
                a = old.availability.clone();
                a.last_error = Some(reason);
                a.stale = true;
            }
        }
        cache.entries.insert(
            key,
            Record {
                availability: a,
                attempted_at: now,
            },
        );
    }
    crate::atomic_write(
        paths,
        &path(paths),
        &serde_json::to_vec_pretty(&cache)
            .map_err(|e| crate::MaccError::Validation(e.to_string()))?,
    )
}
static RUNNING: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
pub fn checking(paths: &ProjectPaths) -> bool {
    RUNNING
        .get()
        .is_some_and(|running| running.lock().unwrap().contains(&paths.root))
}
pub fn schedule(paths: &ProjectPaths) {
    schedule_refresh(paths, false);
}
pub fn schedule_refresh(paths: &ProjectPaths, force: bool) {
    if offline(paths) {
        return;
    }
    let cache = load(paths);
    let now = chrono::Utc::now().timestamp();
    let Ok(catalog) = super::load_skills_catalog_with_local(paths) else {
        return;
    };
    if !force
        && catalog.entries.iter().all(|entry| {
            cache
                .entries
                .get(&key(entry))
                .is_some_and(|record| !due(record, now))
        })
    {
        return;
    }
    let running = RUNNING.get_or_init(Default::default);
    if !running.lock().unwrap().insert(paths.root.clone()) {
        return;
    }
    let paths = paths.clone();
    std::thread::spawn(move || {
        if let Err(error) = refresh(&paths, force) {
            tracing::warn!(%error,"Catalog availability check failed");
        }
        RUNNING.get().unwrap().lock().unwrap().remove(&paths.root);
    });
}
