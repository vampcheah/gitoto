use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::action::Action;
use crate::repo_id::RepoId;

pub(crate) struct GitHubRepoInput {
    pub visible: bool,
    repo_id: Option<RepoId>,
    private: bool,
    local_name: String,
    input: String,
    cursor: usize,
}

impl GitHubRepoInput {
    pub fn new() -> Self {
        Self {
            visible: false,
            repo_id: None,
            private: true,
            local_name: String::new(),
            input: String::new(),
            cursor: 0,
        }
    }

    pub fn show(&mut self, repo_id: RepoId, private: bool, local_name: String) {
        self.visible = true;
        self.repo_id = Some(repo_id);
        self.private = private;
        self.cursor = local_name.len();
        self.input = local_name.clone();
        self.local_name = local_name;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.repo_id = None;
        self.private = true;
        self.local_name.clear();
        self.input.clear();
        self.cursor = 0;
    }

    pub fn repo_id(&self) -> Option<RepoId> {
        self.repo_id.clone()
    }

    pub fn private(&self) -> bool {
        self.private
    }

    pub fn name(&self) -> &str {
        &self.input
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Esc => Ok(Some(Action::CancelCreateGitHubRepo)),
            KeyCode::Enter => Ok(Some(Action::ConfirmCreateGitHubRepo)),
            KeyCode::Backspace => {
                if self.cursor > 0
                    && let Some((idx, _)) = self.input[..self.cursor].char_indices().last()
                {
                    self.input.drain(idx..self.cursor);
                    self.cursor = idx;
                }
                Ok(Some(Action::UpdateGitHubRepoName(self.input.clone())))
            }
            KeyCode::Delete => {
                if self.cursor < self.input.len()
                    && let Some(ch) = self.input[self.cursor..].chars().next()
                {
                    let end = self.cursor + ch.len_utf8();
                    self.input.drain(self.cursor..end);
                }
                Ok(Some(Action::UpdateGitHubRepoName(self.input.clone())))
            }
            KeyCode::Left => {
                if self.cursor > 0
                    && let Some((idx, _)) = self.input[..self.cursor].char_indices().last()
                {
                    self.cursor = idx;
                }
                Ok(None)
            }
            KeyCode::Right => {
                if self.cursor < self.input.len()
                    && let Some(ch) = self.input[self.cursor..].chars().next()
                {
                    self.cursor += ch.len_utf8();
                }
                Ok(None)
            }
            KeyCode::Home | KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor = 0;
                Ok(None)
            }
            KeyCode::End | KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor = self.input.len();
                Ok(None)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.drain(..self.cursor);
                self.cursor = 0;
                Ok(Some(Action::UpdateGitHubRepoName(self.input.clone())))
            }
            KeyCode::Char(c) => {
                self.input.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                Ok(Some(Action::UpdateGitHubRepoName(self.input.clone())))
            }
            _ => Ok(None),
        }
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let input_area = Rect::new(area.x, area.height.saturating_sub(3), area.width, 3);
        frame.render_widget(Clear, input_area);

        let before_cursor = &self.input[..self.cursor];
        let cursor_char = self.input[self.cursor..]
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".to_string());
        let after_cursor = self.input[self.cursor..]
            .chars()
            .next()
            .map(|c| &self.input[self.cursor + c.len_utf8()..])
            .unwrap_or("");

        let visibility = if self.private { "private" } else { "public" };
        let spans = vec![
            Span::styled(
                format!(" GitHub {visibility} repo: "),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(before_cursor.to_string()),
            Span::styled(
                cursor_char,
                Style::default().bg(Color::White).fg(Color::Black),
            ),
            Span::raw(after_cursor.to_string()),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Repository name (Enter: create, Esc: cancel) ");

        let paragraph = Paragraph::new(Line::from(spans)).block(block);
        frame.render_widget(paragraph, input_area);
    }
}
