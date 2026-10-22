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

//! Native editor protocol. Snapshots are observations, never writable buffers.
use crate::editor::*;
use crate::file::{FileCapture, FileChoose, FileObservation};
use maka_plugin_core::contract::{Capture, Contract, Interface, Transport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
impl crate::editor::Props for TextareaProps {
    fn command(&self) -> Option<&EditorCommand> {
        self.command.as_ref()
    }
    fn set_command(&mut self, command: Option<EditorCommand>) {
        self.command = command;
    }
}
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextareaProps {
    pub appearance: crate::appearance::TextareaAppearance,
    /// Exact whitespace-delimited literals rendered and edited as atomic inline tokens.
    #[serde(default)]
    #[schemars(schema_with = "inline_tokens_schema")]
    pub inline_tokens: Vec<String>,
    #[serde(default)]
    pub menu: Option<TextareaMenu>,
    #[schemars(length(min = 1))]
    pub accessibility_label: String,
    pub read_only: bool,
    pub disabled: bool,
    pub soft_wrap: bool,
    /// Preferred terminal text rows, excluding native padding. A constrained
    /// viewport may shrink further.
    #[serde(default)]
    pub terminal_rows: TerminalTextareaRows,
    /// None preserves native multiline editing without a submit shortcut.
    #[serde(default)]
    pub submit: Option<SubmitKey>,
    /// Yield suggestion navigation only for the original observed editor state.
    #[serde(default)]
    pub navigation: Option<EditorNavigation>,
    #[schemars(range(min = -1, max = 32767))]
    pub tab_index: i16,
    /// Offer native focus once when this original input becomes visible and
    /// enabled. Stable updates and viewport scrolling do not repeat the offer.
    #[serde(default)]
    pub auto_focus: bool,
    #[serde(default)]
    pub initial_text: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    /// None issues no new command; an accepted command remains pending until
    /// its matching receipt commits. Clearing props is not cancellation.
    pub command: Option<EditorCommand>,
    /// Opt-in local file-path paste. The context is returned unchanged; it is
    /// correlation, never a filesystem path or an authorization grant.
    #[serde(default)]
    pub file_paste: Option<FilePastePolicy>,
    #[serde(default)]
    pub file_command: Option<FilePasteCommand>,
    #[serde(default)]
    pub persistence: Option<crate::checkpoint::EditorPersistence>,
}

fn inline_tokens_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::json!({"type":"array","items":{
        "type":"string","minLength":1,"pattern":r"^\S+$"
    }})
    .try_into()
    .expect("inline token schema")
}

