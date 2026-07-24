use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Clear, ListItem, ListState},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::components::Component;
use crate::components::panel;
use crate::components::selection;
use crate::components::style::{bold_fg_span, fg_style};
use crate::git::status::RepoStatus;
use crate::repo_id::RepoId;

#[derive(Clone, Debug)]
enum MenuAction {
    OpenGraph,
    Refresh,
    CopyPath,
    OpenGitHub,
    Commit,
    Push,
    Publish,
    CreateGithubPrivate,
    CreateGithubPublic,
    Pull,
    PullRebase,
    PullSubmodules,
    RemoveOriginRemote,
    SubmoduleUpdate,
    SubmoduleSync,
    SubmoduleUpdateLatest,
    RevertFile(std::path::PathBuf),
    CopyFilePath(std::path::PathBuf),
}

impl MenuAction {
    fn into_action(self, id: RepoId) -> Action {
        match self {
            Self::OpenGraph => Action::ShowRepoGitGraph(id),
            Self::Refresh => Action::RefreshRepo(id),
            Self::CopyPath => Action::CopyPath(id),
            Self::OpenGitHub => Action::OpenGitHub(id),
            Self::Commit => Action::StartCommit(id),
            Self::Push => Action::GitPush(id),
            Self::Publish => Action::GitPublish(id),
            Self::CreateGithubPrivate => Action::StartCreateGitHubRepo { id, private: true },
            Self::CreateGithubPublic => Action::StartCreateGitHubRepo { id, private: false },
            Self::Pull => Action::GitPull(id),
            Self::PullRebase => Action::GitPullRebase(id),
            Self::PullSubmodules => Action::GitPullSubmodules(id),
            Self::RemoveOriginRemote => Action::RemoveOriginRemote(id),
            Self::SubmoduleUpdate => Action::GitSubmoduleUpdate(id),
            Self::SubmoduleSync => Action::GitSubmoduleSync(id),
            Self::SubmoduleUpdateLatest => Action::GitSubmoduleUpdateLatest(id),
            Self::RevertFile(path) => Action::RevertFile { id, path },
            Self::CopyFilePath(path) => Action::CopyFilePath(path),
        }
    }

    fn style(&self) -> Style {
        match self {
            Self::Push => fg_style(Color::Green),
            Self::Commit | Self::Publish | Self::CreateGithubPrivate | Self::CreateGithubPublic => {
                fg_style(Color::Cyan)
            }
            Self::Pull | Self::PullRebase | Self::PullSubmodules => fg_style(Color::Yellow),
            Self::RemoveOriginRemote | Self::RevertFile(_) => fg_style(Color::Red),
            Self::SubmoduleUpdate | Self::SubmoduleSync | Self::SubmoduleUpdateLatest => {
                fg_style(Color::LightMagenta)
            }
            _ => Style::default(),
        }
    }
}

struct MenuItem {
    label: String,
    action: Option<MenuAction>,
}

fn menu_item(label: impl Into<String>, action: MenuAction) -> MenuItem {
    MenuItem {
        label: label.into(),
        action: Some(action),
    }
}

fn separator(label: impl Into<String>) -> MenuItem {
    MenuItem {
        label: label.into(),
        action: None,
    }
}

fn push_section<L>(
    items: &mut Vec<MenuItem>,
    label: &'static str,
    actions: impl IntoIterator<Item = (L, MenuAction)>,
) where
    L: Into<String>,
{
    items.push(separator(label));
    items.extend(
        actions
            .into_iter()
            .map(|(label, action)| menu_item(label, action)),
    );
}

fn push_label(state: &RepoMenuState) -> String {
    if !state.has_upstream {
        "Push (publish first)".to_string()
    } else if state.ahead > 0 {
        format!("Push  ↑{}", state.ahead)
    } else {
        "Push".to_string()
    }
}

fn pull_label(state: &RepoMenuState) -> String {
    if state.behind > 0 {
        format!("Pull  ↓{}", state.behind)
    } else {
        "Pull".to_string()
    }
}

