use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::Rect,
    style::Color,
    text::{Line, Span},
    widgets::{ListItem, ListState},
};
use std::collections::HashSet;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::components::Component;
use crate::components::panel;
use crate::components::selection;
use crate::components::style::fg_span;
use crate::config::RepoNameFormat;
use crate::git::remote::github_repo_name_from_web_url;
use crate::git::status::RepoStatus;
use crate::repo_id::RepoId;

#[derive(Clone, Debug)]
pub(crate) struct RepoEntry {
    pub path: PathBuf,
    pub name: String,
    pub status: Option<RepoStatus>,
    /// True only during push/pull/rebase — shows animated spinner
    pub git_op: bool,
}

impl RepoEntry {
    pub(crate) fn id(&self) -> RepoId {
        RepoId(self.path.clone())
    }

    pub(crate) fn display_name(&self) -> String {
        self.display_name_for_format(RepoNameFormat::FolderGithub, false)
    }

    pub(crate) fn display_name_for_format(
        &self,
        format: RepoNameFormat,
        duplicate_fallback: bool,
    ) -> String {
        let folder = self.folder_name();
        match format {
            RepoNameFormat::FolderGithub => match self.remote_repo_name() {
                Some(repo_name) => format!("{folder}:{repo_name}"),
                None if duplicate_fallback => self.parent_folder_label(folder),
                None => folder,
            },
            RepoNameFormat::Folder => folder,
            RepoNameFormat::ParentFolder => self.parent_folder_label(folder),
            RepoNameFormat::Path => self.path.to_string_lossy().to_string(),
        }
    }

    fn parent_folder_label(&self, folder: String) -> String {
        self.parent_folder_name()
            .map(|parent| format!("{parent}:{folder}"))
            .unwrap_or(folder)
    }

    fn folder_name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.name.clone())
    }

    fn parent_folder_name(&self) -> Option<String> {
        self.path
            .parent()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.is_empty())
    }

    fn remote_repo_name(&self) -> Option<&str> {
        self.status
            .as_ref()
            .and_then(|status| status.github_url.as_deref())
            .and_then(github_repo_name_from_web_url)
    }
}

/// Maps a visual row in the list to either a repo or one of its worktrees.
#[derive(Clone, Debug)]
enum DisplayRow {
    Repo(usize),
    Worktree(usize, usize), // (repo_index, worktree_index)
}

pub(crate) struct RepoList {
    pub repos: Vec<RepoEntry>,
    pub state: ListState,
    pub render_area: Rect,
    pub focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    /// Which repos have their worktree list expanded
    expanded_repos: HashSet<RepoId>,
    /// Computed mapping from visual row → data
    display_rows: Vec<DisplayRow>,
    display_rows_dirty: bool,
    duplicate_folder_names: HashSet<String>,
    repo_name_format: RepoNameFormat,
}

