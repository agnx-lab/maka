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

use super::Application;
use super::{support, timer};
use crate::support::scene::Node;
use maka_plugin_core::{
    composition::PresentationTarget,
    contract::Contract,
    package::{EntryTarget, Implementation},
};
use maka_ui_runtime::{
    ComponentCall, ComponentOutput, Frame, NativeCapability, NativeComponent, NativeExport,
    NativeRegistry, NativeView, Surface,
};
use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};

struct Counter {
    calls: Rc<Cell<usize>>,
}
impl NativeComponent<String> for Counter {
    fn start(
        &self,
        call: ComponentCall<'_>,
        previous: Option<&NativeView<String>>,
        values: &maka_ui_runtime::abi::ValueBindings,
        _: &tokio::runtime::Handle,
    ) -> maka_ui_runtime::backend::Job<ComponentOutput<NativeView<String>>> {
        match &call {
            ComponentCall::Mount { .. } => assert!(previous.is_none()),
            ComponentCall::Update { state, .. } | ComponentCall::Event { state, .. } => {
                assert_eq!(
                    previous.unwrap().view.as_str(),
                    format!("{}={}", state["label"].as_str().unwrap(), state["count"])
                );
            }
        }
        maka_ui_runtime::backend::Job::ready(self.call(call, values))
    }
    fn call(
        &self,
        call: ComponentCall<'_>,
        _values: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<String>>> {
        self.calls.set(self.calls.get() + 1);
        let state = match call {
            ComponentCall::Mount { props } => {
                json!({"count":props["initial"],"label":props["label"],"revision":0})
            }
            ComponentCall::Update { state, props } => {
                if props["label"] == "Right 1" {
                    return Err(maka_ui_runtime::Error::Invalid(
                        "reject later sibling".into(),
                    ));
                }
                json!({"count":state["count"],"label":props["label"],"revision":state["revision"].as_u64().unwrap()+1})
            }
            ComponentCall::Event { state, input, .. } => {
                json!({"count":state["count"].as_i64().unwrap()+input["delta"].as_i64().unwrap(),"label":state["label"],"revision":state["revision"].as_u64().unwrap()+1})
            }
        };
        Ok(Frame {
            revision: state["revision"].as_u64().unwrap(),
            surface: None,
            view: NativeView::new(
                Rc::new(format!(
                    "{}={}",
                    state["label"].as_str().unwrap(),
                    state["count"]
                )),
                BTreeMap::from([(
                    "inc".into(),
                    json!({"type":"object","properties":{"delta":{"type":"integer","minimum":-10,"maximum":10},"large":{"type":"string"}},"required":["delta"],"additionalProperties":false}),
                )]), vec![])?,
            state,
        }.output(Vec::new()))
    }
}
fn registered(contract: Contract, calls: Rc<Cell<usize>>) -> NativeRegistry<String> {
    let mut registry = crate::support::scene::registry(PresentationTarget::Tui);
    registry
        .register("test.counter", move |_, _values| {
            Ok(BTreeMap::from([(
                "Counter".into(),
                NativeExport {
                    contract: contract.clone(),
                    value: NativeCapability::Component(Rc::new(Counter {
                        calls: calls.clone(),
                    })),
                },
            )]))
        })
        .unwrap();
    registry
}
fn fixture() -> (
    maka_plugin_core::composition::Input,
    Vec<maka_packages::Package>,
    Contract,
) {
    let (mut input, mut packages) =
        support::composed_fixture(support::CHILD_UPDATE, support::PARENT_EVENT);
    let old = packages.pop().unwrap();
    let mut declaration = old
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let entry = declaration.entries.get_mut(&EntryTarget::Tui).unwrap();
    let contract = entry.exports["Counter"].contract.clone();
    entry.scene = None;
    entry.implementation = Implementation::Native {
        implementation: "test.counter".into(),
    };
    let counter = support::package("native.counter", declaration, None);
    input.instances.get_mut("counter").unwrap().package = counter.application_ref().into();
    packages.push(counter);
    (input, packages, contract)
}
fn close_all<V: ?Sized + 'static>(
    app: &mut Application<V>,
    root: &maka_plugin_core::runtime::Mount,
) {
    app.retire(root).unwrap();
    for mount in app.retiring_mounts() {
        app.close(&mount, Ok(())).unwrap();
    }
    app.shutdown().unwrap();
}

