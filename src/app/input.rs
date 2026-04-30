use color_eyre::Result;
use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::action::Action;
use crate::app::helpers::base64_encode;
use crate::app::{App, FocusPanel, HELP_PAGE_COUNT};
use crate::components::Component;

impl App {
    pub(super) fn handle_key_event(&mut self, key: crossterm::event::KeyEvent) -> Result<()> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.action_tx.send(Action::Quit)?;
            return Ok(());
        }

        if self.notice_dialog.visible {
            let _ = self.notice_dialog.handle_key_event(key)?;
            return Ok(());
        }

        if self.confirm_dialog.visible {
            if let Some(action) = self.confirm_dialog.handle_key_event(key)? {
                self.action_tx.send(action)?;
            }
            return Ok(());
        }

        if self.path_input.visible {
            if let Some(action) = self.path_input.handle_key_event(key)? {
                self.action_tx.send(action)?;
            }
            return Ok(());
        }

        if self.commit_input.visible {
            if let Some(action) = self.commit_input.handle_key_event(key)? {
                self.action_tx.send(action)?;
            }
            return Ok(());
        }

        if self.github_repo_input.visible {
            if let Some(action) = self.github_repo_input.handle_key_event(key)? {
                self.action_tx.send(action)?;
            }
            return Ok(());
        }

        if self.focus == FocusPanel::Graph && self.git_graph.search_visible() {
            self.git_graph.handle_search_key(key)?;
            return Ok(());
        }

        if self.context_menu.visible {
            if let Some(action) = self.context_menu.handle_key_event(key)? {
                if !matches!(action, Action::HideContextMenu) {
                    self.action_tx.send(action)?;
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        if self.show_help {
            match key.code {
                KeyCode::Tab => {
                    self.help_page = (self.help_page + 1) % HELP_PAGE_COUNT;
                }
                KeyCode::BackTab => {
                    self.help_page = (self.help_page + HELP_PAGE_COUNT - 1) % HELP_PAGE_COUNT;
                }
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('h') | KeyCode::Char('?') => {
                    self.show_help = false;
                }
                _ => {}
            }
            return Ok(());
        }

        if matches!(key.code, KeyCode::Char('h') | KeyCode::Char('?')) {
            self.show_help = true;
            self.help_page = 0;
            return Ok(());
        }

        if self.show_operation_log {
            match key.code {
                KeyCode::Esc | KeyCode::Char('o') | KeyCode::Char('q') => {
                    self.show_operation_log = false;
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') => {
                if self.focus == FocusPanel::Changes && self.file_list.viewing_diff() {
                    self.file_list.handle_key_event(key)?;
                    return Ok(());
                }
                self.action_tx.send(Action::Quit)?;
            }
            KeyCode::Esc => {
                if self.focus == FocusPanel::Changes && self.file_list.viewing_diff() {
                    self.file_list.handle_key_event(key)?;
                } else if self.focus == FocusPanel::Graph && self.git_graph.has_detail() {
                    self.git_graph.handle_key_event(key)?;
                } else if self.focused_repo.take().is_some() {
                    self.focus = FocusPanel::Repos;
                } else {
                    match self.focus {
                        FocusPanel::Graph => self.focus = FocusPanel::Changes,
                        FocusPanel::Changes => self.focus = FocusPanel::Repos,
                        FocusPanel::Repos => self.action_tx.send(Action::Quit)?,
                    }
                }
            }
            KeyCode::Tab => {
                self.focus = if self.focused_repo.is_some() {
                    match self.focus {
                        FocusPanel::Changes => FocusPanel::Graph,
                        _ => FocusPanel::Changes,
                    }
                } else {
                    match self.focus {
                        FocusPanel::Repos => FocusPanel::Changes,
                        FocusPanel::Changes => FocusPanel::Graph,
                        FocusPanel::Graph => FocusPanel::Repos,
                    }
                };
            }
            KeyCode::BackTab => {
                self.focus = if self.focused_repo.is_some() {
                    match self.focus {
                        FocusPanel::Graph => FocusPanel::Changes,
                        _ => FocusPanel::Graph,
                    }
                } else {
                    match self.focus {
                        FocusPanel::Repos => FocusPanel::Graph,
                        FocusPanel::Changes => FocusPanel::Repos,
                        FocusPanel::Graph => FocusPanel::Changes,
                    }
                };
            }
            KeyCode::Char('r') => {
                self.action_tx.send(Action::RefreshAll)?;
            }
            KeyCode::Char('R') => {
                self.action_tx.send(Action::RescanRepos)?;
            }
            KeyCode::Char('F') => {
                self.action_tx.send(Action::ToggleFastMode)?;
            }
            KeyCode::Char('o') => {
                self.action_tx.send(Action::ToggleOperationLog)?;
            }
            KeyCode::Char('g') => {
                self.action_tx.send(Action::ShowGitGraph)?;
            }
            KeyCode::Char('a') => {
                self.action_tx.send(Action::OpenAddRepo)?;
            }
            KeyCode::Char('c') if self.focus != FocusPanel::Graph => {
                if let Some(repo_id) = self.active_repo_id() {
                    self.action_tx.send(Action::StartCommit(repo_id))?;
                }
            }
            KeyCode::Char('p') => {
                if let Some(repo_id) = self.active_repo_id() {
                    self.action_tx.send(Action::GitPush(repo_id))?;
                } else {
                    self.error_message = Some((
                        "No repository selected".to_string(),
                        std::time::Instant::now(),
                    ));
                }
            }
            KeyCode::Char('P') => {
                if let Some(repo_id) = self.active_repo_id() {
                    self.action_tx.send(Action::GitPublish(repo_id))?;
                }
            }
            KeyCode::Char('d') => {
                if let Some(repo_id) = self.active_repo_id()
                    && let Some(idx) = self.repo_list.resolve_index(&repo_id)
                {
                    let entry = &self.repo_list.repos[idx];
                    let name = entry.name.clone();
                    self.confirm_dialog
                        .show(format!("Remove {}?", name), Action::RemoveRepo(repo_id));
                }
            }
            KeyCode::Char('s') => {
                self.action_tx.send(Action::CycleSortOrder)?;
            }
            KeyCode::Char('y') => {
                let text = match self.focus {
                    FocusPanel::Repos => self
                        .repo_list
                        .selected_repo()
                        .map(|e| e.path.to_string_lossy().to_string()),
                    FocusPanel::Changes => self.file_list.selected_path(),
                    FocusPanel::Graph => self.git_graph.selected_text(),
                };
                if let Some(text) = text {
                    use std::io::Write;
                    let encoded = base64_encode(text.as_bytes());
                    let _ = write!(std::io::stdout(), "\x1b]52;c;{}\x1b\\", encoded);
                    let _ = std::io::stdout().flush();
                }
            }
            _ => match self.focus {
                FocusPanel::Repos => {
                    if let Some(action) = self.repo_list.handle_key_event(key)? {
                        self.action_tx.send(action)?;
                    }
                }
                FocusPanel::Changes => {
                    if let Some(action) = self.file_list.handle_key_event(key)? {
                        self.action_tx.send(action)?;
                    }
                }
                FocusPanel::Graph => {
                    if let Some(action) = self.git_graph.handle_key_event(key)? {
                        self.action_tx.send(action)?;
                    }
                }
            },
        }
        Ok(())
    }

    pub(super) fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<()> {
        if self.context_menu.visible {
            if let Some(action) = self.context_menu.handle_mouse_event(mouse)? {
                self.action_tx.send(action)?;
            } else if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.context_menu.hide();
            }
            return Ok(());
        }

        let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
        const GRAB_ZONE: u16 = 2;

        if self.repo_area.width > 0 {
            let border1 = self.repo_area.y + self.repo_area.height;
            let border2 = self.changes_area.y + self.changes_area.height;
            let mouse_pos = mouse.row;
            let total = self.repo_area.height + self.changes_area.height + self.graph_area.height;
            let origin = self.repo_area.y;

            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let d1 = mouse_pos.abs_diff(border1);
                    let d2 = mouse_pos.abs_diff(border2);
                    if d1 <= GRAB_ZONE && (d1 <= d2 || d2 > GRAB_ZONE) {
                        self.dragging_border = Some(0);
                    } else if d2 <= GRAB_ZONE {
                        self.dragging_border = Some(1);
                    } else {
                        self.dragging_border = None;
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) if self.dragging_border.is_some() => {
                    let rel = mouse_pos.saturating_sub(origin) as f64 / total as f64;
                    let min_f = 3.0 / total as f64;
                    match self.dragging_border {
                        Some(0) => {
                            self.border_frac[0] = rel.clamp(min_f, self.border_frac[1] - min_f);
                        }
                        Some(1) => {
                            self.border_frac[1] =
                                rel.clamp(self.border_frac[0] + min_f, 1.0 - min_f);
                        }
                        _ => {}
                    }
                    return Ok(());
                }
                MouseEventKind::Up(MouseButton::Left) if self.dragging_border.is_some() => {
                    self.dragging_border = None;
                    return Ok(());
                }
                _ => {}
            }
        }

        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            if self.repo_area.contains(pos) && self.focused_repo.is_none() {
                self.focus = FocusPanel::Repos;
            } else if self.changes_area.contains(pos) {
                self.focus = FocusPanel::Changes;
            } else if self.graph_area.contains(pos) {
                self.focus = FocusPanel::Graph;
            }
        }

        if self.repo_area.contains(pos) {
            if let Some(action) = self.repo_list.handle_mouse_event(mouse)? {
                self.action_tx.send(action)?;
            }
        } else if self.changes_area.contains(pos) {
            if let Some(action) = self.file_list.handle_mouse_event(mouse)? {
                self.action_tx.send(action)?;
            }
        } else if self.graph_area.contains(pos)
            && let Some(action) = self.git_graph.handle_mouse_event(mouse)?
        {
            self.action_tx.send(action)?;
        }
        Ok(())
    }
}