impl RepoList {
    pub fn new(
        repo_paths: Vec<PathBuf>,
        _ignore_dirty_subs: bool,
        repo_name_format: RepoNameFormat,
    ) -> Self {
        let repos: Vec<RepoEntry> = repo_paths
            .into_iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                RepoEntry {
                    path,
                    name,
                    status: None,
                    git_op: false,
                }
            })
            .collect();

        let mut state = ListState::default();
        selection::select_first(&mut state, repos.len());

        let mut list = Self {
            repos,
            state,
            render_area: Rect::default(),
            focused: true,
            action_tx: None,
            expanded_repos: HashSet::new(),
            display_rows: Vec::new(),
            display_rows_dirty: true,
            duplicate_folder_names: HashSet::new(),
            repo_name_format,
        };
        list.rebuild_structure_cache();
        list
    }

    fn mark_structure_dirty(&mut self) {
        self.display_rows_dirty = true;
    }

    fn folder_name_for_entry(entry: &RepoEntry) -> Option<String> {
        entry
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
    }

    fn rebuild_duplicate_folder_names(&mut self) {
        let mut seen = HashSet::new();
        let mut duplicates = HashSet::new();
        for entry in &self.repos {
            if let Some(folder) = Self::folder_name_for_entry(entry)
                && !seen.insert(folder.clone())
            {
                duplicates.insert(folder);
            }
        }
        self.duplicate_folder_names = duplicates;
    }

    fn rebuild_display_rows(&mut self) {
        self.display_rows.clear();
        for (i, entry) in self.repos.iter().enumerate() {
            self.display_rows.push(DisplayRow::Repo(i));
            let id = entry.id();
            if self.expanded_repos.contains(&id)
                && let Some(status) = &entry.status
            {
                for j in 0..status.worktree_info.len() {
                    self.display_rows.push(DisplayRow::Worktree(i, j));
                }
            }
        }
        self.display_rows_dirty = false;
    }

    fn rebuild_structure_cache(&mut self) {
        self.rebuild_duplicate_folder_names();
        self.rebuild_display_rows();
    }

    fn ensure_display_rows(&mut self) {
        if self.display_rows_dirty {
            self.rebuild_display_rows();
        }
    }

    fn has_duplicate_folder_name(&self, repo_idx: usize) -> bool {
        self.repos
            .get(repo_idx)
            .and_then(Self::folder_name_for_entry)
            .is_some_and(|folder| self.duplicate_folder_names.contains(&folder))
    }

    pub(crate) fn display_name_for_index(&self, repo_idx: usize) -> Option<String> {
        let entry = self.repos.get(repo_idx)?;
        Some(entry.display_name_for_format(
            self.repo_name_format,
            self.has_duplicate_folder_name(repo_idx),
        ))
    }

    pub(crate) fn display_name_for_id(&self, id: &RepoId) -> Option<String> {
        self.resolve_index(id)
            .and_then(|idx| self.display_name_for_index(idx))
    }

    fn repo_id_for_index(&self, repo_idx: usize) -> Option<RepoId> {
        self.repos.get(repo_idx).map(RepoEntry::id)
    }

    pub(crate) fn push_repo(&mut self, entry: RepoEntry) {
        self.repos.push(entry);
        self.rebuild_structure_cache();
    }

    pub(crate) fn remove_repo(&mut self, index: usize) -> Option<RepoEntry> {
        if index >= self.repos.len() {
            return None;
        }
        let entry = self.repos.remove(index);
        self.expanded_repos.retain(|id| id.0 != entry.path);
        self.rebuild_structure_cache();
        Some(entry)
    }

    pub(crate) fn sort_alphabetical(&mut self) {
        self.repos.sort_by_key(|r| r.name.to_lowercase());
        self.rebuild_structure_cache();
    }

    pub(crate) fn sort_dirty_first(&mut self) {
        self.repos.sort_by(|a, b| {
            let a_dirty = a.status.as_ref().map(|s| s.is_dirty).unwrap_or(false);
            let b_dirty = b.status.as_ref().map(|s| s.is_dirty).unwrap_or(false);
            b_dirty
                .cmp(&a_dirty)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.rebuild_structure_cache();
    }

    /// Returns the parent repo index for the current selection.
    pub fn selected_index(&self) -> Option<usize> {
        let di = self.state.selected()?;
        match self.display_rows.get(di)? {
            DisplayRow::Repo(i) => Some(*i),
            DisplayRow::Worktree(ri, _) => Some(*ri),
        }
    }

    /// Resolve a stable `RepoId` to its current positional index.
    pub fn resolve_index(&self, id: &RepoId) -> Option<usize> {
        self.repos.iter().position(|e| e.path == id.0)
    }

    pub fn resolve_entry(&self, id: &RepoId) -> Option<&RepoEntry> {
        self.resolve_index(id).and_then(|idx| self.repos.get(idx))
    }

    pub fn resolve_entry_mut(&mut self, id: &RepoId) -> Option<&mut RepoEntry> {
        let idx = self.resolve_index(id)?;
        self.repos.get_mut(idx)
    }

    /// Returns the parent RepoEntry for the current selection.
    pub fn selected_repo(&self) -> Option<&RepoEntry> {
        self.selected_index().and_then(|i| self.repos.get(i))
    }

    /// Select the display row corresponding to a repo index.
    /// Used by app.rs when it needs to programmatically select a repo.
    pub fn select_repo_row(&mut self, repo_idx: usize) {
        for (di, row) in self.display_rows.iter().enumerate() {
            if matches!(row, DisplayRow::Repo(i) if *i == repo_idx) {
                self.state.select(Some(di));
                return;
            }
        }
    }

    pub fn update_status(&mut self, index: usize, repo_status: RepoStatus) {
        if let Some(entry) = self.repos.get_mut(index) {
            entry.status = Some(repo_status);
            entry.git_op = false;
        }
        self.rebuild_display_rows();
    }

    /// Build the action to emit for the current selection.
    fn emit_selection_action(&self) -> Option<Action> {
        let di = self.state.selected()?;
        match self.display_rows.get(di)? {
            DisplayRow::Repo(i) => self.repo_id_for_index(*i).map(Action::SelectRepo),
            DisplayRow::Worktree(ri, wi) => {
                let entry = &self.repos[*ri];
                let wt = entry.status.as_ref()?.worktree_info.get(*wi)?;
                Some(Action::SelectWorktree {
                    repo_id: entry.id(),
                    worktree_path: wt.path.clone(),
                    worktree_branch: wt.branch.clone(),
                })
            }
        }
    }

    fn emit_open_graph_action(&self) -> Option<Action> {
        let repo_idx = self.selected_index()?;
        self.repo_id_for_index(repo_idx)
            .map(Action::ShowRepoGitGraph)
    }

    /// Toggle worktree expansion for the repo at the current selection.
    fn toggle_expand(&mut self) {
        let Some(di) = self.state.selected() else {
            return;
        };
        let repo_idx = match self.display_rows.get(di) {
            Some(DisplayRow::Repo(i)) => *i,
            Some(DisplayRow::Worktree(ri, _)) => *ri,
            None => return,
        };
        let entry = &self.repos[repo_idx];
        let has_worktrees = entry
            .status
            .as_ref()
            .is_some_and(|s| !s.worktree_info.is_empty());
        if !has_worktrees {
            return;
        }
        let id = entry.id();
        if self.expanded_repos.contains(&id) {
            // Collapsing: move selection to the parent repo row
            self.expanded_repos.remove(&id);
            self.mark_structure_dirty();
            self.ensure_display_rows();
            self.select_repo_row(repo_idx);
        } else {
            self.expanded_repos.insert(id);
            self.rebuild_display_rows();
        }
    }

    fn render_repo_item(&self, entry: &RepoEntry, repo_idx: usize) -> ListItem<'static> {
        let mut spans = Vec::new();

        // Dirty / git-op indicator
        if entry.git_op {
            spans.push(fg_span("~ ", Color::Cyan));
        } else if entry.status.as_ref().map(|s| s.is_dirty).unwrap_or(false) {
            spans.push(fg_span("* ", Color::Yellow));
        } else {
            spans.push(Span::raw("  "));
        }

        if let Some(status) = &entry.status {
            // Branch name
            spans.push(fg_span(format!("{:<12} ", status.branch), Color::Cyan));

            // Ahead/behind
            if status.ahead > 0 {
                spans.push(fg_span(format!("\u{2191}{} ", status.ahead), Color::Green));
            }
            if status.behind > 0 {
                spans.push(fg_span(format!("\u{2193}{} ", status.behind), Color::Red));
            }

            // Worktree expand/collapse indicator
            if !status.worktree_info.is_empty() {
                let id = entry.id();
                let expanded = self.expanded_repos.contains(&id);
                let icon = if expanded { "\u{25bc}" } else { "\u{25b6}" };
                spans.push(fg_span(
                    format!("{}{} ", icon, status.worktree_info.len()),
                    Color::Magenta,
                ));
            }

            // Dirty submodule indicator
            if status.has_dirty_submodules {
                spans.push(fg_span("\u{25c8} ", Color::LightMagenta));
            }

            // Fetch failure indicator
            if status.fetch_failed {
                spans.push(fg_span("\u{26a0} ", Color::DarkGray));
            }

            // Change count
            if !status.files.is_empty() {
                spans.push(fg_span(format!("[{}] ", status.files.len()), Color::Yellow));
            }
        }

        // Repo name
        let repo_label = self
            .display_name_for_index(repo_idx)
            .unwrap_or_else(|| entry.display_name());
        spans.push(fg_span(repo_label, Color::White));

        ListItem::new(Line::from(spans))
    }

    fn render_worktree_item(&self, entry: &RepoEntry, wt_idx: usize) -> Option<ListItem<'static>> {
        let wt = entry.status.as_ref()?.worktree_info.get(wt_idx)?;
        let spans = vec![
            fg_span("    \u{2387} ", Color::DarkGray),
            fg_span(wt.branch.clone(), Color::Magenta),
        ];
        Some(ListItem::new(Line::from(spans)))
    }

    fn clicked_row_index(&self, column: u16, row: u16) -> Option<usize> {
        selection::clicked_list_index(
            self.render_area,
            column,
            row,
            self.state.offset(),
            self.display_rows.len(),
        )
    }
}

