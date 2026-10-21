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

//! The application receives this already connected Host object; it cannot choose a Root.

mod assets;
mod files;
mod settings;

use maka_app_client::{Client, Subscription, SubscriptionEvent};
use nsh_core::{JsonValue, ui::prelude::*};
use std::{
    future::Future,
    sync::{Arc, Mutex as StdMutex},
};
use tokio::runtime::Handle;
use tokio_util::task::TaskTracker;
use uuid::Uuid;

const AGENT: &str = "agent";
const EDITOR: &str = "composer";

/// Own native tasks independently of a JavaScript view's lifetime.
pub struct Bindings {
    owner: Arc<Owner>,
}
struct Owner {
    client: Client,
    runtime: Handle,
    tasks: StdMutex<Option<TaskTracker>>,
}
struct Scope {
    owner: Arc<Owner>,
    session: String,
}

/// Root services and session scopes within the Host admitted by the launcher.
#[class]
pub struct ApplicationHost {
    owner: Arc<Owner>,
    initial_session: Option<String>,
}

/// One immutable Agent session's editor and original send operations.
#[class]
pub struct ScopedHost {
    scope: Arc<Scope>,
}

impl Bindings {
    pub fn new(client: Client, runtime: Handle) -> Self {
        Self {
            owner: Arc::new(Owner {
                client,
                runtime,
                tasks: StdMutex::new(Some(TaskTracker::new())),
            }),
        }
    }

    /// Bundled assets and immutable tool images read through this original Host.
    pub fn assets(&self) -> Arc<dyn nsh_engine::Assets> {
        Arc::new(assets::HostAssets(self.owner.clone()))
    }

    /// Native objects own subscriptions; the launcher owns calls and durable recovery.
    pub fn registry(&self, initial_session: Option<String>) -> Registry {
        let owner = self.owner.clone();
        let mut registry = Registry::ui_set("maka/host");
        registry.value("host", move || {
            Instance::new(ApplicationHost {
                owner: owner.clone(),
                initial_session: initial_session.clone(),
            })
        });
        registry
    }

    /// Fence new native tasks, close the original Host session and finish admitted IO.
    pub async fn close(&self) -> Result<(), String> {
        let tasks = self.owner.tasks.lock().unwrap().take();
        let (closed, ()) = tokio::join!(self.owner.client.close(), async move {
            if let Some(tasks) = tasks {
                tasks.close();
                tasks.wait().await;
            }
        });
        closed
    }
}

impl Owner {
    fn spawn<T: Send + 'static>(
        &self,
        operation: impl Future<Output = T> + Send + 'static,
    ) -> Result<tokio::task::JoinHandle<T>, String> {
        let tasks = self.tasks.lock().unwrap();
        Ok(tasks
            .as_ref()
            .ok_or("application bindings are closing")?
            .spawn_on(operation, &self.runtime))
    }
}

// A JS observer may disappear while IO is in progress. Execute on the launcher
// runtime, so freezing presentation never freezes a Call mutex or its cleanup.
async fn execute<T: Send + 'static>(
    owner: Arc<Owner>,
    operation: impl Future<Output = Result<T, String>> + Send + 'static,
) -> Result<T, String> {
    owner
        .spawn(operation)?
        .await
        .map_err(|error| format!("Host task failed: {error}"))?
}

#[class]
impl ApplicationHost {
    /// Display only. Keep the original opaque WorkspacePath for every Host call.
    pub fn path_label(&self, path: JsonValue) -> Result<String, String> {
        let path: maka_agent_api::contract::WorkspacePath =
            serde_json::from_value(path).map_err(|_| "Invalid workspace path")?;
        path.validate().map_err(|_| "Invalid workspace path")?;
        files::path_label(&path)
    }
    /// The optional session initially requested by the launcher.
    #[getter]
    pub fn initial_session(&self) -> Option<String> {
        self.initial_session.clone()
    }

