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
//! Foreground-only JavaScript execution. A callback returns staged JSON intent;
//! the adapter validates and commits it after this method succeeds. No ambient
//! system capabilities or async Host bridge are installed in this execution domain.

use std::{
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Condvar, Mutex, OnceLock, atomic::AtomicBool},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use deno_core::JsRuntime;
use serde_json::Value;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::{Health, Limits, Result, engine, failed};

/// An opaque module reference, valid only in its originating foreground realm.
#[derive(Clone)]
pub struct ForegroundModule {
    domain: Rc<()>,
    id: u64,
}

/// Inert handle to a captured receiver and its required callback functions.
/// Globals stay in the originating realm and are released on module close.
#[derive(Clone)]
pub struct ForegroundCallbacks {
    domain: Rc<()>,
    module: u64,
    id: u64,
}

/// Create and use on the UI owner thread. The watchdog only interrupts V8; it
/// never executes callbacks or owns native UI objects. The embedding supplies a
/// background Tokio timer handle for Deno's native V8 scheduling and keeps that
/// scheduler alive until this realm is destroyed. No ambient Tokio context is needed.
/// Shared modules share a failure domain. A script exception, promise result or
/// budget violation closes the domain before any returned intent can commit.
pub struct Foreground {
    // Module globals must be destroyed before their isolate.
    callbacks: BTreeMap<u64, (u64, engine::Callbacks)>,
    modules: BTreeMap<u64, engine::Module>,
    domain: Rc<()>,
    next_module: u64,
    next_callbacks: u64,
    budget: Budget,
    scheduler: tokio::runtime::Handle,
    runtime: JsRuntime,
}

impl Foreground {
    pub fn new(limits: Limits, scheduler: tokio::runtime::Handle) -> Result<Self> {
        if limits.heap_bytes < 16 * 1024 * 1024 || limits.synchronous_slice.is_zero() {
            return Err(failed("invalid foreground VM limits"));
        }
        crate::initialize_platform();
        let health = Arc::new(Health {
            isolate: OnceLock::new(),
            failure: Mutex::default(),
            stopped: CancellationToken::new(),
            closed: CancellationToken::new(),
            initializing: AtomicBool::new(false),
            polling: watch::channel(None).0,
        });
        let runtime = {
            let _entered = scheduler.enter();
            engine::foreground_runtime(&health, &limits)?
        };
        let budget = Budget::new(health, limits.synchronous_slice)?;
        let mut foreground = Self {
            callbacks: BTreeMap::new(),
            modules: BTreeMap::new(),
            domain: Rc::new(()),
            next_module: 0,
            next_callbacks: 0,
            budget,
            scheduler,
            runtime,
        };
        // OwnedIsolate enters on creation. Foreground realms coexist and may
        // be destroyed in any order, so operations enter their own isolate.
        unsafe {
            foreground.runtime.v8_isolate().exit();
        }
        Ok(foreground)
    }

    pub fn load(&mut self, name: &str, source: &str) -> Result<ForegroundModule> {
        self.ready()?;
        if name.is_empty() || name.len() > 1024 || source.len() > 8 * 1024 * 1024 {
            return Err(failed("foreground module exceeds name/source limits"));
        }
        let id = self
            .next_module
            .checked_add(1)
            .ok_or_else(|| failed("module identity exhausted"))?;
        self.budget.enter();
        let _entered = self.scheduler.enter();
        let outcome = entered(&mut self.runtime, |runtime| {
            let outcome = engine::load(runtime, name, source).and_then(|module| {
                engine::poll_ready_jobs(runtime)?;
                Ok(module)
            });
            self.budget.finish(outcome)
        })?;
        self.next_module = id;
        self.modules.insert(id, outcome);
        Ok(ForegroundModule {
            domain: self.domain.clone(),
            id,
        })
    }

