use color_eyre::Result;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Color,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::time::Instant;

use crate::app::FocusPanel;
use crate::components::Component;
use crate::components::style::{badge_span, fg_span, fg_style};

pub(crate) struct StatusBar {
    pub focus: FocusPanel,
    pub error: Option<(String, Instant)>,
    pub success: Option<(String, Instant)>,
    pub fast_mode: bool,
    pub focused_repo: Option<String>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            focus: FocusPanel::Repos,
            error: None,
            success: None,
            fast_mode: false,
            focused_repo: None,
        }
    }
}

impl Component for StatusBar {
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let version_text = format!("v{} ", env!("CARGO_PKG_VERSION"));
        let version_len = version_text.len() as u16;
        let chunks =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(version_len)]).split(area);
        let content_area = chunks[0];
        let version_area = chunks[1];

        // Always render version (right-aligned, dimmed)
        let version = Paragraph::new(version_text)
            .style(fg_style(Color::DarkGray))
            .right_aligned();
        frame.render_widget(version, version_area);

        // Show error for 5 seconds, then clear
        if let Some((ref msg, when)) = self.error {
            if when.elapsed().as_secs() < 5 {
                let error_bar = Paragraph::new(Line::from(vec![
                    badge("ERROR", Color::White, Color::Red),
                    fg_span(format!(" {}", msg), Color::Red),
                ]));
                frame.render_widget(error_bar, content_area);
                return Ok(());
            } else {
                self.error = None;
            }
        }

        // Show success for 3 seconds
        if let Some((ref msg, when)) = self.success {
            if when.elapsed().as_secs() < 3 {
                let success_bar = Paragraph::new(Line::from(vec![
                    badge("OK", Color::Black, Color::Green),
                    fg_span(format!(" {}", msg), Color::Green),
                ]));
                frame.render_widget(success_bar, content_area);
                return Ok(());
            } else {
                self.success = None;
            }
        }

        let focus_label = match self.focus {
            FocusPanel::Repos => "Repos",
            FocusPanel::Changes => "Changes",
            FocusPanel::Graph => "Graph",
        };
        let mut spans = vec![
            badge(focus_label, Color::Black, Color::Cyan),
            Span::raw("  "),
            key_span("h"),
            Span::raw(" help"),
            Span::raw("  "),
            key_span("o"),
            Span::raw(" log"),
        ];
        if let Some(repo) = &self.focused_repo {
            spans.extend([
                Span::raw("  "),
                badge(format!("FOCUS {repo}"), Color::Black, Color::LightMagenta),
                Span::raw(" Esc unlock"),
            ]);
        }
        if self.fast_mode {
            spans.extend([Span::raw("  "), badge("FAST", Color::Black, Color::Yellow)]);
        }

        let bar = Paragraph::new(Line::from(spans)).style(fg_style(Color::Gray));
        frame.render_widget(bar, content_area);
        Ok(())
    }
}

fn key_span(key: &str) -> Span<'_> {
    badge_span(key.to_string(), Color::Black, Color::DarkGray)
}

fn badge(text: impl Into<std::borrow::Cow<'static, str>>, fg: Color, bg: Color) -> Span<'static> {
    badge_span(text, fg, bg)
}
