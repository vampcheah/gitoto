use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::action::Action;
use crate::components::Component;

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

        let width = 72u16.min(area.width.saturating_sub(4)).max(20);
        let height = 9u16.min(area.height.saturating_sub(2)).max(5);
        let [vert] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(area);
        let [rect] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(vert);

        frame.render_widget(Clear, rect);

        let lines = vec![
            Line::from(self.message.as_str()),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " Enter ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" / "),
                Span::styled(
                    " Esc ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" close"),
            ]),
        ];

        let block = Block::default()
            .title(" Notice ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow));
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, rect);
        Ok(())
    }
}
