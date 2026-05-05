use color_eyre::Result;
use std::path::PathBuf;

use crate::action::Action;
use crate::app::App;
use crate::app::helpers::git_args;
use crate::repo_id::RepoId;

#[derive(Default)]
struct BranchActionState {
    branch: String,
    has_upstream: bool,
    ahead: usize,
}

impl App {
    fn action_repo_name(&self, idx: usize) -> String {
        self.repo_list.repos[idx].display_name_for_format(self.config.ui.repo_name_format, false)
    }

    fn branch_action_state(&self, idx: usize) -> BranchActionState {
        self.repo_list.repos[idx]
            .status
            .as_ref()
            .map(|status| BranchActionState {
                branch: status.branch.clone(),
                has_upstream: status.has_upstream,
                ahead: status.ahead,
            })
            .unwrap_or_default()
    }

    fn confirm_repo_action(
        &mut self,
        id: &RepoId,
        prompt: impl FnOnce(&str) -> String,
        action: Action,
    ) {
        if let Some(idx) = self.repo_list.resolve_index(id) {
            let name = self.action_repo_name(idx);
            self.confirm_dialog.show(prompt(&name), action);
        }
    }

    fn spawn_git_args_operation(
        &mut self,
        idx: usize,
        id: &RepoId,
        git_args: Vec<String>,
        progress: String,
        success: String,
    ) {
        self.spawn_repo_operation(
            idx,
            id,
            progress,
            move |path| crate::git::run_git_args(&path, &git_args),
            move |_| success,
            None,
        );
    }

    pub(super) fn spawn_repo_operation<F, M>(
        &mut self,
        idx: usize,
        id: &RepoId,
        progress: impl Into<String>,
        operation: F,
        success_message: M,
        error_context: Option<&'static str>,
    ) where
        F: FnOnce(PathBuf) -> Result<String> + Send + 'static,
        M: FnOnce(String) -> String + Send + 'static,
    {
        let entry = &mut self.repo_list.repos[idx];
        entry.git_op = true;
        let path = entry.path.clone();
        self.set_success_message(progress.into());
        self.spawn_git_operation(
            id.clone(),
            move || operation(path),
            success_message,
            error_context,
        );
    }

