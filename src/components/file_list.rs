use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Color,
    widgets::{ListItem, ListState, Paragraph},
};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::components::Component;
use crate::components::diff_view;
use crate::components::file_status_view;
use crate::components::layout;
use crate::components::panel;
use crate::components::scroll;
use crate::components::selection;
use crate::components::style::fg_style;
use crate::git::status::FileEntry;
use crate::repo_id::RepoId;
use std::path::PathBuf;

pub(crate) struct FileList {
    files: Vec<FileEntry>,
    /// Files marked (Space) for a partial commit; empty means commit all.
    marked: std::collections::HashSet<PathBuf>,
    state: ListState,
    repo_name: String,
    repo_id: Option<RepoId>,
    pub focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    render_area: Rect,
    file_list_area: Rect,
    diff_area: Rect,
    // Diff view
    diff_content: Option<String>,
    diff_scroll: u16,
    pub horizontal_layout: bool,
    /// Monotonic counter to discard stale DiffLoaded results.
    diff_generation: u64,
}

impl FileList {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            marked: std::collections::HashSet::new(),
            state: ListState::default(),
            repo_name: String::new(),
            repo_id: None,
            focused: false,
            action_tx: None,
            render_area: Rect::default(),
            file_list_area: Rect::default(),
            diff_area: Rect::default(),
            diff_content: None,
            diff_scroll: 0,
            horizontal_layout: false,
            diff_generation: 0,
        }
    }

    pub fn set_files(&mut self, files: Vec<FileEntry>, repo_name: &str, repo_id: RepoId) {
        let is_same_repo = self.repo_id.as_ref() == Some(&repo_id);
        let prev_selected = self.state.selected();
        let files_changed = !is_same_repo || self.files != files;

        if is_same_repo {
            // Committed/reverted files vanish from the list; drop their marks.
            self.marked
                .retain(|path| files.iter().any(|f| f.path == *path));
        } else {
            self.marked.clear();
        }
        self.files = files;
        self.repo_name = repo_name.to_string();
        self.repo_id = Some(repo_id);

        if files_changed {
            self.diff_content = None;
            scroll::reset(&mut self.diff_scroll);
        }

        if self.files.is_empty() {
            self.state.select(None);
        } else if is_same_repo {
            selection::preserve_or_first(&mut self.state, prev_selected, self.files.len());
        } else {
            selection::select_first(&mut self.state, self.files.len());
        }
    }

    pub fn set_diff(&mut self, content: String) {
        self.diff_content = Some(content);
        scroll::reset(&mut self.diff_scroll);
    }

    pub fn viewing_diff(&self) -> bool {
        self.diff_content.is_some()
    }

    pub fn selected_path(&self) -> Option<String> {
        let idx = self.state.selected()?;
        let file = self.files.get(idx)?;
        Some(file.path.to_string_lossy().to_string())
    }

    pub fn diff_generation(&self) -> u64 {
        self.diff_generation
    }

    /// Marked file paths for a partial commit, only if they belong to `repo_id`.
    pub fn marked_paths_for(&self, repo_id: &RepoId) -> Vec<PathBuf> {
        if self.repo_id.as_ref() != Some(repo_id) {
            return Vec::new();
        }
        let mut paths: Vec<PathBuf> = self.marked.iter().cloned().collect();
        paths.sort();
        paths
    }

    fn toggle_mark(&mut self) {
        let Some(file) = self.state.selected().and_then(|i| self.files.get(i)) else {
            return;
        };
        if !self.marked.remove(&file.path) {
            self.marked.insert(file.path.clone());
        }
    }

    fn try_show_diff(&mut self) -> Option<Action> {
        let idx = self.state.selected()?;
        let repo_id = self.repo_id.clone()?;
        let file = self.files.get(idx)?;
        self.diff_generation += 1;
        Some(Action::ShowDiff(repo_id, file.path.clone()))
    }

    fn draw_file_list(&mut self, frame: &mut Frame, area: Rect) {
        let border_color = if self.focused && !self.viewing_diff() {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let title = if self.repo_name.is_empty() {
            " Changes ".to_string()
        } else if self.marked.is_empty() {
            format!(" Changes — {} ", self.repo_name)
        } else {
            format!(
                " Changes — {} ({} marked) ",
                self.repo_name,
                self.marked.len()
            )
        };

        let block = panel::bordered_block(title, border_color);

        if self.files.is_empty() {
            let msg = if self.repo_name.is_empty() {
                "Select a repository"
            } else {
                "No changes"
            };
            let paragraph = Paragraph::new(msg)
                .style(fg_style(Color::DarkGray))
                .block(block);
            frame.render_widget(paragraph, area);
            return;
        }

        let items: Vec<ListItem> = self
            .files
            .iter()
            .map(|entry| {
                file_status_view::worktree_file_item(entry, self.marked.contains(&entry.path))
            })
            .collect();

        frame.render_stateful_widget(panel::highlighted_list(items, block), area, &mut self.state);
    }

    fn draw_diff(&self, frame: &mut Frame, area: Rect) {
        let Some(ref content) = self.diff_content else {
            return;
        };

        diff_view::render_diff(
            frame,
            area,
            format!(" Diff — {} (Esc/h to close) ", self.repo_name),
            content,
            self.diff_scroll,
        );
    }
}

impl Component for FileList {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if self.viewing_diff() {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('h') | KeyCode::Left => {
                    self.diff_content = None;
                    scroll::reset(&mut self.diff_scroll);
                }
                _ if scroll::handle_vertical_key(&mut self.diff_scroll, key) => {}
                _ => {}
            }
            return Ok(None);
        }

        match key.code {
            _ if selection::handle_vertical_key(&mut self.state, self.files.len(), key) => Ok(None),
            KeyCode::Enter => Ok(self.try_show_diff()),
            KeyCode::Char(' ') => {
                self.toggle_mark();
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // In split mode, clicks in file_list_area select files
                let click_area = if self.viewing_diff() {
                    self.file_list_area
                } else {
                    self.render_area
                };

                if let Some(idx) = selection::clicked_list_index(
                    click_area,
                    mouse.column,
                    mouse.row,
                    self.state.offset(),
                    self.files.len(),
                ) {
                    if self.state.selected() == Some(idx) {
                        return Ok(self.try_show_diff());
                    }
                    self.state.select(Some(idx));
                }
                Ok(None)
            }
            MouseEventKind::ScrollUp => {
                if self.viewing_diff() {
                    let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
                    if self.diff_area.contains(pos) {
                        scroll::up(&mut self.diff_scroll);
                    } else {
                        selection::select_prev(&mut self.state, self.files.len());
                    }
                } else {
                    selection::select_prev(&mut self.state, self.files.len());
                }
                Ok(None)
            }
            MouseEventKind::ScrollDown => {
                if self.viewing_diff() {
                    let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
                    if self.diff_area.contains(pos) {
                        scroll::down(&mut self.diff_scroll);
                    } else {
                        selection::select_next(&mut self.state, self.files.len());
                    }
                } else {
                    selection::select_next(&mut self.state, self.files.len());
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.render_area = area;

        if self.diff_content.is_some() {
            // Split: file list 40% | diff 60%
            let chunks = layout::split_oriented(
                area,
                self.horizontal_layout,
                [Constraint::Percentage(40), Constraint::Percentage(60)],
            );

            self.file_list_area = chunks[0];
            self.diff_area = chunks[1];

            self.draw_file_list(frame, chunks[0]);
            self.draw_diff(frame, chunks[1]);
        } else {
            self.file_list_area = area;
            self.diff_area = Rect::default();
            self.draw_file_list(frame, area);
        }

        Ok(())
    }
}
