use color_eyre::Result;
use ratatui::layout::Rect;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::action::Action;
use crate::components::Component;
use crate::components::commit_input::CommitInput;
use crate::components::confirm_dialog::ConfirmDialog;
use crate::components::context_menu::ContextMenu;
use crate::components::file_list::FileList;
use crate::components::git_graph::GitGraph;
use crate::components::github_repo_input::GitHubRepoInput;
use crate::components::notice_dialog::NoticeDialog;
use crate::components::path_input::PathInput;
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

mod actions;
mod diff;
mod git_ops;
mod helpers;
mod input;
mod messages;
mod perf;
mod repo_actions;
mod status;
mod ui;

use helpers::{ActiveWorktree, copy_to_clipboard, is_valid_github_repo_name, open_browser_url};

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
    /// Separate pool for fetch polls so slow fetches (up to 30s timeout each)
    /// can't starve local status refreshes.
    // ponytail: same size as the local pool; add a config knob if it needs tuning
    fetch_semaphore: Arc<tokio::sync::Semaphore>,
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
        let repo_name_format = config.ui.repo_name_format;
        let ignore_dirty_subs = config.submodules.ignore_dirty;
        let poll_semaphore = Arc::new(tokio::sync::Semaphore::new(
            config.watch.max_concurrent_polls,
        ));
        let fetch_semaphore = Arc::new(tokio::sync::Semaphore::new(
            config.watch.max_concurrent_polls,
        ));

        Self {
            config,
            should_quit: false,
            repo_list: RepoList::new(repo_paths, ignore_dirty_subs, repo_name_format),
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
            fetch_semaphore,
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
            SortOrder::Alphabetical => self.repo_list.sort_alphabetical(),
            SortOrder::DirtyFirst => self.repo_list.sort_dirty_first(),
        }
        // Reset selection to first
        if !self.repo_list.repos.is_empty() {
            self.repo_list.select_repo_row(0);
        }
    }

    /// Auto-load graph + file list for the selected repo.
    fn sync_selection(&mut self) {
        if let Some(idx) = self.repo_list.selected_index() {
            let repo_id = self.repo_list.repos[idx].id();
            self.load_repo_panels(idx, repo_id);
        } else {
            // No repos left (e.g. rescan after .git was deleted): clear stale panels.
            self.file_list
                .set_files(Vec::new(), "", RepoId(std::path::PathBuf::new()));
            self.git_graph.clear();
        }
    }

    fn load_repo_panels(&mut self, idx: usize, repo_id: RepoId) {
        let entry = &self.repo_list.repos[idx];
        let name = self.repo_display_name(idx);
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
        self.file_list.set_files(files, &name, repo_id);
        self.git_graph.load_repo(path, &name);
    }

    fn select_repo(&mut self, id: &RepoId) {
        self.context_menu.hide();
        self.active_worktree = None;
        if let Some(idx) = self.repo_list.resolve_index(id) {
            self.repo_list.select_repo_row(idx);
            self.load_repo_panels(idx, id.clone());
        }
    }

    fn open_repo_graph(&mut self, id: &RepoId) {
        self.select_repo(id);
        if self.repo_list.resolve_index(id).is_some() {
            self.focused_repo = Some(id.clone());
            self.focus = FocusPanel::Graph;
        }
    }

    fn selected_repo_id(&self) -> Option<RepoId> {
        self.repo_list.selected_repo().map(|entry| entry.id())
    }

    fn active_repo_id(&self) -> Option<RepoId> {
        self.focused_repo
            .clone()
            .or_else(|| self.selected_repo_id())
    }

    fn focused_repo_name(&self) -> Option<String> {
        self.focused_repo
            .as_ref()
            .and_then(|id| self.repo_list.display_name_for_id(id))
    }

    pub(super) fn repo_display_name(&self, idx: usize) -> String {
        self.repo_list
            .display_name_for_index(idx)
            .unwrap_or_else(|| self.repo_list.repos[idx].name.clone())
    }

    fn add_operation_log(&mut self, message: impl Into<String>) {
        if self.operation_log.len() >= 50 {
            self.operation_log.pop_front();
        }
        self.operation_log.push_back(message.into());
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
                        if let Some(id) = self.selected_repo_id() {
                            self.action_tx.send(Action::RefreshRepo(id))?;
                        }
                    }
                    _ => {}
                }
            }

            // Process actions
            while let Ok(action) = self.action_rx.try_recv() {
                let render_after = !matches!(&action, Action::Tick);
                // Taken by value so the (possibly large) status moves into the
                // repo list instead of being cloned out of a borrowed action.
                if let Action::RepoStatusUpdated { id, status } = action {
                    self.handle_repo_status_updated(id, status)?;
                    render_requested = true;
                    continue;
                }
                if self.handle_git_action(&action)?
                    || self.handle_repo_action(&action)?
                    || self.handle_status_action(&action)?
                    || self.handle_diff_action(&action)?
                {
                    if render_after {
                        render_requested = true;
                    }
                    continue;
                }
                match action {
                    Action::Tick => {
                        if self.expire_messages() {
                            render_requested = true;
                        }
                    }
                    Action::Quit => {
                        self.should_quit = true;
                    }
                    Action::Resize(w, h) => {
                        tui.terminal
                            .resize(ratatui::layout::Rect::new(0, 0, w, h))?;
                    }
                    Action::SelectRepo(ref id) => {
                        if self.focused_repo.is_some() {
                            continue;
                        }
                        self.select_repo(id);
                    }
                    Action::ShowGitGraph => {
                        if let Some(id) = self.selected_repo_id() {
                            self.open_repo_graph(&id);
                        }
                    }
                    Action::ShowRepoGitGraph(ref id) => {
                        self.open_repo_graph(id);
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
                        if let Some(entry) = self.repo_list.resolve_entry(id) {
                            self.context_menu.show(
                                id.clone(),
                                col,
                                row,
                                entry.status.as_ref().into(),
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
                        if let Some(entry) = self.repo_list.resolve_entry(id) {
                            let path_str = entry.path.to_string_lossy().to_string();
                            copy_to_clipboard(&path_str);
                        }
                    }
                    Action::OpenGitHub(ref id) => {
                        if let Some(entry) = self.repo_list.resolve_entry(id) {
                            let url = entry
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
                        if let Some(name) = self.repo_list.display_name_for_id(id) {
                            let marked = self.file_list.marked_paths_for(id).len();
                            self.commit_input.show(id.clone(), name, marked);
                        }
                    }
                    Action::CancelCommit => {
                        self.commit_input.hide();
                    }
                    Action::StartCreateGitHubRepo { ref id, private } => {
                        self.context_menu.hide();
                        if let Some(entry) = self.repo_list.resolve_entry(id) {
                            self.github_repo_input
                                .show(id.clone(), private, entry.name.clone());
                        }
                    }
                    Action::CancelCreateGitHubRepo => {
                        self.github_repo_input.hide();
                    }
                    Action::ConfirmCreateGitHubRepo => {
                        let Some(repo_id) = self.github_repo_input.repo_id() else {
                            continue;
                        };
                        let repo_name = self.github_repo_input.name().trim().to_string();
                        if repo_name.is_empty() {
                            self.set_error_message("Repository name is empty");
                            continue;
                        }
                        if !is_valid_github_repo_name(&repo_name) {
                            self.set_error_message(
                                "Repository name can only contain letters, numbers, '.', '_' and '-'"
                            );
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
                            self.set_error_message("Commit message is empty");
                            continue;
                        }
                        if let Some(idx) = self.repo_list.resolve_index(&repo_id) {
                            let repo_name = self.repo_display_name(idx);
                            let no_verify = self.config.commit.no_verify;
                            let marked = self.file_list.marked_paths_for(&repo_id);
                            self.commit_input.hide();
                            if marked.is_empty() {
                                self.spawn_repo_operation(
                                    idx,
                                    &repo_id,
                                    format!("Committing {repo_name}..."),
                                    move |path| crate::git::commit_all(&path, &message, no_verify),
                                    move |_| format!("Committed {}", repo_name),
                                    Some("Commit failed"),
                                );
                            } else {
                                let count = marked.len();
                                self.spawn_repo_operation(
                                    idx,
                                    &repo_id,
                                    format!("Committing {count} file(s) in {repo_name}..."),
                                    move |path| {
                                        crate::git::commit_paths(
                                            &path, &message, no_verify, &marked,
                                        )
                                    },
                                    move |_| format!("Committed {count} file(s) in {repo_name}"),
                                    Some("Commit failed"),
                                );
                            }
                        }
                    }
                    Action::GitOpComplete {
                        ref id,
                        ref message,
                    } => {
                        self.set_success_message(message.clone());
                        self.add_operation_log(message.clone());
                        self.action_tx.send(Action::RefreshRepo(id.clone()))?;
                    }
                    Action::UpdateAvailable(ref version) => {
                        self.update_version = Some(version.clone());
                    }
                    Action::Error(ref msg) => {
                        tracing::debug!("{}", msg);
                        self.add_operation_log(format!("Error: {msg}"));
                        self.set_sanitized_error_message(msg);
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
}
