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
    input: SingleLineInput,
}

impl CommitInput {
    pub fn new() -> Self {
        Self {
            visible: false,
            repo_id: None,
            repo_name: String::new(),
            input: SingleLineInput::new(),
        }
    }

    pub fn show(&mut self, repo_id: RepoId, repo_name: String) {
        self.visible = true;
        self.repo_id = Some(repo_id);
        self.repo_name = repo_name;
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

        self.input.draw(
            frame,
            area,
            format!(" Commit {}: ", self.repo_name),
            " Message (Enter: git add . && commit, Esc: cancel) ",
        );
    }
}
