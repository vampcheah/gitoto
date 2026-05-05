use ratatui::{
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem},
};

pub(crate) fn bordered_block(title: String, border_color: Color) -> Block<'static> {
    plain_block(border_color).title(title)
}

pub(crate) fn plain_block(border_color: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
}

pub(crate) fn highlighted_list<'a>(items: Vec<ListItem<'a>>, block: Block<'a>) -> List<'a> {
    List::new(items).block(block).highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
}
