use crate::commands::AppContext;
use macc_core::service::interaction::InteractionHandler;
use macc_core::{MaccError, ProjectPaths, Result};

pub(crate) struct CliFetchMaterializer;

impl macc_core::service::lifecycle::LifecycleFetchMaterializer for CliFetchMaterializer {
    fn materialize_fetch_units(
        &self,
        paths: &ProjectPaths,
        units: Vec<macc_core::resolve::FetchUnit>,
        quiet: bool,
        offline: bool,
    ) -> Result<Vec<macc_core::resolve::MaterializedFetchUnit>> {
        macc_adapter_shared::fetch::materialize_fetch_units(paths, units, quiet, offline)
    }
}

#[derive(Default)]
pub(crate) struct CliLifecycleUi {
    pub(super) output: Option<super::apply_output::Options>,
}

impl InteractionHandler for CliLifecycleUi {
    fn info(&self, message: &str) {
        if let Some(options) = &self.output {
            if !options.quiet {
                eprintln!("{}", message);
            }
        } else {
            println!("{}", message);
        }
    }

    fn warn(&self, message: &str) {
        eprintln!("{}", message);
    }

    fn error(&self, message: &str) {
        eprintln!("{}", message);
    }

    fn confirm_yes_no(&self, prompt: &str) -> Result<bool> {
        if self.output.is_some() {
            self.apply_confirm(prompt)
        } else {
            crate::confirm_yes_no(prompt)
        }
    }
}

impl macc_core::service::lifecycle::LifecycleUi for CliLifecycleUi {
    fn review_apply(
        &self,
        paths: &ProjectPaths,
        plan: &macc_core::plan::ActionPlan,
        ops: &[macc_core::plan::PlannedOp],
    ) -> Result<()> {
        if self.output.is_some() {
            self.review(paths, plan, ops)
        } else {
            self.info(&macc_core::service::apply_presentation::review(
                paths, plan, ops,
            ));
            Ok(())
        }
    }
    fn decision_info(&self, message: &str) {
        self.finish_progress();
        eprintln!("{message}");
    }
    fn apply_progress(&self, report: &macc_core::service::preparation::Report) {
        self.preparation_progress(report);
    }
    fn apply_result(&self, report: &macc_core::ApplyReport) -> Result<()> {
        if self.output.is_some() {
            self.result(report)
        } else {
            self.info(&report.render_cli());
            Ok(())
        }
    }
    fn print_checks(&self, checks: &[macc_core::doctor::ToolCheck]) {
        crate::print_checks(checks);
    }

    fn render_plan_preview(
        &self,
        paths: &ProjectPaths,
        plan: &macc_core::plan::ActionPlan,
        ops: &[macc_core::plan::PlannedOp],
        json: bool,
        explain: bool,
    ) -> Result<()> {
        if self.output.as_ref().is_some_and(|o| !o.verbose || o.quiet)
            && !json
            && (!explain || self.output.as_ref().is_some_and(|o| o.quiet))
        {
            if self.output.as_ref().is_some_and(|o| o.quiet) {
                for line in macc_core::service::apply_presentation::review(paths, plan, ops)
                    .lines()
                    .filter(|s| s.starts_with("WARN "))
                {
                    eprintln!("{line}");
                }
            } else {
                self.review_apply(paths, plan, ops)?;
            }
            self.info("Preview only; no project changes applied.");
            Ok(())
        } else {
            crate::render_plan_preview(paths, plan, ops, json, explain)
        }
    }

    fn print_pre_apply_summary(
        &self,
        paths: &ProjectPaths,
        plan: &macc_core::plan::ActionPlan,
        ops: &[macc_core::plan::PlannedOp],
    ) {
        crate::print_pre_apply_summary(paths, plan, ops);
    }

    fn print_pre_apply_explanations(&self, ops: &[macc_core::plan::PlannedOp]) {
        if self.output.as_ref().is_some_and(|o| o.quiet) {
            return;
        }
        if self.output.is_some() {
            for op in ops {
                eprintln!("{:?} {}", op.kind, op.path);
            }
        } else {
            crate::print_pre_apply_explanations(ops);
        }
    }

    fn confirm_user_scope_apply(
        &self,
        paths: &ProjectPaths,
        ops: &[macc_core::plan::PlannedOp],
    ) -> Result<()> {
        if self.output.is_some() {
            self.user_scope(paths, ops)
        } else {
            crate::confirm_user_scope_apply(paths, ops)
        }
    }

    fn mark_apply_completed(&self, paths: &ProjectPaths) -> Result<()> {
        macc_core::service::context::mark_apply_completed(paths)
    }

    fn run_tui(&self) -> Result<()> {
        macc_tui::run_tui().map_err(|e| MaccError::Io {
            path: "tui".into(),
            action: "run_tui".into(),
            source: std::io::Error::other(e.to_string()),
        })
    }

    fn set_current_dir(&self, path: &std::path::Path) -> Result<()> {
        std::env::set_current_dir(path).map_err(|e| MaccError::Io {
            path: path.to_string_lossy().into(),
            action: "set current_dir for tui".into(),
            source: e,
        })
    }

