//! Review canonical edits and the saves observed during this TUI session.
use super::*;
use macc_core::config::CanonicalConfig;
use serde_json::Value;

pub(crate) fn field(path: &str) -> Option<(u8, usize)> {
    if let Some(key) = path.strip_prefix("automation.supervisor.") {
        return macc_core::supervisor::settings::FIELDS
            .iter()
            .position(|field| field.key == key)
            .map(|id| (2, id));
    }
    if let Some(key) = path.strip_prefix("automation.coordinator.decisions.") {
        return AppState::decision_field_index(key).map(|id| (3, id));
    }
    Some(match path {
        "settings.quiet" => (0, 0),
        "settings.offline" => (0, 1),
        "settings.web_port" => (0, 2),
        "settings.debug" => (0, 3),
        "automation.coordinator.phases.testing.enabled" => (1, 34),
        "automation.coordinator.phases.testing.mode" => (1, 35),
        "automation.coordinator.phases.review.enabled" => (1, 36),
        "automation.coordinator.phases.review.mode" => (1, 37),
        "automation.coordinator.max_review_cycles" => (1, 31),
        "automation.coordinator.require_clean_reference_branch" => (1, 38),
        "automation.coordinator.reference_branch_preflight.enabled" => (1, 39),
        _ => return None,
    })
}
// Free-form values may contain credentials. Only expose booleans and known phase modes.
fn visit(path: &str, old: Option<&Value>, new: Option<&Value>, rows: &mut Vec<(String, String)>) {
    if old == new {
        return;
    }
    // Editing the first phase switch can materialize the optional coordinator.
    // Show edits relative to its real defaults, including both mode and enabled.
    if old.is_none() || new.is_none() {
        let defaults = match path {
            "automation.coordinator" => {
                serde_json::to_value(macc_core::config::CoordinatorConfig::default()).ok()
            }
            "automation.coordinator.phases" => {
                serde_json::to_value(macc_core::config::PhasesConfig::default()).ok()
            }
            _ => None,
        };
        if let Some(defaults) = defaults {
            if old.or(new) != Some(&defaults) {
                visit(path, old.or(Some(&defaults)), new.or(Some(&defaults)), rows);
                return;
            }
        }
    }
    if let (Some(Value::Object(a)), Some(Value::Object(b))) = (old, new) {
        for key in a
            .keys()
            .chain(b.keys())
            .collect::<std::collections::BTreeSet<_>>()
        {
            let child = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            visit(&child, a.get(key), b.get(key), rows);
        }
    } else {
        let phase_mode = |v: &str| {
            matches!(
                v,
                "disabled" | "required" | "advisory" | "risk_based" | "manual"
            )
        };
        let change = match (old, new) {
            (None, _) => "Added".into(),
            (_, None) => "Removed".into(),
            (Some(Value::Bool(a)), Some(Value::Bool(b))) => format!(
                "{} → {}",
                crate::workspace_ui::switch(*a),
                crate::workspace_ui::switch(*b)
            ),
            (Some(Value::String(a)), Some(Value::String(b)))
                if path.starts_with("automation.coordinator.phases.")
                    && path.ends_with(".mode")
                    && phase_mode(a)
                    && phase_mode(b) =>
            {
                format!("{a} → {b}")
            }
            (Some(Value::Number(a)), Some(Value::Number(b))) => format!("{a} → {b}"),
            _ => "Changed".into(),
        };
        rows.push((path.into(), change));
    }
}
fn differences(
    before: Option<&CanonicalConfig>,
    after: Option<&CanonicalConfig>,
) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if before != after {
        if before.is_none() || after.is_none() {
            return vec![(
                "Project configuration".into(),
                if before.is_none() { "Added" } else { "Removed" }.into(),
            )];
        }
        let a = serde_json::to_value(before).unwrap_or_default();
        let b = serde_json::to_value(after).unwrap_or_default();
        visit("", Some(&a), Some(&b), &mut rows);
    }
    rows
}

