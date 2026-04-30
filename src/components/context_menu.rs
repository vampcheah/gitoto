use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::components::Component;
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

pub(crate) struct RepoMenuState {
    pub ahead: usize,
    pub behind: usize,
    pub has_upstream: bool,
    pub has_submodules: bool,
    pub has_github_remote: bool,
    pub has_origin_remote: bool,
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

        self.items = vec![
            separator("Repository"),
            menu_item("Open git graph", MenuAction::OpenGraph),
            menu_item("Refresh", MenuAction::Refresh),
            menu_item("Copy path", MenuAction::CopyPath),
            menu_item("Commit all changes", MenuAction::Commit),
            separator("Remote"),
            MenuItem {
                label: if !repo_state.has_upstream {
                    "Push (publish first)".into()
                } else if repo_state.ahead > 0 {
                    format!("Push  ↑{}", repo_state.ahead)
                } else {
                    "Push".into()
                },
                action: Some(MenuAction::Push),
            },
            menu_item("Publish branch", MenuAction::Publish),
            MenuItem {
                label: if repo_state.behind > 0 {
                    format!("Pull  ↓{}", repo_state.behind)
                } else {
                    "Pull".into()
                },
                action: Some(MenuAction::Pull),
            },
            menu_item("Pull --rebase", MenuAction::PullRebase),
        ];

        if repo_state.has_github_remote {
            self.items.push(separator("GitHub"));
            self.items
                .push(menu_item("Open GitHub", MenuAction::OpenGitHub));
        }

        if !repo_state.has_github_remote {
            self.items.push(separator("GitHub"));
            self.items.push(menu_item(
                "Create GitHub repo (private)",
                MenuAction::CreateGithubPrivate,
            ));
            self.items.push(menu_item(
                "Create GitHub repo (public)",
                MenuAction::CreateGithubPublic,
            ));
        }

        if repo_state.has_submodules {
            self.items.push(separator("Submodules"));
            self.items
                .push(menu_item("Pull --recurse-subs", MenuAction::PullSubmodules));
            self.items
                .push(menu_item("Sub: update --init", MenuAction::SubmoduleUpdate));
            self.items
                .push(menu_item("Sub: sync", MenuAction::SubmoduleSync));
            self.items.push(menu_item(
                "Sub: pull latest",
                MenuAction::SubmoduleUpdateLatest,
            ));
        }

        if repo_state.has_origin_remote {
            self.items.push(separator("Danger"));
            self.items.push(menu_item(
                "Remove origin remote",
                MenuAction::RemoveOriginRemote,
            ));
        }

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

    fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        let start = self.state.selected().unwrap_or(0);
        let i = (start + 1..self.items.len())
            .find(|i| self.items[*i].action.is_some())
            .unwrap_or(start);
        self.state.select(Some(i));
    }

    fn select_prev(&mut self) {
        let start = self.state.selected().unwrap_or(0);
        let i = (0..start)
            .rev()
            .find(|i| self.items[*i].action.is_some())
            .unwrap_or(start);
        self.state.select(Some(i));
    }

    fn select_first_action(&mut self) {
        let idx = self.items.iter().position(|item| item.action.is_some());
        self.state.select(idx);
    }

    fn activate_selected(&mut self) -> Option<Action> {
        let idx = self.state.selected()?;
        let item = self.items.get(idx)?;
        let id = self.repo_id.clone()?;
        let menu_action = item.action.clone()?;
        let action = match menu_action {
            MenuAction::OpenGraph => Action::ShowRepoGitGraph(id),
            MenuAction::Refresh => Action::RefreshRepo(id),
            MenuAction::CopyPath => Action::CopyPath(id),
            MenuAction::OpenGitHub => Action::OpenGitHub(id),
            MenuAction::Commit => Action::StartCommit(id),
            MenuAction::Push => Action::GitPush(id),
            MenuAction::Publish => Action::GitPublish(id),
            MenuAction::CreateGithubPrivate => Action::StartCreateGitHubRepo { id, private: true },
            MenuAction::CreateGithubPublic => Action::StartCreateGitHubRepo { id, private: false },
            MenuAction::Pull => Action::GitPull(id),
            MenuAction::PullRebase => Action::GitPullRebase(id),
            MenuAction::PullSubmodules => Action::GitPullSubmodules(id),
            MenuAction::RemoveOriginRemote => Action::RemoveOriginRemote(id),
            MenuAction::SubmoduleUpdate => Action::GitSubmoduleUpdate(id),
            MenuAction::SubmoduleSync => Action::GitSubmoduleSync(id),
            MenuAction::SubmoduleUpdateLatest => Action::GitSubmoduleUpdateLatest(id),
        };
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

        match key.code {
            KeyCode::Esc => {
                self.hide();
                Ok(None)
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                Ok(None)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_prev();
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
                    return ListItem::new(Line::from(Span::styled(
                        format!(" {} ", item.label),
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::BOLD),
                    )));
                };
                let style = match action {
                    MenuAction::Push => Style::default().fg(Color::Green),
                    MenuAction::Commit
                    | MenuAction::Publish
                    | MenuAction::CreateGithubPrivate
                    | MenuAction::CreateGithubPublic => Style::default().fg(Color::Cyan),
                    MenuAction::Pull | MenuAction::PullRebase | MenuAction::PullSubmodules => {
                        Style::default().fg(Color::Yellow)
                    }
                    MenuAction::RemoveOriginRemote => Style::default().fg(Color::Red),
                    MenuAction::SubmoduleUpdate
                    | MenuAction::SubmoduleSync
                    | MenuAction::SubmoduleUpdateLatest => Style::default().fg(Color::LightMagenta),
                    _ => Style::default(),
                };
                ListItem::new(Line::from(Span::styled(&item.label, style)))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(list, rect, &mut self.state);
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
            menu.select_next();
        }
        assert_eq!(selected_label(&menu), "Push  ↑1");

        menu.select_prev();
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
