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

//! Measures that several regions share so their edges line up. Each is in
//! `rem`, so it follows the theme's base font (the application zoom).

use gpui_kit::{Div, Hsla, IntoElement, ParentElement as _, Styled, div, rems};

/// The maximum width of the main pane's content column: the transcript, the
/// composer, and the empty state all use it. 720px at gpui-kit's 16px rem
/// (the base font stays 16px; see `crate::theme`).
pub const COLUMN_MAX_WIDTH_REMS: f32 = 45.;

/// The reading column's side padding inside the plate: the transcript's
/// and the composer's gutters, 24px at gpui-kit's 16px rem (spec §7).
pub const COLUMN_GUTTER_REMS: f32 = 1.5;

/// The room a hovered prompt rail takes from the transcript's right edge:
/// 53px (the conversation's `prompt_rail::HIT_WIDTH`). The task's
/// conversation keeps it right of its reading column, as wide as its
/// transcript allows, whether or not the rail shows.
pub const RAIL_GUTTER_REMS: f32 = 53. / 16.;

/// The narrowest transcript that keeps the rail's room: 560px, where the
/// column keeps 483px between its 24px gutter and the rail's 53. Narrower,
/// the column has even gutters and the rail does not show.
pub const RAIL_MIN_WIDTH_REMS: f32 = 35.;

/// The transcript's inset in its plate either side: a 4px margin (room for
/// its focus ring) inside which its 1px border runs. A row outside the
/// transcript lines its reading column up with the transcript's by taking
/// the same inset ([`transcript_inset`]).
pub const TRANSCRIPT_MARGIN_REMS: f32 = 0.25;

/// The gutters either side of the reading column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColumnGutters {
    /// [`COLUMN_GUTTER_REMS`] either side: a side chat's column, and the
    /// task's in a transcript narrower than [`RAIL_MIN_WIDTH_REMS`].
    #[default]
    Even,
    /// [`COLUMN_GUTTER_REMS`] on the left, [`RAIL_GUTTER_REMS`] on the right:
    /// the task's conversation, whose prompt rail stands in that room.
    Rail,
}

impl ColumnGutters {
    /// The gutters of a column in a transcript `width` rem wide: the rail's
    /// room when the conversation keeps it (`rail`) and the width holds it.
    pub fn for_transcript(rail: bool, width: f32) -> Self {
        if rail && width >= RAIL_MIN_WIDTH_REMS { Self::Rail } else { Self::Even }
    }

    /// The right gutter, in rem.
    pub fn right(self) -> f32 {
        match self {
            Self::Even => COLUMN_GUTTER_REMS,
            Self::Rail => RAIL_GUTTER_REMS,
        }
    }
}

/// A row placing `column` in the reading column: [`COLUMN_MAX_WIDTH_REMS`]
/// wide and centred, unless that leaves a side less than its gutter; then
/// it stands that gutter off the edge, and narrows once both are reached.
/// `column` fills the column (it should be `w_full`).
///
/// With even gutters this is the centred column inside 24px of padding;
/// with the rail's, a column that fits centred still is, so the two agree
/// from the width that holds the rail's room either side.
pub fn reading_column(gutters: ColumnGutters, column: impl IntoElement) -> Div {
    // Each gutter grows from nothing to an equal share of the room the
    // column leaves, but never below its own width; the column shrinks
    // once neither gutter can.
    let gutter = |width: f32| div().flex_1().flex_shrink_0().min_w(rems(width));
    div()
        .flex()
        .flex_row()
        .w_full()
        .child(gutter(COLUMN_GUTTER_REMS))
        .child(div().w(rems(COLUMN_MAX_WIDTH_REMS)).flex_shrink_1().min_w_0().child(column))
        .child(gutter(gutters.right()))
}

/// Insets `row`, as wide as the plate, by the transcript's inset
/// ([`TRANSCRIPT_MARGIN_REMS`] and a clear 1px border either side), so a
/// [`reading_column`] in it lines up with the transcript's.
pub fn transcript_inset<E: Styled>(row: E) -> E {
    row.px(rems(TRANSCRIPT_MARGIN_REMS)).border_x_1().border_color(Hsla::transparent_black())
}

/// The maximum width of a sidebar page's column (Extensions, Scheduled
/// tasks), its 24px side padding included. Desktop's module page is a
/// Layout of `contentWidth={MODULE_PAGE_WIDTH}` (900) and `padding={5}`,
/// whose content Astryx aligns to 900 less 20 each side: 860px of content,
/// as here (908 less 24 each side). Settings' column is Desktop's other
/// width, 920 less 24 each side (review round 12).
pub const PAGE_MAX_WIDTH_REMS: f32 = 56.75;

/// The line a plate's chrome and lists start their content on, in from the
/// plate's edge: 16px. Row fills sit half way, 8px in, so their content
/// lands on the line; an icon button at the plate's edge puts its glyph's
/// ink there ([`ink_padding`]).
pub const PLATE_LINE_REMS: f32 = 1.;

/// A plate's icon button: 28px square, its glyph 16px.
pub const ICON_BUTTON_REMS: f32 = 1.75;
pub const ICON_GLYPH_REMS: f32 = 1.;

/// The padding from an edge that puts a glyph's ink `line` in from it: the
/// glyph, `glyph` square and centred in a `button` square button, draws its
/// strokes `ink` (a share of its side) inside its square on that side. All
/// in rem. The ink, not the button's frame or the glyph's square, is what
/// lines up with the text and the rows' icons on the line.
pub const fn ink_padding(line: f32, button: f32, glyph: f32, ink: f32) -> f32 {
    line - (button - glyph) / 2. - glyph * ink
}
