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

//! Request-local resources and accepted asynchronous service calls.
mod mutations;
pub(super) mod subscriptions;
use super::*;
use maka_capabilities::call::Attachments;
use maka_capabilities::call::ReadResource;
use maka_event_log::applications::intents::{CallerConfirmation, RecordedIntent};
use maka_plugin_core::{
    contract::{CaptureOutput, MethodMode},
    operation::{OperationReply, Receipt, RecoveryRef},
    runtime::{Capability, Execution},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, future::Future, pin::Pin, sync::Arc};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureSource {
    pub child: String,
    pub method: String,
    pub input: Value,
}
/// An inert intent. Resource slots name capture requests, never bearer handles.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub key: String,
    pub binding: String,
    pub member: u32,
    pub method: String,
    pub input: Value,
    pub resources: BTreeMap<String, CaptureSource>,
    // Caller-owned JSON correlation, persisted before mutation dispatch.
    // Excluded from provider input and peer confirmation; not execution authority.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub recovery: Value,
    #[serde(default)]
    pub continuations: Vec<CaptureSource>,
}
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CapturePlan {
    Forward { source: CaptureSource },
    Native,
}
pub type ServiceFuture = Pin<Box<dyn Future<Output = ServiceCompletion> + Send + 'static>>;
pub type StreamFuture = Pin<Box<dyn Future<Output = StreamCompletion> + Send + 'static>>;
/// Moves custody of the same executor to its next pull; an item never confirms cleanup.
pub enum StreamCompletion {
    Item { value: Value, next: StreamFuture },
    Finished(ServiceCompletion),
}
/// Native query/stream execution and effect-free mutation ownership. Implementations
/// receive checked requests, never guest-created authorization evidence.
pub trait NativeService: Send + Sync + 'static {
    /// Construct a background future only. Polling performs the query after
    /// final admission; the executor retains it independently of any UI waiter.
    fn call(&self, request: ServiceRequest) -> ServiceFuture;
    /// Construct an inert pull stream. Cancellation closes demand; Finished
    /// must independently report the original executor's actual cleanup.
    fn subscribe(&self, _request: ServiceRequest, _stop: CancellationToken) -> StreamFuture {
        Box::pin(async {
            StreamCompletion::Finished(ServiceCompletion {
                result: Err("service has no subscription executor".into()),
                cleanup: Ok(()),
            })
        })
    }
    /// Construct an inert owner before polling preparation. A returned error
    /// must leave no cleanup resource; panics are unconfirmed cleanup.
    fn mutation(
        &self,
        _request: ServiceRequest,
    ) -> std::result::Result<Arc<dyn NativeMutation>, String> {
        Err("service has no mutation executor".into())
    }
}

pub type ReferenceFuture =
    Pin<Box<dyn Future<Output = std::result::Result<RecoveryRef, String>> + Send>>;
pub type MutationFuture = Pin<Box<dyn Future<Output = MutationCompletion> + Send>>;
pub type CleanupFuture = Pin<Box<dyn Future<Output = std::result::Result<(), String>> + Send>>;
/// One actual native operation owner. Preparation has no business effects.
/// Dispatch rechecks native authority; close acknowledges actual cleanup, not
/// cancellation. The kernel retains this owner across every asynchronous stage.
pub trait NativeMutation: Send + Sync + 'static {
    fn prepare(self: Arc<Self>) -> ReferenceFuture;
    fn dispatch(
        self: Arc<Self>,
        recorded: RecordedIntent,
        progress: tokio::sync::mpsc::Sender<Value>,
    ) -> MutationFuture;
    /// Request cancellation only. The original dispatch/close still owns cleanup.
    fn cancel(&self);
    fn close(self: Arc<Self>) -> CleanupFuture;
}
pub struct MutationCompletion {
    pub reply: OperationReply,
    pub cleanup: std::result::Result<(), String>,
}

/// Immutable facts from the checked request. Final admission remains required.
/// These observations do not grant Host access.
/// Guest JSON cannot supply or replace this source.
#[derive(Clone, Debug)]
pub struct ServiceSource {
    deployment: Option<maka_plugin_core::deployment::DeploymentRef>,
    consumer: maka_plugin_core::runtime::Activation,
    mount: Mount,
    request: Uuid,
}
impl ServiceSource {
    pub fn deployment(&self) -> Option<&maka_plugin_core::deployment::DeploymentRef> {
        self.deployment.as_ref()
    }
    pub fn consumer(&self) -> &maka_plugin_core::runtime::Activation {
        &self.consumer
    }
    pub fn mount(&self) -> &Mount {
        &self.mount
    }
    pub fn request_id(&self) -> Uuid {
        self.request
    }
}
pub struct ServiceRequest {
    pub source: ServiceSource,
    pub method: String,
    pub input: Value,
    pub resources: Attachments,
}

