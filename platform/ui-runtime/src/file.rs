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

//! User-selected immutable files; paths and mutable buffers never enter guest state.
mod selection;
use crate::{ComponentCall, ComponentOutput, Emission, EventOrigin, Frame};
use maka_plugin_core::contract::{
    Capture, CaptureOutput, Contract, Interface, Transport, binary_resource_type,
};
use schemars::JsonSchema;
pub use selection::{FileSelection, FileSnapshot, FileSnapshotWriter, read_selected};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePickerProps {
    pub appearance: crate::appearance::InputAppearance,
    #[schemars(length(min = 1))]
    pub label: String,
    #[schemars(length(min = 1))]
    pub path_label: String,
    #[schemars(length(min = 1))]
    pub browse_label: String,
    #[schemars(length(min = 1))]
    pub parent_label: String,
    #[schemars(length(min = 1))]
    pub empty_label: String,
    #[schemars(length(min = 1))]
    pub truncated_label: String,
    pub placeholder: String,
    pub disabled: bool,
    #[schemars(range(min = -1, max = 32767))]
    pub tab_index: i16,
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileDescriptor {
    pub name: String,
    pub bytes: usize,
    #[schemars(length(min = 71, max = 71))]
    pub digest: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilePhase {
    Empty,
    Choosing,
    Ready,
    Failed,
}
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema, thiserror::Error,
)]
#[serde(rename_all = "snake_case")]
pub enum FileError {
    #[error("selected path is not an absolute regular file")]
    InvalidFile,
    #[error("selected file could not be read")]
    ReadFailed,
    #[error("selected file exceeds the byte limit")]
    LimitExceeded,
    #[error("native resource budget exhausted")]
    BudgetExhausted,
    #[error("native file worker failed")]
    WorkerFailed,
    #[error("native clipboard has no supported content or is unavailable")]
    ClipboardUnavailable,
    #[error("clipboard text cannot be inserted into the original draft")]
    PasteRejected,
}
impl From<crate::budget::BudgetExhausted> for FileError {
    fn from(_: crate::budget::BudgetExhausted) -> Self {
        Self::BudgetExhausted
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileObservation {
    #[schemars(length(min = 1))]
    pub owner: String,
    pub revision: u64,
    pub phase: FilePhase,
    pub file: Option<FileDescriptor>,
    pub error: Option<FileError>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileCapture {
    #[schemars(length(min = 1))]
    pub owner: String,
    pub revision: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FileChoose {
    #[schemars(length(min = 1))]
    pub request: String,
}
/// Committed mechanical intent; only a renderer's matching private gesture can act.
#[derive(Clone)]
pub struct FileView {
    pub props: FilePickerProps,
    pub request: Option<String>,
}
pub fn observation_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(FileObservation)).expect("file observation")
}
pub fn choose_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(FileChoose)).expect("file choose")
}
pub fn file_picker_contract() -> Contract {
    Contract {
        name: "maka.ui.FilePicker".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Component {
            props: serde_json::to_value(schemars::schema_for!(FilePickerProps))
                .expect("file props")
                .into(),
            events: BTreeMap::from([("changed".into(), observation_schema().into())]),
            captures: BTreeMap::from([(
                "snapshot".into(),
                Capture {
                    input: serde_json::to_value(schemars::schema_for!(FileCapture))
                        .expect("file capture")
                        .into(),
                    output: CaptureOutput::Resource {
                        resource: binary_resource_type(),
                    },
                },
            )]),
        },
    }
}
pub fn reduce(call: ComponentCall<'_>) -> crate::Result<ComponentOutput<FileView>> {
    let (props, revision, request, observation, events) = match call {
        ComponentCall::Mount { props } => (props, 0, Value::Null, Value::Null, vec![]),
        ComponentCall::Update { state, props } => (
            props,
            state["revision"].as_u64().expect("file UI revision") + 1,
            state["request"].clone(),
            state["observation"].clone(),
            vec![],
        ),
        ComponentCall::Event {
            state,
            props,
            key,
            input,
            origin,
        } => {
            if !matches!(origin, EventOrigin::Raw) {
                return Err(crate::Error::Invalid(
                    "file input requires native origin".into(),
                ));
            }
            let revision = state["revision"].as_u64().expect("file UI revision") + 1;
            match key {
                "choose" => {
                    let typed: FilePickerProps = serde_json::from_value(props.clone())
                        .map_err(|e| crate::Error::Invalid(e.to_string()))?;
                    if typed.disabled {
                        return Err(crate::Error::Invalid("file picker disabled".into()));
                    }
                    let choose: FileChoose = serde_json::from_value(input.clone())
                        .map_err(|e| crate::Error::Invalid(e.to_string()))?;
                    (
                        props,
                        revision,
                        json!(choose.request),
                        state["observation"].clone(),
                        vec![],
                    )
                }
                "observe" => (
                    props,
                    revision,
                    state["request"].clone(),
                    input.clone(),
                    vec![Emission {
                        name: "changed".into(),
                        value: input.clone(),
                    }],
                ),
                _ => return Err(crate::Error::Invalid("unknown file input".into())),
            }
        }
    };
    let typed =
        serde_json::from_value(props.clone()).map_err(|e| crate::Error::Invalid(e.to_string()))?;
    let request_value = serde_json::from_value(request.clone()).expect("file request marker");
    Ok(Frame {
        revision,
        state: json!({"revision": revision, "request":request, "observation": observation}),
        surface: None,
        view: FileView {
            props: typed,
            request: request_value,
        },
    }
    .output(events))
}

pub const IMPLEMENTATION: &str = "maka.ui.file";

pub fn native() -> maka_plugin_core::package::NativeDefinition {
    maka_plugin_core::package::NativeDefinition::new(
        IMPLEMENTATION,
        serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        [("FilePicker", file_picker_contract())],
    )
}