#[test]
fn opaque_native_children_share_transaction_identity_and_input_admission_with_js() {
    let timer = timer();
    let (input, packages, contract) = fixture();
    let calls = Rc::new(Cell::new(0));
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(Default::default()),
        registered(contract, calls.clone()),
        Default::default(),
    )
    .unwrap();
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    let right = app.child_mount(&root, "right").unwrap().clone();
    let count = calls.get();
    assert!(
        app.dispatch(&left, 0, "missing", json!({"delta":1}))
            .is_err()
    );
    assert!(app.dispatch(&left, 0, "inc", json!({"delta":11})).is_err());
    assert_eq!(
        calls.get(),
        count,
        "invalid raw inputs cannot enter the native callback"
    );
    app.dispatch(&left, 0, "inc", json!({"delta":2})).unwrap();
    let view = &app.frame(&left).unwrap().view;
    assert_eq!(view.view.as_str(), "Left 0=12");
    assert_eq!(app.frame(&right).unwrap().state["count"], 20);
    assert!(app.dispatch(&left, 0, "inc", json!({"delta":1})).is_err());
    // The first sibling prepares successfully, but a later native rejection
    // must leave the root and both previously published scenes untouched.
    assert!(app.dispatch(&root, 0, "parent", Value::Null).is_err());
    assert_eq!(app.frame(&root).unwrap().revision, 0);
    assert_eq!(app.frame(&left).unwrap().revision, 1);
    assert_eq!(app.frame(&left).unwrap().state["label"], "Left 0");
    assert_eq!(app.frame(&right).unwrap().revision, 0);
    assert!(!app.is_retiring(&left));
    app.dispatch(&root, 0, "toggle", Value::Null).unwrap();
    app.close(&right, Ok(())).unwrap();
    // With the rejecting sibling gone, props update retains the first native
    // child's identity and independently accumulated local state.
    app.dispatch(&root, 1, "parent", Value::Null).unwrap();
    assert_eq!(app.child_mount(&root, "left").unwrap(), &left);
    assert_eq!(app.frame(&left).unwrap().state["count"], 12);
    assert_eq!(app.frame(&left).unwrap().state["label"], "Left 1");
    app.dispatch(&root, 2, "toggle", Value::Null).unwrap();
    let replacement = app.child_mount(&root, "right").unwrap();
    assert_ne!(replacement, &right);
    assert_eq!(app.frame(replacement).unwrap().state["count"], 20);
    assert!(app.dispatch(&right, 0, "inc", json!({"delta":1})).is_err());
    close_all(&mut app, &root);
}

struct DelayedCounter {
    counter: Counter,
    release: Rc<std::cell::RefCell<Option<tokio::sync::oneshot::Receiver<()>>>>,
}
impl NativeComponent<String> for DelayedCounter {
    fn call(
        &self,
        call: ComponentCall<'_>,
        values: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<String>>> {
        self.counter.call(call, values)
    }
    fn start(
        &self,
        call: ComponentCall<'_>,
        _: Option<&NativeView<String>>,
        values: &maka_ui_runtime::abi::ValueBindings,
        _: &tokio::runtime::Handle,
    ) -> maka_ui_runtime::backend::Job<ComponentOutput<NativeView<String>>> {
        let release = if matches!(&call, ComponentCall::Event { .. }) {
            self.release.borrow_mut().take()
        } else {
            None
        };
        let output = self.call(call, values);
        maka_ui_runtime::backend::Job::new(async move {
            if let Some(release) = release {
                release.await.unwrap();
            }
            output
        })
    }
}

#[test]
fn waiting_for_native_work_does_not_expire_the_composed_update() {
    let timer = timer();
    let (input, packages, contract) = fixture();
    let (release, waiting) = tokio::sync::oneshot::channel();
    let waiting = Rc::new(std::cell::RefCell::new(Some(waiting)));
    let mut registry = crate::support::scene::registry(PresentationTarget::Tui);
    registry
        .register("test.counter", move |_, _| {
            Ok(BTreeMap::from([(
                "Counter".into(),
                NativeExport {
                    contract: contract.clone(),
                    value: NativeCapability::Component(Rc::new(DelayedCounter {
                        counter: Counter {
                            calls: Rc::new(Cell::new(0)),
                        },
                        release: waiting.clone(),
                    })),
                },
            )]))
        })
        .unwrap();
    let slice = std::time::Duration::from_millis(100);
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: slice,
            ..Default::default()
        }),
        registry,
        Default::default(),
    )
    .unwrap();
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    maka_ui_runtime::Application::dispatch(&mut app, &left, 0, "inc", json!({"delta":2})).unwrap();
    assert!(
        timer
            .block_on(async { tokio::time::timeout(slice * 2, app.wait_idle()).await })
            .is_err()
    );
    assert_eq!(app.frame(&left).unwrap().state["count"], 10);
    release.send(()).unwrap();
    timer.block_on(app.wait_idle()).unwrap();
    assert_eq!(app.frame(&left).unwrap().state["count"], 12);
    close_all(&mut app, &root);
}

