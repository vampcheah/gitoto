use crossterm::event::KeyEvent;

use crate::components::selection::{self, VerticalMove};

pub(crate) fn reset(scroll: &mut u16) {
    *scroll = 0;
}

pub(crate) fn up(scroll: &mut u16) {
    *scroll = scroll.saturating_sub(1);
}

pub(crate) fn down(scroll: &mut u16) {
    *scroll = scroll.saturating_add(1);
}

pub(crate) fn handle_vertical_key(scroll: &mut u16, key: KeyEvent) -> bool {
    match selection::vertical_move_from_key(key) {
        Some(VerticalMove::Next) => {
            down(scroll);
            true
        }
        Some(VerticalMove::Prev) => {
            up(scroll);
            true
        }
        None => false,
    }
}