    fn prompt_line(&self, prompt: &str) -> Result<String> {
        use std::io::{self, IsTerminal, Write};
        self.finish_progress();
        if self.output.is_some() {
            if !io::stdin().is_terminal() {
                return Err(MaccError::Validation(
                    "Preparation requires interactive input; completed work and logs are retained"
                        .into(),
                ));
            }
            eprint!("{}", prompt);
        } else {
            print!("{}", prompt);
        }
        io::stderr().flush().map_err(|e| MaccError::Io {
            path: "stdout".into(),
            action: "flush prompt".into(),
            source: e,
        })?;
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| MaccError::Io {
                path: "stdin".into(),
                action: "read input".into(),
                source: e,
            })?;
        Ok(input.trim().to_string())
    }

    fn is_command_available(&self, command: &str) -> bool {
        std::process::Command::new("sh")
            .arg("-lc")
            .arg(format!("command -v {} >/dev/null 2>&1", command))
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn start_coordinator_background(
        &self,
        paths: &macc_core::ProjectPaths,
    ) -> macc_core::Result<()> {
        // Resolve the `macc` binary path (same binary we're running from).
        let macc_bin = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("macc"));

        let mut cmd = std::process::Command::new(&macc_bin);
        cmd.arg("--cwd")
            .arg(&paths.root)
            .arg("coordinator")
            .arg("run")
            .arg("--no-tui")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        // Detach: on Unix, put the child in its own process group so it survives
        // the parent (quickstart) exiting.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }

        cmd.spawn().map_err(|e| macc_core::MaccError::Io {
            path: macc_bin.to_string_lossy().into(),
            action: "spawn coordinator background".into(),
            source: e,
        })?;

        Ok(())
    }
}

pub(crate) fn init(app: &AppContext, force: bool, wizard: bool) -> Result<()> {
    macc_core::service::lifecycle::init(
        &app.cwd,
        app.engine.as_ref(),
        force,
        wizard,
        &CliLifecycleUi::default(),
    )
}

pub(crate) fn plan(app: &AppContext, tools: Option<&str>, json: bool, explain: bool) -> Result<()> {
    let mut overrides = app.overrides.clone();
    if let Some(tools_csv) = tools {
        let (_descriptors, _diagnostics) = app.engine.list_tools(&app.project_paths()?);
        let allowed_tools: Vec<String> = _descriptors.iter().map(|d| d.id.clone()).collect();
        overrides.tools =
            macc_core::resolve::CliOverrides::from_tools_csv(tools_csv, &allowed_tools)?.tools;
    }

    macc_core::service::lifecycle::plan(
        &app.cwd,
        app.engine.as_ref(),
        overrides,
        json,
        explain,
        &CliLifecycleUi::default(),
        &CliFetchMaterializer,
    )
}

pub(crate) fn apply(
    app: &AppContext,
    tools: Option<&str>,
    dry_run: bool,
    allow_user_scope: bool,
    json: bool,
    explain: bool,
    yes: bool,
) -> Result<()> {
    let mut overrides = app.overrides.clone();
    if let Some(tools_csv) = tools {
        let (_descriptors, _diagnostics) = app.engine.list_tools(&app.project_paths()?);
        let allowed_tools: Vec<String> = _descriptors.iter().map(|d| d.id.clone()).collect();
        overrides.tools =
            macc_core::resolve::CliOverrides::from_tools_csv(tools_csv, &allowed_tools)?.tools;
    }

    let config = app.canonical_config()?;
    let ui = CliLifecycleUi {
        output: Some(super::apply_output::Options {
            json,
            verbose: config.settings.debug || std::env::var("MACC_DEBUG").is_ok_and(|v| v == "1"),
            quiet: app.overrides.quiet.unwrap_or(config.settings.quiet),
            yes,
            ..Default::default()
        }),
    };
    let result = macc_core::service::lifecycle::apply(
        &app.cwd,
        app.engine.as_ref(),
        overrides,
        dry_run,
        allow_user_scope,
        json,
        explain,
        &ui,
        &CliFetchMaterializer,
    );
    ui.finish_progress();
    if result.is_err() && !json {
        eprintln!(
            "Apply stopped. Completed work is retained. Review the cause and retry Apply. Logs: {}",
            app.cwd.join(".macc/log/apply").display()
        );
    }
    result
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn quickstart_extended(
    app: &AppContext,
    assume_yes: bool,
    apply: bool,
    no_tui: bool,
    tool: Option<&str>,
    starter_task: bool,
    start_coordinator: bool,
    check_only: bool,
    json: bool,
) -> Result<()> {
    macc_core::service::lifecycle::quickstart_extended(
        &app.cwd,
        app.engine.as_ref(),
        app.overrides.clone(),
        assume_yes,
        apply,
        no_tui,
        tool,
        starter_task,
        start_coordinator,
        check_only,
        json,
        &CliLifecycleUi::default(),
        &CliFetchMaterializer,
    )
}
