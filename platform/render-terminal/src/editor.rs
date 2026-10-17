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

//! Bounded, grapheme-aware native editor. Inspired by grok-build's separation of byte
//! editing and display geometry; implemented here against Ratatui 0.30.
//! No Host state or submission authority belongs in this widget.
pub(crate) mod layout;
mod presentation;
pub(crate) use presentation::Geometry;

use std::{collections::VecDeque, ops::Range, sync::Arc};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;
use unicode_segmentation::UnicodeSegmentation;

use crate::application::Placement;
use layout::Layout;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Selection {
    cursor: usize,
    anchor: Option<usize>,
    // A soft-wrap boundary has two visual positions for one byte offset.
    upstream: bool,
}

/// Local insertion target for an admitted asynchronous native paste.
#[derive(Clone, Copy)]
pub(crate) struct PasteTarget {
    revision: u64,
    selection: Selection,
}

const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_HISTORY_BYTES: usize = 1024 * 1024;
const MAX_HISTORY_EDITS: usize = 128;

impl Selection {
    fn range(self) -> Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }
}

struct Edit {
    start: usize,
    removed: String,
    inserted: String,
    before: Selection,
    after: Selection,
}

impl Edit {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}

pub struct Editor {
    revision: u64,
    byte_limit: usize,
    limit_error: &'static str,
    text: String,
    inline_tokens: Vec<String>,
    selection: Selection,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    history_bytes: usize,
    layout: Arc<Layout>,
    area: Option<Placement>,
    top: usize,
    preferred_column: Option<usize>,
    single_line: bool,
    masked: bool,
    left: usize,
    dragging: bool,
    // Message IDs, not English strings; errors survive a locale switch.
    pub error: Option<&'static str>,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            revision: 0,
            byte_limit: MAX_TEXT_BYTES,
            limit_error: "composer-too-large",
            text: String::new(),
            inline_tokens: Vec::new(),
            selection: Selection::default(),
            undo: VecDeque::new(),
            redo: Vec::new(),
            history_bytes: 0,
            layout: Arc::new(Layout::new("", 1)),
            area: None,
            top: 0,
            preferred_column: None,
            single_line: false,
            masked: false,
            left: 0,
            dragging: false,
            error: None,
        }
    }
}

impl Editor {
    pub fn bounded(byte_limit: usize, limit_error: &'static str) -> Self {
        Self {
            byte_limit,
            limit_error,
            ..Self::default()
        }
    }
    pub(crate) fn single_line(byte_limit: usize, masked: bool) -> Self {
        Self {
            single_line: true,
            masked,
            ..Self::bounded(byte_limit, "input-too-large")
        }
    }
    pub(crate) fn set_inline_tokens(&mut self, literals: &[String]) {
        if self.inline_tokens != literals {
            self.inline_tokens = literals.to_vec();
        }
    }
    fn inline_token_ranges(&self) -> Vec<Range<usize>> {
        maka_ui_runtime::textarea::inline_token_ranges(&self.text, &self.inline_tokens)
    }
    fn atomic_range(&self, mut range: Range<usize>) -> Range<usize> {
        for token in self.inline_token_ranges() {
            if range.is_empty() {
                if token.start < range.start && range.start < token.end {
                    return token.end..token.end;
                }
            } else if range.start < token.end && token.start < range.end {
                range.start = range.start.min(token.start);
                range.end = range.end.max(token.end);
            }
        }
        range
    }

