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

//! Per-request projection of explicitly registered tools. A projection can
//! narrow availability or replace its own handler/descriptor, never add a name.

use crate::{CatalogError, ToolCatalog, catalog::RegisteredTool};
use maka_agent_facts::tools::ProviderTool;
use maka_agent_facts::tools::ToolError;
use maka_agent_facts::tools::ToolHandler;
use maka_agent_facts::tools::ToolNesting;
use maka_agent_facts::tools::ToolPreparer;
use maka_agent_facts::tools::ToolSemantics;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use std::{future::Future, pin::Pin};
pub enum BoundTool {
    Unavailable,
    Prepared(Arc<dyn ToolPreparer>),
    /// Update descriptive process state without changing schema or authority.
    Described {
        handler: Arc<dyn ToolPreparer>,
        description: String,
    },
    /// A request-scoped model input projection; execution retains its registered binding.
    Input {
        handler: Arc<dyn ToolPreparer>,
        schema: serde_json::Value,
    },
    Provider(ProviderTool),
}
pub type BindingFuture =
    Pin<Box<dyn Future<Output = Result<BTreeMap<String, BoundTool>, ToolError>> + Send>>;
pub trait RequestBinding: Send + Sync {
    fn bind(&self, request: BindingRequest) -> BindingFuture;
}
#[derive(Clone)]
pub(crate) struct Group {
    names: BTreeSet<String>,
    binding: Arc<dyn RequestBinding>,
}
impl ToolCatalog {
    pub fn with_request_binding(
        mut self,
        names: BTreeSet<String>,
        binding: Arc<dyn RequestBinding>,
    ) -> Result<Self, CatalogError> {
        if names.is_empty()
            || names.iter().any(|name| {
                !self.entries.contains_key(name)
                    || self
                        .request_bindings
                        .iter()
                        .any(|group| group.names.contains(name))
            })
        {
            return Err(CatalogError::Invalid(
                "request binding must own distinct registered tools".into(),
            ));
        }
        Arc::make_mut(&mut self.request_bindings).push(Group { names, binding });
        Ok(self)
    }
    pub(crate) async fn bind_native_request(
        &mut self,
        request: BindingRequest,
    ) -> Result<(), ToolError> {
        for group in self.request_bindings.iter() {
            let selected: BTreeSet<_> = group
                .names
                .iter()
                .filter(|name| self.entries.contains_key(*name))
                .cloned()
                .collect();
            if selected.is_empty() {
                continue;
            }
            let mut input = request.clone();
            input.tools = selected.clone();
            // The embedding owns cancellation and actual cleanup of this work.
            let bound = group.binding.bind(input).await?;
            if bound.keys().cloned().collect::<BTreeSet<_>>() != selected {
                return Err(ToolError::Failed(
                    "request binding changed its registered tool names".into(),
                ));
            }
            for (name, projection) in bound {
                let entries = Arc::make_mut(&mut self.entries);
                let entry = entries.get(&name).expect("selected entry");
                let mut registration = entry.registration.clone();
                let mut validator = entry.validator.clone();
                let projection = match projection {
                    BoundTool::Described {
                        handler,
                        description,
                    } => {
                        if description.trim().is_empty() || description.len() > 16 * 1024 {
                            return Err(ToolError::Failed(
                                "request tool description invalid".into(),
                            ));
                        }
                        registration.definition.description = description;
                        BoundTool::Prepared(handler)
                    }
                    BoundTool::Input { handler, schema } => {
                        if schema != registration.definition.input_schema {
                            validator = Arc::new(
                                jsonschema::options()
                                    .offline()
                                    .should_validate_formats(false)
                                    .build(&schema)
                                    .map_err(|e| ToolError::Failed(e.to_string()))?,
                            );
                            registration.definition.input_schema = schema;
                        }
                        BoundTool::Prepared(handler)
                    }
                    projection => projection,
                };
                match projection {
                    BoundTool::Described { .. } | BoundTool::Input { .. } => {
                        unreachable!("projection already applied")
                    }
                    BoundTool::Unavailable => {
                        entries.remove(&name);
                        continue;
                    }
                    BoundTool::Prepared(handler) => {
                        if !handler.names().contains(&name) {
                            return Err(ToolError::Failed(
                                "request handler does not own its tool".into(),
                            ));
                        }
                        registration.definition.provider = None;
                        registration.handler = ToolHandler::Prepared(handler);
                    }
                    BoundTool::Provider(provider) => {
                        provider
                            .validate()
                            .map_err(|e| ToolError::Failed(e.into()))?;
                        registration.definition.provider = Some(provider);
                        registration.nesting = ToolNesting::DirectOnly;
                        registration.semantics = ToolSemantics::Parallel;
                    }
                }
                let bytes = serde_json::to_vec(&registration.definition)
                    .map_err(|e| ToolError::Failed(e.to_string()))?
                    .len();
                entries.insert(
                    name,
                    Arc::new(RegisteredTool {
                        always_visible: entry.always_visible,
                        registration,
                        validator,
                        bytes,
                    }),
                );
            }
        }
        if self
            .entries
            .values()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            > 1024 * 1024
        {
            return Err(ToolError::Failed("bound tool catalog exceeds 1 MiB".into()));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct BindingRequest {
    /// Frozen behavior identity for visibility decisions, never authority.
    pub behavior: Option<maka_agent_facts::execution::BehaviorId>,
    pub model: Option<maka_agent_facts::tools::ModelToolContext>,
    pub invocation: maka_agent_facts::event::Invocation,
    pub cwd: String,
    /// Effective catalog, already restricted by the Host's tool ceiling.
    pub tools: BTreeSet<String>,
    /// Complete frozen tool ceiling before individual binding groups project it.
    pub available_tools: BTreeSet<String>,
    pub cancellation: tokio_util::sync::CancellationToken,
}
