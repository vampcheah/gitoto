pub(crate) mod commit_files;
pub(crate) mod graph;
pub(crate) mod graph_render;
pub(crate) mod remote;
pub(crate) mod scanner;
pub(crate) mod status;
#[cfg(test)]
pub(crate) mod test_support;

use color_eyre::eyre::{Context, Result, eyre};
use git2::Repository;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn current_branch_name(repo: &Repository, fallback: &str) -> String {
    repo.head()
        .ok()
        .and_then(|reference| reference.shorthand().map(str::to_string))
        .unwrap_or_else(|| fallback.to_string())
}

pub(crate) fn reference_oid(reference: &git2::Reference<'_>) -> Option<git2::Oid> {
    reference
        .peel_to_commit()
        .ok()
        .map(|commit| commit.id())
        .or_else(|| reference.target())
}

pub(crate) fn linked_worktrees(repo: &Repository) -> Vec<(String, PathBuf)> {
    let Ok(names) = repo.worktrees() else {
        return Vec::new();
    };

    names
        .iter()
        .flatten()
        .filter_map(|name| {
            let path = repo.find_worktree(name).ok()?.path().to_path_buf();
            Some((name.to_string(), path))
        })
        .collect()
}

pub(crate) fn commit_all(repo_path: &Path, message: &str, no_verify: bool) -> Result<String> {
    if message.trim().is_empty() {
        return Err(eyre!("commit message is empty"));
    }

    run_git(repo_path, &["add", "."])?;
    let mut args = vec!["commit"];
    if no_verify {
        args.push("--no-verify");
    }
    args.extend(["-m", message]);
    run_git(repo_path, &args)
}

/// Commit only the given paths: stages them, then commits with a pathspec so
/// other staged or dirty files stay untouched.
pub(crate) fn commit_paths(
    repo_path: &Path,
    message: &str,
    no_verify: bool,
    paths: &[std::path::PathBuf],
) -> Result<String> {
    if message.trim().is_empty() {
        return Err(eyre!("commit message is empty"));
    }
    if paths.is_empty() {
        return Err(eyre!("no files marked"));
    }

    let path_args: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    let mut add_args: Vec<String> = vec!["add".into(), "--".into()];
    add_args.extend(path_args.iter().cloned());
    run_git_args(repo_path, &add_args)?;

    let mut args: Vec<String> = vec!["commit".into()];
    if no_verify {
        args.push("--no-verify".into());
    }
    args.extend(["-m".into(), message.to_string()]);
    args.push("--".into());
    args.extend(path_args);
    run_git_args(repo_path, &args)
}

pub(crate) fn push(repo_path: &Path) -> Result<String> {
    run_git(repo_path, &["push"])
}

pub(crate) fn publish(repo_path: &Path, branch: &str) -> Result<String> {
    if branch.trim().is_empty() || branch == "(no branch)" || branch == "HEAD" {
        return Err(eyre!("cannot publish detached HEAD"));
    }

    run_git(repo_path, &["push", "-u", "origin", branch])
}

pub(crate) fn create_github_repo(repo_path: &Path, name: &str, private: bool) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(eyre!("repository name is empty"));
    }
    ensure_github_auth()?;
    let remote = github_remote_name(repo_path)?;
    let visibility = if private { "--private" } else { "--public" };

    let mut command = Command::new("gh");
    command
        .args(["repo", "create", name, visibility])
        .arg("--source")
        .arg(repo_path)
        .args(["--remote", &remote, "--push"]);
    configure_noninteractive(&mut command);
    command
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1");
    let output = command
        .output()
        .context("failed to run gh; install GitHub CLI and run `gh auth login`")?;

    command_output_to_result("gh repo create", output)
}

fn ensure_github_auth() -> Result<()> {
    let mut command = Command::new("gh");
    command.args(["auth", "status"]);
    configure_noninteractive(&mut command);
    command
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1");
    let output = command
        .output()
        .context("failed to run gh; install GitHub CLI and run `gh auth login`")?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = stderr
        .lines()
        .chain(stdout.lines())
        .find(|line| !line.trim().is_empty())
        .unwrap_or("not logged in to GitHub")
        .trim();
    Err(eyre!(
        "GitHub authentication required: {detail}. Run `gh auth login` and `gh auth setup-git`."
    ))
}

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_path).args(args);
    configure_noninteractive(&mut command);
    let output = command.output()?;

    command_output_to_result(&format!("git {}", args.join(" ")), output)
}

pub(crate) fn git_stdout(repo_path: &Path, args: &[&str]) -> std::io::Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_path).args(args);
    configure_noninteractive(&mut command);
    command_stdout(command)
}

pub(crate) fn git_stdout_with_path(
    repo_path: &Path,
    args: &[&str],
    path: &Path,
) -> std::io::Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_path).args(args).arg(path);
    configure_noninteractive(&mut command);
    command_stdout(command)
}

pub(crate) fn run_git_args(repo_path: &Path, args: &[String]) -> Result<String> {
    let command = format!("git {}", args.join(" "));
    let mut child = Command::new("git");
    child.arg("-C").arg(repo_path).args(args);
    configure_noninteractive(&mut child);
    let output = child.output().map_err(|e| eyre!("{command} failed: {e}"))?;

    command_output_to_result(&command, output)
}

pub(crate) fn configure_noninteractive(command: &mut Command) {
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "true")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::null());
}

