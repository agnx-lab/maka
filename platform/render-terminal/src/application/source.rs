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

//! Literal source, prepared Markdown and Diff share the original native reading owner.
use super::{NativeInput, Placement, Slots, Widget, WidgetInput};
use crate::editor::layout::Layout;
use crossterm::event::Event;
use maka_ui_runtime::{
    ComponentCall, ComponentOutput, Frame, NativeCapability, NativeComponent, NativeExport,
    NativeRegistry, NativeView, source::*,
};
use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier, Style},
};
use serde_json::json;
use std::{any::Any, cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};
use unicode_width::UnicodeWidthStr;

use super::selection::{Document, DocumentRef, Glyphs, SelectionScope};
mod diff;
pub(super) mod reveal;
struct Geometry {
    glyphs: Glyphs,
    parent: Option<SelectionScope>,
}
pub(super) struct Owner {
    reveal: reveal::Reveal,
    pub document: DocumentRef,
}
impl Owner {
    fn apply(&mut self, recipe: &Recipe) {
        let props = &recipe.0;
        let previous = self.document.borrow().source.clone();
        if props.animate && props.source.starts_with(previous.as_ref()) {
            self.reveal.append(
                previous.len()..props.source.len(),
                Some(std::time::Instant::now()),
            );
        } else {
            self.reveal.clear();
        }
        let format_changed = match (&self.document.borrow().markdown, &recipe.1) {
            (Some(old), Some(new)) => !Arc::ptr_eq(old, new),
            (None, None) => false,
            _ => true,
        };
        let changed = {
            let doc = self.document.borrow();
            !props.source.starts_with(doc.source.as_ref()) || props.disabled || !recipe.2
        };
        if let Some(scope) = SelectionScope::active(&self.document) {
            if self.document.borrow().source.as_ref() != props.source
                || props.disabled
                || !recipe.2
                || format_changed
            {
                scope.release_pointer(&self.document);
            }
            if changed {
                scope.withdraw(&self.document);
            }
        }
        let mut doc = self.document.borrow_mut();
        if doc.source.as_ref() != props.source || format_changed {
            doc.geometry_revision += 1;
        }
        if changed || format_changed {
            doc.glyphs = None;
        }
        if doc.source.as_ref() != props.source {
            doc.source = Arc::from(props.source.as_str());
        }
        doc.disabled = props.disabled;
        doc.markdown = recipe.1.clone();
    }
}
struct CachedLayout {
    width: u16,
    layout: super::selection::Layout,
    size: (u16, u32),
    used: bool,
}
#[derive(Default)]
struct LayoutCache {
    frame: Option<std::time::Instant>,
    layouts: Vec<CachedLayout>,
}
struct Recipe(
    SourceTextProps,
    Option<Arc<crate::markdown::prepared::Document>>,
    bool,
    RefCell<LayoutCache>,
);
fn color(rgba: u32) -> Color {
    Color::Rgb((rgba >> 24) as u8, (rgba >> 16) as u8, (rgba >> 8) as u8)
}
impl Widget for Recipe {
    fn create(&self, _: super::NativeContext) -> Box<dyn Any> {
        Box::new(Owner {
            reveal: reveal::Reveal::default(),
            document: Rc::new(std::cell::RefCell::new(Document {
                source: Arc::from(self.0.source.as_str()),
                geometry_revision: 0,
                markdown: self.1.clone(),
                disabled: self.0.disabled,
                alive: true,
                physical: false,
                glyphs: None,
                binding: std::rc::Weak::new(),
                standalone: None,
            })),
        })
    }
    fn commit(&self, state: &mut dyn Any) -> maka_ui_runtime::Result<Option<NativeInput>> {
        state.downcast_mut::<Owner>().unwrap().apply(self);
        Ok(None)
    }
    fn input(
        &self,
        state: &mut dyn Any,
        _: &str,
        event: &Event,
    ) -> maka_ui_runtime::Result<WidgetInput> {
        let owner = state.downcast_mut::<Owner>().unwrap();
        let enabled =
            self.2 && !self.0.disabled && self.0.source == owner.document.borrow().source.as_ref();
        let handled = enabled
            && SelectionScope::active(&owner.document)
                .is_some_and(|scope| scope.input(&owner.document, event));
        Ok(
            if handled && !self.0.focus_on_pointer && matches!(event, Event::Mouse(_)) {
                WidgetInput::PreserveFocus
            } else if handled {
                WidgetInput::Consumed(None)
            } else {
                WidgetInput::Unhandled
            },
        )
    }
    fn cancel_pointer(&self, state: &mut dyn Any) {
        let owner = state.downcast_mut::<Owner>().unwrap();
        if let Some(scope) = SelectionScope::active(&owner.document) {
            scope.release_pointer(&owner.document);
        }
    }
    fn pointer_capture(&self, state: &dyn Any) -> bool {
        let owner = state.downcast_ref::<Owner>().unwrap();
        let doc = owner.document.borrow();
        self.2
            && !self.0.disabled
            && doc.alive
            && doc.physical
            && self.0.source == doc.source.as_ref()
            && doc
                .glyphs
                .as_ref()
                .is_some_and(|glyphs| glyphs.source.as_ref() == doc.source.as_ref())
            && SelectionScope::active(&owner.document)
                .is_some_and(|scope| scope.pointer_capture(&owner.document))
    }
    fn copy(&self, state: &dyn Any, _: bool) -> maka_ui_runtime::Result<Option<String>> {
        let owner = state.downcast_ref::<Owner>().unwrap();
        let doc = owner.document.borrow();
        if !self.2
            || self.0.disabled
            || doc.disabled
            || !doc.alive
            || !self.0.source.starts_with(doc.source.as_ref())
        {
            return Ok(None);
        }
        match SelectionScope::active(&owner.document) {
            Some(scope) => scope.copy(),
            None => Ok(None),
        }
    }
    fn presentation(&self, state: &mut dyn Any, visible: bool) {
        let owner = state.downcast_mut::<Owner>().unwrap();
        if !visible && let Some(scope) = SelectionScope::active(&owner.document) {
            scope.release_pointer(&owner.document);
        }
        let mut doc = owner.document.borrow_mut();
        doc.physical = visible;
        if !visible {
            doc.glyphs = None;
        }
    }
    fn deactivate(&self, state: &mut dyn Any) {
        let owner = state.downcast_mut::<Owner>().unwrap();
        if let Some(scope) = SelectionScope::active(&owner.document) {
            scope.withdraw(&owner.document);
        }
        let mut doc = owner.document.borrow_mut();
        doc.alive = false;
        doc.physical = false;
        doc.glyphs = None;
        doc.standalone = None;
    }
    fn measure(&self, slots: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        Ok(self.layout(width, slots.measurements.time())?.1)
    }
    fn paint(
        &self,
        slots: &mut Slots<'_>,
        area: Placement,
        buffer: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        if self.1.is_some() {
            return self.paint_markdown(slots, area, buffer);
        }
        let document = slots.native_state::<Owner>()?.document.clone();
        let parent = if self.2 && !self.0.disabled {
            slots.participant(&document, area)
        } else {
            None
        };
        let reveal = slots
            .scene
            .as_ref()
            .and_then(|scene| scene.scope.as_ref())
            .and_then(|scope| scope.reveal.as_ref())
            .filter(|reveal| reveal.targets(&document));
        if area.width == 0 || (area.is_empty() && reveal.is_none()) {
            return Ok(());
        }
        let doc = document.borrow();
        let scope = parent.clone().or_else(|| SelectionScope::active(&document));
        let (layout, _) = self.layout(area.width, slots.measurements.time())?;
        let super::selection::Layout::Literal(literal_layout) = &layout else {
            unreachable!()
        };
        if let Some(offset) = reveal.map(|reveal| reveal.offset) {
            let row = area.y + layout.cursor(offset, false).0 as i64;
            let visible = area.visible();
            let reveal = slots
                .scene
                .as_mut()
                .unwrap()
                .scope
                .as_mut()
                .unwrap()
                .reveal
                .as_mut()
                .unwrap();
            reveal.row = Some(row);
            reveal.visible = row >= i64::from(visible.y) && row < i64::from(visible.bottom());
        }
        if area.is_empty() {
            return Ok(());
        }
        let highlights = slots
            .scene
            .as_ref()
            .and_then(|scene| scene.scope.as_ref())
            .and_then(|scope| scope.find.as_ref())
            .map(|find| find.highlights(&document))
            .unwrap_or_default();
        let range = scope.as_ref().map_or(0..0, |scope| scope.range(&document));
        let terminal = self.0.terminal.as_ref();
        let mut normal = Style::default().fg(terminal.map_or_else(
            || color(self.0.appearance.foreground.0),
            |p| super::paint::color(p.foreground),
        ));
        if self.0.appearance.font.weight >= 600 {
            normal = normal.add_modifier(Modifier::BOLD);
        }
        let selected = terminal.map_or_else(
            || normal.bg(color(self.0.selection.0)),
            |p| {
                normal
                    .bg(super::paint::color(p.selection))
                    .fg(super::paint::color(p.selection_text))
            },
        );
        let visible = area.visible();
        let first = (i64::from(visible.y) - area.y).max(0) as usize;
        for (row_index, row) in literal_layout
            .rows
            .iter()
            .enumerate()
            .skip(first)
            .take(usize::from(visible.height))
        {
            for cell in &row.cells {
                let Some(point) = area.point(cell.column, row_index) else {
                    continue;
                };
                if area
                    .point(cell.column + cell.width - 1, row_index)
                    .is_none()
                {
                    continue;
                }
                let literal = &doc.source[cell.bytes.clone()];
                let style = if cell.bytes.start < range.end && cell.bytes.end > range.start {
                    selected
                } else if let Some((_, active)) = highlights
                    .get(highlights.partition_point(|(range, _)| range.end <= cell.bytes.start))
                    .filter(|(range, _)| range.start < cell.bytes.end)
                {
                    if let Some(p) = terminal {
                        let style = normal.bg(super::paint::color(if *active {
                            p.search_active
                        } else {
                            p.search
                        }));
                        if *active {
                            style
                                .fg(super::paint::color(p.search_text))
                                .add_modifier(Modifier::BOLD)
                        } else {
                            style
                        }
                    } else {
                        let style = normal.add_modifier(Modifier::UNDERLINED);
                        if *active {
                            style.add_modifier(Modifier::REVERSED | Modifier::BOLD)
                        } else {
                            style
                        }
                    }
                } else {
                    normal
                };
                if literal == "\t" {
                    for column in 0..cell.width {
                        buffer[(point.x + column as u16, point.y)]
                            .set_symbol(" ")
                            .set_style(style);
                    }
                } else {
                    let symbol =
                        if literal.chars().any(char::is_control) || literal.width() > cell.width {
                            "�".to_owned()
                        } else if literal.width() == 0 {
                            format!("◌{literal}")
                        } else {
                            literal.to_owned()
                        };
                    buffer.set_stringn(point.x, point.y, symbol, cell.width, style);
                }
            }
            let break_end = literal_layout
                .rows
                .get(row_index + 1)
                .map_or(row.end, |next| next.start);
            if row.end < range.end && break_end > range.start && break_end > row.end {
                let column = row.cells.last().map_or(0, |cell| cell.column + cell.width);
                if let Some(point) = area.point(
                    column.min(usize::from(area.width.saturating_sub(1))),
                    row_index,
                ) {
                    buffer[(point.x, point.y)].set_style(selected);
                }
            }
        }
        if slots.focused() && range.is_empty() {
            let (byte, trailing) = scope
                .as_ref()
                .map_or((0, false), |scope| scope.cursor(&document));
            let (row, column) = layout.cursor(byte, trailing);
            if let Some(point) = area.point(column, row) {
                buffer[(point.x, point.y)].set_style(normal.add_modifier(Modifier::UNDERLINED));
            }
        }
        let geometry = Geometry {
            glyphs: Glyphs {
                source: doc.source.clone(),
                layout,
                area,
            },
            parent,
        };
        slots.native_geometry(Box::new(geometry))?;
        if self.2 && !self.0.disabled {
            slots.native_target("source", area, 0, false)?;
        }
        Ok(())
    }
    fn present(&self, state: &mut dyn Any, geometry: Box<dyn Any>) -> Option<NativeInput> {
        let owner = state.downcast_mut::<Owner>().unwrap();
        let geometry = *geometry.downcast::<Geometry>().unwrap();
        if self.2
            && !owner.document.borrow().disabled
            && owner
                .document
                .borrow()
                .source
                .starts_with(geometry.glyphs.source.as_ref())
        {
            let area = geometry.glyphs.area;
            owner.document.borrow_mut().glyphs = Some(geometry.glyphs);
            if geometry.parent.is_none() {
                SelectionScope::standalone(&owner.document, area);
            }
        }
        None
    }
}
impl Recipe {
    fn layout(
        &self,
        width: u16,
        frame: std::time::Instant,
    ) -> maka_ui_runtime::Result<(super::selection::Layout, (u16, u32))> {
        let mut cached = self.3.borrow_mut();
        if cached.frame != Some(frame) {
            // Retain the widths used by the preceding paint pass. A scroll bar,
            // intrinsic sizing and nested wrappers can all probe different widths.
            // Widths abandoned after resize disappear on the next pass. This
            // derived working set cannot consume native resource admission.
            cached
                .layouts
                .retain_mut(|entry| std::mem::replace(&mut entry.used, false));
            cached.frame = Some(frame);
        }
        if let Some(entry) = cached.layouts.iter_mut().find(|entry| entry.width == width) {
            entry.used = true;
            return Ok((entry.layout.clone(), entry.size));
        }
        let layout = match &self.1 {
            Some(document) => super::selection::Layout::markdown(document, width)?,
            None => Layout::literal(&self.0.source, width).into(),
        };
        let size = Self::measured(&layout, width);
        cached.layouts.push(CachedLayout {
            width,
            layout: layout.clone(),
            size,
            used: true,
        });
        Ok((layout, size))
    }