    /// Return an inert JSON batch. Native resources, service calls and event
    /// decisions must only be applied by the adapter after validating this batch.
    /// Ready JavaScript jobs are drained under the same budget, without awaiting
    /// I/O. Their return values never publish intent. Promise callback results are rejected.
    pub fn call(
        &mut self,
        module: &ForegroundModule,
        path: &[String],
        args: &[Value],
    ) -> Result<Value> {
        self.ready()?;
        if !Rc::ptr_eq(&module.domain, &self.domain) || !self.modules.contains_key(&module.id) {
            return Err(failed("foreground module is retired or from another realm"));
        }
        if path.is_empty()
            || path.len() > 32
            || path.iter().any(|part| part.is_empty() || part.len() > 128)
        {
            return Err(failed("invalid foreground export path"));
        }
        self.budget.enter();
        let _entered = self.scheduler.enter();
        let outcome = entered(&mut self.runtime, |runtime| {
            engine::call_synchronous(runtime, &self.modules[&module.id], path, args).and_then(
                |value| {
                    engine::poll_ready_jobs(runtime)?;
                    Ok(value)
                },
            )
        });
        self.budget.finish(outcome)
    }

    pub fn capture(
        &mut self,
        module: &ForegroundModule,
        namespace: &[String],
        methods: &[&str],
    ) -> Result<ForegroundCallbacks> {
        self.ready()?;
        if !Rc::ptr_eq(&module.domain, &self.domain) || !self.modules.contains_key(&module.id) {
            return Err(failed("foreground module is retired or from another realm"));
        }
        if namespace.len() > 32
            || namespace
                .iter()
                .any(|part| part.is_empty() || part.len() > 128)
            || methods.is_empty()
            || methods.len() > 64
            || methods
                .iter()
                .any(|part| part.is_empty() || part.len() > 128)
            || methods
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != methods.len()
            || self.callbacks.len() >= 1024
        {
            return Err(failed(
                "invalid or excessive foreground callback registration",
            ));
        }
        let id = self
            .next_callbacks
            .checked_add(1)
            .ok_or_else(|| failed("callback identity exhausted"))?;
        self.budget.enter();
        let _entered = self.scheduler.enter();
        let captured = entered(&mut self.runtime, |runtime| {
            let outcome =
                engine::capture_callbacks(runtime, &self.modules[&module.id], namespace, methods)
                    .and_then(|callbacks| {
                        engine::poll_ready_jobs(runtime)?;
                        Ok(callbacks)
                    });
            // Failed capture must drop its globals while this isolate is entered.
            self.budget.finish(outcome)
        })?;
        self.next_callbacks = id;
        self.callbacks.insert(id, (module.id, captured));
        Ok(ForegroundCallbacks {
            domain: self.domain.clone(),
            module: module.id,
            id,
        })
    }

    /// Encodes shared immutable snapshots directly, without rebuilding JSON
    /// values merely to transfer invocation ownership to an execution worker.
    pub fn call_bound(
        &mut self,
        callbacks: &ForegroundCallbacks,
        method: &str,
        args: &[&Value],
    ) -> Result<Value> {
        self.ready()?;
        if !Rc::ptr_eq(&callbacks.domain, &self.domain)
            || !self.modules.contains_key(&callbacks.module)
            || !self
                .callbacks
                .get(&callbacks.id)
                .is_some_and(|(module, _)| *module == callbacks.module)
        {
            return Err(failed(
                "foreground callbacks are retired or from another realm",
            ));
        }
        self.budget.enter();
        let _entered = self.scheduler.enter();
        let outcome = entered(&mut self.runtime, |runtime| {
            engine::call_bound_synchronous(runtime, &self.callbacks[&callbacks.id].1, method, args)
                .and_then(|value| {
                    engine::poll_ready_jobs(runtime)?;
                    Ok(value)
                })
        });
        self.budget.finish(outcome)
    }

