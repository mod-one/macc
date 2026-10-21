//! One reviewed installation/update path for both CLI spellings.
use super::{lifecycle_support::CliLifecycleUi, AppContext};
use macc_core::{
    config::{self, CanonicalConfig},
    fs_lock::AdvisoryLock,
    resolve::{FetchUnit, Selection, SelectionKind},
    service::{interaction::InteractionHandler, lifecycle::LifecycleUi},
    skills_catalog::{locking, SkillsLockFile},
    MaccError, Result,
};
use std::time::Duration;
#[derive(Clone)]
pub(super) struct Request {
    pub id: String,
    pub tool: String,
    pub reference: Option<String>,
    pub source: Option<String>,
}

pub(super) fn install(app: &AppContext, request: Request, dry_run: bool) -> Result<()> {
    execute(app, vec![request], dry_run, false)
}
pub(super) fn update(
    app: &AppContext,
    id: Option<&str>,
    tool: Option<&str>,
    dry_run: bool,
    latest: bool,
) -> Result<()> {
    let paths = app.project_paths()?;
    let lock = SkillsLockFile::load(&paths.skills_lock_path())?;
    let catalog = macc_core::catalog::load_skills_catalog_with_local(&paths)?;
    let requests = lock
        .skills
        .iter()
        .filter(|e| id.is_none_or(|id| e.id == id) && tool.is_none_or(|tool| e.tool == tool))
        .map(|entry| {
            let reference = entry
                .source
                .requested_ref
                .clone()
                .filter(|reference| !latest || !locking::is_sha(reference))
                .or_else(|| {
                    catalog
                        .entries
                        .iter()
                        .find(|e| e.id == entry.id)
                        .map(|e| e.source.reference.clone())
                });
            Request {
                id: entry.id.clone(),
                tool: entry.tool.clone(),
                reference,
                source: entry.source.url.clone(),
            }
        })
        .collect::<Vec<_>>();
    if requests.is_empty() {
        println!("No installed skills match the filter.");
        return Ok(());
    }
    execute(app, requests, dry_run, true)
}
fn select(config: &mut CanonicalConfig, requests: &[Request]) -> Result<()> {
    for request in requests {
        let object = config
            .tools
            .config
            .entry(request.tool.clone())
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| MaccError::Validation("Tool configuration must be an object".into()))?;
        let mut skills = object
            .get("skills")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if !skills.iter().any(|v| v.as_str() == Some(&request.id)) {
            skills.push(serde_json::Value::String(request.id.clone()));
        }
        object.insert("skills".into(), serde_json::Value::Array(skills));
    }
    Ok(())
}
fn execute(app: &AppContext, requests: Vec<Request>, dry_run: bool, refresh: bool) -> Result<()> {
    let paths = app.project_paths()?;
    if !dry_run {
        super::gate_cli_mutation(&paths.root)?;
    }
    let _command = AdvisoryLock::acquire(
        &paths.macc_dir.join("state/skills-command.lock"),
        Duration::from_secs(30),
        "skill lifecycle command",
    )?;
    let original = app.engine.load_canonical_config(&paths)?;
    let original_revision = config::transaction::revision(&original)?;
    let offline = app.overrides.offline.unwrap_or(original.settings.offline);
    let lock = SkillsLockFile::load(&paths.skills_lock_path())?;
    let catalog = macc_core::catalog::load_skills_catalog_with_local(&paths)?;
    let (descriptors, _) = app.engine.list_tools(&paths);
    let mut units = Vec::new();
    for request in &requests {
        if !descriptors.iter().any(|d| d.id == request.tool) {
            return Err(MaccError::Validation(format!(
                "Unknown tool '{}'",
                request.tool
            )));
        }
        let entry = catalog
            .entries
            .iter()
            .find(|e| e.id == request.id)
            .ok_or_else(|| {
                MaccError::Validation(format!("Skill '{}' is not in the catalog", request.id))
            })?;
        if !entry.tools.is_empty() && !entry.tools.contains(&request.tool) {
            return Err(MaccError::Validation(format!(
                "Skill '{}' does not support {}",
                request.id, request.tool
            )));
        }
        let mut source = locking::source(entry, &lock, &request.tool)?;
        if let Some(reference) = &request.reference {
            source.reference = reference.clone();
        }
        if let Some(url) = &request.source {
            if url != &source.url && request.reference.is_none() {
                source.reference = entry.source.reference.clone();
            }
            source.url = url.clone();
        }
        let subpath = if !refresh && request.reference.is_none() && request.source.is_none() {
            locking::record(&lock, &request.id, &request.tool)
                .filter(|entry| {
                    entry.source.resolved_ref.is_some() || entry.source.checksum.is_some()
                })
                .map(|entry| entry.source.subpath.clone())
                .unwrap_or_else(|| entry.selector.subpath.clone())
        } else {
            entry.selector.subpath.clone()
        };
        source.subpaths = vec![subpath.clone()];
        let mut checked = entry.clone();
        checked.source = source.clone();
        checked.selector.subpath = subpath.clone();
        macc_core::catalog::availability::annotate(&paths, std::slice::from_mut(&mut checked));
        macc_core::catalog::availability::ensure_selectable(&checked)?;
        let unit = FetchUnit {
            source: source.clone(),
            selections: vec![Selection {
                id: request.id.clone(),
                subpath,
                kind: SelectionKind::Skill,
                tools: vec![request.tool.clone()],
            }],
        };
        let mut materialized = macc_adapter_shared::fetch::materialize_fetch_unit_refresh(
            &paths, unit, true, offline, refresh,
        )?;
        if let Some(provenance) = &mut materialized.provenance {
            provenance.tracking_ref = request.reference.clone();
        }
        // Preserve the original tracking ref while the resolver uses a locked SHA.
        if request.reference.is_none() && request.source.is_none() {
            if let (Some(provenance), Some(old)) = (
                &mut materialized.provenance,
                locking::record(&lock, &request.id, &request.tool),
            ) {
                if let Some(reference) = &old.source.requested_ref {
                    provenance.requested.reference = reference.clone();
                }
            }
        }
        units.push(materialized);
    }
    let mut selected = original.clone();
    select(&mut selected, &requests)?;
    let staged = if dry_run {
        selected
    } else {
        config::transaction::update(&paths, Some(&original_revision), |config| {
            select(config, &requests)
        })?
    };
    let staged_revision = config::transaction::revision(&staged)?;
    let roots = requests
        .iter()
        .map(|request| {
            (
                request.tool.clone(),
                app.engine.skill_install_directory(&request.tool),
            )
        })
        .collect();
    let ui = CliLifecycleUi::default();
    let mut attempted = false;
    let result = (|| {
        let plan = macc_core::skills_catalog::lifecycle::installation_plan(&paths, &units, &roots)?;
        let operations = app.engine.plan_operations(&paths, &plan);
        if dry_run {
            return ui.render_plan_preview(&paths, &plan, &operations, false, false);
        }
        ui.info("MACC Skills Install Review");
        for unit in &units {
            if let Some(provenance) = &unit.provenance {
                for selection in &unit.selections {
                    ui.info(&format!(
                        "  {} · {} · {}",
                        selection.id,
                        selection.tools.join(", "),
                        if provenance.resolved.reference.is_empty() {
                            provenance.resolved.checksum.as_deref().unwrap_or("local")
                        } else {
                            &provenance.resolved.reference
                        }
                    ));
                }
            }
        }
        ui.info(&format!(
            "Files: {} · lockfile: .macc/skills.lock.json · verified snapshot cache",
            operations
                .iter()
                .filter(|operation| operation.path != ".macc/skills.lock.json")
                .count()
        ));
        if !ui.confirm_yes_no("Install these exact skill versions and copied files [y/N]? ")? {
            return Err(MaccError::Validation(
                "Skill installation cancelled; selection restored".into(),
            ));
        }
        attempted = true;
        let report = macc_core::skills_catalog::lifecycle::apply_installation(
            &paths, &plan, &units, &roots,
        )?;
        ui.apply_result(&report)
    })();
    if result.is_err() && !dry_run && !attempted {
        config::transaction::update(&paths, Some(&staged_revision), |config| {
            *config = original.clone();
            Ok(())
        })?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, process::Command, sync::Arc};
    fn git(root: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().into()
    }
    #[test]
    fn install_update_dry_run_and_local_edit_protection_use_real_client_commands() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("origin");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["config", "user.name", "Test"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        fs::create_dir(repo.join("scripts")).unwrap();
        fs::write(repo.join("scripts/check.txt"), "nested content").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = repo.join("scripts/run.sh");
            fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(repo.join("SKILL.md"), "one").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "one"]);
        let one = git(&repo, &["rev-parse", "HEAD"]);
        let paths = macc_core::ProjectPaths::from_root(dir.path().join("project"));
        fs::create_dir_all(&paths.macc_dir).unwrap();
        let ids = macc_core::TestEngine::generate_fixture_ids(2);
        let tool = ids[0].clone();
        let engine = Arc::new(macc_core::TestEngine::with_fixtures_for_ids(&ids));
        let mut config = CanonicalConfig::default();
        config.tools.enabled = vec![tool.clone()];
        config.settings.offline = false;
        fs::write(&paths.config_path, config.to_yaml().unwrap()).unwrap();
        let entry:macc_core::catalog::SkillEntry=serde_json::from_value(serde_json::json!({"id":"test-skill","name":"Test skill","description":"","tags":[],"selector":{"subpath":""},"source":{"kind":"git","url":repo.to_str().unwrap(),"ref":"main"}})).unwrap();
        let mut catalog = macc_core::catalog::SkillsCatalog::default();
        catalog.entries.push(entry);
        catalog
            .save_atomically(&paths, &paths.project_skills_catalog_path())
            .unwrap();
        let app = AppContext::new(paths.root.clone(), engine, Default::default());
        use crate::commands::Command as _;
        crate::commands::skills_cmd::SkillsCmdCommand {
            app: app.clone(),
            subcommand: crate::commands::skills_cmd::SkillsSubcommand::Install {
                id: "test-skill".into(),
                tool: tool.clone(),
                source: None,
                reference: None,
                dry_run: false,
            },
        }
        .run()
        .unwrap();
        let installed = paths
            .root
            .join(format!(".{tool}/skills/test-skill/SKILL.md"));
        assert_eq!(fs::read_to_string(&installed).unwrap(), "one");
        assert!(paths
            .root
            .join(format!(".{tool}/skills/test-skill/scripts/check.txt"))
            .exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = paths
                .root
                .join(format!(".{tool}/skills/test-skill/scripts/run.sh"));
            assert_ne!(
                fs::metadata(script).unwrap().permissions().mode() & 0o111,
                0
            );
        }
        let lock = SkillsLockFile::load(&paths.skills_lock_path()).unwrap();
        assert_eq!(
            lock.find("test-skill", &tool)
                .unwrap()
                .source
                .resolved_ref
                .as_deref(),
            Some(one.as_str())
        );
        fs::write(repo.join("SKILL.md"), "two").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "two"]);
        let two = git(&repo, &["rev-parse", "HEAD"]);
        let before = fs::read(paths.skills_lock_path()).unwrap();
        update(&app, Some("test-skill"), Some(&tool), true, false).unwrap();
        assert_eq!(fs::read(paths.skills_lock_path()).unwrap(), before);
        assert_eq!(fs::read_to_string(&installed).unwrap(), "one");
        update(&app, Some("test-skill"), Some(&tool), false, false).unwrap();
        assert_eq!(fs::read_to_string(&installed).unwrap(), "two");
        assert_eq!(
            SkillsLockFile::load(&paths.skills_lock_path())
                .unwrap()
                .find("test-skill", &tool)
                .unwrap()
                .source
                .resolved_ref
                .as_deref(),
            Some(two.as_str())
        );
        fs::rename(&repo, dir.path().join("offline-origin")).unwrap();
        install(
            &app,
            Request {
                id: "test-skill".into(),
                tool: tool.clone(),
                reference: None,
                source: None,
            },
            false,
        )
        .unwrap();
        fs::write(&installed, "local work").unwrap();
        assert!(install(
            &app,
            Request {
                id: "test-skill".into(),
                tool,
                reference: None,
                source: None
            },
            false
        )
        .is_err());
        let lock = SkillsLockFile::load(&paths.skills_lock_path()).unwrap();
        let cached = locking::cache_location(
            &paths.cache_dir,
            &lock.find("test-skill", &ids[0]).unwrap().cache.cache_key,
        )
        .join("tree/SKILL.md");
        assert_eq!(fs::read_to_string(cached).unwrap(), "two");
        assert_eq!(fs::read_to_string(installed).unwrap(), "local work");
    }
}
