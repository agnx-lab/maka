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

//! Prepare and persist without Work; return to the original owner for admission.
use super::*;
use maka_plugin_core::operation::{OperationOutcome, Receipt};
use std::{panic::AssertUnwindSafe, sync::OnceLock};

pub(super) struct Session {
    owner: Arc<dyn NativeMutation>,
    reference: Arc<OnceLock<RecoveryRef>>,
    caller: Option<Box<CallerConfirmation>>,
    pub(super) progress: subscriptions::Flow,
    cancelled: bool,
}
enum Phase {
    Preparing,
    Closing,
    Failed,
}
pub(super) struct Preparing<V: ?Sized> {
    pub(super) action: Prepared<V>,
    resource: Resource,
    session: Option<Session>,
    // A confirmed caller token remains owned through aborted preparation close.
    recorded: Option<RecordedIntent>,
    phase: Phase,
    outcome: Option<ActionOutcome>,
}
impl<V: ?Sized> Preparing<V> {
    pub(super) fn failed(&self) -> bool {
        matches!(self.phase, Phase::Failed)
    }
}

impl Session {
    fn cancel(&mut self) {
        self.cancelled = true;
        self.progress.stop.cancel();
        if std::panic::catch_unwind(AssertUnwindSafe(|| self.owner.cancel())).is_err() {
            self.progress.failure.get_or_insert_with(|| {
                "native cancellation panicked; read original receipt".into()
            });
        }
    }
    fn failure(&self, reason: String) -> ActionOutcome {
        match self.reference.get() {
            Some(reference) => unavailable(reference.clone(), self.caller.clone(), reason),
            None => ActionOutcome::NotAccepted { reason },
        }
    }
}

