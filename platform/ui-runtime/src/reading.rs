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

//! Native reading evidence and commands. Layout remains the selected scene language.
use maka_plugin_core::contract::{Capture, CaptureOutput, Contract, Interface, Transport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingProps {
    /// The presenter's scene language; each requires a scrolling column Stack.
    /// Its scroll policy and layout are the only presentation authority.
    pub content: Value,
    /// Omission retains an accepted command; numbers increase per original mount.
    pub command: Option<ReadingCommand>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingCommand {
    #[schemars(range(min = 1))]
    pub sequence: u64,
    pub target: ReadingTarget,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadingTarget {
    Start,
    End,
    /// Find over native readable descendants, including folded slots. SourceText uses
    /// original literal bytes; Markdown uses its width-independent rendered text.
    Find {
        query: String,
        step: FindStep,
    },
    ClearFind,
    /// Move the original terminal viewport by physical rows.
    ScrollRows {
        rows: i32,
    },
    /// Reveal a UTF-8 byte in the descendant's native readable text, as returned by Find.
    RevealSource {
        #[schemars(length(min = 1))]
        path: Vec<String>,
        offset: u32,
        /// If supplied, reveal only while this accepted find retains its original physical intent.
        #[serde(default)]
        find_sequence: Option<u64>,
    },
    /// TUI: focus and minimally reveal an enabled scene Item. Settlement waits
    /// for its successful presentation; this never submits the item.
    FocusItem {
        #[schemars(length(min = 1))]
        key: String,
    },
    RevealChild {
        #[schemars(length(min = 1))]
        key: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindStep {
    First,
    Next,
    Previous,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingMatch {
    #[schemars(length(min = 1))]
    pub path: Vec<String>,
    pub offset: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingFind {
    pub sequence: u64,
    pub count: u32,
    pub limited: bool,
    /// One-based current match, or null when no match remains.
    pub index: Option<u32>,
    pub active: Option<ReadingMatch>,
}
impl ReadingCommand {
    pub fn valid_target(&self) -> bool {
        match &self.target {
            ReadingTarget::Find { .. } => true,
            ReadingTarget::FocusItem { key } => !key.is_empty(),
            ReadingTarget::ScrollRows { .. } => true,
            ReadingTarget::RevealSource {
                path,
                offset: _,
                find_sequence,
            } => {
                find_sequence.is_none_or(|sequence| sequence > 0)
                    && !path.is_empty()
                    && path.iter().all(|key| !key.is_empty())
            }
            _ => true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadingStatus {
    Applied,
    Cancelled,
    Rejected,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingReceipt {
    pub sequence: u64,
    pub status: ReadingStatus,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingViewport {
    /// Explicit reading intent used by the actually painted children.
    pub reading_revision: u64,
    #[serde(default)]
    pub at_start: bool,
    pub at_end: bool,
    pub visible_child_keys: Vec<String>,
}
/// Most recent physical scroll. Layout changes and programmatic landings do not
/// create a new request, so a product can load adjacent content without loops.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingScroll {
    pub reading_revision: u64,
    pub direction: ReadingScrollDirection,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadingScrollDirection {
    Up,
    Down,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadingObservation {
    /// Immediate native intent; can be newer than the painted viewport.
    pub reading_revision: u64,
    pub viewport: Option<ReadingViewport>,
    #[serde(default)]
    pub scroll: Option<ReadingScroll>,
    /// Direct content child containing the native selection focus. Renderers
    /// without this observation return null. This key conveys no read authority.
    pub selected_child_key: Option<String>,
    pub pending_command: Option<u64>,
    /// Only the most recently settled command is retained, without a ledger.
    pub receipt: Option<ReadingReceipt>,
    pub find: Option<ReadingFind>,
}
pub fn observation_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(ReadingObservation)).expect("reading observation")
}
pub fn reading_contract() -> Contract {
    Contract {
        name: "maka.ui.ReadingArea".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: serde_json::to_value(schemars::schema_for!(ReadingProps))
                .expect("reading props"),
            events: BTreeMap::from([
                ("observed".into(), observation_schema()),
                (
                    "input".into(),
                    serde_json::to_value(schemars::schema_for!(crate::abi::ComponentInput))
                        .expect("reading input"),
                ),
            ]),
            captures: BTreeMap::from([(
                "tail".into(),
                Capture {
                    input: serde_json::json!({"type":"null"}),
                    output: CaptureOutput::Continuation,
                },
            )]),
        },
    }
}

pub const IMPLEMENTATION: &str = "maka.ui.reading";

pub fn native() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        IMPLEMENTATION,
        serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        [("ReadingArea", reading_contract())],
    )
}
