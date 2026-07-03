use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::style::{Color, Modifier};
use ratatui::text::Span;

use crate::components::style::{fg_span, fg_style};
use crate::git::graph::{BranchLabel, GraphRow, LaneSegment, lane_color};

const PALETTE: [Color; 6] = [
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
];

pub(crate) fn render_graph_prefix(row: &GraphRow) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    for (col, segment) in row.lanes.iter().enumerate() {
        // Use span color for horizontal-related segments
        let color = match segment {
            LaneSegment::Commit => {
                if row.is_pushed {
                    Color::Green
                } else {
                    Color::Red
                }
            }
            LaneSegment::Horizontal
            | LaneSegment::CrossHorizontal
            | LaneSegment::RightTee
            | LaneSegment::LeftTee => row
                .horizontal_spans
                .iter()
                .find(|s| s.0 <= col && col <= s.1)
                .map(|s| PALETTE[s.2])
                .unwrap_or(PALETTE[lane_color(col)]),
            _ => PALETTE[lane_color(col)],
        };
        let ch = match segment {
            LaneSegment::Empty => " ",
            LaneSegment::Straight => "│",
            LaneSegment::Commit => "●",
            LaneSegment::MergeLeft => "╯",
            LaneSegment::MergeRight => "╰",
            LaneSegment::ForkLeft => "╮",
            LaneSegment::ForkRight => "╭",
            LaneSegment::Horizontal => "─",
            LaneSegment::CrossHorizontal => "┼",
            LaneSegment::RightTee => "├",
            LaneSegment::LeftTee => "┤",
        };

        spans.push(fg_span(ch.to_string(), color));

        // Inter-column space: ─ if within a horizontal span, " " otherwise
        let h_span = row
            .horizontal_spans
            .iter()
            .find(|s| s.0 <= col && col < s.1);
        if let Some(s) = h_span {
            spans.push(fg_span("─", PALETTE[s.2]));
        } else {
            spans.push(Span::raw(" "));
        }
    }

    spans
}

pub(crate) fn render_branch_labels(labels: &[BranchLabel], max_len: usize) -> Vec<Span<'static>> {
    if labels.is_empty() {
        return Vec::new();
    }

    let paren_style = fg_style(Color::Yellow);
    let mut spans = vec![Span::styled("(", paren_style)];

    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(", ", paren_style));
        }

        let (prefix, color) = if label.is_head {
            ("* ", Color::Green)
        } else if label.is_worktree {
            ("\u{2302} ", Color::Magenta) // ⌂
        } else if label.is_tag {
            ("", Color::LightYellow)
        } else if label.is_remote {
            ("", Color::Red)
        } else {
            ("", Color::Cyan)
        };

        if !prefix.is_empty() {
            spans.push(fg_span(prefix.to_string(), color));
        }

        let name = if label.name.chars().count() > max_len {
            let mut truncated: String = label.name.chars().take(max_len).collect();
            truncated.push('\u{2026}'); // …
            truncated
        } else {
            label.name.clone()
        };

        spans.push(fg_span(name, color));
    }

    spans.push(Span::styled(") ", paren_style));
    spans
}

