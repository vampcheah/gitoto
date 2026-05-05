use std::path::PathBuf;
use std::process::Stdio;

use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::repo_id::RepoId;

#[derive(Clone, Copy)]
pub(super) enum StatusQuery {
    Local,
    Fetch,
}

#[derive(Clone, Copy)]
pub(super) enum StatusFailure {
    UserVisible,
    Debug(&'static str),
}

#[derive(Clone)]
pub(super) struct ActiveWorktree {
    pub path: PathBuf,
    pub repo_id: RepoId,
    pub display_name: String,
    pub graph_key: Option<String>,
}

pub(super) fn git_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

pub(super) fn open_browser_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

/// RAII guard that sends `StatusQueryDone` if the spawned task exits
/// without sending a completion message (e.g., on panic).
pub(super) struct StatusGuard {
    id: RepoId,
    tx: UnboundedSender<Action>,
    completed: bool,
}

impl StatusGuard {
    pub(super) fn new(id: RepoId, tx: UnboundedSender<Action>) -> Self {
        Self {
            id,
            tx,
            completed: false,
        }
    }

    pub(super) fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for StatusGuard {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.tx.send(Action::StatusQueryDone(self.id.clone()));
        }
    }
}

/// RAII guard for git operations that set `git_op = true`.
pub(super) struct GitOpGuard {
    id: RepoId,
    tx: UnboundedSender<Action>,
    completed: bool,
}

impl GitOpGuard {
    pub(super) fn new(id: RepoId, tx: UnboundedSender<Action>) -> Self {
        Self {
            id,
            tx,
            completed: false,
        }
    }

    pub(super) fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for GitOpGuard {
    fn drop(&mut self) {
        if !self.completed {
            let _ = self.tx.send(Action::RefreshRepo(self.id.clone()));
        }
    }
}

/// Simple base64 encoder for OSC 52 clipboard.
pub(super) fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[(n >> 18 & 0x3f) as usize] as char);
        result.push(CHARS[(n >> 12 & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[(n >> 6 & 0x3f) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 0x3f) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

pub(super) fn copy_to_clipboard(text: &str) {
    use std::io::Write;

    let encoded = base64_encode(text.as_bytes());
    let _ = write!(std::io::stdout(), "\x1b]52;c;{}\x1b\\", encoded);
    let _ = std::io::stdout().flush();
}

pub(super) fn is_valid_github_repo_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}
