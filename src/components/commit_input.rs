use color_eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::action::Action;
use crate::components::text_input::SingleLineInput;
use crate::repo_id::RepoId;

pub(crate) struct CommitInput {
    pub visible: bool,
    repo_id: Option<RepoId>,
    repo_name: String,
    /// Number of files marked for a partial commit; 0 means commit all.
    marked_count: usize,
    input: SingleLineInput,
}

impl CommitInput {
    pub fn new() -> Self {
        Self {
            visible: false,
            repo_id: None,
            repo_name: String::new(),
            marked_count: 0,
            input: SingleLineInput::new(),
        }
    }

    pub fn show(&mut self, repo_id: RepoId, repo_name: String, marked_count: usize) {
        self.visible = true;
        self.repo_id = Some(repo_id);
        self.repo_name = repo_name;
        self.marked_count = marked_count;
        self.input.clear();
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.repo_id = None;
        self.repo_name.clear();
        self.input.clear();
    }

    pub fn repo_id(&self) -> Option<RepoId> {
        self.repo_id.clone()
    }

    pub fn message(&self) -> &str {
        self.input.value()
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        self.input
            .handle_key_event(key, Action::CancelCommit, Action::ConfirmCommit)
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let title = if self.marked_count > 0 {
            format!(
                " Message (Enter: commit {} marked file(s), Esc: cancel) ",
                self.marked_count
            )
        } else {
            " Message (Enter: git add . && commit, Esc: cancel) ".to_string()
        };
        self.input
            .draw(frame, area, format!(" Commit {}: ", self.repo_name), &title);
    }
}