pub(crate) fn render_commit_row(
    row: &GraphRow,
    label_max_len: usize,
    dimmed: bool,
) -> Vec<Span<'static>> {
    let is_collapsed = row.collapsed.is_some();
    let mut spans = render_graph_prefix(row);

    if dimmed || is_collapsed {
        for span in &mut spans {
            span.style = fg_style(Color::DarkGray);
        }
    }

    if is_collapsed {
        spans.push(Span::styled(
            row.message.clone(),
            fg_style(Color::Rgb(130, 130, 130)).add_modifier(Modifier::ITALIC),
        ));
        return spans;
    }

    let id_style = if dimmed {
        fg_style(Color::DarkGray)
    } else {
        fg_style(Color::Yellow).add_modifier(Modifier::BOLD)
    };
    spans.push(Span::styled(format!("{} ", row.short_id), id_style));

    if !dimmed {
        spans.extend(render_branch_labels(&row.labels, label_max_len));
    }

    let msg_color = if dimmed {
        Color::DarkGray
    } else if row.is_merge {
        Color::Rgb(130, 130, 130)
    } else {
        Color::White
    };
    spans.push(fg_span(row.message.clone(), msg_color));

    let author_color = if dimmed {
        Color::DarkGray
    } else {
        author_color(&row.author)
    };
    spans.push(fg_span(format!("  — {}", row.author), author_color));
    spans.push(fg_span(
        format!(" {}", format_relative_time(row.time)),
        Color::DarkGray,
    ));

    if let Some(ref stat) = row.diff_stat
        && !dimmed
    {
        if stat.additions > 0 {
            spans.push(fg_span(format!(" +{}", stat.additions), Color::Green));
        }
        if stat.deletions > 0 {
            spans.push(fg_span(format!(" -{}", stat.deletions), Color::Red));
        }
    }

    spans
}

/// Truncate a span list so its total display width fits within `max_width`.
/// Appends `..` at the cut point when truncation occurs.
pub(crate) fn truncate_line(spans: &mut Vec<Span<'static>>, max_width: usize) {
    if max_width == 0 {
        spans.clear();
        return;
    }

    let total: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if total <= max_width {
        return;
    }

    let mut used = 0;
    let mut cut_idx = spans.len();
    let mut remaining = 0;

    for (i, span) in spans.iter().enumerate() {
        let w = span.content.chars().count();
        if used + w > max_width {
            cut_idx = i;
            remaining = max_width - used;
            break;
        }
        used += w;
    }

    spans.truncate(cut_idx + 1);

    if let Some(last) = spans.last_mut() {
        if remaining > 2 {
            let content: String = last.content.chars().take(remaining - 2).collect();
            *last = Span::styled(format!("{}..", content), last.style);
        } else if remaining >= 1 {
            let dots: String = ".".repeat(remaining);
            *last = Span::styled(dots, last.style);
        } else {
            // No room in this span — back up one
            spans.pop();
            if let Some(prev) = spans.last_mut() {
                let content = prev.content.to_string();
                let n = content.chars().count();
                if n >= 2 {
                    let truncated: String = content.chars().take(n - 2).collect();
                    *prev = Span::styled(format!("{}..", truncated), prev.style);
                } else {
                    *prev = Span::styled(".".repeat(n), prev.style);
                }
            }
        }
    }
}

/// Apply horizontal scroll: skip `offset` characters from the left, then truncate to `max_width`.
pub(crate) fn h_scroll_line(spans: &mut Vec<Span<'static>>, offset: usize, max_width: usize) {
    if offset == 0 {
        truncate_line(spans, max_width);
        return;
    }

    // Phase 1: skip `offset` characters from the left
    let mut to_skip = offset;
    let mut first_kept = 0;

    for (i, span) in spans.iter().enumerate() {
        let w = span.content.chars().count();
        if to_skip >= w {
            to_skip -= w;
            first_kept = i + 1;
        } else {
            break;
        }
    }

    // Remove fully-skipped spans
    if first_kept > 0 {
        spans.drain(..first_kept);
    }

    // Partially skip the first remaining span
    if to_skip > 0
        && let Some(first) = spans.first_mut()
    {
        let remaining: String = first.content.chars().skip(to_skip).collect();
        *first = Span::styled(remaining, first.style);
    }

    // Phase 2: truncate to fit max_width
    truncate_line(spans, max_width);
}

pub(crate) fn format_relative_time(epoch_secs: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let delta = (now - epoch_secs).max(0) as u64;

    if delta < 60 {
        format!("{}s ago", delta)
    } else if delta < 3600 {
        format!("{}m ago", delta / 60)
    } else if delta < 86400 {
        format!("{}h ago", delta / 3600)
    } else if delta < 604_800 {
        format!("{}d ago", delta / 86400)
    } else if delta < 2_592_000 {
        format!("{}w ago", delta / 604_800)
    } else if delta < 31_536_000 {
        format!("{}mo ago", delta / 2_592_000)
    } else {
        format!("{}y ago", delta / 31_536_000)
    }
}