/// Static source observations and actual endpoint values are separate facts.
#[derive(Default)]
pub struct RemoteServices {
    pub declarations: maka_plugin_core::plan::RemoteExports,
    pub registrations: Vec<RemoteService>,
}
#[derive(Clone)]
pub struct RemoteService {
    pub target: maka_plugin_core::plan::RemoteCapability,
    pub registration_epoch: Uuid,
    pub contract: maka_plugin_core::contract::Contract,
    pub implementation: Arc<dyn NativeService>,
}
/// Business outcome and actual executor cleanup are separate facts.
pub struct ServiceCompletion {
    pub result: std::result::Result<Value, String>,
    pub cleanup: std::result::Result<(), String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionOutcome {
    NotAccepted {
        reason: String,
    },
    Succeeded {
        value: Value,
    },
    Updated {
        value: Value,
    },
    Failed {
        reason: String,
    },
    /// Business execution settled, but original presentation evidence no longer
    /// admits this result. This is feedback to a live consumer, not a UI fault.
    Stale,
    Operation {
        reply: Box<OperationReply>,
        /// Position of the original confirmed caller commit, independent of the
        /// business outcome. A missing commit never authorizes advancing recovery.
        #[serde(deserialize_with = "Option::deserialize")]
        #[schemars(schema_with = "caller_confirmation_schema")]
        caller: Option<Box<CallerConfirmation>>,
    },
}
fn caller_confirmation_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.subschema_for::<Option<CallerConfirmation>>()
}
/// A presenter can resolve only this exact already-checked native leaf.
/// Construction is private: plugin JSON cannot supply a Mount or resource grant.
pub struct CaptureTarget {
    pub(crate) mount: Mount,
    pub(crate) method: String,
    pub(crate) input: Value,
    output: CaptureOutput,
    edges: Vec<Edge>,
}
impl CaptureTarget {
    pub fn mount(&self) -> &Mount {
        &self.mount
    }
    pub fn method(&self) -> &str {
        &self.method
    }
    pub fn input(&self) -> &Value {
        &self.input
    }
    pub fn output(&self) -> &CaptureOutput {
        &self.output
    }
}
/// An opaque, bounded native snapshot. It stays on the foreground thread and
/// is never serialized, attached to a service request, or treated as authority.
/// Retain observations/identity only, not Entity/Input/subscription/lease owners.
pub struct ContinuationWitness {
    value: Box<dyn std::any::Any>,
    bytes: usize,
}
impl ContinuationWitness {
    pub fn new<T: 'static>(value: T, bytes: usize) -> Self {
        Self {
            value: Box::new(value),
            bytes,
        }
    }
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}
pub enum NativeCapture {
    Resource(Arc<dyn ReadResource>),
    Continuation(ContinuationWitness),
}
pub(super) struct PreparingAction<V: ?Sized> {
    prepared: Option<Prepared<V>>,
    requests: std::collections::VecDeque<(Option<String>, CaptureSource, CaptureOutput)>,
    capture: Option<PendingCapture<V>>,
    slot: Option<String>,
}
struct PendingCapture<V: ?Sized> {
    child: Mount,
    source: CaptureSource,
    output: CaptureOutput,
    edges: Vec<Edge>,
    execution: maka_plugin_core::runtime::Execution<Export<V>>,
    job: backend::Job<CapturePlan>,
}
struct Continuation {
    target: CaptureTarget,
    witness: ContinuationWitness,
    invalidated: bool,
}
pub struct CaptureCall<'a> {
    pub values: &'a abi::ValueBindings,
    pub state: &'a Value,
    pub props: &'a Value,
    pub method: &'a str,
    pub input: &'a Value,
}
pub type TaskFuture = Pin<Box<dyn Future<Output = TaskCompletion> + Send>>;
/// Execute independently of presentation lifetime, then return to the original
/// foreground owner. Completion can produce another task, including close.
pub struct ActionTask {
    pub id: Uuid,
    pub future: TaskFuture,
}
pub enum TaskCompletion {
    Query(ServiceCompletion),
    Subscription(StreamCompletion),
    Prepared(std::result::Result<RecordedIntent, String>),
    Mutation(MutationCompletion),
    MutationProgress { value: Value, next: TaskFuture },
    Closed(std::result::Result<(), String>),
    Interrupted(String),
}
#[derive(Clone)]
pub(super) struct Edge {
    pub parent: Mount,
    pub child: Mount,
    pub slot: String,
}
pub(super) struct Prepared<V: ?Sized> {
    pub source: Option<maka_event_log::applications::SourceSnapshot>,
    pub(super) id: Uuid,
    pub consumer: Mount,
    pub key: String,
    pub method: String,
    pub input: Value,
    pub recovery: Value,
    pub capability: Capability<Export<V>>,
    pub captures: BTreeMap<String, CaptureTarget>,
    continuations: Vec<CaptureTarget>,
    witnesses: Vec<Continuation>,
    pub invalidated: bool,
    pub subscription: bool,
    resources: Option<Attachments>,
}
struct Running<V: ?Sized> {
    consumer: Mount,
    key: String,
    method: String,
    completed: bool,
    execution: Execution<Export<V>>,
    resources: Attachments,
    mutation: Option<mutations::Session>,
    stream: Option<subscriptions::Flow>,
    continuations: Vec<Continuation>,
}
pub(super) struct Outcome {
    pub consumer: Mount,
    pub key: String,
    pub outcome: ActionOutcome,
    pub request: Option<Uuid>,
    continuations: Vec<Continuation>,
}
pub(super) struct Actions<V: ?Sized> {
    pub queue: VecDeque<Prepared<V>>,
    running: BTreeMap<Uuid, Running<V>>,
    preparing: BTreeMap<Uuid, mutations::Preparing<V>>,
    pub outcomes: VecDeque<Outcome>,
}
impl<V: ?Sized> Default for Actions<V> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            running: BTreeMap::new(),
            preparing: BTreeMap::new(),
            outcomes: VecDeque::new(),
        }
    }
}
impl<V: ?Sized> Actions<V> {
    /// The handler key is also the author's correlation for one-shot feedback.
    /// Completed cleanup failures no longer reserve it after feedback delivery.
    pub(super) fn key_pending(&self, consumer: &Mount, key: &str) -> bool {
        self.queue
            .iter()
            .any(|a| !a.subscription && a.consumer == *consumer && a.key == key)
            || self
                .preparing
                .values()
                .any(|p| !p.failed() && p.action.consumer == *consumer && p.action.key == key)
            || self.running.values().any(|r| {
                !r.completed && r.stream.is_none() && r.consumer == *consumer && r.key == key
            })
            || self
                .outcomes
                .iter()
                .any(|o| o.request.is_none() && o.consumer == *consumer && o.key == key)
    }
    pub fn accepted_count(&self) -> usize {
        self.running.len() + self.preparing.len()
    }
    pub fn len(&self) -> usize {
        self.queue.len()
            + self.running.len()
            + self.preparing.len()
            + self
                .outcomes
                .iter()
                .filter(|o| o.request.is_none_or(|id| !self.running.contains_key(&id)))
                .count()
    }
    fn bytes(&self) -> usize {
        self.queue
            .iter()
            .filter_map(|q| q.resources.as_ref())
            .chain(
                self.preparing
                    .values()
                    .filter_map(|p| p.action.resources.as_ref()),
            )
            .chain(self.running.values().map(|r| &r.resources))
            .flat_map(|r| r.values())
            .map(|r| r.bytes())
            .sum::<usize>()
            + self
                .queue
                .iter()
                .flat_map(|a| &a.witnesses)
                .chain(self.running.values().flat_map(|a| &a.continuations))
                .chain(self.outcomes.iter().flat_map(|a| &a.continuations))
                .map(|c| c.witness.bytes())
                .sum::<usize>()
    }
}
fn unavailable(
    reference: RecoveryRef,
    caller: Option<Box<CallerConfirmation>>,
    diagnostic: String,
) -> ActionOutcome {
    ActionOutcome::Operation {
        caller,
        reply: Box::new(OperationReply {
            reference,
            receipt: Receipt::Unavailable {
                diagnostic: bounded_reason(diagnostic),
            },
        }),
    }
}
impl<V: ?Sized> Prepared<V> {
    pub(super) fn invalidate(&mut self, mut exists: impl FnMut(&Edge) -> bool) {
        if self
            .captures
            .values()
            .chain(&self.continuations)
            .chain(self.witnesses.iter().map(|c| &c.target))
            .flat_map(|t| &t.edges)
            .any(|edge| !exists(edge))
        {
            self.invalidated = true;
        }
    }
}
impl<V: ?Sized + 'static> Application<V> {
    /// Check at each actual callback: an earlier queued result can change the
    /// same component's state after executor completion but before delivery.
    pub(super) fn action_feedback(&self, mount: &Mount, outcome: ActionOutcome) -> ActionOutcome {
        if !matches!(
            &outcome,
            ActionOutcome::Operation { .. }
                | ActionOutcome::Succeeded { .. }
                | ActionOutcome::Updated { .. }
        ) {
            return outcome;
        };
        let valid: Result<()> = (|| {
            // The exact provider determines the callback language. Native
            // components may return a portable tree and preserve Rust integers.
            let javascript = self.modules.contains_key(self.provider(mount)?.instance());
            if javascript {
                let value =
                    serde_json::to_value(&outcome).map_err(|e| Error::Invalid(e.to_string()))?;
                maka_plugin_core::json::exact_numbers(&value).map_err(Error::Invalid)?;
                if let Some(view) = self.views.get(&mount.id()) {
                    maka_plugin_core::json::exact_numbers(&view.frame.state)
                        .map_err(Error::Invalid)?;
                    maka_plugin_core::json::exact_numbers(&view.props).map_err(Error::Invalid)?;
                    for members in self.mount_values(mount)?.0.values() {
                        for value in members {
                            maka_plugin_core::json::exact_numbers(value).map_err(Error::Invalid)?;
                        }
                    }
                }
            }
            Ok(())
        })();
        match valid {
            Ok(()) => outcome,
            Err(error) => match outcome {
                ActionOutcome::Operation { reply, caller } => unavailable(
                    reply.reference,
                    caller,
                    format!("operation result delivery failed: {error}; read original receipt"),
                ),
                _ => ActionOutcome::Failed {
                    reason: bounded_reason(format!("action result delivery failed: {error}")),
                },
            },
        }
    }

    pub(super) fn begin_prepare_action(
        &mut self,
        consumer: &Mount,
        action: Action,
        _update: &mut tree::Update<V>,
        subscription: bool,
    ) -> Result<PreparingAction<V>> {
        for name in [&action.key, &action.binding, &action.method] {
            if name.is_empty() || name.len() > 128 {
                return Err(Error::Invalid("invalid action name".into()));
            }
        }
        if action.resources.len() > 16 {
            return Err(Error::Invalid("action resource limit".into()));
        }
        if action.continuations.len() > 16 {
            return Err(Error::Invalid("action continuation limit".into()));
        }
        let provider = self.provider(consumer)?;
        let caps = self.runtime.bind(&provider, &action.binding)?;
        let capability = caps
            .get(action.member as usize)
            .ok_or_else(|| Error::Invalid("action binding member unavailable".into()))?
            .clone();
        let declaration = match self.runtime.inspect(&capability)? {
            Export::Service { methods, .. } => methods.get(&action.method).map(|m| {
                (
                    m.mode,
                    m.resources.clone(),
                    m.input
                        .validate(&action.input)
                        .map_err(|e| Error::Invalid(e.to_string())),
                )
            }),
            _ => None,
        };
        let (mode, resources, valid) =
            declaration.ok_or_else(|| Error::Invalid("undeclared action method".into()))?;
        valid?;
        if !action.recovery.is_null() && mode != MethodMode::Mutation {
            return Err(Error::Invalid(
                "recovery metadata requires a mutation".into(),
            ));
        }
        maka_event_log::applications::intents::validate_recovery(&action.recovery)
            .map_err(|error| Error::Invalid(error.to_string()))?;
        if (mode == MethodMode::Subscription) != subscription {
            return Err(Error::Invalid(
                "action and subscription method mode mismatch".into(),
            ));
        }
        let source = self.origins.get(&provider.epoch()).cloned();
        if mode == MethodMode::Mutation && source.is_none() {
            return Err(Error::Invalid(
                "mutation requires a persisted UI source".into(),
            ));
        }
        if mode != MethodMode::Query && !action.continuations.is_empty() {
            return Err(Error::Invalid("continuations require a query".into()));
        }
        if action
            .resources
            .keys()
            .any(|slot| !resources.contains_key(slot))
            || resources.iter().any(|(slot, declaration)| {
                !declaration.optional && !action.resources.contains_key(slot)
            })
        {
            return Err(Error::Invalid("action resource slots mismatch".into()));
        }
        let mut requests = std::collections::VecDeque::new();
        for (slot, source) in action.resources {
            let output = CaptureOutput::Resource {
                resource: resources[&slot].resource_type.clone(),
            };
            requests.push_back((Some(slot), source, output));
        }
        for source in action.continuations {
            requests.push_back((None, source, CaptureOutput::Continuation));
        }
        Ok(PreparingAction {
            prepared: Some(Prepared {
                source,
                id: Uuid::new_v4(),
                consumer: consumer.clone(),
                key: action.key,
                method: action.method,
                input: action.input,
                recovery: action.recovery,
                capability,
                captures: BTreeMap::new(),
                continuations: Vec::new(),
                witnesses: Vec::new(),
                invalidated: false,
                subscription,
                resources: None,
            }),
            requests,
            capture: None,
            slot: None,
        })
    }
    fn begin_capture(
        &mut self,
        parent: &Mount,
        source: CaptureSource,
        output: &CaptureOutput,
        mut edges: Vec<Edge>,
        update: &mut tree::Update<V>,
    ) -> Result<PendingCapture<V>> {
        if source.child.is_empty()
            || source.child.len() > 128
            || source.method.is_empty()
            || source.method.len() > 128
            || edges.len() >= 32
        {
            return Err(Error::Invalid("invalid capture source/depth".into()));
        }
        let view = self.current(parent, update)?;
        let child = view
            .children
            .get(&source.child)
            .ok_or_else(|| Error::Invalid("capture child unavailable".into()))?
            .mount
            .clone();
        let view = self.current(&child, update)?;
        let values = self.mount_values(&child)?;
        let state = view.frame.state.clone();
        let props = view.props.clone();
        edges.push(Edge {
            parent: parent.clone(),
            child: child.clone(),
            slot: source.child.clone(),
        });
        update.step(self.mount_depth(&child)?)?;
        let execution = self.runtime.admit_mount(&child)?;
        let Export::Component {
            implementation,
            captures,
            ..
        } = execution.value()
        else {
            unreachable!()
        };
        let result = (|| {
            let definition = captures
                .get(&source.method)
                .ok_or_else(|| Error::Invalid("undeclared capture method".into()))?;
            if &definition.output != output {
                return Err(Error::Invalid("capture output type mismatch".into()));
            }
            definition
                .input
                .validate(&source.input)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            match implementation {
                Component::JavaScript { callbacks, .. } => callbacks.capture(backend::Capture {
                    values,
                    state,
                    props,
                    method: source.method.clone(),
                    input: source.input.clone(),
                }),
                Component::Native(component) => {
                    Ok(backend::Job::ready(component.capture(CaptureCall {
                        values: &values,
                        state: &state,
                        props: &props,
                        method: &source.method,
                        input: &source.input,
                    })))
                }
            }
        })();
        let job = match result {
            Ok(job) => job,
            Err(error) => {
                self.runtime.settle(&execution, Ok(()))?;
                return Err(error);
            }
        };
        Ok(PendingCapture {
            child,
            source,
            output: output.clone(),
            edges,
            execution,
            job,
        })
    }

    pub(super) fn poll_prepare_action(
        &mut self,
        preparing: &mut PreparingAction<V>,
        update: &mut tree::Update<V>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Prepared<V>>> {
        loop {
            if let Some(mut pending) = preparing.capture.take() {
                let result = match Pin::new(&mut pending.job).poll(cx) {
                    Poll::Pending => {
                        preparing.capture = Some(pending);
                        return Poll::Pending;
                    }
                    Poll::Ready(result) => result,
                };
                self.runtime.settle(&pending.execution, Ok(()))?;
                if matches!(result, Err(Error::Script(_))) {
                    self.retire_failed_realm(&pending.child)?;
                }
                let plan = result?;
                match plan {
                    CapturePlan::Forward { source } => {
                        preparing.capture = Some(self.begin_capture(
                            &pending.child,
                            source,
                            &pending.output,
                            pending.edges,
                            update,
                        )?);
                    }
                    CapturePlan::Native => {
                        if !matches!(
                            pending.execution.value(),
                            Export::Component {
                                implementation: Component::Native(_),
                                ..
                            }
                        ) {
                            return Poll::Ready(Err(Error::Invalid(
                                "JavaScript cannot forge a native capture".into(),
                            )));
                        }
                        let target = CaptureTarget {
                            mount: pending.child,
                            method: pending.source.method,
                            input: pending.source.input,
                            output: pending.output,
                            edges: pending.edges,
                        };
                        let prepared = preparing.prepared.as_mut().expect("preparing action");
                        if let Some(slot) = preparing.slot.take() {
                            prepared.captures.insert(slot, target);
                        } else {
                            prepared.continuations.push(target);
                        }
                    }
                }
                continue;
            }
            let Some((slot, source, output)) = preparing.requests.pop_front() else {
                return Poll::Ready(Ok(preparing.prepared.take().expect("completed action")));
            };
            preparing.slot = slot;
            let consumer = preparing
                .prepared
                .as_ref()
                .expect("preparing action")
                .consumer
                .clone();
            preparing.capture =
                Some(self.begin_capture(&consumer, source, &output, Vec::new(), update)?);
        }
    }
    pub(super) fn invalidate_pending_captures(&mut self) {
        // Committed source changes permanently revoke pending original captures.
        // Restoring a slot in a later update cannot revive the old invocation.
        let invalid: Vec<_> = self
            .actions
            .queue
            .iter()
            .chain(self.actions.preparing.values().map(|p| &p.action))
            .filter(|action| {
                action
                    .captures
                    .values()
                    .chain(&action.continuations)
                    .chain(action.witnesses.iter().map(|c| &c.target))
                    .any(|target| !self.target_valid(target))
            })
            .map(|action| action.id)
            .collect();
        for action in &mut self.actions.queue {
            action.invalidated |= invalid.contains(&action.id);
        }
        for preparing in self.actions.preparing.values_mut() {
            preparing.action.invalidated |= invalid.contains(&preparing.action.id);
        }
        let running: Vec<_> = self
            .actions
            .running
            .iter()
            .filter(|(_, r)| {
                r.continuations
                    .iter()
                    .any(|c| !self.target_valid(&c.target))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in running {
            for c in &mut self
                .actions
                .running
                .get_mut(&id)
                .expect("original running query")
                .continuations
            {
                c.invalidated = true;
            }
        }
        let outcomes: Vec<_> = self
            .actions
            .outcomes
            .iter()
            .enumerate()
            .filter(|(_, o)| {
                o.continuations
                    .iter()
                    .any(|c| !self.target_valid(&c.target))
            })
            .map(|(index, _)| index)
            .collect();
        for index in outcomes {
            for c in &mut self.actions.outcomes[index].continuations {
                c.invalidated = true;
            }
        }
    }
    fn target_valid(&self, target: &CaptureTarget) -> bool {
        // A visual continuation must remain on the current committed visible route.
        // Resource captures are independent of physical presentation.
        if target.output == CaptureOutput::Continuation && !self.mount_visible(&target.mount) {
            return false;
        }
        target.edges.iter().all(|e| {
            self.views
                .get(&e.parent.id())
                .and_then(|v| v.children.get(&e.slot))
                .is_some_and(|c| c.mount == e.child && !self.is_retiring(&e.child))
        })
    }
    fn reject_action(&mut self, action: Prepared<V>, reason: String) {
        let reason = bounded_reason(reason);
        self.actions.outcomes.push_back(Outcome {
            consumer: action.consumer,
            key: action.key,
            outcome: ActionOutcome::NotAccepted { reason },
            request: action.subscription.then_some(action.id),
            continuations: Vec::new(),
        });
    }
    /// Run after native owners are prepared, before native commands/cleanup.
    /// Capture failure rejects this operation, not the already-published scene.
    pub fn capture_actions(
        &mut self,
        root: &Mount,
        mut capture: impl FnMut(&CaptureTarget) -> Result<NativeCapture>,
    ) -> Result<usize> {
        let mut captured = 0;
        for _ in 0..self.actions.queue.len() {
            let mut action = self.actions.queue.pop_front().expect("bounded queue");
            if self
                .mount_root(&action.consumer)
                .as_ref()
                .is_ok_and(|r| r != root)
                || action.resources.is_some()
            {
                self.actions.queue.push_back(action);
                continue;
            }
            if action.invalidated
                || action.subscription
                    && !self.subscription_wanted(&action.consumer, &action.key, action.id)
                || self.is_retiring(&action.consumer)
                || action
                    .captures
                    .values()
                    .chain(&action.continuations)
                    .any(|t| !self.target_valid(t))
            {
                self.reject_action(action, "capture source or consumer retired".into());
                continue;
            }
            let result = (|| {
                let mut resources = Attachments::new();
                let mut bytes = self.actions.bytes();
                for (slot, target) in &action.captures {
                    let NativeCapture::Resource(resource) = capture(target)? else {
                        return Err(Error::Invalid("native capture output mismatch".into()));
                    };
                    if target.output
                        != (CaptureOutput::Resource {
                            resource: resource.resource_type(),
                        })
                    {
                        return Err(Error::Invalid("native resource type mismatch".into()));
                    }
                    bytes = bytes
                        .checked_add(resource.bytes())
                        .ok_or_else(|| Error::Invalid("resource byte overflow".into()))?;
                    if bytes > 64 * 1024 * 1024 {
                        return Err(Error::Invalid("action resources exceed 64 MiB".into()));
                    }
                    resources.insert(slot.clone(), resource);
                }
                let mut witnesses = Vec::new();
                for target in &action.continuations {
                    let NativeCapture::Continuation(witness) = capture(target)? else {
                        return Err(Error::Invalid("native capture output mismatch".into()));
                    };
                    bytes = bytes
                        .checked_add(witness.bytes())
                        .ok_or_else(|| Error::Invalid("continuation byte overflow".into()))?;
                    if bytes > 64 * 1024 * 1024 {
                        return Err(Error::Invalid("action resources exceed 64 MiB".into()));
                    }
                    witnesses.push(witness);
                }
                Ok((resources, witnesses))
            })();
            match result {
                Ok((resources, witnesses)) => {
                    action.resources = Some(resources);
                    action.witnesses = action
                        .continuations
                        .drain(..)
                        .zip(witnesses)
                        .map(|(target, witness)| Continuation {
                            target,
                            witness,
                            invalidated: false,
                        })
                        .collect();
                    self.actions.queue.push_back(action);
                    captured += 1;
                }
                Err(error) => self.reject_action(action, error.to_string()),
            }
        }
        Ok(captured)
    }
    /// Final Mount and captured registration check. A queue is not accepted Work.
    pub fn start_task(&mut self, root: &Mount) -> Result<Option<ActionTask>> {
        loop {
            let Some(index) = self.actions.queue.iter().position(|a| {
                a.resources.is_some()
                    && (self.is_retiring(&a.consumer)
                        || self
                            .mount_root(&a.consumer)
                            .as_ref()
                            .is_ok_and(|r| r == root))
            }) else {
                return Ok(None);
            };
            let action = self.actions.queue.remove(index).expect("queue position");
            if action.invalidated
                || action.subscription
                    && !self.subscription_wanted(&action.consumer, &action.key, action.id)
                || self.is_retiring(&action.consumer)
                || action
                    .captures
                    .values()
                    .chain(action.witnesses.iter().map(|c| &c.target))
                    .any(|t| !self.target_valid(t))
            {
                self.reject_action(action, "consumer retired before admission".into());
                continue;
            }
            let mode = match self.runtime.inspect(&action.capability) {
                Ok(Export::Service { methods, .. }) => methods[&action.method].mode,
                Ok(_) => unreachable!(),
                Err(error) => {
                    self.reject_action(action, error.to_string());
                    continue;
                }
            };
            if mode == MethodMode::Mutation {
                if let Some(task) = self.start_mutation(action)? {
                    return Ok(Some(task));
                }
                continue;
            }
            let execution = match self.runtime.admit(&action.capability) {
                Ok(execution) => execution,
                Err(error) => {
                    self.reject_action(action, error.to_string());
                    continue;
                }
            };
            let Export::Service { implementation, .. } = execution.value() else {
                unreachable!()
            };
            let implementation = implementation.clone();
            let resources = action.resources.expect("captured");
            let method = action.method;
            let id = action.id;
            let request = ServiceRequest {
                source: ServiceSource {
                    deployment: action.source.map(|source| source.reference().clone()),
                    consumer: action.capability.consumer().clone(),
                    mount: action.consumer.clone(),
                    request: id,
                },
                method: method.clone(),
                input: action.input,
                resources: resources.clone(),
            };
            let flow = action.subscription.then(subscriptions::Flow::new);
            let stop = flow.as_ref().map(|flow| flow.stop.clone());
            self.actions.running.insert(
                id,
                Running {
                    consumer: action.consumer,
                    key: action.key,
                    method,
                    completed: false,
                    execution,
                    resources,
                    mutation: None,
                    stream: flow,
                    continuations: action.witnesses,
                },
            );
            return Ok(Some(ActionTask {
                id,
                future: Box::pin(async move {
                    match stop {
                        Some(stop) => TaskCompletion::Subscription(
                            implementation.subscribe(request, stop).await,
                        ),
                        None => TaskCompletion::Query(implementation.call(request).await),
                    }
                }),
            }));
        }
    }
    /// Executor completion is independent of result delivery. Failed/unknown
    /// cleanup preserves Work and resource ownership; it is never an implicit Ok.
    fn finish_query(&mut self, id: Uuid, completion: ServiceCompletion) -> Result<()> {
        let running = self
            .actions
            .running
            .get(&id)
            .ok_or_else(|| Error::Invalid("unknown accepted action".into()))?;
        if running.completed {
            return Err(Error::Invalid("action outcome already completed".into()));
        }
        if running.mutation.is_some() {
            return Err(Error::Invalid(
                "mutation requires its original receipt".into(),
            ));
        }
        let output = match completion.result {
            Ok(value) => {
                let Export::Service { methods, .. } = running.execution.value() else {
                    unreachable!()
                };
                let valid = methods[&running.method]
                    .output
                    .validate(&value)
                    .map_err(|e| Error::Invalid(e.to_string()));
                match valid {
                    Ok(()) => ActionOutcome::Succeeded { value },
                    Err(e) => ActionOutcome::Failed {
                        reason: bounded_reason(e.to_string()),
                    },
                }
            }
            Err(reason) => ActionOutcome::Failed {
                reason: if reason.len() > 64 * 1024 {
                    "service error exceeds 64 KiB".into()
                } else {
                    reason
                },
            },
        };
        let consumer = running.consumer.clone();
        let key = running.key.clone();
        let request = running.stream.as_ref().map(|_| id);
        let cleanup_result = completion.cleanup.map_err(bounded_reason);
        let failure = cleanup_result.as_ref().err().cloned();
        let cleanup = self.runtime.settle(&running.execution, cleanup_result);
        let continuations = std::mem::take(
            &mut self
                .actions
                .running
                .get_mut(&id)
                .expect("running")
                .continuations,
        );
        self.actions
            .running
            .get_mut(&id)
            .expect("running")
            .completed = true;
        self.actions.outcomes.push_back(Outcome {
            consumer,
            key,
            outcome: output,
            request,
            continuations,
        });
        cleanup?;
        if let Some(reason) = failure {
            return Err(Error::Core(maka_plugin_core::Error::CleanupFailed(vec![
                reason,
            ])));
        }
        self.actions.running.remove(&id);
        Ok(())
    }
    /// Recovery may acknowledge only actual resource cleanup after an unknown
    /// result. It cannot rewrite or redeliver the previously completed outcome.
    pub fn complete_action_cleanup(
        &mut self,
        id: Uuid,
        cleanup: std::result::Result<(), String>,
    ) -> Result<()> {
        if self.actions.preparing.contains_key(&id) {
            return self.confirm_preparation_cleanup(id, cleanup);
        }
        let running = self
            .actions
            .running
            .get(&id)
            .ok_or_else(|| Error::Invalid("unknown accepted action".into()))?;
        if !running.completed {
            return Err(Error::Invalid("executor has not completed".into()));
        }
        let cleanup = cleanup.map_err(bounded_reason);
        let failure = cleanup.as_ref().err().cloned();
        self.runtime.settle(&running.execution, cleanup)?;
        if let Some(reason) = failure {
            return Err(Error::Core(maka_plugin_core::Error::CleanupFailed(vec![
                reason,
            ])));
        }
        self.actions.running.remove(&id);
        Ok(())
    }
    pub fn pending_action_count(&self) -> usize {
        self.actions.len()
    }
    pub fn failed_action_ids(&self) -> Vec<Uuid> {
        self.actions
            .running
            .iter()
            .filter(|(_, r)| r.completed)
            .map(|(id, _)| *id)
            .chain(
                self.actions
                    .preparing
                    .iter()
                    .filter(|(_, p)| p.failed())
                    .map(|(id, _)| *id),
            )
            .collect()
    }

    pub fn accepted_action_count(&self) -> usize {
        self.actions.running.len()
    }
    /// No raw-input authority: only outcomes produced by owned requests can
    /// reach these handler keys. Each feedback is a new bounded UI transaction.
    pub fn deliver_action_results(
        &mut self,
        root: &Mount,
        mut valid: impl FnMut(&CaptureTarget, &V, &ContinuationWitness) -> Result<bool>,
    ) -> Result<usize> {
        // A native commit may have just released the logical transaction while
        // older physical input is still queued. Feedback cannot bypass it.
        if self.ui_pending() {
            return Ok(0);
        }
        let mut count = 0;
        let mut first_error = None;
        for _ in 0..self.actions.outcomes.len() {
            let outcome = self.actions.outcomes.pop_front().expect("bounded outcomes");
            let item = matches!(&outcome.outcome, ActionOutcome::Updated { .. });
            if self.is_retiring(&outcome.consumer)
                || outcome.request.is_some_and(|id| {
                    if item {
                        !self.update_wanted(&outcome.consumer, &outcome.key, id)
                    } else {
                        // A subscription's settled execution is already gone;
                        // its original demand still decides terminal delivery.
                        !self.subscription_wanted(&outcome.consumer, &outcome.key, id)
                    }
                })
            {
                continue;
            }
            if self
                .mount_root(&outcome.consumer)
                .as_ref()
                .is_ok_and(|r| r != root)
            {
                self.actions.outcomes.push_back(outcome);
                continue;
            }
            // Check before invoking any callback: the callback can replace the
            // entire scene and retire the very source these witnesses protect.
            let mut stale = false;
            for c in &outcome.continuations {
                if c.invalidated
                    || !self.target_valid(&c.target)
                    || !self.views.get(&c.target.mount.id()).is_some_and(|view| {
                        valid(&c.target, view.frame.view.view.as_ref(), &c.witness).unwrap_or(false)
                    })
                {
                    stale = true;
                    break;
                }
            }
            let feedback = self.action_feedback(
                &outcome.consumer,
                if stale {
                    ActionOutcome::Stale
                } else {
                    outcome.outcome
                },
            );
            if item
                && !matches!(&feedback, ActionOutcome::Updated { .. })
                && let Some(id) = outcome.request
            {
                self.fail_update(id, "progress delivery failed".into());
            }
            match self.dispatch_action_result(
                &outcome.consumer,
                &outcome.key,
                serde_json::to_value(feedback).expect("typed outcome"),
                tree::Feedback {
                    consumer: outcome.consumer.clone(),
                    request: outcome.request,
                    item,
                },
            ) {
                Ok(()) => {
                    count += 1;
                    if self.logical_pending() {
                        break;
                    }
                }
                Err(error) => {
                    // No healthy consumer may remain waiting on an outcome
                    // its handler rejected. Work has already settled separately.
                    self.runtime.retire_mount(&outcome.consumer)?;
                    self.sync_subscription_demand();
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(count)
    }
    pub(super) fn retire_failed_realm(&mut self, mount: &Mount) -> Result<()> {
        let execution = self.runtime.admit_mount(mount)?;
        let realm = match execution.value() {
            Export::Component {
                implementation: Component::JavaScript { realm, .. },
                ..
            } => Some(realm.clone()),
            _ => None,
        };
        self.runtime.settle(&execution, Ok(()))?;
        if let Some(realm) = realm {
            for (id, loaded) in &self.modules {
                if Rc::ptr_eq(&realm, &loaded.realm) {
                    self.runtime.retire(&self.activations[id])?;
                }
            }
        }
        self.sync_subscription_demand();
        Ok(())
    }
}

fn bounded_reason(reason: String) -> String {
    if reason.len() > 64 * 1024 {
        "action diagnostic exceeds 64 KiB".into()
    } else {
        reason
    }
}