    /// Revocation is synchronous and does not dispatch dispose code. Cleanup of
    /// resources created by a committed batch belongs to the embedding owner.
    pub fn close(&mut self, module: &ForegroundModule) -> Result<()> {
        if !Rc::ptr_eq(&module.domain, &self.domain) || !self.modules.contains_key(&module.id) {
            return Err(failed("foreground module is retired or from another realm"));
        }
        entered(&mut self.runtime, |_| {
            self.callbacks.retain(|_, (owner, _)| *owner != module.id);
            self.modules.remove(&module.id);
        });
        Ok(())
    }

    pub fn failure(&self) -> Option<super::Error> {
        self.budget.health.failure.lock().unwrap().clone()
    }

    fn ready(&self) -> Result<()> {
        if self.budget.health.stopped.is_cancelled() {
            Err(self.budget.health.error())
        } else {
            Ok(())
        }
    }
}

impl Drop for Foreground {
    fn drop(&mut self) {
        // Globals and JsRuntime drop with their isolate current; OwnedIsolate's
        // destructor balances this entry before disposal.
        unsafe {
            self.runtime.v8_isolate().enter();
        }
    }
}

fn entered<T>(runtime: &mut JsRuntime, action: impl FnOnce(&mut JsRuntime) -> T) -> T {
    struct Exit(std::ptr::NonNull<deno_core::v8::Isolate>);
    impl Drop for Exit {
        fn drop(&mut self) {
            // Private actions never replace/destroy the borrowed runtime. V8
            // scopes unwind before this guard restores the previous isolate.
            unsafe {
                self.0.as_ref().exit();
            }
        }
    }
    let isolate = std::ptr::NonNull::from(&mut **runtime.v8_isolate());
    unsafe {
        isolate.as_ref().enter();
    }
    let _exit = Exit(isolate);
    action(runtime)
}

struct Clock {
    deadline: Option<Instant>,
    closed: bool,
}
struct Budget {
    health: Arc<Health>,
    clock: Arc<(Mutex<Clock>, Condvar)>,
    slice: Duration,
    watcher: Option<JoinHandle<()>>,
}

impl Budget {
    fn new(health: Arc<Health>, slice: Duration) -> Result<Self> {
        let clock = Arc::new((
            Mutex::new(Clock {
                deadline: None,
                closed: false,
            }),
            Condvar::new(),
        ));
        let observing = clock.clone();
        let observed_health = health.clone();
        let watcher = std::thread::Builder::new()
            .name("maka-foreground-js-budget".into())
            .spawn(move || {
                let (lock, changed) = &*observing;
                let mut clock = lock.lock().unwrap();
                loop {
                    if clock.closed {
                        break;
                    }
                    match clock.deadline {
                        None => clock = changed.wait(clock).unwrap(),
                        Some(deadline) => {
                            if let Some(remaining) = deadline.checked_duration_since(Instant::now())
                            {
                                clock = changed.wait_timeout(clock, remaining).unwrap().0;
                            } else {
                                observed_health
                                    .fail(failed("foreground synchronous budget exceeded"));
                                clock.deadline = None;
                            }
                        }
                    }
                }
            })
            .map_err(failed)?;
        Ok(Self {
            health,
            clock,
            slice,
            watcher: Some(watcher),
        })
    }

    fn enter(&self) {
        self.clock.0.lock().unwrap().deadline = Some(Instant::now() + self.slice);
        self.clock.1.notify_one();
    }

    fn finish<T>(&self, outcome: Result<T>) -> Result<T> {
        let deadline = self.clock.0.lock().unwrap().deadline.take();
        self.clock.1.notify_one();
        // Even a delayed watchdog cannot let an over-budget callback commit.
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            self.health
                .fail(failed("foreground synchronous budget exceeded"));
        }
        if let Err(error) = &outcome {
            self.health.fail(error.clone());
        }
        if self.health.stopped.is_cancelled() {
            Err(self.health.error())
        } else {
            outcome
        }
    }
}

impl Drop for Budget {
    fn drop(&mut self) {
        self.clock.0.lock().unwrap().closed = true;
        self.clock.1.notify_one();
        if let Some(watcher) = self.watcher.take() {
            let _ = watcher.join();
        }
    }
}
