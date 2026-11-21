use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::state::UiStatusLevel;

#[derive(Copy, Clone)]
pub struct Theme {
    pub background: Color,
    pub foreground: Color,
    pub line: Color,
    pub configure: Color,
    pub draft: Color,
    pub accent: Color,
    pub accent_dim: Color,
    pub muted: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub operate_selection: Color,
    pub configure_selection: Color,
    operate_border: Color,
    configure_border: Color,
}

pub fn theme() -> Theme {
    static TRUECOLOR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    palette(*TRUECOLOR.get_or_init(|| {
        supports_rgb(
            std::env::var("COLORTERM").as_deref().unwrap_or(""),
            std::env::var("TERM").as_deref().unwrap_or(""),
        )
    }))
}

fn supports_rgb(colorterm: &str, term: &str) -> bool {
    ["truecolor", "24bit"]
        .iter()
        .any(|value| colorterm.eq_ignore_ascii_case(value))
        || term
            .split('-')
            .any(|part| matches!(part, "direct" | "truecolor" | "24bit"))
}

fn palette(truecolor: bool) -> Theme {
    if truecolor {
        return Theme {
            background: Color::Rgb(13, 15, 18),
            foreground: Color::Rgb(214, 216, 220),
            line: Color::Rgb(44, 50, 58),
            accent: Color::Rgb(92, 200, 219),
            configure: Color::Rgb(157, 180, 255),
            draft: Color::Rgb(232, 143, 208),
            accent_dim: Color::Rgb(130, 170, 255),
            muted: Color::Rgb(139, 145, 154),
            good: Color::Rgb(95, 207, 128),
            warn: Color::Rgb(230, 180, 80),
            bad: Color::Rgb(240, 113, 120),
            operate_selection: Color::Rgb(28, 54, 64),
            configure_selection: Color::Rgb(35, 45, 74),
            operate_border: Color::Rgb(63, 182, 201),
            configure_border: Color::Rgb(123, 151, 240),
        };
    }
    Theme {
        background: Color::Black,
        foreground: Color::Gray,
        line: Color::DarkGray,
        configure: Color::LightBlue,
        draft: Color::LightMagenta,
        accent: Color::Cyan,
        accent_dim: Color::LightCyan,
        muted: Color::Gray,
        good: Color::Green,
        warn: Color::Yellow,
        bad: Color::Red,
        operate_selection: Color::DarkGray,
        configure_selection: Color::DarkGray,
        operate_border: Color::Cyan,
        configure_border: Color::LightBlue,
    }
}

impl Theme {
    pub(crate) fn mode_accent(self, mode: crate::navigation::Mode) -> Color {
        match mode {
            crate::navigation::Mode::Operate => self.accent,
            crate::navigation::Mode::Configure => self.configure,
        }
    }

    pub(crate) fn selection(self, mode: crate::navigation::Mode) -> Color {
        match mode {
            crate::navigation::Mode::Operate => self.operate_selection,
            crate::navigation::Mode::Configure => self.configure_selection,
        }
    }

    pub fn glyph_color(self, text: &str) -> Option<Color> {
        Some(match text.split_whitespace().next()? {
            "●" | "✓" => self.good,
            "○" | "·" | "—" => self.muted,
            "▲" | "◐" | "?" => self.warn,
            "✕" => self.bad,
            "◆" => self.draft,
            "↻" => self.configure,
            "ℹ" => self.accent,
            _ => return None,
        })
    }
}

pub fn panel<'a>(title: &'a str) -> Block<'a> {
    styled_panel(title, theme().line, Style::default().fg(theme().muted))
}

pub(crate) fn focus_panel(title: &str, mode: crate::navigation::Mode) -> Block<'_> {
    let t = theme();
    let accent = t.mode_accent(mode);
    let border = match mode {
        crate::navigation::Mode::Operate => t.operate_border,
        crate::navigation::Mode::Configure => t.configure_border,
    };
    styled_panel(
        title,
        border,
        Style::default().fg(accent).add_modifier(Modifier::BOLD),
    )
}

fn styled_panel(title: &str, border: Color, title_style: Style) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Span::styled(title, title_style))
}

pub fn wrapped_paragraph<'a>(text: impl Into<String>, title: &'a str) -> Paragraph<'a> {
    Paragraph::new(text.into())
        .block(panel(title))
        .wrap(Wrap { trim: false })
}

pub fn truncate_middle(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    if max_chars < 8 {
        return input.chars().take(max_chars).collect();
    }
    let keep = (max_chars - 3) / 2;
    let start: String = input.chars().take(keep).collect();
    let end: String = input
        .chars()
        .rev()
        .take(keep)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{}...{}", start, end)
}

pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn status_style(level: UiStatusLevel, t: &Theme) -> Style {
    match level {
        UiStatusLevel::Info => Style::default().fg(t.accent_dim),
        UiStatusLevel::Success => Style::default().fg(t.good),
        UiStatusLevel::Warning => Style::default().fg(t.warn),
        UiStatusLevel::Error => Style::default().fg(t.bad).add_modifier(Modifier::BOLD),
    }
}

pub(crate) struct HeaderContext<'a> {
    pub mode: crate::navigation::Mode,
    pub page: &'a str,
    pub project: &'a str,
    pub config_label: &'a str,
    pub errors: usize,
    pub inbox: usize,
    pub draft_count: usize,
    pub coordinator_active: bool,
    pub coordinator_paused: bool,
    pub coordinator_stopped: bool,
    pub action_needed: bool,
    pub next_step: &'a str,
    pub status: Option<(UiStatusLevel, String)>,
    pub width: u16,
    pub trust_strip: Option<String>,
    pub override_strip: Option<String>,
}

