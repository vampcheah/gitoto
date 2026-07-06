use color_eyre::Result;

use crate::action::Action;
use crate::app::App;
use crate::components::Component;
use crate::components::repo_list::{RepoEntry, RepoList};
use crate::git::scanner;
use crate::repo_id::RepoId;

impl App {
    pub(super) fn handle_repo_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::OpenAddRepo => {
                self.path_input.show();
                Ok(true)
            }
            Action::AddRepo(path) => {
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
                        self.set_error_message(format!("Failed to save config: {e}"));
                    }
                    let repo_id = RepoId(path.clone());
                    self.repo_list.push_repo(RepoEntry {
                        path,
                        name,
                        status: None,
                        git_op: false,
                    });
                    self.action_tx.send(Action::RefreshRepo(repo_id.clone()))?;
                    self.action_tx.send(Action::SelectRepo(repo_id))?;
                }
                Ok(true)
            }
            Action::RemoveRepo(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    self.pending_status.remove(id);
                    self.dirty_repos.remove(id);
                    self.graph_keys.remove(&id.0);
                    self.remote_keys.remove(&id.0);

                    let entry = &self.repo_list.repos[idx];
                    self.config.pinned_repos.retain(|p| *p != entry.path);
                    let exclusion_name = entry.name.clone();
                    if !self.config.excluded_repos.contains(&exclusion_name) {
                        self.config.excluded_repos.push(exclusion_name);
                    }
                    if let Err(e) = self.config.save() {
                        self.set_error_message(format!("Failed to save config: {e}"));
                    }

                    self.repo_list.remove_repo(idx);
                    if self.repo_list.repos.is_empty() {
                        self.repo_list.state.select(None);
                        self.sync_selection();
                    } else {
                        let new_idx = idx.min(self.repo_list.repos.len() - 1);
                        self.repo_list.select_repo_row(new_idx);
                        let new_id = self.repo_list.repos[new_idx].id();
                        self.action_tx.send(Action::SelectRepo(new_id))?;
                    }
                }
                Ok(true)
            }
            Action::CycleSortOrder => {
                self.sort_order = self.sort_order.next();
                self.sort_repos();
                self.sync_selection();
                Ok(true)
            }
            Action::RescanRepos => {
                self.pending_status.clear();
                self.dirty_repos.clear();
                self.graph_keys.clear();
                self.remote_keys.clear();
                self.local_poll_tick = 0;

                self.config.excluded_repos.clear();
                if let Err(e) = self.config.save() {
                    self.set_error_message(format!("Failed to save config: {e}"));
                }
                let repo_paths = scanner::discover_repos(&self.config);
                self.repo_list = RepoList::new(
                    repo_paths,
                    self.config.submodules.ignore_dirty,
                    self.config.ui.repo_name_format,
                );
                self.repo_list
                    .register_action_handler(self.action_tx.clone())?;
                self.repo_list.init()?;
                self.action_tx.send(Action::PollLocal)?;
                self.sort_repos();
                self.sync_selection();
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