const AUTHOR_COLORS: [Color; 8] = [
    Color::LightBlue,
    Color::LightGreen,
    Color::LightCyan,
    Color::LightMagenta,
    Color::LightRed,
    Color::LightYellow,
    Color::Rgb(255, 165, 0),   // orange
    Color::Rgb(180, 150, 255), // lavender
];

pub(crate) fn author_color(name: &str) -> Color {
    // FNV-1a hash
    let mut hash: u32 = 2_166_136_261;
    for byte in name.bytes() {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16_777_619);
    }
    AUTHOR_COLORS[(hash as usize) % AUTHOR_COLORS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn label(name: &str, is_head: bool, is_remote: bool, is_worktree: bool) -> BranchLabel {
        BranchLabel {
            name: name.to_string(),
            is_head,
            is_remote,
            is_worktree,
            is_tag: false,
        }
    }

    #[test]
    fn test_truncate_line_no_op_when_fits() {
        let mut spans = vec![Span::raw("abc"), Span::raw("def")];
        truncate_line(&mut spans, 10);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "abcdef");
    }

    #[test]
    fn test_truncate_line_adds_ellipsis() {
        let mut spans = vec![Span::raw("hello "), Span::raw("world this is long")];
        truncate_line(&mut spans, 10);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "hello wo..");
    }

    #[test]
    fn test_truncate_line_cuts_at_span_boundary() {
        let mut spans = vec![Span::raw("12345"), Span::raw("67890")];
        truncate_line(&mut spans, 5);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        // First span fills exactly 5, second span starts overflow → back up into previous span
        assert_eq!(text, "123..");
    }

    #[test]
    fn test_truncate_line_zero_width() {
        let mut spans = vec![Span::raw("hello")];
        truncate_line(&mut spans, 0);
        assert!(spans.is_empty());
    }

    #[test]
    fn test_empty_labels_returns_empty() {
        let spans = render_branch_labels(&[], 24);
        assert!(spans.is_empty());
    }

    #[test]
    fn test_label_truncation_multibyte_no_panic() {
        // Byte-slicing this name at 24 would split a 3-byte char and panic
        let labels = vec![label(
            "origin/功能分支名称很长的多字节测试超过上限",
            false,
            true,
            false,
        )];
        let spans = render_branch_labels(&labels, 24);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains('\u{2026}'), "got: {text}");
    }

    #[test]
    fn test_head_label_has_star_prefix() {
        let labels = vec![label("main", true, false, false)];
        let spans = render_branch_labels(&labels, 24);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("* main"), "got: {text}");
    }

    #[test]
    fn test_truncation_adds_ellipsis() {
        let labels = vec![label("very-long-branch-name-here", false, false, false)];
        let spans = render_branch_labels(&labels, 10);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("very-long-\u{2026}"), "got: {text}");
        assert!(!text.contains("very-long-branch-name-here"));
    }

    #[test]
    fn test_worktree_label_has_house_prefix() {
        let labels = vec![label("feature", false, false, true)];
        let spans = render_branch_labels(&labels, 24);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("\u{2302} feature"), "got: {text}");
    }

    #[test]
    fn test_multiple_labels_comma_separated() {
        let labels = vec![
            label("main", true, false, false),
            label("origin/main", false, true, false),
        ];
        let spans = render_branch_labels(&labels, 24);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains(", "), "got: {text}");
        assert!(text.starts_with('('));
        assert!(text.contains(')'));
    }

    #[test]
    fn test_relative_time_formats() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        for (offset, expected) in [
            (30, "30s ago"),
            (7200, "2h ago"),
            (259_200, "3d ago"),
            (1_209_600, "2w ago"),
            (12_960_000, "5mo ago"),
            (63_072_000, "2y ago"),
            (-1000, "0s ago"),
        ] {
            assert_eq!(format_relative_time(now - offset), expected);
        }
    }

    #[test]
    fn test_truncate_line_unicode_chars() {
        // Box-drawing chars are each 1 display column
        let mut spans = vec![Span::raw("│ ● "), Span::raw("hello world")];
        truncate_line(&mut spans, 8);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        // 4 chars from first span + 2 chars + ".." from second
        assert_eq!(text, "│ ● he..");
    }

    #[test]
    fn test_render_graph_prefix_horizontal_dash_between_spans() {
        use crate::git::graph::{LaneSegment, lane_color};
        use crate::git::test_support;

        let mut row =
            test_support::graph_row("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "", Vec::new());
        row.lanes = vec![
            LaneSegment::RightTee,
            LaneSegment::CrossHorizontal,
            LaneSegment::MergeLeft,
        ];
        row.horizontal_spans = vec![(0, 2, lane_color(2))];

        let spans = render_graph_prefix(&row);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        // ├─┼─╯ (space after last glyph)
        assert_eq!(text, "├─┼─╯ ");
    }

    #[test]
    fn test_commit_dot_red_when_unpushed() {
        use crate::git::test_support;

        let row =
            test_support::graph_row("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "", Vec::new());

        let spans = render_graph_prefix(&row);
        assert_eq!(spans[0].content.as_ref(), "●");
        assert_eq!(spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn test_commit_dot_green_when_pushed() {
        use crate::git::test_support;

        let mut row =
            test_support::graph_row("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "", Vec::new());
        row.is_pushed = true;

        let spans = render_graph_prefix(&row);
        assert_eq!(spans[0].content.as_ref(), "●");
        assert_eq!(spans[0].style.fg, Some(Color::Green));
    }

    #[test]
    fn test_author_color_deterministic() {
        let c1 = author_color("Alice");
        let c2 = author_color("Alice");
        assert_eq!(c1, c2);
        // Different names should (likely) get different colors
        let c3 = author_color("Bob");
        assert_ne!(c1, c3);
    }

    #[test]
    fn test_tag_label_renders_yellow() {
        let labels = vec![BranchLabel {
            name: "v1.0.0".to_string(),
            is_head: false,
            is_remote: false,
            is_worktree: false,
            is_tag: true,
        }];
        let spans = render_branch_labels(&labels, 24);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("v1.0.0"), "got: {text}");
        // Tag span should use LightYellow
        let tag_span = spans
            .iter()
            .find(|s| s.content.as_ref() == "v1.0.0")
            .unwrap();
        assert_eq!(tag_span.style.fg, Some(Color::LightYellow));
    }

    #[test]
    fn test_h_scroll_zero_offset_same_as_truncate() {
        let mut a = vec![Span::raw("hello "), Span::raw("world this is long")];
        let mut b = a.clone();
        h_scroll_line(&mut a, 0, 10);
        truncate_line(&mut b, 10);
        let text_a: String = a.iter().map(|s| s.content.as_ref()).collect();
        let text_b: String = b.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text_a, text_b);
    }

    #[test]
    fn test_h_scroll_skips_characters() {
        let mut spans = vec![Span::raw("abcdef"), Span::raw("ghij")];
        h_scroll_line(&mut spans, 3, 20);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "defghij");
    }

    #[test]
    fn test_h_scroll_skips_full_span() {
        let mut spans = vec![Span::raw("abc"), Span::raw("def"), Span::raw("ghi")];
        h_scroll_line(&mut spans, 4, 20);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "efghi");
    }

    #[test]
    fn test_h_scroll_then_truncate() {
        let mut spans = vec![Span::raw("abcdef"), Span::raw("ghijklmnop")];
        h_scroll_line(&mut spans, 3, 5);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "def..");
    }

    #[test]
    fn test_h_scroll_beyond_content_yields_empty() {
        let mut spans = vec![Span::raw("abc")];
        h_scroll_line(&mut spans, 10, 20);
        assert!(spans.is_empty());
    }
}
