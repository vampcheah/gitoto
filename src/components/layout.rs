use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub(crate) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(crate) fn modal_rect(
    area: Rect,
    max_width: u16,
    min_width: u16,
    max_height: u16,
    min_height: u16,
) -> Rect {
    let width = max_width.min(area.width.saturating_sub(4)).max(min_width);
    let height = max_height
        .min(area.height.saturating_sub(2))
        .max(min_height);
    centered_rect(area, width, height)
}

pub(crate) fn split_with_status_bar(area: Rect) -> (Rect, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    (chunks[0], chunks[1])
}

pub(crate) fn split_three_vertical_by_fraction(
    area: Rect,
    first_end: f64,
    second_end: f64,
) -> (Rect, Rect, Rect) {
    let height = area.height as f64;
    let first = ((first_end * height).round() as u16).max(3);
    let second = (((second_end - first_end) * height).round() as u16).max(3);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(first),
            Constraint::Length(second),
            Constraint::Min(3),
        ])
        .split(area);
    (chunks[0], chunks[1], chunks[2])
}

pub(crate) fn split_oriented(
    area: Rect,
    vertical: bool,
    constraints: impl IntoIterator<Item = Constraint>,
) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(if vertical {
            Direction::Vertical
        } else {
            Direction::Horizontal
        })
        .constraints(constraints)
        .split(area)
}
