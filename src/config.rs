use color_eyre::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::git::status::UntrackedMode;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Config {
    #[serde(default = "default_root_dirs")]
    pub root_dirs: Vec<PathBuf>,
    #[serde(default)]
    pub excluded_repos: Vec<String>,
    #[serde(default)]
    pub pinned_repos: Vec<PathBuf>,
    #[serde(default = "default_scan_depth")]
    pub scan_depth: usize,
    #[serde(default)]
    pub watch: WatchConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub github: GitHubConfig,
    #[serde(default)]
    pub graph: GraphConfig,
    #[serde(default)]
    pub submodules: SubmoduleConfig,
    #[serde(default)]
    pub commit: CommitConfig,
    #[serde(default)]
    pub status: StatusConfig,
    #[serde(default)]
    pub performance: PerformanceConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct WatchConfig {
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
    /// Local status poll interval in seconds (fast, catches missed watcher events)
    #[serde(default = "default_poll_local_secs")]
    pub poll_local_secs: u64,
    /// Remote fetch poll interval in seconds (updates ahead/behind from origin)
    #[serde(default = "default_poll_fetch_secs")]
    pub poll_fetch_secs: u64,
    /// Max concurrent poll tasks (limits CPU usage with many repos)
    #[serde(default = "default_max_concurrent_polls")]
    pub max_concurrent_polls: usize,
    /// Run a full local scan every N local poll ticks; other ticks scan selected/uninitialized repos.
    #[serde(default = "default_poll_local_full_every")]
    pub poll_local_full_every: usize,
    /// Directory names to ignore in watcher events (reduces noise)
    #[serde(default = "default_watch_exclude_dirs")]
    pub watch_exclude_dirs: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UpdatePosition {
    #[default]
    TopRight,
    TopLeft,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RepoNameFormat {
    /// GitHub repos display as local-folder:github-repo; local-only repos display as folder.
    #[default]
    FolderGithub,
    /// Always display only the local folder name.
    Folder,
    /// Display parent-folder:local-folder.
    ParentFolder,
    /// Display the full local path.
    Path,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct UiConfig {
    #[serde(default = "default_frame_rate")]
    pub frame_rate: u16,
    #[serde(default = "default_check_for_updates")]
    pub check_for_updates: bool,
    #[serde(default)]
    pub update_position: UpdatePosition,
    #[serde(default)]
    pub repo_name_format: RepoNameFormat,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct GitHubConfig {
    #[serde(default = "default_github_hosts")]
    pub hosts: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum BranchFilter {
    #[default]
    All,
    Local,
    Remote,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct GraphConfig {
    #[serde(default)]
    pub branches: BranchFilter,
    #[serde(default = "default_label_max_len")]
    pub label_max_len: usize,
    #[serde(default = "default_show_stats")]
    pub show_stats: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct SubmoduleConfig {
    #[serde(default)]
    pub ignore_dirty: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct CommitConfig {
    /// When true, commit actions pass --no-verify to skip repository hooks.
    #[serde(default)]
    pub no_verify: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct StatusConfig {
    /// Controls how much untracked-file discovery to do during status scans.
    #[serde(default)]
    pub untracked: UntrackedMode,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct PerformanceConfig {
    /// Start in fast mode: no automatic fetch polling, no graph diff stats, no untracked scan.
    #[serde(default)]
    pub fast_mode: bool,
}

fn default_show_stats() -> bool {
    true
}

fn default_label_max_len() -> usize {
    24
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            branches: BranchFilter::default(),
            label_max_len: default_label_max_len(),
            show_stats: default_show_stats(),
        }
    }
}

fn default_root_dirs() -> Vec<PathBuf> {
    dirs::home_dir()
        .map(|h| vec![h.join("Code")])
        .unwrap_or_default()
}

fn default_scan_depth() -> usize {
    3
}

fn default_debounce_ms() -> u64 {
    500
}

fn default_poll_local_secs() -> u64 {
    5
}

fn default_poll_fetch_secs() -> u64 {
    30
}

fn default_max_concurrent_polls() -> usize {
    4
}

fn default_poll_local_full_every() -> usize {
    12
}

fn default_watch_exclude_dirs() -> Vec<String> {
    [
        "node_modules",
        "target",
        ".build",
        "dist",
        "vendor",
        ".venv",
        "__pycache__",
        ".next",
        "Pods",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn default_frame_rate() -> u16 {
    10
}

fn default_check_for_updates() -> bool {
    false
}

fn default_github_hosts() -> Vec<String> {
    vec!["github.com".to_string()]
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce_ms: default_debounce_ms(),
            poll_local_secs: default_poll_local_secs(),
            poll_fetch_secs: default_poll_fetch_secs(),
            max_concurrent_polls: default_max_concurrent_polls(),
            poll_local_full_every: default_poll_local_full_every(),
            watch_exclude_dirs: default_watch_exclude_dirs(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            frame_rate: default_frame_rate(),
            check_for_updates: default_check_for_updates(),
            update_position: UpdatePosition::default(),
            repo_name_format: RepoNameFormat::default(),
        }
    }
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            hosts: default_github_hosts(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root_dirs: default_root_dirs(),
            excluded_repos: vec!["node_modules".into(), ".cargo".into()],
            pinned_repos: Vec::new(),
            scan_depth: default_scan_depth(),
            watch: WatchConfig::default(),
            ui: UiConfig::default(),
            github: GitHubConfig::default(),
            graph: GraphConfig::default(),
            submodules: SubmoduleConfig::default(),
            commit: CommitConfig::default(),
            status: StatusConfig::default(),
            performance: PerformanceConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        Self::load_or_create_at(&Self::config_path())
    }

    fn load_or_create_at(config_path: &Path) -> Result<Self> {
        if config_path.exists() {
            let contents = std::fs::read_to_string(config_path)?;
            let mut config: Config = toml::from_str(&contents)?;
            config.expand_tildes();
            Ok(config)
        } else {
            let config = Self::default();
            config.save_to_path(config_path)?;
            Ok(config)
        }
    }

    pub fn config_path() -> PathBuf {
        ProjectDirs::from("", "", "gitoto")
            .map(|dirs| dirs.config_dir().join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("config.toml"))
    }

    pub fn save(&self) -> Result<()> {
        self.save_to_path(&Self::config_path())
    }

    fn save_to_path(&self, config_path: &Path) -> Result<()> {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(config_path, contents)?;
        Ok(())
    }

    pub fn add_pinned_repo(&mut self, path: PathBuf) {
        if !self.pinned_repos.contains(&path) {
            self.pinned_repos.push(path);
        }
    }

    pub fn override_root(&mut self, root: PathBuf) {
        self.root_dirs = vec![root];
    }

    fn expand_tildes(&mut self) {
        if let Some(home) = dirs::home_dir() {
            for dir in &mut self.root_dirs {
                if dir.starts_with("~") {
                    *dir = home.join(dir.strip_prefix("~").unwrap());
                }
            }
            for dir in &mut self.pinned_repos {
                if dir.starts_with("~") {
                    *dir = home.join(dir.strip_prefix("~").unwrap());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_config(toml_str: &str) -> Config {
        toml::from_str(toml_str).unwrap()
    }

    fn serialize_config(config: &Config) -> String {
        toml::to_string_pretty(config).unwrap()
    }

    fn roundtrip(config: &Config) -> Config {
        parse_config(&serialize_config(config))
    }

    #[test]
    fn test_default_config_has_code_root() {
        let config = Config::default();
        assert!(!config.root_dirs.is_empty());
        let first = &config.root_dirs[0];
        assert!(first.ends_with("Code"));
    }

    #[test]
    fn test_default_scan_depth_supports_third_level_git_dirs() {
        let config = Config::default();
        assert_eq!(config.scan_depth, 3);
    }

    #[test]
    fn test_cli_root_overrides_config() {
        let mut config = Config::default();
        config.override_root(PathBuf::from("/tmp/my-repos"));
        assert_eq!(config.root_dirs, vec![PathBuf::from("/tmp/my-repos")]);
    }

    #[test]
    fn test_save_and_reload_roundtrip() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();

        let mut config = Config::default();
        config.pinned_repos.push(PathBuf::from("/tmp/test-repo"));

        // Write directly to temp path
        let contents = serialize_config(&config);
        std::fs::write(&path, &contents).unwrap();

        let loaded = parse_config(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(loaded.pinned_repos, vec![PathBuf::from("/tmp/test-repo")]);
    }

    #[test]
    fn test_load_creates_missing_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("gitoto").join("config.toml");

        assert!(!path.exists());
        let config = Config::load_or_create_at(&path).unwrap();

        assert!(path.exists());
        let expected = serialize_config(&Config::default());
        assert_eq!(serialize_config(&config), expected);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    }

    #[test]
    fn test_add_pinned_repo_deduplication() {
        let mut config = Config::default();
        config.add_pinned_repo(PathBuf::from("/tmp/repo-a"));
        config.add_pinned_repo(PathBuf::from("/tmp/repo-a"));
        config.add_pinned_repo(PathBuf::from("/tmp/repo-b"));
        assert_eq!(config.pinned_repos.len(), 2);
    }

    #[test]
    fn test_config_defaults() {
        let config = parse_config("");
        assert_eq!(config.graph.branches, BranchFilter::All);
        assert_eq!(config.graph.label_max_len, 24);
        assert!(config.graph.show_stats);
        assert!(!config.ui.check_for_updates);
        assert_eq!(config.ui.update_position, UpdatePosition::TopRight);
        assert_eq!(config.github.hosts, vec!["github.com"]);
        assert!(!config.submodules.ignore_dirty);
        assert!(!config.commit.no_verify);
        assert_eq!(config.watch.max_concurrent_polls, 4);
        assert_eq!(config.watch.poll_local_full_every, 12);
    }

    #[test]
    fn test_config_parses_non_default_sections() {
        let toml_str = r#"
            [graph]
            branches = "local"

            [ui]
            check_for_updates = false
            update_position = "top-left"
            repo_name_format = "path"

            [github]
            hosts = ["github.com", "git.example.com"]

            [submodules]
            ignore_dirty = true

            [commit]
            no_verify = true

            [status]
            untracked = "normal"
        "#;
        let config = parse_config(toml_str);
        assert_eq!(config.graph.branches, BranchFilter::Local);
        assert_eq!(config.ui.update_position, UpdatePosition::TopLeft);
        assert_eq!(config.ui.repo_name_format, RepoNameFormat::Path);
        assert_eq!(config.github.hosts, vec!["github.com", "git.example.com"]);
        assert!(config.submodules.ignore_dirty);
        assert!(config.commit.no_verify);
        assert_eq!(config.status.untracked, UntrackedMode::Normal);
    }

    #[test]
    fn test_config_roundtrip_preserves_non_defaults() {
        let mut config = Config::default();
        config.graph.branches = BranchFilter::Remote;
        config.graph.label_max_len = 16;
        config.graph.show_stats = false;
        config.ui.update_position = UpdatePosition::TopLeft;
        config.ui.repo_name_format = RepoNameFormat::ParentFolder;
        config.submodules.ignore_dirty = true;
        config.commit.no_verify = true;
        config.status.untracked = UntrackedMode::None;

        let loaded = roundtrip(&config);
        assert_eq!(loaded.graph.branches, BranchFilter::Remote);
        assert_eq!(loaded.graph.label_max_len, 16);
        assert!(!loaded.graph.show_stats);
        assert_eq!(loaded.ui.update_position, UpdatePosition::TopLeft);
        assert_eq!(loaded.ui.repo_name_format, RepoNameFormat::ParentFolder);
        assert!(loaded.submodules.ignore_dirty);
        assert!(loaded.commit.no_verify);
        assert_eq!(loaded.status.untracked, UntrackedMode::None);
    }

    #[test]
    fn test_watch_exclude_dirs_default() {
        let config = parse_config("");
        for dir in ["node_modules", "target", ".next"] {
            assert!(config.watch.watch_exclude_dirs.contains(&dir.to_string()));
        }
    }
}
