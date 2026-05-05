use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Color,
    text::Line,
    widgets::{ListItem, ListState, Paragraph, Wrap},
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::components::diff_view;
use crate::components::file_status_view;
use crate::components::panel;
use crate::components::scroll;
use crate::components::selection;
use crate::components::style::fg_style;
use crate::git::graph::{BranchSegment, GraphBuilder, GraphOptions, GraphRow};
use crate::git::graph_render;

mod collapse;
mod component;
mod detail;
mod search;
use detail::CommitDetail;
use search::SearchState;

pub(crate) struct GitGraph {
    /// Display rows (may contain collapsed placeholders).
    rows: Vec<GraphRow>,
    /// Full rows from the graph builder (never filtered).
    all_rows: Vec<GraphRow>,
    /// Branches currently collapsed in the view.
    collapsed_branches: std::collections::HashSet<String>,
    /// DAG-computed branch segments (non-trunk groups of commits).
    segments: Vec<BranchSegment>,
    /// Maps all_rows index → segment index (None = main trunk).
    row_to_segment: Vec<Option<usize>>,
    state: ListState,
    repo_name: String,
    repo_path: Option<PathBuf>,
    loading: bool,
    error: Option<String>,
    pub focused: bool,
    action_tx: Option<UnboundedSender<Action>>,
    render_area: Rect,
    graph_list_area: Rect,
    files_area: Rect,
    diff_area: Rect,
    commit_detail: Option<CommitDetail>,
    pub(crate) graph_options: GraphOptions,
    search: SearchState,
    /// Horizontal scroll offset (characters) for the graph list
    h_scroll: usize,
    pub horizontal_layout: bool,
    /// Deferred reload: set when graph data arrives while detail is open.
    needs_reload: bool,
    /// Monotonic counter to discard stale GraphLoaded/DiffStatsLoaded results.
    load_generation: u64,
    /// Monotonic counter to discard stale CommitFilesLoaded/CommitDiffLoaded results.
    detail_generation: u64,
    /// Cached diff stats for commits in the currently loaded repository.
    diff_stat_cache: HashMap<git2::Oid, crate::git::graph::DiffStat>,
    /// OIDs with an in-flight diff stat request.
    pending_diff_stats: HashSet<git2::Oid>,
}

