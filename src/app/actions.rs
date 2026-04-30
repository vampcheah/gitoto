use color_eyre::Result;
use std::time::Instant;

use crate::action::Action;
use crate::app::App;
use crate::app::helpers::git_args;

impl App {
    pub(super) fn handle_git_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::GitPush(id) => {
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
                        return Ok(true);
                    }
                    if !has_upstream {
                        self.error_message = Some((
                            format!("Branch '{branch}' has no upstream; press P to publish"),
                            Instant::now(),
                        ));
                        return Ok(true);
                    }
                    if ahead == 0 {
                        self.success_message =
                            Some(("Nothing to push".to_string(), Instant::now()));
                        return Ok(true);
                    }
                    entry.git_op = true;
                    let push_target =
                        entry.display_name_for_format(self.config.ui.repo_name_format, false);
                    self.success_message =
                        Some((format!("Pushing to {push_target}..."), Instant::now()));
                    let path = entry.path.clone();
                    let repo_id = id.clone();
                    let success_target = push_target.clone();
                    self.spawn_git_operation(
                        repo_id,
                        move || crate::git::push(&path),
                        move |_| format!("Pushed {success_target}"),
                        None,
                    );
                }
                Ok(true)
            }
            Action::GitPublish(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let entry = &mut self.repo_list.repos[idx];
                    let branch = entry
                        .status
                        .as_ref()
                        .map(|s| s.branch.clone())
                        .unwrap_or_default();
                    entry.git_op = true;
                    let publish_target =
                        entry.display_name_for_format(self.config.ui.repo_name_format, false);
                    self.success_message =
                        Some((format!("Publishing {publish_target}..."), Instant::now()));
                    let path = entry.path.clone();
                    let repo_id = id.clone();
                    let success_branch = branch.clone();
                    let success_target = publish_target.clone();
                    self.spawn_git_operation(
                        repo_id,
                        move || crate::git::publish(&path, &branch),
                        move |_| format!("Published {success_target}:{success_branch}"),
                        Some("Publish failed"),
                    );
                }
                Ok(true)
            }
            Action::CreateGitHubRepo { id, private, name } => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let entry = &mut self.repo_list.repos[idx];
                    entry.git_op = true;
                    let private = *private;
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
                Ok(true)
            }
            Action::GitPullRebase(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let name = self.repo_list.repos[idx]
                        .display_name_for_format(self.config.ui.repo_name_format, false);
                    self.confirm_dialog.show(
                        format!("Pull --rebase {name}?"),
                        Action::RunGitPullRebase(id.clone()),
                    );
                }
                Ok(true)
            }
            Action::GitPullSubmodules(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let name = self.repo_list.repos[idx]
                        .display_name_for_format(self.config.ui.repo_name_format, false);
                    self.confirm_dialog.show(
                        format!("Pull submodules for {name}?"),
                        Action::RunGitPullSubmodules(id.clone()),
                    );
                }
                Ok(true)
            }
            Action::RemoveOriginRemote(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let name = self.repo_list.repos[idx]
                        .display_name_for_format(self.config.ui.repo_name_format, false);
                    self.confirm_dialog.show(
                        format!("Remove origin remote from {name}?"),
                        Action::RunRemoveOriginRemote(id.clone()),
                    );
                }
                Ok(true)
            }
            Action::GitPull(id)
            | Action::RunGitPullRebase(id)
            | Action::RunGitPullSubmodules(id)
            | Action::RunRemoveOriginRemote(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let entry = &mut self.repo_list.repos[idx];
                    let branch = entry
                        .status
                        .as_ref()
                        .map(|s| s.branch.clone())
                        .unwrap_or_default();
                    let should_add_origin_branch =
                        !matches!(action, Action::RunRemoveOriginRemote(_));
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
                    if should_add_origin_branch && !branch.is_empty() && branch != "(no branch)" {
                        git_args.push("origin".into());
                        git_args.push(branch);
                    }
                    entry.git_op = true;
                    let display_name =
                        entry.display_name_for_format(self.config.ui.repo_name_format, false);
                    let progress = match action {
                        Action::GitPull(_) => format!("Pulling {display_name}..."),
                        Action::RunGitPullRebase(_) => format!("Rebasing {display_name}..."),
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
                    let success = match action {
                        Action::GitPull(_) => format!("Pulled {display_name}"),
                        Action::RunGitPullRebase(_) => format!("Rebased {display_name}"),
                        Action::RunGitPullSubmodules(_) => {
                            format!("Pulled submodules for {display_name}")
                        }
                        Action::RunRemoveOriginRemote(_) => {
                            format!("Removed origin from {display_name}")
                        }
                        _ => unreachable!(),
                    };
                    self.spawn_git_operation(
                        repo_id,
                        move || crate::git::run_git_args(&path, &git_args),
                        move |_| success,
                        None,
                    );
                }
                Ok(true)
            }
            Action::GitSubmoduleUpdateLatest(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let name = self.repo_list.repos[idx]
                        .display_name_for_format(self.config.ui.repo_name_format, false);
                    self.confirm_dialog.show(
                        format!("Pull latest in all submodules for {name}?"),
                        Action::RunGitSubmoduleUpdateLatest(id.clone()),
                    );
                }
                Ok(true)
            }
            Action::GitSubmoduleUpdate(id)
            | Action::GitSubmoduleSync(id)
            | Action::RunGitSubmoduleUpdateLatest(id) => {
                if let Some(idx) = self.repo_list.resolve_index(id) {
                    let entry = &mut self.repo_list.repos[idx];
                    let git_args = match action {
                        Action::GitSubmoduleUpdate(_) => {
                            git_args(&["submodule", "update", "--init", "--recursive"])
                        }
                        Action::GitSubmoduleSync(_) => git_args(&["submodule", "sync"]),
                        Action::RunGitSubmoduleUpdateLatest(_) => {
                            git_args(&["submodule", "foreach", "git", "pull", "origin", "HEAD"])
                        }
                        _ => unreachable!(),
                    };
                    entry.git_op = true;
                    let display_name =
                        entry.display_name_for_format(self.config.ui.repo_name_format, false);
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
                    let success = match action {
                        Action::GitSubmoduleUpdate(_) => {
                            format!("Updated submodules in {display_name}")
                        }
                        Action::GitSubmoduleSync(_) => {
                            format!("Synced submodules in {display_name}")
                        }
                        Action::RunGitSubmoduleUpdateLatest(_) => {
                            format!("Pulled latest in submodules for {display_name}")
                        }
                        _ => unreachable!(),
                    };
                    self.spawn_git_operation(
                        repo_id,
                        move || crate::git::run_git_args(&path, &git_args),
                        move |_| success,
                        None,
                    );
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
