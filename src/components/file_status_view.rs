use ratatui::{
    style::Color,
    text::{Line, Span},
    widgets::ListItem,
};

use crate::components::style::{bold_fg_span, fg_span};
use crate::git::status::{FileEntry, SubmoduleState};

pub(crate) fn status_color(label: &str) -> Color {
    match label {
        "M" => Color::Yellow,
        "A" => Color::Green,
        "D" => Color::Red,
        "R" => Color::Blue,
        "C" => Color::LightRed,
        _ => Color::DarkGray,
    }
}

pub(crate) fn status_span(label: &str) -> Span<'static> {
    bold_fg_span(format!(" {} ", label), status_color(label))
}

pub(crate) fn commit_file_item(status: &str, path: &str) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        status_span(status),
        fg_span(path.to_string(), Color::White),
    ]))
}

pub(crate) fn worktree_file_item(entry: &FileEntry) -> ListItem<'static> {
    let mut spans = vec![status_span(entry.status.label())];
    if entry.is_submodule {
        spans.push(fg_span(
            submodule_label(entry.submodule_state),
            Color::LightMagenta,
        ));
    }
    let path_color = if entry.is_submodule {
        Color::LightMagenta
    } else {
        Color::White
    };
    spans.push(fg_span(
        entry.path.to_string_lossy().to_string(),
        path_color,
    ));
    ListItem::new(Line::from(spans))
}

fn submodule_label(state: Option<SubmoduleState>) -> &'static str {
    match state {
        Some(SubmoduleState::Modified) => "[sub: +commit] ",
        Some(SubmoduleState::Uninitialized) => "[sub: -uninit] ",
        Some(SubmoduleState::Dirty) => "[sub: ~dirty] ",
        None => "[submodule] ",
    }
}
