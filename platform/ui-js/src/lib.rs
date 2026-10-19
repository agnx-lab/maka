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

//! Background JavaScript execution. The original UI owner alone admits input,
//! commits candidate batches and settles native resources and mutation receipts.
use maka_js_runtime::plugin::{Foreground, ForegroundCallbacks, ForegroundModule, Limits};
use maka_ui_runtime::actions::CapturePlan;
use maka_ui_runtime::{
    ComponentOutput, Error, ExecutionOptions, PrepareScene, PreparedScene, Result, abi, backend,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};
use tokio::sync::oneshot;

pub fn execution(limits: Limits) -> ExecutionOptions {
    ExecutionOptions {
        script: Some(Rc::new(Factory {
            limits,
            worker: RefCell::new(None),
        })),
    }
}
struct Factory {
    limits: Limits,
    worker: RefCell<Option<Worker>>,
}
impl backend::Factory for Factory {
    fn create(&self, scheduler: tokio::runtime::Handle) -> Result<Rc<dyn backend::Realm>> {
        if self.limits.heap_bytes < 16 * 1024 * 1024 || self.limits.synchronous_slice.is_zero() {
            return Err(script("invalid UI guest limits"));
        }
        let mut worker = self.worker.borrow_mut();
        if worker.is_none() {
            *worker = Some(Worker::new()?);
        }
        let worker = worker.as_ref().expect("created UI worker");
        let id = worker.identity();
        let health = Arc::new(Mutex::new(None));
        let retained = Arc::downgrade(&health);
        let limits = self.limits.clone();
        worker.submit(move |state| {
            match Foreground::new(limits, scheduler) {
                Ok(vm) => {
                    state.realms.insert(
                        id,
                        Vm {
                            vm,
                            modules: BTreeMap::new(),
                            health: retained,
                        },
                    );
                }
                Err(error) => {
                    if let Some(health) = retained.upgrade() {
                        *health.lock().unwrap() = Some(error.to_string());
                    }
                }
            }
            Ok(())
        })?;
        Ok(Rc::new(Realm {
            id,
            worker: worker.clone(),
            health,
        }))
    }
}

