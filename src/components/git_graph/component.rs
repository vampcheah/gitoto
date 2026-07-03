use color_eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Color,
    widgets::Paragraph,
};
use tokio::sync::mpsc::UnboundedSender;

use crate::action::Action;
use crate::components::Component;
use crate::components::git_graph::GitGraph;
use crate::components::layout;
use crate::components::scroll;
use crate::components::selection;
use crate::components::style::fg_style;

impl Component for GitGraph {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> Result<()> {
        self.action_tx = Some(tx);
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        if let Some(ref mut detail) = self.commit_detail {
            if detail.diff_content.is_some() {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                        detail.diff_content = None;
                        scroll::reset(&mut detail.diff_scroll);
                    }
                    _ if scroll::handle_vertical_key(&mut detail.diff_scroll, key) => {}
                    _ => {}
                }
                return Ok(None);
            }

            match key.code {
                KeyCode::Esc => {
                    self.commit_detail = None;
                    if std::mem::take(&mut self.needs_reload) {
                        self.reload_graph();
                    }
                    return Ok(None);
                }
                _ if selection::handle_vertical_key(
                    &mut detail.file_state,
                    detail.files.len(),
                    key,
                ) =>
                {
                    return Ok(None);
                }
                KeyCode::Enter => {
                    return Ok(self.try_show_commit_diff());
                }
                _ => return Ok(None),
            }
        }

        let row_count = self.display_rows().len();
        match key.code {
            KeyCode::Char('n') => {
                self.search_next();
                Ok(None)
            }
            KeyCode::Char('N') => {
                self.search_prev();
                Ok(None)
            }
            _ if selection::handle_vertical_key(&mut self.state, row_count, key) => Ok(None),
            KeyCode::Char('/') => {
                self.search.open();
                Ok(None)
            }
            KeyCode::Enter => Ok(self.try_show_commit_files()),
            KeyCode::Char('f') => {
                self.graph_options.first_parent = !self.graph_options.first_parent;
                self.reload_graph();
                Ok(None)
            }
            KeyCode::Char('c') => {
                self.toggle_collapse_selected();
                Ok(None)
            }
            KeyCode::Char('H') => {
                self.expand_all_branches();
                Ok(None)
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.h_scroll = self.h_scroll.saturating_add(4);
                Ok(None)
            }
            KeyCode::Char('h') | KeyCode::Left => {
                self.h_scroll = self.h_scroll.saturating_sub(4);
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn handle_mouse_event(&mut self, mouse: MouseEvent) -> Result<Option<Action>> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let pos = ratatui::layout::Position::new(mouse.column, mouse.row);

                if self.graph_list_area.contains(pos) {
                    if let Some(idx) = selection::clicked_list_index(
                        self.graph_list_area,
                        mouse.column,
                        mouse.row,
                        self.state.offset(),
                        self.display_rows().len(),
                    ) {
                        if self.state.selected() == Some(idx) && self.commit_detail.is_none() {
                            return Ok(self.try_show_commit_files());
                        }
                        self.state.select(Some(idx));
                        self.commit_detail = None;
                        if std::mem::take(&mut self.needs_reload) {
                            self.reload_graph();
                        }
                    }
                    return Ok(None);
                }

                let mut open_file_diff = false;
                if let Some(ref mut detail) = self.commit_detail
                    && detail.file_list_area.contains(pos)
                    && let Some(idx) = selection::clicked_list_index(
                        detail.file_list_area,
                        mouse.column,
                        mouse.row,
                        detail.file_state.offset(),
                        detail.files.len(),
                    )
                {
                    if detail.file_state.selected() == Some(idx) {
                        open_file_diff = true;
                    } else {
                        detail.file_state.select(Some(idx));
                    }
                }
                if open_file_diff {
                    return Ok(self.try_show_commit_diff());
                }

                Ok(None)
            }
            MouseEventKind::ScrollUp => {
                let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
                if let Some(ref mut detail) = self.commit_detail {
                    if self.diff_area.contains(pos) && detail.diff_content.is_some() {
                        scroll::up(&mut detail.diff_scroll);
                        return Ok(None);
                    }
                    if detail.msg_area.contains(pos) {
                        scroll::up(&mut detail.msg_scroll);
                        return Ok(None);
                    }
                    if detail.file_list_area.contains(pos) && !detail.files.is_empty() {
                        selection::select_prev(&mut detail.file_state, detail.files.len());
                        return Ok(None);
                    }
                }
                let row_count = self.display_rows().len();
                selection::select_prev(&mut self.state, row_count);
                Ok(None)
            }
            MouseEventKind::ScrollDown => {
                let pos = ratatui::layout::Position::new(mouse.column, mouse.row);
                if let Some(ref mut detail) = self.commit_detail {
                    if self.diff_area.contains(pos) && detail.diff_content.is_some() {
                        scroll::down(&mut detail.diff_scroll);
                        return Ok(None);
                    }
                    if detail.msg_area.contains(pos) {
                        scroll::down(&mut detail.msg_scroll);
                        return Ok(None);
                    }
                    if detail.file_list_area.contains(pos) && !detail.files.is_empty() {
                        selection::select_next(&mut detail.file_state, detail.files.len());
                        return Ok(None);
                    }
                }
                let row_count = self.display_rows().len();
                selection::select_next(&mut self.state, row_count);
                Ok(None)
            }
            MouseEventKind::ScrollLeft => {
                self.h_scroll = self.h_scroll.saturating_sub(4);
                Ok(None)
            }
            MouseEventKind::ScrollRight => {
                self.h_scroll = self.h_scroll.saturating_add(4);
                Ok(None)
            }
            MouseEventKind::Down(MouseButton::Right) => Ok(None),
            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.render_area = area;

        match &self.commit_detail {
            Some(detail) if detail.diff_content.is_some() => {
                let chunks = layout::split_oriented(
                    area,
                    self.horizontal_layout,
                    [
                        Constraint::Percentage(40),
                        Constraint::Percentage(25),
                        Constraint::Percentage(35),
                    ],
                );

                self.graph_list_area = chunks[0];
                self.files_area = chunks[1];
                self.diff_area = chunks[2];

                self.draw_graph_list(frame, chunks[0]);
                let detail = self.commit_detail.as_mut().unwrap();
                Self::draw_commit_files(detail, frame, chunks[1]);
                Self::draw_commit_diff(detail, frame, chunks[2]);
            }
            Some(_) => {
                let chunks = layout::split_oriented(
                    area,
                    self.horizontal_layout,
                    [Constraint::Percentage(50), Constraint::Percentage(50)],
                );

                self.graph_list_area = chunks[0];
                self.files_area = chunks[1];
                self.diff_area = Rect::default();

                self.draw_graph_list(frame, chunks[0]);
                let detail = self.commit_detail.as_mut().unwrap();
                Self::draw_commit_files(detail, frame, chunks[1]);
            }
            None => {
                self.graph_list_area = area;
                self.files_area = Rect::default();
                self.diff_area = Rect::default();

                self.draw_graph_list(frame, area);
            }
        }

        if self.search.visible {
            let match_info = if self.search.input.is_empty() {
                String::new()
            } else {
                let current = self.search.current_match.map(|i| i + 1).unwrap_or(0);
                format!(" {}/{}", current, self.search.matches.len())
            };
            let overlay_text = format!(" / {}{} ", self.search.input, match_info);
            let overlay_area = Rect::new(
                self.graph_list_area.x,
                self.graph_list_area.y + self.graph_list_area.height.saturating_sub(1),
                self.graph_list_area
                    .width
                    .min(ratatui::text::Span::raw(overlay_text.as_str()).width() as u16 + 2),
                1,
            );
            let overlay =
                Paragraph::new(overlay_text).style(fg_style(Color::White).bg(Color::DarkGray));
            frame.render_widget(overlay, overlay_area);
        }

        Ok(())
    }
}