#[test]
fn native_availability_is_not_activation_and_actual_contract_or_value_mismatches_fail() {
    let timer = timer();
    for failure in 0..4 {
        let (input, packages, mut contract) = fixture();
        let mut registry = crate::support::scene::registry::<String>(PresentationTarget::Tui);
        if failure != 0 {
            if failure == 1 {
                contract.version += 1;
            }
            registry
                .register("test.counter", move |_, _values| {
                    Ok(BTreeMap::from([(
                        "Counter".into(),
                        NativeExport {
                            contract: contract.clone(),
                            value: if failure == 2 {
                                NativeCapability::Surface(PresentationTarget::Tui)
                            } else {
                                NativeCapability::Component(Rc::new(Counter {
                                    calls: Rc::new(Cell::new(0)),
                                }))
                            },
                        },
                    )]))
                })
                .unwrap();
            if failure == 3 {
                // Extra implementation availability must remain inert.
                registry
                    .register("unused", |_, _values| {
                        panic!("unselected native factory activated")
                    })
                    .unwrap();
            }
        }
        let result = Application::new(
            &input,
            packages,
            timer.handle().clone(),
            maka_ui_js::execution(Default::default()),
            registry,
            Default::default(),
        );
        if failure == 3 {
            let mut app = result.unwrap();
            let root = app.mount_launch(json!({})).unwrap();
            close_all(&mut app, &root);
        } else {
            assert!(result.is_err());
        }
    }
    // The example imports the label explicitly; the surface factory alone
    // cannot stand in for a declared native component implementation.
    let (input, packages) = support::editor_fixture();
    assert!(
        Application::new(
            &input,
            packages,
            timer.handle().clone(),
            maka_ui_js::execution(Default::default()),
            crate::support::scene::registry::<String>(PresentationTarget::Tui),
            Default::default()
        )
        .is_err()
    );
}

