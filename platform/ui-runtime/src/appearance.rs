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

//! Complete immutable presentation inputs. Brands supply values; native
//! adapters project them without reading an application-wide palette.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Packed RGBA, including explicit alpha zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Color(pub u32);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FontFamily(#[schemars(length(min = 1))] pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontAppearance {
    pub family: FontFamily,
    pub fallbacks: Vec<FontFamily>,
    #[schemars(range(min = 6, max = 96))]
    pub size: f32,
    /// Line height relative to this font size.
    #[schemars(range(min = 1, max = 3))]
    pub line_height: f32,
    #[schemars(range(min = 1, max = 1000))]
    pub weight: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextAppearance {
    pub font: FontAppearance,
    pub foreground: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Insets {
    #[schemars(range(min = 0, max = 128))]
    pub top: f32,
    #[schemars(range(min = 0, max = 128))]
    pub right: f32,
    #[schemars(range(min = 0, max = 128))]
    pub bottom: f32,
    #[schemars(range(min = 0, max = 128))]
    pub left: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputAppearance {
    #[serde(default)]
    pub ascii: bool,
    #[serde(default)]
    pub terminal: Option<crate::scene::terminal::Palette>,
    pub text: TextAppearance,
    pub muted_foreground: Color,
    pub background: Color,
    pub selection: Color,
    pub caret: Color,
    pub border: Color,
    pub focused_border: Color,
    #[schemars(range(min = 0, max = 8))]
    /// In TUI inputs, a positive width draws a one-cell border; zero has no inset.
    pub border_width: f32,
    #[schemars(range(min = 0, max = 64))]
    pub radius: f32,
    pub padding: Insets,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextareaAppearance {
    #[serde(default)]
    pub ascii: bool,
    #[serde(default)]
    pub terminal: Option<crate::scene::terminal::Palette>,
    pub text: TextAppearance,
    pub muted_foreground: Color,
    pub background: Color,
    pub selection: Color,
    pub caret: Color,
    pub border: Color,
    pub focused_border: Color,
    #[schemars(range(min = 0, max = 8))]
    /// In TUI inputs, a positive width draws a one-cell border; zero has no inset.
    pub border_width: f32,
    #[schemars(range(min = 0, max = 64))]
    pub radius: f32,
    pub padding: Insets,
    pub scrollbar: ScrollbarAppearance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScrollbarMode {
    Scrolling,
    Hover,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScrollbarEntrance {
    Fade,
    SlideAndFade,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScrollbarAppearance {
    pub mode: ScrollbarMode,
    pub entrance: ScrollbarEntrance,
    #[schemars(range(min = 0, max = 60000))]
    pub idle_millis: u32,
    #[schemars(range(min = 0, max = 1000))]
    pub enter_millis: u32,
    #[schemars(range(min = 0, max = 1000))]
    pub exit_millis: u32,
    #[schemars(range(min = 0, max = 1000))]
    pub expand_millis: u32,
    #[schemars(range(min = 1, max = 64))]
    pub track_width: f32,
    pub track: Color,
    pub track_hover: Color,
    pub track_active: Color,
    pub thumb: ScrollbarThumb,
    pub thumb_hover: ScrollbarThumb,
    pub thumb_active: ScrollbarThumb,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScrollbarThumb {
    pub color: Color,
    #[schemars(range(min = 0, max = 64))]
    pub width: f32,
    #[schemars(range(min = 0, max = 64))]
    pub inset: f32,
    #[schemars(range(min = 0, max = 32))]
    pub radius: f32,
    #[schemars(range(min = 1, max = 4096))]
    pub min_length: f32,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MenuAppearance {
    pub text: TextAppearance,
    pub background: Color,
    pub border: Color,
    pub active_background: Color,
    pub active_foreground: Color,
    pub disabled_foreground: Color,
    #[schemars(range(min = 0, max = 8))]
    pub border_width: f32,
    #[schemars(range(min = 0, max = 64))]
    pub radius: f32,
    pub padding: Insets,
    pub row_padding: Insets,
    #[schemars(range(min = 32, max = 1024))]
    pub width: f32,
    #[schemars(range(min = 0, max = 128))]
    pub window_margin: f32,
}