impl GitGraph {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            all_rows: Vec::new(),
            collapsed_branches: std::collections::HashSet::new(),
            segments: Vec::new(),
            row_to_segment: Vec::new(),
            state: ListState::default(),
            repo_name: String::new(),
            repo_path: None,
            loading: false,
            error: None,
            focused: false,
            action_tx: None,
            render_area: Rect::default(),
            graph_list_area: Rect::default(),
            files_area: Rect::default(),
            diff_area: Rect::default(),
            commit_detail: None,
            graph_options: GraphOptions::default(),
            search: SearchState::new(),
            h_scroll: 0,
            horizontal_layout: false,
            needs_reload: false,
            load_generation: 0,
            detail_generation: 0,
            diff_stat_cache: HashMap::new(),
            pending_diff_stats: HashSet::new(),
        }
    }

    pub fn load_repo(&mut self, path: PathBuf, repo_name: &str) {
        let is_same_repo = self.repo_path.as_deref() == Some(path.as_path());

        self.repo_name = repo_name.to_string();
        self.repo_path = Some(path.clone());
        self.error = None;

        // Keep old rows visible during reload (prevents blinking).
        // Only clear on repo switch.
        if !is_same_repo {
            self.loading = true;
            self.rows.clear();
            self.all_rows.clear();
            self.state.select(None);
            self.commit_detail = None;
            self.needs_reload = false;
            self.search.clear();
            self.collapsed_branches.clear();
            self.segments.clear();
            self.row_to_segment.clear();
            self.diff_stat_cache.clear();
            self.pending_diff_stats.clear();
        }

        let Some(tx) = &self.action_tx else { return };
        let tx = tx.clone();
        let options = self.graph_options.clone();
        self.load_generation += 1;
        let load_gen = self.load_generation;

        tokio::task::spawn_blocking(move || {
            let builder = GraphBuilder::new();
            match builder.build(&path, &options) {
                Ok(rows) => {
                    let _ = tx.send(Action::GraphLoaded {
                        generation: load_gen,
                        rows,
                    });
                }
                Err(e) => {
                    let _ = tx.send(Action::GraphError(format!("Failed to load graph: {}", e)));
                }
            }
        });
    }

    pub fn set_error(&mut self, msg: String) {
        self.error = Some(msg);
        self.loading = false;
    }

    pub fn set_rows(&mut self, mut rows: Vec<GraphRow>) {
        // Preserve selection position on refresh if possible
        let prev_selected = self.state.selected();
        // Carry forward diff_stats from previous all_rows to avoid blink on refresh
        if !self.all_rows.is_empty() {
            let old_stats: std::collections::HashMap<git2::Oid, crate::git::graph::DiffStat> = self
                .all_rows
                .iter()
                .filter_map(|r| r.diff_stat.clone().map(|s| (r.oid, s)))
                .collect();
            for row in &mut rows {
                if row.diff_stat.is_none() {
                    row.diff_stat = old_stats.get(&row.oid).cloned();
                }
            }
        }
        for row in &mut rows {
            if row.diff_stat.is_none() {
                row.diff_stat = self.diff_stat_cache.get(&row.oid).cloned();
            }
        }
        self.all_rows = rows;
        self.loading = false;
        self.recompute_segments();
        self.recompute_collapsed_rows();
        let row_count = self.display_rows().len();
        selection::preserve_or_first(&mut self.state, prev_selected, row_count);
    }

    pub fn set_diff_stats(&mut self, stats: Vec<(git2::Oid, crate::git::graph::DiffStat)>) {
        for (oid, stat) in &stats {
            self.pending_diff_stats.remove(oid);
            self.diff_stat_cache.insert(*oid, stat.clone());
        }
        let stat_map: HashMap<_, _> = stats.into_iter().collect();
        for row in &mut self.all_rows {
            if let Some(stat) = stat_map.get(&row.oid) {
                row.diff_stat = Some(stat.clone());
            }
        }
        self.recompute_collapsed_rows();
    }

    fn request_visible_diff_stats(&mut self, area: Rect) {
        if !self.graph_options.show_stats {
            return;
        }
        let Some(path) = self.repo_path.clone() else {
            return;
        };
        let Some(tx) = self.action_tx.clone() else {
            return;
        };
        let rows = self.display_rows();
        if rows.is_empty() {
            return;
        }

        let visible = area.height.saturating_sub(2).max(1) as usize;
        let selected = self.state.selected().unwrap_or(0).min(rows.len() - 1);
        let start = selected.saturating_sub(visible / 2);
        let end = (start + visible + 8).min(rows.len());

        let candidates = visible_diff_candidates(
            &rows[start..end],
            &self.diff_stat_cache,
            &self.pending_diff_stats,
        );
        let oids = register_pending_oids(candidates, &mut self.pending_diff_stats);
        if oids.is_empty() {
            return;
        }

        let requested = oids.clone();
        let generation = self.load_generation;
        tokio::task::spawn_blocking(move || {
            let mut stats =
                crate::git::commit_files::batch_diff_stats(&path, &oids).unwrap_or_default();
            let found: HashSet<_> = stats.iter().map(|(oid, _)| *oid).collect();
            stats.extend(
                requested
                    .into_iter()
                    .filter(|oid| !found.contains(oid))
                    .map(|oid| {
                        (
                            oid,
                            crate::git::graph::DiffStat {
                                additions: 0,
                                deletions: 0,
                            },
                        )
                    }),
            );
            let _ = tx.send(Action::DiffStatsLoaded { generation, stats });
        });
    }

    pub fn set_commit_files(&mut self, oid: String, message: String, files: Vec<(String, String)>) {
        let mut file_state = ListState::default();
        if !files.is_empty() {
            file_state.select(Some(0));
        }
        self.commit_detail = Some(CommitDetail {
            oid,
            message,
            files,
            file_state,
            diff_content: None,
            diff_scroll: 0,
            msg_scroll: 0,
            msg_area: Rect::default(),
            file_list_area: Rect::default(),
        });
    }

    pub fn set_commit_diff(&mut self, content: String) {
        if let Some(ref mut detail) = self.commit_detail {
            detail.diff_content = Some(content);
            scroll::reset(&mut detail.diff_scroll);
        }
    }

    pub fn has_detail(&self) -> bool {
        self.commit_detail.is_some()
    }

    pub fn set_needs_reload(&mut self) {
        self.needs_reload = true;
    }

    pub fn current_generation(&self) -> u64 {
        self.load_generation
    }

    pub fn refresh_pushed_status(&mut self) {
        let Some(path) = self.repo_path.clone() else {
            return;
        };
        self.refresh_pushed_status_for_path(path);
    }

    pub fn refresh_pushed_status_for_path(&mut self, path: PathBuf) {
        if self.repo_path.as_deref() != Some(path.as_path()) {
            return;
        }
        let Some(tx) = self.action_tx.clone() else {
            return;
        };
        let generation = self.load_generation;
        tokio::task::spawn_blocking(move || match crate::git::graph::pushed_oids(&path) {
            Ok(oids) => {
                let _ = tx.send(Action::PushedStatusLoaded { generation, oids });
            }
            Err(e) => {
                let _ = tx.send(Action::GraphError(format!(
                    "Failed to refresh pushed status: {}",
                    e
                )));
            }
        });
    }

    pub fn set_pushed_oids(&mut self, oids: Vec<git2::Oid>) {
        let pushed: HashSet<_> = oids.into_iter().collect();
        for row in &mut self.all_rows {
            row.is_pushed = pushed.contains(&row.oid);
        }
        self.recompute_collapsed_rows();
    }

    pub fn current_detail_generation(&self) -> u64 {
        self.detail_generation
    }

    /// Toggle collapse on the selected row's branch (or expand a collapsed group).
    fn toggle_collapse_selected(&mut self) {
        let Some(idx) = self.state.selected() else {
            return;
        };
        let Some(row) = self.display_rows().get(idx) else {
            return;
        };

        // Extract data before dropping the borrow on self
        let collapsed_key = row.collapsed.as_ref().map(|(k, _)| k.clone());
        let row_oid = row.oid;

        // If this is a collapsed placeholder, expand it
        if let Some(key) = collapsed_key {
            self.collapsed_branches.remove(key.as_str());
            self.recompute_collapsed_rows();
            return;
        }

        // Find this row in all_rows and look up its segment
        let Some(all_idx) = self.all_rows.iter().position(|r| r.oid == row_oid) else {
            return;
        };
        let Some(Some(seg_idx)) = self.row_to_segment.get(all_idx) else {
            return; // Main trunk — not collapsible
        };
        let seg = &self.segments[*seg_idx];
        self.collapsed_branches.insert(seg.id.clone());
        self.recompute_collapsed_rows();
    }

    /// Expand all collapsed branches.
    fn expand_all_branches(&mut self) {
        if self.collapsed_branches.is_empty() {
            return;
        }
        self.collapsed_branches.clear();
        self.recompute_collapsed_rows();
    }

    fn reload_graph(&mut self) {
        if let Some(path) = self.repo_path.clone() {
            let name = self.repo_name.clone();
            self.load_repo(path, &name);
        }
    }

    /// Recompute segments and row_to_segment mapping from all_rows.
    fn recompute_segments(&mut self) {
        self.segments = crate::git::graph::compute_branch_segments(&self.all_rows);
        self.row_to_segment = vec![None; self.all_rows.len()];
        for (seg_idx, seg) in self.segments.iter().enumerate() {
            for &row_idx in &seg.row_indices {
                self.row_to_segment[row_idx] = Some(seg_idx);
            }
        }
    }

    /// Returns the appropriate row slice for read-only access.
    /// When no branches are collapsed, reads directly from `all_rows`
    /// to avoid an unnecessary clone.
    fn display_rows(&self) -> &[GraphRow] {
        if self.collapsed_branches.is_empty() {
            &self.all_rows
        } else {
            &self.rows
        }
    }

    /// Recompute `self.rows` from `self.all_rows`, collapsing groups.
    fn recompute_collapsed_rows(&mut self) {
        if self.collapsed_branches.is_empty() {
            self.rows.clear();
            return;
        }

        self.rows =
            collapse::collapsed_rows(&self.all_rows, &self.segments, &self.collapsed_branches);
    }

    pub fn selected_text(&self) -> Option<String> {
        // If viewing commit files, copy the selected file path
        if let Some(ref detail) = self.commit_detail
            && let Some(idx) = detail.file_state.selected()
            && let Some((_, path)) = detail.files.get(idx)
        {
            return Some(path.clone());
        }
        // Otherwise copy the selected commit's short id + message
        let idx = self.state.selected()?;
        let row = self.display_rows().get(idx)?;
        Some(format!("{} {}", row.short_id, row.message))
    }

    pub fn search_visible(&self) -> bool {
        self.search.visible
    }

    pub fn handle_search_key(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Esc => {
                self.search.visible = false;
            }
            KeyCode::Enter => {
                self.search.visible = false;
                // Jump to first match if any
                if let Some(&idx) = self.search.matches.first() {
                    self.search.current_match = Some(0);
                    self.state.select(Some(idx));
                }
            }
            KeyCode::Backspace => {
                self.search.input.pop();
                self.update_search_matches();
            }
            KeyCode::Char(c) => {
                self.search.input.push(c);
                self.update_search_matches();
            }
            _ => {}
        }
        Ok(None)
    }

    fn update_search_matches(&mut self) {
        let matches = search::matching_rows(&self.search.input, self.display_rows());
        self.search.set_matches(matches);
    }

    fn search_next(&mut self) {
        if let Some(idx) = self.search.next_match() {
            self.state.select(Some(idx));
        }
    }

    fn search_prev(&mut self) {
        if let Some(idx) = self.search.prev_match() {
            self.state.select(Some(idx));
        }
    }

    fn try_show_commit_files(&mut self) -> Option<Action> {
        let idx = self.state.selected()?;
        let oid = self.display_rows().get(idx)?.oid.to_string();
        let repo_path = self.repo_path.clone()?;
        self.detail_generation += 1;
        Some(Action::ShowCommitFiles { repo_path, oid })
    }

    fn try_show_commit_diff(&mut self) -> Option<Action> {
        let detail = self.commit_detail.as_ref()?;
        let file_idx = detail.file_state.selected()?;
        let (_, file_path) = detail.files.get(file_idx)?;
        let repo_path = self.repo_path.clone()?;
        self.detail_generation += 1;
        Some(Action::ShowCommitDiff {
            repo_path,
            oid: detail.oid.clone(),
            file_path: file_path.clone(),
        })
    }

    fn draw_graph_list(&mut self, frame: &mut Frame, area: Rect) {
        let collapsed_count = self.collapsed_branches.len();
        let title = match (self.graph_options.first_parent, collapsed_count) {
            (true, 0) => format!(" Git Graph — {} [1st-parent] ", self.repo_name),
            (true, n) => format!(
                " Git Graph — {} [1st-parent] ({n} collapsed) ",
                self.repo_name
            ),
            (false, 0) => format!(" Git Graph — {} ", self.repo_name),
            (false, n) => format!(" Git Graph — {} ({n} collapsed) ", self.repo_name),
        };
        let border_color = if self.focused && self.commit_detail.is_none() {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let block = panel::bordered_block(title, border_color);

        if self.loading {
            let paragraph = Paragraph::new("Loading graph...")
                .style(fg_style(Color::Yellow))
                .block(block);
            frame.render_widget(paragraph, area);
            return;
        }

        if let Some(ref err) = self.error {
            let paragraph = Paragraph::new(err.as_str())
                .style(fg_style(Color::Red))
                .block(block);
            frame.render_widget(paragraph, area);
            return;
        }

        if self.display_rows().is_empty() {
            let paragraph = Paragraph::new("No commits")
                .style(fg_style(Color::Gray))
                .block(block);
            frame.render_widget(paragraph, area);
            return;
        }

        self.request_visible_diff_stats(area);

        let rows_len = self.display_rows().len();
        let visible = area.height.saturating_sub(2).max(1) as usize;
        let selected = self.state.selected().unwrap_or(0).min(rows_len - 1);
        let max_start = rows_len.saturating_sub(visible);
        let start = selected.saturating_sub(visible / 2).min(max_start);
        let end = (start + visible).min(rows_len);
        *self.state.offset_mut() = start;

        let label_max_len = self.graph_options.label_max_len;
        let max_width = area.width.saturating_sub(2) as usize; // 2 for borders
        let has_search = !self.search.input.is_empty() && !self.search.matches.is_empty();
        let items: Vec<ListItem> = self.display_rows()[start..end]
            .iter()
            .enumerate()
            .map(|(local_i, row)| {
                let row_idx = start + local_i;
                let dimmed = has_search && !self.search.matches.contains(&row_idx);
                let mut spans = graph_render::render_commit_row(row, label_max_len, dimmed);
                graph_render::h_scroll_line(&mut spans, self.h_scroll, max_width);
                ListItem::new(Line::from(spans))
            })
            .collect();

        let mut render_state = ListState::default();
        render_state.select(Some(selected.saturating_sub(start)));
        frame.render_stateful_widget(
            panel::highlighted_list(items, block),
            area,
            &mut render_state,
        );
    }

    fn draw_commit_files(detail: &mut CommitDetail, frame: &mut Frame, area: Rect) {
        let title = format!(" Files — {} ", &detail.oid[..7.min(detail.oid.len())]);

        // Split area: commit message at top, file list below
        let msg_line_count = detail.message.lines().count().max(1) as u16;
        // Cap message height: 2 for border + lines, max ~1/3 of area
        // Always guarantee at least 3 (1 content line + 2 borders)
        let msg_height = (msg_line_count + 2).min(area.height / 3).clamp(3, 8);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(msg_height), Constraint::Min(3)])
            .split(area);

        // Store sub-rects for mouse handling
        detail.msg_area = chunks[0];
        detail.file_list_area = chunks[1];

        // Draw commit message block
        let msg_block = panel::bordered_block(title, Color::Cyan);

        let msg_paragraph = Paragraph::new(detail.message.as_str())
            .style(fg_style(Color::White))
            .block(msg_block)
            .wrap(Wrap { trim: false })
            .scroll((detail.msg_scroll, 0));
        frame.render_widget(msg_paragraph, chunks[0]);

        // Draw file list block
        let files_block = panel::plain_block(Color::Cyan);

        if detail.files.is_empty() {
            let paragraph = Paragraph::new("No files changed")
                .style(fg_style(Color::DarkGray))
                .block(files_block);
            frame.render_widget(paragraph, chunks[1]);
            return;
        }

        let items: Vec<ListItem> = detail
            .files
            .iter()
            .map(|(status, path)| file_status_view::commit_file_item(status, path))
            .collect();

        frame.render_stateful_widget(
            panel::highlighted_list(items, files_block),
            chunks[1],
            &mut detail.file_state,
        );
    }

    fn draw_commit_diff(detail: &CommitDetail, frame: &mut Frame, area: Rect) {
        let Some(ref content) = detail.diff_content else {
            return;
        };

        diff_view::render_diff(
            frame,
            area,
            " Commit Diff (Esc to close) ".to_string(),
            content,
            detail.diff_scroll,
        );
    }
}

