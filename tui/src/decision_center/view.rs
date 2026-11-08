use super::{
    interaction::{Control, MORE},
    *,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

impl Center {
    pub fn draw(&self, frame: &mut Frame) {
        if !self.open {
            return;
        }
        let area = frame.size();
        let width = area
            .width
            .saturating_sub(if area.width > 40 { 4 } else { 0 })
            .min(110);
        let height = area
            .height
            .saturating_sub(if area.height > 12 { 2 } else { 0 })
            .min(24);
        let rect = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, area);
        let title = if self.actions.open {
            "More actions"
        } else if self.feedback {
            "Project direction"
        } else if self
            .current
            .as_ref()
            .is_some_and(|d| d.status != "pending_human")
        {
            "Recorded decision"
        } else if self
            .current
            .as_ref()
            .is_some_and(|d| d.request.kind == "intervention")
        {
            "Action needed"
        } else {
            "Performer question"
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title} · {} pending ", self.pending()));
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        if inner.height == 0 || inner.width == 0 {
            return;
        }
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(if self.message.is_empty() {
                    0
                } else {
                    2.min(inner.height / 3)
                }),
                Constraint::Min(1),
                Constraint::Length(if inner.height < 18 {
                    4.min(inner.height.saturating_sub(3))
                } else {
                    7
                }),
                Constraint::Length(if inner.height < 10 { 1 } else { 3 }),
            ])
            .split(inner);
        if !self.message.is_empty() {
            frame.render_widget(
                Paragraph::new(self.message.as_str())
                    .style(Style::default().fg(if self.notice_success {
                        Color::Green
                    } else {
                        Color::Yellow
                    }))
                    .wrap(Wrap { trim: false }),
                parts[0],
            );
        }
        let key = if self.feedback {
            "feedback".into()
        } else {
            self.current.as_ref().map(Self::key).unwrap_or_default()
        };
        let draft = self.drafts.get(&key).cloned().unwrap_or_default();
        let fields = self.form_fields();
        let required = |field: &dm::ResponseField| self.field_required(field);
        let controls = self.controls();
        let focus = controls[self.focus % controls.len()];
        let mut summary = String::new();
        let mut editor = String::new();
        let mut caption = String::new();
        let mut editing = false;
        if self.actions.open {
            (summary, editor) = self.actions.view(
                self.current.as_ref(),
                self.snapshot.as_ref().unwrap_or(&Value::Null),
            );
            editor.push_str(&format!(
                "\n{}",
                self.actions
                    .navigation(self.snapshot.as_ref().unwrap_or(&Value::Null))
            ));
            caption = editor.lines().next().unwrap_or("Action form").to_owned();
        } else if self.reviewing_patch {
            summary = self
                .selected_patch()
                .map(|patch| format!("EXACT TASK-PLAN CHANGE\n{}", patch.describe().join("\n")))
                .unwrap_or_else(|| "No task-plan patch for this choice.".into());
            caption = "Review before accepting · PgUp/PgDn scroll".into();
            editor = "Accepting includes this exact patch with your response. It does not send the response yet. Tab selects Accept or Back; Enter confirms the selected button.".into();
        } else if self.quick_menu {
            for (i, label) in MORE.iter().enumerate() {
                summary.push_str(&format!(
                    "{} {label}\n",
                    if i == self.menu_index { "›" } else { " " }
                ));
            }
            let start = self
                .menu_index
                .saturating_sub(parts[1].height.saturating_sub(1) as usize);
            summary = summary.lines().skip(start).collect::<Vec<_>>().join("\n");
            caption = "Choose an action · ↑/↓ then Enter · Esc back".into();
            if self.menu_index == 10 {
                editor = self
                    .current
                    .as_ref()
                    .and_then(|d| {
                        d.request.patch_for(
                            draft
                                .option
                                .checked_sub(1)
                                .and_then(|i| d.request.options.get(i))
                                .map(|o| o.id.as_str()),
                        )
                    })
                    .map(|patch| {
                        format!("Exact task-plan change:\n{}", patch.describe().join("\n"))
                    })
                    .unwrap_or_else(|| "No task-plan patch is available for this choice.".into());
            } else {
                editor = MORE[self.menu_index].into();
            }
        } else {
            if let Some(d) = &self.current {
                let pending = d.status == "pending_human";
                summary.push_str(&format!(
                    "{}\n",
                    if self.expanded {
                        d.request.question.clone()
                    } else {
                        compact_question(&d.request.question)
                    }
                ));
                if self.expanded {
                    if let Some(action) = &d.request.intervention {
                        if action.action.trim() != d.request.question.trim() {
                            summary.push_str(&format!("\nAction: {}\n", action.action));
                        }
                        summary
                            .push_str(&format!("Result to return: {}\n", action.expected_result));
                    }
                    summary.push_str(&format!("\nTask {} · {}\n", d.request.task_id, d.status));
                } else if pending {
                    if let Some(action) = &d.request.intervention {
                        summary.push_str(&format!(
                            "\nExpected result: {}\n",
                            compact_question(&action.expected_result)
                        ));
                    }
                }
                if let Some(operation) = &d.request.operation_spec {
                    summary.push_str(&format!(
                        "\nExact external operation requested:\n{}\n",
                        serde_json::to_string_pretty(operation).unwrap_or_default()
                    ));
                }
                if self.expanded {
                    summary.push_str(&format!(
                        "\nCONTEXT\n{}\n\nRecommendation: {}\nRisk: {:?} · Version {}\n",
                        d.request.context, d.request.recommendation, d.effective_risk, d.version
                    ));
                    if let Some(a) = &d.request.intervention {
                        summary.push_str(&format!(
                            "\nWhy your help is needed: {}\nVerification: {}\n",
                            a.inaccessible_reason, a.verification
                        ));
                    }
                    if let Some(patch) = self.selected_patch() {
                        summary.push_str(&format!(
                            "\nExact proposed task-plan change:\n{}\n",
                            patch.describe().join("\n")
                        ));
                    }
                    for o in &d.request.options {
                        summary.push_str(&format!(
                            "\n{}: {}\n",
                            o.label,
                            o.consequences.join("; ")
                        ));
                    }
                    if let Some(contexts) = &self.inspected_contexts {
                        summary.push_str(&format!(
                            "\nContexts / receipts:\n{}",
                            serde_json::to_string_pretty(contexts).unwrap_or_default()
                        ));
                    }
                    if let Some(r) = &d.resolution {
                        for condition in &r.conditions {
                            summary.push_str(&format!(
                                "\nCondition: {} · verification task {:?} · evidence {:?} {:?}\n",
                                condition.description,
                                condition.task_id,
                                condition.source_path,
                                condition.source_digest
                            ));
                        }
                        if !r.fields.is_empty() {
                            summary.push_str(&format!(
                                "\nRecorded user-reported fields:\n{}\n",
                                serde_json::to_string_pretty(&r.fields).unwrap_or_default()
                            ));
                        }

                        summary.push_str(&format!(
                            "\nRecorded by {} ({}):\n{}\n",
                            r.actor, r.origin, r.text
                        ));
                    }
                }
            } else {
                summary
                    .push_str("No pending questions. More opens history and project directions.");
            }
            if self
                .current
                .as_ref()
                .is_some_and(|d| d.status == "pending_human")
            {
                summary.push_str(&self.response_overview());
            }
            summary.push_str(&self.delivery_overview());
            match focus {
                Control::Mode => {
                    caption = "Response type — choose explicitly".into();
                    editor = [
                        dm::InterventionResponse::Completed,
                        dm::InterventionResponse::ConfirmFact,
                        dm::InterventionResponse::DeferObservation,
                        dm::InterventionResponse::Unable,
                    ]
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        format!(
                            "{} {}. {}",
                            if *m == self.response_mode() {
                                "●"
                            } else {
                                "○"
                            },
                            i + 1,
                            m.label()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                }
                Control::Note => {
                    caption = if self.feedback {
                        "Your project direction"
                    } else if self
                        .current
                        .as_ref()
                        .is_some_and(|d| d.request.kind == "intervention")
                        && draft.outcome == dm::AnswerOutcome::Answer
                    {
                        "Additional note (optional)"
                    } else {
                        "Your answer / reason"
                    }
                    .into();
                    editor = draft.text.clone();
                    editing = true;
                }
                Control::Field(i) => {
                    if let Some(field) = fields.get(i) {
                        caption = format!(
                            "{}{}",
                            field.label,
                            if required(field) {
                                " (required)"
                            } else {
                                " (optional)"
                            }
                        );
                        let value = draft.fields.get(&field.id).cloned().unwrap_or_default();
                        if field.choices.is_empty() {
                            editor = value;
                            editing = true;
                        } else {
                            editor = format!(
                                "{}\n←/→ choose · Enter continues",
                                field
                                    .choices
                                    .iter()
                                    .enumerate()
                                    .map(|(i, choice)| format!(
                                        "{} {} {choice}",
                                        if *choice == value { "●" } else { "○" },
                                        i + 1
                                    ))
                                    .collect::<Vec<_>>()
                                    .join("   ")
                            );
                        }
                    }
                }
                Control::Option => {
                    caption = "↑/↓ choose · Enter continues · 0: your own answer".into();
                    editor = "".into();
                    if let Some(d) = &self.current {
                        let labels = std::iter::once("Custom answer".to_owned())
                            .chain(d.request.options.iter().map(|o| {
                                format!(
                                    "{}{}",
                                    o.label,
                                    if Some(&o.id) == d.request.recommended_option_id.as_ref() {
                                        " (recommended)"
                                    } else {
                                        ""
                                    }
                                )
                            }))
                            .collect::<Vec<_>>();
                        let start = draft
                            .option
                            .saturating_sub(parts[2].height.saturating_sub(3) as usize);
                        editor = labels
                            .iter()
                            .enumerate()
                            .skip(start)
                            .map(|(i, label)| {
                                format!(
                                    "{} {i}. {label}",
                                    if i == draft.option { "●" } else { "○" }
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n");
                    }
                }
                _ => {
                    if self
                        .current
                        .as_ref()
                        .is_some_and(|d| d.status != "pending_human")
                    {
                        caption = "Recorded response".into();
                        editor = self
                            .current
                            .as_ref()
                            .and_then(|d| d.resolution.as_ref())
                            .map(|r| r.text.clone())
                            .unwrap_or_else(|| "Nothing to answer.".into());
                    } else {
                        caption = if self.submitting {
                            "Sending…"
                        } else {
                            "Enter: Send and close · Shift+Tab: change response"
                        }
                        .into();
                        editor = self
                            .current
                            .as_ref()
                            .map(|d| response_text(&d.request, &draft))
                            .unwrap_or_else(|| draft.text.clone());
                    }
                }
            }
        }
        if !self.quick_menu
            && !self.actions.open
            && self
                .current
                .as_ref()
                .is_some_and(|d| d.status == "pending_human")
        {
            let steps = self.controls();
            let editable = |c: &Control| {
                matches!(
                    c,
                    Control::Option | Control::Mode | Control::Field(_) | Control::Note
                )
            };
            if steps.get(self.focus).is_some_and(editable) {
                let total = steps.iter().filter(|c| editable(c)).count();
                let done = steps
                    .iter()
                    .take(self.focus + 1)
                    .filter(|c| editable(c))
                    .count();
                caption = format!("Step {done}/{total} · {caption}");
            }
        }
        let summary = Paragraph::new(summary).wrap(Wrap { trim: false });
        let max_scroll = summary
            .line_count(parts[1].width)
            .saturating_sub(parts[1].height as usize)
            .min(u16::MAX as usize) as u16;
        frame.render_widget(
            summary.scroll((
                if self.quick_menu {
                    0
                } else {
                    self.scroll.min(max_scroll)
                },
                0,
            )),
            parts[1],
        );
        let input_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(caption);
        let input_area = input_block.inner(parts[2]);
        if editing
            && !self.quick_menu
            && !self.actions.open
            && input_area.width > 0
            && input_area.height > 0
        {
            let cursor_key = match focus {
                Control::Field(i) => fields.get(i).map(|f| f.id.as_str()).unwrap_or("__text"),
                _ => "__text",
            };
            let cursor = draft
                .cursors
                .get(cursor_key)
                .copied()
                .filter(|i| editor.is_char_boundary(*i))
                .unwrap_or(editor.len());
            let prefix = &editor[..cursor];
            let row = prefix.chars().filter(|c| *c == '\n').count();
            let column = Line::from(prefix.rsplit('\n').next().unwrap_or("")).width();
            let vertical = row
                .saturating_sub(input_area.height.saturating_sub(1) as usize)
                .min(u16::MAX as usize) as u16;
            let horizontal = column
                .saturating_sub(input_area.width.saturating_sub(1) as usize)
                .min(u16::MAX as usize) as u16;
            frame.render_widget(
                Paragraph::new(editor)
                    .scroll((vertical, horizontal))
                    .block(input_block),
                parts[2],
            );
            frame.set_cursor(
                input_area.x
                    + column
                        .saturating_sub(horizontal as usize)
                        .min(input_area.width.saturating_sub(1) as usize)
                        as u16,
                input_area.y
                    + row
                        .saturating_sub(vertical as usize)
                        .min(input_area.height.saturating_sub(1) as usize)
                        as u16,
            );
        } else {
            let paragraph = Paragraph::new(editor).wrap(Wrap { trim: false });
            let offset = if self.actions.open {
                paragraph
                    .line_count(input_area.width)
                    .saturating_sub(input_area.height as usize)
                    .min(u16::MAX as usize) as u16
            } else {
                0
            };
            frame.render_widget(paragraph.scroll((offset, 0)).block(input_block), parts[2]);
        }
        let button_style = Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let mut buttons = Vec::new();
        for (control, label) in [
            (Control::Send, "Send"),
            (
                Control::Later,
                if self
                    .current
                    .as_ref()
                    .is_some_and(|d| d.status != "pending_human")
                {
                    "Close"
                } else {
                    "Later"
                },
            ),
            (
                Control::Details,
                if self.expanded {
                    "Hide details"
                } else {
                    "Details"
                },
            ),
            (Control::More, "More…"),
        ] {
            if controls.contains(&control) {
                buttons.push(Span::styled(
                    format!(" [{label}] "),
                    if focus == control && !self.quick_menu {
                        button_style
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ));
            }
        }
        let footer = if self.reviewing_patch {
            vec![
                Line::from(vec![
                    Span::styled(
                        " [Accept exact patch] ",
                        if self.patch_accept {
                            button_style
                        } else {
                            Style::default()
                        },
                    ),
                    Span::styled(
                        " [Back] ",
                        if !self.patch_accept {
                            button_style
                        } else {
                            Style::default()
                        },
                    ),
                ]),
                Line::from("Tab: choose · Enter: confirm · PgUp/PgDn: review · Esc: back"),
            ]
        } else if self.actions.open {
            vec![
                Line::from("Tab / Shift+Tab: fields and buttons"),
                Line::from("Enter: activate focused action · Esc: back"),
            ]
        } else if inner.height < 10 {
            vec![Line::from("Tab/Enter · Esc Later")]
        } else {
            vec![
                Line::from(buttons),
                Line::from(self.keyboard_hint(focus)),
                Line::from("PgUp/PgDn: scroll details · your draft is kept"),
            ]
        };
        frame.render_widget(Paragraph::new(footer).wrap(Wrap { trim: false }), parts[3]);
    }
}
