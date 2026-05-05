use git2::{BranchType, Oid, Repository, Sort};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

use crate::config::BranchFilter;

const MAX_COMMITS: usize = 200;
const PALETTE_SIZE: usize = 6;

#[derive(Clone, Debug)]
pub(crate) struct BranchLabel {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub is_worktree: bool,
    pub is_tag: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct GraphOptions {
    pub branch_filter: BranchFilter,
    pub label_max_len: usize,
    pub first_parent: bool,
    pub show_stats: bool,
}

impl Default for GraphOptions {
    fn default() -> Self {
        Self {
            branch_filter: BranchFilter::All,
            label_max_len: 24,
            first_parent: false,
            show_stats: true,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DiffStat {
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct GraphRow {
    pub lanes: Vec<LaneSegment>,
    pub oid: Oid,
    pub short_id: String,
    pub message: String,
    pub author: String,
    pub time: i64,
    pub labels: Vec<BranchLabel>,
    pub is_pushed: bool,
    pub is_merge: bool,
    pub horizontal_spans: Vec<(usize, usize, usize)>,
    pub parent_oids: Vec<Oid>,
    pub diff_stat: Option<DiffStat>,
    /// If set, this row is a collapsed-branch placeholder: (branch_name, hidden_count).
    pub collapsed: Option<(String, usize)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LaneSegment {
    Empty,
    Straight,
    Commit,
    MergeLeft,
    MergeRight,
    ForkLeft,
    ForkRight,
    Horizontal,
    CrossHorizontal,
    RightTee,
    LeftTee,
}

#[derive(Clone, Debug)]
pub(crate) struct GraphBuilder {
    active_lanes: Vec<Option<Oid>>,
}

impl GraphBuilder {
    pub fn new() -> Self {
        Self {
            active_lanes: Vec::new(),
        }
    }

    pub fn build(
        mut self,
        path: &Path,
        options: &GraphOptions,
    ) -> color_eyre::Result<Vec<GraphRow>> {
        let started = Instant::now();
        let repo = Repository::open(path)?;
        let mut ref_map = resolve_refs(&repo, &options.branch_filter);
        let pushed_oids = collect_pushed_oids(&repo);

        let mut revwalk = repo.revwalk()?;
        revwalk.push_head().ok(); // ok: handles unborn HEAD
        for &oid in ref_map.keys() {
            revwalk.push(oid).ok(); // git2 deduplicates
        }
        revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        if options.first_parent {
            revwalk.simplify_first_parent()?;
        }

        let mut rows = Vec::new();

        for oid_result in revwalk.take(MAX_COMMITS) {
            let oid = oid_result?;
            let commit = repo.find_commit(oid)?;

            let parent_oids: Vec<Oid> = commit.parent_ids().collect();
            let is_merge = commit.parent_count() > 1;
            let labels = ref_map.remove(&oid).unwrap_or_default();
            let (_, lanes, horizontal_spans) = self.process_commit(oid, &parent_oids);

            let short_id = oid.to_string()[..7].to_string();
            let message = commit.summary().unwrap_or("").to_string();
            let author = commit.author().name().unwrap_or("").to_string();
            let time = commit.time().seconds();

            rows.push(GraphRow {
                lanes,
                oid,
                short_id,
                message,
                author,
                time,
                labels,
                is_pushed: pushed_oids.contains(&oid),
                is_merge,
                horizontal_spans,
                parent_oids,
                diff_stat: None,
                collapsed: None,
            });
        }

        tracing::debug!(
            target: "gitoto::perf",
            path = %path.display(),
            rows = rows.len(),
            first_parent = options.first_parent,
            elapsed_ms = started.elapsed().as_millis(),
            "graph build completed"
        );
        Ok(rows)
    }

    fn process_commit(
        &mut self,
        oid: Oid,
        parent_oids: &[Oid],
    ) -> (usize, Vec<LaneSegment>, Vec<(usize, usize, usize)>) {
        // Find which lane this commit occupies
        let commit_col = self
            .active_lanes
            .iter()
            .position(|lane| *lane == Some(oid))
            .unwrap_or_else(|| {
                // Allocate a new lane
                let col = self.find_free_lane();
                if col < self.active_lanes.len() {
                    self.active_lanes[col] = Some(oid);
                } else {
                    self.active_lanes.push(Some(oid));
                }
                col
            });

        // Build lane segments for this row
        let lane_count = self.active_lanes.len().max(commit_col + 1);
        let mut lanes = vec![LaneSegment::Empty; lane_count];

        // Mark continuing lanes
        for (i, lane) in self.active_lanes.iter().enumerate() {
            if i < lanes.len() && lane.is_some() && i != commit_col {
                lanes[i] = LaneSegment::Straight;
            }
        }

        // Mark commit position
        lanes[commit_col] = LaneSegment::Commit;

        // Process parents
        // Clear this commit's lane first
        self.active_lanes[commit_col] = None;
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();

        if !parent_oids.is_empty() {
            // First parent continues in same lane
            let first_parent = parent_oids[0];

            // Check if first parent is already in another lane
            let existing_lane = self
                .active_lanes
                .iter()
                .position(|lane| *lane == Some(first_parent));

            if let Some(existing) = existing_lane {
                // First parent already has a lane — merge to it
                if existing < commit_col {
                    lanes[commit_col] = LaneSegment::MergeLeft;
                    spans.push((existing, commit_col, lane_color(commit_col)));
                } else if existing > commit_col {
                    lanes[commit_col] = LaneSegment::MergeRight;
                    spans.push((commit_col, existing, lane_color(commit_col)));
                }
                // Don't re-assign; lane stays as is
            } else {
                // First parent takes over this lane
                self.active_lanes[commit_col] = Some(first_parent);
            }

            // Additional parents fork into new lanes
            for &parent_oid in &parent_oids[1..] {
                let existing = self
                    .active_lanes
                    .iter()
                    .position(|lane| *lane == Some(parent_oid));

                if existing.is_none() {
                    let new_col = self.find_free_lane();
                    if new_col < self.active_lanes.len() {
                        self.active_lanes[new_col] = Some(parent_oid);
                    } else {
                        self.active_lanes.push(Some(parent_oid));
                    }
                    // Extend lanes if needed
                    while lanes.len() <= new_col {
                        lanes.push(LaneSegment::Empty);
                    }
                    if new_col > commit_col {
                        lanes[new_col] = LaneSegment::ForkRight;
                        spans.push((commit_col, new_col, lane_color(new_col)));
                    } else {
                        lanes[new_col] = LaneSegment::ForkLeft;
                        spans.push((new_col, commit_col, lane_color(new_col)));
                    }
                }
            }
        }

        // Horizontal fill: connect merge/fork endpoints with ─ and ┼
        for &(left, right, _) in &spans {
            if lanes[left] == LaneSegment::Straight {
                lanes[left] = LaneSegment::RightTee;
            }
            if right < lanes.len() && lanes[right] == LaneSegment::Straight {
                lanes[right] = LaneSegment::LeftTee;
            }
            for col in (left + 1)..right {
                if col < lanes.len() {
                    if lanes[col] == LaneSegment::Straight {
                        lanes[col] = LaneSegment::CrossHorizontal;
                    } else if lanes[col] == LaneSegment::Empty {
                        lanes[col] = LaneSegment::Horizontal;
                    }
                }
            }
        }

        // Compact: remove trailing empty lanes
        while self.active_lanes.last() == Some(&None) {
            self.active_lanes.pop();
        }

        (commit_col, lanes, spans)
    }

    fn find_free_lane(&self) -> usize {
        self.active_lanes
            .iter()
            .position(|lane| lane.is_none())
            .unwrap_or(self.active_lanes.len())
    }
}

fn collect_pushed_oids(repo: &Repository) -> HashSet<Oid> {
    let mut pushed = HashSet::new();
    let Ok(branches) = repo.branches(Some(BranchType::Remote)) else {
        return pushed;
    };

    let mut revwalk = match repo.revwalk() {
        Ok(r) => r,
        Err(_) => return pushed,
    };

    for branch_result in branches {
        let Ok((branch, _)) = branch_result else {
            continue;
        };
        let Some(target) = branch.get().target() else {
            continue;
        };
        revwalk.push(target).ok();
    }

    if revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME).is_err() {
        return pushed;
    }

    for oid in revwalk.take(MAX_COMMITS).flatten() {
        pushed.insert(oid);
    }

    pushed
}

pub(crate) fn pushed_oids(path: &Path) -> color_eyre::Result<Vec<Oid>> {
    let started = Instant::now();
    let repo = Repository::open(path)?;
    let oids: Vec<_> = collect_pushed_oids(&repo).into_iter().collect();
    tracing::debug!(
        target: "gitoto::perf",
        path = %path.display(),
        oids = oids.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "pushed status refresh completed"
    );
    Ok(oids)
}

#[derive(Clone, Debug)]
pub(crate) struct BranchSegment {
    pub id: String,
    pub display_name: String,
    pub row_indices: Vec<usize>,
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            // Union by smaller index so the tip (earliest row) becomes root
            if ra < rb {
                self.parent[rb] = ra;
            } else {
                self.parent[ra] = rb;
            }
        }
    }
}

pub(crate) fn compute_branch_segments(rows: &[GraphRow]) -> Vec<BranchSegment> {
    if rows.is_empty() {
        return Vec::new();
    }

    let oid_to_idx: HashMap<Oid, usize> =
        rows.iter().enumerate().map(|(i, r)| (r.oid, i)).collect();

    // Mark main trunk: walk first-parent chain from row 0
    let mut main_trunk: HashSet<usize> = HashSet::new();
    let mut cur = 0usize;
    loop {
        main_trunk.insert(cur);
        let first_parent = rows[cur].parent_oids.first();
        match first_parent.and_then(|oid| oid_to_idx.get(oid)) {
            Some(&next_idx) => cur = next_idx,
            None => break,
        }
    }

    // Union-Find over non-trunk rows via first-parent links
    let mut uf = UnionFind::new(rows.len());
    for (i, row) in rows.iter().enumerate() {
        if main_trunk.contains(&i) {
            continue;
        }
        if let Some(&parent_idx) = row.parent_oids.first().and_then(|oid| oid_to_idx.get(oid))
            && !main_trunk.contains(&parent_idx)
        {
            uf.union(i, parent_idx);
        }
    }

    // Group by union-find root
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..rows.len() {
        if main_trunk.contains(&i) {
            continue;
        }
        let root = uf.find(i);
        groups.entry(root).or_default().push(i);
    }

    let mut segments: Vec<BranchSegment> = Vec::new();
    for (_, mut indices) in groups {
        indices.sort();
        let tip_idx = indices[0];
        let tip = &rows[tip_idx];
        let display_name = tip
            .labels
            .iter()
            .find(|l| !l.is_tag)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| tip.short_id.clone());
        segments.push(BranchSegment {
            id: tip.oid.to_string(),
            display_name,
            row_indices: indices,
        });
    }

    // Sort segments by their first row index for deterministic ordering
    segments.sort_by_key(|s| s.row_indices[0]);
    segments
}

fn resolve_refs(repo: &Repository, filter: &BranchFilter) -> HashMap<Oid, Vec<BranchLabel>> {
    if *filter == BranchFilter::None {
        return HashMap::new();
    }

    let head_oid = repo.head().ok().and_then(|r| r.target());
    let head_name = repo
        .head()
        .ok()
        .and_then(|r| r.shorthand().map(String::from));
    let wt_branches = collect_worktree_branches(repo);

    let mut map: HashMap<Oid, Vec<BranchLabel>> = HashMap::new();

    let branch_types: Vec<BranchType> = match filter {
        BranchFilter::All => vec![BranchType::Local, BranchType::Remote],
        BranchFilter::Local => vec![BranchType::Local],
        BranchFilter::Remote => vec![BranchType::Remote],
        BranchFilter::None => Vec::new(),
    };

    for bt in branch_types {
        let branches = match repo.branches(Some(bt)) {
            Ok(b) => b,
            Err(_) => continue,
        };
        for branch_result in branches {
            let (branch, _) = match branch_result {
                Ok(b) => b,
                Err(_) => continue,
            };
            let target = match branch.get().target() {
                Some(oid) => oid,
                None => continue,
            };
            let name = match branch.name() {
                Ok(Some(n)) => n.to_string(),
                _ => continue,
            };
            let is_remote = bt == BranchType::Remote;
            let is_head =
                !is_remote && head_oid == Some(target) && head_name.as_deref() == Some(&name);
            let is_worktree = !is_remote && wt_branches.contains(&name);

            map.entry(target).or_default().push(BranchLabel {
                name,
                is_head,
                is_remote,
                is_worktree,
                is_tag: false,
            });
        }
    }

    // Tags
    if let Ok(tag_names) = repo.tag_names(None) {
        for name in tag_names.iter().flatten() {
            let refname = format!("refs/tags/{}", name);
            let Ok(reference) = repo.find_reference(&refname) else {
                continue;
            };
            if let Some(oid) = crate::git::reference_oid(&reference) {
                map.entry(oid).or_default().push(BranchLabel {
                    name: name.to_string(),
                    is_head: false,
                    is_remote: false,
                    is_worktree: false,
                    is_tag: true,
                });
            }
        }
    }

    // Sort: HEAD first, then local, then remote, then tags, then alphabetical
    for labels in map.values_mut() {
        labels.sort_by(|a, b| {
            b.is_head
                .cmp(&a.is_head)
                .then(a.is_tag.cmp(&b.is_tag))
                .then(a.is_remote.cmp(&b.is_remote))
                .then(a.name.cmp(&b.name))
        });
    }

    map
}

fn collect_worktree_branches(repo: &Repository) -> HashSet<String> {
    crate::git::linked_worktrees(repo)
        .into_iter()
        .filter_map(|(_, path)| {
            let wt_repo = Repository::open(path).ok()?;
            Some(crate::git::current_branch_name(&wt_repo, "HEAD"))
        })
        .collect()
}

/// Assign a color index (0..PALETTE_SIZE) for a given lane column.
/// Adjacent lanes get different colors.
pub(crate) fn lane_color(col: usize) -> usize {
    col % PALETTE_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::test_support;
    use crate::git::test_support::oid;

    fn create_commit(repo: &git2::Repository, message: &str, parents: &[&git2::Commit]) -> Oid {
        test_support::commit(repo, message, parents)
    }

    fn merge_history_rows(options: &GraphOptions) -> Vec<GraphRow> {
        let (tmp, repo) = test_support::init_repo();
        let oid1 = create_commit(&repo, "root", &[]);
        let c1 = repo.find_commit(oid1).unwrap();
        let oid2 = test_support::commit_detached(&repo, "branch-a", &[&c1]);
        let c2 = repo.find_commit(oid2).unwrap();
        let oid3 = test_support::commit_detached(&repo, "branch-b", &[&c1]);
        let c3 = repo.find_commit(oid3).unwrap();
        let merge_oid = test_support::commit_detached(&repo, "merge", &[&c2, &c3]);
        repo.set_head_detached(merge_oid).unwrap();
        GraphBuilder::new().build(tmp.path(), options).unwrap()
    }

    fn assert_lane_result(
        result: (usize, Vec<LaneSegment>, Vec<(usize, usize, usize)>),
        expected_col: usize,
        expected_lanes: &[LaneSegment],
        expected_spans: &[(usize, usize, usize)],
    ) {
        let (col, lanes, spans) = result;
        assert_eq!(col, expected_col);
        assert_eq!(lanes, expected_lanes);
        assert_eq!(spans, expected_spans);
    }

    #[test]
    fn test_linear_history_single_lane() {
        let (tmp, repo) = test_support::init_repo();

        let oid1 = create_commit(&repo, "first", &[]);
        let c1 = repo.find_commit(oid1).unwrap();
        let _oid2 = create_commit(&repo, "second", &[&c1]);

        let builder = GraphBuilder::new();
        let rows = builder.build(tmp.path(), &GraphOptions::default()).unwrap();

        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| row.lanes[0] == LaneSegment::Commit));
    }