pub(crate) fn draft(s: &AppState) -> Vec<(String, String)> {
    let mut rows = differences(s.config.as_ref(), s.working_copy.as_ref());
    for (path, old, new, inherited) in s.sessions.policy_changes() {
        if inherited {
            rows.push((
                if path.is_empty() { "O2 settings" } else { path }.into(),
                "Restore inherited policy".into(),
            ));
        } else {
            visit(path, Some(&old), Some(new), &mut rows);
        }
    }
    rows
}
pub(crate) fn saved(s: &AppState) -> Vec<(String, String)> {
    s.delivery_baseline
        .as_ref()
        .map(|baseline| differences(Some(baseline), s.config.as_ref()))
        .unwrap_or_default()
}

fn destination(path: &str) -> Destination {
    let mut parts = path.split('.');
    let section = parts.next().unwrap_or_default();
    let subsection = parts.next();
    match (section, subsection) {
        ("Session settings" | "session_management", _) => Destination::SessionSettings,
        (
            "O2 settings"
            | "memory"
            | "context_engine"
            | "adaptive_compaction"
            | "code_intelligence",
            _,
        ) => Destination::MemorySettings,
        ("tools", _) => Destination::Tools,
        ("hooks", _) => Destination::Hooks,
        ("skills", _) | ("selections", Some("skills")) => Destination::Skills,
        ("mcp_templates", _) | ("selections", Some("mcp")) => Destination::Mcp,
        ("selections", Some("agents")) => Destination::Agents,
        ("automation", _) => Destination::Coordinator,
        _ => Destination::General,
    }
}

fn location(path: &str) -> &str {
    if path.starts_with("automation.coordinator.phases.") {
        "Coordinator › Phases"
    } else if path.starts_with("automation.supervisor") {
        "Coordinator › Supervisor"
    } else if path.starts_with("automation.coordinator.decisions") {
        "Coordinator › Decisions"
    } else {
        destination(path).label()
    }
}
pub fn edit_change(s: &mut AppState) {
    let mut rows = draft(s);
    rows.extend(saved(s));
    let Some((path, _)) = rows.get(s.inspector_index.min(rows.len().saturating_sub(1))) else {
        return;
    };
    let dest = destination(path);
    dest.open(s);
    if matches!(
        dest,
        Destination::SessionSettings | Destination::MemorySettings
    ) {
        s.sessions.focus_policy_field(path);
    }
    if let Some((source, index)) = field(path) {
        if let Some((tab, row)) = (0..=7).find_map(|tab| {
            AppState::config_tab_fields(tab)
                .iter()
                .position(|f| *f == (source, index))
                .map(|row| (tab, row))
        }) {
            s.jump_config_tab(tab);
            s.config_view_index = row;
            s.sync_config_indices();
            return;
        }
    }
    let mut keys = path.split('.');
    match (keys.next(), keys.next(), keys.next()) {
        (Some("automation"), Some("coordinator"), Some("phases")) => s.jump_config_tab(3),
        (Some("automation"), Some("supervisor"), _) => s.jump_config_tab(6),
        (Some("automation"), Some("coordinator"), Some("decisions")) => s.jump_config_tab(7),
        _ => {}
    }
}

