use ratatui::{layout::Rect, widgets::ListState};

pub(super) struct CommitDetail {
    pub(super) oid: String,
    pub(super) message: String,
    pub(super) files: Vec<(String, String)>,
    pub(super) file_state: ListState,
    pub(super) diff_content: Option<String>,
    pub(super) diff_scroll: u16,
    pub(super) msg_scroll: u16,
    /// Rendered rect for the commit message block (set during draw).
    pub(super) msg_area: Rect,
    /// Rendered rect for the file list block (set during draw).
    pub(super) file_list_area: Rect,
}