    /// Capture a session without rebinding another view's editor or sends.
    pub fn session(&self, session_id: String) -> Result<Instance<ScopedHost>, String> {
        if session_id.is_empty() || session_id.len() > 128 {
            return Err("session must contain between 1 and 128 bytes".into());
        }
        Ok(Instance::new(ScopedHost {
            scope: Arc::new(Scope {
                owner: self.owner.clone(),
                session: session_id,
            }),
        }))
    }

    /// Query a service exposed by the original Host catalogue.
    pub async fn query(
        this: Instance<Self>,
        endpoint: String,
        method: String,
        input: JsonValue,
    ) -> Result<JsonValue, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        execute(owner.clone(), async move {
            owner.client.query(&endpoint, &method, input).await
        })
        .await
    }

    /// Return an owned native subscription, with no second handle table.
    pub async fn subscribe(
        this: Instance<Self>,
        endpoint: String,
        method: String,
        input: JsonValue,
    ) -> Result<Instance<ScopedSubscription>, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        let kind = if endpoint == AGENT && method == "readFile" {
            SubscriptionKind::File(Some(
                serde_json::from_value(input.clone()).map_err(|_| "Invalid file read selector")?,
            ))
        } else {
            SubscriptionKind::Events
        };
        let subscription = execute(owner.clone(), async move {
            let subscription = owner.client.subscribe(&endpoint, &method, input).await?;
            Ok(ScopedSubscription {
                owner,
                subscription: Arc::new(subscription),
                kind,
            })
        })
        .await?;
        Ok(Instance::new(subscription))
    }

    /// Prepare and confirm one allowed Agent control, preserving its original receipt.
    pub async fn control(
        this: Instance<Self>,
        method: String,
        input: JsonValue,
    ) -> Result<JsonValue, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        execute(owner.clone(), async move {
            let id = Uuid::new_v4();
            let mutation = owner.client.control(id, &method, input).await?;
            mutation.prepare().await?;
            let reply = mutation.confirm().await?;
            Ok(serde_json::json!({"operationId": id, "reply": reply}))
        })
        .await
    }

    /// Confirm one Settings mutation with an optional request-local secret resource.
    pub async fn settings_mutation(
        this: Instance<Self>,
        endpoint: String,
        method: String,
        input: JsonValue,
        secret: Option<String>,
    ) -> Result<JsonValue, String> {
        let resources = settings::attachments(&endpoint, &method, secret)?;
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        execute(owner.clone(), async move {
            let id = Uuid::new_v4();
            let mutation = owner
                .client
                .settings_mutation(id, &endpoint, &method, input, resources)
                .await?;
            mutation.prepare().await?;
            let reply = mutation.confirm().await?;
            Ok(serde_json::json!({"operationId": id, "reply": reply}))
        })
        .await
    }

    /// Unresolved Settings attempts, independent of the selected Agent session.
    pub async fn pending_settings(this: Instance<Self>) -> Result<JsonValue, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        execute(owner.clone(), async move {
            let operations = owner.client.pending_settings().await?;
            serde_json::to_value(operations).map_err(|error| error.to_string())
        })
        .await
    }

    /// Read a Settings attempt's original receipt without repeating its mutation.
    pub async fn recover_setting(
        this: Instance<Self>,
        operation_id: String,
    ) -> Result<JsonValue, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        let id = Uuid::parse_str(&operation_id).map_err(|error| error.to_string())?;
        execute(owner.clone(), async move {
            owner.client.operation_for_settings(id).await?;
            let reply = owner.client.recover(id).await?;
            serde_json::to_value(reply).map_err(|error| error.to_string())
        })
        .await
    }

    /// Fence a Settings attempt only before its preparation has committed.
    pub async fn abandon_setting(
        this: Instance<Self>,
        operation_id: String,
    ) -> Result<JsonValue, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        let id = Uuid::parse_str(&operation_id).map_err(|error| error.to_string())?;
        execute(owner.clone(), async move {
            owner.client.operation_for_settings(id).await?;
            let operation = owner.client.abandon_unprepared(id).await?;
            serde_json::to_value(operation).map_err(|error| error.to_string())
        })
        .await
    }

    /// Unresolved controls across this admitted Root, independent of the selected session.
    pub async fn pending_controls(this: Instance<Self>) -> Result<JsonValue, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        execute(owner.clone(), async move {
            let operations = owner.client.pending_controls().await?;
            serde_json::to_value(operations).map_err(|error| error.to_string())
        })
        .await
    }

    /// Read a control's original receipt without reissuing its business method.
    pub async fn recover_control(
        this: Instance<Self>,
        operation_id: String,
    ) -> Result<JsonValue, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        let id = Uuid::parse_str(&operation_id).map_err(|error| error.to_string())?;
        execute(owner.clone(), async move {
            owner.client.operation_for_control(id).await?;
            let reply = owner.client.recover(id).await?;
            serde_json::to_value(reply).map_err(|error| error.to_string())
        })
        .await
    }

    /// Fence a control only if its preparation has not committed.
    pub async fn abandon_control(
        this: Instance<Self>,
        operation_id: String,
    ) -> Result<JsonValue, String> {
        let owner = this.with(|host| host.owner.clone());
        drop(this);
        let id = Uuid::parse_str(&operation_id).map_err(|error| error.to_string())?;
        execute(owner.clone(), async move {
            owner.client.operation_for_control(id).await?;
            let operation = owner.client.abandon_unprepared(id).await?;
            serde_json::to_value(operation).map_err(|error| error.to_string())
        })
        .await
    }
}

