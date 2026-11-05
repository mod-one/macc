use crossterm::event::KeyCode;
use ratatui::{
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::path::PathBuf;

#[derive(Default)]
pub struct Editor {
    pub files: Vec<String>,
    root: PathBuf,
    selected: usize,
    grabbed: bool,
    input: Option<String>,
    message: String,
    confirm_reset: bool,
    message_scroll: u16,
    report_max_scroll: std::cell::Cell<u16>,
}

pub enum Outcome {
    Continue,
    Save,
    Cancel,
    Reset,
}

impl Editor {
    pub fn new(root: PathBuf, files: Vec<String>) -> Self {
        let message = match macc_core::prd_queue::load(&root) {
            Ok(progress) if !progress.entries.is_empty() => format!(
                "Saved progress: {} | {}/{} completed. {}",
                progress.status,
                progress.next,
                progress.entries.len(),
                progress.reason.unwrap_or_default()
            ),
            Ok(_) => "No saved queue progress.".into(),
            Err(error) => error.to_string(),
        };
        Self {
            root,
            files,
            message,
            ..Self::default()
        }
    }

    pub(crate) fn command_bindings(&self) -> Vec<(&'static str, &'static str)> {
        if self.confirm_reset {
            return vec![("y", "Confirm reset"), ("Esc", "Cancel reset")];
        }
        if self.input.is_some() {
            return vec![
                ("Enter", "Import"),
                ("Backspace", "Delete character"),
                ("Esc", "Cancel input"),
            ];
        }
        vec![
            ("↑↓", "Select / move PRD"),
            ("Space", "Grab / release"),
            ("a", "Import PRD…"),
            ("Delete", "Remove PRD"),
            ("R", "Reset progress…"),
            ("PgUp/PgDn", "Scroll import report"),
            ("Enter", "Save queue"),
            ("Esc", "Cancel"),
        ]
    }

    pub fn key(&mut self, key: KeyCode) -> Outcome {
        if self.confirm_reset {
            match key {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirm_reset = false;
                    return Outcome::Reset;
                }
                KeyCode::Esc | KeyCode::Char('n') => self.confirm_reset = false,
                _ => {}
            }
            return Outcome::Continue;
        }
        if let Some(input) = &mut self.input {
            match key {
                KeyCode::Esc => self.input = None,
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => input.push(c),
                KeyCode::Enter => {
                    match macc_core::prd_queue::import(&self.root, input.trim(), &self.files) {
                        Ok(report) => {
                            self.message = report.message();
                            self.files = report.files;
                            self.input = None;
                            self.message_scroll = 0;
                        }
                        Err(e) => self.message = e.to_string(),
                    }
                }
                _ => {}
            }
            return Outcome::Continue;
        }
        match key {
            KeyCode::PageDown => {
                self.message_scroll = self
                    .message_scroll
                    .saturating_add(3)
                    .min(self.report_max_scroll.get())
            }
            KeyCode::PageUp => self.message_scroll = self.message_scroll.saturating_sub(3),
            KeyCode::Home => self.message_scroll = 0,
            KeyCode::Char('R') => self.confirm_reset = true,
            KeyCode::Char('a') => {
                self.input = Some(String::new());
                self.message.clear();
            }
            KeyCode::Char(' ') => self.grabbed = !self.grabbed,
            KeyCode::Up | KeyCode::Char('k') if self.selected > 0 => {
                if self.grabbed {
                    self.files.swap(self.selected, self.selected - 1);
                }
                self.selected -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') if self.selected + 1 < self.files.len() => {
                if self.grabbed {
                    self.files.swap(self.selected, self.selected + 1);
                }
                self.selected += 1;
            }
            KeyCode::Delete | KeyCode::Char('d') if !self.files.is_empty() => {
                self.files.remove(self.selected);
                self.selected = self.selected.min(self.files.len().saturating_sub(1));
            }
            KeyCode::Enter | KeyCode::Char('s') => {
                match macc_core::prd_queue::validate(&self.root, &self.files) {
                    Ok(_) => return Outcome::Save,
                    Err(e) => self.message = e.to_string(),
                }
            }
            KeyCode::Esc => return Outcome::Cancel,
            _ => {}
        }
        Outcome::Continue
    }

    pub fn report_reset(&mut self, result: macc_core::Result<()>) {
        self.message = match result {
            Ok(()) => {
                "Saved progress reset now. Delivered tasks unchanged; draft files not saved yet."
                    .into()
            }
            Err(error) => error.to_string(),
        };
    }

    pub fn draw(&self, frame: &mut Frame) {
        self.draw_in(frame, frame.size());
    }
    pub(crate) fn draw_in(&self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let mut lines = vec![
            Line::from("PRDs run in this order. A blocked/incomplete PRD stops the queue."),
            Line::from("a Add file/directory | Space Grab/release | Up/Down Move | d Remove | Enter Accept | Esc Cancel"),
            Line::from("Directories: immediate *.json files, sorted by name. Invalid files are skipped with warnings."),
            Line::from("R Reset saved progress (confirmation required; does not undo delivered tasks)"),
            Line::from(""),
        ];
        let report_height = if self.message.is_empty() {
            0
        } else {
            (self.message.lines().count() as u16 + 2)
                .min(area.height / 2)
                .max(3)
        };
        let sections = ratatui::layout::Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                ratatui::layout::Constraint::Min(0),
                ratatui::layout::Constraint::Length(report_height),
            ])
            .split(area);
        let visible = sections[0].height.saturating_sub(9).max(1) as usize;
        let start = self.selected.saturating_sub(visible - 1);
        for (index, file) in self.files.iter().enumerate().skip(start).take(visible) {
            let line = Line::from(format!(
                "{} {:>3}. {}{}",
                if index == self.selected { ">" } else { " " },
                index + 1,
                file,
                if index == self.selected && self.grabbed {
                    " [moving]"
                } else {
                    ""
                }
            ));
            lines.push(if index == self.selected {
                line.style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                line
            });
        }
        if self.files.is_empty() {
            lines.push(Line::from(
                "Queue empty: legacy prd_file remains in use. Press a to add a PRD.",
            ));
        }
        if let Some(input) = &self.input {
            lines.push(Line::from(format!(
                "Path (relative to project or absolute): {input}_"
            )));
        }
        if self.confirm_reset {
            lines.push(
                Line::from(
                    "Reset saved progress NOW? Draft files stay unsaved. y Confirm | n/Esc Cancel",
                )
                .style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            );
        }
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Coordinator PRD queue"),
            ),
            sections[0],
        );
        if report_height > 0 {
            let paragraph = Paragraph::new(self.message.as_str())
                .style(Style::default().fg(if self.message.contains("Warning:") {
                    Color::Yellow
                } else {
                    Color::Cyan
                }))
                .wrap(Wrap { trim: false });
            let max_scroll = paragraph
                .line_count(sections[1].width.saturating_sub(2))
                .saturating_sub(sections[1].height.saturating_sub(2) as usize)
                .min(u16::MAX as usize) as u16;
            self.report_max_scroll.set(max_scroll);
            frame.render_widget(
                paragraph
                    .scroll((self.message_scroll.min(max_scroll), 0))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Import / status — PgUp/PgDn scroll; Home top"),
                    ),
                sections[1],
            );
        }
    }
}

