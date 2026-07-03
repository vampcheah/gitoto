use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Wrap},
};

use crate::app::{App, HELP_PAGE_COUNT};
use crate::components::Component;
use crate::components::layout::{self as component_layout, centered_rect, modal_rect};
use crate::components::panel;
use crate::components::style::fg_span;
use crate::config::UpdatePosition;

enum HelpRow {
    Item(&'static str, &'static str),
    Note(&'static str),
}

const HELP_PAGES: &[(&str, &[HelpRow])] = &[
    (
        "Global",
        &[
            HelpRow::Item("h / ?", "Open or close this help"),
            HelpRow::Item("Tab", "Next help page while help is open"),
            HelpRow::Item("Shift+Tab", "Previous help page while help is open"),
            HelpRow::Item("Esc / q", "Close help, close current view, or quit"),
            HelpRow::Item("Ctrl+C", "Quit immediately"),
            HelpRow::Item("Tab", "Cycle focus when help is closed"),
            HelpRow::Item("r", "Refresh all repo statuses"),
            HelpRow::Item("R", "Rescan configured directories"),
            HelpRow::Item("F", "Toggle fast mode"),
            HelpRow::Item("o", "Show operation log"),
            HelpRow::Item("g", "Reload graph for selected repo"),
            HelpRow::Item("a", "Add a repository path"),
            HelpRow::Item("y", "Copy selected item to clipboard"),
        ],
    ),
    (
        "Repositories",
        &[
            HelpRow::Item("j / Down", "Select next repo or worktree"),
            HelpRow::Item("k / Up", "Select previous repo or worktree"),
            HelpRow::Item("Enter", "Focus selected repo in Changes and Graph"),
            HelpRow::Item("w", "Toggle linked worktrees"),
            HelpRow::Item("c", "Commit all changes in selected repo"),
            HelpRow::Item("p", "Push selected repo when upstream exists"),
            HelpRow::Item("P", "Publish branch with git push -u origin"),
            HelpRow::Item("d", "Remove repo from gitoto after confirmation"),
            HelpRow::Item("s", "Cycle sort order"),
            HelpRow::Item("Right click", "Open repo context menu"),
            HelpRow::Item("Wheel", "Move selection"),
        ],
    ),
    (
        "Changes",
        &[
            HelpRow::Item("j / Down", "Select next changed file"),
            HelpRow::Item("k / Up", "Previous changed file"),
            HelpRow::Item("Enter", "Open split diff view"),
            HelpRow::Item("Esc / Left", "Close diff view"),
            HelpRow::Item("Space", "Mark/unmark file for partial commit"),
            HelpRow::Item("c", "Commit selected repo (marked files only, or all)"),
            HelpRow::Item("p", "Push selected repo"),
            HelpRow::Item("P", "Publish selected branch"),
            HelpRow::Item("Wheel", "Scroll diff or file list"),
        ],
    ),
    (
        "Graph",
        &[
            HelpRow::Item("j / Down", "Select next commit or file"),
            HelpRow::Item("k / Up", "Select previous commit or file"),
            HelpRow::Item("Left / Right", "Scroll graph horizontally"),
            HelpRow::Item("Enter", "Open commit files or file diff"),
            HelpRow::Item("Esc / Left", "Close commit diff/detail"),
            HelpRow::Item("/", "Search commits"),
            HelpRow::Item("n / N", "Next or previous search result"),
            HelpRow::Item("f", "Toggle first-parent mode"),
            HelpRow::Item("c", "Collapse or expand branch"),
            HelpRow::Item("H", "Expand all collapsed branches"),
        ],
    ),
    (
        "Legend And Menu",
        &[
            HelpRow::Item("*", "Repo has uncommitted changes"),
            HelpRow::Item("↑ / ↓", "Commits ahead / behind upstream"),
            HelpRow::Item("[n]", "Changed file count"),
            HelpRow::Item("~", "Git operation in progress"),
            HelpRow::Item("Red dot", "Commit is not on a GitHub remote"),
            HelpRow::Item("Green dot", "Commit is on a GitHub remote"),
            HelpRow::Item("Private repo", "Context menu creates GitHub private repo"),
            HelpRow::Item("Public repo", "Context menu creates GitHub public repo"),
            HelpRow::Note("GitHub repo creation requires gh auth login."),
        ],
    ),
];

fn help_key(key: &str) -> Span<'static> {
    fg_span(format!("  {key:<14}"), Color::Yellow)
}

fn help_note(text: &str) -> Line<'static> {
    Line::from(fg_span(format!(" {text}"), Color::DarkGray))
}

fn help_line(row: &HelpRow) -> Line<'static> {
    match row {
        HelpRow::Item(key, desc) => Line::from(vec![help_key(key), Span::raw(*desc)]),
        HelpRow::Note(text) => help_note(text),
    }
}