pub(crate) fn draw(f: &mut Frame, s: &AppState, area: Rect, drafts: &[(String, String)]) {
    let saved = saved(s);
    let (list, inspector) = workspace_ui::columns(area);
    let theme = theme();
    let mut items = Vec::new();
    let mut selected_row = None;
    let where_width = if list.width >= 69 { 24 } else { 0 };
    let total = drafts.len() + saved.len();
    let selected = s.inspector_index.min(total.saturating_sub(1));
    for (title, changes, offset, marker, color, empty) in [
        ("Unsaved", drafts, 0, "◆", theme.draft, "No unsaved edits."),
        (
            "Saved since Apply",
            saved.as_slice(),
            drafts.len(),
            "·",
            theme.muted,
            "No pending saves recorded here.",
        ),
    ] {
        if !items.is_empty() {
            items.push(ListItem::new(""));
        }
        items.push(ListItem::new(Line::styled(
            format!("{title} · {}", changes.len()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        if changes.is_empty() {
            items.push(ListItem::new(Line::styled(
                empty,
                Style::default().fg(theme.muted),
            )));
        }
        if where_width > 0 && !changes.is_empty() {
            items.push(ListItem::new(Line::styled(
                format!("  {:<where_width$}Change", "Where"),
                Style::default().fg(theme.muted),
            )));
        }
        for (i, (path, delta)) in changes.iter().enumerate() {
            if offset + i == selected {
                selected_row = Some(items.len());
            }
            let label = field(path)
                .map(|(source, id)| s.config_field_label(source, id))
                .unwrap_or(path);
            let width = usize::from(list.width.saturating_sub(8)).saturating_sub(where_width);
            let remaining = width.saturating_sub(Span::raw(delta).width() + 2);
            let separate = remaining < 9;
            let label = ui::truncate_middle(label, if separate { width } else { remaining });
            let mut line = vec![
                Span::styled(format!("{marker} "), Style::default().fg(color)),
                Span::styled(
                    if where_width > 0 {
                        format!("{:<where_width$}", location(path))
                    } else {
                        String::new()
                    },
                    Style::default().fg(theme.muted),
                ),
                Span::raw(label),
            ];
            if !separate {
                line.push(Span::styled(
                    format!("  {delta}"),
                    Style::default().fg(color),
                ));
            }
            let mut lines = vec![Line::from(line)];
            if separate {
                lines.push(Line::styled(
                    format!("  {delta}"),
                    Style::default().fg(color),
                ));
            }
            items.push(ListItem::new(lines));
        }
    }
    let list_widget = List::new(items)
        .block(ui::focus_panel("Changes", navigation::Mode::Configure))
        .highlight_symbol("› ")
        .highlight_style(Style::default().bg(theme.configure_selection));
    f.render_stateful_widget(
        list_widget,
        list,
        &mut ListState::default().with_selected(selected_row),
    );

    let selected_change = drafts.iter().chain(&saved).nth(selected);
    let mut lines = Vec::new();
    if let Some((path, delta)) = selected_change {
        lines.push(Line::styled(
            location(path),
            Style::default().fg(theme.configure),
        ));
        lines.extend([
            Line::styled(path.clone(), Style::default().add_modifier(Modifier::BOLD)),
            Line::raw(delta.clone()),
            Line::raw(""),
        ]);
    }
    let policy = s.sessions.policy_changes().next().is_some();
    let apply_note = if s.apply_preparing() {
        "Preparation or deployment in progress."
    } else if s.apply_error.is_some() {
        "The last Apply needs attention."
    } else if s.apply_feedback.is_some() && saved.is_empty() {
        "Apply completed. Its report is available."
    } else {
        "Review the managed file changes and confirm."
    };
    let stages = [
        (
            "1  Draft",
            if policy {
                "Open policy drafts for their reviewed save."
            } else {
                "s  Save to .macc/macc.yaml."
            },
        ),
        ("2  Saved", "p  Preview the managed file changes."),
        ("3  Apply to tools…  [a]", apply_note),
    ];
    if inspector.height >= 23 && inspector.width >= 35 {
        let block = panel("Pipeline");
        let inner = block.inner(inspector);
        f.render_widget(block, inspector);
        let stages_area = Layout::default()
            .constraints([
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Length(5),
                Constraint::Min(0),
            ])
            .split(inner);
        for (i, (stage, note)) in stages.iter().enumerate() {
            let active = (i == 0 && !drafts.is_empty())
                || (i == 1 && drafts.is_empty() && !saved.is_empty());
            let border = if active { theme.draft } else { theme.line };
            let content = match i {
                0 => format!("{} unsaved\n{note}", drafts.len()),
                1 => format!(".macc/macc.yaml\n{note}"),
                _ => (*note).into(),
            };
            f.render_widget(
                Paragraph::new(content)
                    .wrap(Wrap { trim: false })
                    .block(panel(stage).border_style(Style::default().fg(border))),
                stages_area[i],
            );
        }
        f.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            stages_area[3],
        );
        return;
    }
    for (stage, note) in stages {
        lines.extend([
            Line::styled(
                stage,
                Style::default()
                    .fg(theme.configure)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::raw(note),
            Line::raw(""),
        ]);
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(panel("Pipeline")),
        inspector,
    );
}
