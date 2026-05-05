use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

use crate::action::Action;
use crate::components::layout::centered_rect;
use crate::components::panel;
use crate::components::style::{bold_fg_span, fg_span};

pub(crate) struct ConfirmDialog {
    pub visible: bool,
    message: String,
    pending_action: Option<Action>,
}

impl ConfirmDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            message: String::new(),
            pending_action: None,
        }
    }

    pub fn show(&mut self, message: String, action: Action) {
        self.visible = true;
        self.message = message;
        self.pending_action = Some(action);
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.message.clear();
        self.pending_action = None;
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let action = self.pending_action.take();
                self.hide();
                Ok(action)
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.hide();
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let width = 40u16.min(area.width.saturating_sub(4));
        let rect = centered_rect(area, width, 5);

        frame.render_widget(Clear, rect);

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                &self.message,
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                bold_fg_span(" y", Color::Green),
                Span::raw("/"),
                fg_span("Enter ", Color::Green),
                Span::raw("confirm   "),
                bold_fg_span("n", Color::Red),
                Span::raw("/"),
                fg_span("Esc ", Color::Red),
                Span::raw("cancel"),
            ]),
        ];

        let paragraph = Paragraph::new(lines)
            .centered()
            .block(panel::bordered_block(
                " Confirm ".to_string(),
                Color::Yellow,
            ));
        frame.render_widget(paragraph, rect);
    }
}
