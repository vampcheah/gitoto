use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::components::panel;
use crate::components::style::fg_style;

pub(crate) fn render_diff(
    frame: &mut Frame,
    area: Rect,
    title: String,
    content: &str,
    scroll: u16,
) {
    let paragraph = Paragraph::new(styled_diff_lines(content))
        .block(panel::bordered_block(title, Color::Cyan))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(paragraph, area);
}

pub(crate) fn styled_diff_lines(content: &str) -> Vec<Line<'_>> {
    content
        .lines()
        .map(|line| Line::from(Span::styled(line, diff_line_style(line))))
        .collect()
}

fn diff_line_style(line: &str) -> Style {
    if line.starts_with('+') && !line.starts_with("+++") {
        fg_style(Color::Green)
    } else if line.starts_with('-') && !line.starts_with("---") {
        fg_style(Color::Red)
    } else if line.starts_with("@@") {
        fg_style(Color::Cyan)
    } else if line.starts_with("diff ") || line.starts_with("index ") {
        fg_style(Color::DarkGray)
    } else {
        fg_style(Color::White)
    }
}
