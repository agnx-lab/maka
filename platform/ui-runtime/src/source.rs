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

//! Literal selectable source; no markup interpretation or resource authority.
use crate::appearance::{Color, TextAppearance};
use maka_plugin_core::contract::{Contract, Interface, Transport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceTextProps {
    #[serde(default)]
    pub animate: bool,
    #[serde(default)]
    pub terminal: Option<crate::scene::terminal::Palette>,
    pub source: String,
    pub appearance: TextAppearance,
    pub selection: Color,
    pub disabled: bool,
    /// Whether pointer selection also moves keyboard focus to this source.
    #[serde(default = "pointer_focus_default")]
    pub focus_on_pointer: bool,
}
fn pointer_focus_default() -> bool {
    true
}
pub fn source_contract() -> Contract {
    Contract {
        name: "maka.ui.SourceText".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: serde_json::to_value(schemars::schema_for!(SourceTextProps))
                .expect("source props"),
            events: BTreeMap::new(),
            captures: BTreeMap::new(),
        },
    }
}

pub const IMPLEMENTATION: &str = "maka.ui.source";

pub fn native() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        IMPLEMENTATION,
        serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        [("SourceText", source_contract())],
    )
}