#[derive(Default)]
pub(crate) struct RepoMenuState {
    pub ahead: usize,
    pub behind: usize,
    pub has_upstream: bool,
    pub has_submodules: bool,
    pub has_github_remote: bool,
    pub has_origin_remote: bool,
}

impl From<Option<&RepoStatus>> for RepoMenuState {
    fn from(status: Option<&RepoStatus>) -> Self {
        status
            .map(|s| Self {
                ahead: s.ahead,
                behind: s.behind,
                has_upstream: s.has_upstream,
                has_submodules: s.has_submodules,
                has_github_remote: s.has_github_remote,
                has_origin_remote: s.has_origin_remote,
            })
            .unwrap_or_default()
    }
}

pub(crate) struct ContextMenu {
    pub visible: bool,
    pub repo_id: Option<RepoId>,
    pub position: (u16, u16), // (col, row)
    items: Vec<MenuItem>,
    state: ListState,
    last_rendered_area: Rect,
    action_tx: Option<UnboundedSender<Action>>,
}

impl ContextMenu {
    pub fn new() -> Self {
        Self {
            visible: false,
            repo_id: None,
            position: (0, 0),
            items: Vec::new(),
            state: ListState::default(),
            last_rendered_area: Rect::default(),
            action_tx: None,
        }
    }

    pub fn show(&mut self, repo_id: RepoId, col: u16, row: u16, repo_state: RepoMenuState) {
        self.visible = true;
        self.repo_id = Some(repo_id);
        self.position = (col, row);

        let mut items = Vec::new();
        push_section(
            &mut items,
            "Repository",
            [
                ("Open git graph", MenuAction::OpenGraph),
                ("Refresh", MenuAction::Refresh),
                ("Copy path", MenuAction::CopyPath),
                ("Commit all changes", MenuAction::Commit),
            ],
        );
        push_section(
            &mut items,
            "Remote",
            [
                (push_label(&repo_state), MenuAction::Push),
                (String::from("Publish branch"), MenuAction::Publish),
                (pull_label(&repo_state), MenuAction::Pull),
                (String::from("Pull --rebase"), MenuAction::PullRebase),
            ],
        );

        if repo_state.has_github_remote {
            push_section(
                &mut items,
                "GitHub",
                [("Open GitHub", MenuAction::OpenGitHub)],
            );
        } else {
            push_section(
                &mut items,
                "GitHub",
                [
                    (
                        "Create GitHub repo (private)",
                        MenuAction::CreateGithubPrivate,
                    ),
                    (
                        "Create GitHub repo (public)",
                        MenuAction::CreateGithubPublic,
                    ),
                ],
            );
        }

        if repo_state.has_submodules {
            push_section(
                &mut items,
                "Submodules",
                [
                    ("Pull --recurse-subs", MenuAction::PullSubmodules),
                    ("Sub: update --init", MenuAction::SubmoduleUpdate),
                    ("Sub: sync", MenuAction::SubmoduleSync),
                    ("Sub: pull latest", MenuAction::SubmoduleUpdateLatest),
                ],
            );
        }

        if repo_state.has_origin_remote {
            push_section(
                &mut items,
                "Danger",
                [("Remove origin remote", MenuAction::RemoveOriginRemote)],
            );
        }

        self.items = items;
        self.select_first_action();
    }