    fn measured(layout: &super::selection::Layout, width: u16) -> (u16, u32) {
        match layout {
            super::selection::Layout::Markdown { lines, .. } => (
                lines
                    .iter()
                    .map(|line| line.line.width())
                    .max()
                    .unwrap_or(0)
                    .min(usize::from(width)) as u16,
                lines.len() as u32,
            ),
            super::selection::Layout::Literal(layout) => (
                layout
                    .rows
                    .iter()
                    .map(|row| row.cells.last().map_or(0, |cell| cell.column + cell.width))
                    .max()
                    .unwrap_or(0)
                    .min(usize::from(width)) as u16,
                layout.rows.len() as u32,
            ),
        }
    }

    fn paint_markdown(
        &self,
        slots: &mut Slots<'_>,
        area: Placement,
        buffer: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        use unicode_segmentation::UnicodeSegmentation;
        let fade = slots.native_state::<Owner>()?.reveal.clone();
        let time =
            (self.0.animate && slots.presenter.window_focused).then_some(slots.measurements.time());
        let document = slots.native_state::<Owner>()?.document.clone();
        let parent = if self.2 && !self.0.disabled {
            slots.participant(&document, area)
        } else {
            None
        };
        let reveal = slots
            .scene
            .as_ref()
            .and_then(|scene| scene.scope.as_ref())
            .and_then(|scope| scope.reveal.as_ref())
            .filter(|reveal| reveal.targets(&document))
            .map(|reveal| reveal.offset);
        if area.width == 0 || area.is_empty() && reveal.is_none() {
            return Ok(());
        }
        let (layout, _) = self.layout(area.width, slots.measurements.time())?;
        if let Some(offset) = reveal {
            let row = area.y + layout.cursor(offset, false).0 as i64;
            let visible = area.visible();
            let reveal = slots
                .scene
                .as_mut()
                .unwrap()
                .scope
                .as_mut()
                .unwrap()
                .reveal
                .as_mut()
                .unwrap();
            reveal.row = Some(row);
            reveal.visible = row >= i64::from(visible.y) && row < i64::from(visible.bottom());
        }
        if area.is_empty() {
            return Ok(());
        }
        let highlights = slots
            .scene
            .as_ref()
            .and_then(|scene| scene.scope.as_ref())
            .and_then(|scope| scope.find.as_ref())
            .map(|find| find.highlights(&document))
            .unwrap_or_default();
        let scope = parent.clone().or_else(|| SelectionScope::active(&document));
        let range = scope.as_ref().map_or(0..0, |scope| scope.range(&document));
        let terminal = self.0.terminal.as_ref();
        let mut normal = Style::default().fg(terminal.map_or_else(
            || color(self.0.appearance.foreground.0),
            |p| super::paint::color(p.foreground),
        ));
        if self.0.appearance.font.weight >= 600 {
            normal = normal.add_modifier(Modifier::BOLD);
        }
        let super::selection::Layout::Markdown { lines, .. } = &layout else {
            unreachable!()
        };
        let visible = area.visible();
        let first = (i64::from(visible.y) - area.y).max(0) as usize;
        for (row, visual) in lines
            .iter()
            .enumerate()
            .skip(first)
            .take(usize::from(visible.height))
        {
            let mut revealed = visual.clone();
            if let Some(palette) = &self.0.terminal
                && let Some(delay) =
                    fade.paint(visual, &mut revealed.line, time, terminal_palette(palette))
            {
                slots.measurements.repaint_after(delay);
            }
            let visual = &revealed;
            let text = visual.line.to_string();
            let mut column = 0;
            let mut styles = Vec::with_capacity(visual.line.spans.len());
            let mut start = 0;
            for span in &visual.line.spans {
                styles.push((start..start + span.content.len(), span.style));
                start += span.content.len();
            }
            for (byte, glyph) in text.grapheme_indices(true) {
                let width = glyph.width().max(1);
                let mut logical: Option<std::ops::Range<usize>> = None;
                for span in &visual.mapping {
                    let start = byte.max(span.display.start);
                    let end = (byte + glyph.len()).min(span.display.end);
                    if start >= end {
                        continue;
                    }
                    let part = if span.display.len() == span.logical.len() {
                        span.logical.start + start - span.display.start
                            ..span.logical.start + end - span.display.start
                    } else {
                        span.logical.clone()
                    };
                    logical = Some(logical.map_or(part.clone(), |old| {
                        old.start.min(part.start)..old.end.max(part.end)
                    }));
                }
                let mut style = normal.patch(visual.line.style);
                for (span, part) in &styles {
                    if span.start < byte + glyph.len() && span.end > byte {
                        style = style.patch(*part);
                    }
                }
                if let Some(logical) = logical {
                    if logical.start < range.end && logical.end > range.start {
                        style = if let Some(p) = terminal {
                            style
                                .bg(super::paint::color(p.selection))
                                .fg(super::paint::color(p.selection_text))
                        } else {
                            style.bg(color(self.0.selection.0))
                        };
                    } else if let Some((_, active)) = highlights
                        .get(highlights.partition_point(|(hit, _)| hit.end <= logical.start))
                        .filter(|(hit, _)| hit.start < logical.end)
                    {
                        if let Some(p) = terminal {
                            style = style.bg(super::paint::color(if *active {
                                p.search_active
                            } else {
                                p.search
                            }));
                            if *active {
                                style = style
                                    .fg(super::paint::color(p.search_text))
                                    .add_modifier(Modifier::BOLD);
                            }
                        } else {
                            style = style.add_modifier(Modifier::UNDERLINED);
                            if *active {
                                style = style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
                            }
                        }
                    }
                }
                if let Some(point) = area.point(column, row)
                    && area.point(column + width - 1, row).is_some()
                {
                    if glyph.width() == 0 {
                        buffer.set_stringn(point.x, point.y, format!("◌{glyph}"), width, style);
                    } else {
                        buffer.set_stringn(point.x, point.y, glyph, width, style);
                    }
                }
                column += width;
            }
        }
        if slots.focused() && range.is_empty() {
            let (byte, trailing) = scope
                .as_ref()
                .map_or((0, false), |scope| scope.cursor(&document));
            let (row, column) = layout.cursor(byte, trailing);
            if let Some(point) = area.point(column, row) {
                buffer[(point.x, point.y)].set_style(normal.add_modifier(Modifier::UNDERLINED));
            }
        }
        slots.native_geometry(Box::new(Geometry {
            glyphs: Glyphs {
                source: document.borrow().source.clone(),
                layout,
                area,
            },
            parent,
        }))?;
        if self.2 && !self.0.disabled {
            slots.native_target("source", area, 0, false)?;
        }
        Ok(())
    }
}
fn terminal_palette(p: &maka_ui_runtime::scene::terminal::Palette) -> crate::markdown::Palette {
    let mut colors = crate::markdown::Palette::default();
    let c = super::paint::color;
    colors.background = c(p.background);
    colors.foreground = c(p.foreground);
    colors.surface = c(p.surface);
    colors.accent = c(p.accent);
    colors.warning = c(p.warning);
    colors.muted = c(p.muted);
    colors.subtle = c(p.subtle);
    colors.border = c(p.border);
    colors.success = c(p.success);
    colors.error = c(p.error);
    colors.syntax = p.syntax.map(c);
    colors.terminal = p.terminal;
    colors.headings = [
        colors.syntax[5],
        colors.accent,
        colors.syntax[0],
        colors.muted,
        colors.muted,
        colors.subtle,
    ]
    .map(|color| Style::default().fg(color).add_modifier(Modifier::BOLD));
    colors.headings[5] = Style::default().fg(colors.subtle);
    colors
}
fn markdown_palette(a: &maka_ui_runtime::markdown::MarkdownAppearance) -> crate::markdown::Palette {
    let mut colors = crate::markdown::Palette::default();
    if let Some(p) = &a.terminal {
        return terminal_palette(p);
    }
    if !a.dark {
        colors.syntax = [
            0x804caf, 0x246c48, 0x5e6c82, 0x985812, 0x205fc1, 0x147078, 0x4c5c72,
        ]
        .map(|rgb| color((rgb << 8) | 255));
    }
    colors.foreground = color(a.code.foreground.0);
    colors.background = color(a.code_background.0);
    colors.surface = colors.background;
    colors.accent = color(a.link.0);
    colors.muted = color(a.muted.0);
    colors.subtle = colors.muted;
    colors.border = color(a.border.0);
    colors.warning = color(a.code.foreground.0);
    colors.headings = a.headings.clone().map(|heading| {
        let style = Style::default().fg(color(heading.foreground.0));
        if heading.font.weight >= 600 {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    });
    colors
}
fn prepare_markdown(
    props: &serde_json::Value,
) -> maka_ui_runtime::Result<maka_ui_runtime::PreparedScene> {
    let props: maka_ui_runtime::markdown::MarkdownProps = serde_json::from_value(props.clone())
        .map_err(|e| maka_ui_runtime::Error::Invalid(e.to_string()))?;
    if props.source.len() > 65536 {
        return Err(maka_ui_runtime::Error::Invalid(
            "Markdown source byte budget".into(),
        ));
    }
    let document = Arc::new(
        crate::markdown::prepared::Document::markdown(
            &props.source,
            props.ascii,
            markdown_palette(&props.appearance),
        )
        .map_err(|e| maka_ui_runtime::Error::Invalid(e.into()))?,
    );
    if document.text().len() > 65536 {
        return Err(maka_ui_runtime::Error::Invalid(
            "Markdown readable text byte budget".into(),
        ));
    }
    let source = SourceTextProps {
        animate: props.animate,
        terminal: props.appearance.terminal.clone(),
        source: document.text().to_owned(),
        appearance: props.appearance.body,
        selection: props.appearance.selection,
        disabled: props.disabled,
        focus_on_pointer: props.focus_on_pointer,
    };
    maka_ui_runtime::PreparedScene::new(
        Recipe(source, Some(document), props.selectable, RefCell::default()),
        BTreeMap::new(),
        vec![],
    )
}

fn prepare_diff(
    props: &serde_json::Value,
) -> maka_ui_runtime::Result<maka_ui_runtime::PreparedScene> {
    use maka_ui_runtime::diff::DiffProps;
    let props: DiffProps = serde_json::from_value(props.clone())
        .map_err(|e| maka_ui_runtime::Error::Invalid(e.to_string()))?;
    let (text, rows) = diff::prepare(&props.content)?;
    let a = &props.appearance;
    let mut colors = crate::markdown::Palette::default();
    if !a.dark {
        colors.syntax = [
            0x804caf, 0x246c48, 0x5e6c82, 0x985812, 0x205fc1, 0x147078, 0x4c5c72,
        ]
        .map(|rgb| color((rgb << 8) | 255));
    }
    colors.foreground = color(a.text.foreground.0);
    colors.background = color(a.background.0);
    colors.surface = colors.background;
    colors.success = color(a.added.0);
    colors.error = color(a.removed.0);
    if let Some(p) = &a.terminal {
        colors = terminal_palette(p);
    }
    let document = Arc::new(
        crate::markdown::prepared::Document::diff(&text, &rows, props.ascii, colors)
            .map_err(|e| maka_ui_runtime::Error::Invalid(e.into()))?,
    );
    if document.text().len() > 65536 {
        return Err(maka_ui_runtime::Error::Invalid(
            "Diff readable text byte budget".into(),
        ));
    }
    let source = SourceTextProps {
        animate: false,
        terminal: props.appearance.terminal.clone(),
        source: document.text().to_owned(),
        appearance: props.appearance.text,
        selection: props.appearance.selection,
        disabled: props.disabled,
        focus_on_pointer: props.focus_on_pointer,
    };
    maka_ui_runtime::PreparedScene::new(
        Recipe(source, Some(document), true, RefCell::default()),
        BTreeMap::new(),
        vec![],
    )
}

fn finish(
    prepared: maka_ui_runtime::PreparedScene,
) -> maka_ui_runtime::Result<NativeView<dyn Widget>> {
    prepared.finish::<Recipe, dyn Widget>(|recipe| Rc::new(recipe))
}
#[cfg(test)]
pub(super) fn compile_markdown(
    props: &serde_json::Value,
) -> maka_ui_runtime::Result<NativeView<dyn Widget>> {
    finish(prepare_markdown(props)?)
}
#[cfg(test)]
pub(super) fn compile_diff(
    props: &serde_json::Value,
) -> maka_ui_runtime::Result<NativeView<dyn Widget>> {
    finish(prepare_diff(props)?)
}

type Prepare = fn(&serde_json::Value) -> maka_ui_runtime::Result<maka_ui_runtime::PreparedScene>;
struct Component(Prepare);
fn call_data(call: ComponentCall<'_>) -> maka_ui_runtime::Result<(&serde_json::Value, u64)> {
    match call {
        ComponentCall::Mount { props } => Ok((props, 0)),
        ComponentCall::Update { props, state } => {
            Ok((props, state["revision"].as_u64().unwrap() + 1))
        }
        ComponentCall::Event { .. } => Err(maka_ui_runtime::Error::Invalid(
            "SourceText has no component events".into(),
        )),
    }
}
impl NativeComponent<dyn Widget> for Component {
    fn call(
        &self,
        call: ComponentCall<'_>,
        _: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<dyn Widget>>> {
        let (props, revision) = call_data(call)?;
        Ok(Frame {
            revision,
            state: json!({"revision": revision}),
            surface: None,
            view: finish((self.0)(props)?)?,
        }
        .output(vec![]))
    }
    fn start(
        &self,
        call: ComponentCall<'_>,
        _: Option<&NativeView<dyn Widget>>,
        _: &maka_ui_runtime::abi::ValueBindings,
        scheduler: &tokio::runtime::Handle,
    ) -> maka_ui_runtime::backend::Job<ComponentOutput<NativeView<dyn Widget>>> {
        let (props, revision) = match call_data(call) {
            Ok((props, revision)) => (props.clone(), revision),
            Err(error) => return maka_ui_runtime::backend::Job::ready(Err(error)),
        };
        let prepare = self.0;
        let work = scheduler.spawn_blocking(move || prepare(&props));
        maka_ui_runtime::backend::Job::new(async move {
            let prepared = work
                .await
                .map_err(|error| maka_ui_runtime::Error::Invalid(error.to_string()))??;
            Ok(Frame {
                revision,
                state: json!({"revision": revision}),
                surface: None,
                view: finish(prepared)?,
            }
            .output(vec![]))
        })
    }
}
fn prepare_source(
    props: &serde_json::Value,
) -> maka_ui_runtime::Result<maka_ui_runtime::PreparedScene> {
    let props: SourceTextProps = serde_json::from_value(props.clone())
        .map_err(|error| maka_ui_runtime::Error::Invalid(error.to_string()))?;
    if props.source.len() > 65536 {
        return Err(maka_ui_runtime::Error::Invalid("source byte budget".into()));
    }
    maka_ui_runtime::PreparedScene::new(
        Recipe(props, None, true, RefCell::default()),
        BTreeMap::new(),
        vec![],
    )
}
#[cfg(test)]
pub(super) fn compile(
    props: &serde_json::Value,
) -> maka_ui_runtime::Result<NativeView<dyn Widget>> {
    finish(prepare_source(props)?)
}
pub(super) fn register(registry: &mut NativeRegistry<dyn Widget>) {
    registry
        .register(maka_ui_runtime::diff::IMPLEMENTATION, |_, _| {
            Ok(BTreeMap::from([(
                "Diff".into(),
                NativeExport {
                    contract: maka_ui_runtime::diff::diff_contract(),
                    value: NativeCapability::Component(Rc::new(Component(prepare_diff))),
                },
            )]))
        })
        .expect("unique Diff factory");
    registry
        .register(maka_ui_runtime::markdown::IMPLEMENTATION, |_, _| {
            Ok(BTreeMap::from([(
                "Markdown".into(),
                NativeExport {
                    contract: maka_ui_runtime::markdown::markdown_contract(),
                    value: NativeCapability::Component(Rc::new(Component(prepare_markdown))),
                },
            )]))
        })
        .expect("unique Markdown factory");
    registry
        .register(maka_ui_runtime::source::IMPLEMENTATION, |_, _| {
            Ok(BTreeMap::from([(
                "SourceText".into(),
                NativeExport {
                    contract: source_contract(),
                    value: NativeCapability::Component(Rc::new(Component(prepare_source))),
                },
            )]))
        })
        .expect("unique source implementation");
}

#[cfg(test)]
pub(super) fn painted_area(state: &dyn Any) -> Option<Placement> {
    state
        .downcast_ref::<Owner>()?
        .document
        .borrow()
        .glyphs
        .as_ref()
        .map(|glyphs| glyphs.area)
}
