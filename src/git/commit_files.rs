use git2::{Diff, DiffOptions, Oid, Repository};
use std::path::Path;
use std::time::Instant;

/// List files changed in a commit (vs its first parent, or empty tree for root).
/// Returns the full commit message and `(status_label, file_path)` pairs.
pub(crate) fn list_commit_files(
    path: &Path,
    oid_str: &str,
) -> color_eyre::Result<(String, Vec<(String, String)>)> {
    let started = Instant::now();
    let repo = Repository::open(path)?;
    let oid = Oid::from_str(oid_str)?;
    let commit = repo.find_commit(oid)?;

    let message = commit.message().unwrap_or("").trim().to_string();

    let diff = diff_for_commit(&repo, &commit, None)?;

    let mut files = Vec::new();
    for delta in diff.deltas() {
        let status = match delta.status() {
            git2::Delta::Added => "A",
            git2::Delta::Deleted => "D",
            git2::Delta::Modified => "M",
            git2::Delta::Renamed => "R",
            _ => "?",
        };
        let file_path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        files.push((status.to_string(), file_path));
    }

    tracing::debug!(
        target: "gitoto::perf",
        path = %path.display(),
        oid = oid_str,
        files = files.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "commit files loaded"
    );
    Ok((message, files))
}

/// Get the diff text for a single file in a commit.
pub(crate) fn commit_file_diff(
    path: &Path,
    oid_str: &str,
    file_path: &str,
) -> color_eyre::Result<String> {
    let started = Instant::now();
    let repo = Repository::open(path)?;
    let oid = Oid::from_str(oid_str)?;
    let commit = repo.find_commit(oid)?;

    let mut opts = DiffOptions::new();
    opts.pathspec(file_path);

    let diff = diff_for_commit(&repo, &commit, Some(&mut opts))?;

    let mut output = String::new();
    diff_to_string(&diff, &mut output)?;

    if output.is_empty() {
        output = "(no diff available)".to_string();
    }

    tracing::debug!(
        target: "gitoto::perf",
        path = %path.display(),
        oid = oid_str,
        file_path,
        bytes = output.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "commit file diff loaded"
    );
    Ok(output)
}

use crate::git::graph::DiffStat;

/// Compute diff stats (additions/deletions) for a batch of commits.
pub(crate) fn batch_diff_stats(
    path: &Path,
    oids: &[Oid],
) -> color_eyre::Result<Vec<(Oid, DiffStat)>> {
    let started = Instant::now();
    let repo = Repository::open(path)?;
    let mut results = Vec::with_capacity(oids.len());
    for &oid in oids {
        let Ok(commit) = repo.find_commit(oid) else {
            continue;
        };
        let Ok(diff) = diff_for_commit(&repo, &commit, None) else {
            continue;
        };
        let Ok(stats) = diff.stats() else {
            continue;
        };
        results.push((
            oid,
            DiffStat {
                additions: stats.insertions(),
                deletions: stats.deletions(),
            },
        ));
    }
    tracing::debug!(
        target: "gitoto::perf",
        path = %path.display(),
        requested = oids.len(),
        completed = results.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "batch diff stats completed"
    );
    Ok(results)
}

fn diff_for_commit<'repo>(
    repo: &'repo Repository,
    commit: &git2::Commit<'repo>,
    opts: Option<&mut DiffOptions>,
) -> color_eyre::Result<Diff<'repo>> {
    let tree = commit.tree()?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());
    Ok(repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), opts)?)
}

fn diff_to_string(diff: &Diff<'_>, output: &mut String) -> color_eyre::Result<()> {
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let prefix = match line.origin() {
            '+' => "+",
            '-' => "-",
            ' ' => " ",
            _ => "",
        };
        output.push_str(prefix);
        output.push_str(&String::from_utf8_lossy(line.content()));
        true
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support;
    use git2::Repository;
    use tempfile::TempDir;

    fn create_repo_with_file(file_name: &str, content: &str) -> (TempDir, Repository, String) {
        let (tmp, repo) = test_support::init_repo();
        let oid =
            test_support::write_commit(&repo, tmp.path(), file_name, content, "Add file", &[]);

        (tmp, repo, oid.to_string())
    }

    #[test]
    fn test_list_commit_files_on_known_commit() {
        let (tmp, repo, first_oid) = create_repo_with_file("hello.txt", "hello");

        // Second commit with a modification
        let parent = repo
            .find_commit(git2::Oid::from_str(&first_oid).unwrap())
            .unwrap();
        let oid2 = test_support::write_commit(
            &repo,
            tmp.path(),
            "hello.txt",
            "world",
            "Modify file",
            &[&parent],
        );

        let (message, files) = list_commit_files(tmp.path(), &oid2.to_string()).unwrap();
        assert_eq!(message, "Modify file");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "M");
        assert_eq!(files[0].1, "hello.txt");
    }

    #[test]
    fn test_root_commit_lists_files() {
        let (tmp, _repo, oid) = create_repo_with_file("root.txt", "content");

        let (message, files) = list_commit_files(tmp.path(), &oid).unwrap();
        assert_eq!(message, "Add file");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "A");
        assert_eq!(files[0].1, "root.txt");
    }

    #[test]
    fn test_batch_diff_stats_returns_additions() {
        let (tmp, repo, first_oid) = create_repo_with_file("file.txt", "line1\n");

        // Second commit adds lines
        let parent = repo
            .find_commit(Oid::from_str(&first_oid).unwrap())
            .unwrap();
        let oid2 = test_support::write_commit(
            &repo,
            tmp.path(),
            "file.txt",
            "line1\nline2\nline3\n",
            "Add lines",
            &[&parent],
        );

        let stats = batch_diff_stats(tmp.path(), &[oid2]).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].0, oid2);
        assert!(
            stats[0].1.additions > 0,
            "expected additions, got {}",
            stats[0].1.additions
        );
    }

    #[test]
    fn test_batch_diff_stats_root_commit() {
        let (tmp, _repo, oid_str) = create_repo_with_file("root.txt", "content\n");
        let oid = Oid::from_str(&oid_str).unwrap();

        let stats = batch_diff_stats(tmp.path(), &[oid]).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].0, oid);
        assert!(stats[0].1.additions > 0);
        assert_eq!(stats[0].1.deletions, 0);
    }
}