    fn make_layout(&self, width: u16) -> Layout {
        if self.single_line {
            Layout::single_line(&self.text, width, self.masked)
        } else {
            Layout::new(&self.text, width)
        }
    }
    pub(crate) fn secret(&self) -> bool {
        self.single_line && self.masked
    }
    pub(crate) fn accepts_text(&self, text: &str) -> bool {
        text.len() <= self.byte_limit
            && !text
                .chars()
                .any(|c| c.is_control() && (self.single_line || !matches!(c, '\n' | '\t')))
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn paste_target(&self) -> PasteTarget {
        PasteTarget {
            revision: self.revision,
            selection: self.selection,
        }
    }
    pub(crate) fn paste_at(&mut self, target: PasteTarget, text: &str) -> bool {
        if self.revision != target.revision || text.len() > self.byte_limit {
            return false;
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if !self.accepts_text(&text)
            || self.text.len() - target.selection.range().len() + text.len() > self.byte_limit
        {
            return false;
        }
        self.selection = target.selection;
        self.insert(&text);
        true
    }
    pub(crate) fn selection_range(&self) -> Range<usize> {
        self.selection.range()
    }
    pub(crate) fn selection_reversed(&self) -> bool {
        self.selection
            .anchor
            .is_some_and(|anchor| anchor > self.selection.cursor)
    }
    pub(crate) fn selected_text(&self) -> &str {
        &self.text[self.atomic_range(self.selection.range())]
    }
    pub(crate) fn select_all(&mut self) {
        self.selection.anchor = Some(0);
        self.selection.cursor = self.text.len();
        self.reveal_cursor();
    }
    /// Initialization from validated native UTF-8/grapheme selection, without
    /// restoring undo, pointer state or old presentation geometry.
    pub(crate) fn restore(&mut self, text: &str, anchor: usize, focus: usize) -> bool {
        if !self.accepts_text(text)
            || ![anchor, focus].into_iter().all(|byte| {
                byte == text.len() || text.grapheme_indices(true).any(|(index, _)| index == byte)
            })
        {
            return false;
        }
        self.text = text.to_owned();
        self.revision = self.revision.wrapping_add(1);
        self.selection = Selection {
            cursor: focus,
            anchor: (anchor != focus).then_some(anchor),
            upstream: false,
        };
        self.clear_history();
        self.dragging = false;
        self.preferred_column = None;
        self.top = 0;
        self.left = 0;
        self.error = None;
        self.reflow();
        self.invalidate_geometry();
        true
    }
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.history_bytes = 0;
    }

    /// Rows the text wraps to at `width`, leaving the cached layout alone.
    pub fn rows(&self, width: u16) -> u16 {
        let rows = if self.layout.width == width {
            self.layout.rows.len()
        } else {
            self.make_layout(width).rows.len()
        };
        rows.max(1) as u16
    }

    pub fn invalidate_geometry(&mut self) {
        self.area = None;
        self.dragging = false;
    }

    /// The visible caret in the last drawn editor area, with its current wrap.
    #[cfg(test)]
    pub fn cursor_position(&self) -> Option<Position> {
        let area = self.area.filter(|area| !area.is_empty())?;
        let (row, column) = self
            .layout
            .cursor(self.selection.cursor, self.selection.upstream);
        if row < self.top || row >= self.top + area.height as usize {
            return None;
        }
        area.point(
            column
                .saturating_sub(self.left)
                .min(area.width.saturating_sub(1) as usize),
            row - self.top,
        )
    }

    pub fn cancel_pointer(&mut self) {
        self.dragging = false;
    }

    pub fn dragging(&self) -> bool {
        self.dragging
    }

    fn reflow(&mut self) {
        self.layout = Arc::new(self.make_layout(self.layout.width));
    }

    fn reveal_cursor(&mut self) {
        let Some(area) = self.area else {
            return;
        };
        if self.single_line {
            let (_, column) = self
                .layout
                .cursor(self.selection.cursor, self.selection.upstream);
            if column < self.left {
                self.left = column;
            }
            if column >= self.left + area.width as usize {
                self.left = column.saturating_add(1).saturating_sub(area.width as usize);
            }
            self.top = 0;
            return;
        }
        let (row, _) = self
            .layout
            .cursor(self.selection.cursor, self.selection.upstream);
        if row < self.top {
            self.top = row;
        } else if row >= self.top + area.height as usize {
            self.top = (row + 1).saturating_sub(area.height as usize);
        }
        self.top = self
            .top
            .min(self.layout.rows.len().saturating_sub(area.height as usize));
    }

    /// Bracketed paste is one edit, never interpreted as keys or submitted.
    pub fn insert(&mut self, text: &str) -> bool {
        if self.single_line && text.chars().any(char::is_control) {
            self.error = Some("input-control");
            return true;
        }
        if text.len() > self.byte_limit {
            self.error = Some(self.limit_error);
            return true;
        }
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            self.error = Some("composer-control");
            return true;
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let range = self.atomic_range(self.selection.range());
        if self.text.len() - range.len() + text.len() > self.byte_limit {
            self.error = Some(self.limit_error);
            return true;
        }
        self.replace(range, &text);
        true
    }

    /// Apply an already admitted byte range as one history entry.
    pub(crate) fn replace_range(&mut self, range: Range<usize>, text: &str) {
        self.replace(range, text);
    }

    pub fn clear(&mut self) {
        self.replace(0..self.text.len(), "");
    }

    fn replace(&mut self, range: Range<usize>, inserted: &str) {
        let range = self.atomic_range(range);
        self.error = None;
        if self.text[range.clone()] == *inserted {
            self.selection = Selection {
                cursor: range.end,
                anchor: None,
                upstream: false,
            };
            self.reveal_cursor();
            return;
        }
        self.revision = self.revision.wrapping_add(1);
        let before = self.selection;
        let removed = self.text[range.clone()].to_owned();
        self.text.replace_range(range.clone(), inserted);
        self.reflow();
        self.selection = Selection {
            cursor: self.layout.snap_right(range.start + inserted.len()),
            anchor: None,
            upstream: false,
        };
        self.preferred_column = None;
        for edit in self.redo.drain(..) {
            self.history_bytes -= edit.bytes();
        }
        let edit = Edit {
            start: range.start,
            removed,
            inserted: inserted.to_owned(),
            before,
            after: self.selection,
        };
        self.history_bytes += edit.bytes();
        self.undo.push_back(edit);
        self.trim_history();
        self.reveal_cursor();
    }

    fn trim_history(&mut self) {
        while self.history_bytes > MAX_HISTORY_BYTES || self.undo.len() > MAX_HISTORY_EDITS {
            if let Some(edit) = self.undo.pop_front() {
                self.history_bytes -= edit.bytes();
            }
        }
    }

    fn undo(&mut self, redo: bool) {
        let edit = if redo {
            self.redo.pop()
        } else {
            self.undo.pop_back()
        };
        let Some(edit) = edit else {
            return;
        };
        self.revision = self.revision.wrapping_add(1);
        if redo {
            self.text
                .replace_range(edit.start..edit.start + edit.removed.len(), &edit.inserted);
            self.selection = edit.after;
        } else {
            self.text
                .replace_range(edit.start..edit.start + edit.inserted.len(), &edit.removed);
            self.selection = edit.before;
        }
        self.reflow();
        self.preferred_column = None;
        self.error = None;
        if redo {
            self.undo.push_back(edit);
        } else {
            self.redo.push(edit);
        }
        self.reveal_cursor();
    }

    fn move_to(&mut self, (byte, upstream): (usize, bool), extend: bool) {
        if extend {
            self.selection.anchor.get_or_insert(self.selection.cursor);
        } else {
            self.selection.anchor = None;
        }
        self.selection.cursor = self.layout.snap_right(byte);
        self.selection.upstream = upstream;
        self.error = None;
        self.reveal_cursor();
    }

    fn word_boundary(&self, forward: bool) -> usize {
        if self.single_line && self.masked {
            return if forward { self.text.len() } else { 0 };
        }
        let cursor = self.selection.cursor;
        if forward {
            self.text[cursor..]
                .unicode_word_indices()
                .next()
                .map_or(self.text.len(), |(offset, word)| {
                    cursor + offset + word.len()
                })
        } else {
            self.text[..cursor]
                .unicode_word_indices()
                .next_back()
                .map_or(0, |(offset, _)| offset)
        }
    }

    /// Tab, Escape, application commands and submission stay with the focus scope.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let word = key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        if control {
            match key.code {
                KeyCode::Char('c' | 'C')
                    if !self.single_line
                        && key.modifiers == KeyModifiers::CONTROL
                        && !self.text.is_empty() =>
                {
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        self.clear();
                    }
                    return true;
                }
                KeyCode::Char('j')
                    if !self.single_line && key.modifiers == KeyModifiers::CONTROL =>
                {
                    return self.insert("\n");
                }
                KeyCode::Char('z') => {
                    self.undo(shift);
                    return true;
                }
                KeyCode::Char('y') => {
                    self.undo(true);
                    return true;
                }
                KeyCode::Char('a') => {
                    self.selection.anchor = Some(0);
                    self.selection.cursor = self.text.len();
                    self.selection.upstream = false;
                    self.reveal_cursor();
                    return true;
                }
                _ => {}
            }
        }
        let cursor = self.selection.cursor;
        let range = self.selection.range();
        let mut upstream = false;
        let target = match key.code {
            KeyCode::Left if !shift && !range.is_empty() => Some(range.start),
            KeyCode::Right if !shift && !range.is_empty() => Some(range.end),
            KeyCode::Left => Some(if word {
                self.word_boundary(false)
            } else {
                self.layout.previous(cursor)
            }),
            KeyCode::Right => Some(if word {
                self.word_boundary(true)
            } else {
                self.layout.next(cursor)
            }),
            KeyCode::Home if control => Some(0),
            KeyCode::End if control => Some(self.text.len()),
            KeyCode::Home => {
                Some(self.layout.rows[self.layout.cursor(cursor, self.selection.upstream).0].start)
            }
            KeyCode::End => {
                upstream = true;
                Some(self.layout.rows[self.layout.cursor(cursor, self.selection.upstream).0].end)
            }
            KeyCode::Up | KeyCode::Down if !self.single_line => {
                let (row, column) = self.layout.cursor(cursor, self.selection.upstream);
                let column = *self.preferred_column.get_or_insert(column);
                let row = if key.code == KeyCode::Up {
                    row.saturating_sub(1)
                } else {
                    row + 1
                };
                let (byte, affinity) = self.layout.hit(row, column);
                upstream = affinity;
                Some(byte)
            }
            _ => None,
        };
        if let Some(target) = target {
            if !matches!(key.code, KeyCode::Up | KeyCode::Down) {
                self.preferred_column = None;
            }
            self.move_to((target, upstream), shift);
            return true;
        }
        match key.code {
            KeyCode::Backspace | KeyCode::Delete => {
                let range = if !range.is_empty() {
                    range
                } else if key.code == KeyCode::Backspace {
                    (if word {
                        self.word_boundary(false)
                    } else {
                        self.layout.previous(cursor)
                    })..cursor
                } else {
                    cursor..(if word {
                        self.word_boundary(true)
                    } else {
                        self.layout.next(cursor)
                    })
                };
                self.replace(range, "");
                true
            }
            KeyCode::Enter if !control && !self.single_line => self.insert("\n"),
            KeyCode::Char(c)
                if !c.is_control()
                    && (!key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) || cfg!(windows)
                        && key
                            .modifiers
                            .contains(KeyModifiers::CONTROL | KeyModifiers::ALT)) =>
            {
                self.insert(c.encode_utf8(&mut [0; 4]))
            }
            _ => false,
        }
    }

    fn pointer(&self, point: Position) -> (usize, bool) {
        let area = self.area.expect("presented editor");
        if self.single_line {
            self.layout.hit(
                0,
                self.left
                    + (i64::from(point.x) - area.x)
                        .clamp(0, i64::from(area.width.saturating_sub(1)))
                        as usize,
            )
        } else {
            self.layout.pointer(area, self.top, point)
        }
    }

    pub fn mouse(&mut self, mouse: MouseEvent) -> bool {
        let Some(area) = self.area.filter(|area| !area.is_empty()) else {
            return false;
        };
        let point = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if area.contains(point) => {
                self.dragging = true;
                self.preferred_column = None;
                self.move_to(
                    self.pointer(point),
                    mouse.modifiers.contains(KeyModifiers::SHIFT),
                );
                self.selection.anchor.get_or_insert(self.selection.cursor);
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                if i64::from(point.y) < area.y {
                    self.top = self.top.saturating_sub(1);
                } else if i64::from(point.y) >= area.bottom() {
                    self.top = (self.top + 1)
                        .min(self.layout.rows.len().saturating_sub(area.height as usize));
                }
                self.move_to(self.pointer(point), true);
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                self.dragging = false;
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if area.contains(point) => {
                let next = if mouse.kind == MouseEventKind::ScrollDown {
                    (self.top + 3).min(self.layout.rows.len().saturating_sub(area.height as usize))
                } else {
                    self.top.saturating_sub(3)
                };
                if next == self.top {
                    // Keep an overflowing editor's wheel gesture at its edge,
                    // matching native editor behavior. Text that fits still yields to the page.
                    return !self.single_line && self.layout.rows.len() > area.height as usize;
                }
                self.top = next;
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_literals_delete_atomically_from_edges_inside_and_partial_selection() {
        let original = "🙂 /skill:技能 suffix";
        let start = "🙂 ".len();
        let end = start + "/skill:技能".len();
        for (cursor, anchor, code) in [
            (end, None, KeyCode::Backspace),
            (start, None, KeyCode::Delete),
            (start + 2, None, KeyCode::Delete),
            (start + 5, Some(start + 2), KeyCode::Backspace),
        ] {
            let mut editor = Editor::default();
            editor.set_inline_tokens(&["/skill:技能".into()]);
            editor.insert(original);
            editor.clear_history();
            editor.selection = Selection {
                cursor,
                anchor,
                upstream: false,
            };
            editor.key(KeyEvent::new(code, KeyModifiers::NONE));
            assert_eq!(editor.text(), "🙂  suffix");
            editor.undo(false);
            assert_eq!(editor.text(), original);
            assert!(editor.undo.is_empty());
        }
        let mut editor = Editor::default();
        editor.set_inline_tokens(&["/skill:foo".into()]);
        editor.insert("/skill:foo-extra");
        editor.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(editor.text(), "/skill:foo-extr");
    }

    use ratatui::{
        Frame, Terminal,
        backend::TestBackend,
        layout::Rect,
        style::{Modifier, Style},
    };

    fn draw(editor: &mut Editor, frame: &mut Frame<'_>, area: Rect, focused: bool) {
        let geometry = editor.paint_prepared(
            area.into(),
            frame.buffer_mut(),
            focused,
            Style::default(),
            Style::default().add_modifier(Modifier::REVERSED),
            Style::default().add_modifier(Modifier::REVERSED),
        );
        editor.present_geometry(geometry);
        if focused && let Some(position) = editor.cursor_position() {
            frame.set_cursor_position(position);
        }
    }

    fn key(editor: &mut Editor, code: KeyCode, modifiers: KeyModifiers) {
        assert!(editor.key(KeyEvent::new(code, modifiers)));
        assert!(editor.layout.boundaries.contains(&editor.selection.cursor));
    }

    #[test]
    fn grapheme_deletion_selection_and_undo_preserve_unicode() {
        let mut editor = Editor::default();
        for grapheme in ["e\u{301}", "👩🏽‍💻", "🇨🇳", "क्\u{200d}ष"] {
            editor.insert(grapheme);
            key(&mut editor, KeyCode::Backspace, KeyModifiers::NONE);
            assert_eq!(editor.text(), "");
            key(&mut editor, KeyCode::Char('z'), KeyModifiers::CONTROL);
            assert_eq!(editor.text(), grapheme);
            key(&mut editor, KeyCode::Left, KeyModifiers::SHIFT);
            assert_eq!(editor.selection.range(), 0..grapheme.len());
            editor.insert("中文");
            key(&mut editor, KeyCode::Char('z'), KeyModifiers::CONTROL);
            assert_eq!(editor.text(), grapheme);
            key(&mut editor, KeyCode::Char('y'), KeyModifiers::CONTROL);
            assert_eq!(editor.text(), "中文");
            key(&mut editor, KeyCode::Char('a'), KeyModifiers::CONTROL);
            editor.insert("");
        }
        editor.insert("e");
        editor.insert("\u{301}");
        assert_eq!(editor.selection.cursor, "e\u{301}".len());
        key(&mut editor, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(editor.text(), "");
    }

    #[test]
    fn wrap_mouse_drag_resize_and_vertical_motion_share_cell_geometry() {
        let mut editor = Editor::default();
        editor.insert("ab中👩🏽‍💻\nxy\t末");
        let mut terminal = Terminal::new(TestBackend::new(12, 8)).unwrap();
        let area = Rect::new(2, 1, 7, 4);
        terminal
            .draw(|frame| draw(&mut editor, frame, area, true))
            .unwrap();
        // ab(2) + 中(2) + emoji(2) fit, then explicit newline.
        assert_eq!(editor.layout.rows.len(), 2); // tab reaches column 4, 末 fits.
        let mouse = |kind, x, y| MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        };
        editor.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 1));
        assert_eq!(editor.selection.cursor, 2); // middle of CJK snaps left
        editor.mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 8, 1));
        assert_eq!(&editor.text[editor.selection.range()], "中👩🏽‍💻");
        editor.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 11, 7));
        assert!(!editor.dragging());
        editor.insert("好");
        assert_eq!(editor.text(), "ab好\nxy\t末");
        key(&mut editor, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(editor.text(), "ab中👩🏽‍💻\nxy\t末");
        editor.invalidate_geometry();
        assert!(!editor.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 1)));
        terminal
            .draw(|frame| draw(&mut editor, frame, Rect::new(2, 1, 5, 4), true))
            .unwrap();
        assert_eq!(editor.layout.rows[1].start, "ab中".len());
        editor.selection = Selection::default();
        key(&mut editor, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(editor.selection.cursor, "ab中".len());
        key(&mut editor, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(editor.selection.cursor, 0);
        key(&mut editor, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(
            editor
                .layout
                .cursor(editor.selection.cursor, editor.selection.upstream),
            (0, 4)
        );
        key(&mut editor, KeyCode::Home, KeyModifiers::NONE);
        assert_eq!(
            editor.selection.cursor, 0,
            "End at a wrap must stay on the same visual row"
        );
        editor.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 6, 1));
        assert_eq!(
            editor
                .layout
                .cursor(editor.selection.cursor, editor.selection.upstream),
            (0, 4)
        );
    }

    #[test]
    fn paste_is_atomic_bounded_and_controls_never_reach_the_terminal() {
        let mut editor = Editor::default();
        let paste = "中文👩🏽‍💻\tcode\r\n".repeat(4096);
        assert!(paste.len() >= 100 * 1024);
        editor.insert(&paste);
        assert_eq!(editor.text(), paste.replace("\r\n", "\n"));
        assert_eq!(editor.undo.len(), 1);
        key(&mut editor, KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert_eq!(editor.text(), "");
        key(&mut editor, KeyCode::Char('y'), KeyModifiers::CONTROL);
        let before = editor.text.clone();
        editor.insert("\u{1b}[2J");
        assert_eq!(editor.text(), before);
        assert_eq!(editor.error, Some("composer-control"));
        editor.insert(&"a".repeat(MAX_TEXT_BYTES + 1));
        assert_eq!(editor.text(), before);
        assert_eq!(editor.error, Some("composer-too-large"));
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut samples = Vec::new();
        for _ in 0..140 {
            let start = std::time::Instant::now();
            editor.insert("!");
            terminal
                .draw(|frame| draw(&mut editor, frame, Rect::new(2, 28, 116, 8), true))
                .unwrap();
            samples.push(start.elapsed());
        }
        samples.sort_unstable();
        eprintln!(
            "100+ KiB editor insert+TestBackend draw: p50={:?}, p95={:?}; debug_assertions={}",
            samples[70],
            samples[133],
            cfg!(debug_assertions)
        );
        assert!(editor.history_bytes <= MAX_HISTORY_BYTES);
        assert!(editor.undo.len() <= MAX_HISTORY_EDITS);
        while !editor.undo.is_empty() {
            editor.undo(false);
        }
        while !editor.redo.is_empty() {
            editor.undo(true);
        }
        assert!(editor.text().ends_with(&"!".repeat(140)));
    }
}