    #[test]
    fn test_merge_creates_two_lanes() {
        let rows = merge_history_rows(&GraphOptions::default());

        assert!(rows.len() >= 3);
        let merge_row = &rows[0];
        assert!(!merge_row.lanes.is_empty());
    }

    #[test]
    fn test_root_commit_closes_lane() {
        let (tmp, repo) = test_support::init_repo();

        let _oid1 = create_commit(&repo, "only", &[]);

        let builder = GraphBuilder::new();
        let rows = builder.build(tmp.path(), &GraphOptions::default()).unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].lanes[0], LaneSegment::Commit);
    }

    #[test]
    fn test_multiple_branches_assign_different_columns() {
        let rows = merge_history_rows(&GraphOptions::default());

        // After merge, we should see a fork to a second column
        let merge_row = &rows[0];
        assert!(
            merge_row.lanes.len() >= 2,
            "Expected >= 2 lanes at merge, got {}",
            merge_row.lanes.len()
        );
    }

    #[test]
    fn test_graph_rows_carry_labels() {
        let (tmp, repo) = test_support::init_repo();

        let oid1 = create_commit(&repo, "first", &[]);
        let c1 = repo.find_commit(oid1).unwrap();
        let _oid2 = create_commit(&repo, "second", &[&c1]);

        // HEAD is on the default branch — tip commit should have a label
        let builder = GraphBuilder::new();
        let rows = builder.build(tmp.path(), &GraphOptions::default()).unwrap();

        let tip = &rows[0];
        assert!(
            !tip.labels.is_empty(),
            "tip commit should have at least one branch label"
        );
    }

    #[test]
    fn test_head_marked() {
        let (tmp, repo) = test_support::init_repo();

        let _oid1 = create_commit(&repo, "init", &[]);

        let builder = GraphBuilder::new();
        let rows = builder.build(tmp.path(), &GraphOptions::default()).unwrap();

        let head_labels: Vec<_> = rows[0].labels.iter().filter(|l| l.is_head).collect();
        assert_eq!(head_labels.len(), 1, "exactly one label should be HEAD");
    }

    #[test]
    fn test_merge_is_merge_true() {
        let rows = merge_history_rows(&GraphOptions::default());

        assert!(rows[0].is_merge, "first row should be a merge commit");
        assert!(
            !rows[1].is_merge,
            "non-merge commit should have is_merge=false"
        );
    }

    #[test]
    fn test_merge_left_horizontal_fill() {
        let mut builder = GraphBuilder::new();
        let oid_target = oid('a');
        let oid_b = oid('b');
        let oid_c = oid('c');
        let oid_commit = oid('d');

        builder.active_lanes = vec![Some(oid_target), Some(oid_b), Some(oid_c), Some(oid_commit)];

        assert_lane_result(
            builder.process_commit(oid_commit, &[oid_target]),
            3,
            &[
                LaneSegment::RightTee,
                LaneSegment::CrossHorizontal,
                LaneSegment::CrossHorizontal,
                LaneSegment::MergeLeft,
            ],
            &[(0, 3, lane_color(3))],
        );
    }

    #[test]
    fn test_fork_right_horizontal_fill() {
        let mut builder = GraphBuilder::new();
        let oid_commit = oid('a');
        let oid_active = oid('b');
        let oid_parent1 = oid('c');
        let oid_parent2 = oid('d');

        builder.active_lanes = vec![Some(oid_commit), Some(oid_active), None];

        assert_lane_result(
            builder.process_commit(oid_commit, &[oid_parent1, oid_parent2]),
            0,
            &[
                LaneSegment::Commit,
                LaneSegment::CrossHorizontal,
                LaneSegment::ForkRight,
            ],
            &[(0, 2, lane_color(2))],
        );
    }

    #[test]
    fn test_adjacent_merge_no_intermediate() {
        let mut builder = GraphBuilder::new();
        let oid_target = oid('a');
        let oid_commit = oid('b');

        builder.active_lanes = vec![Some(oid_target), Some(oid_commit)];

        assert_lane_result(
            builder.process_commit(oid_commit, &[oid_target]),
            1,
            &[LaneSegment::RightTee, LaneSegment::MergeLeft],
            &[(0, 1, lane_color(1))],
        );
    }

    #[test]
    fn test_merge_right_horizontal_fill() {
        let mut builder = GraphBuilder::new();
        let oid_commit = oid('a');
        let oid_b = oid('b');
        let oid_target = oid('c');

        // Commit at col 0, first parent already in col 2 → MergeRight
        builder.active_lanes = vec![Some(oid_commit), Some(oid_b), Some(oid_target)];

        assert_lane_result(
            builder.process_commit(oid_commit, &[oid_target]),
            0,
            &[
                LaneSegment::MergeRight,
                LaneSegment::CrossHorizontal,
                LaneSegment::LeftTee,
            ],
            &[(0, 2, lane_color(0))],
        );
    }

    #[test]
    fn test_first_parent_simplifies_graph() {
        let all_opts = GraphOptions::default();
        let rows_all = merge_history_rows(&all_opts);

        let fp_opts = GraphOptions {
            first_parent: true,
            ..Default::default()
        };
        let rows_fp = merge_history_rows(&fp_opts);

        // First-parent should have fewer rows (skips branch-b)
        assert!(
            rows_fp.len() < rows_all.len(),
            "first-parent ({}) should have fewer rows than all ({})",
            rows_fp.len(),
            rows_all.len()
        );
    }

    #[test]
    fn test_tag_labels_appear_on_tagged_commit() {
        let (tmp, repo) = test_support::init_repo();

        let oid = create_commit(&repo, "tagged commit", &[]);

        // Create a lightweight tag
        let obj = repo.find_object(oid, None).unwrap();
        repo.tag_lightweight("v1.0.0", &obj, false).unwrap();

        let builder = GraphBuilder::new();
        let rows = builder.build(tmp.path(), &GraphOptions::default()).unwrap();

        let tagged_row = rows.iter().find(|r| r.oid == oid).unwrap();
        let tag_labels: Vec<_> = tagged_row.labels.iter().filter(|l| l.is_tag).collect();
        assert_eq!(tag_labels.len(), 1);
        assert_eq!(tag_labels[0].name, "v1.0.0");
    }

    #[test]
    fn test_tags_sort_after_branches() {
        let (tmp, repo) = test_support::init_repo();

        let oid = create_commit(&repo, "init", &[]);

        let obj = repo.find_object(oid, None).unwrap();
        repo.tag_lightweight("v0.1", &obj, false).unwrap();

        let builder = GraphBuilder::new();
        let rows = builder.build(tmp.path(), &GraphOptions::default()).unwrap();

        let row = &rows[0];
        // HEAD branch label should come before tag
        assert!(row.labels.len() >= 2);
        let branch_idx = row.labels.iter().position(|l| !l.is_tag).unwrap();
        let tag_idx = row.labels.iter().position(|l| l.is_tag).unwrap();
        assert!(
            branch_idx < tag_idx,
            "branch ({branch_idx}) should sort before tag ({tag_idx})"
        );
    }

    #[test]
    fn test_filter_none_yields_no_labels() {
        let (tmp, repo) = test_support::init_repo();

        let _oid1 = create_commit(&repo, "init", &[]);

        let options = GraphOptions {
            branch_filter: BranchFilter::None,
            ..Default::default()
        };
        let builder = GraphBuilder::new();
        let rows = builder.build(tmp.path(), &options).unwrap();

        for row in &rows {
            assert!(
                row.labels.is_empty(),
                "filter=None should produce no labels"
            );
        }
    }

    // --- compute_branch_segments tests ---

    fn segment_row(ch: char, short_id: &str, parents: &[char]) -> GraphRow {
        test_support::graph_row(
            &test_support::oid_id(ch),
            short_id,
            parents.iter().map(|&ch| oid(ch)).collect(),
        )
    }

    #[test]
    fn test_segments_linear_history_no_segments() {
        // A -> B -> C (linear, all main trunk)
        let rows = vec![
            segment_row('a', "aaa", &['b']),
            segment_row('b', "bbb", &['c']),
            segment_row('c', "ccc", &[]),
        ];

        let segments = compute_branch_segments(&rows);
        assert!(
            segments.is_empty(),
            "linear history should have no segments"
        );
    }

    #[test]
    fn test_segments_simple_branch_and_merge() {
        let rows = vec![
            segment_row('1', "merge", &['a', 'e']),
            segment_row('a', "aaa", &['f']),
            segment_row('e', "eee", &['f']),
            segment_row('f', "fff", &[]),
        ];

        let segments = compute_branch_segments(&rows);
        assert_eq!(segments.len(), 1, "one side branch expected");
        assert_eq!(segments[0].row_indices, vec![2]);
        assert_eq!(segments[0].id, oid('e').to_string());
    }

    #[test]
    fn test_segments_two_independent_branches() {
        let rows = vec![
            segment_row('1', "merge", &['a', 'd', 'e']),
            segment_row('a', "aaa", &['f']),
            segment_row('d', "ddd", &['f']),
            segment_row('e', "eee", &['f']),
            segment_row('f', "fff", &[]),
        ];

        let segments = compute_branch_segments(&rows);
        assert_eq!(segments.len(), 2, "two side branches expected");
        assert_eq!(segments[0].row_indices, vec![2]);
        assert_eq!(segments[1].row_indices, vec![3]);
    }

    #[test]
    fn test_segments_multi_commit_branch() {
        let rows = vec![
            segment_row('1', "merge", &['a', 'd']),
            segment_row('a', "aaa", &['f']),
            segment_row('d', "tip", &['e']),
            segment_row('e', "mid", &['f']),
            segment_row('f', "fff", &[]),
        ];

        let segments = compute_branch_segments(&rows);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].row_indices, vec![2, 3]);
        assert_eq!(segments[0].id, oid('d').to_string());
    }
}
