use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::App;
use crate::config::UpdatePosition;

impl App {
    pub(super) fn draw_operation_log(&self, frame: &mut ratatui::Frame, area: Rect) {
        let width = 84u16.min(area.width.saturating_sub(4)).max(30);
        let height = 14u16.min(area.height.saturating_sub(2)).max(6);
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let rect = Rect::new(x, y, width, height);
        let max_lines = height.saturating_sub(2) as usize;

        let mut lines: Vec<Line> = self
            .operation_log
            .iter()
            .rev()
            .take(max_lines.saturating_sub(1))
            .map(|entry| Line::from(Span::raw(entry.clone())))
            .collect();
        if lines.is_empty() {
            lines.push(Line::from(Span::styled(
                "No operations yet",
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(Line::from(Span::styled(
            "Esc/o closes",
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .title(" Operation Log ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
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
            Span::styled(" \u{2191} ", Style::default().fg(Color::Green)),
            Span::styled(format!("v{version}"), Style::default().fg(Color::Yellow)),
            Span::styled(
                " \u{00b7} cargo install gitoto ",
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));

        let paragraph = Paragraph::new(line).block(block);

        frame.render_widget(Clear, rect);
        frame.render_widget(paragraph, rect);
    }

    pub(super) fn draw_help(&self, frame: &mut ratatui::Frame, area: Rect) {
        fn key(k: &str) -> Span<'static> {
            Span::styled(format!("  {k:<14}"), Style::default().fg(Color::Yellow))
        }
        fn desc(d: &str) -> Span<'static> {
            Span::raw(d.to_string())
        }
        fn item(k: &str, d: &str) -> Line<'static> {
            Line::from(vec![key(k), desc(d)])
        }
        fn note(text: &str) -> Line<'static> {
            Line::from(Span::styled(
                format!(" {text}"),
                Style::default().fg(Color::DarkGray),
            ))
        }

        let pages: Vec<(&str, Vec<Line<'static>>)> = vec![
            (
                "Global",
                vec![
                    item("h / ?", "Open or close this help"),
                    item("Tab", "Next help page while help is open"),
                    item("Shift+Tab", "Previous help page while help is open"),
                    item("Esc / q", "Close help, close current view, or quit"),
                    item("Ctrl+C", "Quit immediately"),
                    item("Tab", "Cycle focus when help is closed"),
                    item("r", "Refresh all repo statuses"),
                    item("R", "Rescan configured directories"),
                    item("F", "Toggle fast mode"),
                    item("o", "Show operation log"),
                    item("g", "Reload graph for selected repo"),
                    item("a", "Add a repository path"),
                    item("y", "Copy selected item to clipboard"),
                ],
            ),
            (
                "Repositories",
                vec![
                    item("j / Down", "Select next repo or worktree"),
                    item("k / Up", "Select previous repo or worktree"),
                    item("Enter", "Focus selected repo in Changes and Graph"),
                    item("w", "Toggle linked worktrees"),
                    item("c", "Commit all changes in selected repo"),
                    item("p", "Push selected repo when upstream exists"),
                    item("P", "Publish branch with git push -u origin"),
                    item("d", "Remove repo from gitoto after confirmation"),
                    item("s", "Cycle sort order"),
                    item("Right click", "Open repo context menu"),
                    item("Wheel", "Move selection"),
                ],
            ),
            (
                "Changes",
                vec![
                    item("j / Down", "Select next changed file"),
                    item("k / Up", "Previous changed file"),
                    item("Enter", "Open split diff view"),
                    item("Esc / Left", "Close diff view"),
                    item("c", "Commit selected repo"),
                    item("p", "Push selected repo"),
                    item("P", "Publish selected branch"),
                    item("Wheel", "Scroll diff or file list"),
                ],
            ),
            (
                "Graph",
                vec![
                    item("j / Down", "Select next commit or file"),
                    item("k / Up", "Select previous commit or file"),
                    item("Left / Right", "Scroll graph horizontally"),
                    item("Enter", "Open commit files or file diff"),
                    item("Esc / Left", "Close commit diff/detail"),
                    item("/", "Search commits"),
                    item("n / N", "Next or previous search result"),
                    item("f", "Toggle first-parent mode"),
                    item("c", "Collapse or expand branch"),
                    item("H", "Expand all collapsed branches"),
                ],
            ),
            (
                "Legend And Menu",
                vec![
                    item("*", "Repo has uncommitted changes"),
                    item("↑ / ↓", "Commits ahead / behind upstream"),
                    item("[n]", "Changed file count"),
                    item("~", "Git operation in progress"),
                    item("Red dot", "Commit is not on a GitHub remote"),
                    item("Green dot", "Commit is on a GitHub remote"),
                    item("Private repo", "Context menu creates GitHub private repo"),
                    item("Public repo", "Context menu creates GitHub public repo"),
                    note("GitHub repo creation requires gh auth login."),
                ],
            ),
        ];

        let page_count = pages.len();
        let page_idx = self.help_page.min(page_count.saturating_sub(1));
        let (title, mut lines) = pages[page_idx].clone();
        lines.push(Line::from(""));
        lines.push(note("Tab/Shift+Tab pages. Esc/q/h closes help."));

        let height = (lines.len() as u16 + 2).min(area.height);
        let width = 64u16.min(area.width);
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let help_area = Rect::new(x, y, width, height);

        let block = Block::default()
            .title(format!(
                " Help {}/{} - {} ",
                page_idx + 1,
                page_count,
                title
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .style(Style::default().bg(Color::Black));

        frame.render_widget(Clear, help_area);
        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, help_area);
    }
}
