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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(rename = "WorkHubPreset")]
pub struct Preset {
    #[serde(default)]
    pub isolated_workspace: bool,
    #[schemars(length(min = 1), regex(pattern = "^[a-zA-Z0-9._:-]+$"))]
    pub id: String,
    #[schemars(length(min = 1))]
    pub name: String,
    pub description: String,
    #[schemars(length(min = 1))]
    pub worker: String,
    /// Exact installed names; an empty list permits reasoning without tools.
    #[schemars(extend("uniqueItems" = true))]
    pub tools: Vec<String>,
    pub model: maka_agent_facts::configuration::ConnectionTarget,
    pub thinking_level: Option<maka_agent_facts::execution::ThinkingLevel>,
    pub enabled: bool,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "WorkHubPresets")]
pub struct Presets {
    #[schemars(with = "maka_plugin_core::wire_schema::RequiredNullable<u64>")]
    pub revision: Option<u64>,
    pub entries: Vec<Preset>,
}
#[derive(Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(rename = "WorkHubPresetsResult")]
pub enum Result {
    Saved { snapshot: Presets },
    Conflict { snapshot: Presets },
    Invalid,
}