/// Source byte spans for exact whole-word literals; no prefix matching.
pub fn inline_token_ranges(text: &str, literals: &[String]) -> Vec<std::ops::Range<usize>> {
    if literals.is_empty() {
        return Vec::new();
    }
    let literals: std::collections::BTreeSet<&str> = literals.iter().map(String::as_str).collect();
    let mut ranges = Vec::new();
    let mut start = 0;
    for word in text.split_whitespace() {
        let offset = text[start..].find(word).expect("word in source") + start;
        if literals.contains(word) {
            ranges.push(offset..offset + word.len());
        }
        start = offset + word.len();
    }
    if !ranges.is_empty() {
        use unicode_segmentation::UnicodeSegmentation;
        let mut boundaries = text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
            .peekable();
        ranges.retain(|range| {
            [range.start, range.end].into_iter().all(|offset| {
                while boundaries.peek().is_some_and(|boundary| *boundary < offset) {
                    boundaries.next();
                }
                boundaries.peek() == Some(&offset)
            })
        });
    }
    ranges
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePastePolicy {
    #[schemars(length(min = 1))]
    pub context: String,
    /// Clipboard text remains available when file attachments are disabled.
    pub allow_files: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilePasteObservation {
    #[schemars(length(min = 1))]
    pub context: String,
    pub selection: FileObservation,
    /// Includes the currently reading, ready or failed item.
    pub remaining: usize,
    /// A failed original path can be reread. Clipboard reads require a new gesture.
    pub retryable: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilePasteCommand {
    pub selection: FileCapture,
    pub action: FilePasteAction,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilePasteAction {
    /// Release this ready/failed item and continue the original physical paste.
    Next,
    /// Retry the original failed path, without a new path from the guest.
    Retry,
    /// Discard this batch; already captured resources retain their own custody.
    Cancel,
}
#[derive(Clone)]
pub struct TextareaView {
    pub props: TextareaProps,
    pub file_request: Option<String>,
}
pub fn file_paste_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(FilePasteObservation))
        .expect("file paste observation")
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalTextareaRows {
    #[schemars(range(min = 1))]
    pub min: u16,
    #[schemars(range(min = 1))]
    pub max: u16,
    /// Allow dragging the lower-right grip to override preferred rows, up to
    /// the containing viewport height. The minimum still applies.
    #[serde(default)]
    pub resizable: bool,
    /// Optional cap relative to the nearest declared layout viewport's height,
    /// or the surface when none is declared. The minimum still applies.
    #[serde(default)]
    #[schemars(range(min = 0, max = 1))]
    pub max_viewport_fraction: Option<f64>,
}
impl Default for TerminalTextareaRows {
    fn default() -> Self {
        Self {
            min: 3,
            max: 8,
            resizable: false,
            max_viewport_fraction: None,
        }
    }
}

/// Local presentation for user-initiated native editing actions. This is not
/// a command interface: clipboard access still requires native activation.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextareaMenu {
    pub appearance: crate::appearance::MenuAppearance,
    pub labels: TextareaMenuLabels,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextareaMenuLabels {
    #[schemars(length(min = 1))]
    pub title: String,
    #[schemars(length(min = 1))]
    pub undo: String,
    #[schemars(length(min = 1))]
    pub redo: String,
    #[schemars(length(min = 1))]
    pub cut: String,
    #[schemars(length(min = 1))]
    pub copy: String,
    #[schemars(length(min = 1))]
    pub paste: String,
    #[schemars(length(min = 1))]
    pub select_all: String,
}
/// Product-selected native submit gesture. PrimaryEnter uses the platform
/// Control+Enter gesture in a terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubmitKey {
    Enter,
    PrimaryEnter,
    ControlEnter,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorNavigation {
    #[schemars(length(min = 1))]
    pub owner: String,
    #[schemars(range(max = 9007199254740991u64))]
    pub based_on: u64,
}
impl EditorNavigation {
    pub fn matches(&self, observation: &EditorObservation) -> bool {
        self.owner == observation.snapshot.text.owner
            && self.based_on == observation.revision
            && observation.snapshot.marked.is_none()
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditorNavigationKey {
    Up,
    Down,
    Enter,
    Tab,
    Escape,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EditorNavigated {
    pub key: EditorNavigationKey,
    pub observation: EditorObservation,
}
pub fn navigation_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(EditorNavigated)).expect("editor navigation schema")
}

/// Submit carries the same original editor observation as a change; it grants
/// no text read and adds no second buffer or command owner.
pub fn reduce(
    call: crate::ComponentCall<'_>,
) -> crate::Result<crate::ComponentOutput<TextareaView>> {
    let previous_persistence = match &call {
        crate::ComponentCall::Mount { .. } => None,
        crate::ComponentCall::Update { state, .. } | crate::ComponentCall::Event { state, .. } => {
            Some(state["persistence"].clone())
        }
    };
    let submitted = matches!(&call, crate::ComponentCall::Event { key, .. } if *key == "submit");
    let mut file_request: Option<String> = match &call {
        crate::ComponentCall::Mount { .. } => None,
        crate::ComponentCall::Update { state, .. } | crate::ComponentCall::Event { state, .. } => {
            serde_json::from_value(state["fileRequest"].clone()).expect("native file paste request")
        }
    };
    let mut files = None;
    let mut checkpoint = None;
    let mut navigation = None;
    let call = match call {
        crate::ComponentCall::Event {
            state,
            props,
            key: "navigate",
            input,
            origin,
        } => {
            if !matches!(origin, crate::EventOrigin::Raw) {
                return Err(crate::Error::Invalid(
                    "editor navigation requires native origin".into(),
                ));
            }
            navigation = Some(input.clone());
            crate::ComponentCall::Update { state, props }
        }
        crate::ComponentCall::Event {
            state,
            props,
            key,
            input,
            origin,
        } if matches!(key, "file_choose" | "file_observe") => {
            if !matches!(origin, crate::EventOrigin::Raw) {
                return Err(crate::Error::Invalid(
                    "file paste requires native origin".into(),
                ));
            }
            if key == "file_choose" {
                let typed: TextareaProps = serde_json::from_value(props.clone())
                    .map_err(|error| crate::Error::Invalid(error.to_string()))?;
                if typed.disabled || typed.read_only || typed.file_paste.is_none() {
                    return Err(crate::Error::Invalid("file paste disabled".into()));
                }
                let choose: FileChoose = serde_json::from_value(input.clone())
                    .map_err(|error| crate::Error::Invalid(error.to_string()))?;
                file_request = Some(choose.request);
            } else {
                files = Some(input.clone());
            }
            crate::ComponentCall::Update { state, props }
        }
        crate::ComponentCall::Event {
            state,
            props,
            key: "checkpoint",
            input,
            origin,
        } => {
            if !matches!(origin, crate::EventOrigin::Raw) {
                return Err(crate::Error::Invalid(
                    "checkpoint observation requires native origin".into(),
                ));
            }
            checkpoint = Some(input.clone());
            crate::ComponentCall::Update { state, props }
        }
        other => other,
    };
    let mut output = crate::editor::reduce::<TextareaProps>(call)?;
    if let Some(policy) = &output.frame.view.persistence {
        policy.validate()?;
    }
    let persistence = output
        .frame
        .view
        .persistence
        .as_ref()
        .map(|policy| serde_json::json!([policy.key, policy.format, policy.data_source]));
    let identity = serde_json::json!(persistence);
    if previous_persistence
        .as_ref()
        .is_some_and(|previous| *previous != identity)
    {
        return Err(crate::Error::Invalid(
            "editor persistence identity requires a new mount".into(),
        ));
    }
    output.frame.state["persistence"] = identity;
    if output.frame.view.inline_tokens.iter().any(|literal| {
        literal.is_empty() || literal.chars().any(|c| c.is_whitespace() || c.is_control())
    }) {
        return Err(crate::Error::Invalid(
            "invalid inline token literals".into(),
        ));
    }
    let rows = output.frame.view.terminal_rows;
    if rows.min == 0
        || rows.min > rows.max
        || rows
            .max_viewport_fraction
            .is_some_and(|fraction| !fraction.is_finite() || fraction <= 0.0 || fraction > 1.0)
    {
        return Err(crate::Error::Invalid(
            "invalid terminal textarea row range".into(),
        ));
    }
    if submitted {
        for event in &mut output.events {
            event.name = "submitted".into();
        }
    }
    output.frame.state["fileRequest"] = serde_json::json!(file_request);
    if let Some(value) = files {
        output.events.push(crate::Emission {
            name: "files".into(),
            value,
        });
    }
    if let Some(value) = checkpoint {
        output.events.push(crate::Emission {
            name: "checkpoint".into(),
            value,
        });
    }
    if let Some(value) = navigation {
        output.events.push(crate::Emission {
            name: "navigated".into(),
            value,
        });
    }
    Ok(output.map_view(|props| TextareaView {
        props,
        file_request,
    }))
}

pub fn textarea_contract() -> Contract {
    Contract {
        name: "maka.ui.Textarea".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: serde_json::to_value(schemars::schema_for!(TextareaProps))
                .expect("editor props schema")
                .into(),
            events: BTreeMap::from([
                ("changed".into(), observation_schema().into()),
                ("submitted".into(), observation_schema().into()),
                ("navigated".into(), navigation_schema().into()),
                ("files".into(), file_paste_schema().into()),
                (
                    "checkpoint".into(),
                    crate::checkpoint::observation_schema().into(),
                ),
            ]),
            captures: BTreeMap::from([
                (
                    "snapshot".into(),
                    Capture {
                        input: serde_json::to_value(schemars::schema_for!(EditorCapture))
                            .expect("capture schema")
                            .into(),
                        output: maka_plugin_core::contract::CaptureOutput::Resource {
                            resource: text_resource_type(),
                        },
                    },
                ),
                (
                    "file".into(),
                    Capture {
                        input: serde_json::to_value(schemars::schema_for!(FileCapture))
                            .expect("file paste capture")
                            .into(),
                        output: maka_plugin_core::contract::CaptureOutput::Resource {
                            resource: maka_plugin_core::contract::binary_resource_type(),
                        },
                    },
                ),
            ]),
        },
    }
}

pub const IMPLEMENTATION: &str = "maka.ui.textarea";

pub fn native() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        IMPLEMENTATION,
        serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        [("Textarea", textarea_contract())],
    )
}

#[cfg(test)]
mod inline_token_tests {
    use super::*;

    #[test]
    fn complete_inline_catalog_matches_the_exact_long_tail_literal() {
        let mut literals: Vec<_> = (0..4096)
            .map(|index| format!("/skill:item-{index}"))
            .collect();
        let tail = format!("/skill:{}", "技能".repeat(100));
        assert!(tail.len() > 512);
        literals.push(tail.clone());
        let prefix = "普通 /skill:missing ";
        let text = format!("{prefix}{tail} {tail}-extra");
        assert_eq!(
            inline_token_ranges(&text, &literals),
            vec![prefix.len()..prefix.len() + tail.len()]
        );
        assert_eq!(
            &text[inline_token_ranges(&text, &literals)[0].clone()],
            tail
        );
    }

    #[test]
    fn inline_literals_match_whole_unicode_words_only() {
        let text = "🙂 /skill:foo /skill:foo-extra\n/skill:技能 後";
        let literals = vec!["/skill:foo".into(), "/skill:技能".into()];
        let spans = inline_token_ranges(text, &literals);
        assert_eq!(
            spans
                .iter()
                .map(|range| &text[range.clone()])
                .collect::<Vec<_>>(),
            vec!["/skill:foo", "/skill:技能"]
        );
        assert_eq!(spans[0].start, "🙂 ".len());
        assert_eq!(
            inline_token_ranges("/skill:foo /skill:foo", &literals),
            vec![0..10, 11..21]
        );
        assert!(inline_token_ranges(text, &[]).is_empty());

        assert!(inline_token_ranges(" \u{301}x", &["\u{301}x".into()]).is_empty());
    }
}
