use std::fs;
use std::path::Path;
use std::process::Output;

use git2::{Oid, Repository, Signature};
use tempfile::TempDir;

use crate::git::graph::{BranchLabel, GraphRow, LaneSegment};

pub(crate) fn init_repo() -> (TempDir, Repository) {
    let tmp = TempDir::new().unwrap();
    let repo = Repository::init(tmp.path()).unwrap();
    (tmp, repo)
}

pub(crate) fn init_repo_with_commit() -> (TempDir, Repository) {
    let (tmp, repo) = init_repo();
    commit(&repo, "Initial commit", &[]);
    (tmp, repo)
}

pub(crate) fn commit(repo: &Repository, message: &str, parents: &[&git2::Commit]) -> Oid {
    commit_to(repo, Some("HEAD"), message, parents)
}

pub(crate) fn commit_detached(repo: &Repository, message: &str, parents: &[&git2::Commit]) -> Oid {
    commit_to(repo, None, message, parents)
}

pub(crate) fn commit_to(
    repo: &Repository,
    update_ref: Option<&str>,
    message: &str,
    parents: &[&git2::Commit],
) -> Oid {
    let sig = signature();
    let tree_id = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(tree_id).unwrap();
    repo.commit(update_ref, &sig, &sig, message, &tree, parents)
        .unwrap()
}

pub(crate) fn write_and_stage(repo: &Repository, root: &Path, path: &str, content: &str) {
    let file_path = root.join(path);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(file_path, content).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(path)).unwrap();
    index.write().unwrap();
}

pub(crate) fn write_commit(
    repo: &Repository,
    root: &Path,
    path: &str,
    content: &str,
    message: &str,
    parents: &[&git2::Commit],
) -> Oid {
    write_and_stage(repo, root, path, content);
    commit(repo, message, parents)
}

pub(crate) fn signature() -> Signature<'static> {
    Signature::now("Test", "test@test.com").unwrap()
}

pub(crate) fn git(repo_path: &Path, args: &[&str]) -> Output {
    let mut command = std::process::Command::new("git");
    command.arg("-C").arg(repo_path).args(args);
    command.output().unwrap()
}

pub(crate) fn git_ok(repo_path: &Path, args: &[&str], context: &str) -> Output {
    let output = git(repo_path, args);
    assert!(
        output.status.success(),
        "{context} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

pub(crate) fn git_commit(repo_path: &Path, message: &str) -> Output {
    git_ok(
        repo_path,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@test.com",
            "commit",
            "-m",
            message,
        ],
        "git commit",
    )
}

pub(crate) fn graph_row(oid_str: &str, short_id: &str, parent_oids: Vec<Oid>) -> GraphRow {
    GraphRow {
        lanes: vec![LaneSegment::Commit],
        horizontal_spans: Vec::new(),
        oid: Oid::from_str(oid_str).unwrap(),
        short_id: short_id.to_string(),
        message: String::new(),
        author: String::new(),
        time: 0,
        labels: Vec::new(),
        is_pushed: false,
        is_merge: parent_oids.len() > 1,
        parent_oids,
        diff_stat: None,
        collapsed: None,
    }
}

pub(crate) fn oid(ch: char) -> Oid {
    Oid::from_str(&ch.to_string().repeat(40)).unwrap()
}

pub(crate) fn oid_id(ch: char) -> String {
    oid(ch).to_string()
}

pub(crate) fn branch_label(name: &str) -> BranchLabel {
    BranchLabel {
        name: name.to_string(),
        is_head: false,
        is_remote: false,
        is_worktree: false,
        is_tag: false,
    }
}

pub(crate) fn dag_row(
    ch: char,
    short_id: &str,
    parents: &[char],
    labels: Vec<BranchLabel>,
) -> GraphRow {
    let mut row = graph_row(
        &oid_id(ch),
        short_id,
        parents.iter().map(|&ch| oid(ch)).collect(),
    );
    row.message = format!("msg-{short_id}");
    row.author = "Author".to_string();
    row.labels = labels;
    row
}

pub(crate) fn graph_row_text(short_id: &str, message: &str, author: &str) -> GraphRow {
    let mut row = graph_row(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        short_id,
        Vec::new(),
    );
    row.message = message.to_string();
    row.author = author.to_string();
    row
}
