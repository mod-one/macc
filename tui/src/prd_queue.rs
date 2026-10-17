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
                        Ok(files) => {
                            self.files = files;
                            self.input = None;
                            self.message = "Imported. Use Space and arrows to reorder.".into();
                        }
                        Err(e) => self.message = e.to_string(),
                    }
                }
                _ => {}
            }
            return Outcome::Continue;
        }
        match key {
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
        let mut lines = vec![
            Line::from("PRDs run in this order. A blocked/incomplete PRD stops the queue."),
            Line::from("a Add file/directory | Space Grab/release | Up/Down Move | d Remove | Enter Accept | Esc Cancel"),
            Line::from("Directories: immediate *.json files, sorted by name; no live directory expansion."),
            Line::from("R Reset saved progress (confirmation required; does not undo delivered tasks)"),
            Line::from(""),
        ];
        let visible = frame.size().height.saturating_sub(10).max(1) as usize;
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
        lines.push(Line::from(self.message.clone()));
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
            frame.size(),
        );
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