fn visible_diff_candidates(
    rows: &[GraphRow],
    diff_stat_cache: &HashMap<git2::Oid, crate::git::graph::DiffStat>,
    pending_diff_stats: &HashSet<git2::Oid>,
) -> Vec<git2::Oid> {
    rows.iter()
        .filter_map(|row| {
            if row.collapsed.is_some()
                || row.diff_stat.is_some()
                || diff_stat_cache.contains_key(&row.oid)
                || pending_diff_stats.contains(&row.oid)
            {
                None
            } else {
                Some(row.oid)
            }
        })
        .collect()
}

fn register_pending_oids(
    candidates: Vec<git2::Oid>,
    pending_diff_stats: &mut HashSet<git2::Oid>,
) -> Vec<git2::Oid> {
    candidates
        .into_iter()
        .filter(|oid| pending_diff_stats.insert(*oid))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::graph::GraphRow;
    use crate::git::test_support;

    fn mock_row(short_id: &str, message: &str, author: &str) -> GraphRow {
        test_support::graph_row_text(short_id, message, author)
    }

    fn graph_with_rows(rows: Vec<GraphRow>) -> GitGraph {
        let mut graph = GitGraph::new();
        graph.set_rows(rows);
        graph
    }

    fn search_matches(rows: Vec<GraphRow>, query: &str) -> GitGraph {
        let mut graph = graph_with_rows(rows);
        graph.search.input = query.to_string();
        graph.update_search_matches();
        graph
    }

    #[test]
    fn test_search_matches_message() {
        let graph = search_matches(
            vec![
                mock_row("abc1234", "fix: resolve crash", "Alice"),
                mock_row("def5678", "feat: add login", "Bob"),
                mock_row("ghi9012", "chore: update deps", "Alice"),
            ],
            "login",
        );
        assert_eq!(graph.search.matches, vec![1]);
    }

    #[test]
    fn test_search_matches_author() {
        let graph = search_matches(
            vec![
                mock_row("abc1234", "first", "Alice"),
                mock_row("def5678", "second", "Bob"),
                mock_row("ghi9012", "third", "Alice"),
            ],
            "alice",
        );
        assert_eq!(graph.search.matches, vec![0, 2]);
    }

    #[test]
    fn test_search_matches_short_id() {
        let graph = search_matches(
            vec![
                mock_row("abc1234", "first", "Alice"),
                mock_row("def5678", "second", "Bob"),
            ],
            "def",
        );
        assert_eq!(graph.search.matches, vec![1]);
    }

    #[test]
    fn test_search_case_insensitive() {
        let graph = search_matches(vec![mock_row("abc1234", "Fix Bug", "Alice")], "fix bug");
        assert_eq!(graph.search.matches, vec![0]);
    }

    #[test]
    fn test_search_next_wraps_around() {
        let mut graph = GitGraph::new();
        graph.set_rows(vec![
            mock_row("a", "match", "X"),
            mock_row("b", "no", "Y"),
            mock_row("c", "match", "Z"),
        ]);

        graph.search.input = "match".to_string();
        graph.update_search_matches();

        // matches = [0, 2]
        assert_eq!(graph.search.current_match, Some(0));

        graph.search_next();
        assert_eq!(graph.search.current_match, Some(1));
        assert_eq!(graph.state.selected(), Some(2)); // row index 2

        graph.search_next();
        assert_eq!(graph.search.current_match, Some(0)); // wraps
        assert_eq!(graph.state.selected(), Some(0));
    }

    #[test]
    fn test_search_prev_wraps_around() {
        let mut graph = GitGraph::new();
        graph.set_rows(vec![
            mock_row("a", "match", "X"),
            mock_row("b", "no", "Y"),
            mock_row("c", "match", "Z"),
        ]);

        graph.search.input = "match".to_string();
        graph.update_search_matches();

        // Start at match 0
        graph.search_prev();
        assert_eq!(graph.search.current_match, Some(1)); // wraps to last
        assert_eq!(graph.state.selected(), Some(2));
    }

    #[test]
    fn test_search_empty_input_no_matches() {
        let mut graph = graph_with_rows(vec![mock_row("a", "hello", "X")]);

        graph.search.input.clear();
        graph.update_search_matches();

        assert!(graph.search.matches.is_empty());
        assert_eq!(graph.search.current_match, None);
    }

    #[test]
    fn test_search_no_results() {
        let graph = search_matches(vec![mock_row("a", "hello", "Alice")], "zzzzz");
        assert!(graph.search.matches.is_empty());
        assert_eq!(graph.search.current_match, None);
    }

    /// Standard topology for collapse tests:
    /// Row 0: main0 (col=0, parents=[], labels=["main"])  ← main trunk
    /// Row 1: tip   (col=1, parents=[mid], labels)         ← side branch tip
    /// Row 2: mid   (col=1, parents=[main0])               ← side branch base
    fn make_branch_rows(tip_labels: Vec<crate::git::graph::BranchLabel>) -> Vec<GraphRow> {
        vec![
            test_support::dag_row('1', "m", &[], vec![test_support::branch_label("main")]),
            test_support::dag_row('a', "a", &['b'], tip_labels),
            test_support::dag_row('b', "b", &['1'], vec![]),
        ]
    }

    fn collapse_branch_rows(
        tip_labels: Vec<crate::git::graph::BranchLabel>,
        selected: usize,
    ) -> GitGraph {
        let mut graph = graph_with_rows(make_branch_rows(tip_labels));
        graph.state.select(Some(selected));
        graph.toggle_collapse_selected();
        graph
    }

    #[test]
    fn test_collapse_labeled_branch() {
        let graph = collapse_branch_rows(vec![test_support::branch_label("feature")], 1);

        assert!(
            graph
                .collapsed_branches
                .contains(test_support::oid_id('a').as_str())
        );
        // main0 + placeholder = 2 rows
        assert_eq!(graph.rows.len(), 2);
        let (_, count) = graph.rows[1].collapsed.as_ref().unwrap();
        assert_eq!(*count, 2);
        assert!(graph.rows[1].message.contains("feature"));
    }

    #[test]
    fn test_collapse_unlabeled_merge_lane() {
        let graph = collapse_branch_rows(vec![], 2);

        assert!(
            graph
                .collapsed_branches
                .contains(test_support::oid_id('a').as_str())
        );
        assert_eq!(graph.rows.len(), 2);
        // Placeholder uses short OID since there's no label
        assert!(graph.rows[1].message.contains("a"));
    }

    #[test]
    fn test_expand_collapsed_group() {
        let mut graph = GitGraph::new();
        graph.set_rows(make_branch_rows(vec![test_support::branch_label(
            "feature",
        )]));
        graph.state.select(Some(1));
        graph.toggle_collapse_selected();
        assert_eq!(graph.rows.len(), 2);

        // Select the placeholder and toggle to expand
        graph.state.select(Some(1));
        graph.toggle_collapse_selected();

        assert!(graph.collapsed_branches.is_empty());
        assert_eq!(graph.display_rows().len(), 3);
    }

    #[test]
    fn test_collapse_from_middle_of_branch() {
        let graph = collapse_branch_rows(vec![test_support::branch_label("feature")], 2);

        assert!(
            graph
                .collapsed_branches
                .contains(test_support::oid_id('a').as_str())
        );
        assert_eq!(graph.rows.len(), 2);
        assert!(graph.rows[1].collapsed.is_some());
    }

    #[test]
    fn test_expand_all() {
        let mut graph = GitGraph::new();
        graph.set_rows(make_branch_rows(vec![test_support::branch_label("feat-a")]));
        graph.state.select(Some(1));
        graph.toggle_collapse_selected();
        assert!(!graph.collapsed_branches.is_empty());

        graph.expand_all_branches();
        assert!(graph.collapsed_branches.is_empty());
        assert_eq!(graph.display_rows().len(), 3);
    }

    #[test]
    fn test_main_trunk_not_collapsible() {
        let graph = collapse_branch_rows(vec![], 0);

        assert!(graph.collapsed_branches.is_empty());
        assert_eq!(graph.display_rows().len(), 3);
    }

    #[test]
    fn test_interleaved_commits_collapse_together() {
        // Row 0: main0 (col=0, parents=[main1])
        // Row 1: tip_x (col=1, parents=[base_x]) -- branch X
        // Row 2: main1 (col=0, parents=[])        -- main trunk
        // Row 3: base_x (col=1, parents=[main0])  -- branch X (interleaved with main1)
        let mut graph = GitGraph::new();
        graph.set_rows(vec![
            test_support::dag_row('1', "m0", &['c'], vec![test_support::branch_label("main")]),
            test_support::dag_row('a', "a", &['b'], vec![]),
            test_support::dag_row('c', "c", &[], vec![]),
            test_support::dag_row('b', "b", &['1'], vec![]),
        ]);

        // Select row 1 (tip of branch X)
        graph.state.select(Some(1));
        graph.toggle_collapse_selected();

        assert!(
            graph
                .collapsed_branches
                .contains(test_support::oid_id('a').as_str())
        );
        // Rows 1 and 3 (non-contiguous) should both be collapsed
        // main0 + placeholder + main1 = 3 rows
        assert_eq!(graph.rows.len(), 3);
        let (_, count) = graph.rows[1].collapsed.as_ref().unwrap();
        assert_eq!(*count, 2);
    }

    #[test]
    fn test_unlabeled_branch_collapsible() {
        let graph = collapse_branch_rows(vec![], 1);

        assert!(!graph.collapsed_branches.is_empty());
        // Placeholder uses short OID as display name
        let placeholder = &graph.rows[1];
        assert!(placeholder.collapsed.is_some());
        assert!(placeholder.message.contains("a")); // short_id of tip
    }
}
