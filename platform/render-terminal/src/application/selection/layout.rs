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

//! Reading geometry keeps semantic byte positions separate from visual cells.
//! Markdown caret resolution is extracted from the original transcript's
//! caret_position/line_caret algorithms, including wrapped and stacked tables.
use super::Placement;
use crate::{
    editor::layout::Layout as Literal,
    markdown::{VisualLine, prepared::Document},
};
use ratatui::layout::Position;
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
pub(in crate::application) enum Layout {
    Literal(Arc<Literal>),
    Markdown {
        lines: Arc<[VisualLine]>,
        text: Arc<str>,
    },
}
impl From<Literal> for Layout {
    fn from(value: Literal) -> Self {
        Self::Literal(Arc::new(value))
    }
}
impl Layout {
    pub fn markdown(document: &Document, width: u16) -> maka_ui_runtime::Result<Self> {
        let mut cursor = document.cursor(width, 0..crate::markdown::MAX_LINES);
        while !cursor
            .advance(usize::MAX)
            .map_err(|e| maka_ui_runtime::Error::Invalid(e.into()))?
            .complete
        {}
        Ok(Self::Markdown {
            lines: cursor.lines().to_vec().into(),
            text: document.shared_text(),
        })
    }

    pub fn cursor(&self, byte: usize, trailing: bool) -> (usize, usize) {
        let Self::Markdown { lines, .. } = self else {
            let Self::Literal(layout) = self else {
                unreachable!()
            };
            return layout.cursor(byte, trailing);
        };
        let mut end = None;
        let mut nearest = None;
        for (row, visual) in lines.iter().enumerate() {
            let text = visual.line.to_string();
            for span in &visual.mapping {
                for (logical, display) in [
                    (span.logical.start, span.display.start),
                    (span.logical.end, span.display.end),
                ] {
                    let distance = byte.abs_diff(logical);
                    if nearest.as_ref().is_none_or(|(old, _)| distance < *old) {
                        nearest = Some((distance, (row, text[..display].width())));
                    }
                }
                if span.logical.contains(&byte) || span.logical.end == byte {
                    let display = if span.logical.len() == span.display.len() {
                        span.display.start + byte - span.logical.start
                    } else if byte == span.logical.end {
                        span.display.end
                    } else {
                        span.display.start
                    };
                    if let Some(prefix) = text.get(..display) {
                        let position = (row, prefix.width());
                        if span.logical.contains(&byte) || trailing && span.logical.end == byte {
                            return position;
                        }
                        end = Some(position);
                    }
                }
            }
        }
        end.or_else(|| nearest.map(|(_, position)| position))
            .unwrap_or((0, 0))
    }
    pub fn byte_at(&self, row: usize, column: usize) -> usize {
        match self {
            Self::Literal(layout) => layout.byte_at(row, column),
            Self::Markdown { lines, .. } => {
                let row = row.min(lines.len().saturating_sub(1));
                let visual = lines
                    .get(row)
                    .filter(|line| !line.mapping.is_empty())
                    .or_else(|| lines.iter().skip(row).find(|line| !line.mapping.is_empty()))
                    .or_else(|| {
                        lines[..row]
                            .iter()
                            .rev()
                            .find(|line| !line.mapping.is_empty())
                    });
                let Some(visual) = visual else {
                    return 0;
                };
                let text = visual.line.to_string();
                let mut nearest = None;
                let mut cell = 0;
                for (byte, glyph) in text.grapheme_indices(true) {
                    let width = glyph.width();
                    let mut mapped: Option<std::ops::Range<usize>> = None;
                    for span in &visual.mapping {
                        let start = span.display.start.max(byte);
                        let end = span.display.end.min(byte + glyph.len());
                        if start >= end {
                            continue;
                        }
                        let logical = if span.display.len() == span.logical.len() {
                            span.logical.start + start - span.display.start
                                ..span.logical.start + end - span.display.start
                        } else {
                            span.logical.clone()
                        };
                        mapped = Some(mapped.map_or(logical.clone(), |old| {
                            old.start.min(logical.start)..old.end.max(logical.end)
                        }));
                    }
                    if let Some(logical) = mapped {
                        for (position, offset) in
                            [(cell, logical.start), (cell + width, logical.end)]
                        {
                            let distance = column.abs_diff(position);
                            if nearest.as_ref().is_none_or(|(old, _)| distance < *old) {
                                nearest = Some((distance, offset));
                            }
                        }
                    }
                    cell += width;
                }
                nearest.map_or(0, |(_, offset)| offset)
            }
        }
    }
    pub fn pointer(&self, area: Placement, top: usize, point: Position) -> (usize, bool) {
        match self {
            Self::Literal(layout) => layout.pointer(area, top, point),
            Self::Markdown { .. } => {
                let visible = area.visible();
                let row = top
                    + (i64::from(point.y.clamp(visible.y, visible.bottom().saturating_sub(1)))
                        - area.y) as usize;
                let column = (i64::from(point.x) - area.x)
                    .clamp(0, i64::from(area.width.saturating_sub(1)))
                    as usize;
                let byte = self.byte_at(row, column);
                (byte, self.cursor(byte, false).0 != row)
            }
        }
    }
    pub fn adjacent_row(&self, row: usize, forward: bool) -> Option<usize> {
        let count = match self {
            Self::Literal(layout) => layout.rows.len(),
            Self::Markdown { lines, .. } => lines.len(),
        };
        let readable = |row: &usize| match self {
            Self::Literal(..) => true,
            Self::Markdown { lines, .. } => !lines[*row].mapping.is_empty(),
        };
        if forward {
            (row + 1..count).find(readable)
        } else {
            (0..row).rev().find(readable)
        }
    }
    pub fn edge_row(&self, first: bool) -> usize {
        match self {
            Self::Literal(layout) => {
                if first {
                    0
                } else {
                    layout.rows.len().saturating_sub(1)
                }
            }
            Self::Markdown { lines, .. } => if first {
                lines.iter().position(|line| !line.mapping.is_empty())
            } else {
                lines.iter().rposition(|line| !line.mapping.is_empty())
            }
            .unwrap_or(0),
        }
    }
    pub fn previous(&self, byte: usize) -> usize {
        match self {
            Self::Literal(layout) => layout.previous(byte),
            Self::Markdown { text, .. } => text
                .grapheme_indices(true)
                .map(|(offset, _)| offset)
                .take_while(|offset| *offset < byte)
                .last()
                .unwrap_or(0),
        }
    }
    pub fn next(&self, byte: usize) -> usize {
        match self {
            Self::Literal(layout) => layout.next(byte),
            Self::Markdown { text, .. } => text
                .grapheme_indices(true)
                .map(|(offset, _)| offset)
                .find(|offset| *offset > byte)
                .unwrap_or(text.len()),
        }
    }
    pub fn row_start(&self, row: usize) -> usize {
        self.byte_at(row, 0)
    }
    pub fn row_end(&self, row: usize) -> usize {
        match self {
            Self::Literal(layout) => layout.rows[row.min(layout.rows.len() - 1)].end,
            Self::Markdown { .. } => self.byte_at(row, usize::MAX),
        }
    }
}