    pub(super) fn handle_git_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::GitPush(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let BranchActionState {
                        branch,
                        has_upstream,
                        ahead,
                    } = self.branch_action_state(idx);
                    if branch.is_empty() || branch == "(no branch)" || branch == "HEAD" {
                        self.set_error_message("Cannot push detached HEAD");
                        return Ok(true);
                    }
                    if !has_upstream {
                        self.set_error_message(format!(
                            "Branch '{branch}' has no upstream; press P to publish"
                        ));
                        return Ok(true);
                    }
                    if ahead == 0 {
                        self.set_success_message("Nothing to push");
                        return Ok(true);
                    }
                    let push_target = self.action_repo_name(idx);
                    let success_target = push_target.clone();
                    self.spawn_repo_operation(
                        idx,
                        id,
                        format!("Pushing to {push_target}..."),
                        |path| crate::git::push(&path),
                        move |_| format!("Pushed {success_target}"),
                        None,
                    );
                }
                Ok(true)
            }
            Action::GitPublish(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let publish_target = self.action_repo_name(idx);
                    let branch = self.branch_action_state(idx).branch;
                    let success_branch = branch.clone();
                    let success_target = publish_target.clone();
                    self.spawn_repo_operation(
                        idx,
                        id,
                        format!("Publishing {publish_target}..."),
                        move |path| crate::git::publish(&path, &branch),
                        move |_| format!("Published {success_target}:{success_branch}"),
                        Some("Publish failed"),
                    );
                }
                Ok(true)
            }
            Action::CreateGitHubRepo { id, private, name } => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let private = *private;
                    let visibility = if private { "private" } else { "public" };
                    let repo_name = name.clone();
                    let success_name = name.clone();
                    self.spawn_repo_operation(
                        idx,
                        id,
                        format!("Creating {visibility} GitHub repo {name}..."),
                        move |path| crate::git::create_github_repo(&path, &repo_name, private),
                        move |_| format!("Created {visibility} GitHub repo {success_name}"),
                        Some("Create GitHub repo failed"),
                    );
                }
                Ok(true)
            }
            Action::GitPullRebase(id) => {
                self.confirm_repo_action(
                    id,
                    |name| format!("Pull --rebase {name}?"),
                    Action::RunGitPullRebase(id.clone()),
                );
                Ok(true)
            }
            Action::GitPullSubmodules(id) => {
                self.confirm_repo_action(
                    id,
                    |name| format!("Pull submodules for {name}?"),
                    Action::RunGitPullSubmodules(id.clone()),
                );
                Ok(true)
            }
            Action::RemoveOriginRemote(id) => {
                self.confirm_repo_action(
                    id,
                    |name| format!("Remove origin remote from {name}?"),
                    Action::RunRemoveOriginRemote(id.clone()),
                );
                Ok(true)
            }
            Action::GitPull(id)
            | Action::RunGitPullRebase(id)
            | Action::RunGitPullSubmodules(id)
            | Action::RunRemoveOriginRemote(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let branch = self.branch_action_state(idx).branch;
                    let display_name = self.action_repo_name(idx);
                    let (mut git_args, progress, success, should_add_origin_branch) = match action {
                        Action::GitPull(_) => (
                            git_args(&["pull"]),
                            format!("Pulling {display_name}..."),
                            format!("Pulled {display_name}"),
                            true,
                        ),
                        Action::RunGitPullRebase(_) => (
                            git_args(&["pull", "--rebase"]),
                            format!("Rebasing {display_name}..."),
                            format!("Rebased {display_name}"),
                            true,
                        ),
                        Action::RunGitPullSubmodules(_) => (
                            git_args(&["pull", "--recurse-submodules"]),
                            format!("Pulling submodules for {display_name}..."),
                            format!("Pulled submodules for {display_name}"),
                            true,
                        ),
                        Action::RunRemoveOriginRemote(_) => (
                            git_args(&["remote", "remove", "origin"]),
                            format!("Removing origin from {display_name}..."),
                            format!("Removed origin from {display_name}"),
                            false,
                        ),
                        _ => return Ok(false),
                    };
                    if should_add_origin_branch && !branch.is_empty() && branch != "(no branch)" {
                        git_args.push("origin".into());
                        git_args.push(branch);
                    }
                    self.spawn_git_args_operation(idx, id, git_args, progress, success);
                }
                Ok(true)
            }
            Action::GitSubmoduleUpdateLatest(id) => {
                self.confirm_repo_action(
                    id,
                    |name| format!("Pull latest in all submodules for {name}?"),
                    Action::RunGitSubmoduleUpdateLatest(id.clone()),
                );
                Ok(true)
            }
            Action::GitSubmoduleUpdate(id)
            | Action::GitSubmoduleSync(id)
            | Action::RunGitSubmoduleUpdateLatest(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let display_name = self.action_repo_name(idx);
                    let (git_args, progress, success) = match action {
                        Action::GitSubmoduleUpdate(_) => (
                            git_args(&["submodule", "update", "--init", "--recursive"]),
                            format!("Updating submodules for {display_name}..."),
                            format!("Updated submodules in {display_name}"),
                        ),
                        Action::GitSubmoduleSync(_) => (
                            git_args(&["submodule", "sync"]),
                            format!("Syncing submodules for {display_name}..."),
                            format!("Synced submodules in {display_name}"),
                        ),
                        Action::RunGitSubmoduleUpdateLatest(_) => (
                            git_args(&["submodule", "foreach", "git", "pull", "origin", "HEAD"]),
                            format!("Pulling latest in submodules for {display_name}..."),
                            format!("Pulled latest in submodules for {display_name}"),
                        ),
                        _ => return Ok(false),
                    };
                    self.spawn_git_args_operation(idx, id, git_args, progress, success);
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