pub fn edit(root: PathBuf, files: Vec<String>) -> anyhow::Result<Option<Vec<String>>> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Interactive editor requires a terminal. Use prds add/move/remove instead.");
    }
    let mut guard = super::TerminalGuard::new()?;
    let mut editor = Editor::new(root, files);
    loop {
        guard.terminal.draw(|frame| editor.draw(frame))?;
        if let crossterm::event::Event::Key(key) = crossterm::event::read()? {
            if key.kind != crossterm::event::KeyEventKind::Press {
                continue;
            }
            match editor.key(key.code) {
                Outcome::Continue => {}
                Outcome::Save => return Ok(Some(editor.files)),
                Outcome::Cancel => return Ok(None),
                Outcome::Reset => editor.report_reset(macc_core::prd_queue::save(
                    &editor.root,
                    &macc_core::prd_queue::Progress::default(),
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_folder_import_shows_scrollable_warnings_and_keeps_valid_files() {
        use ratatui::{backend::TestBackend, Terminal};
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("empty.json"), r#"{"tasks":[]}"#).unwrap();
        std::fs::write(root.path().join("valid.json"), r#"{"tasks":[{"id":"A"}]}"#).unwrap();
        let mut editor = Editor::new(root.path().into(), vec![]);
        editor.key(KeyCode::Char('a'));
        editor.key(KeyCode::Char('.'));
        editor.key(KeyCode::Enter);
        assert_eq!(editor.files, ["valid.json"]);
        assert!(editor.input.is_none());
        assert!(editor.message.contains("Added 1 PRD(s)"));
        assert!(editor.message.contains("empty.json"));
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|frame| editor.draw(frame)).unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(rendered.contains("Warning:"));
        assert!(rendered.contains("PgUp/PgDn"));
        editor
            .message
            .push_str(&"\nAdditional rejection detail".repeat(20));
        terminal.draw(|frame| editor.draw(frame)).unwrap();
        editor.key(KeyCode::PageDown);
        assert!(editor.message_scroll > 0);
        editor.key(KeyCode::PageUp);
        assert_eq!(editor.message_scroll, 0);
        assert!(matches!(editor.key(KeyCode::Enter), Outcome::Save));
    }

    #[test]
    fn grab_moves_files_and_cancel_leaves_original_unchanged() {
        let original = vec!["a.json".into(), "b.json".into()];
        let mut editor = Editor::new(PathBuf::new(), original.clone());
        editor.key(KeyCode::Char(' '));
        editor.key(KeyCode::Down);
        assert_eq!(editor.files, vec!["b.json", "a.json"]);
        editor.key(KeyCode::Down);
        assert_eq!(editor.selected, 1);
        assert!(matches!(editor.key(KeyCode::Esc), Outcome::Cancel));
        assert_eq!(original, vec!["a.json", "b.json"]);
    }

    #[test]
    fn deleting_last_entry_and_empty_navigation_are_safe() {
        let mut editor = Editor::new(PathBuf::new(), vec!["a.json".into()]);
        editor.key(KeyCode::Delete);
        editor.key(KeyCode::Up);
        editor.key(KeyCode::Down);
        editor.key(KeyCode::Delete);
        assert!(editor.files.is_empty());
        assert!(matches!(editor.key(KeyCode::Enter), Outcome::Save));
    }

    #[test]
    fn progress_reset_requires_separate_confirmation() {
        let mut editor = Editor::default();
        assert!(matches!(editor.key(KeyCode::Char('R')), Outcome::Continue));
        assert!(matches!(editor.key(KeyCode::Esc), Outcome::Continue));
        assert!(!editor.confirm_reset);
        editor.key(KeyCode::Char('R'));
        assert!(matches!(editor.key(KeyCode::Char('y')), Outcome::Reset));
    }
}
