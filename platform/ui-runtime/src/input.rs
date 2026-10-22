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

//! Fixed native single-line exports; secrets never become observation values.
use crate::{ComponentCall, ComponentOutput, Emission, editor::*};
use maka_plugin_core::contract::{Capture, Contract, Interface, Transport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputProps {
    pub appearance: crate::appearance::InputAppearance,
    #[serde(default)]
    pub menu: Option<crate::textarea::TextareaMenu>,
    #[schemars(length(min = 1))]
    pub accessibility_label: String,
    pub read_only: bool,
    pub disabled: bool,
    #[schemars(range(min = -1, max = 32767))]
    pub tab_index: i16,
    /// Offer native focus once when this original input becomes visible and
    /// enabled. Stable updates and viewport scrolling do not repeat the offer.
    #[serde(default)]
    pub auto_focus: bool,
    /// UTF-8 byte capacity, fixed for this original native editor mount.
    #[serde(default = "default_max_bytes")]
    #[schemars(range(min = 1))]
    pub max_bytes: u32,
    /// Terminal bracketed-paste policy. Other native insertion remains literal.
    #[serde(default)]
    pub terminal_paste: TerminalPaste,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub command: Option<EditorCommand>,
    /// Clipboard default for physical Enter at this original editor revision.
    /// Programmatic submitted events never execute this native default.
    #[serde(default)]
    pub submit_copy: Option<SubmitCopy>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmitCopy {
    pub basis: EditorTextRef,
    #[schemars(length(min = 1))]
    pub text: String,
}
fn default_max_bytes() -> u32 {
    65536
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPaste {
    #[default]
    RejectControls,
    Spaces,
}
impl crate::editor::Props for InputProps {
    fn command(&self) -> Option<&EditorCommand> {
        self.command.as_ref()
    }
    fn set_command(&mut self, command: Option<EditorCommand>) {
        self.command = command;
    }
}
pub fn representable(text: &str) -> bool {
    !text.chars().any(char::is_control)
}
pub fn submitted_schema() -> serde_json::Value {
    json!({"type":"object","additionalProperties":false,"properties":{}})
}
pub fn reduce(call: ComponentCall<'_>) -> crate::Result<ComponentOutput<InputProps>> {
    let (props, previous) = match &call {
        ComponentCall::Mount { props } => (*props, None),
        ComponentCall::Update { props, state } | ComponentCall::Event { props, state, .. } => {
            (*props, Some(*state))
        }
    };
    let typed: InputProps = serde_json::from_value(props.clone())
        .map_err(|error| crate::Error::Invalid(error.to_string()))?;
    let limit = typed.max_bytes;
    if typed
        .submit_copy
        .as_ref()
        .is_some_and(|copy| copy.text.is_empty())
    {
        return Err(crate::Error::Invalid(
            "input submit copy must be nonempty".into(),
        ));
    }
    if limit == 0
        || previous.is_some_and(|state| state["maxBytes"].as_u64() != Some(u64::from(limit)))
    {
        return Err(crate::Error::Invalid(
            "input byte capacity must be valid and remain fixed for its original mount".into(),
        ));
    }
    let mut output = if let ComponentCall::Event {
        state,
        props,
        key: "submit",
        ..
    } = call
    {
        let mut output =
            crate::editor::reduce::<InputProps>(ComponentCall::Update { state, props })?;
        output.events.push(Emission {
            name: "submitted".into(),
            value: json!({}),
        });
        output
    } else {
        crate::editor::reduce(call)?
    };
    output.frame.state["maxBytes"] = json!(limit);
    Ok(output)
}
pub fn text_input_contract() -> Contract {
    contract("maka.ui.TextInput")
}
pub fn secret_input_contract() -> Contract {
    contract("maka.ui.SecretInput")
}
fn contract(name: &str) -> Contract {
    Contract {
        name: name.into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: serde_json::to_value(schemars::schema_for!(InputProps))
                .expect("input props")
                .into(),
            events: BTreeMap::from([
                ("changed".into(), observation_schema().into()),
                ("submitted".into(), submitted_schema().into()),
            ]),
            captures: BTreeMap::from([(
                "snapshot".into(),
                Capture {
                    input: serde_json::to_value(schemars::schema_for!(EditorCapture))
                        .expect("input capture")
                        .into(),
                    output: maka_plugin_core::contract::CaptureOutput::Resource {
                        resource: text_resource_type(),
                    },
                },
            )]),
        },
    }
}

pub const IMPLEMENTATION: &str = "maka.ui.input";

pub fn native() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        IMPLEMENTATION,
        serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        [
            ("TextInput", text_input_contract()),
            ("SecretInput", secret_input_contract()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capacity_is_validated_and_cannot_replace_the_original_mount_policy() {
        let appearance = json!({"text":{"font":{"family":"mono","fallbacks":[],"size":14,"lineHeight":1,"weight":400},"foreground":4294967295u32},"mutedForeground":4294967295u32,"background":0,"selection":0,"caret":4294967295u32,"border":0,"focusedBorder":0,"borderWidth":0,"radius":0,"padding":{"top":0,"right":0,"bottom":0,"left":0}});
        let mut props = json!({"appearance":appearance,"accessibilityLabel":"Find","readOnly":false,"disabled":false,"tabIndex":0,"maxBytes":512});
        let original = reduce(ComponentCall::Mount { props: &props }).unwrap();
        for limit in [0, 513, 65537] {
            props["maxBytes"] = json!(limit);
            assert!(
                reduce(ComponentCall::Update {
                    props: &props,
                    state: &original.frame.state
                })
                .is_err()
            );
        }
        props["maxBytes"] = json!(512);
        let next = reduce(ComponentCall::Update {
            props: &props,
            state: &original.frame.state,
        })
        .unwrap();
        assert_eq!(next.frame.state["maxBytes"], 512);
        {
            let text = String::new();
            props["submitCopy"] =
                json!({"basis":{"owner":"original","revision":0,"bytes":0},"text":text});
            assert!(reduce(ComponentCall::Mount { props: &props }).is_err());
        }
        props["submitCopy"]["text"] = json!("界".repeat(21846));
        assert!(reduce(ComponentCall::Mount { props: &props }).is_ok());
    }
}
