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

//! Requests to the explicitly connected foreground UI, never direct Root access.
use maka_plugin_core::{
    contract::{Contract, Interface, Method, MethodMode, Transport},
    deployment::management::*,
    package::NativeDefinition,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
pub const IMPLEMENTATION: &str = "maka.application.authoring";
pub const EXPORT: &str = "Authoring";

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", content = "input", rename_all = "camelCase")]
#[schemars(rename = "UiAuthoringCommand", extend("type" = "object"))]
pub enum Command {
    Instances(DeploymentInstances),
    ReadDocument(DeploymentDocumentRead),
    ReadPackage(PackageSourceRead),
    InstallSource(PackageSourceInstall),
    InstallUrl(PackageUrlInstall),
    ReplaceSource(DeploymentSourceReplace),
    EditComposition(CompositionEdit),
    Configure(InstanceConfigure),
    BindingCandidates(BindingCandidatesRead),
    PackageSettings(PackageSettingsRead),
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "UiAuthoringRequestId")]
pub struct RequestId {
    pub id: String,
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "UiAuthoringCompletion")]
pub struct Completion {
    pub id: String,
    pub outcome: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
#[schemars(rename = "UiAuthoringState")]
pub enum State {
    Queued,
    /// The foreground has claimed this request. A lost foreground leaves an
    /// unconfirmed result; it must not dispatch this request again.
    Claimed,
    Completed {
        outcome: Value,
    },
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "UiAuthoringRequest")]
pub struct Request {
    pub id: String,
    pub command: Command,
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "UiAuthoringClaim")]
pub struct Claim {
    pub request: Option<Request>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
fn method<I: JsonSchema, O: JsonSchema>(mode: MethodMode) -> Method {
    Method {
        mode,
        input: serde_json::to_value(schemars::schema_for!(I))
            .unwrap()
            .into(),
        output: serde_json::to_value(schemars::schema_for!(O))
            .unwrap()
            .into(),
        resources: BTreeMap::new(),
        progress: None,
    }
}
pub fn contract() -> Contract {
    Contract {
        name: "maka.application.Authoring".into(),
        version: 1,
        transport: Transport::Remote,
        interface: Interface::Service {
            methods: BTreeMap::from([
                (
                    "submit".into(),
                    method::<Command, RequestId>(MethodMode::Mutation),
                ),
                (
                    "result".into(),
                    method::<RequestId, State>(MethodMode::Query),
                ),
                (
                    "watch".into(),
                    method::<Empty, Value>(MethodMode::Subscription),
                ),
                ("claim".into(), method::<Empty, Claim>(MethodMode::Mutation)),
                (
                    "complete".into(),
                    method::<Completion, bool>(MethodMode::Mutation),
                ),
            ]),
        },
    }
}
pub fn native() -> NativeDefinition {
    NativeDefinition {
        implementation: IMPLEMENTATION.into(),
        configuration: serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}).into(),
        exports: BTreeMap::from([(EXPORT.into(), contract())]),
    }
}
