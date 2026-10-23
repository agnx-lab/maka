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

use maka_agent_api::{contract::WorkspaceAccess, session::SessionConfiguration};
use maka_agent_facts::{
    configuration::mcp::ServerConfiguration,
    tool_call::ToolRejection,
    tools::{
        PreparationFuture, PreparedEffect, ToolCallContext, ToolError, ToolHandler, ToolNesting,
        ToolPreparer, ToolRegistration, ToolSemantics,
    },
};
use maka_event_log::EventLog;
use maka_mcp::Connection;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(crate) struct Servers {
    connections: Vec<Connection>,
    pub registrations: Vec<ToolRegistration>,
    pub planning: Vec<String>,
    pub bindings: BTreeMap<String, Option<String>>,
}
impl Servers {
    pub async fn open(
        configurations: &[ServerConfiguration],
        store: Arc<EventLog>,
        configuration: &maka_config::ConfigurationStore,
    ) -> Result<Self, String> {
        let mut servers = Self::default();
        if configurations.is_empty() {
            return Ok(servers);
        }
        let mut names = BTreeSet::new();
        if configurations.len() > 64 {
            return Err("At most 64 MCP servers may be configured".into());
        }
        for config in configurations {
            maka_mcp::validate(config)?;
            if !names.insert(&config.name) {
                return Err("MCP server names must be unique".into());
            }
        }
        let network = configuration
            .network_configuration()
            .await
            .map_err(|_| "MCP network settings are unavailable")?;
        let network =
            maka_network::Policy::from_host_settings(&network.proxy, network.password.as_deref())
                .map_err(|_| "MCP network settings are invalid")?;
        for config in configurations {
            let connection = match Connection::connect(config.clone(), network.clone()).await {
                Ok(connection) => connection,
                Err(error) => {
                    servers.close().await?;
                    return Err(format!("MCP server {}: {error}", config.name));
                }
            };
            // A new external protocol session is a new authority generation.
            // Previously admitted work cannot silently acquire its replacement.
            let generation = format!("mcp:{}:{}", config.name, uuid::Uuid::new_v4());
            for tool in connection.tools() {
                let name = tool.definition.name.clone();
                if tool.read_only {
                    servers.planning.push(name.clone());
                }
                servers
                    .bindings
                    .insert(name.clone(), Some(generation.clone()));
                servers.registrations.push(ToolRegistration {
                    definition: tool.definition.clone(),
                    nesting: ToolNesting::Nestable,
                    semantics: ToolSemantics::Parallel,
                    handler: ToolHandler::Prepared(Arc::new(Binding {
                        connection: connection.clone(),
                        store: store.clone(),
                        name,
                        remote_name: tool.remote_name.clone(),
                        server: config.name.clone(),
                        generation: generation.clone(),
                        preapproved: config.preapproved,
                        read_only: tool.read_only,
                    })),
                });
            }
            servers.connections.push(connection);
        }
        Ok(servers)
    }
    pub async fn close(&self) -> Result<(), String> {
        let mut error = None;
        for connection in &self.connections {
            if let Err(cause) = connection.close().await {
                error.get_or_insert(cause);
            }
        }
        error.map_or(Ok(()), Err)
    }
}
struct Binding {
    connection: Connection,
    store: Arc<EventLog>,
    name: String,
    remote_name: String,
    server: String,
    generation: String,
    preapproved: bool,
    read_only: bool,
}
fn permitted(configuration: &SessionConfiguration, name: &str, read_only: bool) -> bool {
    configuration
        .tools
        .as_ref()
        .is_none_or(|names| names.iter().any(|allowed| allowed == name))
        && (configuration.permissions.access != WorkspaceAccess::ReadOnly || read_only)
}
impl ToolPreparer for Binding {
    fn names(&self) -> Vec<String> {
        vec![self.name.clone()]
    }
    fn prepare(
        &self,
        name: String,
        input: Value,
        context: ToolCallContext,
        cancellation: CancellationToken,
    ) -> PreparationFuture {
        let connection = self.connection.clone();
        let store = self.store.clone();
        let expected = self.name.clone();
        let remote = self.remote_name.clone();
        let server = self.server.clone();
        let generation = self.generation.clone();
        let read_only = self.read_only;
        let preapproved = self.preapproved;
        Box::pin(async move {
            if name != expected || !connection.available() {
                return Err(ToolRejection::Unavailable);
            }
            if cancellation.is_cancelled() {
                return Err(ToolRejection::Cancelled);
            }
            let live = maka_agent_authority::permissions::live(&store, &context.invocation)
                .await
                .map_err(|error| ToolRejection::PreparationFailed {
                    message: error.to_string(),
                })?;
            if !permitted(&live, &name, read_only) {
                return Err(ToolRejection::PolicyDenied {
                    message: "MCP tool is outside the current session permissions".into(),
                });
            }
            Ok(PreparedEffect::new(move |cancel| {
                Box::pin(async move {
                    if !preapproved {
                        crate::tools::approval::confirm(
                            &store,
                            &context,
                            crate::tools::approval::Selection {
                                name: &name,
                                method: &remote,
                                binding: &generation,
                                service: &format!("MCP server {server}"),
                            },
                            &input,
                            &cancel,
                        )
                        .await?;
                    }
                    let live = maka_agent_authority::permissions::live(&store, &context.invocation)
                        .await?;
                    if !permitted(&live, &name, read_only) {
                        return Err(ToolError::Failed(
                            "MCP permissions changed before dispatch".into(),
                        ));
                    }
                    connection.call(remote, input, cancel).await
                })
            }))
        })
    }
}
