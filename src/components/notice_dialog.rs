use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::Color,
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::action::Action;
use crate::components::Component;
use crate::components::layout::modal_rect;
use crate::components::panel;
use crate::components::style::badge_span;

pub(crate) struct NoticeDialog {
    pub visible: bool,
    message: String,
}

impl NoticeDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            message: String::new(),
        }
    }

    pub fn show(&mut self, message: String) {
        self.visible = true;
        self.message = message;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.message.clear();
    }
}

impl Component for NoticeDialog {
    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                self.hide();
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if !self.visible {
            return Ok(());
        }

        let rect = modal_rect(area, 72, 20, 9, 5);

        frame.render_widget(Clear, rect);

        let lines = vec![
            Line::from(self.message.as_str()),
            Line::from(""),
            Line::from(vec![
                badge_span("Enter", Color::Black, Color::Cyan),
                Span::raw(" / "),
                badge_span("Esc", Color::Black, Color::DarkGray),
                Span::raw(" close"),
            ]),
        ];

        let paragraph = Paragraph::new(lines)
            .block(panel::bordered_block(" Notice ".to_string(), Color::Yellow))
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, rect);
        Ok(())
    }
}