/// One sleeping worker multiplexes realms without sharing their V8 failure
/// domains. Submitting work never blocks the UI owner.
#[derive(Clone)]
struct Worker {
    queue: mpsc::Sender<Command>,
    ids: Arc<AtomicU64>,
}
struct Command {
    run: Box<dyn FnOnce(&mut State) + Send>,
}
impl Worker {
    fn new() -> Result<Self> {
        let (queue, receive) = mpsc::channel::<Command>();
        std::thread::Builder::new()
            .name("maka-ui-js".into())
            .spawn(move || {
                let mut state = State {
                    realms: BTreeMap::new(),
                };
                while let Ok(command) = receive.recv() {
                    (command.run)(&mut state);
                }
                // All VM and captured callback destruction happens on this thread.
            })
            .map_err(script)?;
        Ok(Self {
            queue,
            ids: Arc::new(AtomicU64::new(1)),
        })
    }
    fn identity(&self) -> u64 {
        self.ids.fetch_add(1, Ordering::Relaxed)
    }
    fn submit<T: Send + 'static>(
        &self,
        action: impl FnOnce(&mut State) -> Result<T> + Send + 'static,
    ) -> Result<backend::Job<T>> {
        let (send, receive) = oneshot::channel();
        self.queue
            .send(Command {
                run: Box::new(move |state| {
                    let _ = send.send(action(state));
                }),
            })
            .map_err(|_| script("UI worker stopped without cleanup acknowledgement"))?;
        Ok(backend::Job::new(async move {
            receive
                .await
                .map_err(|_| script("UI worker stopped without cleanup acknowledgement"))?
        }))
    }
}
struct State {
    realms: BTreeMap<u64, Vm>,
}
struct Vm {
    vm: Foreground,
    modules: BTreeMap<u64, OwnedModule>,
    health: std::sync::Weak<Mutex<Option<String>>>,
}
struct OwnedModule {
    module: ForegroundModule,
    callbacks: BTreeMap<Vec<String>, ForegroundCallbacks>,
}
impl State {
    fn with_vm<T>(&mut self, id: u64, action: impl FnOnce(&mut Vm) -> Result<T>) -> Result<T> {
        let vm = self
            .realms
            .get_mut(&id)
            .ok_or_else(|| script("UI realm is unavailable"))?;
        let result = action(vm);
        if let Some(error) = vm.vm.failure()
            && let Some(health) = vm.health.upgrade()
        {
            *health.lock().unwrap() = Some(error.to_string());
        }
        result
    }
}
struct Realm {
    id: u64,
    worker: Worker,
    health: Arc<Mutex<Option<String>>>,
}
impl backend::Realm for Realm {
    fn load(&self, name: &str, source: &str) -> Result<Rc<dyn backend::Module>> {
        let id = self.worker.identity();
        Ok(Rc::new(Module {
            realm: self.id,
            id,
            worker: self.worker.clone(),
            source: RefCell::new(Some((name.to_owned(), source.to_owned()))),
        }))
    }
    fn failure(&self) -> Option<String> {
        self.health.lock().unwrap().clone()
    }
}
impl Drop for Realm {
    fn drop(&mut self) {
        let id = self.id;
        // Module close receipts precede releasing the final realm reference.
        // Failure to admit disposal remains an execution failure, never a close ACK.
        let _ = self.worker.submit(move |state| {
            state.realms.remove(&id);
            Ok(())
        });
    }
}
struct Module {
    realm: u64,
    id: u64,
    worker: Worker,
    source: RefCell<Option<(String, String)>>,
}
impl backend::Module for Module {
    fn activate(
        &self,
        configuration: &Value,
        values: &abi::ValueBindings,
    ) -> Result<backend::Job<BTreeMap<String, abi::UiExport>>> {
        let args = vec![
            configuration.clone(),
            serde_json::to_value(values).map_err(invalid)?,
        ];
        delivery(&args)?;
        let (name, source) = self
            .source
            .borrow_mut()
            .take()
            .ok_or_else(|| invalid("UI module activation already admitted"))?;
        let (realm, id) = (self.realm, self.id);
        self.worker.submit(move |state| {
            state.with_vm(realm, |vm| {
                // Loading and activation share one original completion receipt.
                // A syntax/load failure cannot disappear in a discarded Job.
                let module = vm.vm.load(&name, &source).map_err(script)?;
                vm.modules.insert(
                    id,
                    OwnedModule {
                        module,
                        callbacks: BTreeMap::new(),
                    },
                );
                let owned = vm
                    .modules
                    .get_mut(&id)
                    .ok_or_else(|| script("UI module is unavailable"))?;
                let value = vm
                    .vm
                    .call(&owned.module, &["activate".into()], &args)
                    .map_err(script)?;
                let exports: BTreeMap<String, abi::UiExport> =
                    serde_json::from_value(value).map_err(invalid)?;
                for export in exports.values() {
                    if let abi::UiExport::Component {
                        contract,
                        component,
                    } = export
                    {
                        let maka_plugin_core::contract::Interface::Component { captures, .. } =
                            &contract.interface
                        else {
                            return Err(invalid("UI export must declare a component"));
                        };
                        let methods = if captures.is_empty() {
                            &["mount", "event", "update"][..]
                        } else {
                            &["mount", "event", "update", "capture"][..]
                        };
                        let callbacks = vm
                            .vm
                            .capture(&owned.module, component, methods)
                            .map_err(script)?;
                        owned.callbacks.insert(component.clone(), callbacks);
                    }
                }
                Ok(exports)
            })
        })
    }
    fn component(&self, namespace: &[String], _: bool) -> Result<Rc<dyn backend::Component>> {
        Ok(Rc::new(Component {
            realm: self.realm,
            module: self.id,
            namespace: namespace.to_vec(),
            worker: self.worker.clone(),
        }))
    }
    fn close(&self) -> Result<backend::Job<()>> {
        let (realm, id) = (self.realm, self.id);
        self.worker.submit(move |state| {
            state.with_vm(realm, |vm| {
                // Rejected admission or a failed load has no VM module handle.
                // The original realm Job still confirms this exact absence.
                let Some(owned) = vm.modules.remove(&id) else {
                    return Ok(());
                };
                vm.vm.close(&owned.module).map_err(script)
            })
        })
    }
}
struct Component {
    realm: u64,
    module: u64,
    namespace: Vec<String>,
    worker: Worker,
}
impl Component {
    fn execute<T: Send + 'static>(
        &self,
        method: &'static str,
        arguments: impl FnOnce() -> Result<Vec<Arc<Value>>> + Send + 'static,
        decode: impl FnOnce(Value) -> Result<T> + Send + 'static,
    ) -> Result<backend::Job<T>> {
        let (realm, id, namespace) = (self.realm, self.module, self.namespace.clone());
        self.worker.submit(move |state| {
            state.with_vm(realm, |vm| {
                let args = arguments()?;
                let args: Vec<_> = args.iter().map(AsRef::as_ref).collect();
                delivery(args.iter().copied())?;
                let callbacks = vm
                    .modules
                    .get(&id)
                    .and_then(|module| module.callbacks.get(&namespace))
                    .ok_or_else(|| script("UI callbacks are retired or unavailable"))?;
                let value = vm.vm.call_bound(callbacks, method, &args).map_err(script)?;
                decode(value)
            })
        })
    }
}
impl backend::Component for Component {
    fn call(
        &self,
        call: backend::Call,
        values: abi::ValueBindings,
        prepare: Arc<PrepareScene>,
    ) -> Result<backend::Job<ComponentOutput<PreparedScene>>> {
        let method = match &call {
            backend::Call::Mount { .. } => "mount",
            backend::Call::Update { .. } => "update",
            backend::Call::Event { .. } => "event",
        };
        self.execute(
            method,
            move || {
                let mut args = match call {
                    backend::Call::Mount { props } => vec![props],
                    backend::Call::Update { state, props } => vec![state, props],
                    backend::Call::Event {
                        state,
                        props,
                        input,
                    } => {
                        vec![
                            state,
                            Arc::new(serde_json::to_value(input.as_ref()).map_err(invalid)?),
                            props,
                        ]
                    }
                };
                args.push(Arc::new(serde_json::to_value(values).map_err(invalid)?));
                Ok(args)
            },
            move |value| {
                // Validate number precision before compiler code observes the result.
                maka_plugin_core::json::exact_numbers(&value).map_err(Error::Invalid)?;
                let output = component_output(value)?;
                let view = prepare(&output.frame.view)?;
                Ok(output.map_view(|_| view))
            },
        )
    }
    fn capture(&self, call: backend::Capture) -> Result<backend::Job<CapturePlan>> {
        self.execute(
            "capture",
            move || {
                Ok(vec![
                    call.state,
                    call.props,
                    Arc::new(json!(call.method)),
                    Arc::new(call.input),
                    Arc::new(serde_json::to_value(call.values).map_err(invalid)?),
                ])
            },
            |value| serde_json::from_value(value).map_err(invalid),
        )
    }
}
fn delivery<'a>(args: impl IntoIterator<Item = &'a Value>) -> Result<()> {
    for value in args {
        maka_plugin_core::json::exact_numbers(value).map_err(Error::Invalid)?;
    }
    Ok(())
}
fn script(error: impl ToString) -> Error {
    Error::Script(error.to_string())
}
fn invalid(error: impl ToString) -> Error {
    Error::Invalid(error.to_string())
}

