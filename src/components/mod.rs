pub(crate) mod commit_input;
pub(crate) mod confirm_dialog;
pub(crate) mod context_menu;
pub(crate) mod diff_view;
pub(crate) mod file_list;
pub(crate) mod file_status_view;
pub(crate) mod git_graph;
pub(crate) mod github_repo_input;
pub(crate) mod layout;
pub(crate) mod notice_dialog;
pub(crate) mod panel;
pub(crate) mod path_input;
pub(crate) mod repo_list;
pub(crate) mod scroll;
pub(crate) mod selection;
pub(crate) mod status_bar;
pub(crate) mod style;
pub(crate) mod text_input;

use color_eyre::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use ratatui::Frame;
use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;

pub(crate) trait Component {
    fn register_action_handler(&mut self, _tx: UnboundedSender<Action>) -> Result<()> {
        Ok(())
    }

    fn init(&mut self) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, _key: KeyEvent) -> Result<Option<Action>> {
        Ok(None)
    }

    fn handle_mouse_event(&mut self, _mouse: MouseEvent) -> Result<Option<Action>> {
        Ok(None)
    }

    fn update(&mut self, _action: Action) -> Result<Option<Action>> {
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()>;
}