struct Root;
impl NativeComponent<String> for Root {
    fn call(
        &self,
        call: ComponentCall<'_>,
        _values: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<String>>> {
        assert!(matches!(call, ComponentCall::Mount { .. }));
        Ok(Frame {
            revision: 0,
            state: json!({}),
            surface: Some(Surface::Terminal {
                binding: "surface".into(),
            }),
            view: crate::support::scene::view(Node::Component(maka_ui_runtime::ComponentSlot {
                key: "child".into(),
                binding: "counter".into(),
                member: 0,
                props: json!({"label":"Native","initial":4}),
                on: BTreeMap::new(),
            }))?,
        }
        .output(Vec::new()))
    }
}
#[test]
fn native_root_uses_explicit_surface_and_can_compose_an_imported_component() {
    let timer = timer();
    let (mut input, mut packages, contract) = fixture();
    let index = packages
        .iter()
        .position(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let old = packages.remove(index);
    let mut declaration = old
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let entry = declaration.entries.get_mut(&EntryTarget::Tui).unwrap();
    entry.scene = None;
    entry.implementation = Implementation::Native {
        implementation: "test.root".into(),
    };
    let root_contract = entry.exports["App"].contract.clone();
    let root = support::package("native.root", declaration, None);
    input.instances.get_mut("app").unwrap().package = root.application_ref().into();
    packages.push(root);
    let mut registry = registered(contract, Rc::new(Cell::new(0)));
    registry
        .register("test.root", move |_, _values| {
            Ok(BTreeMap::from([(
                "App".into(),
                NativeExport {
                    contract: root_contract.clone(),
                    value: NativeCapability::Component(Rc::new(Root)),
                },
            )]))
        })
        .unwrap();
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(Default::default()),
        registry,
        Default::default(),
    )
    .unwrap();
    let root = app.mount_launch(json!({})).unwrap();
    let child = app.child_mount(&root, "child").unwrap().clone();
    assert_eq!(
        app.frame(&root).unwrap().surface,
        Some(Surface::Terminal {
            binding: "surface".into()
        })
    );
    app.dispatch(&child, 0, "inc", json!({"delta":3})).unwrap();
    assert_eq!(app.frame(&child).unwrap().state["count"], 7);
    close_all(&mut app, &root);
}

struct SlotParent;
impl NativeComponent<String> for SlotParent {
    fn call(
        &self,
        call: ComponentCall<'_>,
        _values: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<String>>> {
        let mut next = match call {
            ComponentCall::Mount { .. } => {
                json!({"revision":0,"version":0,"right":true,"native":true,"seen":[]})
            }
            ComponentCall::Update { state, .. } | ComponentCall::Event { state, .. } => {
                state.clone()
            }
        };
        if let ComponentCall::Event {
            key, input, origin, ..
        } = call
        {
            next["revision"] = json!(next["revision"].as_u64().unwrap() + 1);
            match key {
                "notice" => {
                    let maka_ui_runtime::EventOrigin::Component { child, name } = origin else {
                        panic!("kernel child origin")
                    };
                    next["seen"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!([child, name, input]));
                }
                "parent" => next["version"] = json!(next["version"].as_u64().unwrap() + 1),
                "toggle" => next["right"] = json!(!next["right"].as_bool().unwrap()),
                "tree" => next["native"] = json!(!next["native"].as_bool().unwrap()),
                _ => {}
            }
            next["last"] = json!(key);
        }
        let mut slots = vec![maka_ui_runtime::ComponentSlot {
            key: "left".into(),
            binding: "counter".into(),
            member: 0,
            props: json!({"label":format!("Left {}",next["version"]),"initial":10}),
            on: BTreeMap::from([("changed".into(), "notice".into())]),
        }];
        if next["right"] == true {
            slots.push(maka_ui_runtime::ComponentSlot {
                key: "right".into(),
                binding: "counter".into(),
                member: 0,
                props: json!({"label":if next["last"]=="reject" { "Right Reject".into() } else { format!("Right {}",next["version"]) },"initial":20}),
                on: BTreeMap::new(),
            });
        }
        match next["last"].as_str() {
            Some("inexact") => slots[0].props["initial"] = json!(9_007_199_254_740_993u64),
            Some("invalid") => slots[0].binding = "undeclared".into(),
            Some("bad_event") => {
                slots[0].on.insert("undeclared".into(), "notice".into());
            }
            Some("duplicate") => slots.push(slots[0].clone()),
            _ => {}
        }
        let view = if next["native"] == true {
            NativeView::new(
                Rc::new("Native parent".into()),
                [
                    "parent",
                    "toggle",
                    "tree",
                    "invalid",
                    "bad_event",
                    "duplicate",
                    "inexact",
                    "reject",
                ]
                .into_iter()
                .map(|k| (k.into(), json!(true)))
                .collect(),
                slots,
            )?
        } else {
            crate::support::scene::view(Node::Stack {
                key: "root".into(),
                direction: crate::support::scene::Direction::Column,
                gap: 0,
                padding: 0,
                background: 0,
                children: slots
                    .into_iter()
                    .map(Node::Component)
                    .chain([Node::Button {
                        key: "tree".into(),
                        label: "Native".into(),
                    }])
                    .collect(),
            })?
        };
        Ok(Frame {
            revision: next["revision"].as_u64().unwrap(),
            state: next,
            surface: Some(Surface::Terminal {
                binding: "surface".into(),
            }),
            view,
        }
        .output(vec![]))
    }
}
#[test]
fn native_parent_slots_share_js_child_events_rollback_identity_and_retirement() {
    let timer = timer();
    let (mut input, mut packages, _) = super::events::fixture(
        "return frame({count:props.initial,label:props.label,revision:0,calls:0});",
        "return frame({...state,count:state.count+1,revision:state.revision+1},[{name:'changed',value:state.count+1}]);",
        "if(props.label==='Right Reject')throw new Error('later sibling');globalThis.updates=(globalThis.updates??0)+1;return frame({...state,label:props.label,revision:state.revision+1,calls:globalThis.updates});",
        support::PARENT_EVENT,
        "{}",
    );
    let i = packages
        .iter()
        .position(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let old = packages.remove(i);
    let mut declaration = old
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let entry = declaration.entries.get_mut(&EntryTarget::Tui).unwrap();
    entry.scene = None;
    entry.implementation = Implementation::Native {
        implementation: "test.slot-parent".into(),
    };
    let contract = entry.exports["App"].contract.clone();
    let parent = support::package("native.slot-parent", declaration, None);
    input.instances.get_mut("app").unwrap().package = parent.application_ref().into();
    packages.push(parent);
    let mut native = crate::support::scene::registry(PresentationTarget::Tui);
    native
        .register("test.slot-parent", move |_, _values| {
            Ok(BTreeMap::from([(
                "App".into(),
                NativeExport {
                    contract: contract.clone(),
                    value: NativeCapability::Component(Rc::new(SlotParent)),
                },
            )]))
        })
        .unwrap();
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(Default::default()),
        native,
        Default::default(),
    )
    .unwrap();
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    let right = app.child_mount(&root, "right").unwrap().clone();
    assert_eq!(
        app.frame(&root).unwrap().view.view.as_str(),
        "Native parent"
    );
    app.dispatch(&left, 0, "inc", Value::Null).unwrap();
    assert_eq!(
        app.frame(&root).unwrap().state["seen"],
        json!([["left", "changed", 11]])
    );
    for key in ["invalid", "bad_event", "duplicate", "inexact"] {
        assert!(app.dispatch(&root, 1, key, Value::Null).is_err(), "{key}");
        assert_eq!(app.frame(&root).unwrap().revision, 1);
        assert_eq!(app.child_mount(&root, "left").unwrap(), &left);
        assert_eq!(app.frame(&left).unwrap().state["label"], "Left 0");
        assert_eq!(app.frame(&left).unwrap().state["count"], 11);
        assert!(!app.is_retiring(&left));
        assert!(!app.is_retiring(&right));
    }
    app.dispatch(&root, 1, "toggle", Value::Null).unwrap();
    assert!(app.is_retiring(&right));
    app.close(&right, Ok(())).unwrap();
    app.dispatch(&root, 2, "parent", Value::Null).unwrap();
    assert_eq!(app.child_mount(&root, "left").unwrap(), &left);
    assert_eq!(app.frame(&left).unwrap().state["label"], "Left 1");
    assert_eq!(
        app.frame(&left).unwrap().state["calls"],
        1,
        "inexact props never executed JS callback or retired shared realm"
    );
    app.dispatch(&root, 3, "tree", Value::Null).unwrap();
    assert_eq!(app.frame(&root).unwrap().view.view.as_str(), "");
    assert_eq!(app.child_mount(&root, "left").unwrap(), &left);
    app.dispatch(&root, 4, "tree", Value::Null).unwrap();
    assert_eq!(
        app.frame(&root).unwrap().view.view.as_str(),
        "Native parent"
    );
    assert_eq!(app.child_mount(&root, "left").unwrap(), &left);
    // Restore the removed slot at the non-rejecting version 2; it is a new mount.
    app.dispatch(&root, 5, "parent", Value::Null).unwrap();
    app.dispatch(&root, 6, "toggle", Value::Null).unwrap();
    assert_ne!(app.child_mount(&root, "right").unwrap(), &right);
    assert_eq!(app.frame(&left).unwrap().state["count"], 11);
    assert!(app.dispatch(&right, 0, "inc", Value::Null).is_err());
    let before = app.frame(&left).unwrap().revision;
    assert!(app.dispatch(&root, 7, "reject", Value::Null).is_err());
    assert_eq!(app.frame(&root).unwrap().revision, 7);
    assert_eq!(app.frame(&left).unwrap().revision, before);
    assert_eq!(app.frame(&left).unwrap().state["label"], "Left 2");
    // A real JS callback failure retires its realm, while the last committed
    // scenes and exact mount identities remain intact pending actual cleanup.
    assert!(app.is_retiring(&left));
    assert!(
        app.is_retiring(&root),
        "fault retirement reaches dependent parent"
    );
    close_all(&mut app, &root);
}

#[test]
fn native_slot_metadata_rejects_duplicate_and_empty_keys() {
    let slot = |key: String| maka_ui_runtime::ComponentSlot {
        key,
        binding: "child".into(),
        member: 0,
        props: json!({}),
        on: BTreeMap::new(),
    };
    assert!(
        NativeView::new(
            Rc::new(()),
            BTreeMap::new(),
            vec![slot("same".into()), slot("same".into())]
        )
        .is_err()
    );
    assert!(NativeView::new(Rc::new(()), BTreeMap::new(), vec![slot(String::new())]).is_err());
}
