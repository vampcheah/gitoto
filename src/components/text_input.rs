use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::Color,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::action::Action;
use crate::components::panel;
use crate::components::style::{bold_fg_span, fg_style};

pub(crate) struct SingleLineInput {
    value: String,
    cursor: usize,
}

impl SingleLineInput {
    pub(crate) fn new() -> Self {
        Self {
            value: String::new(),
            cursor: 0,
        }
    }

    pub(crate) fn set(&mut self, value: String) {
        self.cursor = value.len();
        self.value = value;
    }

    pub(crate) fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    pub(crate) fn handle_key_event(
        &mut self,
        key: KeyEvent,
        cancel: Action,
        confirm: Action,
    ) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Esc => Ok(Some(cancel)),
            KeyCode::Enter => Ok(Some(confirm)),
            _ => {
                self.handle_edit_key_event(key);
                Ok(None)
            }
        }
    }

    pub(crate) fn handle_edit_key_event(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Backspace => {
                if self.cursor > 0
                    && let Some((idx, _)) = self.value[..self.cursor].char_indices().last()
                {
                    self.value.drain(idx..self.cursor);
                    self.cursor = idx;
                    return true;
                }
                false
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len()
                    && let Some(ch) = self.value[self.cursor..].chars().next()
                {
                    let end = self.cursor + ch.len_utf8();
                    self.value.drain(self.cursor..end);
                    return true;
                }
                false
            }
            KeyCode::Left => {
                if self.cursor > 0
                    && let Some((idx, _)) = self.value[..self.cursor].char_indices().last()
                {
                    self.cursor = idx;
                }
                false
            }
            KeyCode::Right => {
                if self.cursor < self.value.len()
                    && let Some(ch) = self.value[self.cursor..].chars().next()
                {
                    self.cursor += ch.len_utf8();
                }
                false
            }
            KeyCode::Home | KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor = 0;
                false
            }
            KeyCode::End | KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.cursor = self.value.len();
                false
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                let changed = self.cursor > 0;
                self.value.drain(..self.cursor);
                self.cursor = 0;
                changed
            }
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn draw(&self, frame: &mut Frame, area: Rect, label: String, title: &str) {
        self.draw_with_extra(frame, area, label, title, Vec::new());
    }

    pub(crate) fn draw_with_extra(
        &self,
        frame: &mut Frame,
        area: Rect,
        label: String,
        title: &str,
        extra_spans: Vec<Span<'static>>,
    ) {
        let input_area = Rect::new(area.x, area.height.saturating_sub(3), area.width, 3);
        frame.render_widget(Clear, input_area);

        let before_cursor = &self.value[..self.cursor];
        let cursor_char = self.value[self.cursor..]
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".to_string());
        let after_cursor = self.value[self.cursor..]
            .chars()
            .next()
            .map(|c| &self.value[self.cursor + c.len_utf8()..])
            .unwrap_or("");

        let mut spans = vec![
            bold_fg_span(label, Color::Cyan),
            Span::raw(before_cursor.to_string()),
            Span::styled(cursor_char, fg_style(Color::Black).bg(Color::White)),
            Span::raw(after_cursor.to_string()),
        ];
        spans.extend(extra_spans);

        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .block(panel::bordered_block(title.to_string(), Color::Cyan)),
            input_area,
        );
    }
}
