use std::path::PathBuf;
use std::time::Instant;

use crate::app::App;
use crate::git::status::UntrackedMode;
use crate::repo_id::RepoId;

impl App {
    pub(super) fn repo_graph_changed(&mut self, path: PathBuf, graph_key: &str) -> bool {
        match self.graph_keys.insert(path, graph_key.to_string()) {
            Some(previous) => previous != graph_key,
            None => true,
        }
    }

    pub(super) fn repo_remote_changed(&mut self, path: PathBuf, remote_key: &str) -> bool {
        match self.remote_keys.insert(path, remote_key.to_string()) {
            Some(previous) => previous != remote_key,
            None => !remote_key.is_empty(),
        }
    }

    pub(super) fn effective_untracked_mode(&self) -> UntrackedMode {
        if self.fast_mode {
            UntrackedMode::None
        } else {
            self.config.status.untracked
        }
    }

    pub(super) fn apply_fast_mode(&mut self, enabled: bool) {
        self.fast_mode = enabled;
        self.git_graph.graph_options.show_stats = self.config.graph.show_stats && !enabled;
        self.success_message = Some((
            if enabled {
                "Fast mode enabled".to_string()
            } else {
                "Fast mode disabled".to_string()
            },
            Instant::now(),
        ));
    }

    pub(super) fn toggle_fast_mode(&mut self) {
        self.apply_fast_mode(!self.fast_mode);
    }

    pub(super) fn should_poll_repo(&self, repo_id: &RepoId, idx: usize, full_scan: bool) -> bool {
        full_scan
            || self.repo_list.selected_index() == Some(idx)
            || self.repo_list.repos[idx].status.is_none()
            || self.dirty_repos.contains(repo_id)
    }

    pub(super) fn expire_messages(&mut self) -> bool {
        let mut changed = false;
        if let Some((_, when)) = &self.error_message
            && when.elapsed().as_secs() >= 5
        {
            self.error_message = None;
            changed = true;
        }
        if let Some((_, when)) = &self.success_message
            && when.elapsed().as_secs() >= 3
        {
            self.success_message = None;
            changed = true;
        }
        changed
    }
}