impl<V: ?Sized + 'static> Application<V> {
    pub(crate) fn cancel_mutation(&mut self, consumer: &Mount, key: &str) {
        let queued = std::mem::take(&mut self.actions.queue);
        for action in queued {
            if !action.subscription && action.consumer == *consumer && action.key == key {
                self.reject_action(action, "action cancelled before admission".into());
            } else {
                self.actions.queue.push_back(action);
            }
        }
        for preparing in self.actions.preparing.values_mut() {
            if preparing.action.consumer == *consumer
                && preparing.action.key == key
                && let Some(session) = &mut preparing.session
            {
                session.cancel();
            }
        }
        for running in self.actions.running.values_mut() {
            if running.consumer == *consumer
                && running.key == key
                && let Some(session) = &mut running.mutation
            {
                session.cancel();
            }
        }
    }
    pub(super) fn start_mutation(&mut self, action: Prepared<V>) -> Result<Option<ActionTask>> {
        let (Some(source), Some(store)) = (action.source.clone(), self.store.clone()) else {
            self.reject_action(action, "mutation requires a persisted UI source".into());
            return Ok(None);
        };
        let Export::Service { implementation, .. } = self.runtime.inspect(&action.capability)?
        else {
            unreachable!()
        };
        let implementation = implementation.clone();
        let resource = self
            .runtime
            .own_resource(action.capability.provider(), "mutation preparation")?;
        let id = action.id;
        let consumer = action.capability.consumer().instance().clone();
        let recovery = action.recovery.clone();
        let request = ServiceRequest {
            source: ServiceSource {
                deployment: Some(source.reference().clone()),
                consumer: action.capability.consumer().clone(),
                mount: action.consumer.clone(),
                request: id,
            },
            method: action.method.clone(),
            input: action.input.clone(),
            resources: action.resources.as_ref().expect("captured").clone(),
        };
        let owner = std::panic::catch_unwind(AssertUnwindSafe(|| implementation.mutation(request)));
        let owner = match owner {
            Ok(Ok(owner)) => owner,
            Ok(Err(reason)) => {
                self.runtime.complete_resource(&resource, Ok(()))?;
                self.reject_action(action, reason);
                return Ok(None);
            }
            Err(_) => {
                let reason =
                    "mutation owner construction panicked; cleanup unconfirmed".to_string();
                self.actions.outcomes.push_back(Outcome {
                    continuations: Vec::new(),
                    consumer: action.consumer.clone(),
                    key: action.key.clone(),
                    outcome: ActionOutcome::NotAccepted {
                        reason: reason.clone(),
                    },
                    request: None,
                });
                self.actions.preparing.insert(
                    id,
                    Preparing {
                        action,
                        resource: resource.clone(),
                        session: None,
                        recorded: None,
                        phase: Phase::Failed,
                        outcome: None,
                    },
                );
                let completed = self.runtime.complete_resource(&resource, Err(reason));
                self.sync_subscription_demand();
                completed?;
                return Ok(None);
            }
        };
        let reference = Arc::new(OnceLock::new());
        self.actions.preparing.insert(
            id,
            Preparing {
                action,
                resource,
                session: Some(Session {
                    owner: owner.clone(),
                    reference: reference.clone(),
                    caller: None,
                    progress: subscriptions::Flow::new(),
                    cancelled: false,
                }),
                recorded: None,
                phase: Phase::Preparing,
                outcome: None,
            },
        );
        Ok(Some(ActionTask {
            id,
            future: Box::pin(async move {
                let prepared = async {
                    let original = owner.prepare().await?;
                    original.validate().map_err(|e| e.to_string())?;
                    maka_plugin_core::json::exact_numbers(
                        &serde_json::to_value(&original).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    if original.operation != id {
                        return Err(
                            "native mutation changed the original operation identity".into()
                        );
                    }
                    // This immutable locator is retained even if SQL commit or
                    // task delivery becomes unknown. It grants no execution.
                    reference
                        .set(original.clone())
                        .map_err(|_| "original operation already prepared")?;
                    store
                        .record_application_intent_from(&source, consumer, original, recovery)
                        .await
                        .map_err(|e| e.to_string())
                }
                .await;
                TaskCompletion::Prepared(prepared)
            }),
        }))
    }

    /// Complete on the original foreground owner and execute any returned task
    /// even after its presentation has disappeared. No error implies cleanup.
    pub fn finish_task(
        &mut self,
        id: Uuid,
        completion: TaskCompletion,
    ) -> Result<Option<ActionTask>> {
        let result = (|| match completion {
            TaskCompletion::Subscription(completion) => self.finish_subscription(id, completion),
            TaskCompletion::Query(completion) => {
                if self
                    .actions
                    .running
                    .get(&id)
                    .is_some_and(|r| r.stream.is_some())
                {
                    return Err(Error::Invalid(
                        "subscription requires its original stream completion".into(),
                    ));
                }
                self.finish_query(id, completion)?;
                Ok(None)
            }
            TaskCompletion::Prepared(recorded) => self.finish_preparation(id, recorded),
            TaskCompletion::MutationProgress { value, next } => {
                self.finish_mutation_progress(id, value, next)
            }
            TaskCompletion::Mutation(completion) => {
                self.finish_mutation(id, completion)?;
                Ok(None)
            }
            TaskCompletion::Closed(cleanup) => {
                self.finish_preparation_close(id, cleanup)?;
                Ok(None)
            }
            TaskCompletion::Interrupted(reason) => {
                self.interrupt_task(id, reason)?;
                Ok(None)
            }
        })();
        self.sync_subscription_demand();
        result
    }

    fn finish_preparation(
        &mut self,
        id: Uuid,
        recorded: std::result::Result<RecordedIntent, String>,
    ) -> Result<Option<ActionTask>> {
        let preparing = self
            .actions
            .preparing
            .get(&id)
            .ok_or_else(|| Error::Invalid("unknown preparation".into()))?;
        if !matches!(preparing.phase, Phase::Preparing) {
            return Err(Error::Invalid("preparation already completed".into()));
        }
        let session = preparing.session.as_ref().expect("constructed owner");
        let gate = recorded.and_then(|recorded| {
            if session.reference.get() != Some(&recorded.intent().reference) {
                return Err("caller token disagrees with original preparation".into());
            }
            Ok(recorded)
        });
        let recorded = match gate {
            Ok(recorded) => recorded,
            Err(reason) => return self.close_preparation(id, reason, None),
        };
        if session.cancelled
            || self.is_retiring(&preparing.action.consumer)
            || preparing.action.invalidated
            || preparing
                .action
                .captures
                .values()
                .any(|target| !self.target_valid(target))
        {
            return self.close_preparation(
                id,
                "mutation cancelled or consumer retired before admission".into(),
                Some(recorded),
            );
        }
        let execution = match self.runtime.admit(&preparing.action.capability) {
            Ok(execution) => execution,
            Err(error) => return self.close_preparation(id, error.to_string(), Some(recorded)),
        };
        let preparing = self.actions.preparing.remove(&id).expect("preparing");
        let mut session = preparing.session.expect("constructed owner");
        session.caller = Some(Box::new(recorded.intent().confirmation()));
        let owner = session.owner.clone();
        self.actions.running.insert(
            id,
            Running {
                continuations: Vec::new(),
                consumer: preparing.action.consumer,
                key: preparing.action.key,
                method: preparing.action.method,
                completed: false,
                execution,
                resources: preparing.action.resources.expect("captured"),
                mutation: Some(session),
                stream: None,
            },
        );
        // Accepted Work protects the provider before its preparation fence ends.
        self.runtime
            .complete_resource(&preparing.resource, Ok(()))?;
        let (send, receive) = tokio::sync::mpsc::channel(1);
        Ok(Some(ActionTask {
            id,
            future: mutation_progress(owner.dispatch(recorded, send), receive),
        }))
    }

    fn close_preparation(
        &mut self,
        id: Uuid,
        reason: String,
        recorded: Option<RecordedIntent>,
    ) -> Result<Option<ActionTask>> {
        let preparing = self.actions.preparing.get_mut(&id).expect("preparing");
        let session = preparing.session.as_mut().expect("constructed owner");
        session.caller = recorded
            .as_ref()
            .map(|recorded| Box::new(recorded.intent().confirmation()));
        preparing.outcome = Some(session.failure(reason));
        preparing.recorded = recorded;
        preparing.phase = Phase::Closing;
        let owner = session.owner.clone();
        Ok(Some(ActionTask {
            id,
            future: Box::pin(async move { TaskCompletion::Closed(owner.close().await) }),
        }))
    }

    fn finish_preparation_close(
        &mut self,
        id: Uuid,
        cleanup: std::result::Result<(), String>,
    ) -> Result<()> {
        let preparing = self
            .actions
            .preparing
            .get_mut(&id)
            .ok_or_else(|| Error::Invalid("unknown preparation close".into()))?;
        if !matches!(preparing.phase, Phase::Closing) {
            return Err(Error::Invalid("preparation is not closing".into()));
        }
        let failure = cleanup.as_ref().err().cloned();
        preparing.phase = Phase::Failed;
        if let Some(outcome) = preparing.outcome.take() {
            self.actions.outcomes.push_back(Outcome {
                continuations: Vec::new(),
                consumer: preparing.action.consumer.clone(),
                key: preparing.action.key.clone(),
                outcome,
                request: None,
            });
        }
        self.runtime
            .complete_resource(&preparing.resource, cleanup)?;
        if let Some(reason) = failure {
            return Err(Error::Core(maka_plugin_core::Error::CleanupFailed(vec![
                reason,
            ])));
        }
        self.actions.preparing.remove(&id);
        Ok(())
    }

    pub(super) fn confirm_preparation_cleanup(
        &mut self,
        id: Uuid,
        cleanup: std::result::Result<(), String>,
    ) -> Result<()> {
        let preparing = self.actions.preparing.get_mut(&id).expect("preparation");
        if !preparing.failed() {
            return Err(Error::Invalid("preparation has not completed".into()));
        }
        // Only an embedding's real recovery receipt may confirm the original
        // task/session cleanup. This neither repeats preparation nor dispatches.
        preparing.phase = Phase::Closing;
        self.finish_preparation_close(id, cleanup)
    }

    fn finish_mutation_progress(
        &mut self,
        id: Uuid,
        value: Value,
        next: TaskFuture,
    ) -> Result<Option<ActionTask>> {
        let running = self
            .actions
            .running
            .get(&id)
            .ok_or_else(|| Error::Invalid("unknown mutation progress".into()))?;
        let session = running
            .mutation
            .as_ref()
            .filter(|_| !running.completed)
            .ok_or_else(|| Error::Invalid("progress requires an active mutation".into()))?;
        let Export::Service { methods, .. } = running.execution.value() else {
            unreachable!()
        };
        let valid = methods[&running.method]
            .progress
            .as_ref()
            .ok_or_else(|| Error::Invalid("mutation has no progress declaration".into()))
            .and_then(|progress| {
                progress
                    .validate(&value)
                    .map_err(|e| Error::Invalid(e.to_string()))
            });
        let consumer = running.consumer.clone();
        let key = running.key.clone();
        let wanted = self.update_wanted(&consumer, &key, id);
        if session.progress.pending_feedback && !session.progress.stop.is_cancelled() {
            return Err(Error::Invalid(
                "mutation progress feedback has not committed".into(),
            ));
        }
        if let Err(error) = valid {
            self.fail_update(id, error.to_string());
        }
        let flow = &mut self
            .actions
            .running
            .get_mut(&id)
            .expect("running")
            .mutation
            .as_mut()
            .expect("mutation")
            .progress;
        if wanted && !flow.stop.is_cancelled() {
            flow.pending_feedback = true;
            self.actions.outcomes.push_back(Outcome {
                continuations: Vec::new(),
                consumer,
                key,
                outcome: ActionOutcome::Updated { value },
                request: Some(id),
            });
        } else {
            // Closing only the presentation drains the original mutation. It is
            // neither cancellation of accepted effects nor physical cleanup.
            flow.stop.cancel();
        }
        let stop = flow.stop.clone();
        let ack = flow.ack.clone();
        Ok(Some(ActionTask {
            id,
            future: Box::pin(async move {
                tokio::select! { biased;
                    () = stop.cancelled() => {},
                    permit = ack.acquire_owned() => { if let Ok(permit) = permit { permit.forget(); } },
                }
                next.await
            }),
        }))
    }
    fn finish_mutation(&mut self, id: Uuid, completion: MutationCompletion) -> Result<()> {
        let running = self
            .actions
            .running
            .get(&id)
            .ok_or_else(|| Error::Invalid("unknown mutation".into()))?;
        if running.completed {
            return Err(Error::Invalid("mutation already completed".into()));
        }
        let session = running
            .mutation
            .as_ref()
            .ok_or_else(|| Error::Invalid("query has no operation receipt".into()))?;
        let original = session.reference.get().expect("prepared reference");
        let output = if let Some(reason) = &session.progress.failure {
            session.failure(format!(
                "mutation progress failed: {reason}; read original receipt"
            ))
        } else if completion.reply.reference != *original {
            session.failure(
                "native completion changed the original reference; read original receipt".into(),
            )
        } else {
            let candidate = ActionOutcome::Operation {
                caller: session.caller.clone(),
                reply: Box::new(completion.reply),
            };
            let valid: Result<()> = (|| {
                let ActionOutcome::Operation { reply, .. } = &candidate else {
                    unreachable!()
                };
                if let Receipt::Recorded {
                    outcome: OperationOutcome::Returned { value },
                    ..
                } = &reply.receipt
                {
                    let Export::Service { methods, .. } = running.execution.value() else {
                        unreachable!()
                    };
                    methods[&running.method]
                        .output
                        .validate(value)
                        .map_err(|e| Error::Invalid(e.to_string()))?;
                }
                Ok(())
            })();
            match valid {
                Ok(()) => self.action_feedback(&running.consumer, candidate),
                Err(error) => session.failure(format!(
                    "operation result delivery failed: {error}; read original receipt"
                )),
            }
        };
        let cleanup = completion.cleanup;
        let failure = cleanup.as_ref().err().cloned();
        self.runtime.settle(&running.execution, cleanup)?;
        self.actions.outcomes.push_back(Outcome {
            continuations: Vec::new(),
            consumer: running.consumer.clone(),
            key: running.key.clone(),
            outcome: output,
            request: None,
        });
        self.actions
            .running
            .get_mut(&id)
            .expect("running")
            .completed = true;
        if let Some(reason) = failure {
            return Err(Error::Core(maka_plugin_core::Error::CleanupFailed(vec![
                reason,
            ])));
        }
        self.actions.running.remove(&id);
        Ok(())
    }

    fn interrupt_task(&mut self, id: Uuid, reason: String) -> Result<()> {
        let reason = format!("executor interrupted; cleanup unconfirmed: {reason}");
        if let Some(preparing) = self.actions.preparing.get_mut(&id) {
            if matches!(preparing.phase, Phase::Failed) {
                return Err(Error::Invalid("preparation already failed".into()));
            }
            preparing.phase = Phase::Failed;
            let outcome = preparing.outcome.take().unwrap_or_else(|| {
                preparing
                    .session
                    .as_ref()
                    .expect("constructed owner")
                    .failure(reason.clone())
            });
            self.actions.outcomes.push_back(Outcome {
                continuations: Vec::new(),
                consumer: preparing.action.consumer.clone(),
                key: preparing.action.key.clone(),
                outcome,
                request: None,
            });
            self.runtime
                .complete_resource(&preparing.resource, Err(reason.clone()))?;
            return Err(Error::Core(maka_plugin_core::Error::CleanupFailed(vec![
                reason,
            ])));
        }
        let running = self
            .actions
            .running
            .get(&id)
            .ok_or_else(|| Error::Invalid("unknown interrupted task".into()))?;
        if let Some(session) = &running.mutation {
            let reference = session.reference.get().expect("prepared reference").clone();
            self.finish_mutation(
                id,
                MutationCompletion {
                    reply: OperationReply {
                        reference,
                        receipt: Receipt::Unavailable {
                            diagnostic: reason.clone(),
                        },
                    },
                    cleanup: Err(reason),
                },
            )
        } else {
            self.finish_query(
                id,
                ServiceCompletion {
                    result: Err(reason.clone()),
                    cleanup: Err(reason),
                },
            )
        }
    }
}

// Keep the same completion future and resource owner across each bounded item.
fn mutation_progress(
    mut completion: MutationFuture,
    mut receive: tokio::sync::mpsc::Receiver<Value>,
) -> TaskFuture {
    Box::pin(async move {
        tokio::select! { biased;
            value = receive.recv() => match value {
                Some(value) => TaskCompletion::MutationProgress {
                    value, next: mutation_progress(completion, receive),
                },
                None => TaskCompletion::Mutation(completion.await),
            },
            completed = &mut completion => TaskCompletion::Mutation(completed),
        }
    })
}
