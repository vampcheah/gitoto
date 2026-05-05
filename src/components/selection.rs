use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::ListState;

pub(crate) enum VerticalMove {
    Next,
    Prev,
}

pub(crate) fn select_first(state: &mut ListState, len: usize) {
    state.select((len > 0).then_some(0));
}

pub(crate) fn preserve_or_first(state: &mut ListState, previous: Option<usize>, len: usize) {
    if len == 0 {
        state.select(None);
    } else {
        state.select(Some(previous.map(|i| i.min(len - 1)).unwrap_or(0)));
    }
}

pub(crate) fn select_next(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let next = state.selected().map(|i| (i + 1).min(len - 1)).unwrap_or(0);
    state.select(Some(next));
}

pub(crate) fn select_prev(state: &mut ListState, len: usize) {
    if len == 0 {
        return;
    }
    let prev = state.selected().map(|i| i.saturating_sub(1)).unwrap_or(0);
    state.select(Some(prev));
}

pub(crate) fn handle_vertical_key(state: &mut ListState, len: usize, key: KeyEvent) -> bool {
    handle_vertical_key_matching(state, len, key, |_| true)
}

pub(crate) fn vertical_move_from_key(key: KeyEvent) -> Option<VerticalMove> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => Some(VerticalMove::Next),
        KeyCode::Char('k') | KeyCode::Up => Some(VerticalMove::Prev),
        _ => None,
    }
}

pub(crate) fn handle_vertical_key_matching(
    state: &mut ListState,
    len: usize,
    key: KeyEvent,
    is_selectable: impl Fn(usize) -> bool,
) -> bool {
    match vertical_move_from_key(key) {
        Some(VerticalMove::Next) => {
            select_next_matching(state, len, is_selectable);
            true
        }
        Some(VerticalMove::Prev) => {
            select_prev_matching(state, len, is_selectable);
            true
        }
        None => false,
    }
}

pub(crate) fn select_next_matching(
    state: &mut ListState,
    len: usize,
    is_selectable: impl Fn(usize) -> bool,
) {
    if len == 0 {
        return;
    }
    let start = state.selected().unwrap_or(0);
    let next = (start + 1..len)
        .find(|idx| is_selectable(*idx))
        .unwrap_or(start);
    state.select(Some(next));
}

pub(crate) fn select_prev_matching(
    state: &mut ListState,
    len: usize,
    is_selectable: impl Fn(usize) -> bool,
) {
    if len == 0 {
        return;
    }
    let start = state.selected().unwrap_or(0);
    let prev = (0..start)
        .rev()
        .find(|idx| is_selectable(*idx))
        .unwrap_or(start);
    state.select(Some(prev));
}

pub(crate) fn clicked_list_index(
    area: Rect,
    column: u16,
    row: u16,
    offset: usize,
    len: usize,
) -> Option<usize> {
    let content_y = area.y + 1;
    if column < area.x || column >= area.x + area.width || row < content_y {
        return None;
    }
    let idx = (row - content_y) as usize + offset;
    (idx < len).then_some(idx)
}