fn command_output_to_result(command: &str, output: std::process::Output) -> Result<String> {
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let text = if stdout.is_empty() { stderr } else { stdout };
        return Ok(text);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = stderr
        .lines()
        .chain(stdout.lines())
        .find(|line| !line.trim().is_empty())
        .unwrap_or("unknown git error")
        .trim()
        .to_string();

    Err(eyre!(
        "{} failed: {}",
        command,
        explain_git_failure(&message)
    ))
}

fn command_stdout(mut command: Command) -> std::io::Result<String> {
    command
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
}

fn explain_git_failure(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("authentication failed")
        || lower.contains("could not read username")
        || lower.contains("could not read password")
        || lower.contains("terminal prompts disabled")
        || lower.contains("no anonymous write access")
        || lower.contains("write access to repository not granted")
        || lower.contains("repository not found")
        || lower.contains("permission denied")
        || lower.contains("authentication required")
        || lower.contains("403")
    {
        return format!(
            "{message}. Authentication or repository write permission is required. Run `gh auth login` and `gh auth setup-git`, switch the remote to SSH, or ask for write access to this repository."
        );
    }

    if lower.contains("could not resolve host") || lower.contains("failed to connect") {
        return format!("{message}. Network access failed; check your connection or remote URL.");
    }

    message.to_string()
}

fn github_remote_name(repo_path: &Path) -> Result<String> {
    let repo = Repository::open(repo_path)?;

    if repo.find_remote("origin").is_err() {
        return Ok("origin".to_string());
    }
    if repo.find_remote("github").is_err() {
        return Ok("github".to_string());
    }

    Err(eyre!(
        "cannot add GitHub remote: both 'origin' and 'github' already exist"
    ))
}

pub(crate) fn revert_file(repo_path: &Path, file_path: &Path) -> Result<String> {
    let full_path = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        repo_path.join(file_path)
    };

    let file_str = file_path.to_string_lossy().into_owned();

    // 1. Try checkout HEAD -- <file_path>
    let checkout_res = run_git_args(
        repo_path,
        &[
            "checkout".into(),
            "HEAD".into(),
            "--".into(),
            file_str.clone(),
        ],
    );

    if checkout_res.is_ok() {
        if full_path.exists() {
            let _ = run_git_args(
                repo_path,
                &[
                    "clean".into(),
                    "-f".into(),
                    "-d".into(),
                    "--".into(),
                    file_str,
                ],
            );
        }
        return Ok(format!("Reverted {}", file_path.display()));
    }

    // 2. If checkout HEAD failed (e.g. untracked file or newly added file not in HEAD):
    let _ = run_git_args(
        repo_path,
        &["reset".into(), "HEAD".into(), "--".into(), file_str],
    );

    // 3. Remove untracked file or directory
    if full_path.exists() {
        if full_path.is_dir() {
            std::fs::remove_dir_all(&full_path).map_err(|e| {
                eyre!(
                    "Failed to remove untracked directory {}: {}",
                    file_path.display(),
                    e
                )
            })?;
        } else {
            std::fs::remove_file(&full_path).map_err(|e| {
                eyre!(
                    "Failed to remove untracked file {}: {}",
                    file_path.display(),
                    e
                )
            })?;
        }
    }

    Ok(format!("Reverted {}", file_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support::init_repo_with_commit;
    use std::path::PathBuf;

    #[test]
    fn test_commit_paths_commits_only_marked_files() {
        let (tmp, repo) = init_repo_with_commit();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@test.com").unwrap();

        std::fs::write(tmp.path().join("keep.txt"), "keep").unwrap();
        std::fs::write(tmp.path().join("skip.txt"), "skip").unwrap();

        commit_paths(tmp.path(), "partial", false, &[PathBuf::from("keep.txt")]).unwrap();

        let statuses = repo.statuses(None).unwrap();
        let dirty: Vec<String> = statuses
            .iter()
            .filter_map(|s| s.path().map(str::to_string))
            .collect();
        assert_eq!(dirty, vec!["skip.txt".to_string()]);
    }

    #[test]
    fn test_commit_paths_rejects_empty_selection() {
        let (tmp, _repo) = init_repo_with_commit();
        assert!(commit_paths(tmp.path(), "msg", false, &[]).is_err());
    }

    #[test]
    fn test_revert_file_modified() {
        let (tmp, repo) = crate::git::test_support::init_repo();
        crate::git::test_support::write_commit(
            &repo,
            tmp.path(),
            "test.txt",
            "hello",
            "initial",
            &[],
        );
        let test_file = tmp.path().join("test.txt");
        std::fs::write(&test_file, "modified content").unwrap();

        revert_file(tmp.path(), &PathBuf::from("test.txt")).unwrap();

        let content = std::fs::read_to_string(&test_file).unwrap();
        assert_eq!(content, "hello");

        let statuses = repo.statuses(None).unwrap();
        assert!(statuses.is_empty());
    }

    #[test]
    fn test_revert_file_untracked() {
        let (tmp, repo) = init_repo_with_commit();
        let new_file = tmp.path().join("new_file.txt");
        std::fs::write(&new_file, "new content").unwrap();

        revert_file(tmp.path(), &PathBuf::from("new_file.txt")).unwrap();

        assert!(!new_file.exists());
        let statuses = repo.statuses(None).unwrap();
        assert!(statuses.is_empty());
    }
}
