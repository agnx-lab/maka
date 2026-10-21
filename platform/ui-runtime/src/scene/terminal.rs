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

//! Terminal cell scene declarations and protocol validation.

use crate::ComponentSlot;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Metadata {
    pub slots: Vec<ComponentSlot>,
    pub inputs: BTreeMap<String, Value>,
    pub hidden_slots: BTreeSet<String>,
}

/// Semantic terminal colors for rich text and editors, supplied by the product.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "TerminalPalette")]
pub struct Palette {
    pub background: Color,
    pub foreground: Color,
    pub surface: Color,
    pub accent: Color,
    pub thinking: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub muted: Color,
    pub subtle: Color,
    pub border: Color,
    pub scrollbar: Color,
    pub selection: Color,
    pub selection_text: Color,
    pub search: Color,
    pub search_active: Color,
    pub search_text: Color,
    pub syntax: [Color; 7],
    pub terminal: bool,
}

/// Terminal colors retain the user's ANSI palette and default background.
/// Numeric values are RGB; named values are emitted as terminal attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
#[schemars(rename = "TerminalColor")]
pub enum Color {
    Rgb(#[schemars(range(max = 0xffffff))] u32),
    Named(NamedColor),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "TerminalNamedColor")]
pub enum NamedColor {
    Reset,
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
}
impl Color {
    pub fn valid(self) -> bool {
        !matches!(self, Self::Rgb(value) if value > 0xffffff)
    }
}
impl From<u32> for Color {
    fn from(value: u32) -> Self {
        Self::Rgb(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "TerminalDirection")]
pub enum Direction {
    Row,
    Column,
}
/// Native terminal cell constraints on a child in its parent's main axis.
/// `grow` distributes remaining cells after fixed/intrinsic siblings; `basis`
/// supplies an explicit base instead of intrinsic length. `shrink` allows that
/// base to shrink proportionally when siblings exceed the available cells.
/// Row wrap preserves
/// source order and measures its rows before allocating the parent viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
#[schemars(rename = "TerminalLayout")]
pub struct Layout {
    pub grow: u16,
    pub basis: Option<u16>,
    pub basis_fraction: Option<FractionalBasis>,
    pub shrink: bool,
    pub wrap: bool,
}
/// Fraction of the allocated main axis, rounded down and clamped before growth
/// or shrinking. An unbounded axis uses the intrinsic length with the same bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalFractionalBasis")]
pub struct FractionalBasis {
    pub numerator: u16,
    #[schemars(range(min = 1))]
    pub denominator: u16,
    #[serde(default)]
    pub min: u16,
    #[serde(default)]
    pub max: Option<u16>,
}
impl Layout {
    fn valid(&self) -> bool {
        self.basis_fraction.is_none_or(|fraction| {
            self.basis.is_none()
                && fraction.denominator > 0
                && fraction.numerator <= fraction.denominator
                && fraction.max.is_none_or(|max| max >= fraction.min)
        })
    }
}
/// Rearrange the same direct children at an allocated inner-width breakpoint.
/// Child overrides affect allocation in this stack, not a child's own wrapping.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalResponsiveStack")]
pub struct ResponsiveStack {
    #[schemars(range(min = 1))]
    pub min_width: u16,
    pub direction: Direction,
    /// Scrolling can follow the same allocated-width breakpoint as the layout.
    #[serde(default)]
    pub scroll: Option<ScrollPolicy>,
    #[serde(default)]
    pub children: BTreeMap<String, Layout>,
}
/// Explicit wheel actions in an original scrolling viewport. Offsets remain native.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalWheelCommands")]
pub struct WheelCommands {
    #[schemars(length(min = 1))]
    pub up: String,
    #[schemars(length(min = 1))]
    pub down: String,
}
/// Native scroll policy; offsets and accepted viewport geometry remain native.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "TerminalScrollPolicy")]
pub enum ScrollPolicy {
    #[default]
    None,
    Manual,
    Anchor,
    FollowEnd,
}
/// A one-cell frame. Colors follow the same RGB convention as terminal text.
/// Focus color follows any focused native target inside this stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalBorder")]
pub struct Border {
    #[serde(default)]
    pub breathing: bool,
    pub color: Color,
    pub focused_color: Color,
    pub style: BorderStyle,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "TerminalBorderStyle")]