#[class]
impl ScopedHost {
    /// The immutable session selected when this editor scope was created.
    #[getter]
    pub fn session_id(&self) -> String {
        self.scope.session.clone()
    }

    /// Read this editor's persisted draft and its monotonic editing revision.
    pub async fn load_draft(this: Instance<Self>) -> Result<JsonValue, String> {
        let scope = this.with(|host| host.scope.clone());
        execute(scope.owner.clone(), async move {
            match scope
                .owner
                .client
                .load_draft(AGENT, &scope.session, EDITOR)
                .await?
            {
                Some(draft) => serde_json::to_value(draft).map_err(|error| error.to_string()),
                None => Ok(serde_json::json!({"revision": 0, "text": ""})),
            }
        })
        .await
    }

    /// Persist an edit before sending; a stale writer cannot replace a newer draft.
    pub async fn save_draft(
        this: Instance<Self>,
        expected_revision: u64,
        text: String,
    ) -> Result<JsonValue, String> {
        let scope = this.with(|host| host.scope.clone());
        execute(scope.owner.clone(), async move {
            let draft = scope
                .owner
                .client
                .save_draft(
                    AGENT,
                    &scope.session,
                    EDITOR,
                    (expected_revision != 0).then_some(expected_revision),
                    text.into(),
                )
                .await?;
            serde_json::to_value(draft).map_err(|error| error.to_string())
        })
        .await
    }

    /// Prepare and confirm one new send from an exact persisted draft revision.
    pub async fn send(
        this: Instance<Self>,
        draft_revision: u64,
        selection: JsonValue,
    ) -> Result<JsonValue, String> {
        let scope = this.with(|host| host.scope.clone());
        execute(scope.owner.clone(), async move {
            let id = Uuid::new_v4();
            let mutation = scope
                .owner
                .client
                .send_draft(id, AGENT, &scope.session, EDITOR, draft_revision, selection)
                .await?;
            mutation.prepare().await?;
            let reply = mutation.confirm().await?;
            Ok(serde_json::json!({"operationId": id, "reply": reply}))
        })
        .await
    }

    /// Read unresolved original sends after a view or application restart.
    pub async fn pending(this: Instance<Self>) -> Result<JsonValue, String> {
        let scope = this.with(|host| host.scope.clone());
        execute(scope.owner.clone(), async move {
            let operations = scope
                .owner
                .client
                .pending_operations(AGENT, &scope.session, EDITOR)
                .await?;
            serde_json::to_value(operations).map_err(|error| error.to_string())
        })
        .await
    }