    pub fn show_for_file(
        &mut self,
        repo_id: RepoId,
        path: std::path::PathBuf,
        col: u16,
        row: u16,
    ) {
        self.visible = true;
        self.repo_id = Some(repo_id);
        self.position = (col, row);

        let filename = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let mut items = Vec::new();
        push_section(
            &mut items,
            "File",
            [
                (format!("Revert {filename}"), MenuAction::RevertFile(path.clone())),
                ("Copy path".to_string(), MenuAction::CopyFilePath(path)),
            ],
        );

        self.items = items;
        self.select_first_action();
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    fn menu_rect(&self, terminal_area: Rect) -> Rect {
        let width = self
            .items
            .iter()
            .map(|item| item.label.chars().count() as u16 + 2)
            .max()
            .unwrap_or(24)
            .max(24);
        let height = (self.items.len() as u16) + 2; // +2 for border

        let x = self
            .position
            .0
            .min(terminal_area.width.saturating_sub(width));
        let y = self
            .position
            .1
            .min(terminal_area.height.saturating_sub(height));

        Rect::new(x, y, width, height)
    }

    fn select_first_action(&mut self) {
        let idx = self.items.iter().position(|item| item.action.is_some());
        self.state.select(idx);
    }

    fn activate_selected(&mut self) -> Option<Action> {
        let idx = self.state.selected()?;
        let item = self.items.get(idx)?;
        let id = self.repo_id.clone()?;
        let action = item.action.clone()?.into_action(id);
        self.hide();
        Some(action)
    }

    fn click_item_index(&self, col: u16, row: u16) -> Option<usize> {
        let rect = self.menu_rect(self.last_rendered_area);
        let content_x = rect.x + 1;
        let content_y = rect.y + 1;
        let content_right = rect.x + rect.width.saturating_sub(1);
        let content_bottom = content_y + self.items.len() as u16;

        if col >= content_x && col < content_right && row >= content_y && row < content_bottom {
            Some((row - content_y) as usize)
        } else {
            None
        }
    }
}

impl Component for ContextMenu {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if !self.visible {
            return Ok(None);
        }

        if selection::handle_vertical_key_matching(&mut self.state, self.items.len(), key, |idx| {
            self.items[idx].action.is_some()
        }) {
            return Ok(None);
        }

        match key.code {
            KeyCode::Esc => {
                self.hide();
                Ok(None)
            }
            KeyCode::Enter => Ok(self.activate_selected()),
            _ => {
                self.hide();
                Ok(Some(Action::HideContextMenu))
            }
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        if !self.visible {
            return Ok(None);
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(idx) = self.click_item_index(mouse.column, mouse.row) {
                    self.state.select(Some(idx));
                    return Ok(self.activate_selected());
                }
                self.hide();
                Ok(None)
            }
            MouseEventKind::Down(_) => {
                self.hide();
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if !self.visible {
            return Ok(());
        }

        self.last_rendered_area = area;
        let rect = self.menu_rect(area);

        frame.render_widget(Clear, rect);

        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|item| {
                let Some(action) = &item.action else {
                    return ListItem::new(Line::from(bold_fg_span(
                        format!(" {} ", item.label),
                        Color::DarkGray,
                    )));
                };
                ListItem::new(Line::from(Span::styled(item.label.clone(), action.style())))
            })
            .collect();

        let block = panel::plain_block(Color::Cyan);
        frame.render_stateful_widget(panel::highlighted_list(items, block), rect, &mut self.state);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_id() -> RepoId {
        RepoId(std::path::PathBuf::from("/tmp/repo"))
    }

    fn menu_state() -> RepoMenuState {
        RepoMenuState {
            ahead: 1,
            behind: 1,
            has_upstream: true,
            has_submodules: true,
            has_github_remote: true,
            has_origin_remote: true,
        }
    }

    fn selected_label(menu: &ContextMenu) -> &str {
        let idx = menu.state.selected().unwrap();
        &menu.items[idx].label
    }

    #[test]
    fn initial_selection_skips_separator() {
        let mut menu = ContextMenu::new();
        menu.show(repo_id(), 0, 0, menu_state());

        assert_eq!(selected_label(&menu), "Open git graph");
    }

    #[test]
    fn navigation_skips_separators() {
        let mut menu = ContextMenu::new();
        menu.show(repo_id(), 0, 0, menu_state());

        for _ in 0..4 {
            menu.handle_key_event(KeyEvent::from(KeyCode::Char('j')))
                .unwrap();
        }
        assert_eq!(selected_label(&menu), "Push  ↑1");

        menu.handle_key_event(KeyEvent::from(KeyCode::Char('k')))
            .unwrap();
        assert_eq!(selected_label(&menu), "Commit all changes");
    }

    #[test]
    fn activating_separator_does_not_emit_action() {
        let mut menu = ContextMenu::new();
        menu.show(repo_id(), 0, 0, menu_state());
        menu.state.select(Some(0));

        assert!(menu.activate_selected().is_none());
        assert!(menu.visible);
    }
}