pub enum BorderStyle {
    Rounded,
    Square,
    Ascii,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "TerminalAlign")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalAnimationFrame")]
pub struct AnimationFrame {
    pub text: String,
    #[schemars(range(min = 1))]
    pub millis: u16,
}

/// Native single-choice menu. Only committed selections leave the presenter.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalChoice")]
pub struct Choice {
    pub key: String,
    pub label: String,
    pub options: Vec<ChoiceOption>,
    pub current: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub ascii: bool,
    pub foreground: Color,
    pub background: Color,
    pub border: Color,
    pub selected: Color,
    pub accent: Color,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalChoiceOption")]
pub struct ChoiceOption {
    pub key: String,
    pub label: String,
}
impl Choice {
    fn validate(&self) -> Result<(), String> {
        let mut keys = BTreeSet::new();
        if [
            self.foreground,
            self.background,
            self.border,
            self.selected,
            self.accent,
        ]
        .iter()
        .any(|color| !color.valid())
            || self
                .options
                .iter()
                .any(|option| option.key.is_empty() || !keys.insert(&option.key))
            || self.current.as_ref().is_some_and(|key| !keys.contains(key))
        {
            return Err("invalid terminal choice".into());
        }
        Ok(())
    }
}
/// Appearance shared by controlled dialogs and transient adaptive panes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalDialogAppearance")]
pub struct DialogAppearance {
    /// Place a controlled dialog next to its background, preferring above it.
    /// The viewport bounds remain authoritative on small terminals.
    #[serde(default)]
    pub anchor_to_background: bool,
    #[serde(default)]
    pub ascii: bool,
    pub label: String,
    /// Maximum foreground body dimensions, in terminal cells.
    pub width: u16,
    pub height: u16,
    pub background_color: Color,
    pub border_color: Color,
    #[serde(default)]
    pub title_color: Option<Color>,
    #[serde(default)]
    pub padding: u16,
}
impl DialogAppearance {
    fn validate(&self) -> Result<(), String> {
        if self.label.is_empty()
            || self.width == 0
            || self.height == 0
            || !self.background_color.valid()
            || !self.border_color.valid()
            || self.title_color.is_some_and(|color| !color.valid())
        {
            return Err("invalid terminal dialog presentation".into());
        }
        Ok(())
    }
}
/// A one-shot mechanical command. Repeated or older sequences are ignored.
/// Toggle uses the last successfully placed viewport; before first placement it
/// is consumed without changing state. Dismiss never changes inline preference.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "TerminalPaneCommand")]
pub struct PaneCommand {
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub op: PaneOperation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "TerminalPaneOperation")]
pub enum PaneOperation {
    Toggle,
    DismissTransient,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(rename = "TerminalScene")]
