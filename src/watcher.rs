use notify_debouncer_full::{
    DebounceEventResult, Debouncer, NoCache, new_debouncer_opt,
    notify::{Config, RecommendedWatcher, RecursiveMode},
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use walkdir::WalkDir;

use crate::event::Event;

pub(crate) struct RepoWatcher {
    _debouncer: Arc<Mutex<Debouncer<RecommendedWatcher, NoCache>>>,
}

impl RepoWatcher {
    pub fn new(
        repo_paths: &[PathBuf],
        debounce_ms: u64,
        event_tx: UnboundedSender<Event>,
        watch_exclude_dirs: &[String],
    ) -> color_eyre::Result<Self> {
        let owned_paths: Vec<PathBuf> = repo_paths.to_vec();

        // Bridge channel: notify callback (OS thread) -> tokio task
        let (bridge_tx, mut bridge_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<PathBuf>>();

        // Spawn tokio task to route changed paths to repo paths.
        // Filters out .git/ internals to prevent feedback loops (git2 reads
        // trigger watcher events which would re-trigger git2 queries).
        let paths_for_routing = owned_paths.clone();
        let excludes: Vec<String> = watch_exclude_dirs.to_vec();
        tokio::spawn(async move {
            while let Some(changed_paths) = bridge_rx.recv().await {
                let mut affected_repos: std::collections::HashSet<PathBuf> =
                    std::collections::HashSet::new();

                for changed_path in &changed_paths {
                    // Skip events from excluded directories (node_modules, target, etc.)
                    // or noisy temporary files.
                    if contains_excluded_component(changed_path, &excludes) {
                        continue;
                    }

                    // Allow key .git/ files that change on commit/pull/checkout,
                    // but skip noisy internals that cause feedback loops with git2.
                    if is_git_path(changed_path) && !is_meaningful_git_event(changed_path) {
                        continue;
                    }

                    for repo_path in &paths_for_routing {
                        if changed_path.starts_with(repo_path) {
                            affected_repos.insert(repo_path.clone());
                            break;
                        }
                    }
                }

                for path in affected_repos {
                    let _ = event_tx.send(Event::RepoChanged(path));
                }
            }
        });

        let config = Config::default().with_poll_interval(Duration::from_secs(2));

        let debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
            Duration::from_millis(debounce_ms),
            None,
            move |result: DebounceEventResult| {
                if let Ok(events) = result {
                    let paths: Vec<PathBuf> =
                        events.into_iter().flat_map(|e| e.event.paths).collect();
                    if !paths.is_empty() {
                        let _ = bridge_tx.send(paths);
                    }
                }
            },
            NoCache,
            config,
        )?;

        // Watch each repo subtree, pruning excluded dirs at the OS-watch level.
        // RecursiveMode::Recursive installs an inotify watch on EVERY subdir --
        // including node_modules/target/.git/objects -- which exhausts
        // fs.inotify.max_user_watches. The event-level filters below can't stop
        // that; only declining to watch the dir can. So walk manually and watch
        // each surviving dir NonRecursive.
        // Register watches on a background thread: walking every repo subtree
        // and installing tens of thousands of inotify watches can take tens of
        // seconds, and doing it inline blocks the first frame. The local status
        // poll covers changes until registration catches up.
        let debouncer = Arc::new(Mutex::new(debouncer));
        let watch_excludes: Vec<String> = watch_exclude_dirs.to_vec();
        let walker = Arc::clone(&debouncer);
        std::thread::spawn(move || {
            let mut watched = 0usize;
            for root in &owned_paths {
                if !root.exists() {
                    continue;
                }
                for entry in WalkDir::new(root)
                    .into_iter()
                    .filter_entry(|e| keep_dir(e, &watch_excludes))
                    .flatten()
                {
                    if !entry.file_type().is_dir() {
                        continue;
                    }
                    let result = walker
                        .lock()
                        .expect("watcher mutex poisoned")
                        .watch(entry.path(), RecursiveMode::NonRecursive);
                    if let Err(e) = result {
                        tracing::warn!("Failed to watch {}: {}", entry.path().display(), e);
                    } else {
                        watched += 1;
                    }
                }
            }
            tracing::info!(
                "Watching {} dirs across {} repos",
                watched,
                owned_paths.len()
            );
        });

        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

/// Matches a glob pattern against a target string.
/// Supports `*` (zero or more chars) and `?` (single char).
/// Ignores leading/trailing slashes and compares case-insensitively.
pub(crate) fn is_pattern_match(pattern: &str, target: &str) -> bool {
    let p = pattern.trim().trim_matches(['/', '\\']);
    if p.is_empty() {
        return false;
    }
    let t = target.trim().trim_matches(['/', '\\']);
    if !p.contains('*') && !p.contains('?') {
        return p.eq_ignore_ascii_case(t);
    }
    wildcard_match(p.as_bytes(), t.as_bytes())
}

fn wildcard_match(pattern: &[u8], text: &[u8]) -> bool {
    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_p_idx = None;
    let mut star_t_idx = None;

    while t_idx < text.len() {
        if p_idx < pattern.len()
            && (pattern[p_idx] == b'?' || pattern[p_idx].eq_ignore_ascii_case(&text[t_idx]))
        {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < pattern.len() && pattern[p_idx] == b'*' {
            star_p_idx = Some(p_idx);
            p_idx += 1;
            star_t_idx = Some(t_idx);
        } else if let Some(sp) = star_p_idx {
            p_idx = sp + 1;
            star_t_idx = Some(star_t_idx.unwrap() + 1);
            t_idx = star_t_idx.unwrap();
        } else {
            return false;
        }
    }

    while p_idx < pattern.len() && pattern[p_idx] == b'*' {
        p_idx += 1;
    }

    p_idx == pattern.len()
}

/// Checks if a file is a noisy temporary file or lock file that should never trigger rescans.
fn is_noisy_temp_file(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(".tmp")
        || name.ends_with(".temp")
        || name.ends_with(".swp")
        || name.ends_with(".swo")
        || name.ends_with('~')
        || name.ends_with(".lock")
        || name.ends_with(".bak")
        || name == "4913"
        || name == ".DS_Store"
        || name == "Thumbs.db"
}

pub(crate) fn contains_excluded_component(
    path: &std::path::Path,
    exclude_patterns: &[String],
) -> bool {
    if is_noisy_temp_file(path) {
        return true;
    }

    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy())
        .unwrap_or_default();

    for component in path.components() {
        let comp_str = component.as_os_str().to_string_lossy();
        for pattern in exclude_patterns {
            if is_pattern_match(pattern, &comp_str) {
                return true;
            }
        }
    }

    if !file_name.is_empty() {
        for pattern in exclude_patterns {
            if is_pattern_match(pattern, &file_name) {
                return true;
            }
        }
    }

    false
}

/// Whether to descend into / watch a directory entry. Prunes the excluded
/// build & dependency dirs and the noisy .git internals at the watch-setup
/// level, so they never consume an inotify watch in the first place.
fn keep_dir(entry: &walkdir::DirEntry, exclude_patterns: &[String]) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    for pattern in exclude_patterns {
        if is_pattern_match(pattern, &name) {
            return false;
        }
    }

    // Inside .git, only refs produces meaningful branch/tag update events;
    // objects, hooks, logs, lfs, modules etc. are noisy and large.
    // The .git top level itself is kept (HEAD, MERGE_HEAD, packed-refs live there).
    if entry.path().components().any(|c| c.as_os_str() == ".git") {
        let path_str = entry.path().to_string_lossy();
        return name == ".git" || path_str.contains(".git/refs");
    }
    true
}

fn is_git_path(path: &std::path::Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

/// Meaningful git metadata changes that should trigger a repo status refresh.
/// NOTE: `.git/index` is explicitly excluded because git status queries (including git2)
/// refresh index stat entries and touch `.git/index`, which would cause an infinite feedback loop.
fn is_meaningful_git_event(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    matches!(
        name.as_ref(),
        "HEAD" | "MERGE_HEAD" | "REBASE_HEAD" | "COMMIT_EDITMSG" | "packed-refs"
    ) || path.to_string_lossy().contains(".git/refs/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_pattern_match() {
        assert!(is_pattern_match("node_modules", "node_modules"));
        assert!(is_pattern_match("node_modules/", "node_modules"));
        assert!(is_pattern_match("node_modules", "NODE_MODULES"));
        assert!(is_pattern_match("*.tmp", "foo.tmp"));
        assert!(is_pattern_match("*.tmp", "bar.TMP"));
        assert!(is_pattern_match("build-*", "build-debug"));
        assert!(is_pattern_match("venv*", "venv311"));
        assert!(!is_pattern_match("node_modules", "src"));
        assert!(!is_pattern_match("*.log", "foo.rs"));
    }

    #[test]
    fn test_contains_excluded_component() {
        let excludes = vec!["node_modules".to_string(), "target".to_string(), "*.log".to_string()];
        assert!(contains_excluded_component(
            std::path::Path::new("my-project/node_modules/package.json"),
            &excludes
        ));
        assert!(contains_excluded_component(
            std::path::Path::new("my-project/target/debug/app"),
            &excludes
        ));
        assert!(contains_excluded_component(
            std::path::Path::new("my-project/app.log"),
            &excludes
        ));
        assert!(contains_excluded_component(
            std::path::Path::new("my-project/src/file.swp"),
            &excludes
        ));
        assert!(!contains_excluded_component(
            std::path::Path::new("my-project/src/main.rs"),
            &excludes
        ));
    }

    #[test]
    fn test_git_index_does_not_trigger_event() {
        // Essential to prevent feedback loops with git status
        assert!(!is_meaningful_git_event(std::path::Path::new("/repo/.git/index")));
        assert!(!is_meaningful_git_event(std::path::Path::new("/repo/.git/index.lock")));
        assert!(is_meaningful_git_event(std::path::Path::new("/repo/.git/HEAD")));
        assert!(is_meaningful_git_event(std::path::Path::new("/repo/.git/refs/heads/main")));
        assert!(is_meaningful_git_event(std::path::Path::new("/repo/.git/refs/heads/feature/one")));
    }

    #[test]
    fn keep_dir_prunes_excluded_and_git_internals() {
        let tmp = std::env::temp_dir().join("gitoto_watch_keep_dir_test");
        let _ = std::fs::remove_dir_all(&tmp);
        for sub in [
            "node_modules/deep/nested",
            "src/app",
            ".git/objects/ab",
            ".git/hooks",
            ".git/refs/heads/feature",
        ] {
            std::fs::create_dir_all(tmp.join(sub)).unwrap();
        }
        let excludes = vec!["node_modules".to_string()];

        let watched: Vec<String> = WalkDir::new(&tmp)
            .into_iter()
            .filter_entry(|e| keep_dir(e, &excludes))
            .flatten()
            .filter(|e| e.file_type().is_dir())
            .map(|e| {
                e.path()
                    .strip_prefix(&tmp)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();

        // Excluded dep dir and its subtree are never watched.
        assert!(!watched.iter().any(|p| p.contains("node_modules")));
        // .git noise is pruned, but refs/heads and subdirectories are kept.
        assert!(!watched.iter().any(|p| p.contains("objects")));
        assert!(!watched.iter().any(|p| p.ends_with("hooks")));
        assert!(watched.iter().any(|p| p.contains("refs")));
        assert!(watched.iter().any(|p| p.contains("feature")));
        // Real working-tree dirs are still watched.
        assert!(watched.iter().any(|p| p == "src/app"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
