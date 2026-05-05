use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{Frame, layout::Rect, style::Color};
use std::path::PathBuf;

use crate::action::Action;
use crate::components::style::fg_span;
use crate::components::text_input::SingleLineInput;

pub(crate) struct PathInput {
    pub visible: bool,
    input: SingleLineInput,
    completions: Vec<String>,
    completion_index: Option<usize>,
}

impl PathInput {
    pub fn new() -> Self {
        Self {
            visible: false,
            input: SingleLineInput::new(),
            completions: Vec::new(),
            completion_index: None,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.reset();
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.reset();
    }

    fn reset(&mut self) {
        self.input.clear();
        self.clear_completions();
    }

    fn clear_completions(&mut self) {
        self.completions.clear();
        self.completion_index = None;
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Esc => {
                self.hide();
                Ok(None)
            }
            KeyCode::Enter => {
                if self.input.is_empty() {
                    self.hide();
                    return Ok(None);
                }
                let path = expand_tilde(self.input.value());
                self.hide();
                Ok(Some(Action::AddRepo(path)))
            }
            KeyCode::Tab => {
                self.complete_path();
                Ok(None)
            }
            _ => {
                if self.input.handle_edit_key_event(key) {
                    self.clear_completions();
                }
                Ok(None)
            }
        }
    }

    fn complete_path(&mut self) {
        if !self.completions.is_empty() {
            let next = self
                .completion_index
                .map(|i| (i + 1) % self.completions.len())
                .unwrap_or(0);
            self.completion_index = Some(next);
            self.input.set(self.completions[next].clone());
            return;
        }

        let input = self.input.value();
        let expanded = expand_tilde(input);
        let (dir, prefix) = if expanded.is_dir() && input.ends_with('/') {
            (expanded.clone(), String::new())
        } else {
            let parent = expanded
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            let prefix = expanded
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            (parent, prefix)
        };

        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };

        let mut matches: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') && !prefix.starts_with('.') {
                    return None;
                }
                name.starts_with(&prefix)
                    .then(|| completed_path(input, &prefix, &name))
            })
            .collect();

        matches.sort();

        match matches.len() {
            0 => {}
            1 => self.input.set(matches[0].clone()),
            _ => {
                self.completions = matches;
                self.completion_index = Some(0);
                self.input.set(self.completions[0].clone());
            }
        }
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let extra = self
            .completion_index
            .map(|idx| {
                vec![fg_span(
                    format!("  ({}/{})", idx + 1, self.completions.len()),
                    Color::DarkGray,
                )]
            })
            .unwrap_or_default();
        self.input.draw_with_extra(
            frame,
            area,
            " Add repo: ".to_string(),
            " Path (Tab: complete, Enter: add, Esc: cancel) ",
            extra,
        );
    }
}

fn completed_path(input: &str, prefix: &str, name: &str) -> String {
    if input.ends_with('/') || prefix.is_empty() {
        format!("{}{}/", input, name)
    } else {
        let base = input.rsplit_once('/').map(|(base, _)| base).unwrap_or("");
        if base.is_empty() {
            format!("{}/", name)
        } else {
            format!("{}/{}/", base, name)
        }
    }
}

fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix('~')
        && let Some(home) = dirs::home_dir()
    {
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        return home.join(rest);
    }
    PathBuf::from(input)
}
