use std::collections::HashSet;

use crate::git::graph::{BranchSegment, GraphRow};

pub(super) fn collapsed_rows(
    all_rows: &[GraphRow],
    segments: &[BranchSegment],
    collapsed_branches: &HashSet<String>,
) -> Vec<GraphRow> {
    let mut hidden: HashSet<usize> = HashSet::new();
    let mut placeholders: Vec<(usize, String, String, usize)> = Vec::new();

    for seg in segments {
        if !collapsed_branches.contains(&seg.id) {
            continue;
        }
        for &row_idx in &seg.row_indices {
            hidden.insert(row_idx);
        }
        let tip_idx = seg.row_indices[0];
        placeholders.push((
            tip_idx,
            seg.id.clone(),
            seg.display_name.clone(),
            seg.row_indices.len(),
        ));
    }

    let mut rows = Vec::new();
    for (i, row) in all_rows.iter().enumerate() {
        if hidden.contains(&i) {
            if let Some((_, seg_id, name, count)) =
                placeholders.iter().find(|(tip, _, _, _)| *tip == i)
            {
                let mut placeholder = row.clone();
                placeholder.message = format!("\u{25b6} {name} ({count} commits)");
                placeholder.short_id = String::new();
                placeholder.author = String::new();
                placeholder.labels = Vec::new();
                placeholder.diff_stat = None;
                placeholder.collapsed = Some((seg_id.clone(), *count));
                rows.push(placeholder);
            }
            continue;
        }
        rows.push(row.clone());
    }

    rows
}
