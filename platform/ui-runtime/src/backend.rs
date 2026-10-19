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

//! Component execution backends return inert declarations and batches. The UI
//! runtime alone owns input admission, transaction commit and native cleanup.
use crate::{
    ComponentCall, ComponentOutput, EventOrigin, PrepareScene, PreparedScene, Result, abi,
    actions::CapturePlan,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll},
};

/// An admitted execution result. Polling never executes guest code on the owner
/// thread. Dropping observation does not revoke work already owned by a realm.
pub struct Job<T>(Pin<Box<dyn Future<Output = Result<T>>>>);
impl<T: 'static> Job<T> {
    pub fn new(future: impl Future<Output = Result<T>> + 'static) -> Self {
        Self(Box::pin(future))
    }
    pub fn ready(result: Result<T>) -> Self {
        Self::new(std::future::ready(result))
    }
}
impl<T> Future for Job<T> {
    type Output = Result<T>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.as_mut().poll(cx)
    }
}

pub trait Factory: 'static {
    fn create(&self, scheduler: tokio::runtime::Handle) -> Result<Rc<dyn Realm>>;
}

/// Shared modules retain the same realm object and therefore the same failure fate.
pub trait Realm: 'static {
    fn load(&self, name: &str, source: &str) -> Result<Rc<dyn Module>>;
    fn failure(&self) -> Option<String>;
}

/// An original module remains owned until close confirms its handles are released.
pub trait Module: 'static {
    fn activate(
        &self,
        configuration: &Value,
        values: &abi::ValueBindings,
    ) -> Result<Job<BTreeMap<String, abi::UiExport>>>;
    fn component(&self, namespace: &[String], captures: bool) -> Result<Rc<dyn Component>>;
    fn close(&self) -> Result<Job<()>>;
}

/// Immutable invocation snapshots. Queued work shares the original candidate or
/// committed JSON; only the UI runtime decides which candidate becomes current.
#[derive(Clone)]
pub enum Call {
    Mount {
        props: Arc<Value>,
    },
    Update {
        state: Arc<Value>,
        props: Arc<Value>,
    },
    Event {
        state: Arc<Value>,
        props: Arc<Value>,
        input: Arc<abi::ComponentInput>,
    },
}
impl Call {
    pub fn props(&self) -> &Arc<Value> {
        match self {
            Self::Mount { props } | Self::Update { props, .. } | Self::Event { props, .. } => props,
        }
    }
    pub fn as_native(&self) -> ComponentCall<'_> {
        match self {
            Self::Mount { props } => ComponentCall::Mount { props },
            Self::Update { state, props } => ComponentCall::Update { state, props },
            Self::Event {
                state,
                props,
                input,
            } => {
                let (key, event, origin) = match input.as_ref() {
                    abi::ComponentInput::Raw { key, event } => (key, event, EventOrigin::Raw),
                    abi::ComponentInput::Action { key, event } => (key, event, EventOrigin::Action),
                    abi::ComponentInput::Component {
                        key,
                        child,
                        name,
                        event,
                    } => (key, event, EventOrigin::Component { child, name }),
                };
                ComponentCall::Event {
                    state,
                    props,
                    key,
                    input: event,
                    origin,
                }
            }
        }
    }
}

/// A capture retains the exact same immutable state as its invoking transaction.
pub struct Capture {
    pub values: abi::ValueBindings,
    pub state: Arc<Value>,
    pub props: Arc<Value>,
    pub method: String,
    pub input: Value,
}

/// Captured callbacks retain their original receiver and realm. Returned batches
/// contain no native resource authority and cannot commit effects themselves.
pub trait Component: 'static {
    fn call(
        &self,
        call: Call,
        values: abi::ValueBindings,
        prepare: std::sync::Arc<PrepareScene>,
    ) -> Result<Job<ComponentOutput<PreparedScene>>>;
    fn capture(&self, call: Capture) -> Result<Job<CapturePlan>>;
}