pub enum Node {
    /// A one-cell separator spanning its allocated axis.
    Rule {
        key: String,
        direction: Direction,
        color: Color,
        ascii: bool,
    },
    Choice(Choice),
    /// An interactive rich-content item, using the original Surface semantics.
    Item {
        key: String,
        content: Box<Node>,
        #[serde(default)]
        disabled: bool,
        #[serde(default)]
        current: bool,
        /// Current-row fill; absent uses the terminal's reverse-video selection.
        #[serde(default)]
        selected_color: Option<Color>,
        #[serde(default)]
        selected_foreground: Option<Color>,
        #[serde(default)]
        follow_focus: bool,
        #[serde(default)]
        submit: bool,
        /// Emit submit for physical Space; focus and pointer activation stay separate.
        #[serde(default)]
        submit_space: bool,
    },
    /// Native object menu: right click or its visible action button opens locally.
    ContextMenu {
        key: String,
        /// Native Copy selection entry; absent omits selection capture.
        #[serde(default)]
        copy_selection_label: Option<String>,
        background: Box<Node>,
        content: Box<Node>,
        #[serde(flatten)]
        appearance: DialogAppearance,
    },
    Dialog {
        key: String,
        background: Box<Node>,
        content: Box<Node>,
        open: bool,
        #[serde(flatten)]
        appearance: DialogAppearance,
    },
    /// One retained pane: native inline preference plus an independent transient
    /// dialog. Resizing never opens or dismisses the transient.
    AdaptivePane {
        key: String,
        background: Box<Node>,
        content: Box<Node>,
        pane_width: u16,
        inline_min_width: u16,
        auto_inline_min_width: u16,
        overlay: DialogAppearance,
        motion: bool,
        command: Option<PaneCommand>,
    },
    Stack {
        key: String,
        direction: Direction,
        #[serde(default)]
        wide: Option<ResponsiveStack>,
        gap: u16,
        padding: u16,
        /// Layout-only stacks inherit their parent; an explicit color fills the area.
        #[serde(default)]
        background: Option<Color>,
        children: Vec<Node>,
        /// One cell outside padding; absent means no border or border inset.
        #[serde(default)]
        border: Option<Border>,
        /// Establish the allocated inner size as the descendants' layout viewport.
        /// Children do not contribute intrinsic size; use grow/basis or an
        /// externally allocated root area to supply this container's size.
        #[serde(default)]
        viewport: bool,
        /// One Tab stop; arrows navigate its items in declaration order.
        #[serde(default)]
        focus_group: bool,
        /// On structural reveal, focus the first visible enabled tab stop;
        /// a scrollable container without tab stops receives focus itself.
        #[serde(default)]
        auto_focus: bool,
        #[serde(default)]
        wheel: Option<WheelCommands>,
        #[serde(default)]
        shortcuts: Vec<crate::shortcut::Shortcut>,
        #[serde(default)]
        focus_shortcuts: Vec<crate::shortcut::FocusShortcut>,
        #[serde(default)]
        layout: Layout,
        #[serde(default)]
        scroll: ScrollPolicy,
        #[serde(default = "visible")]
        visible: bool,
    },
    Text {
        key: String,
        text: String,
        /// Passive hover description; never participates in focus or activation.
        #[serde(default)]
        hint: String,
        color: Color,
        /// Render a single line, eliding overflow at a complete grapheme.
        #[serde(default)]
        truncate: bool,
        #[serde(default)]
        animation: Vec<AnimationFrame>,
        #[serde(default)]
        align: Align,
        #[serde(default)]
        bold: bool,
    },
    Button {
        key: String,
        label: String,
        /// Label alignment within the button hit area; absent centers it.
        #[serde(default)]
        align: Option<Align>,
        #[serde(default)]
        hint: String,
        /// Enter a declared child's native focus target instead of emitting a click.
        #[serde(default)]
        focus_child: Option<String>,
        /// Copy this committed text as the physical button's default action.
        /// No clipboard access occurs from a synthetic component dispatch.
        #[serde(default)]
        copy_text: Option<String>,
        #[serde(default)]
        disabled: bool,
        /// Absent foreground uses the terminal's default text color.
        #[serde(default)]
        color: Option<Color>,
        /// Fill the whole button hit area at rest; absent inherits its parent.
        #[serde(default)]
        background: Option<Color>,
        /// Hover lifts the fill without selecting or focusing the button.
        #[serde(default)]
        hover_background: Option<Color>,
        /// Emphasize the label and hit area independently of keyboard focus.
        #[serde(default)]
        bold: bool,
    },
    Component(ComponentSlot),
}
fn visible() -> bool {
    true
}
impl Node {
    pub fn key(&self) -> &str {
        match self {
            Self::Stack { key, .. }
            | Self::Rule { key, .. }
            | Self::Text { key, .. }
            | Self::Button { key, .. }
            | Self::Dialog { key, .. }
            | Self::ContextMenu { key, .. }
            | Self::AdaptivePane { key, .. }
            | Self::Item { key, .. } => key,
            Self::Component(slot) => &slot.key,
            Self::Choice(choice) => &choice.key,
        }
    }
    pub fn layout(&self) -> Layout {
        match self {
            Self::Stack { layout, .. } => *layout,
            Self::Dialog { background, .. }
            | Self::ContextMenu { background, .. }
            | Self::AdaptivePane { background, .. } => background.layout(),
            Self::Item { content, .. } => content.layout(),
            _ => Layout::default(),
        }
    }
    pub fn visible(&self) -> bool {
        !matches!(self, Self::Stack { visible: false, .. })
    }
    pub fn validate_root(&self) -> Result<(), String> {
        let mut keys = BTreeSet::new();
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            node.validate(0, &mut keys)?;
            node.children_into(&mut pending);
        }
        Ok(())
    }
    fn validate(&self, _depth: usize, keys: &mut BTreeSet<String>) -> Result<(), String> {
        let key = match self {
            Self::Stack { key, .. }
            | Self::Rule { key, .. }
            | Self::Text { key, .. }
            | Self::Button { key, .. }
            | Self::Dialog { key, .. }
            | Self::ContextMenu { key, .. }
            | Self::AdaptivePane { key, .. }
            | Self::Item { key, .. } => key,
            Self::Component(slot) => &slot.key,
            Self::Choice(choice) => &choice.key,
        };
        if key.is_empty() {
            return Err("invalid UI node identity".into());
        }
        if !keys.insert(key.clone()) {
            return Err(format!("duplicate UI node identity: {key:?}"));
        }
        if let Self::Stack {
            shortcuts,
            focus_shortcuts,
            ..
        } = self
        {
            crate::shortcut::validate_focus(shortcuts, focus_shortcuts)?;
        }
        match self {
            Self::Rule { color, .. } => {
                if !color.valid() {
                    return Err("invalid separator color".into());
                }
            }
            Self::Choice(choice) => choice.validate()?,
            Self::Item {
                selected_color,
                selected_foreground,
                ..
            } => {
                if selected_color.is_some_and(|color| !color.valid())
                    || selected_foreground.is_some_and(|color| !color.valid())
                {
                    return Err("invalid item selection color".into());
                }
            }
            Self::ContextMenu { appearance, .. } | Self::Dialog { appearance, .. } => {
                appearance.validate()?;
            }
            Self::AdaptivePane {
                pane_width,
                inline_min_width,
                auto_inline_min_width,
                overlay,
                command,
                ..
            } => {
                overlay.validate()?;
                if *pane_width == 0
                    || inline_min_width <= pane_width
                    || auto_inline_min_width < inline_min_width
                    || command
                        .as_ref()
                        .is_some_and(|command| command.sequence == 0)
                {
                    return Err("invalid terminal adaptive pane".into());
                }
            }
            Self::Stack {
                background,
                border,
                children,
                layout,
                direction,
                wide,
                scroll,
                wheel,
                ..
            } => {
                if wheel.as_ref().is_some_and(|wheel| {
                    (*scroll == ScrollPolicy::None
                        && wide
                            .as_ref()
                            .and_then(|wide| wide.scroll)
                            .is_none_or(|policy| policy == ScrollPolicy::None))
                        || [wheel.up.as_str(), wheel.down.as_str()]
                            .iter()
                            .any(|key| key.is_empty())
                }) {
                    return Err(
                        "wheel commands require a scrolling viewport and valid input keys".into(),
                    );
                }
                if !layout.valid()
                    || (layout.wrap && !matches!(direction, Direction::Row))
                    || (*scroll != ScrollPolicy::None && !matches!(direction, Direction::Column))
                    || wide.as_ref().is_some_and(|wide| {
                        wide.min_width == 0
                            || (layout.wrap && wide.direction != Direction::Row)
                            || (wide.scroll.unwrap_or(*scroll) != ScrollPolicy::None
                                && wide.direction != Direction::Column)
                            || wide.children.iter().any(|(key, layout)| {
                                !children.iter().any(|child| child.key() == key)
                                    || !layout.valid()
                                    || layout.wrap
                            })
                    })
                {
                    return Err("invalid terminal layout".into());
                }
                if background.is_some_and(|color| !color.valid())
                    || border.is_some_and(|border| {
                        !border.color.valid() || !border.focused_color.valid()
                    })
                {
                    return Err("invalid UI style".into());
                }
            }
            Self::Text {
                color, animation, ..
            } => {
                if !color.valid() || animation.iter().any(|frame| frame.millis == 0) {
                    return Err("invalid text".into());
                }
            }
            Self::Button {
                color,
                background,
                hover_background,
                copy_text,
                focus_child,
                ..
            } => {
                if color.is_some_and(|color| !color.valid())
                    || background.is_some_and(|color| !color.valid())
                    || hover_background.is_some_and(|color| !color.valid())
                    || copy_text.as_ref().is_some_and(|text| text.is_empty())
                    || (copy_text.is_some() && focus_child.is_some())
                {
                    return Err("invalid button".into());
                }
            }
            Self::Component(slot) => {
                if slot.binding.is_empty() {
                    return Err("invalid slot binding".into());
                }
            }
        }
        Ok(())
    }
    fn children_into<'a>(&'a self, pending: &mut Vec<&'a Self>) {
        match self {
            Self::Stack { children, .. } => pending.extend(children.iter().rev()),
            Self::Item { content, .. } => pending.push(content),
            Self::Dialog {
                background,
                content,
                ..
            }
            | Self::ContextMenu {
                background,
                content,
                ..
            }
            | Self::AdaptivePane {
                background,
                content,
                ..
            } => {
                pending.push(content);
                pending.push(background);
            }
            _ => {}
        }
    }
    pub fn validate_focus_targets(&self, slots: &[ComponentSlot]) -> Result<(), String> {
        let mut pending = vec![self];
        while let Some(node) = pending.pop() {
            match node {
                Self::Button {
                    focus_child: Some(child),
                    ..
                } if !slots.iter().any(|slot| slot.key == *child) => {
                    return Err("focus button names an undeclared child".into());
                }
                Self::Stack {
                    focus_shortcuts, ..
                } if focus_shortcuts
                    .iter()
                    .any(|binding| !slots.iter().any(|slot| slot.key == binding.child)) =>
                {
                    return Err("focus shortcut names an undeclared child".into());
                }
                _ => {}
            }
            node.children_into(&mut pending);
        }
        Ok(())
    }
    pub fn wheel_commands(&self, key: &str) -> Option<&WheelCommands> {
        match self {
            Self::Stack {
                key: own,
                wheel,
                children,
                visible: true,
                ..
            } => {
                if own == key {
                    wheel.as_ref()
                } else {
                    children.iter().find_map(|child| child.wheel_commands(key))
                }
            }
            Self::Item { content, .. } => content.wheel_commands(key),
            Self::Dialog {
                background,
                content,
                open,
                ..
            } => {
                if *open {
                    content.wheel_commands(key)
                } else {
                    background.wheel_commands(key)
                }
            }
            Self::ContextMenu {
                background,
                content,
                ..
            }
            | Self::AdaptivePane {
                background,
                content,
                ..
            } => background
                .wheel_commands(key)
                .or_else(|| content.wheel_commands(key)),
            _ => None,
        }
    }
    pub fn metadata(&self, inputs: BTreeMap<String, Value>) -> Result<Metadata, String> {
        let mut metadata = Metadata {
            inputs,
            ..Metadata::default()
        };
        let mut commands = Vec::new();
        self.collect_metadata(&mut metadata, &mut commands, true)?;
        // Native schemas must all exist before commands can merge or reject collisions.
        self.validate_focus_targets(&metadata.slots)?;
        for command in commands {
            if let Self::Stack {
                shortcuts, wheel, ..
            } = command
            {
                crate::shortcut::inputs(shortcuts, &mut metadata.inputs)?;
                if let Some(wheel) = wheel {
                    for key in [&wheel.up, &wheel.down] {
                        if metadata
                            .inputs
                            .get(key)
                            .is_some_and(|schema| schema != &Value::Bool(true))
                        {
                            return Err("wheel command collides with typed native input".into());
                        }
                        metadata.inputs.insert(key.clone(), Value::Bool(true));
                    }
                }
            }
        }
        Ok(metadata)
    }

    fn collect_metadata<'a>(
        &'a self,
        metadata: &mut Metadata,
        commands: &mut Vec<&'a Self>,
        visible: bool,
    ) -> Result<(), String> {
        match self {
            Self::Item {
                key,
                content,
                disabled,
                submit,
                submit_space,
                ..
            } => {
                if visible && !disabled {
                    let kinds = if *submit || *submit_space {
                        vec!["activate", "submit"]
                    } else {
                        vec!["activate"]
                    };
                    if metadata.inputs.insert(key.clone(), json!({"type":"object","properties":{"kind":{"enum":kinds}},"required":["kind"],"additionalProperties":false})).is_some() {
                        return Err("reserved scene input identity".into());
                    }
                }
                content.collect_metadata(metadata, commands, visible)?;
            }
            Self::Choice(choice) => {
                if visible && !choice.disabled && !choice.options.is_empty() {
                    let schema = json!({
                        "type":"object", "properties":{
                            "kind":{"const":"selected"},
                            "key":{"enum":choice.options.iter().map(|option| &option.key).collect::<Vec<_>>()}
                        }, "required":["kind","key"], "additionalProperties":false
                    });
                    if metadata.inputs.insert(choice.key.clone(), schema).is_some() {
                        return Err("reserved scene input identity".into());
                    }
                }
            }
            Self::Dialog {
                key,
                background,
                content,
                open,
                ..
            } => {
                if visible
                    && *open
                    && metadata
                        .inputs
                        .insert(
                            key.clone(),
                            serde_json::to_value(schemars::schema_for!(crate::dismiss::Input))
                                .expect("dismiss schema"),
                        )
                        .is_some()
                {
                    return Err("reserved scene input identity".into());
                }
                background.collect_metadata(metadata, commands, visible)?;
                content.collect_metadata(metadata, commands, visible && *open)?;
            }
            Self::ContextMenu {
                background,
                content,
                ..
            }
            | Self::AdaptivePane {
                background,
                content,
                ..
            } => {
                // Native visibility is local layout; both declarations keep
                // their original component owners and input schemas.
                background.collect_metadata(metadata, commands, visible)?;
                content.collect_metadata(metadata, commands, visible)?;
            }
            Self::Component(slot) => {
                metadata.slots.push(slot.clone());
                if !visible {
                    metadata.hidden_slots.insert(slot.key.clone());
                }
            }
            Self::Button {
                key,
                disabled,
                focus_child,
                ..
            } => {
                if visible
                    && !disabled
                    && focus_child.is_none()
                    && metadata.inputs.insert(key.clone(), json!(true)).is_some()
                {
                    return Err("reserved scene input identity".into());
                }
            }
            Self::Stack {
                children,
                visible: local,
                shortcuts,
                wheel,
                ..
            } => {
                if visible && *local && (!shortcuts.is_empty() || wheel.is_some()) {
                    commands.push(self);
                }
                for child in children {
                    child.collect_metadata(metadata, commands, visible && *local)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Node {
    pub fn overlay(&self, key: &str) -> Option<&Node> {
        match self {
            Self::Choice(choice) if choice.key == key => Some(self),
            Self::Dialog {
                key: own,
                background,
                content,
                ..
            }
            | Self::ContextMenu {
                key: own,
                background,
                content,
                ..
            }
            | Self::AdaptivePane {
                key: own,
                background,
                content,
                ..
            } => {
                if own == key {
                    Some(self)
                } else {
                    background.overlay(key).or_else(|| content.overlay(key))
                }
            }
            Self::Stack { children, .. } => children.iter().find_map(|node| node.overlay(key)),
            Self::Item { content, .. } => content.overlay(key),
            _ => None,
        }
    }
}

impl Node {
    pub fn dialog_content(&self) -> Option<(&DialogAppearance, &Node)> {
        match self {
            Self::ContextMenu {
                appearance,
                content,
                ..
            }
            | Self::Dialog {
                appearance,
                content,
                ..
            } => Some((appearance, content)),
            Self::AdaptivePane {
                overlay, content, ..
            } => Some((overlay, content)),
            _ => None,
        }
    }
}
