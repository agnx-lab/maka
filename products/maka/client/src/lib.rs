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

//! View-independent Maka calls. The launcher retains this scope until `close` succeeds.
mod journal;
pub use journal::{Draft, DraftKey, DraftReference, Journal, Operation};
use maka_capabilities::call::Attachments;
use maka_host_runtime::application::{
    host::{Client as HostClient, LocalCall},
    transport::{Description, wire::Reply},
};
use maka_plugin_core::{
    deployment::DeploymentRef,
    operation::{OperationReply, RecoveryContract, receipt_reader_contract},
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, String>;

const CONTROL_METHODS: [&str; 4] = ["create", "selectBackend", "stop", "respond"];
fn control_correlation() -> Value {
    serde_json::json!({"kind": "agent-control"})
}

/// A shell-owned scope over one launcher-selected, authenticated Host connection.
/// Clones share custody; dropping a JS handle does not retire an accepted request.
#[derive(Clone)]
pub struct Client(Arc<Scope>);
struct Scope {
    host: Mutex<HostClient>,
    source: DeploymentRef,
    receipts: String,
    journal: Journal,
    requests: Mutex<BTreeMap<Uuid, Arc<Request>>>,
    closing: AtomicBool,
}
struct Request {
    id: Uuid,
    operation: Option<Uuid>,
    stop: CancellationToken,
    state: Mutex<State>,
}
struct State {
    call: LocalCall,
    phase: Phase,
}
#[derive(PartialEq)]
enum Phase {
    New,
    Preparing,
    Prepared,
    Confirming,
    Streaming,
    Pulling,
    Interrupted,
    Complete,
}

impl Client {
    pub fn new(host: HostClient, journal: Journal, receipts: String) -> Result<Self> {
        if host.catalogue().describe(&receipts)?.contract != receipt_reader_contract() {
            return Err("client receipt endpoint has a different contract".into());
        }
        Ok(Self(Arc::new(Scope {
            source: host.source().clone(),
            host: Mutex::new(host),
            journal,
            receipts,
            requests: Mutex::new(BTreeMap::new()),
            closing: AtomicBool::new(false),
        })))
    }
    pub fn source(&self) -> &DeploymentRef {
        &self.0.source
    }

    async fn open(
        &self,
        endpoint: &str,
        method: &str,
        input: Value,
        resources: Attachments,
        operation: Option<Uuid>,
    ) -> Result<(Arc<Request>, Description)> {
        // Opening and registration share the anchor lock with shutdown. Once closing
        // fences admission, its snapshot includes every previously opened request.
        let host = self.0.host.lock().await;
        if self.0.closing.load(Ordering::Acquire) {
            return Err("client scope is closing".into());
        }
        if self.0.requests.lock().await.len() >= 128 {
            return Err("client request capacity exhausted; settle earlier requests".into());
        }
        let description = host.catalogue().describe(endpoint)?.clone();
        let call = host.call(endpoint, method.into(), input, resources).await?;
        let request = Arc::new(Request {
            id: Uuid::new_v4(),
            operation,
            stop: CancellationToken::new(),
            state: Mutex::new(State {
                call,
                phase: Phase::New,
            }),
        });
        self.0
            .requests
            .lock()
            .await
            .insert(request.id, request.clone());
        Ok((request, description))
    }
    async fn release_if_clean(&self, request: &Request, state: &State) {
        if state.call.cleanup_confirmed() {
            self.0.requests.lock().await.remove(&request.id);
        }
    }
    pub async fn query(&self, endpoint: &str, method: &str, input: Value) -> Result<Value> {
        let (request, _) = self
            .open(endpoint, method, input, Attachments::new(), None)
            .await?;
        let mut state = request.state.lock().await;
        let result = tokio::select! {
            biased;
            () = request.stop.cancelled() => Err("client query cancelled".into()),
            result = async {
                state.call.start().await?;
                match state.call.receive().await? {
                    Reply::Finished { result, cleanup } => { cleanup?; result }
                    Reply::Aborted { reason, cleanup } => { cleanup?; Err(reason) }
                    _ => Err("Host method is not a query".into()),
                }
            } => result,
        };
        if result.is_err() {
            let _ = state.call.close().await;
        }
        self.release_if_clean(&request, &state).await;
        result
    }
    /// Reserve an attempt before opening it. An existing ID is recovery-only,
    /// including when the previous observer disappeared before preparation.
    #[allow(clippy::too_many_arguments)]
    pub async fn mutation(
        &self,
        id: Uuid,
        endpoint: &str,
        method: &str,
        input: Value,
        resources: Attachments,
        session_id: Option<String>,
        correlation: Value,
    ) -> Result<Mutation> {
        let operation = Operation {
            id,
            host: self.0.source.clone(),
            endpoint: endpoint.into(),
            method: method.into(),
            session_id,
            correlation,
            draft: None,
            abandoned: false,
            reference: None,
            receipt: None,
        };
        self.0.journal.reserve(&operation).await?;
        let (request, description) = self
            .open(endpoint, method, input, resources, Some(id))
            .await?;
        Ok(Mutation {
            client: self.clone(),
            request,
            operation,
            description,
        })
    }
    /// Reserve a shell control attempt separately from composer draft sends.
    pub async fn control(&self, id: Uuid, method: &str, input: Value) -> Result<Mutation> {
        if !CONTROL_METHODS.contains(&method) {
            return Err("unsupported agent control method".into());
        }
        let session_id = if method == "create" {
            None
        } else {
            Some(
                input
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .ok_or("agent control requires a sessionId")?
                    .to_owned(),
            )
        };
        self.mutation(
            id,
            "agent",
            method,
            input,
            Attachments::new(),
            session_id,
            control_correlation(),
        )
        .await
    }
    async fn draft_key(&self, endpoint: &str, session: &str, editor: &str) -> Result<DraftKey> {
        let host = self.0.host.lock().await;
        let target = host.catalogue().describe(endpoint)?;
        DraftKey::new(
            &self.0.source,
            &target.provider,
            &target.export,
            session,
            editor,
        )
    }
    pub async fn load_draft(
        &self,
        endpoint: &str,
        session: &str,
        editor: &str,
    ) -> Result<Option<Draft>> {
        let key = self.draft_key(endpoint, session, editor).await?;
        self.0.journal.load_draft(&key).await
    }
    pub async fn save_draft(
        &self,
        endpoint: &str,
        session: &str,
        editor: &str,
        expected_revision: Option<u64>,
        text: String,
    ) -> Result<Draft> {
        let key = self.draft_key(endpoint, session, editor).await?;
        self.0
            .journal
            .save_draft(&key, expected_revision, text)
            .await
    }
    /// Read the exact saved text in Rust. UI callers provide only its revision
    /// and model selection; editing a subsequent revision remains independent.
    #[allow(clippy::too_many_arguments)]
    pub async fn send_draft(
        &self,
        id: Uuid,
        endpoint: &str,
        session: &str,
        editor: &str,
        revision: u64,
        selection: Value,
    ) -> Result<Mutation> {
        let key = self.draft_key(endpoint, session, editor).await?;
        let draft = self
            .0
            .journal
            .load_draft(&key)
            .await?
            .ok_or("saved draft not found")?;
        if draft.revision != revision {
            return Err("saved draft revision changed before sending".into());
        }
        if draft.text.trim().is_empty() {
            return Err("saved draft is empty or already consumed".into());
        }
        let mut input = selection
            .as_object()
            .cloned()
            .ok_or("send selection must be an object")?;
        input.insert("sessionId".into(), Value::String(session.into()));
        input.insert("message".into(), Value::String(draft.text));
        let operation = Operation {
            id,
            host: self.0.source.clone(),
            endpoint: endpoint.into(),
            method: "send".into(),
            session_id: Some(session.into()),
            correlation: Value::Null,
            draft: Some(DraftReference { key, revision }),
            abandoned: false,
            reference: None,
            receipt: None,
        };
        self.0.journal.reserve(&operation).await?;
        let (request, description) = self
            .open(
                endpoint,
                "send",
                Value::Object(input),
                Attachments::new(),
                Some(id),
            )
            .await?;
        Ok(Mutation {
            client: self.clone(),
            request,
            operation,
            description,
        })
    }
    pub async fn operation(&self, id: Uuid) -> Result<Operation> {
        self.0.journal.load(&self.0.source, id).await
    }
    /// A settled or abandoned original still retains its control owner.
    pub async fn operation_for_control(&self, id: Uuid) -> Result<Operation> {
        let operation = self.operation(id).await?;
        if operation.draft.is_some()
            || operation.endpoint != "agent"
            || !CONTROL_METHODS.contains(&operation.method.as_str())
            || operation.correlation != control_correlation()
        {
            return Err("original client operation belongs to another control owner".into());
        }
        Ok(operation)
    }
    /// Only this shell's unsettled Agent controls on the current Host deployment.
    pub async fn pending_controls(&self) -> Result<Vec<Operation>> {
        self.0.journal.pending_controls(&self.0.source).await
    }
    /// Check the immutable product owner independently of a mutable pending list.
    /// A settled or abandoned original still belongs to the same draft editor.
    pub async fn operation_for_draft(
        &self,
        endpoint: &str,
        session: &str,
        editor: &str,
        id: Uuid,
    ) -> Result<Operation> {
        let key = self.draft_key(endpoint, session, editor).await?;
        let operation = self.operation(id).await?;
        if operation.draft.as_ref().map(|draft| &draft.key) != Some(&key) {
            return Err("original client operation belongs to another draft owner".into());
        }
        Ok(operation)
    }
    /// Atomically fence an attempt with no committed original reference. If
    /// preparation won that race, the returned operation remains recovery-only.
    /// This never authorizes resending the same ID or acknowledges physical cleanup.
    pub async fn abandon_unprepared(&self, id: Uuid) -> Result<Operation> {
        let operation = self.0.journal.abandon(&self.0.source, id).await?;
        if operation.abandoned {
            let requests: Vec<_> = self
                .0
                .requests
                .lock()
                .await
                .values()
                .filter(|request| request.operation == Some(id))
                .cloned()
                .collect();
            for request in requests {
                request.stop.cancel();
            }
        }
        Ok(operation)
    }
    /// Durable attempts for this exact Agent/editor owner. A reserved attempt
    /// without a reference remains unknown and cannot be sent again.
    pub async fn pending_operations(
        &self,
        endpoint: &str,
        session: &str,
        editor: &str,
    ) -> Result<Vec<Operation>> {
        let key = self.draft_key(endpoint, session, editor).await?;
        self.0.journal.pending(&self.0.source, &key).await
    }
    /// Query only the exact reference recorded before confirmation. A missing or
    /// unknown receipt never authorizes replay of the business method.
    pub async fn recover(&self, id: Uuid) -> Result<OperationReply> {
        let operation = self.operation(id).await?;
        let original = operation
            .reference
            .as_ref()
            .ok_or("client attempt has no confirmed preparation; it cannot be replayed")?;
        let value = self
            .query(
                &self.0.receipts,
                "read",
                serde_json::to_value(original).map_err(error)?,
            )
            .await?;
        let reply: OperationReply = serde_json::from_value(value).map_err(error)?;
        if &reply.reference != original {
            return Err("Host receipt differs from the original client operation".into());
        }
        self.0.journal.observe(&operation, &reply.receipt).await?;
        Ok(reply)
    }
    pub async fn subscribe(
        &self,
        endpoint: &str,
        method: &str,
        input: Value,
    ) -> Result<Subscription> {
        let (request, _) = self
            .open(endpoint, method, input, Attachments::new(), None)
            .await?;
        let mut state = request.state.lock().await;
        let result = tokio::select! {
            biased;
            () = request.stop.cancelled() => Err("client subscription cancelled".into()),
            result = async {
                state.call.start().await?;
                match state.call.receive().await? {
                    Reply::Subscribed => { state.phase = Phase::Streaming; Ok(()) },
                    Reply::Aborted { reason, cleanup } => { cleanup?; Err(reason) }
                    _ => Err("Host method is not a subscription".into()),
                }
            } => result,
        };
        if result.is_err() {
            let _ = state.call.close().await;
        }
        self.release_if_clean(&request, &state).await;
        drop(state);
        result?;
        Ok(Subscription {
            client: self.clone(),
            request,
        })
    }
    pub async fn pending_requests(&self) -> usize {
        self.0.requests.lock().await.len()
    }
    /// Request cancellation and drain this exact session. A recovered business
    /// result does not prove physical cleanup; only this session's own close ack
    /// can release disconnected requests. Retain the scope if this returns an error.
    pub async fn close(&self) -> Result<()> {
        self.0.closing.store(true, Ordering::Release);
        let mut host = self.0.host.lock().await;
        let requests: Vec<_> = self.0.requests.lock().await.values().cloned().collect();
        for request in &requests {
            request.stop.cancel();
        }
        let (closed, ()) = tokio::join!(host.close(), async {
            for request in requests {
                let mut state = request.state.lock().await;
                let _ = state.call.close().await;
                self.release_if_clean(&request, &state).await;
            }
        });
        closed?;
        self.0.requests.lock().await.clear();
        Ok(())
    }
}

/// A handle into the shell scope's original request, not a replayable command.
pub struct Mutation {
    client: Client,
    request: Arc<Request>,
    operation: Operation,
    description: Description,
}
impl Mutation {
    pub fn id(&self) -> Uuid {
        self.operation.id
    }
    pub async fn prepare(&self) -> Result<Operation> {
        let mut state = self.request.state.lock().await;
        if state.phase != Phase::New {
            return Err("client mutation has already started; recover or close it".into());
        }
        state.phase = Phase::Preparing;
        let result = tokio::select! {
            biased;
            () = self.request.stop.cancelled() => Err("client mutation preparation cancelled".into()),
            result = async {
                state.call.start().await?;
                let reference = match state.call.receive().await? {
                    Reply::Prepared { reference } => reference,
                    Reply::Aborted { reason, cleanup } => { cleanup?; return Err(reason); }
                    _ => return Err("Host method is not a mutation".into()),
                };
                let contract = RecoveryContract::for_method(&self.description.contract, &self.operation.method).map_err(error)?;
                if reference.source != self.operation.host || reference.namespace != self.description.provider
                    || reference.export != self.description.export || reference.method != self.operation.method || reference.contract != contract {
                    return Err("Host preparation differs from the original client target".into());
                }
                if self.operation.draft.as_ref().is_some_and(|draft| !draft.key.matches(&reference)) {
                    return Err("Host preparation belongs to another draft owner".into());
                }
                self.client.0.journal.prepared(self.operation.id, &reference).await?;
                state.phase = Phase::Prepared;
                self.client.operation(self.operation.id).await
            } => result,
        };
        if result.is_err() {
            state.phase = Phase::Interrupted;
            let _ = state.call.close().await;
        }
        self.client.release_if_clean(&self.request, &state).await;
        result
    }
    /// Confirm only after successful durable preparation. Cancellation of this
    /// observer leaves the original Call in the shell scope and fences a second confirm.
    pub async fn confirm(&self) -> Result<OperationReply> {
        let mut state = self.request.state.lock().await;
        if state.phase != Phase::Prepared {
            return Err("client mutation is not durably prepared".into());
        }
        state.phase = Phase::Confirming;
        let result = tokio::select! {
            biased;
            () = self.request.stop.cancelled() => Err("client mutation observation cancelled; recover its original receipt".into()),
            result = async {
                let mut reply = state.call.confirm().await?;
                loop {
                    match reply {
                        Reply::Progress { .. } => reply = state.call.receive().await?,
                        Reply::Mutation { reply, cleanup } => {
                            let operation = self.client.operation(self.operation.id).await?;
                            self.client.0.journal.observe(&operation, &reply.receipt).await?;
                            state.phase = Phase::Complete;
                            cleanup?;
                            return Ok(*reply);
                        }
                        Reply::Aborted { reason, cleanup } => { cleanup?; return Err(reason); }
                        _ => return Err("Host mutation did not return its original receipt".into()),
                    }
                }
            } => result,
        };
        if result.is_err() {
            let _ = state.call.close().await;
        }
        self.client.release_if_clean(&self.request, &state).await;
        result
    }
    /// Lose only the physical observer. The shell still owns the original request.
    pub async fn disconnect(&self) {
        let mut state = self.request.state.lock().await;
        state.call.disconnect();
        state.phase = Phase::Interrupted;
    }
    pub async fn close(&self) -> Result<()> {
        self.request.stop.cancel();
        let mut state = self.request.state.lock().await;
        state.phase = Phase::Interrupted;
        let result = state.call.close().await;
        self.client.release_if_clean(&self.request, &state).await;
        result
    }
}

pub enum SubscriptionEvent {
    Item(Value),
    Finished(Value),
}
pub struct Subscription {
    client: Client,
    request: Arc<Request>,
}
impl Subscription {
    pub async fn next(&self) -> Result<Option<SubscriptionEvent>> {
        let mut state = self.request.state.lock().await;
        if state.call.cleanup_confirmed() {
            return Ok(None);
        }
        if state.phase != Phase::Streaming {
            return Err(
                "original subscription pull lost its observer; close the subscription".into(),
            );
        }
        state.phase = Phase::Pulling;
        let result = tokio::select! {
            biased;
            () = self.request.stop.cancelled() => Err("client subscription cancelled".into()),
            result = async {
                match state.call.pull().await? {
                    Reply::Item { value } => Ok(Some(SubscriptionEvent::Item(value))),
                    Reply::Finished { result, cleanup } => { cleanup?; Ok(Some(SubscriptionEvent::Finished(result?))) }
                    Reply::Aborted { reason, cleanup } => { cleanup?; Err(reason) }
                    _ => Err("Host subscription returned an unexpected response".into()),
                }
            } => result,
        };
        if result.is_err() {
            let _ = state.call.close().await;
        }
        state.phase = if state.call.cleanup_confirmed() {
            Phase::Complete
        } else {
            Phase::Streaming
        };
        self.client.release_if_clean(&self.request, &state).await;
        result
    }
    pub async fn close(&self) -> Result<()> {
        self.request.stop.cancel();
        let mut state = self.request.state.lock().await;
        let result = state.call.close().await;
        self.client.release_if_clean(&self.request, &state).await;
        result
    }
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests;
