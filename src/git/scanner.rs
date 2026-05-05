use crate::config::Config;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

pub(crate) fn discover_repos(config: &Config) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut repos = Vec::new();

    for pinned in &config.pinned_repos {
        let canonical = pinned.canonicalize().unwrap_or_else(|_| pinned.clone());
        if is_git_repo(&canonical) && seen.insert(canonical.clone()) {
            repos.push(canonical);
        }
    }

    for root in &config.root_dirs {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(root)
            .max_depth(config.scan_depth)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| should_descend(entry, config))
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == ".git" && entry.file_type().is_dir() {
                let repo_path = canonical_parent(entry.path());
                if !is_excluded_repo(&repo_path, &config.excluded_repos)
                    && seen.insert(repo_path.clone())
                {
                    repos.push(repo_path);
                }
            }
        }
    }

    sort_repos_with_pinned_first(&mut repos, &config.pinned_repos);

    repos
}

fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

fn canonical_parent(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap();
    parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf())
}

fn is_excluded_repo(repo_path: &Path, excluded_repos: &[String]) -> bool {
    let repo_name = repo_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let path_str = repo_path.to_string_lossy();
    excluded_repos
        .iter()
        .any(|pattern| repo_name == *pattern || path_str.contains(pattern))
}

fn sort_repos_with_pinned_first(repos: &mut Vec<PathBuf>, pinned_repos: &[PathBuf]) {
    repos.sort_by_key(|path| path.file_name().unwrap_or_default().to_ascii_lowercase());

    let pinned_set: HashSet<PathBuf> = pinned_repos
        .iter()
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    if pinned_set.is_empty() {
        return;
    }

    let mut pinned = Vec::new();
    let mut rest = Vec::new();
    for repo in std::mem::take(repos) {
        if pinned_set.contains(&repo) {
            pinned.push(repo);
        } else {
            rest.push(repo);
        }
    }
    pinned.extend(rest);
    *repos = pinned;
}

fn should_descend(entry: &DirEntry, config: &Config) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return true;
    }

    let name = entry.file_name();
    if name == OsStr::new(".git") {
        return true;
    }

    let name = name.to_string_lossy();
    !config
        .watch
        .watch_exclude_dirs
        .iter()
        .any(|dir| dir == &name)
        && !config.excluded_repos.iter().any(|pattern| pattern == &name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_repo(parent: &std::path::Path, name: &str) -> PathBuf {
        let repo_dir = parent.join(name);
        fs::create_dir_all(repo_dir.join(".git")).unwrap();
        repo_dir
    }

    #[test]
    fn test_discover_finds_git_repos() {
        let tmp = TempDir::new().unwrap();
        make_repo(tmp.path(), "alpha");
        make_repo(tmp.path(), "beta");

        let config = Config {
            root_dirs: vec![tmp.path().to_path_buf()],
            scan_depth: 2,
            ..Config::default()
        };

        let repos = discover_repos(&config);
        assert_eq!(repos.len(), 2);
    }

    #[test]
    fn test_default_scan_depth_finds_third_level_git_dir() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("one");
        make_repo(&nested, "deep-repo");

        let config = Config {
            root_dirs: vec![tmp.path().to_path_buf()],
            ..Config::default()
        };

        let repos = discover_repos(&config);
        assert_eq!(repos.len(), 1);
        assert!(repos[0].ends_with("deep-repo"));
    }

    #[test]
    fn test_excluded_repos_are_filtered() {
        let tmp = TempDir::new().unwrap();
        make_repo(tmp.path(), "good-repo");
        make_repo(tmp.path(), "node_modules");

        let config = Config {
            root_dirs: vec![tmp.path().to_path_buf()],
            excluded_repos: vec!["node_modules".into()],
            scan_depth: 2,
            ..Config::default()
        };

        let repos = discover_repos(&config);
        assert_eq!(repos.len(), 1);
        assert!(repos[0].ends_with("good-repo"));
    }

    #[test]
    fn test_pinned_repos_appear_first() {
        let tmp = TempDir::new().unwrap();
        let z_repo = make_repo(tmp.path(), "z-repo");
        make_repo(tmp.path(), "a-repo");

        let config = Config {
            root_dirs: vec![tmp.path().to_path_buf()],
            pinned_repos: vec![z_repo.clone()],
            scan_depth: 2,
            ..Config::default()
        };

        let repos = discover_repos(&config);
        assert_eq!(repos.len(), 2);
        assert!(repos[0].ends_with("z-repo"));
    }

    #[test]
    fn test_watch_excluded_dirs_are_not_traversed() {
        let tmp = TempDir::new().unwrap();
        make_repo(tmp.path(), "visible");
        let ignored = tmp.path().join("node_modules");
        fs::create_dir_all(&ignored).unwrap();
        make_repo(&ignored, "hidden");

        let config = Config {
            root_dirs: vec![tmp.path().to_path_buf()],
            scan_depth: 3,
            ..Config::default()
        };

        let repos = discover_repos(&config);
        assert_eq!(repos.len(), 1);
        assert!(repos[0].ends_with("visible"));
    }
}
