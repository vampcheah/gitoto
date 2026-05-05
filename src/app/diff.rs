use color_eyre::Result;
use std::path::{Path, PathBuf};

use crate::action::Action;
use crate::app::App;
use crate::git::status::SubmoduleState;

impl App {
    pub(super) fn handle_diff_action(&mut self, action: &Action) -> Result<bool> {
        match action {
            Action::ShowDiff(id, file_path) => {
                if let Some(entry) = self.repo_list.resolve_entry(id) {
                    let diff_gen = self.file_list.diff_generation();
                    let sub_info = entry
                        .status
                        .as_ref()
                        .and_then(|s| s.submodules.iter().find(|sm| sm.path == *file_path));

                    if let Some(sub) = sub_info {
                        self.spawn_submodule_diff(
                            entry.path.clone(),
                            file_path.clone(),
                            sub.head_oid.clone().unwrap_or_default(),
                            sub.workdir_oid.clone().unwrap_or_default(),
                            sub.state,
                            diff_gen,
                        );
                    } else {
                        let path = self
                            .active_worktree
                            .as_ref()
                            .map(|aw| aw.path.clone())
                            .unwrap_or_else(|| entry.path.clone());
                        self.spawn_file_diff(path, file_path.clone(), diff_gen);
                    }
                }
                Ok(true)
            }
            Action::DiffLoaded {
                generation,
                content,
            } => {
                if *generation == self.file_list.diff_generation() {
                    self.file_list.set_diff(content.clone());
                }
                Ok(true)
            }
            Action::ShowCommitFiles { repo_path, oid } => {
                self.spawn_commit_files_load(repo_path.clone(), oid.clone());
                Ok(true)
            }
            Action::CommitFilesLoaded {
                generation,
                oid,
                message,
                files,
            } => {
                if *generation == self.git_graph.current_detail_generation() {
                    self.git_graph
                        .set_commit_files(oid.clone(), message.clone(), files.clone());
                }
                Ok(true)
            }
            Action::ShowCommitDiff {
                repo_path,
                oid,
                file_path,
            } => {
                self.spawn_commit_diff_load(repo_path.clone(), oid.clone(), file_path.clone());
                Ok(true)
            }
            Action::CommitDiffLoaded {
                generation,
                content,
            } => {
                if *generation == self.git_graph.current_detail_generation() {
                    self.git_graph.set_commit_diff(content.clone());
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn spawn_submodule_diff(
        &self,
        repo_path: PathBuf,
        sub_path: PathBuf,
        old_oid: String,
        new_oid: String,
        sub_state: SubmoduleState,
        diff_gen: u64,
    ) {
        let tx = self.action_tx.clone();
        tokio::task::spawn_blocking(move || {
            let submodule_abs = repo_path.join(&sub_path);
            let pointer_changed = !old_oid.is_empty() && !new_oid.is_empty() && old_oid != new_oid;
            let use_diff = sub_state == SubmoduleState::Dirty || !pointer_changed;

            let content = if use_diff {
                format!(
                    "{}{}",
                    section_header(format!(
                        "Submodule {} ({})",
                        sub_path.display(),
                        submodule_state_label(sub_state)
                    )),
                    submodule_diff_body(&submodule_abs),
                )
            } else {
                let range = format!("{}..{}", old_oid, new_oid);
                format!(
                    "{}{}",
                    section_header(format!(
                        "Submodule {} → {}",
                        short_oid(&old_oid),
                        short_oid(&new_oid)
                    )),
                    submodule_log_body(&submodule_abs, &range),
                )
            };

            send_diff_loaded(&tx, diff_gen, content);
        });
    }

    fn spawn_file_diff(&self, path: PathBuf, file_path: PathBuf, diff_gen: u64) {
        let tx = self.action_tx.clone();
        tokio::task::spawn_blocking(move || {
            send_diff_loaded(&tx, diff_gen, file_diff_body(&path, &file_path));
        });
    }

    fn spawn_commit_files_load(&self, path: PathBuf, oid: String) {
        let detail_gen = self.git_graph.current_detail_generation();
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
                    let _ = tx.send(Action::Error(format!("Failed to list commit files: {}", e)));
                }
            }
        });
    }

    fn spawn_commit_diff_load(&self, path: PathBuf, oid: String, fp: String) {
        let detail_gen = self.git_graph.current_detail_generation();
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
                    let _ = tx.send(Action::Error(format!("Failed to get commit diff: {}", e)));
                }
            }
        });
    }
}

fn send_diff_loaded(
    tx: &tokio::sync::mpsc::UnboundedSender<Action>,
    generation: u64,
    content: String,
) {
    let _ = tx.send(Action::DiffLoaded {
        generation,
        content,
    });
}

fn section_header(title: impl std::fmt::Display) -> String {
    format!("{title}\n{}\n", "─".repeat(40))
}

fn short_oid(oid: &str) -> &str {
    oid.get(..7).unwrap_or(oid)
}

fn submodule_state_label(state: SubmoduleState) -> &'static str {
    match state {
        SubmoduleState::Dirty => "uncommitted changes",
        SubmoduleState::Uninitialized => "not initialized",
        SubmoduleState::Modified => "modified",
    }
}

fn submodule_diff_body(submodule_abs: &Path) -> String {
    match crate::git::git_stdout(submodule_abs, &["diff", "HEAD"]) {
        Ok(text) => {
            if !text.is_empty() {
                return text;
            }
            submodule_status_body(submodule_abs)
        }
        Err(e) => format!("Failed to get submodule diff: {}", e),
    }
}

fn submodule_status_body(submodule_abs: &Path) -> String {
    let status = crate::git::git_stdout(submodule_abs, &["status", "--short"]).unwrap_or_default();
    if status.is_empty() {
        "(no changes detected)".to_string()
    } else {
        status
    }
}

fn submodule_log_body(submodule_abs: &Path, range: &str) -> String {
    match crate::git::git_stdout(submodule_abs, &["log", "--oneline", "--graph", range]) {
        Ok(text) => {
            if text.is_empty() {
                "(no commits in range)".to_string()
            } else {
                text
            }
        }
        Err(e) => format!("Failed to get submodule log: {}", e),
    }
}

fn file_diff_body(path: &Path, file_path: &Path) -> String {
    match crate::git::git_stdout_with_path(path, &["diff", "HEAD", "--"], file_path) {
        Ok(mut text) => {
            if text.is_empty() {
                text = new_file_diff_body(path, file_path);
            }
            if text.is_empty() {
                "(no diff available)".to_string()
            } else {
                text
            }
        }
        Err(e) => format!("Failed to get diff: {}", e),
    }
}

fn new_file_diff_body(path: &Path, file_path: &Path) -> String {
    crate::git::git_stdout_with_path(path, &["diff", "--no-index", "/dev/null"], file_path)
        .unwrap_or_default()
}
