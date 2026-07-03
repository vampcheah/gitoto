use git2::{Repository, StatusOptions, SubmoduleStatus};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Debug)]
pub(crate) struct RepoStatus {
    pub branch: String,
    pub files: Vec<FileEntry>,
    pub ahead: usize,
    pub behind: usize,
    pub has_upstream: bool,
    pub is_dirty: bool,
    /// Linked worktrees (excludes the main working tree)
    pub worktree_info: Vec<WorktreeEntry>,
    /// True when .gitmodules exists (repo uses submodules)
    pub has_submodules: bool,
    pub submodules: Vec<SubmoduleInfo>,
    pub has_dirty_submodules: bool,
    /// True when the last `git fetch` failed (auth, network, timeout)
    pub fetch_failed: bool,
    /// True when any configured remote points at github.com.
    pub has_github_remote: bool,
    /// Browser URL for the first GitHub remote, when available.
    pub github_url: Option<String>,
    /// True when the repo has an `origin` remote.
    pub has_origin_remote: bool,
    /// Lightweight snapshot of HEAD/refs/worktree heads used to avoid graph reloads.
    pub graph_key: String,
    /// Snapshot of remote refs used to refresh pushed markers without rebuilding graph rows.
    pub remote_key: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UntrackedMode {
    /// Include untracked files and recursively scan untracked directories.
    #[default]
    All,
    /// Include untracked files without recursively expanding untracked directories.
    Normal,
    /// Ignore untracked files entirely.
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FileEntry {
    pub path: PathBuf,
    pub status: FileStatus,
    pub is_submodule: bool,
    pub submodule_state: Option<SubmoduleState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubmoduleState {
    Modified,
    Uninitialized,
    Dirty,
}

#[derive(Clone, Debug)]
pub(crate) struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SubmoduleInfo {
    pub path: PathBuf,
    pub state: SubmoduleState,
    pub head_oid: Option<String>,
    pub workdir_oid: Option<String>,
}

impl FileStatus {
    fn from_git_status(status: git2::Status) -> Option<Self> {
        if status.is_conflicted() {
            Some(Self::Conflicted)
        } else if status.is_index_new() || status.is_wt_new() {
            if status.is_wt_new() && !status.is_index_new() {
                Some(Self::Untracked)
            } else {
                Some(Self::Added)
            }
        } else if status.is_index_deleted() || status.is_wt_deleted() {
            Some(Self::Deleted)
        } else if status.is_index_renamed() || status.is_wt_renamed() {
            Some(Self::Renamed)
        } else if status.is_index_modified() || status.is_wt_modified() {
            Some(Self::Modified)
        } else {
            None
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Untracked => "?",
            Self::Conflicted => "C",
        }
    }
}

impl SubmoduleState {
    fn from_git_status(status: SubmoduleStatus) -> Option<Self> {
        if status.is_wd_uninitialized() {
            Some(Self::Uninitialized)
        } else if status.is_wd_wd_modified() || status.contains(SubmoduleStatus::WD_UNTRACKED) {
            Some(Self::Dirty)
        } else if status.is_wd_modified() || status.contains(SubmoduleStatus::WD_INDEX_MODIFIED) {
            Some(Self::Modified)
        } else {
            None
        }
    }
}

/// Fast local-only status query (no network). Used by filesystem watcher refreshes.
#[cfg(test)]
pub(crate) fn query_status(path: &Path, ignore_dirty_subs: bool) -> color_eyre::Result<RepoStatus> {
    query_status_with_untracked(path, ignore_dirty_subs, UntrackedMode::default())
}

#[cfg(test)]
pub(crate) fn query_status_with_untracked(
    path: &Path,
    ignore_dirty_subs: bool,
    untracked: UntrackedMode,
) -> color_eyre::Result<RepoStatus> {
    query_status_inner(
        path,
        false,
        ignore_dirty_subs,
        untracked,
        &crate::git::remote::default_github_hosts(),
    )
}

pub(crate) fn query_status_with_untracked_and_github_hosts(
    path: &Path,
    ignore_dirty_subs: bool,
    untracked: UntrackedMode,
    github_hosts: &[String],
) -> color_eyre::Result<RepoStatus> {
    query_status_inner(path, false, ignore_dirty_subs, untracked, github_hosts)
}

pub(crate) fn query_status_with_fetch_untracked_and_github_hosts(
    path: &Path,
    ignore_dirty_subs: bool,
    untracked: UntrackedMode,
    github_hosts: &[String],
) -> color_eyre::Result<RepoStatus> {
    query_status_inner(path, true, ignore_dirty_subs, untracked, github_hosts)
}

fn query_status_inner(
    path: &Path,
    fetch: bool,
    ignore_dirty_subs: bool,
    untracked: UntrackedMode,
    github_hosts: &[String],
) -> color_eyre::Result<RepoStatus> {
    let started = Instant::now();
    let repo = Repository::open(path)?;

    let branch = crate::git::current_branch_name(&repo, "(no branch)");

    // Only fetch remote-tracking refs when explicitly requested
    let fetch_failed = if fetch {
        !fetch_remote_silent(path)
    } else {
        false
    };

    let graph_key = graph_cache_key_from_repo(&repo);
    let remote_key = remote_cache_key_from_repo(&repo);

    // Ahead/behind
    let has_upstream = has_upstream(&repo);
    let (ahead, behind) = compute_ahead_behind(&repo);
    let github_url = crate::git::remote::github_remote_url(&repo, github_hosts);
    let has_github_remote = github_url.is_some();
    let has_origin_remote = crate::git::remote::has_origin_remote(&repo);

    // File statuses
    let mut opts = StatusOptions::new();
    opts.renames_head_to_index(true);

    match untracked {
        UntrackedMode::All => {
            opts.include_untracked(true).recurse_untracked_dirs(true);
        }
        UntrackedMode::Normal => {
            opts.include_untracked(true).recurse_untracked_dirs(false);
        }
        UntrackedMode::None => {
            opts.include_untracked(false);
        }
    }

    if ignore_dirty_subs {
        opts.exclude_submodules(true);
    }

    let mut files = collect_file_statuses(&repo, &mut opts)?;
    let has_regular_changes = !files.is_empty();

    // Collect linked worktree details (excludes the main working tree)
    let worktree_info = collect_worktree_info(&repo);

    // Detect submodules by checking for .gitmodules
    let has_submodules = path.join(".gitmodules").is_file();
    let submodules = collect_dirty_submodules(&repo, has_submodules, ignore_dirty_subs, &mut files);
    let has_dirty_submodules = !submodules.is_empty();

    let status = RepoStatus {
        branch,
        files,
        ahead,
        behind,
        has_upstream,
        is_dirty: has_regular_changes || has_dirty_submodules,
        worktree_info,
        has_submodules,
        submodules,
        has_dirty_submodules,
        fetch_failed,
        has_github_remote,
        github_url,
        has_origin_remote,
        graph_key,
        remote_key,
    };
    tracing::debug!(
        target: "gitoto::perf",
        path = %path.display(),
        fetch,
        untracked = ?untracked,
        files = status.files.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "status query completed"
    );
    Ok(status)
}

fn collect_file_statuses(
    repo: &Repository,
    opts: &mut StatusOptions,
) -> color_eyre::Result<Vec<FileEntry>> {
    Ok(repo
        .statuses(Some(opts))?
        .iter()
        .filter_map(|entry| {
            Some(FileEntry {
                path: PathBuf::from(entry.path().unwrap_or("")),
                status: FileStatus::from_git_status(entry.status())?,
                is_submodule: false,
                submodule_state: None,
            })
        })
        .collect())
}

fn collect_dirty_submodules(
    repo: &Repository,
    has_submodules: bool,
    ignore_dirty_subs: bool,
    files: &mut Vec<FileEntry>,
) -> Vec<SubmoduleInfo> {
    if !has_submodules || ignore_dirty_subs {
        return Vec::new();
    }
    let Ok(subs) = repo.submodules() else {
        return Vec::new();
    };

    subs.iter()
        .filter_map(|sub| {
            let name = sub.name().unwrap_or("");
            let sub_path = PathBuf::from(sub.path());
            let state = SubmoduleState::from_git_status(
                repo.submodule_status(name, git2::SubmoduleIgnore::Unspecified)
                    .unwrap_or(SubmoduleStatus::empty()),
            )?;

            mark_submodule_file(files, sub_path.clone(), state);
            Some(SubmoduleInfo {
                path: sub_path,
                state,
                head_oid: sub.head_id().map(|id| id.to_string()),
                workdir_oid: sub.workdir_id().map(|id| id.to_string()),
            })
        })
        .collect()
}

fn mark_submodule_file(files: &mut Vec<FileEntry>, sub_path: PathBuf, state: SubmoduleState) {
    if let Some(file_entry) = files.iter_mut().find(|file| file.path == sub_path) {
        file_entry.is_submodule = true;
        file_entry.submodule_state = Some(state);
    } else {
        files.push(FileEntry {
            path: sub_path,
            status: FileStatus::Modified,
            is_submodule: true,
            submodule_state: Some(state),
        });
    }
}

fn graph_cache_key_from_repo(repo: &Repository) -> String {
    let mut parts = Vec::new();

    let head = repo
        .head()
        .ok()
        .and_then(|head| {
            let name = head.name().unwrap_or("HEAD");
            head.target().map(|oid| format!("HEAD:{name}:{oid}"))
        })
        .unwrap_or_else(|| "HEAD:unborn".to_string());
    parts.push(head);

    parts.extend(ref_cache_parts(repo, |name| {
        name.starts_with("refs/heads/") || name.starts_with("refs/tags/")
    }));

    for (name, path) in crate::git::linked_worktrees(repo) {
        let Ok(wt_repo) = Repository::open(&path) else {
            continue;
        };
        let head = wt_repo
            .head()
            .ok()
            .and_then(|head| {
                let branch = head.shorthand().unwrap_or("HEAD");
                head.target()
                    .map(|oid| format!("worktree:{name}:{branch}:{oid}"))
            })
            .unwrap_or_else(|| format!("worktree:{name}:unborn"));
        parts.push(head);
    }

    sorted_cache_key(parts)
}

fn remote_cache_key_from_repo(repo: &Repository) -> String {
    sorted_cache_key(ref_cache_parts(repo, |name| {
        name.starts_with("refs/remotes/")
    }))
}

fn ref_cache_parts(repo: &Repository, include: impl Fn(&str) -> bool) -> Vec<String> {
    let mut parts = Vec::new();
    if let Ok(mut refs) = repo.references() {
        while let Some(Ok(reference)) = refs.next() {
            let Some(name) = reference.name() else {
                continue;
            };
            if !include(name) {
                continue;
            }
            if let Some(oid) = crate::git::reference_oid(&reference) {
                parts.push(format!("{name}:{oid}"));
            }
        }
    }
    parts
}

fn sorted_cache_key(mut parts: Vec<String>) -> String {
    parts.sort_unstable();
    parts.join("|")
}

fn has_upstream(repo: &Repository) -> bool {
    let Ok(head) = repo.head() else {
        return false;
    };
    let Some(branch_name) = head.shorthand() else {
        return false;
    };
    repo.find_branch(branch_name, git2::BranchType::Local)
        .and_then(|branch| branch.upstream())
        .is_ok()
}

/// Collect details for each linked worktree using the git2 API.
/// Mirrors the pattern in `git/graph.rs::collect_worktree_branches`.
fn collect_worktree_info(repo: &Repository) -> Vec<WorktreeEntry> {
    crate::git::linked_worktrees(repo)
        .into_iter()
        .filter_map(|(_, wt_path)| {
            let wt_repo = Repository::open(&wt_path).ok()?;
            Some(WorktreeEntry {
                path: wt_path,
                branch: crate::git::current_branch_name(&wt_repo, "(no branch)"),
            })
        })
        .collect()
}

/// Run `git fetch` with a 30-second timeout to update remote-tracking refs.
/// Uses the CLI because git2 fetch doesn't support SSH agent / credential helpers
/// out of the box. Returns `true` on success, `false` on failure/timeout.
fn fetch_remote_silent(path: &Path) -> bool {
    use wait_timeout::ChildExt;

    let mut command = std::process::Command::new("git");
    command.arg("-C").arg(path).args(["fetch", "--quiet"]);
    crate::git::configure_noninteractive(&mut command);

    let child = command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match child {
        Ok(mut c) => {
            match c.wait_timeout(std::time::Duration::from_secs(30)) {
                Ok(Some(status)) => status.success(),
                Ok(None) => {
                    // Timed out — kill the hung process
                    let _ = c.kill();
                    let _ = c.wait();
                    false
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

fn compute_ahead_behind(repo: &Repository) -> (usize, usize) {
    let head = match repo.head() {
        Ok(h) => h,
        Err(_) => return (0, 0),
    };

    let local_oid = match head.target() {
        Some(oid) => oid,
        None => return (0, 0),
    };

    let branch_name = match head.shorthand() {
        Some(name) => name.to_string(),
        None => return (0, 0),
    };

    // Use git2's branch upstream tracking instead of hardcoding "origin"
    let branch = match repo.find_branch(&branch_name, git2::BranchType::Local) {
        Ok(b) => b,
        Err(_) => return (0, 0),
    };

    let upstream = match branch.upstream() {
        Ok(u) => u,
        Err(_) => return (0, 0),
    };

    let upstream_oid = match upstream.get().target() {
        Some(oid) => oid,
        None => return (0, 0),
    };

    repo.graph_ahead_behind(local_oid, upstream_oid)
        .unwrap_or((0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support;
    use std::fs;
    use tempfile::TempDir;

    fn has_file_status(status: &RepoStatus, file_status: FileStatus) -> bool {
        status.files.iter().any(|f| f.status == file_status)
    }

    fn assert_no_dirty_submodules(status: &RepoStatus) {
        assert!(!status.has_dirty_submodules);
        assert!(status.submodules.is_empty());
    }

    fn assert_clean_submodule_status(status: &RepoStatus) {
        assert!(status.has_submodules);
        assert_no_dirty_submodules(status);
        assert!(!status.files.iter().any(|f| f.is_submodule));
    }

    fn assert_submodule_state(status: &RepoStatus, state: SubmoduleState) {
        assert!(status.has_submodules);
        assert!(status.has_dirty_submodules);
        let sub_info = &status.submodules[0];
        assert_eq!(sub_info.path, Path::new("my-sub"));
        assert_eq!(sub_info.state, state);
    }

    #[test]
    fn test_clean_repo_reports_no_changes() {
        let (tmp, _repo) = test_support::init_repo_with_commit();
        let status = query_status(tmp.path(), false).unwrap();
        assert!(!status.is_dirty);
        assert!(status.files.is_empty());
        assert!(!status.has_github_remote);
        assert!(!status.has_origin_remote);
    }

    #[test]
    fn test_github_remote_detected() {
        let (tmp, repo) = test_support::init_repo_with_commit();
        repo.remote("origin", "git@github.com:owner/repo.git")
            .unwrap();

        let status = query_status(tmp.path(), false).unwrap();

        assert!(status.has_github_remote);
        assert_eq!(
            status.github_url.as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert!(status.has_origin_remote);
    }

    #[test]
    fn test_modified_file_detected() {
        let (tmp, repo) = test_support::init_repo_with_commit();

        let head = repo.head().unwrap().peel_to_commit().unwrap();
        test_support::write_commit(&repo, tmp.path(), "test.txt", "hello", "Add file", &[&head]);

        // Modify it
        fs::write(tmp.path().join("test.txt"), "world").unwrap();

        let status = query_status(tmp.path(), false).unwrap();
        assert!(status.is_dirty);
        assert!(has_file_status(&status, FileStatus::Modified));
    }

    #[test]
    fn test_untracked_file_detected() {
        let (tmp, _repo) = test_support::init_repo_with_commit();
        fs::write(tmp.path().join("new.txt"), "new").unwrap();

        let status = query_status(tmp.path(), false).unwrap();
        assert!(status.is_dirty);
        assert!(has_file_status(&status, FileStatus::Untracked));
    }

    #[test]
    fn test_worktree_info_empty_for_plain_repo() {
        let (tmp, _repo) = test_support::init_repo_with_commit();
        let status = query_status(tmp.path(), false).unwrap();
        assert!(status.worktree_info.is_empty());
    }

    #[test]
    fn test_worktree_info_reflects_linked_worktrees() {
        let (tmp, _repo) = test_support::init_repo_with_commit();
        // Create a linked worktree via git CLI
        let wt_dir = tmp.path().join("wt1");
        let wt_dir = wt_dir.to_string_lossy();
        test_support::git_ok(
            tmp.path(),
            &["worktree", "add", &wt_dir, "-b", "wt-branch"],
            "git worktree add",
        );

        let status = query_status(tmp.path(), false).unwrap();
        assert_eq!(status.worktree_info.len(), 1);
        assert_eq!(status.worktree_info[0].branch, "wt-branch");
    }

    #[test]
    fn test_submodule_state_mapping() {
        for (git_status, state) in [
            (
                SubmoduleStatus::WD_UNINITIALIZED,
                SubmoduleState::Uninitialized,
            ),
            (SubmoduleStatus::WD_WD_MODIFIED, SubmoduleState::Dirty),
            (SubmoduleStatus::WD_UNTRACKED, SubmoduleState::Dirty),
            (SubmoduleStatus::WD_MODIFIED, SubmoduleState::Modified),
            (SubmoduleStatus::WD_INDEX_MODIFIED, SubmoduleState::Modified),
            (
                SubmoduleStatus::WD_UNINITIALIZED | SubmoduleStatus::WD_MODIFIED,
                SubmoduleState::Uninitialized,
            ),
            (
                SubmoduleStatus::WD_WD_MODIFIED | SubmoduleStatus::WD_MODIFIED,
                SubmoduleState::Dirty,
            ),
        ] {
            assert_eq!(SubmoduleState::from_git_status(git_status), Some(state));
        }
    }

    #[test]
    fn test_clean_repo_no_dirty_submodules() {
        let (tmp, _repo) = test_support::init_repo_with_commit();
        let status = query_status(tmp.path(), false).unwrap();
        assert_no_dirty_submodules(&status);
    }

    #[test]
    fn test_status_maps_correctly() {
        for (status, label) in [
            (FileStatus::Modified, "M"),
            (FileStatus::Added, "A"),
            (FileStatus::Deleted, "D"),
            (FileStatus::Renamed, "R"),
            (FileStatus::Untracked, "?"),
            (FileStatus::Conflicted, "C"),
        ] {
            assert_eq!(status.label(), label);
        }
    }

    #[test]
    fn test_ignore_dirty_subs_on_clean_repo() {
        // ignore_dirty_subs = true should work fine on repos without submodules
        let (tmp, _repo) = test_support::init_repo_with_commit();
        let status = query_status(tmp.path(), true).unwrap();
        assert!(!status.is_dirty);
        assert!(status.files.is_empty());
        assert_no_dirty_submodules(&status);
    }

    #[test]
    fn test_ignore_dirty_subs_still_detects_regular_changes() {
        let (tmp, _repo) = test_support::init_repo_with_commit();
        fs::write(tmp.path().join("new.txt"), "new").unwrap();

        let status = query_status(tmp.path(), true).unwrap();
        assert!(status.is_dirty);
        assert!(has_file_status(&status, FileStatus::Untracked));
        assert_no_dirty_submodules(&status);
    }

    /// Helper: creates a temp repo with a submodule, returns (parent_tmp, sub_source_tmp, sub_repo)
    fn init_repo_with_submodule() -> (TempDir, TempDir, Repository) {
        let (tmp, _repo) = test_support::init_repo_with_commit();

        let (sub_source, sub_repo) = test_support::init_repo();
        test_support::write_commit(
            &sub_repo,
            sub_source.path(),
            "lib.rs",
            "fn hello() {}",
            "init sub",
            &[],
        );

        // Add submodule (requires protocol.file.allow for local paths)
        test_support::git_ok(
            tmp.path(),
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                sub_source.path().to_str().unwrap(),
                "my-sub",
            ],
            "git submodule add",
        );

        test_support::git_ok(tmp.path(), &["add", "."], "git add submodule files");
        test_support::git_commit(tmp.path(), "add submodule");

        (tmp, sub_source, sub_repo)
    }

    #[test]
    fn test_dirty_submodule_with_real_git_submodule() {
        let (tmp, _sub_source, _sub_repo) = init_repo_with_submodule();

        // Verify: clean state should show has_submodules but no dirty submodules
        let status = query_status(tmp.path(), false).unwrap();
        assert_clean_submodule_status(&status);

        // Now make the submodule dirty by modifying a file inside it
        let sub_workdir = tmp.path().join("my-sub");
        fs::write(sub_workdir.join("lib.rs"), "fn hello() { /* changed */ }").unwrap();

        let status = query_status(tmp.path(), false).unwrap();
        assert_submodule_state(&status, SubmoduleState::Dirty);

        // Verify the file entry is annotated
        let file_entry = status.files.iter().find(|f| f.path == Path::new("my-sub"));
        assert!(file_entry.is_some());
        let file_entry = file_entry.unwrap();
        assert!(file_entry.is_submodule);
        assert_eq!(file_entry.submodule_state, Some(SubmoduleState::Dirty));
    }

    #[test]
    fn test_ignore_dirty_subs_hides_submodule_state() {
        let (tmp, _sub_source, _sub_repo) = init_repo_with_submodule();

        // Make the submodule dirty
        fs::write(tmp.path().join("my-sub/lib.rs"), "fn changed() {}").unwrap();

        // With ignore_dirty_subs = true, submodule state should be hidden
        let status = query_status(tmp.path(), true).unwrap();
        assert_clean_submodule_status(&status);
    }

    #[test]
    fn test_submodule_modified_pointer() {
        let (tmp, _sub_source, sub_repo) = init_repo_with_submodule();

        // Add a new commit to the submodule source
        {
            test_support::write_and_stage(&sub_repo, _sub_source.path(), "lib.rs", "v2");
            let head = sub_repo.head().unwrap().peel_to_commit().unwrap();
            test_support::commit(&sub_repo, "v2", &[&head]);
        }

        // Pull the new commit inside the submodule workdir
        let submodule_path = tmp.path().join("my-sub");
        let output = test_support::git(
            &submodule_path,
            &[
                "-c",
                "protocol.file.allow=always",
                "pull",
                "origin",
                "master",
            ],
        );
        // Try main if master fails
        if !output.status.success() {
            let _ = test_support::git(
                &submodule_path,
                &["-c", "protocol.file.allow=always", "pull", "origin", "main"],
            );
        }

        // Now the submodule pointer has changed (HEAD in submodule != recorded in parent)
        let status = query_status(tmp.path(), false).unwrap();
        let sub_info = &status.submodules[0];
        assert!(status.has_submodules);
        assert!(status.has_dirty_submodules);
        assert_eq!(sub_info.path, Path::new("my-sub"));
        // Could be Modified or Dirty depending on exact git state
        assert!(
            sub_info.state == SubmoduleState::Modified || sub_info.state == SubmoduleState::Dirty,
            "expected Modified or Dirty, got {:?}",
            sub_info.state
        );

        // Verify OIDs are populated
        assert!(sub_info.head_oid.is_some());
        assert!(sub_info.workdir_oid.is_some());
        assert_ne!(sub_info.head_oid, sub_info.workdir_oid);
    }

    #[test]
    fn test_clean_submodule_not_reported() {
        let (tmp, _sub_source, _sub_repo) = init_repo_with_submodule();

        // Without any modifications, the submodule should be clean
        let status = query_status(tmp.path(), false).unwrap();
        assert_clean_submodule_status(&status);
    }

    #[test]
    fn test_dirty_submodule_makes_repo_dirty() {
        let (tmp, _sub_source, _sub_repo) = init_repo_with_submodule();

        // Start clean
        let status = query_status(tmp.path(), false).unwrap();
        assert!(!status.is_dirty);

        // Make submodule dirty
        fs::write(tmp.path().join("my-sub/lib.rs"), "dirty").unwrap();

        // Now repo should be dirty
        let status = query_status(tmp.path(), false).unwrap();
        assert!(status.is_dirty);
    }
}
