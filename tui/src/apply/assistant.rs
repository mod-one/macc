//! Interactive recovery stays inside Apply and reuses typed O2 configuration edits.
use super::*;
use macc_core::service::preparation::{Approval, Inputs, Report};
use ratatui::{
    widgets::{Clear, Paragraph, Wrap},
    Frame,
};

#[derive(Default)]
pub(crate) struct State {
    pub open: bool,
    pub report: Option<Report>,
    mode: Mode,
    selected: usize,
    input: String,
    error: Option<String>,
    edit: Option<macc_core::o2::policy_edit::Preview>,
    scroll: u16,
}
#[derive(Default, PartialEq)]
enum Mode {
    #[default]
    Overview,
    Suite,
    Settings,
    Value,
    Confirm,
}
impl AppState {
    pub(crate) fn apply_assistant_open(&self) -> bool {
        self.apply_runtime.assistant.open
    }
    fn resume_apply_assistant(&mut self, inputs: Inputs) {
        self.apply_runtime.assistant.open = false;
        if let Some(paths) = &self.project_paths {
            if let Ok(config) = macc_core::load_canonical_config(&paths.config_path) {
                self.config = Some(config.clone());
                self.working_copy = Some(config);
            }
        }
        self.open_apply_screen();
        self.apply_runtime.inputs = inputs;
        self.apply_runtime.resume_after_prepare = true;
    }
    pub(super) fn apply_assistant_key(&mut self, key: KeyCode) {
        let Some(root) = self.project_paths.as_ref().map(|p| p.root.clone()) else {
            return;
        };
        if key == KeyCode::Esc {
            self.apply_runtime.assistant.open = false;
            return;
        }
        let fields = macc_core::o2::policy::fields();
        let state = &mut self.apply_runtime.assistant;
        if key == KeyCode::PageDown {
            state.scroll = state.scroll.saturating_add(5);
            return;
        }
        if key == KeyCode::PageUp {
            state.scroll = state.scroll.saturating_sub(5);
            return;
        }
        state.error = None;
        match state.mode {
            Mode::Settings => match key {
                KeyCode::Up => state.selected = state.selected.saturating_sub(1),
                KeyCode::Down => {
                    state.selected = (state.selected + 1).min(fields.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    let field = &fields[state.selected];
                    match macc_core::o2::policy_edit::get(&root, &field.path) {
                        Ok(view) => {
                            state.input = view
                                .value
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| view.value.to_string());
                            state.mode = Mode::Value;
                        }
                        Err(e) => state.error = Some(e.to_string()),
                    }
                }
                _ => {}
            },
            Mode::Value | Mode::Suite => match key {
                KeyCode::Backspace => {
                    state.input.pop();
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                    if state.mode == Mode::Value && fields[state.selected].kind == "bool" =>
                {
                    state.input = if state.input == "true" {
                        "false"
                    } else {
                        "true"
                    }
                    .into();
                }
                KeyCode::Left | KeyCode::Right
                    if state.mode == Mode::Value && fields[state.selected].kind == "enum" =>
                {
                    let choices = &fields[state.selected].choices;
                    if !choices.is_empty() {
                        let n = choices.iter().position(|s| s == &state.input).unwrap_or(0);
                        state.input = choices[(n + 1) % choices.len()].clone();
                    }
                }
                KeyCode::Char(c) => state.input.push(c),
                KeyCode::Enter if state.mode == Mode::Suite => {
                    let bytes = if state.input.trim().starts_with('{') {
                        Ok(state.input.as_bytes().to_vec())
                    } else {
                        std::fs::read(root.join(state.input.trim()))
                    };
                    let parsed = bytes.map_err(|e| e.to_string()).and_then(|raw| {
                        if raw.len() > 131072 {
                            return Err("Benchmark input exceeds 128 KiB".into());
                        }
                        serde_json::from_slice(&raw).map_err(|e| e.to_string())
                    });
                    match parsed {
                        Ok(suite) => {
                            self.resume_apply_assistant(Inputs {
                                semantic_suite: Some(suite),
                                semantic_approval: None,
                            });
                        }
                        Err(e) => state.error = Some(e),
                    }
                }
                KeyCode::Enter => match macc_core::o2::policy_edit::preview(
                    &root,
                    &fields[state.selected].path,
                    &state.input,
                    None,
                ) {
                    Ok(preview) => {
                        state.edit = Some(preview);
                        state.mode = Mode::Confirm;
                    }
                    Err(e) => state.error = Some(e.to_string()),
                },
                _ => {}
            },
            Mode::Confirm => {
                if key == KeyCode::Enter {
                    if let Some(edit) = &state.edit {
                        match macc_core::o2::policy_edit::apply(
                            &root,
                            &edit.path,
                            &state.input,
                            &edit.revision,
                            true,
                        ) {
                            Ok(_) => self.resume_apply_assistant(Default::default()),
                            Err(e) => state.error = Some(e.to_string()),
                        }
                    }
                }
            }
            Mode::Overview => match key {
                KeyCode::Char('c') => {
                    state.mode = Mode::Settings;
                    state.scroll = 0;
                }
                KeyCode::Char('s') => {
                    state.mode = Mode::Suite;
                    state.input.clear();
                }
                KeyCode::Enter => {
                    let question = state.report.as_ref().and_then(|r| r.question.as_ref());
                    match question.map(|q| q.kind.as_str()) {
                        Some("network_access") => {
                            let revision = state.report.as_ref().unwrap().revision.clone();
                            match macc_core::service::preparation::allow_network(
                                &root, &revision, true,
                            ) {
                                Ok(()) => self.resume_apply_assistant(Default::default()),
                                Err(e) => state.error = Some(e.to_string()),
                            }
                        }
                        Some("semantic_suite") => {
                            state.mode = Mode::Suite;
                            state.input.clear();
                        }
                        Some("semantic_review") => {
                            let data = &question.unwrap().data;
                            if data["approvable"] == true {
                                let approval = Approval {
                                    report_id: data["report"]["id"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .into(),
                                    checksum: data["checksum"].as_str().unwrap_or_default().into(),
                                };
                                self.resume_apply_assistant(Inputs {
                                    semantic_suite: None,
                                    semantic_approval: Some(approval),
                                });
                            } else {
                                state.error=Some("Benchmark did not pass. Supply revised labels or adjust configuration.".into());
                            }
                        }
                        _ => self.resume_apply_assistant(Default::default()),
                    }
                }
                _ => {}
            },
        }
    }
    pub(crate) fn draw_apply_assistant(&self, f: &mut Frame) {
        let state = &self.apply_runtime.assistant;
        let area = crate::ui::centered_rect(90, 90, crate::ui::content_area(f.size()));
        f.render_widget(Clear, area);
        let mut text = String::new();
        let fields = macc_core::o2::policy::fields();
        match state.mode {
            Mode::Overview => {
                if let Some(report) = &state.report {
                    text.push_str(&format!("Preparation: {}\n", report.status));
                    for step in &report.completed {
                        text.push_str(&format!("✓ {step}\n"));
                    }
                    if let Some(error) = &report.error {
                        text.push_str(&format!("\n{error}\n"));
                    }
                    if let Some(question) = &report.question {
                        text.push_str(&format!("\n{}\n", question.message));
                        if question.kind == "semantic_review" {
                            text.push_str(
                                &serde_json::to_string_pretty(&question.data).unwrap_or_default(),
                            );
                        }
                    }
                } else {
                    text.push_str(
                        self.apply_error
                            .as_deref()
                            .unwrap_or("Review the Apply error and adjust configuration."),
                    );
                }
            }
            Mode::Settings => {
                text.push_str("Choose an O2 setting. Values are validated before saving.\n\n");
                let first = state.selected.saturating_sub(6);
                for (i, field) in fields.iter().enumerate().skip(first).take(12) {
                    text.push_str(&format!(
                        "{} {}\n",
                        if i == state.selected { "›" } else { " " },
                        field.label
                    ));
                }
            }
            Mode::Value => {
                let field = &fields[state.selected];
                text.push_str(&format!(
                    "{}\n{}\n\n{}_\n\n{}",
                    field.label, field.path, state.input, field.activation
                ));
            }
            Mode::Confirm => {
                text.push_str("Confirm configuration change\n\n");
                if let Some(edit) = &state.edit {
                    text.push_str(&serde_json::to_string_pretty(edit).unwrap_or_default());
                }
            }
            Mode::Suite => text.push_str(&format!(
                "Labelled benchmark suite\nEnter a JSON file path or paste a JSON object.\n\n{}_\n",
                state.input
            )),
        }
        if let Some(error) = &state.error {
            text.push_str(&format!("\n\n{error}"));
        }
        f.render_widget(
            Paragraph::new(text)
                .scroll((state.scroll, 0))
                .block(crate::ui::panel("Apply assistant"))
                .wrap(Wrap { trim: false }),
            area,
        );
    }
}