    /// Query only a previously persisted operation; never submit its send again.
    pub async fn recover(this: Instance<Self>, operation_id: String) -> Result<JsonValue, String> {
        let scope = this.with(|host| host.scope.clone());
        let id = Uuid::parse_str(&operation_id).map_err(|error| error.to_string())?;
        execute(scope.owner.clone(), async move {
            scope.owned_operation(id).await?;
            let reply = scope.owner.client.recover(id).await?;
            serde_json::to_value(reply).map_err(|error| error.to_string())
        })
        .await
    }

    /// Fence only an original attempt whose preparation has not committed.
    pub async fn abandon_unprepared(
        this: Instance<Self>,
        operation_id: String,
    ) -> Result<JsonValue, String> {
        let scope = this.with(|host| host.scope.clone());
        let id = Uuid::parse_str(&operation_id).map_err(|error| error.to_string())?;
        execute(scope.owner.clone(), async move {
            scope.owned_operation(id).await?;
            let operation = scope.owner.client.abandon_unprepared(id).await?;
            serde_json::to_value(operation).map_err(|error| error.to_string())
        })
        .await
    }
}

impl Scope {
    async fn owned_operation(&self, id: Uuid) -> Result<(), String> {
        self.owner
            .client
            .operation_for_draft(AGENT, &self.session, EDITOR, id)
            .await?;
        Ok(())
    }
}

/// A native subscription owns cleanup even if its promise or VM disappears.
#[class]
pub struct ScopedSubscription {
    owner: Arc<Owner>,
    subscription: Arc<Subscription>,
    kind: SubscriptionKind,
}
enum SubscriptionKind {
    Events,
    File(Option<maka_agent_api::contract::FileRead>),
}

impl Drop for ScopedSubscription {
    fn drop(&mut self) {
        let subscription = self.subscription.clone();
        let _ = self.owner.spawn(async move { subscription.close().await });
        // Once task admission closes, Client.close owns final session cleanup.
    }
}

#[class]
impl ScopedSubscription {
    /// Verify a complete current-file stream using its original selector, off the JS heap.
    pub async fn read_file(this: Instance<Self>) -> Result<JsonValue, String> {
        let (owner, subscription, source) = this.with_mut(|item| {
            let source = match &mut item.kind {
                SubscriptionKind::File(source) => source.take(),
                SubscriptionKind::Events => None,
            };
            (item.owner.clone(), item.subscription.clone(), source)
        });
        drop(this);
        let source = source.ok_or("File subscription already consumed or is not a file read")?;
        execute(owner, async move {
            let outcome = files::collect(&subscription, &source).await?;
            tokio::task::spawn_blocking(move || files::presentation(outcome, source))
                .await
                .map_err(|_| "Current file presentation worker failed")?
        })
        .await
    }
    /// Pull one observation from the original subscription.
    pub async fn next(this: Instance<Self>) -> Result<JsonValue, String> {
        if this.with(|item| matches!(item.kind, SubscriptionKind::File(_))) {
            return Err("Consume file subscriptions with readFile".into());
        }
        let (owner, subscription) =
            this.with(|item| (item.owner.clone(), item.subscription.clone()));
        drop(this);
        execute(owner, async move {
            match subscription.next().await? {
                Some(SubscriptionEvent::Item(value)) => Ok(value),
                Some(SubscriptionEvent::Finished(_)) | None => Ok(JsonValue::Null),
            }
        })
        .await
    }

    /// Close this original subscription without releasing unknown physical work.
    pub async fn close(this: Instance<Self>) -> Result<(), String> {
        let (owner, subscription) =
            this.with(|item| (item.owner.clone(), item.subscription.clone()));
        drop(this);
        execute(owner, async move { subscription.close().await }).await
    }
}
