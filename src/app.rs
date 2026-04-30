use color_eyre::Result;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::action::Action;
use crate::components::Component;
use crate::components::commit_input::CommitInput;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::{ContextMenu, RepoMenuState};
use crate::components::file_list::FileList;
use crate::components::git_graph::GitGraph;
use crate::components::github_repo_input::GitHubRepoInput;
use crate::components::notice_dialog::NoticeDialog;
use crate::components::path_input::PathInput;
use crate::components::repo_list::RepoEntry;
use crate::components::repo_list::RepoList;
use crate::components::status_bar::StatusBar;
use crate::config::Config;
use crate::config::UpdatePosition;
use crate::event::Event;
use crate::git::graph::GraphOptions;
use crate::git::scanner;
use crate::repo_id::RepoId;
use crate::tui::Tui;
use crate::watcher::RepoWatcher;

mod git_ops;
mod helpers;
mod input;
mod perf;
mod ui;

use helpers::{
    ActiveWorktree, StatusFailure, StatusGuard, StatusQuery, base64_encode, git_args,
    is_valid_github_repo_name, open_browser_url,
};

const HELP_PAGE_COUNT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusPanel {
    Repos,
    Changes,
    Graph,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortOrder {
    Alphabetical,
    DirtyFirst,
}

impl SortOrder {
    fn next(self) -> Self {
        match self {
            Self::Alphabetical => Self::DirtyFirst,
            Self::DirtyFirst => Self::Alphabetical,
        }
    }
}

pub(crate) struct App {
    config: Config,
    should_quit: bool,
    repo_list: RepoList,
    file_list: FileList,
    git_graph: GitGraph,
    confirm_dialog: ConfirmDialog,
    notice_dialog: NoticeDialog,
    commit_input: CommitInput,
    github_repo_input: GitHubRepoInput,
    context_menu: ContextMenu,
    path_input: PathInput,
    status_bar: StatusBar,
    focus: FocusPanel,
    sort_order: SortOrder,
    action_tx: UnboundedSender<Action>,
    action_rx: UnboundedReceiver<Action>,
    repo_area: Rect,
    changes_area: Rect,
    graph_area: Rect,
    error_message: Option<(String, Instant)>,
    success_message: Option<(String, Instant)>,
    /// Which border is being dragged: 0 = repos/changes, 1 = changes/graph
    dragging_border: Option<u8>,
    /// Fraction of the vertical layout axis for each border (0.0..1.0).
    /// [0] = repos/changes split, [1] = changes/graph split.
    border_frac: [f64; 2],
    /// Single-repo workspace entered from double click / Enter on a repo row.
    focused_repo: Option<RepoId>,
    operation_log: VecDeque<String>,
    show_operation_log: bool,
    /// Newer version available (set by background update check)
    update_version: Option<String>,
    /// Where to render the update notification
    update_position: UpdatePosition,
    /// Show the keybindings help overlay
    show_help: bool,
    help_page: usize,
    /// Limits concurrent poll/refresh tasks to avoid CPU spikes
    poll_semaphore: Arc<tokio::sync::Semaphore>,
    /// Repos with an in-flight status query (prevents duplicate spawns)
    pending_status: HashSet<RepoId>,
    /// Repos that changed while a status query was in-flight (re-queued on completion)
    dirty_repos: HashSet<RepoId>,
    /// Last graph snapshot per repo path; unchanged snapshots skip graph reloads.
    graph_keys: HashMap<PathBuf, String>,
    /// Last remote snapshot per repo path; changed snapshots refresh pushed markers.
    remote_keys: HashMap<PathBuf, String>,
    /// Runtime performance mode. Toggle with `F` or start with `--fast`.
    fast_mode: bool,
    /// Number of local poll ticks since startup, used to occasionally run a full scan.
    local_poll_tick: usize,
    /// When a worktree row is selected, stores context for diff/status routing
    /// and live-polling the worktree's changes.
    active_worktree: Option<ActiveWorktree>,
}

impl App {
    pub fn new(config: Config) -> Self {
        let repo_paths = scanner::discover_repos(&config);
        let (action_tx, action_rx) = mpsc::unbounded_channel();

        let fast_mode = config.performance.fast_mode;
        let mut git_graph = GitGraph::new();
        git_graph.graph_options = GraphOptions {
            branch_filter: config.graph.branches,
            label_max_len: config.graph.label_max_len,
            first_parent: false,
            show_stats: config.graph.show_stats && !fast_mode,
        };

        let update_position = config.ui.update_position;
        let ignore_dirty_subs = config.submodules.ignore_dirty;
        let poll_semaphore = Arc::new(tokio::sync::Semaphore::new(
            config.watch.max_concurrent_polls,
        ));

        Self {
            config,
            should_quit: false,
            repo_list: RepoList::new(repo_paths, ignore_dirty_subs),
            file_list: FileList::new(),
            git_graph,
            confirm_dialog: ConfirmDialog::new(),
            notice_dialog: NoticeDialog::new(),
            commit_input: CommitInput::new(),
            github_repo_input: GitHubRepoInput::new(),
            context_menu: ContextMenu::new(),
            path_input: PathInput::new(),
            status_bar: StatusBar::new(),
            focus: FocusPanel::Repos,
            sort_order: SortOrder::Alphabetical,
            action_tx,
            action_rx,
            repo_area: Rect::default(),
            changes_area: Rect::default(),
            graph_area: Rect::default(),
            error_message: None,
            success_message: None,
            dragging_border: None,
            border_frac: [0.25, 0.50],
            focused_repo: None,
            operation_log: VecDeque::new(),
            show_operation_log: false,
            update_version: None,
            update_position,
            show_help: false,
            help_page: 0,
            poll_semaphore,
            pending_status: HashSet::new(),
            dirty_repos: HashSet::new(),
            graph_keys: HashMap::new(),
            remote_keys: HashMap::new(),
            fast_mode,
            local_poll_tick: 0,
            active_worktree: None,
        }
    }

    fn sort_repos(&mut self) {
        match self.sort_order {
            SortOrder::Alphabetical => {
                self.repo_list.repos.sort_by_key(|r| r.name.to_lowercase());
            }
            SortOrder::DirtyFirst => {
                self.repo_list.repos.sort_by(|a, b| {
                    let a_dirty = a.status.as_ref().map(|s| s.is_dirty).unwrap_or(false);
                    let b_dirty = b.status.as_ref().map(|s| s.is_dirty).unwrap_or(false);
                    b_dirty
                        .cmp(&a_dirty)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
        }
        // Reset selection to first
        if !self.repo_list.repos.is_empty() {
            self.repo_list.select_repo_row(0);
        }
    }

    /// Auto-load graph + file list for the selected repo.
    fn sync_selection(&mut self) {
        if let Some(idx) = self.repo_list.selected_index()
            && let Some(entry) = self.repo_list.repos.get(idx)
        {
            let name = entry.name.clone();
            let repo_id = RepoId(entry.path.clone());
            let files = entry
                .status
                .as_ref()
                .map(|s| s.files.clone())
                .unwrap_or_default();
            if let Some(status) = &entry.status {
                self.graph_keys
                    .insert(entry.path.clone(), status.graph_key.clone());
                self.remote_keys
                    .insert(entry.path.clone(), status.remote_key.clone());
            }
            self.file_list.set_files(files, &name, repo_id);

            let path = entry.path.clone();
            self.git_graph.load_repo(path, &name);
        }
    }

    fn open_repo_graph(&mut self, id: &RepoId) {
        self.context_menu.hide();
        self.active_worktree = None;
        if let Some(idx) = self.repo_list.resolve_index(id) {
            self.repo_list.select_repo_row(idx);
            let entry = &self.repo_list.repos[idx];
            let name = entry.name.clone();
            let path = entry.path.clone();
            let files = entry
                .status
                .as_ref()
                .map(|s| s.files.clone())
                .unwrap_or_default();
            if let Some(status) = &entry.status {
                self.graph_keys
                    .insert(path.clone(), status.graph_key.clone());
                self.remote_keys
                    .insert(path.clone(), status.remote_key.clone());
            }
            self.file_list.set_files(files, &name, id.clone());
            self.git_graph.load_repo(path, &name);
            self.focused_repo = Some(id.clone());
            self.focus = FocusPanel::Graph;
        }
    }

    fn active_repo_id(&self) -> Option<RepoId> {
        self.focused_repo.clone().or_else(|| {
            self.repo_list
                .selected_index()
                .map(|idx| RepoId(self.repo_list.repos[idx].path.clone()))
        })
    }

    fn focused_repo_name(&self) -> Option<String> {
        self.focused_repo
            .as_ref()
            .and_then(|id| self.repo_list.resolve_index(id))
            .map(|idx| self.repo_list.repos[idx].name.clone())
    }

    fn add_operation_log(&mut self, message: impl Into<String>) {
        if self.operation_log.len() >= 50 {
            self.operation_log.pop_front();
        }
        self.operation_log.push_back(message.into());
    }

    fn spawn_status_query(
        &self,
        repo_id: RepoId,
        path: PathBuf,
        query: StatusQuery,
        failure: StatusFailure,
    ) {
        let tx = self.action_tx.clone();
        let sem = self.poll_semaphore.clone();
        let ignore_dirty_subs = self.config.submodules.ignore_dirty;
        let untracked = self.effective_untracked_mode();

        tokio::spawn(async move {
            let _permit = sem.acquire().await;
            let guard = StatusGuard::new(repo_id.clone(), tx.clone());
            tokio::task::spawn_blocking(move || {
                let result = match query {
                    StatusQuery::Local => crate::git::status::query_status_with_untracked(
                        &path,
                        ignore_dirty_subs,
                        untracked,
                    ),
                    StatusQuery::Fetch => {
                        crate::git::status::query_status_with_fetch_and_untracked(
                            &path,
                            ignore_dirty_subs,
                            untracked,
                        )
                    }
                };

                match result {
                    Ok(status) => {
                        let _ = tx.send(Action::RepoStatusUpdated {
                            id: repo_id,
                            status,
                        });
                        guard.complete();
                    }
                    Err(e) => {
                        guard.complete();
                        let _ = tx.send(Action::StatusQueryDone(repo_id));
                        match failure {
                            StatusFailure::UserVisible => {
                                let _ = tx.send(Action::Error(format!("Failed to query: {}", e)));
                            }
                            StatusFailure::Debug(prefix) => {
                                tracing::debug!("{} for {}: {}", prefix, path.display(), e);
                            }
                        }
                    }
                }
            })
            .await
        });
    }

    fn spawn_worktree_status_query(&self, worktree: ActiveWorktree) {
        let tx = self.action_tx.clone();
        let sem = self.poll_semaphore.clone();
        let ignore_dirty_subs = self.config.submodules.ignore_dirty;
        let untracked = self.effective_untracked_mode();

        tokio::spawn(async move {
            let _permit = sem.acquire().await;
            tokio::task::spawn_blocking(move || {
                match crate::git::status::query_status_with_untracked(
                    &worktree.path,
                    ignore_dirty_subs,
                    untracked,
                ) {
                    Ok(status) => {
                        let _ = tx.send(Action::WorktreeFilesLoaded {
                            repo_id: worktree.repo_id,
                            worktree_path: worktree.path,
                            name: worktree.display_name,
                            files: status.files,
                            graph_key: status.graph_key,
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(Action::Error(format!("Worktree status: {}", e)));
                    }
                }
            })
            .await
        });
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut tui = Tui::new()?
            .mouse(true)
            .poll_local_interval(std::time::Duration::from_secs(
                self.config.watch.poll_local_secs,
            ))
            .poll_fetch_interval(std::time::Duration::from_secs(
                self.config.watch.poll_fetch_secs,
            ));
        tui.enter()?;

        // Register action handlers
        self.repo_list
            .register_action_handler(self.action_tx.clone())?;
        self.file_list
            .register_action_handler(self.action_tx.clone())?;
        self.git_graph
            .register_action_handler(self.action_tx.clone())?;
        self.context_menu
            .register_action_handler(self.action_tx.clone())?;

        // Init components
        self.repo_list.init()?;

        // Trigger immediate status poll so repos don't show "..." until the
        // first PollLocal timer fires. Goes through the semaphore-controlled path.
        self.action_tx.send(Action::PollLocal)?;

        // Start filesystem watcher
        let repo_paths: Vec<_> = self
            .repo_list
            .repos
            .iter()
            .map(|r| r.path.clone())
            .collect();
        let _watcher = RepoWatcher::new(
            &repo_paths,
            self.config.watch.debounce_ms,
            tui.event_tx.clone(),
            &self.config.watch.watch_exclude_dirs,
        )?;

        // Check for updates in the background
        if self.config.ui.check_for_updates {
            let tx = self.action_tx.clone();
            tokio::task::spawn_blocking(move || {
                if let Some(version) = crate::update_checker::check_latest() {
                    let _ = tx.send(Action::UpdateAvailable(version));
                }
            });
        }

        // Auto-select the first repo (graph loads once status arrives)
        self.sync_selection();

        loop {
            let mut render_requested = false;

            // Process events from TUI
            if let Some(event) = tui.event_rx.recv().await {
                match event {
                    Event::Quit => {
                        self.action_tx.send(Action::Quit)?;
                    }
                    Event::Tick => {
                        self.action_tx.send(Action::Tick)?;
                    }
                    Event::Render => {
                        render_requested = true;
                    }
                    Event::Key(key) => {
                        self.handle_key_event(key)?;
                        render_requested = true;
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse_event(mouse)?;
                        render_requested = true;
                    }
                    Event::Resize(w, h) => {
                        self.action_tx.send(Action::Resize(w, h))?;
                    }
                    Event::RepoChanged(ref path) => {
                        self.action_tx
                            .send(Action::RefreshRepo(RepoId(path.clone())))?;
                    }
                    Event::PollLocal => {
                        self.action_tx.send(Action::PollLocal)?;
                    }
                    Event::PollFetch => {
                        self.action_tx.send(Action::PollFetch)?;
                    }
                    Event::FocusGained => {
                        if let Some(entry) = self.repo_list.selected_repo() {
                            self.action_tx
                                .send(Action::RefreshRepo(RepoId(entry.path.clone())))?;
                        }
                    }
                    _ => {}
                }
            }

            // Process actions
            while let Ok(action) = self.action_rx.try_recv() {
                let render_after = !matches!(&action, Action::Tick | Action::Render);
                match action {
                    Action::Tick => {
                        if self.expire_messages() {
                            render_requested = true;
                        }
                    }
                    Action::Quit => {
                        self.should_quit = true;
                    }
                    Action::Render => {
                        render_requested = true;
                    }
                    Action::Resize(w, h) => {
                        tui.terminal
                            .resize(ratatui::layout::Rect::new(0, 0, w, h))?;
                    }
                    Action::SelectRepo(ref id) => {
                        if self.focused_repo.is_some() {
                            continue;
                        }
                        self.context_menu.hide();
                        self.active_worktree = None;
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &self.repo_list.repos[idx];
                            let name = entry.name.clone();
                            let path = entry.path.clone();
                            let repo_id = id.clone();
                            let files = entry
                                .status
                                .as_ref()
                                .map(|s| s.files.clone())
                                .unwrap_or_default();
                            if let Some(status) = &entry.status {
                                self.graph_keys
                                    .insert(path.clone(), status.graph_key.clone());
                            }
                            self.file_list.set_files(files, &name, repo_id);
                            self.git_graph.load_repo(path, &name);
                            self.repo_list.select_repo_row(idx);
                        }
                    }
                    Action::SelectWorktree {
                        ref repo_id,
                        ref worktree_path,
                        ref worktree_branch,
                    } => {
                        self.context_menu.hide();

                        let repo_name = self
                            .repo_list
                            .resolve_index(repo_id)
                            .map(|i| self.repo_list.repos[i].name.clone())
                            .unwrap_or_default();
                        let display_name = format!("{}:{}", repo_name, worktree_branch);

                        self.active_worktree = Some(ActiveWorktree {
                            path: worktree_path.clone(),
                            repo_id: repo_id.clone(),
                            display_name: display_name.clone(),
                            graph_key: crate::git::status::graph_cache_key(worktree_path).ok(),
                        });

                        // Clear file list while loading (use parent repo_id for resolve_index)
                        self.file_list
                            .set_files(Vec::new(), &display_name, repo_id.clone());

                        // Load graph from worktree path
                        self.git_graph
                            .load_repo(worktree_path.clone(), &display_name);

                        // Query worktree status in background
                        if let Some(worktree) = self.active_worktree.clone() {
                            self.spawn_worktree_status_query(worktree);
                        }
                    }
                    Action::WorktreeFilesLoaded {
                        repo_id,
                        worktree_path,
                        name,
                        files,
                        graph_key,
                    } => {
                        // Only apply if this worktree is still selected
                        if self
                            .active_worktree
                            .as_ref()
                            .is_some_and(|aw| aw.path == worktree_path)
                        {
                            self.file_list.set_files(files, &name, repo_id.clone());

                            let graph_changed = self
                                .active_worktree
                                .as_ref()
                                .is_some_and(|aw| aw.graph_key.as_deref() != Some(&graph_key));
                            if graph_changed {
                                if let Some(aw) = self.active_worktree.as_mut() {
                                    aw.graph_key = Some(graph_key);
                                }
                                if self.git_graph.has_detail() {
                                    self.git_graph.set_needs_reload();
                                } else {
                                    self.git_graph.load_repo(worktree_path, &name);
                                }
                            }
                        }
                    }
                    Action::StatusQueryDone(ref id) => {
                        self.pending_status.remove(id);
                        // Clear git_op so the repo isn't permanently skipped
                        // by future polls after a failed status query.
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            self.repo_list.repos[idx].git_op = false;
                        }
                        if self.dirty_repos.remove(id) {
                            self.action_tx.send(Action::RefreshRepo(id.clone()))?;
                        }
                    }
                    Action::RepoStatusUpdated { id, status } => {
                        self.pending_status.remove(&id);
                        let is_dirty = self.dirty_repos.remove(&id);
                        if let Some(idx) = self.repo_list.resolve_index(&id) {
                            let repo_path = self.repo_list.repos[idx].path.clone();
                            let graph_changed =
                                self.repo_graph_changed(repo_path.clone(), &status.graph_key);
                            let remote_changed =
                                self.repo_remote_changed(repo_path.clone(), &status.remote_key);
                            let selected_repo = self.repo_list.selected_index() == Some(idx)
                                && self.active_worktree.is_none();
                            let selected_files = selected_repo.then(|| status.files.clone());
                            let selected_name = self.repo_list.repos[idx].name.clone();
                            self.repo_list.update_status(idx, status);

                            // Refresh the file list so stale diffs are cleared
                            // when files are staged/unstaged. Skip when a worktree
                            // is being viewed — its files come from WorktreeFilesLoaded,
                            // not the parent repo's status.
                            if selected_repo {
                                self.file_list.set_files(
                                    selected_files.unwrap_or_default(),
                                    &selected_name,
                                    id.clone(),
                                );

                                if graph_changed {
                                    if self.git_graph.has_detail() {
                                        self.git_graph.set_needs_reload();
                                    } else {
                                        self.git_graph.load_repo(repo_path, &selected_name);
                                    }
                                } else if self.git_graph.current_generation() == 0 {
                                    self.git_graph.load_repo(repo_path, &selected_name);
                                } else if remote_changed {
                                    self.git_graph.refresh_pushed_status();
                                }
                            } else if remote_changed {
                                self.git_graph.refresh_pushed_status_for_path(repo_path);
                            }
                        }
                        if is_dirty {
                            self.action_tx.send(Action::RefreshRepo(id))?;
                        }
                    }
                    Action::RefreshAll => {
                        // User-initiated refresh: fetch from remote + show spinner
                        let mut queries = Vec::new();
                        for entry in self.repo_list.repos.iter_mut() {
                            entry.git_op = true;
                            let repo_id = RepoId(entry.path.clone());
                            self.pending_status.insert(repo_id.clone());
                            queries.push((repo_id, entry.path.clone()));
                        }
                        for (repo_id, path) in queries {
                            self.spawn_status_query(
                                repo_id,
                                path,
                                StatusQuery::Fetch,
                                StatusFailure::UserVisible,
                            );
                        }
                    }
                    Action::PollLocal => {
                        // Fast local status poll (no network, no spinner)
                        let full_every = self.config.watch.poll_local_full_every.max(1);
                        let full_scan = self.local_poll_tick == 0
                            || self.local_poll_tick.is_multiple_of(full_every);
                        self.local_poll_tick = self.local_poll_tick.saturating_add(1);

                        for (idx, entry) in self.repo_list.repos.iter().enumerate() {
                            let repo_id = RepoId(entry.path.clone());
                            if entry.git_op || self.pending_status.contains(&repo_id) {
                                continue;
                            }
                            if !self.should_poll_repo(&repo_id, idx, full_scan) {
                                continue;
                            }
                            self.pending_status.insert(repo_id.clone());
                            let path = entry.path.clone();
                            self.spawn_status_query(
                                repo_id,
                                path,
                                StatusQuery::Local,
                                StatusFailure::Debug("Local poll failed"),
                            );
                        }

                        // Also re-query the active worktree so its changes update live
                        if let Some(aw) = self.active_worktree.clone() {
                            self.spawn_worktree_status_query(aw);
                        }
                    }
                    Action::PollFetch => {
                        // Remote fetch poll (updates ahead/behind, no spinner)
                        if self.fast_mode {
                            continue;
                        }
                        for entry in self.repo_list.repos.iter() {
                            let repo_id = RepoId(entry.path.clone());
                            if entry.git_op || self.pending_status.contains(&repo_id) {
                                continue;
                            }
                            self.pending_status.insert(repo_id.clone());
                            let path = entry.path.clone();
                            self.spawn_status_query(
                                repo_id,
                                path,
                                StatusQuery::Fetch,
                                StatusFailure::Debug("Fetch poll failed"),
                            );
                        }
                    }
                    Action::RefreshRepo(ref id) => {
                        // Watcher-triggered: fast local-only, no spinner
                        if self.pending_status.contains(id) {
                            self.dirty_repos.insert(id.clone());
                            tracing::debug!(
                                "skipping repo {}: already in-flight (marked dirty)",
                                id
                            );
                            continue;
                        }
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let repo_id = id.clone();
                            self.pending_status.insert(repo_id.clone());
                            let path = self.repo_list.repos[idx].path.clone();
                            self.spawn_status_query(
                                repo_id,
                                path,
                                StatusQuery::Local,
                                StatusFailure::UserVisible,
                            );
                        }
                    }
                    Action::ShowGitGraph => {
                        if let Some(entry) = self.repo_list.selected_repo() {
                            let id = RepoId(entry.path.clone());
                            self.open_repo_graph(&id);
                        }
                    }
                    Action::ShowRepoGitGraph(ref id) => {
                        self.open_repo_graph(id);
                    }
                    Action::ShowFileList => {
                        self.focus = FocusPanel::Changes;
                    }
                    Action::GraphLoaded { generation, rows } => {
                        if generation == self.git_graph.current_generation() {
                            self.git_graph.set_rows(rows);
                        }
                    }
                    Action::PushedStatusLoaded { generation, oids } => {
                        if generation == self.git_graph.current_generation() {
                            self.git_graph.set_pushed_oids(oids);
                        }
                    }
                    Action::DiffStatsLoaded { generation, stats } => {
                        if generation == self.git_graph.current_generation() {
                            self.git_graph.set_diff_stats(stats);
                        }
                    }
                    Action::GraphError(ref msg) => {
                        self.git_graph.set_error(msg.clone());
                    }
                    Action::ShowContextMenu { ref id, row, col } => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let (
                                ahead,
                                behind,
                                has_upstream,
                                has_submodules,
                                has_github_remote,
                                has_origin_remote,
                            ) = self.repo_list.repos[idx]
                                .status
                                .as_ref()
                                .map(|s| {
                                    (
                                        s.ahead,
                                        s.behind,
                                        s.has_upstream,
                                        s.has_submodules,
                                        s.has_github_remote,
                                        s.has_origin_remote,
                                    )
                                })
                                .unwrap_or((0, 0, false, false, false, false));
                            self.context_menu.show(
                                id.clone(),
                                col,
                                row,
                                RepoMenuState {
                                    ahead,
                                    behind,
                                    has_upstream,
                                    has_submodules,
                                    has_github_remote,
                                    has_origin_remote,
                                },
                            );
                        }
                    }
                    Action::HideContextMenu => {
                        self.context_menu.hide();
                    }
                    Action::ToggleOperationLog => {
                        self.show_operation_log = !self.show_operation_log;
                    }
                    Action::ToggleFastMode => {
                        self.toggle_fast_mode();
                        self.action_tx.send(Action::PollLocal)?;
                    }
                    Action::CopyPath(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &self.repo_list.repos[idx];
                            let path_str = entry.path.to_string_lossy().to_string();
                            use std::io::Write;
                            let encoded = base64_encode(path_str.as_bytes());
                            let _ = write!(std::io::stdout(), "\x1b]52;c;{}\x1b\\", encoded);
                            let _ = std::io::stdout().flush();
                        }
                    }
                    Action::OpenGitHub(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let url = self.repo_list.repos[idx]
                                .status
                                .as_ref()
                                .and_then(|status| status.github_url.as_deref());
                            match url {
                                Some(url) => {
                                    if let Err(err) = open_browser_url(url) {
                                        self.action_tx.send(Action::Notice(format!(
                                            "Open GitHub failed: {err}"
                                        )))?;
                                    }
                                }
                                None => {
                                    self.action_tx.send(Action::Notice(
                                        "This repo has no GitHub remote".to_string(),
                                    ))?;
                                }
                            }
                        }
                    }
                    Action::StartCommit(ref id) => {
                        self.context_menu.hide();
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &self.repo_list.repos[idx];
                            self.commit_input.show(id.clone(), entry.name.clone());
                        }
                    }
                    Action::UpdateCommitMessage(ref _message) => {}
                    Action::CancelCommit => {
                        self.commit_input.hide();
                    }
                    Action::StartCreateGitHubRepo { ref id, private } => {
                        self.context_menu.hide();
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &self.repo_list.repos[idx];
                            self.github_repo_input
                                .show(id.clone(), private, entry.name.clone());
                        }
                    }
                    Action::UpdateGitHubRepoName(ref _name) => {}
                    Action::CancelCreateGitHubRepo => {
                        self.github_repo_input.hide();
                    }
                    Action::ConfirmCreateGitHubRepo => {
                        let Some(repo_id) = self.github_repo_input.repo_id() else {
                            continue;
                        };
                        let repo_name = self.github_repo_input.name().trim().to_string();
                        if repo_name.is_empty() {
                            self.error_message =
                                Some(("Repository name is empty".to_string(), Instant::now()));
                            continue;
                        }
                        if !is_valid_github_repo_name(&repo_name) {
                            self.error_message = Some((
                                "Repository name can only contain letters, numbers, '.', '_' and '-'"
                                    .to_string(),
                                Instant::now(),
                            ));
                            continue;
                        }
                        self.action_tx.send(Action::CreateGitHubRepo {
                            id: repo_id,
                            private: self.github_repo_input.private(),
                            name: repo_name,
                        })?;
                        self.github_repo_input.hide();
                    }
                    Action::ConfirmCommit => {
                        let Some(repo_id) = self.commit_input.repo_id() else {
                            continue;
                        };
                        let message = self.commit_input.message().trim().to_string();
                        if message.is_empty() {
                            self.error_message =
                                Some(("Commit message is empty".to_string(), Instant::now()));
                            continue;
                        }
                        if let Some(idx) = self.repo_list.resolve_index(&repo_id) {
                            let entry = &mut self.repo_list.repos[idx];
                            entry.git_op = true;
                            let path = entry.path.clone();
                            let repo_name = entry.name.clone();
                            let no_verify = self.config.commit.no_verify;
                            self.commit_input.hide();
                            self.spawn_git_operation(
                                repo_id,
                                move || crate::git::commit_all(&path, &message, no_verify),
                                move |_| format!("Committed {}", repo_name),
                                Some("Commit failed"),
                            );
                        }
                    }
                    Action::GitPush(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &mut self.repo_list.repos[idx];
                            let (branch, has_upstream, ahead) = entry
                                .status
                                .as_ref()
                                .map(|s| (s.branch.clone(), s.has_upstream, s.ahead))
                                .unwrap_or_default();
                            if branch.is_empty() || branch == "(no branch)" || branch == "HEAD" {
                                self.error_message =
                                    Some(("Cannot push detached HEAD".to_string(), Instant::now()));
                                continue;
                            }
                            if !has_upstream {
                                self.error_message = Some((
                                    format!(
                                        "Branch '{branch}' has no upstream; press P to publish"
                                    ),
                                    Instant::now(),
                                ));
                                continue;
                            }
                            if ahead == 0 {
                                self.success_message =
                                    Some(("Nothing to push".to_string(), Instant::now()));
                                continue;
                            }
                            entry.git_op = true;
                            let push_target = entry.display_name();
                            self.success_message =
                                Some((format!("Pushing to {push_target}..."), Instant::now()));
                            let path = entry.path.clone();
                            let repo_id = id.clone();
                            self.spawn_git_operation(
                                repo_id,
                                move || crate::git::push(&path),
                                |_| "git push succeeded".to_string(),
                                None,
                            );
                        }
                    }
                    Action::GitPublish(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &mut self.repo_list.repos[idx];
                            let branch = entry
                                .status
                                .as_ref()
                                .map(|s| s.branch.clone())
                                .unwrap_or_default();
                            entry.git_op = true;
                            let publish_target = entry.display_name();
                            self.success_message =
                                Some((format!("Publishing {publish_target}..."), Instant::now()));
                            let path = entry.path.clone();
                            let repo_id = id.clone();
                            let success_branch = branch.clone();
                            self.spawn_git_operation(
                                repo_id,
                                move || crate::git::publish(&path, &branch),
                                move |_| format!("Published {success_branch}"),
                                Some("Publish failed"),
                            );
                        }
                    }
                    Action::CreateGitHubRepo {
                        ref id,
                        private,
                        ref name,
                    } => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &mut self.repo_list.repos[idx];
                            entry.git_op = true;
                            let visibility = if private { "private" } else { "public" };
                            self.success_message = Some((
                                format!("Creating {visibility} GitHub repo {name}..."),
                                Instant::now(),
                            ));
                            let path = entry.path.clone();
                            let repo_id = id.clone();
                            let repo_name = name.clone();
                            let success_name = name.clone();
                            self.spawn_git_operation(
                                repo_id,
                                move || crate::git::create_github_repo(&path, &repo_name, private),
                                move |_| format!("Created {visibility} GitHub repo {success_name}"),
                                Some("Create GitHub repo failed"),
                            );
                        }
                    }
                    Action::GitPullRebase(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let name = self.repo_list.repos[idx].name.clone();
                            self.confirm_dialog.show(
                                format!("Pull --rebase {name}?"),
                                Action::RunGitPullRebase(id.clone()),
                            );
                        }
                    }
                    Action::GitPullSubmodules(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let name = self.repo_list.repos[idx].name.clone();
                            self.confirm_dialog.show(
                                format!("Pull submodules for {name}?"),
                                Action::RunGitPullSubmodules(id.clone()),
                            );
                        }
                    }
                    Action::RemoveOriginRemote(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let name = self.repo_list.repos[idx].name.clone();
                            self.confirm_dialog.show(
                                format!("Remove origin remote from {name}?"),
                                Action::RunRemoveOriginRemote(id.clone()),
                            );
                        }
                    }
                    Action::GitPull(ref id)
                    | Action::RunGitPullRebase(ref id)
                    | Action::RunGitPullSubmodules(ref id)
                    | Action::RunRemoveOriginRemote(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &mut self.repo_list.repos[idx];
                            let branch = entry
                                .status
                                .as_ref()
                                .map(|s| s.branch.clone())
                                .unwrap_or_default();
                            let should_add_origin_branch =
                                !matches!(&action, Action::RunRemoveOriginRemote(_));
                            let mut git_args = match action {
                                Action::GitPull(_) => git_args(&["pull"]),
                                Action::RunGitPullRebase(_) => git_args(&["pull", "--rebase"]),
                                Action::RunGitPullSubmodules(_) => {
                                    git_args(&["pull", "--recurse-submodules"])
                                }
                                Action::RunRemoveOriginRemote(_) => {
                                    git_args(&["remote", "remove", "origin"])
                                }
                                _ => unreachable!(),
                            };
                            // Add origin <branch> so pull/push works even without upstream config
                            if should_add_origin_branch
                                && !branch.is_empty()
                                && branch != "(no branch)"
                            {
                                git_args.push("origin".into());
                                git_args.push(branch);
                            }
                            entry.git_op = true;
                            let display_name = entry.display_name();
                            let progress = match action {
                                Action::GitPull(_) => format!("Pulling {display_name}..."),
                                Action::RunGitPullRebase(_) => {
                                    format!("Rebasing {display_name}...")
                                }
                                Action::RunGitPullSubmodules(_) => {
                                    format!("Pulling submodules for {display_name}...")
                                }
                                Action::RunRemoveOriginRemote(_) => {
                                    format!("Removing origin from {display_name}...")
                                }
                                _ => unreachable!(),
                            };
                            self.success_message = Some((progress, Instant::now()));
                            let path = entry.path.clone();
                            let repo_id = id.clone();
                            let success = format!("git {} succeeded", git_args.join(" "));
                            self.spawn_git_operation(
                                repo_id,
                                move || crate::git::run_git_args(&path, &git_args),
                                move |_| success,
                                None,
                            );
                        }
                    }
                    Action::GitSubmoduleUpdateLatest(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let name = self.repo_list.repos[idx].name.clone();
                            self.confirm_dialog.show(
                                format!("Pull latest in all submodules for {name}?"),
                                Action::RunGitSubmoduleUpdateLatest(id.clone()),
                            );
                        }
                    }
                    Action::GitSubmoduleUpdate(ref id)
                    | Action::GitSubmoduleSync(ref id)
                    | Action::RunGitSubmoduleUpdateLatest(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &mut self.repo_list.repos[idx];
                            let git_args = match action {
                                Action::GitSubmoduleUpdate(_) => {
                                    git_args(&["submodule", "update", "--init", "--recursive"])
                                }
                                Action::GitSubmoduleSync(_) => git_args(&["submodule", "sync"]),
                                Action::RunGitSubmoduleUpdateLatest(_) => git_args(&[
                                    "submodule",
                                    "foreach",
                                    "git",
                                    "pull",
                                    "origin",
                                    "HEAD",
                                ]),
                                _ => unreachable!(),
                            };
                            entry.git_op = true;
                            let display_name = entry.display_name();
                            let progress = match action {
                                Action::GitSubmoduleUpdate(_) => {
                                    format!("Updating submodules for {display_name}...")
                                }
                                Action::GitSubmoduleSync(_) => {
                                    format!("Syncing submodules for {display_name}...")
                                }
                                Action::RunGitSubmoduleUpdateLatest(_) => {
                                    format!("Pulling latest in submodules for {display_name}...")
                                }
                                _ => unreachable!(),
                            };
                            self.success_message = Some((progress, Instant::now()));
                            let path = entry.path.clone();
                            let repo_id = id.clone();
                            let success = format!("git {} succeeded", git_args.join(" "));
                            self.spawn_git_operation(
                                repo_id,
                                move || crate::git::run_git_args(&path, &git_args),
                                move |_| success,
                                None,
                            );
                        }
                    }
                    Action::GitOpComplete {
                        ref id,
                        ref message,
                    } => {
                        self.success_message = Some((message.clone(), Instant::now()));
                        self.add_operation_log(message.clone());
                        self.action_tx.send(Action::RefreshRepo(id.clone()))?;
                    }
                    Action::ShowDiff(ref id, ref file_path) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            let entry = &self.repo_list.repos[idx];
                            let diff_gen = self.file_list.diff_generation();
                            let sub_info = entry
                                .status
                                .as_ref()
                                .and_then(|s| s.submodules.iter().find(|sm| sm.path == *file_path));

                            if let Some(sub) = sub_info {
                                let repo_path = entry.path.clone();
                                let sub_path = file_path.clone();
                                let old_oid = sub.head_oid.clone().unwrap_or_default();
                                let new_oid = sub.workdir_oid.clone().unwrap_or_default();
                                let sub_state = sub.state.clone();
                                let tx = self.action_tx.clone();
                                tokio::task::spawn_blocking(move || {
                                    let submodule_abs = repo_path.join(&sub_path);
                                    let short_old = if old_oid.len() >= 7 {
                                        &old_oid[..7]
                                    } else {
                                        &old_oid
                                    };
                                    let short_new = if new_oid.len() >= 7 {
                                        &new_oid[..7]
                                    } else {
                                        &new_oid
                                    };

                                    // Dirty submodule (local uncommitted changes): show git diff
                                    // Modified submodule (pointer changed): show commit log
                                    let pointer_changed = !old_oid.is_empty()
                                        && !new_oid.is_empty()
                                        && old_oid != new_oid;
                                    let use_diff = sub_state
                                        == crate::git::status::SubmoduleState::Dirty
                                        || !pointer_changed;

                                    if use_diff {
                                        let header = format!(
                                            "Submodule {} ({})\n{}\n",
                                            sub_path.display(),
                                            match sub_state {
                                                crate::git::status::SubmoduleState::Dirty => "uncommitted changes",
                                                crate::git::status::SubmoduleState::Uninitialized => "not initialized",
                                                crate::git::status::SubmoduleState::Modified => "modified",
                                            },
                                            "─".repeat(40),
                                        );
                                        let mut command = std::process::Command::new("git");
                                        command
                                            .arg("-C")
                                            .arg(&submodule_abs)
                                            .args(["diff", "HEAD"]);
                                        crate::git::configure_noninteractive(&mut command);
                                        let output = command.output();
                                        let body = match output {
                                            Ok(o) => {
                                                let text =
                                                    String::from_utf8_lossy(&o.stdout).to_string();
                                                if text.is_empty() {
                                                    // Fallback: show status
                                                    let mut command =
                                                        std::process::Command::new("git");
                                                    command
                                                        .arg("-C")
                                                        .arg(&submodule_abs)
                                                        .args(["status", "--short"]);
                                                    crate::git::configure_noninteractive(
                                                        &mut command,
                                                    );
                                                    let status_out = command
                                                        .output()
                                                        .map(|o| {
                                                            String::from_utf8_lossy(&o.stdout)
                                                                .to_string()
                                                        })
                                                        .unwrap_or_default();
                                                    if status_out.is_empty() {
                                                        "(no changes detected)".to_string()
                                                    } else {
                                                        status_out
                                                    }
                                                } else {
                                                    text
                                                }
                                            }
                                            Err(e) => {
                                                format!("Failed to get submodule diff: {}", e)
                                            }
                                        };
                                        let _ = tx.send(Action::DiffLoaded {
                                            generation: diff_gen,
                                            content: format!("{}{}", header, body),
                                        });
                                    } else {
                                        // Pointer changed: show commit log between old and new
                                        let header = format!(
                                            "Submodule {} → {}\n{}\n",
                                            short_old,
                                            short_new,
                                            "─".repeat(40),
                                        );
                                        let range = format!("{}..{}", old_oid, new_oid);
                                        let mut command = std::process::Command::new("git");
                                        command.arg("-C").arg(&submodule_abs).args([
                                            "log",
                                            "--oneline",
                                            "--graph",
                                            &range,
                                        ]);
                                        crate::git::configure_noninteractive(&mut command);
                                        let output = command.output();
                                        let body = match output {
                                            Ok(o) => {
                                                let text =
                                                    String::from_utf8_lossy(&o.stdout).to_string();
                                                if text.is_empty() {
                                                    "(no commits in range)".to_string()
                                                } else {
                                                    text
                                                }
                                            }
                                            Err(e) => format!("Failed to get submodule log: {}", e),
                                        };
                                        let _ = tx.send(Action::DiffLoaded {
                                            generation: diff_gen,
                                            content: format!("{}{}", header, body),
                                        });
                                    }
                                });
                            } else {
                                // Use worktree path for diffs when a worktree is selected
                                let path = self
                                    .active_worktree
                                    .as_ref()
                                    .map(|aw| aw.path.clone())
                                    .unwrap_or_else(|| entry.path.clone());
                                let fp = file_path.clone();
                                let tx = self.action_tx.clone();
                                tokio::task::spawn_blocking(move || {
                                    let mut command = std::process::Command::new("git");
                                    command
                                        .arg("-C")
                                        .arg(&path)
                                        .arg("diff")
                                        .arg("HEAD")
                                        .arg("--")
                                        .arg(&fp);
                                    crate::git::configure_noninteractive(&mut command);
                                    let output = command.output();
                                    match output {
                                        Ok(o) => {
                                            let mut text =
                                                String::from_utf8_lossy(&o.stdout).to_string();
                                            if text.is_empty() {
                                                text = String::from_utf8_lossy(&{
                                                    let mut command =
                                                        std::process::Command::new("git");
                                                    command
                                                        .arg("-C")
                                                        .arg(&path)
                                                        .arg("diff")
                                                        .arg("--no-index")
                                                        .arg("/dev/null")
                                                        .arg(&fp);
                                                    crate::git::configure_noninteractive(
                                                        &mut command,
                                                    );
                                                    command
                                                        .output()
                                                        .map(|o| o.stdout)
                                                        .unwrap_or_default()
                                                })
                                                .to_string();
                                            }
                                            if text.is_empty() {
                                                text = "(no diff available)".to_string();
                                            }
                                            let _ = tx.send(Action::DiffLoaded {
                                                generation: diff_gen,
                                                content: text,
                                            });
                                        }
                                        Err(e) => {
                                            let _ = tx.send(Action::DiffLoaded {
                                                generation: diff_gen,
                                                content: format!("Failed to get diff: {}", e),
                                            });
                                        }
                                    }
                                });
                            }
                        }
                    }
                    Action::DiffLoaded {
                        generation,
                        content,
                    } => {
                        if generation == self.file_list.diff_generation() {
                            self.file_list.set_diff(content);
                        }
                    }
                    Action::ShowCommitFiles {
                        ref repo_path,
                        ref oid,
                    } => {
                        let detail_gen = self.git_graph.current_detail_generation();
                        let path = repo_path.clone();
                        let oid = oid.clone();
                        let tx = self.action_tx.clone();
                        tokio::task::spawn_blocking(move || {
                            match crate::git::commit_files::list_commit_files(&path, &oid) {
                                Ok((message, files)) => {
                                    let _ = tx.send(Action::CommitFilesLoaded {
                                        generation: detail_gen,
                                        oid,
                                        message,
                                        files,
                                    });
                                }
                                Err(e) => {
                                    let _ = tx.send(Action::Error(format!(
                                        "Failed to list commit files: {}",
                                        e
                                    )));
                                }
                            }
                        });
                    }
                    Action::CommitFilesLoaded {
                        generation,
                        oid,
                        message,
                        files,
                    } => {
                        if generation == self.git_graph.current_detail_generation() {
                            self.git_graph.set_commit_files(oid, message, files);
                        }
                    }
                    Action::ShowCommitDiff {
                        ref repo_path,
                        ref oid,
                        ref file_path,
                    } => {
                        let detail_gen = self.git_graph.current_detail_generation();
                        let path = repo_path.clone();
                        let oid = oid.clone();
                        let fp = file_path.clone();
                        let tx = self.action_tx.clone();
                        tokio::task::spawn_blocking(move || {
                            match crate::git::commit_files::commit_file_diff(&path, &oid, &fp) {
                                Ok(diff) => {
                                    let _ = tx.send(Action::CommitDiffLoaded {
                                        generation: detail_gen,
                                        content: diff,
                                    });
                                }
                                Err(e) => {
                                    let _ = tx.send(Action::Error(format!(
                                        "Failed to get commit diff: {}",
                                        e
                                    )));
                                }
                            }
                        });
                    }
                    Action::CommitDiffLoaded {
                        generation,
                        content,
                    } => {
                        if generation == self.git_graph.current_detail_generation() {
                            self.git_graph.set_commit_diff(content);
                        }
                    }
                    Action::OpenAddRepo => {
                        self.path_input.show();
                    }
                    Action::AddRepo(ref path) => {
                        self.path_input.hide();
                        let path = path.clone();
                        if !path.join(".git").exists() && !path.join("HEAD").exists() {
                            let input = path.to_string_lossy();
                            let message = if input.starts_with("http://")
                                || input.starts_with("https://")
                                || input.starts_with("git@")
                            {
                                format!(
                                    "Remote repository URLs are not added directly yet: {input}. Clone the repo locally first, then add the local path."
                                )
                            } else {
                                format!("Not a git repository: {}", path.display())
                            };
                            self.action_tx.send(Action::Notice(message))?;
                        } else {
                            let name = path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_else(|| path.to_string_lossy().to_string());
                            self.config.add_pinned_repo(path.clone());
                            if let Err(e) = self.config.save() {
                                tracing::error!("Failed to save config: {}", e);
                            }
                            let repo_id = RepoId(path.clone());
                            self.repo_list.repos.push(RepoEntry {
                                path,
                                name,
                                status: None,
                                git_op: false,
                            });
                            self.action_tx.send(Action::RefreshRepo(repo_id.clone()))?;
                            self.action_tx.send(Action::SelectRepo(repo_id))?;
                        }
                    }
                    Action::RemoveRepo(ref id) => {
                        if let Some(idx) = self.repo_list.resolve_index(id) {
                            // Clean up tracking sets for the removed repo
                            self.pending_status.remove(id);
                            self.dirty_repos.remove(id);
                            self.graph_keys.remove(&id.0);
                            self.remote_keys.remove(&id.0);
                            let entry = &self.repo_list.repos[idx];
                            // Remove from pinned if it was pinned
                            self.config.pinned_repos.retain(|p| *p != entry.path);
                            // Add to excluded so it won't reappear on rescan
                            let name = entry.name.clone();
                            if !self.config.excluded_repos.contains(&name) {
                                self.config.excluded_repos.push(name);
                            }
                            if let Err(e) = self.config.save() {
                                tracing::error!("Failed to save config: {}", e);
                            }
                            self.repo_list.repos.remove(idx);
                            // Fix selection
                            if self.repo_list.repos.is_empty() {
                                self.repo_list.state.select(None);
                                self.file_list.set_files(
                                    Vec::new(),
                                    "",
                                    RepoId(std::path::PathBuf::new()),
                                );
                            } else {
                                let new_idx = idx.min(self.repo_list.repos.len() - 1);
                                self.repo_list.select_repo_row(new_idx);
                                let new_id = RepoId(self.repo_list.repos[new_idx].path.clone());
                                self.action_tx.send(Action::SelectRepo(new_id))?;
                            }
                        }
                    }
                    Action::CycleSortOrder => {
                        self.sort_order = self.sort_order.next();
                        self.sort_repos();
                        self.sync_selection();
                    }
                    Action::RescanRepos => {
                        // Clear tracking sets — old paths are stale after rescan
                        self.pending_status.clear();
                        self.dirty_repos.clear();
                        self.graph_keys.clear();
                        self.remote_keys.clear();
                        self.local_poll_tick = 0;
                        // Clear user-added exclusions, save, and re-discover repos
                        self.config.excluded_repos.clear();
                        if let Err(e) = self.config.save() {
                            tracing::error!("Failed to save config: {}", e);
                        }
                        let repo_paths = scanner::discover_repos(&self.config);
                        self.repo_list =
                            RepoList::new(repo_paths, self.config.submodules.ignore_dirty);
                        self.repo_list
                            .register_action_handler(self.action_tx.clone())?;
                        self.repo_list.init()?;
                        self.action_tx.send(Action::PollLocal)?;
                        self.sort_repos();
                        self.sync_selection();
                    }
                    Action::UpdateAvailable(ref version) => {
                        self.update_version = Some(version.clone());
                    }
                    Action::Error(ref msg) => {
                        tracing::debug!("{}", msg);
                        self.add_operation_log(format!("Error: {msg}"));
                        // Sanitize: single line, max 120 chars for status bar
                        let clean: String = msg
                            .chars()
                            .map(|c| if c == '\n' { ' ' } else { c })
                            .collect();
                        let truncated = if clean.len() > 120 {
                            format!("{}...", &clean[..117])
                        } else {
                            clean
                        };
                        self.error_message = Some((truncated, Instant::now()));
                    }
                    Action::Notice(ref msg) => {
                        tracing::debug!("{}", msg);
                        self.add_operation_log(format!("Notice: {msg}"));
                        self.notice_dialog.show(msg.clone());
                    }
                    _ => {
                        let _ = self.repo_list.update(action)?;
                    }
                }
                if render_after {
                    render_requested = true;
                }
            }

            if self.should_quit {
                tui.exit()?;
                break;
            }

            if render_requested {
                tui.terminal.draw(|frame| {
                    let _ = self.draw(frame);
                })?;
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) -> Result<()> {
        let area = frame.area();

        // Vertical: main area + status bar
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);

        let main_area = outer[0];
        let status_area = outer[1];

        // Three-panel layout: repositories on top, changes in the middle,
        // graph at the bottom. Drag the horizontal borders to resize.
        let h = main_area.height as f64;
        let r1 = (self.border_frac[0] * h).round() as u16;
        let r2 = ((self.border_frac[1] - self.border_frac[0]) * h).round() as u16;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(r1),
                Constraint::Length(r2),
                Constraint::Min(3),
            ])
            .split(main_area);
        let (repo_area, changes_area, graph_area) = (chunks[0], chunks[1], chunks[2]);

        self.repo_area = repo_area;
        self.changes_area = changes_area;
        self.graph_area = graph_area;

        self.repo_list.focused = self.focus == FocusPanel::Repos;
        self.file_list.focused = self.focus == FocusPanel::Changes;
        self.git_graph.focused = self.focus == FocusPanel::Graph;

        self.file_list.horizontal_layout = false;
        self.git_graph.horizontal_layout = false;

        self.repo_list.draw(frame, repo_area)?;
        self.file_list.draw(frame, changes_area)?;
        self.git_graph.draw(frame, graph_area)?;

        if self.dragging_border.is_some() {
            // Only highlight the seam during active drag.
            use ratatui::style::{Color, Style};

            let style = Style::default().fg(Color::Yellow);
            let buf = frame.buffer_mut();
            for (dragging, y) in [
                (self.dragging_border == Some(0), changes_area.y),
                (self.dragging_border == Some(1), graph_area.y),
            ] {
                if !dragging {
                    continue;
                }
                // Paint just the border characters (skip first col = title area preserved)
                for x in repo_area.x..repo_area.x + repo_area.width {
                    if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(x, y)) {
                        cell.set_style(style);
                    }
                }
            }
        }

        self.status_bar.focus = self.focus;
        self.status_bar.error = self.error_message.clone();
        self.status_bar.success = self.success_message.clone();
        self.status_bar.fast_mode = self.fast_mode;
        self.status_bar.focused_repo = self.focused_repo_name();
        self.status_bar.draw(frame, status_area)?;

        // Overlays rendered last
        self.context_menu.draw(frame, area)?;
        self.path_input.draw(frame, area);
        self.commit_input.draw(frame, area);
        self.github_repo_input.draw(frame, area);
        self.confirm_dialog.draw(frame, area);
        self.notice_dialog.draw(frame, area)?;

        // Update notification overlay
        if let Some(ref version) = self.update_version {
            self.draw_update_notification(frame, main_area, version);
        }

        // Help overlay (rendered last so it's on top of everything)
        if self.show_help {
            self.draw_help(frame, main_area);
        }

        if self.show_operation_log {
            self.draw_operation_log(frame, main_area);
        }

        Ok(())
    }
}
