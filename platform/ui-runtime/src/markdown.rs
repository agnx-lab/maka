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

//! Read-only Markdown presentation. Source is content, never a resource grant.
use crate::appearance::{Color, TextAppearance};
use maka_plugin_core::contract::{Contract, Interface, Transport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkdownAppearance {
    #[serde(default)]
    pub terminal: Option<crate::scene::terminal::Palette>,
    pub body: TextAppearance,
    pub code: TextAppearance,
    /// Typography for heading levels 1 through 6, in source order.
    pub headings: [TextAppearance; 6],
    pub muted: Color,
    pub link: Color,
    pub selection: Color,
    pub code_background: Color,
    pub table_background: Color,
    pub border: Color,
    pub dark: bool,
    #[schemars(range(min = 0, max = 128))]
    /// Paragraph spacing in logical pixels.
    pub paragraph_gap: f32,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarkdownProps {
    #[serde(default)]
    pub ascii: bool,
    #[serde(default)]
    pub animate: bool,
    pub source: String,
    pub appearance: MarkdownAppearance,
    pub selectable: bool,
    pub disabled: bool,
    /// Whether pointer selection also moves keyboard focus to this document.
    #[serde(default = "pointer_focus_default")]
    pub focus_on_pointer: bool,
    /// The committed document decides native URL opening before guest work.
    #[serde(default = "pointer_focus_default")]
    pub open_web_links: bool,
    #[serde(default = "pointer_focus_default")]
    pub bubble_links: bool,
}
fn pointer_focus_default() -> bool {
    true
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkdownLink {
    pub url: String,
}
pub fn link_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(MarkdownLink)).expect("Markdown link schema")
}
pub fn markdown_contract() -> Contract {
    Contract {
        name: "maka.ui.Markdown".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: serde_json::to_value(schemars::schema_for!(MarkdownProps))
                .expect("Markdown props")
                .into(),
            events: BTreeMap::from([("link".into(), link_schema().into())]),
            captures: BTreeMap::new(),
        },
    }
}

/// Complete source-to-readable-text conversion using the presenter's Markdown semantics.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkdownTextInput {
    pub source: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MarkdownTextResult {
    Ready { text: String },
}
pub fn text_contract() -> Contract {
    use maka_plugin_core::contract::{Method, MethodMode};
    Contract {
        name: "maka.text.Markdown".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Service {
            methods: BTreeMap::from([(
                "plain".into(),
                Method {
                    progress: None,
                    mode: MethodMode::Query,
                    input: serde_json::to_value(schemars::schema_for!(MarkdownTextInput))
                        .expect("Markdown text input")
                        .into(),
                    output: serde_json::to_value(schemars::schema_for!(MarkdownTextResult))
                        .expect("Markdown text output")
                        .into(),
                    resources: BTreeMap::new(),
                },
            )]),
        },
    }
}

pub const IMPLEMENTATION: &str = "maka.ui.markdown";

pub fn native() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        IMPLEMENTATION,
        serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        [("Markdown", markdown_contract())],
    )
}

pub fn native_markdown_text() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        "maka.text.markdown.tui",
        serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        [("MarkdownText", text_contract())],
    )
}