pub(crate) fn header_lines(ctx: &HeaderContext<'_>, t: &Theme) -> Vec<Line<'static>> {
    use crate::navigation::Mode;
    let mode_style = |mode| {
        if mode == ctx.mode {
            Style::default()
                .fg(Color::Black)
                .bg(if mode == Mode::Operate {
                    t.accent
                } else {
                    t.configure
                })
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(t.muted)
        }
    };
    let mut row = vec![Span::styled(
        "MACC  ",
        Style::default().add_modifier(Modifier::BOLD),
    )];
    for (mode, name) in [
        (Mode::Operate, " OPERATE "),
        (Mode::Configure, " CONFIGURE "),
    ] {
        if ctx.width >= 68 || mode == ctx.mode {
            row.push(Span::styled(name, mode_style(mode)));
        }
    }
    row.push(Span::styled(" Ctrl+T  ", Style::default().fg(t.muted)));
    row.push(Span::styled(
        ctx.page.to_owned(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    let (run, color) = if ctx.coordinator_paused {
        ("▲ paused", t.warn)
    } else if ctx.coordinator_active {
        ("● running", t.good)
    } else if ctx.coordinator_stopped {
        ("▲ stopped", t.warn)
    } else {
        ("○ idle", t.muted)
    };
    let mut badges = vec![];
    if ctx.width >= 98 {
        badges.push(Span::raw(format!("{}  ", truncate_middle(ctx.project, 18))));
        badges.push(Span::styled(
            format!(
                "config {} {}  ",
                if ctx.config_label == "ok" {
                    "●"
                } else {
                    "▲"
                },
                ctx.config_label
            ),
            Style::default().fg(if ctx.config_label == "ok" {
                t.good
            } else {
                t.warn
            }),
        ));
    }
    if ctx.width >= 68 {
        badges.push(Span::styled(
            format!("coordinator {run}  "),
            Style::default().fg(color),
        ));
    }
    badges.push(Span::styled(
        format!(
            "inbox {}{}",
            if ctx.inbox > 0 { "▲ " } else { "" },
            ctx.inbox
        ),
        Style::default().fg(if ctx.inbox > 0 { t.warn } else { t.muted }),
    ));
    if ctx.draft_count > 0 {
        badges.push(Span::styled(
            format!("  ◆ {} unsaved", ctx.draft_count),
            Style::default().fg(t.draft),
        ));
    }
    // Keep inbox and draft visible first when the project or page name is long.
    let available = usize::from(ctx.width);
    let mut used = row.iter().map(Span::width).sum::<usize>();
    let mut visible = Vec::new();
    for badge in badges.into_iter().rev() {
        if used + badge.width() <= available {
            used += badge.width();
            visible.push(badge);
        }
    }
    row.push(Span::raw(" ".repeat(available.saturating_sub(used))));
    row.extend(visible.into_iter().rev());
    let mut lines = vec![Line::from(row)];
    if let Some((level, msg)) = &ctx.status {
        lines.push(Line::styled(msg.clone(), status_style(*level, t)));
    } else if ctx.errors > 0 {
        lines.push(Line::styled(
            format!("✕ {} project issue(s) · {}", ctx.errors, ctx.next_step),
            Style::default().fg(t.bad),
        ));
    } else if !ctx.next_step.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(
                if ctx.action_needed {
                    "▲ Next  "
                } else {
                    "Next  "
                },
                Style::default()
                    .fg(if ctx.action_needed { t.warn } else { t.accent })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(ctx.next_step.to_owned()),
        ]));
    }
    for (label, info) in [
        ("Trust", &ctx.trust_strip),
        ("Runtime", &ctx.override_strip),
    ] {
        if let Some(msg) = info {
            lines.push(Line::styled(
                format!("▲ {label} · {msg}"),
                Style::default().fg(t.warn),
            ));
        }
    }
    lines
}

/// Reserve the persistent command area beneath every page and modal.
pub(crate) fn command_height(area: ratatui::layout::Rect) -> u16 {
    if area.height < 10 {
        1
    } else {
        3
    }
}
pub(crate) fn content_area(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect {
        height: area.height.saturating_sub(command_height(area)),
        ..area
    }
}

#[cfg(test)]
mod palette_tests {
    use super::*;
    use crate::navigation::Mode;

    #[test]
    fn terminal_capabilities_select_the_design_palette_or_ansi_fallback() {
        assert!(supports_rgb("TrueColor", "xterm-256color"));
        assert!(supports_rgb("", "tmux-direct"));
        assert!(!supports_rgb("", "xterm-256color"));
        let rgb = palette(true);
        assert_eq!(rgb.background, Color::Rgb(13, 15, 18));
        assert_eq!(rgb.mode_accent(Mode::Operate), Color::Rgb(92, 200, 219));
        assert_eq!(rgb.mode_accent(Mode::Configure), Color::Rgb(157, 180, 255));
        let ansi = palette(false);
        for (symbol, rgb_color, ansi_color) in [
            ("✓", Color::Rgb(95, 207, 128), Color::Green),
            ("▲", Color::Rgb(230, 180, 80), Color::Yellow),
            ("✕", Color::Rgb(240, 113, 120), Color::Red),
            ("◆", Color::Rgb(232, 143, 208), Color::LightMagenta),
        ] {
            assert_eq!(rgb.glyph_color(symbol), Some(rgb_color));
            assert_eq!(ansi.glyph_color(symbol), Some(ansi_color));
        }
    }
}