impl Component for RepoList {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn init(&mut self) -> Result<()> {
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if selection::handle_vertical_key(&mut self.state, self.display_rows.len(), key) {
            return Ok(self.emit_selection_action());
        }

        match key.code {
            KeyCode::Char('w') => {
                self.toggle_expand();
                Ok(self.emit_selection_action())
            }
            KeyCode::Enter => Ok(self.emit_open_graph_action()),
            _ => Ok(None),
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(idx) = self.clicked_row_index(mouse.column, mouse.row) {
                    // A second click on the selected repo row opens its graph.
                    // Worktree expansion stays on `w` so click/Enter both mean "enter".
                    if self.state.selected() == Some(idx) {
                        return match self.display_rows.get(idx) {
                            Some(DisplayRow::Repo(_)) => Ok(self.emit_open_graph_action()),
                            Some(DisplayRow::Worktree(_, _)) => Ok(self.emit_selection_action()),
                            None => Ok(None),
                        };
                    }
                    self.state.select(Some(idx));
                    return Ok(self.emit_selection_action());
                }
                Ok(None)
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some(idx) = self.clicked_row_index(mouse.column, mouse.row) {
                    self.state.select(Some(idx));
                    // Only show context menu for repo rows
                    if let Some(DisplayRow::Repo(i)) = self.display_rows.get(idx)
                        && let Some(id) = self.repo_id_for_index(*i)
                    {
                        return Ok(Some(Action::ShowContextMenu {
                            id,
                            row: mouse.row,
                            col: mouse.column,
                        }));
                    }
                }
                Ok(None)
            }
            MouseEventKind::ScrollUp => {
                selection::select_prev(&mut self.state, self.display_rows.len());
                Ok(self.emit_selection_action())
            }
            MouseEventKind::ScrollDown => {
                selection::select_next(&mut self.state, self.display_rows.len());
                Ok(self.emit_selection_action())
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action) -> Result<Option<Action>> {
        match action {
            Action::RepoStatusUpdated { ref id, ref status } => {
                if let Some(idx) = self.resolve_index(id) {
                    self.update_status(idx, status.clone());
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.render_area = area;
        self.ensure_display_rows();

        let items: Vec<ListItem> = self
            .display_rows
            .iter()
            .filter_map(|row| match row {
                DisplayRow::Repo(i) => Some(self.render_repo_item(&self.repos[*i], *i)),
                DisplayRow::Worktree(ri, wi) => self.render_worktree_item(&self.repos[*ri], *wi),
            })
            .collect();

        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let block = panel::bordered_block(" Repositories ".to_string(), border_color);
        frame.render_stateful_widget(panel::highlighted_list(items, block), area, &mut self.state);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::status::RepoStatus;

    fn entry(path: &str, github_url: Option<&str>) -> RepoEntry {
        RepoEntry {
            path: PathBuf::from(path),
            name: PathBuf::from(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            status: github_url.map(|url| RepoStatus {
                branch: "main".to_string(),
                files: Vec::new(),
                ahead: 0,
                behind: 0,
                has_upstream: false,
                is_dirty: false,
                worktree_info: Vec::new(),
                has_submodules: false,
                submodules: Vec::new(),
                has_dirty_submodules: false,
                fetch_failed: false,
                has_github_remote: true,
                github_url: Some(url.to_string()),
                has_origin_remote: true,
                graph_key: String::new(),
                remote_key: String::new(),
            }),
            git_op: false,
        }
    }

    #[test]
    fn display_name_uses_folder_and_github_repo() {
        let entry = entry("/home/me/015_gitoto", Some("https://github.com/me/gitoto"));
        assert_eq!(entry.display_name(), "015_gitoto:gitoto");
    }

    #[test]
    fn display_name_folder_github_falls_back_to_folder() {
        let entry = entry("/home/me/015_gitoto", None);
        assert_eq!(
            entry.display_name_for_format(RepoNameFormat::FolderGithub, false),
            "015_gitoto"
        );
    }

    #[test]
    fn display_name_parent_folder_format() {
        let entry = entry("/home/me/projects/015_gitoto", None);
        assert_eq!(
            entry.display_name_for_format(RepoNameFormat::ParentFolder, false),
            "projects:015_gitoto"
        );
    }

    #[test]
    fn display_name_duplicate_folder_uses_parent_fallback() {
        let list = RepoList::new(
            vec![
                PathBuf::from("/home/me/work/app"),
                PathBuf::from("/home/me/archive/app"),
            ],
            false,
            RepoNameFormat::FolderGithub,
        );

        assert_eq!(list.display_name_for_index(0).as_deref(), Some("work:app"));
        assert_eq!(
            list.display_name_for_index(1).as_deref(),
            Some("archive:app")
        );
    }

    #[test]
    fn display_name_duplicate_cache_updates_after_remove() {
        let mut list = RepoList::new(
            vec![
                PathBuf::from("/home/me/work/app"),
                PathBuf::from("/home/me/archive/app"),
            ],
            false,
            RepoNameFormat::FolderGithub,
        );

        list.remove_repo(1);

        assert_eq!(list.display_name_for_index(0).as_deref(), Some("app"));
    }
}
