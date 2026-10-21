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

//! Conversation-scoped graph decisions; WorkHub retains task execution authority.
pub use maka_graph::schedule::Work as WorkDefinition;
use maka_graph::{
    Epoch, GraphId, Mode, WorkId,
    schedule::{Finish, Target},
    view::{Status, WorkState},
};
use maka_plugin_core::{
    contract::{Method, MethodMode},
    operation::RecoveryRef,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphRead")]
pub struct Read {
    #[schemars(length(min = 1))]
    pub session_id: String,
    pub graph_id: Option<GraphId>,
    pub after: Option<WorkId>,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphView")]
pub struct View {
    pub enabled: bool,
    pub current_epoch: Option<Epoch>,
    pub selected_mode: Option<Mode>,
    pub epoch: Option<Epoch>,
    pub revision: u64,
    pub status: Status,
    pub quiescent: bool,
    #[schemars(length(max = 16))]
    pub work: Vec<Work>,
    pub after: Option<WorkId>,
    pub finish: Option<Finish>,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphWork")]
pub struct Work {
    pub work_id: WorkId,
    pub task_id: String,
    pub target: Target,
    pub status: WorkState,
    pub instruction_excerpt: String,
    pub instruction_truncated: bool,
    pub result_id: Option<String>,
    pub failure: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphWorkRead")]
pub struct WorkRead {
    #[schemars(length(min = 1))]
    pub session_id: String,
    pub graph_id: GraphId,
    pub work_id: WorkId,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphResultRead")]
pub struct ResultRead {
    #[schemars(length(min = 1))]
    pub session_id: String,
    pub graph_id: GraphId,
    pub work_id: WorkId,
    #[schemars(length(min = 1))]
    pub result_id: String,
    #[schemars(range(max = 9_007_199_254_740_991u64))]
    pub offset: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphResult")]
pub struct ResultPage {
    pub work_id: WorkId,
    pub result_id: String,
    pub execution: crate::contract::Execution,
    pub text: String,
    pub next_offset: Option<u64>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphHistoryRead")]
pub struct HistoryRead {
    #[schemars(length(min = 1))]
    pub session_id: String,
    pub before: Option<u64>,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphHistory")]
pub struct History {
    #[schemars(length(max = 32))]
    pub epochs: Vec<Epoch>,
    pub current_epoch: Option<u64>,
    pub next_before: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphChange")]
pub struct Change {
    #[schemars(length(min = 1))]
    pub session_id: String,
    pub expected_graph_id: Option<GraphId>,
    pub action: Action,
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphAction")]
pub enum Action {
    Start { mode: Mode },
    Stop,
    Leave,
}
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[schemars(rename = "WorkHubGraphChanged")]
pub enum Changed {
    Committed { epoch: Epoch },
    Conflict { current: Option<Epoch> },
    Rejected { reason: String },
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "WorkHubGraphReceiptRead")]
pub struct ReceiptRead {
    pub reference: RecoveryRef,
}

pub(super) fn methods() -> Vec<(String, Method)> {
    let method = |name: &str, mode, input, output| {
        (
            name.into(),
            Method {
                mode,
                input,
                output,
                progress: None,
                resources: BTreeMap::new(),
            },
        )
    };
    vec![
        method(
            "graph",
            MethodMode::Query,
            schemars::schema_for!(Read).into(),
            schemars::schema_for!(View).into(),
        ),
        method(
            "watchGraph",
            MethodMode::Subscription,
            schemars::schema_for!(Read).into(),
            schemars::schema_for!(View).into(),
        ),
        method(
            "graphWork",
            MethodMode::Query,
            schemars::schema_for!(WorkRead).into(),
            schemars::schema_for!(maka_graph::schedule::Work).into(),
        ),
        method(
            "graphResult",
            MethodMode::Query,
            schemars::schema_for!(ResultRead).into(),
            schemars::schema_for!(ResultPage).into(),
        ),
        method(
            "graphHistory",
            MethodMode::Query,
            schemars::schema_for!(HistoryRead).into(),
            schemars::schema_for!(History).into(),
        ),
        method(
            "changeGraph",
            MethodMode::Mutation,
            schemars::schema_for!(Change).into(),
            schemars::schema_for!(Changed).into(),
        ),
        method(
            "graphReceipt",
            MethodMode::Query,
            schemars::schema_for!(ReceiptRead).into(),
            schemars::schema_for!(Option<Changed>).into(),
        ),
    ]
}