// State and scene are already checked JSON values. Preserve them directly while
// deserializing the typed envelope, instead of rebuilding both value trees.
fn component_output(mut value: Value) -> Result<ComponentOutput> {
    let payload = value
        .get_mut("frame")
        .and_then(Value::as_object_mut)
        .map(|frame| {
            (
                frame.get_mut("state").map(Value::take),
                frame.get_mut("view").map(Value::take),
            )
        });
    let mut output: ComponentOutput = serde_json::from_value(value).map_err(script)?;
    if let Some((state, view)) = payload {
        if let Some(state) = state {
            output.frame.state = state;
        }
        if let Some(view) = view {
            output.frame.view = view;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_decoding_preserves_payloads_and_envelope_validation() {
        let valid = json!({
            "frame": {"revision": 1, "state": {"nested": [null, true, "text", 42]},
                "surface": null, "view": {"children": [{"kind": "text", "text": "中😀"}]}},
            "events": [], "actions": []
        });
        let mut cases = vec![valid.clone(), Value::Null, json!({})];
        for key in ["state", "view", "revision", "surface"] {
            let mut missing = valid.clone();
            missing["frame"].as_object_mut().unwrap().remove(key);
            cases.push(missing);
        }
        for (key, value) in [
            ("revision", json!(-1)),
            ("surface", json!({"kind": "invalid"})),
            ("unexpected", json!(true)),
        ] {
            let mut invalid = valid.clone();
            invalid["frame"][key] = value;
            cases.push(invalid);
        }
        let mut unknown = valid;
        unknown["unexpected"] = json!(true);
        cases.push(unknown);
        for value in cases {
            let expected = serde_json::from_value::<ComponentOutput>(value.clone());
            let actual = component_output(value);
            assert_eq!(actual.is_ok(), expected.is_ok());
            if let (Ok(actual), Ok(expected)) = (actual, expected) {
                assert_eq!(
                    serde_json::to_value(actual).unwrap(),
                    serde_json::to_value(expected).unwrap()
                );
            }
        }
    }
}
