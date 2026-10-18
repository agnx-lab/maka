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

use crate::{PreparedEffect, ToolCatalog, ToolDefinition};
use maka_agent_facts::tool_call::ToolRejection;
use maka_agent_facts::tools::ToolError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

pub const SEARCH: &str = "tool_search";

/// Session-owned discovery preferences, never executable capabilities. Runs
/// borrow this one source; each request still captures the current catalog.
#[derive(Clone, Default)]
pub struct LoadedTools(Arc<Mutex<BTreeSet<String>>>);

impl LoadedTools {
    pub fn clear(&self) {
        self.0.lock().unwrap().clear();
    }

    /// Immutable discovery evidence; inspection must not mutate the Session.
    pub fn snapshot(&self) -> BTreeSet<String> {
        self.0.lock().unwrap().clone()
    }
}

impl From<BTreeSet<String>> for LoadedTools {
    fn from(names: BTreeSet<String>) -> Self {
        Self(Arc::new(Mutex::new(names)))
    }
}

#[derive(Clone)]
pub struct Availability {
    catalog: ToolCatalog,
    behavior: Option<maka_agent_facts::execution::BehaviorId>,
    loaded: LoadedTools,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct SearchInput {
    #[schemars(length(min = 1))]
    query: String,
    #[serde(default = "default_limit")]
    #[schemars(range(min = 1, max = 20))]
    limit: usize,
}
fn default_limit() -> usize {
    8
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchResult {
    activated: Vec<String>,
}

impl Availability {
    pub async fn capture(
        &self,
        model: Option<maka_agent_facts::tools::ModelToolContext>,
        invocation: maka_agent_facts::event::Invocation,
        cwd: String,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<Self, ToolError> {
        let mut catalog = self.catalog.clone();
        let request = crate::request::BindingRequest {
            behavior: self.behavior.clone(),
            model,
            invocation,
            cwd,
            tools: catalog.names().into_iter().collect(),
            cancellation,
        };
        catalog.bind_native_request(request).await?;
        // Only the complete, successfully bound catalog is authoritative here.
        // Base and nested-only views would erase live DirectOnly names.
        self.loaded
            .0
            .lock()
            .unwrap()
            .retain(|name| catalog.contains(name));
        Ok(Self {
            catalog,
            behavior: self.behavior.clone(),
            loaded: self.loaded.clone(),
        })
    }

    pub fn new(catalog: ToolCatalog, loaded: LoadedTools) -> Self {
        Self {
            catalog,
            behavior: None,
            loaded,
        }
    }
    pub fn set_behavior(&mut self, behavior: maka_agent_facts::execution::BehaviorId) {
        self.behavior = Some(behavior);
    }
    pub fn enabled(&self) -> bool {
        self.catalog.discovery
            && self
                .catalog
                .definitions()
                .any(|definition| !self.direct(&definition.name))
    }
    pub fn nested(&self) -> Self {
        Self {
            catalog: self.catalog.nested(),
            behavior: self.behavior.clone(),
            loaded: self.loaded.clone(),
        }
    }

    pub fn direct_only(&self) -> ToolCatalog {
        self.catalog.direct_only()
    }
    pub fn digest(&self) -> String {
        self.catalog.digest()
    }
    pub fn snapshot(&self) -> ToolCatalog {
        if !self.enabled() {
            return self.catalog.clone();
        }
        let active = self.loaded.0.lock().unwrap();
        self.catalog
            .select(|name| self.direct(name) || active.contains(name))
    }
    /// The full, already-authorized request snapshot. Code Mode defers only
    /// descriptions, so metadata discovery never changes executable authority.
    pub fn all(&self) -> ToolCatalog {
        self.catalog.clone()
    }
    pub fn checkpoint(&self) -> maka_agent_facts::handoff::HandoffTools {
        maka_agent_facts::handoff::HandoffTools {
            catalog_digest: self.catalog.digest(),
            // Names survive handoff; implementations and grants do not. The
            // successor intersects them with its freshly bound capabilities.
            loaded: self.loaded.0.lock().unwrap().clone(),
        }
    }

    pub fn restore(
        &self,
        checkpoint: &maka_agent_facts::handoff::HandoffTools,
    ) -> Result<(), ToolError> {
        if checkpoint.catalog_digest != self.catalog.digest()
            || checkpoint
                .loaded
                .iter()
                .any(|name| name.is_empty() || name.len() > 128)
        {
            return Err(ToolError::Failed("handoff tool catalog changed".into()));
        }
        *self.loaded.0.lock().unwrap() = checkpoint.loaded.clone();
        Ok(())
    }
    pub fn definition(&self) -> Option<ToolDefinition> {
        if !self.enabled() {
            return None;
        }
        let names = self
            .catalog
            .definitions()
            .filter(|d| !self.direct(&d.name))
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>();
        Some(ToolDefinition {
            freeform: None,
            output_schema: None,
            provider: None,
            name: SEARCH.into(),
            description: format!(
                "Find tools by name or description. Matches become callable on the next model step. Loaded names persist in this Session until successful compaction or Host restart; current permissions and availability apply. Inventory: {}.",
                names.join(", ")
            ),
            input_schema: schemars::schema_for!(SearchInput).into(),
        })
    }
    pub fn prepare_search(&self, input: &Value) -> Result<PreparedEffect, ToolRejection> {
        if !self.enabled() {
            return Err(ToolRejection::Unavailable);
        }
        let input: SearchInput = serde_json::from_value(input.clone()).map_err(|_| invalid())?;
        if input.query.trim().is_empty() || !(1..=20).contains(&input.limit) {
            return Err(invalid());
        }
        let availability = self.clone();
        Ok(PreparedEffect::new(move |_| {
            Box::pin(async move {
                serde_json::to_value(availability.search(&input))
                    .map(Into::into)
                    .map_err(|error| ToolError::Failed(error.to_string()))
            })
        }))
    }
    /// Delivery from ToolJournal proves T2 committed. These preferences change only
    /// future snapshots; neither an in-flight step nor its captured effects widen.
    pub fn settled(&self, name: &str, result: &Value) -> Result<(), ToolError> {
        if name != SEARCH || !self.enabled() {
            return Ok(());
        }
        let result: SearchResult = serde_json::from_value(result.clone()).map_err(|error| {
            ToolError::OutcomeUnknown(format!("invalid committed search result: {error}"))
        })?;
        self.loaded.0.lock().unwrap().extend(result.activated);
        Ok(())
    }
    fn direct(&self, name: &str) -> bool {
        self.catalog.always_visible(name) || direct(name)
    }

    fn search(&self, input: &SearchInput) -> SearchResult {
        let query = input.query.trim().to_lowercase();
        let terms: Vec<_> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect();
        let active = self.loaded.0.lock().unwrap();
        // Scan the bound catalog metadata directly;
        // no search service or dependency graph is needed for this request view.
        let mut ranked: Vec<_> = self
            .catalog
            .definitions()
            .filter(|d| !self.direct(&d.name) && !active.contains(&d.name))
            .filter_map(|d| {
                let name = d.name.to_lowercase();
                let description = d.description.to_lowercase();
                let score = usize::from(name == query) * 1000
                    + terms
                        .iter()
                        .map(|term| {
                            if name.contains(term) {
                                10
                            } else if description.contains(term) {
                                1
                            } else {
                                0
                            }
                        })
                        .sum::<usize>();
                (score > 0).then_some((score, d))
            })
            .collect();
        ranked.sort_by(|(left, a), (right, b)| right.cmp(left).then_with(|| a.name.cmp(&b.name)));
        SearchResult {
            activated: ranked
                .into_iter()
                .take(input.limit)
                .map(|(_, definition)| definition.name.clone())
                .collect(),
        }
    }
}
fn invalid() -> ToolRejection {
    ToolRejection::InvalidInput {
        message: "Search requires a nonempty query and an optional limit from 1 to 20".into(),
    }
}
fn direct(name: &str) -> bool {
    matches!(
        name,
        "Shell"
            | "Read"
            | "Write"
            | "Edit"
            | "Glob"
            | "Grep"
            | "AskUserQuestion"
            | "StopBackgroundTask"
            | "WriteStdin"
            | "apply_patch"
    )
}