impl App {
    pub(super) fn draw(&mut self, frame: &mut ratatui::Frame) -> color_eyre::Result<()> {
        let area = frame.area();
        let (main_area, status_area) = component_layout::split_with_status_bar(area);
        let (repo_area, changes_area, graph_area) =
            component_layout::split_three_vertical_by_fraction(
                main_area,
                self.border_frac[0],
                self.border_frac[1],
            );

        self.repo_area = repo_area;
        self.changes_area = changes_area;
        self.graph_area = graph_area;

        self.repo_list.focused = self.focus == crate::app::FocusPanel::Repos;
        self.file_list.focused = self.focus == crate::app::FocusPanel::Changes;
        self.git_graph.focused = self.focus == crate::app::FocusPanel::Graph;

        self.file_list.horizontal_layout = false;
        self.git_graph.horizontal_layout = false;

        self.repo_list.draw(frame, repo_area)?;
        self.file_list.draw(frame, changes_area)?;
        self.git_graph.draw(frame, graph_area)?;

        if self.dragging_border.is_some() {
            self.draw_drag_highlight(frame, repo_area, changes_area, graph_area);
        }

        self.status_bar.focus = self.focus;
        self.status_bar.error = self.error_message.clone();
        self.status_bar.success = self.success_message.clone();
        self.status_bar.fast_mode = self.fast_mode;
        self.status_bar.focused_repo = self.focused_repo_name();
        self.status_bar.draw(frame, status_area)?;

        self.context_menu.draw(frame, area)?;
        self.path_input.draw(frame, area);
        self.commit_input.draw(frame, area);
        self.github_repo_input.draw(frame, area);
        self.confirm_dialog.draw(frame, area);
        self.notice_dialog.draw(frame, area)?;

        if let Some(ref version) = self.update_version {
            self.draw_update_notification(frame, main_area, version);
        }
        if self.show_help {
            self.draw_help(frame, main_area);
        }
        if self.show_operation_log {
            self.draw_operation_log(frame, main_area);
        }

        Ok(())
    }

    fn draw_drag_highlight(
        &self,
        frame: &mut ratatui::Frame,
        repo_area: Rect,
        changes_area: Rect,
        graph_area: Rect,
    ) {
        let style = Style::default().fg(Color::Yellow);
        let buf = frame.buffer_mut();
        for (dragging, y) in [
            (self.dragging_border == Some(0), changes_area.y),
            (self.dragging_border == Some(1), graph_area.y),
        ] {
            if !dragging {
                continue;
            }
            for x in repo_area.x..repo_area.x + repo_area.width {
                if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(x, y)) {
                    cell.set_style(style);
                }
            }
        }
    }

    pub(super) fn draw_operation_log(&self, frame: &mut ratatui::Frame, area: Rect) {
        let rect = modal_rect(area, 84, 30, 14, 6);
        let height = rect.height;
        let max_lines = height.saturating_sub(2) as usize;

        let mut lines: Vec<Line> = self
            .operation_log
            .iter()
            .rev()
            .take(max_lines.saturating_sub(1))
            .map(|entry| Line::from(Span::raw(entry.clone())))
            .collect();
        if lines.is_empty() {
            lines.push(Line::from(fg_span("No operations yet", Color::DarkGray)));
        }
        lines.push(Line::from(fg_span("Esc/o closes", Color::DarkGray)));

        let block = panel::bordered_block(" Operation Log ".to_string(), Color::Cyan);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            rect,
        );
    }

    pub(super) fn draw_update_notification(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        version: &str,
    ) {
        let text = format!(" \u{2191} v{version} \u{00b7} cargo install gitoto ");
        let width = text.len() as u16 + 2;
        let height = 3;

        if area.width < width || area.height < height {
            return;
        }

        let x = match self.update_position {
            UpdatePosition::TopRight => area.x + area.width.saturating_sub(width + 1),
            UpdatePosition::TopLeft => area.x + 1,
        };
        let y = area.y;

        let rect = Rect::new(x, y, width, height);

        let line = Line::from(vec![
            fg_span(" \u{2191} ", Color::Green),
            fg_span(format!("v{version}"), Color::Yellow),
            fg_span(" \u{00b7} cargo install gitoto ", Color::DarkGray),
        ]);

        let block = panel::plain_block(Color::DarkGray);

        let paragraph = Paragraph::new(line).block(block);

        frame.render_widget(Clear, rect);
        frame.render_widget(paragraph, rect);
    }

    pub(super) fn draw_help(&self, frame: &mut ratatui::Frame, area: Rect) {
        let page_count = HELP_PAGES.len();
        debug_assert_eq!(page_count, HELP_PAGE_COUNT);
        let page_idx = self.help_page.min(page_count.saturating_sub(1));
        let (title, rows) = HELP_PAGES[page_idx];
        let mut lines: Vec<Line<'static>> = rows.iter().map(help_line).collect();
        lines.push(Line::from(""));
        lines.push(help_note("Tab/Shift+Tab pages. Esc/q/h closes help."));

        let height = (lines.len() as u16 + 2).min(area.height);
        let width = 64u16.min(area.width);
        let help_area = centered_rect(area, width, height);

        let block = panel::bordered_block(
            format!(" Help {}/{} - {} ", page_idx + 1, page_count, title),
            Color::Yellow,
        )
        .style(Style::default().bg(Color::Black));

        frame.render_widget(Clear, help_area);
        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, help_area);
    }
}
