use macc_core::{prd_queue, MaccError, ProjectPaths, Result};

pub fn handle(paths: &ProjectPaths, args: &[String]) -> Result<()> {
    let action = args.first().map(String::as_str).unwrap_or("list");
    if matches!(action, "help" | "--help") {
        println!("macc coordinator prds [list|check|status|edit|add PATH|remove POSITION|move FROM TO|reset --confirm]\nPositions start at 1. Directories import immediate JSON files in lexical order. edit opens the visual terminal editor. reset resets progress only; it does not undo delivered tasks.");
        return Ok(());
    }
    let mut config = macc_core::load_canonical_config(&paths.config_path)?;
    let coordinator = config
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    let mut files = if coordinator.prd_files.is_empty() {
        coordinator.prd_file.iter().cloned().collect()
    } else {
        coordinator.prd_files.clone()
    };
    if action == "status" {
        let progress = prd_queue::load(&paths.root)?;
        if args.get(1).is_some_and(|arg| arg == "--json") {
            println!(
                "{}",
                serde_json::to_string_pretty(&progress)
                    .map_err(|e| MaccError::Validation(e.to_string()))?
            );
        } else {
            println!(
                "PRD queue: {} | {}/{} completed",
                if progress.status.is_empty() {
                    "not started"
                } else {
                    &progress.status
                },
                progress.next,
                progress.entries.len()
            );
            if let Some(entry) = progress.entries.get(progress.next) {
                println!("Current: {}", entry.path);
            }
            if let Some(reason) = progress.reason {
                println!("Cause: {reason}");
            }
            println!("Inspect: macc coordinator prds check\nReset progress only: macc coordinator prds reset --confirm");
        }
        return Ok(());
    }
    if action == "list" || action == "check" {
        if files.is_empty() {
            println!("No queue configured; legacy default: prd.json");
        }
        for (i, file) in files.iter().enumerate() {
            println!("{:>3}. {}", i + 1, file);
        }
        if action == "check" {
            let entries = prd_queue::validate(&paths.root, &files)?;
            println!(
                "Validated {} PRDs, {} tasks",
                entries.len(),
                entries.iter().map(|e| e.task_ids.len()).sum::<usize>()
            );
        }
        return Ok(());
    }
    let _lock = macc_core::fs_lock::AdvisoryLock::acquire(
        &paths.root.join(".macc/state/prd-queue.lock"),
        std::time::Duration::ZERO,
        "PRD queue edit",
    )?;
    let parameter = |index: usize| {
        args.get(index).ok_or_else(|| {
            MaccError::Validation("Missing argument; use coordinator prds help".into())
        })
    };
    let position = |index: usize| -> Result<usize> {
        parameter(index)?
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0 && *n <= files.len())
            .map(|n| n - 1)
            .ok_or_else(|| {
                MaccError::Validation("Position out of range (positions start at 1)".into())
            })
    };
    match action {
        "add" => files = prd_queue::import(&paths.root, parameter(1)?, &files)?,
        "remove" => {
            let index = position(1)?;
            files.remove(index);
        }
        "move" => {
            let from = position(1)?;
            let to = position(2)?;
            let file = files.remove(from);
            files.insert(to, file);
        }
        "edit" => match macc_tui::prd_queue::edit(paths.root.clone(), files.clone())
            .map_err(|e| MaccError::Validation(e.to_string()))?
        {
            Some(edited) => files = edited,
            None => return Ok(()),
        },
        "reset" if args.get(1).map(String::as_str) == Some("--confirm") => {
            prd_queue::save(&paths.root, &prd_queue::Progress::default())?;
            println!("Queue progress reset; existing task delivery state is preserved.");
            return Ok(());
        }
        _ => {
            return Err(MaccError::Validation(
                "Unknown action; use coordinator prds help".into(),
            ))
        }
    }
    coordinator.prd_files = files;
    macc_core::save_canonical_config(paths, &config)?;
    println!("PRD queue saved. Run coordinator prds check before launching. Existing progress is preserved.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prd_queue_reset_confirmation_reaches_command_handler() {
        use clap::Parser;
        let cli = crate::Cli::try_parse_from(["macc", "coordinator", "prds", "reset", "--confirm"])
            .unwrap();
        match cli.command {
            Some(crate::Commands::Coordinator {
                command_name,
                extra_args,
                ..
            }) => {
                assert_eq!(command_name, "prds");
                assert_eq!(extra_args, vec!["reset", "--confirm"]);
            }
            _ => panic!("expected coordinator command"),
        }
    }

    #[test]
    fn commands_persist_order_remove_entries_and_reset_only_progress() {
        let root = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(root.path());
        std::fs::create_dir_all(&paths.macc_dir).unwrap();
        macc_core::save_canonical_config(&paths, &macc_core::config::CanonicalConfig::default())
            .unwrap();
        std::fs::write(root.path().join("a.json"), r#"{"tasks":[{"id":"A"}]}"#).unwrap();
        std::fs::write(root.path().join("b.json"), r#"{"tasks":[{"id":"B"}]}"#).unwrap();
        let command = |args: &[&str]| {
            handle(
                &paths,
                &args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
            )
        };
        command(&["add", "."]).unwrap();
        command(&["move", "2", "1"]).unwrap();
        let read_files = || {
            macc_core::load_canonical_config(&paths.config_path)
                .unwrap()
                .automation
                .coordinator
                .unwrap()
                .prd_files
        };
        assert_eq!(read_files(), vec!["b.json", "a.json"]);
        command(&["check"]).unwrap();
        assert!(command(&["move", "0", "1"]).is_err());
        command(&["remove", "1"]).unwrap();
        assert_eq!(read_files(), vec!["a.json"]);
        assert!(root.path().join("b.json").exists());
        prd_queue::save(
            root.path(),
            &prd_queue::Progress {
                entries: prd_queue::validate(root.path(), &read_files()).unwrap(),
                next: 1,
                status: "completed".into(),
                reason: None,
            },
        )
        .unwrap();
        assert!(command(&["reset"]).is_err());
        assert_eq!(prd_queue::load(root.path()).unwrap().next, 1);
        command(&["reset", "--confirm"]).unwrap();
        assert_eq!(prd_queue::load(root.path()).unwrap().next, 0);
        assert_eq!(read_files(), vec!["a.json"]);
    }
}
