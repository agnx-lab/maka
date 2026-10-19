/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

use super::*;
use ratatui::{buffer::Buffer, style::Style};
use unicode_width::UnicodeWidthStr;

/// Pending read-only geometry. No buffer/selection/history is copied or mutated.
#[derive(Clone)]
pub(crate) struct Geometry {
    revision: u64,
    selection: Selection,
    layout: Arc<Layout>,
    area: Placement,
    top: usize,
    left: usize,
    scrollbar: Option<Placement>,
}
impl Geometry {
    /// Physical editing position from this pending frame, including wrap and clipping.
    pub(crate) fn cursor_position(&self) -> Option<Position> {
        if self.area.is_empty() {
            return None;
        }
        let (row, column) = self
            .layout
            .cursor(self.selection.cursor, self.selection.upstream);
        if row < self.top || row >= self.top + self.area.height as usize {
            return None;
        }
        self.area.point(
            column
                .saturating_sub(self.left)
                .min(self.area.width.saturating_sub(1) as usize),
            row - self.top,
        )
    }
    pub(crate) fn paint_scrollbar(
        &self,
        buffer: &mut Buffer,
        track: Style,
        thumb_style: Style,
        ascii: bool,
    ) {
        let Some(bar) = self.scrollbar else { return };
        let visible = bar.visible();
        let height = usize::from(visible.height);
        if height == 0 {
            return;
        }
        // The editor may be partly clipped by its page. Keep its logical
        // scroll range, but fit the whole track into the visible cells.
        let thumb = (height * bar.height as usize / self.layout.rows.len()).clamp(1, height);
        let maximum = self
            .layout
            .rows
            .len()
            .saturating_sub(bar.height as usize)
            .max(1);
        let top = self.top * (height - thumb) / maximum;
        for row in 0..height {
            let active = (top..top + thumb).contains(&row);
            buffer[(visible.x, visible.y + row as u16)]
                .set_char(match (ascii, active) {
                    (true, true) => '#',
                    (true, false) => '|',
                    (false, true) => '┃',
                    (false, false) => '│',
                })
                .set_style(if active { thumb_style } else { track });
        }
    }
}
impl Editor {
    pub(crate) fn mouse_presented(&mut self, mut mouse: MouseEvent, geometry: &Geometry) -> bool {
        if geometry.revision != self.revision {
            return false;
        }
        self.layout = geometry.layout.clone();
        self.left = geometry.left;
        self.area = (!geometry.area.is_empty()).then_some(geometry.area);
        // Release/move events may arrive before the next flush. They must not
        // roll a viewport change back to the previous presented frame.
        if matches!(mouse.kind, MouseEventKind::Down(_))
            || matches!(mouse.kind, MouseEventKind::Drag(_)) && self.dragging
        {
            self.top = geometry.top;
        }
        if let Some(bar) = geometry.scrollbar
            && bar.contains(Position::new(mouse.column, mouse.row))
        {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let maximum = self.layout.rows.len().saturating_sub(bar.height as usize);
                    let visible = bar.visible();
                    let row = usize::from(mouse.row - visible.y);
                    self.top = row * maximum / usize::from(visible.height.saturating_sub(1)).max(1);
                    return true;
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    // The track belongs to this editor, including at scroll boundaries.
                    mouse.column = mouse.column.saturating_sub(1);
                }
                _ => {}
            }
        }
        self.mouse(mouse)
    }
    pub(crate) fn paint_prepared(
        &self,
        area: Placement,
        buffer: &mut Buffer,
        focused: bool,
        normal: Style,
        selected: Style,
        caret: Style,
    ) -> Geometry {
        let tokens = self.inline_token_ranges();
        let mut area = area;
        let mut layout = self.retained_layout(area.width);
        let scrollbar = if !self.single_line
            && area.width > 1
            && area.height > 0
            && layout.rows.len() > area.height as usize
        {
            let bar = Placement::new(
                crate::application::CellRect::new(area.right() - 1, area.y, 1, area.height),
                area.visible(),
            );
            area.bounds.width -= 1;
            layout = self.retained_layout(area.width);
            Some(bar)
        } else {
            None
        };
        let mut left = self.left;
        let (cursor_row, cursor_column) =
            layout.cursor(self.selection.cursor, self.selection.upstream);
        let mut top = self
            .top
            .min(layout.rows.len().saturating_sub(area.height as usize));
        if self.area != Some(area) || self.layout.width != area.width {
            if cursor_row < top {
                top = cursor_row;
            }
            if cursor_row >= top + area.height as usize {
                top = cursor_row.saturating_sub(area.height.saturating_sub(1) as usize);
            }
        }
        if self.single_line {
            top = 0;
            left = left.min(
                layout.rows[0]
                    .cells
                    .last()
                    .map_or(0, |cell| cell.column + cell.width),
            );
            if cursor_column < left {
                left = cursor_column;
            }
            if cursor_column >= left + area.width as usize {
                left = cursor_column
                    .saturating_add(1)
                    .saturating_sub(area.width as usize);
            }
        } else {
            left = 0;
        }
        if !area.is_empty() {
            let selection = self.selection.range();
            for (screen_row, row) in layout
                .rows
                .iter()
                .skip(top)
                .take(area.height as usize)
                .enumerate()
            {
                let y = area.y + screen_row as i64;
                for cell in &row.cells {
                    if cell.column < left || cell.column + cell.width > left + area.width as usize {
                        continue;
                    }
                    let Some(point) = area.point(cell.column - left, screen_row) else {
                        continue;
                    };
                    if area
                        .point(
                            cell.column - left + cell.width.saturating_sub(1),
                            screen_row,
                        )
                        .is_none()
                    {
                        continue;
                    }
                    let (x, y) = (point.x, point.y);
                    let style =
                        if cell.bytes.start < selection.end && cell.bytes.end > selection.start {
                            selected
                        } else if tokens.iter().any(|token| {
                            token.start <= cell.bytes.start && cell.bytes.end <= token.end
                        }) {
                            normal.add_modifier(
                                ratatui::style::Modifier::BOLD | ratatui::style::Modifier::REVERSED,
                            )
                        } else {
                            normal
                        };
                    let text = if self.single_line && self.masked {
                        "•"
                    } else {
                        &self.text[cell.bytes.clone()]
                    };
                    if text == "\t" {
                        for column in 0..cell.width {
                            buffer[(x + column as u16, y)]
                                .set_symbol(" ")
                                .set_style(style);
                        }
                    } else if text.width() > cell.width {
                        buffer[(x, y)].set_symbol("�").set_style(style);
                    } else {
                        let symbol = if text.width() == 0 {
                            format!("◌{text}")
                        } else {
                            text.to_owned()
                        };
                        buffer.set_stringn(x, y, symbol, cell.width, style);
                    }
                }
                if self.text.as_bytes().get(row.end) == Some(&b'\n') && selection.contains(&row.end)
                {
                    let column = row.cells.last().map_or(0, |cell| cell.column + cell.width);
                    if let Some(point) = area.point(
                        column.min(area.width.saturating_sub(1) as usize),
                        (y - area.y) as usize,
                    ) {
                        buffer[(point.x, point.y)].set_style(selected);
                    }
                }
            }
            if focused
                && cursor_row >= top
                && cursor_row < top + area.height as usize
                && let Some(point) = area.point(
                    cursor_column
                        .saturating_sub(left)
                        .min(area.width.saturating_sub(1) as usize),
                    cursor_row - top,
                )
            {
                buffer[(point.x, point.y)].set_style(caret);
            }
        }
        Geometry {
            revision: self.revision,
            selection: self.selection,
            layout,
            area,
            top,
            left,
            scrollbar,
        }
    }
    pub(crate) fn present_geometry(&mut self, geometry: Geometry) {
        // A successful flush cannot overwrite geometry after a newer native edit.
        if geometry.revision != self.revision || geometry.selection != self.selection {
            return;
        }
        self.layout = geometry.layout;
        self.area = (!geometry.area.is_empty()).then_some(geometry.area);
        self.top = geometry.top;
        self.left = geometry.left;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_literal_style_stays_inside_source_cells() {
        let mut editor = Editor::default();
        editor.set_inline_tokens(&["/skill:foo".into()]);
        editor.insert("x /skill:foo z");
        let mut buffer = Buffer::empty(ratatui::layout::Rect::new(0, 0, 30, 1));
        editor.paint_prepared(
            ratatui::layout::Rect::new(0, 0, 30, 1).into(),
            &mut buffer,
            false,
            Style::default(),
            Style::default(),
            Style::default(),
        );
        assert!(
            !buffer[(0, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert!(
            buffer[(2, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD | ratatui::style::Modifier::REVERSED)
        );
        assert_eq!(buffer[(2, 0)].symbol(), "/");
        assert!(
            !buffer[(12, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
    }

    use ratatui::layout::Rect;
    fn paint(editor: &Editor, area: Rect) -> (Geometry, Buffer) {
        let mut buffer = Buffer::empty(area);
        let geometry = editor.paint_prepared(
            area.into(),
            &mut buffer,
            true,
            Style::default(),
            Style::default(),
            Style::default(),
        );
        geometry.paint_scrollbar(&mut buffer, Style::default(), Style::default(), false);
        (geometry, buffer)
    }
    #[test]
    fn long_editor_reuses_both_scrollbar_widths_across_measure_paint_and_selection() {
        let mut editor = Editor::default();
        editor.insert(&"0123456789abcdef\n".repeat(1024));
        editor.selection = Selection::default();
        let area = Rect::new(0, 0, 12, 3);
        assert_eq!(editor.rows(area.width), 2049);
        let measured = editor.retained_layout(area.width);
        let (first, buffer) = paint(&editor, area);
        assert_eq!(first.area.width, 11);
        assert_eq!(
            (0..10).map(|x| buffer[(x, 0)].symbol()).collect::<String>(),
            "0123456789"
        );
        assert_eq!(
            (0..6).map(|x| buffer[(x, 1)].symbol()).collect::<String>(),
            "abcdef"
        );
        editor.present_geometry(first.clone());
        editor.key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        assert_eq!(editor.selected_text(), "0");
        let (second, _) = paint(&editor, area);
        assert!(Arc::ptr_eq(&first.layout, &second.layout));
        assert!(Arc::ptr_eq(&measured, &editor.retained_layout(area.width)));
        assert_eq!(editor.rows(area.width), 2049);
        assert_eq!(second.cursor_position(), Some(Position::new(1, 0)));
        assert_eq!(editor.layout_cache.borrow().widths.len(), 2);
    }

    #[test]
    fn resize_reuses_previous_width_but_edits_undo_and_restore_replace_content_geometry() {
        let mut editor = Editor::default();
        editor.insert("abcdefgh");
        let wide = Rect::new(0, 0, 6, 4);
        let (first, _) = paint(&editor, wide);
        assert_eq!(first.cursor_position(), Some(Position::new(3, 1)));
        editor.present_geometry(first.clone());
        editor.key(KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT));
        assert_eq!(editor.selected_text(), "h");

        let (narrow, buffer) = paint(&editor, Rect::new(0, 0, 4, 4));
        assert_eq!(buffer[(0, 1)].symbol(), "d");
        assert_eq!(narrow.cursor_position(), Some(Position::new(1, 2)));
        editor.present_geometry(narrow);
        let (wide_again, _) = paint(&editor, wide);
        assert!(Arc::ptr_eq(&first.layout, &wide_again.layout));
        assert_eq!(wide_again.cursor_position(), Some(Position::new(2, 1)));
        editor.present_geometry(wide_again);

        editor.insert("Z");
        let (edited, buffer) = paint(&editor, wide);
        assert!(!Arc::ptr_eq(&first.layout, &edited.layout));
        assert_eq!(buffer[(2, 1)].symbol(), "Z");
        assert_eq!(editor.text(), "abcdefgZ");
        editor.undo(false);
        let (undone, buffer) = paint(&editor, wide);
        assert!(!Arc::ptr_eq(&edited.layout, &undone.layout));
        assert_eq!(buffer[(2, 1)].symbol(), "h");
        assert_eq!(editor.selected_text(), "h");

        assert!(editor.restore("中🙂ab", 0, 0));
        let (restored, buffer) = paint(&editor, wide);
        assert!(!Arc::ptr_eq(&undone.layout, &restored.layout));
        assert_eq!(buffer[(0, 0)].symbol(), "中");
        assert_eq!(buffer[(2, 0)].symbol(), "🙂");
        assert_eq!(restored.cursor_position(), Some(Position::new(0, 0)));
    }

    #[test]
    fn inline_token_updates_refresh_paint_and_atomic_selection_without_reflow() {
        let mut editor = Editor::default();
        editor.insert("x /skill:foo z");
        let area = Rect::new(0, 0, 30, 1);
        let (plain, buffer) = paint(&editor, area);
        assert!(
            !buffer[(2, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        editor.set_inline_tokens(&["/skill:foo".into()]);
        let (token, buffer) = paint(&editor, area);
        assert!(Arc::ptr_eq(&plain.layout, &token.layout));
        assert_eq!(buffer[(2, 0)].symbol(), "/");
        assert!(
            buffer[(2, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        editor.selection = Selection {
            cursor: 7,
            anchor: Some(6),
            upstream: false,
        };
        assert_eq!(editor.selected_text(), "/skill:foo");
        editor.set_inline_tokens(&[]);
        assert_eq!(editor.selected_text(), "l");
        let (without_token, buffer) = paint(&editor, area);
        assert!(Arc::ptr_eq(&token.layout, &without_token.layout));
        assert!(
            !buffer[(2, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );

        editor.set_inline_tokens(&["/skill:foo".into()]);
        assert!(editor.restore("/skill:foo x", 2, 3));
        assert_eq!(editor.selected_text(), "/skill:foo");
        let (_, buffer) = paint(&editor, area);
        assert_eq!(buffer[(0, 0)].symbol(), "/");
        assert!(
            buffer[(0, 0)]
                .modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
    }

    #[test]
    fn overflow_track_reserves_cells_and_scrolls_without_changing_selection() {
        let mut editor = Editor::default();
        editor.insert("abcdef\nghijkl\nmnopqr\nstuvwx");
        editor.selection = Selection::default();
        let area = Rect::new(0, 0, 6, 3);
        let (geometry, mut buffer) = paint(&editor, area);
        assert_eq!(geometry.area.width, 5);
        assert_eq!(buffer[(3, 0)].symbol(), "d");
        assert_eq!(buffer[(4, 0)].symbol(), " ");
        assert_eq!(buffer[(0, 1)].symbol(), "e");
        assert_eq!(buffer[(5, 0)].symbol(), "┃");
        assert_eq!(buffer[(5, 2)].symbol(), "│");
        editor.present_geometry(geometry.clone());
        let selection = editor.selection;
        assert!(editor.mouse_presented(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            &geometry
        ));
        for kind in [
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            editor.mouse_presented(
                MouseEvent {
                    kind,
                    column: 5,
                    row: 2,
                    modifiers: KeyModifiers::NONE,
                },
                &geometry,
            );
        }
        assert_eq!(editor.selection, selection);
        let (geometry, bottom) = paint(&editor, area);
        assert_eq!(bottom[(0, 2)].symbol(), "w");
        assert_eq!(bottom[(5, 2)].symbol(), "┃");
        editor.present_geometry(geometry.clone());
        assert!(editor.mouse_presented(
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 5,
                row: 2,
                modifiers: KeyModifiers::NONE,
            },
            &geometry
        ));
        assert_eq!(editor.selection, selection);
        geometry.paint_scrollbar(&mut buffer, Style::default(), Style::default(), true);
        assert_eq!(buffer[(5, 0)].symbol(), "|");
        assert_eq!(buffer[(5, 2)].symbol(), "#");
        let wheel = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 2,
            modifiers: KeyModifiers::NONE,
        };
        assert!(editor.mouse_presented(wheel, &geometry));
        assert!(editor.mouse_presented(wheel, &geometry));
        let (_, top) = paint(&editor, area);
        assert_eq!(top[(0, 0)].symbol(), "a");
        assert_eq!(top[(5, 0)].symbol(), "┃");
        assert_eq!(editor.selection, selection);
    }
    #[test]
    fn clipped_editor_keeps_a_visible_moving_scrollbar_thumb() {
        use ratatui::style::Color;
        let mut editor = Editor::default();
        editor.insert(
            &(0..40)
                .map(|n| format!("line-{n:02}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let area = Placement::new(
            crate::application::CellRect::new(0, 0, 12, 12),
            Rect::new(0, 0, 12, 4),
        );
        let draw = |editor: &Editor| {
            let mut buffer = Buffer::empty(area.visible());
            let geometry = editor.paint_prepared(
                area,
                &mut buffer,
                false,
                Style::default(),
                Style::default(),
                Style::default(),
            );
            geometry.paint_scrollbar(
                &mut buffer,
                Style::default().fg(Color::DarkGray),
                Style::default().fg(Color::Gray),
                false,
            );
            (geometry, buffer)
        };
        let (geometry, bottom) = draw(&editor);
        assert_eq!(
            bottom[(11, 3)].symbol(),
            "┃",
            "the thumb must remain visible when the parent clips the editor"
        );
        assert_eq!(bottom[(11, 3)].fg, Color::Gray);
        assert_eq!(bottom[(11, 0)].fg, Color::DarkGray);
        editor.present_geometry(geometry.clone());
        let selection = editor.selection;
        for _ in 0..40 {
            assert!(editor.mouse_presented(
                MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    column: 11,
                    row: 1,
                    modifiers: KeyModifiers::NONE
                },
                &geometry
            ));
        }
        let (geometry, top) = draw(&editor);
        assert_eq!(top[(11, 0)].symbol(), "┃");
        assert_eq!(top[(11, 3)].symbol(), "│");
        assert_eq!(top[(11, 0)].fg, Color::Gray);
        assert_eq!(editor.selection, selection);
        editor.mouse_presented(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 11,
                row: 3,
                modifiers: KeyModifiers::NONE,
            },
            &geometry,
        );
        assert_eq!(
            editor.top, 28,
            "clicks use the visible track, including after clipping"
        );
        assert_eq!(editor.selection, selection);
    }
    #[test]
    fn fitting_and_single_column_editors_keep_their_full_content_width() {
        for (text, area) in [
            ("abc", Rect::new(0, 0, 3, 2)),
            ("abc", Rect::new(0, 0, 1, 2)),
        ] {
            let mut editor = Editor::default();
            editor.insert(text);
            let (geometry, _) = paint(&editor, area);
            assert_eq!(geometry.area.width, area.width);
            assert!(geometry.scrollbar.is_none());
        }
    }
    #[test]
    fn wheel_viewport_survives_repaint_and_unrelated_selection_observation() {
        let mut editor = Editor::default();
        editor.insert(
            &(0..20)
                .map(|i| format!("row {i:02}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let area = Rect::new(0, 0, 20, 4);
        let (visible, _) = paint(&editor, area);
        editor.present_geometry(visible.clone());
        let wheel = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        };
        assert!(editor.mouse_presented(wheel, &visible));
        assert!(editor.mouse_presented(wheel, &visible));
        let (pending, first) = paint(&editor, area);
        let line = (0..6).map(|x| first[(x, 0)].symbol()).collect::<String>();
        assert_eq!(line, "row 10");
        editor.present_geometry(pending);
        let (_, second) = paint(&editor, area);
        assert_eq!(first, second);
        // Native keyboard navigation requests reveal, unlike passive paint.
        editor.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let (_, after_key) = paint(&editor, area);
        assert_ne!(first, after_key);
    }
    #[test]
    fn masked_single_line_full_capacity_and_unicode_hit_use_original_bytes() {
        let mut editor = Editor::single_line(65536, true);
        editor.insert(&"x".repeat(65536));
        let area = Rect::new(3, 2, 20, 1);
        let (visible, buffer) = paint(&editor, area);
        assert_eq!(visible.layout.rows.len(), 1);
        assert_eq!(visible.layout.rows[0].cells.last().unwrap().column, 65535);
        assert_eq!(visible.left, 65517);
        assert!(!buffer.content().iter().any(|cell| cell.symbol() == "x"));
        editor.present_geometry(visible.clone());
        assert_eq!(editor.cursor_position(), Some(Position::new(22, 2)));
        assert!(editor.mouse_presented(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 3,
                row: 2,
                modifiers: KeyModifiers::NONE
            },
            &visible
        ));
        assert_eq!(editor.selection.cursor, 65517);
        let old = visible;
        editor.clear();
        editor.insert("A👩🏽‍💻e\u{301}中");
        assert!(!editor.mouse_presented(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 3,
                row: 2,
                modifiers: KeyModifiers::NONE
            },
            &old
        ));
        let (visible, _) = paint(&editor, area);
        editor.present_geometry(visible.clone());
        assert!(editor.mouse_presented(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 5,
                row: 2,
                modifiers: KeyModifiers::NONE
            },
            &visible
        ));
        assert_eq!(editor.selection.cursor, "A👩🏽‍💻".len());
        editor.key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        assert_eq!(editor.selected_text(), "e\u{301}");
    }
    #[test]
    fn clipped_editor_paints_and_hits_original_unicode_row_without_rebasing_source() {
        let mut editor = Editor::default();
        editor.insert("00\n11\n中文😀\n44\n55");
        editor.selection.cursor = 0;
        let clip = Rect::new(0, 0, 6, 3);
        let area = Placement::new(crate::application::CellRect::new(0, -2, 12, 6), clip);
        let mut buffer = Buffer::empty(clip);
        let geometry = editor.paint_prepared(
            area,
            &mut buffer,
            false,
            Style::default(),
            Style::default(),
            Style::default(),
        );
        assert_eq!(buffer[(0, 0)].symbol(), "中");
        assert_eq!(buffer[(2, 0)].symbol(), "文");
        assert_eq!(buffer[(4, 0)].symbol(), "😀");
        assert!(editor.mouse_presented(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 4,
                row: 0,
                modifiers: KeyModifiers::NONE
            },
            &geometry
        ));
        assert_eq!(editor.selection.cursor, "00\n11\n中文".len());
    }
}
