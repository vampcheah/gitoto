pub(crate) mod commit_files;
pub(crate) mod graph;
pub(crate) mod graph_render;
pub(crate) mod scanner;
pub(crate) mod status;

use color_eyre::eyre::{Context, Result, eyre};
use git2::Repository;
use std::path::Path;
use std::process::{Command, Stdio};

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

pub(crate) fn push(repo_path: &Path) -> Result<String> {
    run_git(repo_path, &["push"])
}

pub(crate) fn publish(repo_path: &Path, branch: &str) -> Result<String> {
    if branch.trim().is_empty() || branch == "(no branch)" || branch == "HEAD" {
        return Err(eyre!("cannot publish detached HEAD"));
    }

    run_git(repo_path, &["push", "-u", "origin", branch])
}

pub(crate) fn create_github_repo(repo_path: &Path, private: bool) -> Result<String> {
    let name = repo_path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| eyre!("invalid repository name"))?;
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

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo_path).args(args);
    configure_noninteractive(&mut command);
    let output = command.output()?;

    command_output_to_result(&format!("git {}", args.join(" ")), output)
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
