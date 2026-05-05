use std::borrow::Cow;

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

pub(crate) fn fg_style(color: Color) -> Style {
    Style::default().fg(color)
}

pub(crate) fn fg_span(text: impl Into<Cow<'static, str>>, color: Color) -> Span<'static> {
    Span::styled(text, fg_style(color))
}

pub(crate) fn bold_fg_span(text: impl Into<Cow<'static, str>>, color: Color) -> Span<'static> {
    Span::styled(text, fg_style(color).add_modifier(Modifier::BOLD))
}

pub(crate) fn badge_span(
    text: impl Into<Cow<'static, str>>,
    fg: Color,
    bg: Color,
) -> Span<'static> {
    Span::styled(
        format!(" {} ", text.into()),
        fg_style(fg).bg(bg).add_modifier(Modifier::BOLD),
    )
}
