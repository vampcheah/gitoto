use notify_debouncer_full::{
    DebounceEventResult, Debouncer, NoCache, new_debouncer_opt,
    notify::{Config, RecommendedWatcher, RecursiveMode},
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
use walkdir::WalkDir;

use crate::event::Event;

pub(crate) struct RepoWatcher {
    _debouncer: Debouncer<RecommendedWatcher, NoCache>,
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
        let exclude_set: HashSet<String> = watch_exclude_dirs.iter().cloned().collect();
        tokio::spawn(async move {
            while let Some(changed_paths) = bridge_rx.recv().await {
                let mut affected_repos: HashSet<PathBuf> = HashSet::new();

                for changed_path in &changed_paths {
                    // Skip events from excluded directories (node_modules, target, etc.)
                    if contains_excluded_component(changed_path, &exclude_set) {
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

        let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
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
        // ponytail: no incremental watch for dirs created after startup -- their
        // creation still fires on the watched parent and triggers a rescan, but
        // edits deep inside a brand-new subtree are missed until restart. Add a
        // watch-on-create channel if users report that.
        let watch_excludes: HashSet<String> = watch_exclude_dirs.iter().cloned().collect();
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
                if let Err(e) = debouncer.watch(entry.path(), RecursiveMode::NonRecursive) {
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

        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

fn contains_excluded_component(path: &std::path::Path, exclude_set: &HashSet<String>) -> bool {
    path.components()
        .any(|component| exclude_set.contains(component.as_os_str().to_string_lossy().as_ref()))
}

/// Whether to descend into / watch a directory entry. Prunes the excluded
/// build & dependency dirs and the noisy .git internals at the watch-setup
/// level, so they never consume an inotify watch in the first place.
fn keep_dir(entry: &walkdir::DirEntry, exclude_set: &HashSet<String>) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    if exclude_set.contains(name.as_ref()) {
        return false;
    }
    // Inside .git, only refs/logs produce meaningful events; objects, hooks,
    // lfs, modules etc. are large and pure noise. The .git top level itself is
    // kept (HEAD, index, COMMIT_EDITMSG, packed-refs live there).
    if entry.path().components().any(|c| c.as_os_str() == ".git") {
        return matches!(
            name.as_ref(),
            ".git" | "refs" | "logs" | "heads" | "remotes" | "tags"
        );
    }
    true
}

fn is_git_path(path: &std::path::Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".git")
}

fn is_meaningful_git_event(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    matches!(
        name.as_ref(),
        "HEAD" | "index" | "MERGE_HEAD" | "REBASE_HEAD" | "COMMIT_EDITMSG" | "packed-refs"
    ) || path.to_string_lossy().contains(".git/refs/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_dir_prunes_excluded_and_git_internals() {
        let tmp = std::env::temp_dir().join("gitoto_watch_keep_dir_test");
        let _ = std::fs::remove_dir_all(&tmp);
        for sub in [
            "node_modules/deep/nested",
            "src/app",
            ".git/objects/ab",
            ".git/hooks",
            ".git/refs/heads",
        ] {
            std::fs::create_dir_all(tmp.join(sub)).unwrap();
        }
        let excludes: HashSet<String> = ["node_modules".to_string()].into_iter().collect();

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
        // .git noise is pruned, but refs/heads are kept.
        assert!(!watched.iter().any(|p| p.contains("objects")));
        assert!(!watched.iter().any(|p| p.ends_with("hooks")));
        assert!(watched.iter().any(|p| p.contains("refs")));
        // Real working-tree dirs are still watched.
        assert!(watched.iter().any(|p| p == "src/app"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
