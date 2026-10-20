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

//! Committed demand, one-item feedback backpressure and original executor custody.
use super::*;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(crate) struct Demand {
    pub action: Action,
    pub request: Uuid,
}
pub(super) struct Flow {
    pub stop: CancellationToken,
    pub(super) ack: Arc<Semaphore>,
    pub(super) failure: Option<String>,
    pub(super) pending_feedback: bool,
}
impl Flow {
    pub fn new() -> Self {
        Self {
            stop: CancellationToken::new(),
            ack: Arc::new(Semaphore::new(0)),
            failure: None,
            pending_feedback: false,
        }
    }
}
impl Drop for Flow {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
impl<V: ?Sized + 'static> Application<V> {
    pub(crate) fn subscription_wanted(&self, consumer: &Mount, key: &str, request: Uuid) -> bool {
        !self.is_retiring(consumer)
            && self
                .views
                .get(&consumer.id())
                .and_then(|view| view.subscriptions.get(key))
                .is_some_and(|demand| demand.request == request)
    }
    pub(super) fn update_wanted(&self, consumer: &Mount, key: &str, id: Uuid) -> bool {
        !self.is_retiring(consumer)
            && self.actions.running.get(&id).is_some_and(|running| {
                &running.consumer == consumer
                    && running.key == key
                    && (running.mutation.is_some() || self.subscription_wanted(consumer, key, id))
            })
    }
    pub(crate) fn sync_subscription_demand(&self) {
        for (id, running) in &self.actions.running {
            if let Some(flow) = &running.stream
                && !self.subscription_wanted(&running.consumer, &running.key, *id)
            {
                flow.stop.cancel();
            }
            if let Some(session) = &running.mutation
                && self.is_retiring(&running.consumer)
            {
                session.progress.stop.cancel();
            }
        }
    }
    pub(crate) fn acknowledge_update(&mut self, id: Uuid) {
        if let Some(flow) = self.actions.running.get_mut(&id).and_then(|r| {
            r.stream
                .as_mut()
                .or_else(|| r.mutation.as_mut().map(|s| &mut s.progress))
        }) && flow.pending_feedback
            && !flow.stop.is_cancelled()
        {
            flow.pending_feedback = false;
            flow.ack.add_permits(1);
        }
    }
    pub(crate) fn fail_update(&mut self, id: Uuid, reason: String) {
        if let Some(flow) = self.actions.running.get_mut(&id).and_then(|r| {
            r.stream
                .as_mut()
                .or_else(|| r.mutation.as_mut().map(|s| &mut s.progress))
        }) {
            flow.failure.get_or_insert(reason);
            flow.stop.cancel();
        }
    }
    pub(super) fn finish_subscription(
        &mut self,
        id: Uuid,
        completion: StreamCompletion,
    ) -> Result<Option<ActionTask>> {
        let running = self
            .actions
            .running
            .get(&id)
            .ok_or_else(|| Error::Invalid("unknown subscription".into()))?;
        if running.completed || running.stream.is_none() {
            return Err(Error::Invalid(
                "request is not an active subscription".into(),
            ));
        }
        match completion {
            StreamCompletion::Finished(mut completion) => {
                if let Some(reason) = running
                    .stream
                    .as_ref()
                    .and_then(|flow| flow.failure.clone())
                {
                    completion.result = Err(reason);
                }
                self.finish_query(id, completion)?;
                Ok(None)
            }
            StreamCompletion::Item { value, next } => {
                if running
                    .stream
                    .as_ref()
                    .is_some_and(|flow| flow.pending_feedback && !flow.stop.is_cancelled())
                {
                    return Err(Error::Invalid(
                        "subscription feedback has not committed".into(),
                    ));
                }
                let wanted = self.subscription_wanted(&running.consumer, &running.key, id);
                let valid = tree::serialized_bytes(&value).and_then(|bytes| {
                    if bytes > maka_plugin_core::json::SUBSCRIPTION_ITEM_BYTES {
                        return Err(Error::Invalid("subscription item exceeds 64 KiB".into()));
                    }
                    let Export::Service { methods, .. } = running.execution.value() else {
                        unreachable!()
                    };
                    methods[&running.method]
                        .output
                        .validate(&value)
                        .map_err(|e| Error::Invalid(e.to_string()))
                });
                let consumer = running.consumer.clone();
                let key = running.key.clone();
                if let Err(error) = valid {
                    self.fail_update(id, bounded_reason(error.to_string()));
                }
                let flow = self
                    .actions
                    .running
                    .get_mut(&id)
                    .expect("running")
                    .stream
                    .as_mut()
                    .expect("checked stream");
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
                    flow.stop.cancel();
                }
                let stop = flow.stop.clone();
                let ack = flow.ack.clone();
                Ok(Some(ActionTask {
                    id,
                    future: Box::pin(async move {
                        // Every successor remains with the original foreground owner,
                        // including after the last surface has gone. Stop bypasses
                        // ACK but still polls the source's actual close/cleanup path.
                        tokio::select! {
                            biased;
                            () = stop.cancelled() => {},
                            permit = ack.acquire_owned() => { if let Ok(permit) = permit { permit.forget(); } },
                        }
                        TaskCompletion::Subscription(next.await)
                    }),
                }))
            }
        }
    }
}
