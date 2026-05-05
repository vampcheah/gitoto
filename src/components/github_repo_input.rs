use color_eyre::Result;
use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use crate::action::Action;
use crate::components::text_input::SingleLineInput;
use crate::repo_id::RepoId;

pub(crate) struct GitHubRepoInput {
    pub visible: bool,
    repo_id: Option<RepoId>,
    private: bool,
    local_name: String,
    input: SingleLineInput,
}

impl GitHubRepoInput {
    pub fn new() -> Self {
        Self {
            visible: false,
            repo_id: None,
            private: true,
            local_name: String::new(),
            input: SingleLineInput::new(),
        }
    }

    pub fn show(&mut self, repo_id: RepoId, private: bool, local_name: String) {
        self.visible = true;
        self.repo_id = Some(repo_id);
        self.private = private;
        self.input.set(local_name.clone());
        self.local_name = local_name;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.repo_id = None;
        self.private = true;
        self.local_name.clear();
        self.input.clear();
    }

    pub fn repo_id(&self) -> Option<RepoId> {
        self.repo_id.clone()
    }

    pub fn private(&self) -> bool {
        self.private
    }

    pub fn name(&self) -> &str {
        self.input.value()
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        self.input.handle_key_event(
            key,
            Action::CancelCreateGitHubRepo,
            Action::ConfirmCreateGitHubRepo,
        )
    }

    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let visibility = if self.private { "private" } else { "public" };
        self.input.draw(
            frame,
            area,
            format!(" GitHub {visibility} repo: "),
            " Repository name (Enter: create, Esc: cancel) ",
        );
    }
}
