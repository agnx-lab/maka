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

//! Bounded native observations, owned by the original renderer resource.
//! Queue admission transfers custody; only the original logical commit confirms
//! publication. Candidate rejection retains the exact observation and its ACK.
use crate::{
    Error, Result,
    backend::Job,
    budget::{Kind, NativeBudget, Permit},
};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Wake, Waker},
};
use tokio::sync::oneshot;

const MAX_PENDING: usize = 128;
struct Observation {
    key: String,
    value: Value,
    replaceable: bool,
    input: bool,
    rejected: Option<(u64, u64)>,
    _bytes: Permit,
    acknowledgement: oneshot::Sender<Job<()>>,
}
struct State {
    reserved: BTreeMap<String, VecDeque<Permit>>,
    pending: VecDeque<Observation>,
    publishing: Option<(usize, (u64, u64), Job<()>)>,
    closed: bool,
}
/// Capacity prepared by a trusted immutable recipe before its native commands.
/// Dropping a failed candidate releases it without touching the committed queue.
pub struct Capacity(Vec<(String, Permit)>);
struct Notification(std::sync::Arc<dyn Fn() + Send + Sync>);
impl Wake for Notification {
    fn wake(self: std::sync::Arc<Self>) {
        (self.0)();
    }
    fn wake_by_ref(self: &std::sync::Arc<Self>) {
        (self.0)();
    }
}
#[derive(Clone)]
pub struct Observations {
    state: Rc<RefCell<State>>,
    budget: NativeBudget,
    wake: Waker,
}
impl Observations {
    pub fn new(budget: NativeBudget, notify: impl Fn() + Send + Sync + 'static) -> Self {
        let wake = Waker::from(std::sync::Arc::new(Notification(std::sync::Arc::new(
            notify,
        ))));
        Self {
            state: Rc::new(RefCell::new(State {
                reserved: BTreeMap::new(),
                pending: VecDeque::new(),
                publishing: None,
                closed: false,
            })),
            budget,
            wake,
        }
    }
    pub fn prepare(
        &self,
        requirements: impl IntoIterator<Item = (&'static str, usize)>,
    ) -> Result<Capacity> {
        let state = self.state.borrow();
        if state.closed {
            return Err(Error::Invalid(
                "original native publication owner closed".into(),
            ));
        }
        let mut required = BTreeMap::<&str, (usize, usize)>::new();
        for (key, bytes) in requirements {
            let capacity = required.entry(key).or_default();
            capacity.0 += 1;
            capacity.1 = capacity.1.max(bytes);
        }
        let mut reserved = Vec::new();
        let mut retained_total: usize = state.reserved.values().map(VecDeque::len).sum();
        for (key, (slots, bytes)) in required {
            let retained = state.reserved.get(key).map_or(0, VecDeque::len);
            let extra = slots.saturating_sub(retained);
            retained_total += extra;
            if state.pending.len() + retained_total > MAX_PENDING {
                return Err(Error::Invalid(
                    "native publication capacity exhausted before command".into(),
                ));
            }
            for _ in 0..extra {
                reserved.push((
                    key.to_owned(),
                    self.budget
                        .reserve(Kind::Encoded, bytes)
                        .map_err(|e| Error::Invalid(e.to_string()))?,
                ));
            }
        }
        Ok(Capacity(reserved))
    }
    pub fn adopt(&self, capacity: Capacity) {
        let mut state = self.state.borrow_mut();
        assert!(!state.closed, "original native publication owner");
        for (key, permit) in capacity.0 {
            state.reserved.entry(key).or_default().push_back(permit);
        }
    }
    /// Capture at the native effect boundary, before a later command can change
    /// its receipt. Identical evidence already in custody needs no second ACK.
    pub fn capture(&self, key: &str, value: Value, replaceable: bool) -> Result<Option<Job<()>>> {
        self.enqueue(key, value, replaceable, false)
    }
    /// Independent user intents keep their original order and ACK even when
    /// payloads match. They follow observations already captured by this owner.
    pub fn capture_input(&self, key: &str, value: Value) -> Result<Job<()>> {
        self.enqueue(key, value, false, true)?
            .ok_or_else(|| Error::Invalid("original native input owner retired".into()))
    }
    fn enqueue(
        &self,
        key: &str,
        value: Value,
        replaceable: bool,
        input: bool,
    ) -> Result<Option<Job<()>>> {
        let mut state = self.state.borrow_mut();
        if state.closed {
            return Err(Error::Invalid(
                "original native publication owner closed".into(),
            ));
        }
        // Repeated producer polls do not add a second copy while other keys
        // interleave. User intents are never evidence deduplication candidates.
        if !input
            && state
                .pending
                .iter()
                .rev()
                .find(|entry| entry.key == key && !entry.input)
                .is_some_and(|entry| entry.value == value)
        {
            return Ok(None);
        }
        let bytes = serde_json::to_vec(&value)
            .map_err(|e| Error::Invalid(e.to_string()))?
            .len();

        let mut permit = if !input
            && !replaceable
            && state
                .reserved
                .get(key)
                .is_some_and(|slots| !slots.is_empty())
        {
            state.reserved.get_mut(key).unwrap().pop_front().unwrap()
        } else {
            self.budget
                .reserve(Kind::Encoded, bytes)
                .map_err(|e| Error::Invalid(e.to_string()))?
        };
        permit.shrink_to(bytes);
        if replaceable
            && state
                .pending
                .back()
                .is_some_and(|entry| entry.key == key && entry.replaceable && !entry.input)
            && state
                .publishing
                .as_ref()
                .is_none_or(|(index, _, _)| *index + 1 != state.pending.len())
        {
            if let Some(entry) = state.pending.pop_back() {
                let _ = entry.acknowledgement.send(Job::ready(Err(Error::Invalid(
                    "replaceable native observation superseded".into(),
                ))));
            }
        } else if state.pending.len() + state.reserved.values().map(VecDeque::len).sum::<usize>()
            >= MAX_PENDING
        {
            return Err(Error::Invalid(
                "native publication capacity exhausted".into(),
            ));
        }
        let (acknowledgement, result) = oneshot::channel();
        state.pending.push_back(Observation {
            key: key.to_owned(),
            value,
            replaceable,
            input,
            rejected: None,
            _bytes: permit,
            acknowledgement,
        });
        drop(state);
        self.wake.wake_by_ref();
        Ok(Some(Job::new(async move {
            result
                .await
                .map_err(|_| Error::Invalid("original native publication owner stopped".into()))?
                .await
        })))
    }
    /// This original owner publishes one ordered item at a time. A rejected
    /// observation remains in custody until a real business/source basis changes;
    /// rejected user intents finish with their original error and never replay.
    pub fn advance<C>(
        &self,
        revision: (u64, u64),
        context: &mut C,
        mut send: impl FnMut(&str, Value, &mut C) -> Result<Job<()>>,
        mut committed: impl FnMut(&str, &Value, &mut C),
    ) -> Vec<Error> {
        let mut state = self.state.borrow_mut();
        if state.closed {
            return Vec::new();
        }
        let mut cx = Context::from_waker(&self.wake);
        let mut errors = Vec::new();
        if let Some((index, basis, mut job)) = state.publishing.take() {
            match Pin::new(&mut job).poll(&mut cx) {
                Poll::Pending => state.publishing = Some((index, basis, job)),
                Poll::Ready(Ok(())) => {
                    let entry = state
                        .pending
                        .remove(index)
                        .expect("original publishing observation");
                    committed(&entry.key, &entry.value, context);
                    let _ = entry.acknowledgement.send(Job::ready(Ok(())));
                }
                Poll::Ready(Err(error)) => {
                    Self::reject(&mut state, index, basis, &error);
                    errors.push(error);
                }
            }
        }
        if state.publishing.is_none()
            && let Some(index) = Self::eligible(&state, revision)
        {
            let entry = &state.pending[index];
            match send(&entry.key, entry.value.clone(), context) {
                Ok(mut job) => match Pin::new(&mut job).poll(&mut cx) {
                    Poll::Pending => state.publishing = Some((index, revision, job)),
                    Poll::Ready(Ok(())) => {
                        let entry = state
                            .pending
                            .remove(index)
                            .expect("original publishing observation");
                        committed(&entry.key, &entry.value, context);
                        let _ = entry.acknowledgement.send(Job::ready(Ok(())));
                    }
                    Poll::Ready(Err(error)) => {
                        Self::reject(&mut state, index, revision, &error);
                        errors.push(error);
                    }
                },
                Err(error) => {
                    Self::reject(&mut state, index, revision, &error);
                    errors.push(error);
                }
            }
        }
        if state.publishing.is_none() && Self::eligible(&state, revision).is_some() {
            self.wake.wake_by_ref();
        }
        errors
    }
    fn eligible(state: &State, revision: (u64, u64)) -> Option<usize> {
        state.pending.iter().enumerate().position(|(index, entry)| {
            entry.rejected != Some(revision)
                && (entry.input
                    || !state
                        .pending
                        .iter()
                        .take(index)
                        .any(|prior| !prior.input && prior.key == entry.key))
        })
    }
    fn reject(state: &mut State, index: usize, basis: (u64, u64), error: &Error) {
        if state.pending[index].input {
            let entry = state
                .pending
                .remove(index)
                .expect("original rejected input");
            let _ = entry
                .acknowledgement
                .send(Job::ready(Self::rejection(error)));
            return;
        }
        // A first attempt fences the inputs already captured behind its
        // unpublished evidence. Later independent inputs may run while this
        // exact rejected observation remains retained for a changed basis.
        let first_attempt = state.pending[index].rejected.is_none();
        state.pending[index].rejected = Some(basis);
        if !first_attempt {
            return;
        }
        let mut retained = VecDeque::new();
        let mut position = 0;
        while let Some(entry) = state.pending.pop_front() {
            if position > index && entry.input {
                let _ = entry
                    .acknowledgement
                    .send(Job::ready(Self::rejection(error)));
            } else {
                retained.push_back(entry);
            }
            position += 1;
        }
        state.pending = retained;
    }
    fn rejection(error: &Error) -> Result<()> {
        Err(
            if matches!(error, Error::Core(maka_plugin_core::Error::Retired)) {
                maka_plugin_core::Error::Retired.into()
            } else {
                Error::Invalid(error.to_string())
            },
        )
    }
    /// A removed declaration cannot reinterpret the accepted input on a new
    /// control. Resolve its original custody with retirement, never fake success.
    pub fn retain(&self, mut declared: impl FnMut(&str) -> bool) {
        let mut state = self.state.borrow_mut();
        let mut publishing = state.publishing.take();
        let mut next_publishing = None;
        let mut retained = VecDeque::new();
        for (index, entry) in state.pending.drain(..).enumerate() {
            if declared(&entry.key) {
                if publishing
                    .as_ref()
                    .is_some_and(|(current, _, _)| *current == index)
                {
                    next_publishing = Some(retained.len());
                }
                retained.push_back(entry);
            } else {
                Self::acknowledge_removed(entry, index, &mut publishing);
            }
        }
        state.pending = retained;
        if let (Some((_, basis, job)), Some(index)) = (publishing, next_publishing) {
            state.publishing = Some((index, basis, job));
        }
        state.reserved.retain(|key, _| declared(key));
    }
    pub fn close(&self) {
        let mut state = self.state.borrow_mut();
        state.closed = true;
        let mut publishing = state.publishing.take();
        for (index, entry) in state.pending.drain(..).enumerate() {
            Self::acknowledge_removed(entry, index, &mut publishing);
        }
        state.reserved.clear();
    }
    fn acknowledge_removed(
        entry: Observation,
        index: usize,
        publishing: &mut Option<(usize, (u64, u64), Job<()>)>,
    ) {
        let job = if publishing
            .as_ref()
            .is_some_and(|(current, _, _)| *current == index)
        {
            // Native ownership can close before the already accepted logical
            // transaction settles. Its original ACK waiter takes custody of the
            // same job; no retired native resource is published or reactivated.
            publishing.take().expect("original removed publisher").2
        } else {
            Job::ready(Err(maka_plugin_core::Error::Retired.into()))
        };
        let _ = entry.acknowledgement.send(job);
    }
    pub fn contains(&self, key: &str) -> bool {
        self.state
            .borrow()
            .pending
            .iter()
            .any(|entry| entry.key == key)
    }
    pub fn pending(&self) -> bool {
        !self.state.borrow().pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn complete_native_publications_keep_original_acknowledgements_and_release_budget() {
        let budget = NativeBudget::default();
        let observations = Observations::new(budget.clone(), || {});
        let text = format!(
            "PUBLICATION START{}PUBLICATION END",
            "保留😀z".repeat(100_000)
        );
        let value = serde_json::json!({"text":text});
        let bytes = serde_json::to_vec(&value).unwrap().len();
        assert!(bytes > 1024 * 1024);
        let capacity = observations.prepare([("observed", bytes)]);
        let prepared = capacity.is_ok();
        if let Ok(capacity) = capacity {
            observations.adopt(capacity);
        }
        let captured = observations.capture("observed", value.clone(), false);
        let input = observations.capture_input("physical", value.clone());
        let mut committed = Vec::new();
        let mut errors = Vec::new();
        for _ in 0..2 {
            errors.extend(observations.advance(
                (1, 1),
                &mut committed,
                |_, _, _| Ok(Job::ready(Ok(()))),
                |key, value, committed| committed.push((key.to_owned(), value.clone())),
            ));
        }
        observations.close();
        assert_eq!(budget.usage(), (0, 0, 0));
        assert!(
            prepared,
            "complete observation must reserve its actual native budget"
        );
        let acknowledgement = captured
            .expect("complete observation must enqueue")
            .expect("original observation must retain an acknowledgement");
        acknowledgement.await.unwrap();
        input
            .expect("complete original physical input must enqueue")
            .await
            .unwrap();
        assert!(errors.is_empty());
        assert_eq!(committed.len(), 2);
        assert_eq!(committed[0].0, "observed");
        assert_eq!(committed[1].0, "physical");
        assert!(
            committed.iter().all(|(_, actual)| actual == &value),
            "complete original native publication bytes changed"
        );
        assert!(!observations.pending());
    }
}
