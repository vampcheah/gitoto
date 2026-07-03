use color_eyre::Result;

use crate::action::Action;
use crate::app::App;
use crate::app::helpers::{ActiveWorktree, StatusFailure, StatusGuard, StatusQuery};
use crate::repo_id::RepoId;

impl App {
    fn queue_status_queries(
        &mut self,
        queries: Vec<(RepoId, std::path::PathBuf)>,
        query: StatusQuery,
        failure: StatusFailure,
    ) {
        for (repo_id, path) in queries {
            if self.pending_status.insert(repo_id.clone()) {
                self.spawn_status_query(repo_id, path, query, failure);
            }
        }
    }

    pub(super) fn spawn_status_query(
        &self,
        repo_id: RepoId,
        path: std::path::PathBuf,
        query: StatusQuery,
        failure: StatusFailure,
    ) {
        let tx = self.action_tx.clone();
        let sem = match query {
            StatusQuery::Local => self.poll_semaphore.clone(),
            StatusQuery::Fetch => self.fetch_semaphore.clone(),
        };
        let ignore_dirty_subs = self.config.submodules.ignore_dirty;
        let untracked = self.effective_untracked_mode();
        let github_hosts = self.config.github.hosts.clone();

        tokio::spawn(async move {
            let _permit = sem.acquire().await;
            let guard = StatusGuard::new(repo_id.clone(), tx.clone());
            tokio::task::spawn_blocking(move || {
                let result = match query {
                    StatusQuery::Local => {
                        crate::git::status::query_status_with_untracked_and_github_hosts(
                            &path,
                            ignore_dirty_subs,
                            untracked,
                            &github_hosts,
                        )
                    }
                    StatusQuery::Fetch => {
                        crate::git::status::query_status_with_fetch_untracked_and_github_hosts(
                            &path,
                            ignore_dirty_subs,
                            untracked,
                            &github_hosts,
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

    pub(super) fn spawn_worktree_status_query(&self, worktree: ActiveWorktree) {
        let tx = self.action_tx.clone();
        let sem = self.poll_semaphore.clone();
        let ignore_dirty_subs = self.config.submodules.ignore_dirty;
        let untracked = self.effective_untracked_mode();
        let github_hosts = self.config.github.hosts.clone();

        tokio::spawn(async move {
            let _permit = sem.acquire().await;
            tokio::task::spawn_blocking(move || {
                match crate::git::status::query_status_with_untracked_and_github_hosts(
                    &worktree.path,
                    ignore_dirty_subs,
                    untracked,
                    &github_hosts,
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

    /// Handles `Action::RepoStatusUpdated` by value: the status (with its
    /// potentially large file list) moves into the repo list instead of
    /// being cloned, and files are cloned once only for the selected repo.
    pub(super) fn handle_repo_status_updated(
        &mut self,
        id: RepoId,
        status: crate::git::status::RepoStatus,
    ) -> Result<()> {
        self.pending_status.remove(&id);
        let is_dirty = self.dirty_repos.remove(&id);
        if let Some(idx) = self.repo_list.resolve_index(&id) {
            let repo_path = self.repo_list.repos[idx].path.clone();
            let graph_changed = self.repo_graph_changed(repo_path.clone(), &status.graph_key);
            let remote_changed = self.repo_remote_changed(repo_path.clone(), &status.remote_key);
            let selected_repo =
                self.repo_list.selected_index() == Some(idx) && self.active_worktree.is_none();
            let selected_files = selected_repo.then(|| status.files.clone());
            let selected_name = self.repo_display_name(idx);
            self.repo_list.update_status(idx, status);

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
        Ok(())
    }

    pub(super) fn handle_status_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::SelectWorktree {
                repo_id,
                worktree_path,
                worktree_branch,
            } => {
                self.context_menu.hide();

                let repo_name = self
                    .repo_list
                    .display_name_for_id(repo_id)
                    .unwrap_or_default();
                let display_name = format!("{}:{}", repo_name, worktree_branch);

                self.active_worktree = Some(ActiveWorktree {
                    path: worktree_path.clone(),
                    repo_id: repo_id.clone(),
                    display_name: display_name.clone(),
                    // Filled by the async status query below; computing it here
                    // would run git2 on the UI thread.
                    graph_key: None,
                });

                self.file_list
                    .set_files(Vec::new(), &display_name, repo_id.clone());
                self.git_graph
                    .load_repo(worktree_path.clone(), &display_name);

                if let Some(worktree) = self.active_worktree.clone() {
                    self.spawn_worktree_status_query(worktree);
                }
                Ok(true)
            }
            Action::WorktreeFilesLoaded {
                repo_id,
                worktree_path,
                name,
                files,
                graph_key,
            } => {
                if self
                    .active_worktree
                    .as_ref()
                    .is_some_and(|aw| aw.path == *worktree_path)
                {
                    self.file_list
                        .set_files(files.clone(), name, repo_id.clone());

                    // A None baseline means this is the first query result, not a change.
                    let graph_changed = self
                        .active_worktree
                        .as_ref()
                        .is_some_and(|aw| aw.graph_key.as_deref().is_some_and(|k| k != graph_key));
                    if let Some(aw) = self.active_worktree.as_mut() {
                        aw.graph_key = Some(graph_key.clone());
                    }
                    if graph_changed {
                        if self.git_graph.has_detail() {
                            self.git_graph.set_needs_reload();
                        } else {
                            self.git_graph.load_repo(worktree_path.clone(), name);
                        }
                    }
                }
                Ok(true)
            }
            Action::StatusQueryDone(id) => {
                self.pending_status.remove(id);
                if let Some(entry) = self.repo_list.resolve_entry_mut(id) {
                    entry.git_op = false;
                }
                if self.dirty_repos.remove(id) {
                    self.action_tx.send(Action::RefreshRepo(id.clone()))?;
                }
                Ok(true)
            }
            Action::RefreshAll => {
                let queries = self
                    .repo_list
                    .repos
                    .iter_mut()
                    .map(|entry| {
                        entry.git_op = true;
                        (entry.id(), entry.path.clone())
                    })
                    .collect();
                self.queue_status_queries(queries, StatusQuery::Fetch, StatusFailure::UserVisible);
                Ok(true)
            }
            Action::PollLocal => {
                let full_every = self.config.watch.poll_local_full_every.max(1);
                let full_scan =
                    self.local_poll_tick == 0 || self.local_poll_tick.is_multiple_of(full_every);
                self.local_poll_tick = self.local_poll_tick.saturating_add(1);

                let queries: Vec<_> = self
                    .repo_list
                    .repos
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, entry)| {
                        let repo_id = entry.id();
                        if entry.git_op
                            || self.pending_status.contains(&repo_id)
                            || !self.should_poll_repo(&repo_id, idx, full_scan)
                        {
                            None
                        } else {
                            Some((repo_id, entry.path.clone()))
                        }
                    })
                    .collect();
                self.queue_status_queries(
                    queries,
                    StatusQuery::Local,
                    StatusFailure::Debug("Local poll failed"),
                );

                if let Some(aw) = self.active_worktree.clone() {
                    self.spawn_worktree_status_query(aw);
                }
                Ok(true)
            }
            Action::PollFetch => {
                if self.fast_mode {
                    return Ok(true);
                }
                let queries: Vec<_> = self
                    .repo_list
                    .repos
                    .iter()
                    .filter_map(|entry| {
                        let repo_id = entry.id();
                        if entry.git_op || self.pending_status.contains(&repo_id) {
                            None
                        } else {
                            Some((repo_id, entry.path.clone()))
                        }
                    })
                    .collect();
                self.queue_status_queries(
                    queries,
                    StatusQuery::Fetch,
                    StatusFailure::Debug("Fetch poll failed"),
                );
                Ok(true)
            }
            Action::RefreshRepo(id) => {
                if self.pending_status.contains(id) {
                    self.dirty_repos.insert(id.clone());
                    tracing::debug!("skipping repo {}: already in-flight (marked dirty)", id);
                    return Ok(true);
                }
                if let Some(entry) = self.repo_list.resolve_entry(id) {
                    self.queue_status_queries(
                        vec![(id.clone(), entry.path.clone())],
                        StatusQuery::Local,
                        StatusFailure::UserVisible,
                    );
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
