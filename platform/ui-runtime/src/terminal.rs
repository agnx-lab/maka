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

//! Native terminal presentation. Controller values correlate UI work; only the
//! independently bound Host service can authorize or perform process effects.
pub mod state;
use crate::appearance::{Color, TextAppearance};
use crate::{ComponentCall, ComponentOutput, Emission, Frame};
use input::InputAction;
pub use maka_agent_facts::shell_result::ShellOutput;
pub use maka_agent_facts::terminal::TerminalSize;
pub use maka_agent_facts::terminal::input;
use maka_plugin_core::contract::{Contract, Interface, Transport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalViewProps {
    /// Changed when the selected canonical resource changes, including read-only views.
    #[schemars(length(min = 1))]
    pub source: String,
    #[schemars(length(min = 1))]
    pub accessibility_label: String,
    pub output: Option<ShellOutput>,
    pub controller: Option<Uuid>,
    pub acknowledgement: Option<TerminalInputAcknowledgement>,
    pub appearance: TextAppearance,
    pub background: Color,
    pub caret: Color,
    #[schemars(range(min = 1))]
    pub rows: u16,
    #[schemars(range(min = -1, max = 32767))]
    pub tab_index: i16,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalInputAcknowledgement {
    pub owner: Uuid,
    pub controller: Uuid,
    #[schemars(range(min = 1, max = 9007199254740991u64))]
    pub sequence: u64,
    /// True only after the exact original mutation has a successful result.
    pub accepted: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalInputBatch {
    pub controller: Uuid,
    #[schemars(range(min = 1, max = 9007199254740991u64))]
    pub sequence: u64,
    #[schemars(length(max = 64))]
    pub actions: Vec<InputAction>,
    pub size: Option<TerminalSize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalViewError {
    InputFull,
    InputFailed,
    BudgetExhausted,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminalViewObservation {
    pub owner: Uuid,
    pub revision: u64,
    pub source: String,
    pub controller: Option<Uuid>,
    pub captured: bool,
    pub batch: Option<TerminalInputBatch>,
    pub error: Option<TerminalViewError>,
}
pub fn observation_schema() -> serde_json::Value {
    schemars::schema_for!(TerminalViewObservation).into()
}
pub fn terminal_view_contract() -> Contract {
    Contract {
        name: "maka.ui.TerminalView".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: schemars::schema_for!(TerminalViewProps).into(),
            events: BTreeMap::from([("observed".into(), observation_schema())]),
            captures: BTreeMap::new(),
        },
    }
}
pub fn reduce(call: ComponentCall<'_>) -> crate::Result<ComponentOutput<TerminalViewProps>> {
    let (revision, props, events) = match call {
        ComponentCall::Mount { props } => (0, props, vec![]),
        ComponentCall::Update { state, props } => {
            (state["revision"].as_u64().unwrap() + 1, props, vec![])
        }
        ComponentCall::Event {
            state,
            props,
            key: "observe",
            input,
            ..
        } => (
            state["revision"].as_u64().unwrap() + 1,
            props,
            vec![Emission {
                name: "observed".into(),
                value: input.clone(),
            }],
        ),
        ComponentCall::Event { .. } => {
            return Err(crate::Error::Invalid("unknown terminal input".into()));
        }
    };
    let props: TerminalViewProps =
        serde_json::from_value(props.clone()).map_err(|e| crate::Error::Invalid(e.to_string()))?;
    if serde_json::to_vec(&props.output)
        .map_err(|e| crate::Error::Invalid(e.to_string()))?
        .len()
        > 52 * 1024
    {
        return Err(crate::Error::Invalid(
            "terminal canonical output exceeds presentation limit".into(),
        ));
    }
    Ok(Frame {
        revision,
        state: serde_json::json!({"revision":revision}),
        surface: None,
        view: props,
    }
    .output(events))
}

pub const IMPLEMENTATION: &str = "maka.ui.terminal";

pub fn native() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        IMPLEMENTATION,
        serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        [("TerminalView", terminal_view_contract())],
    )
}
