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
use maka_plugin_core::composition::PresentationTarget;
use maka_ui_runtime::{NativeRegistry, PreparedScene};
use serde_json::{Value, json};
use std::{cell::Cell, collections::BTreeMap, rc::Rc};
fn fixture(
    payload: usize,
) -> (
    maka_plugin_core::composition::Input,
    Vec<maka_packages::Package>,
) {
    let (mut input, mut packages) =
        support::fixture("return frame({count:state.count+1,revision:state.revision+1});");
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
    declaration
        .entries
        .get_mut(&maka_plugin_core::package::EntryTarget::Tui)
        .unwrap()
        .scene = Some(language());
    let mut source = std::str::from_utf8(old.file("index.mjs").unwrap())
        .unwrap()
        .to_owned();
    source.push_str(&format!("const previous=frame;frame=(state)=>{{const output=previous(state);output.frame.view={{language:'private.fixture',blob:'x'.repeat({payload})}};return output;}};"));
    let package = support::package("dynamic.app", declaration, Some(source));
    input.instances.get_mut("app").unwrap().package = package.application_ref().into();
    packages.push(package);
    (input, packages)
}

#[test]
fn missing_scene_compiler_is_rejected_before_native_activation() {
    let timer = timer();
    let (input, packages) = fixture(0);
    let native = NativeRegistry::<()>::with_surface(PresentationTarget::Tui);
    let error = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(Default::default()),
        native,
        Default::default(),
    )
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("scene compiler required"));
}
#[test]
fn scene_compile_failure_preserves_last_committed_frame_and_input_custody() {
    let timer = timer();
    let (input, packages) = fixture(0);
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = fail.clone();
    let native = NativeRegistry::with_surface(PresentationTarget::Tui)
        .with_scene_compiler(
            language(),
            move |_| {
                if observed.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(maka_ui_runtime::Error::Invalid(
                        "scene compile failure".into(),
                    ));
                }
                PreparedScene::new((), BTreeMap::from([("inc".into(), json!(true))]), vec![])
            },
            |prepared| prepared.finish::<(), ()>(Rc::new),
        )
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
    let mount = app.mount_launch(json!({"count":4})).unwrap();
    fail.store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(app.dispatch(&mount, 0, "inc", Value::Null).is_err());
    assert_eq!(app.frame(&mount).unwrap().revision, 0);
    assert_eq!(app.frame(&mount).unwrap().state["count"], 4);
    assert!(!app.is_retiring(&mount));
    fail.store(false, std::sync::atomic::Ordering::SeqCst);
    app.dispatch(&mount, 0, "inc", Value::Null).unwrap();
    assert_eq!(app.frame(&mount).unwrap().state["count"], 5);
    app.close(&mount, Ok(())).unwrap();
    app.shutdown().unwrap();
}

fn language() -> maka_plugin_core::package::SceneLanguage {
    maka_plugin_core::package::SceneLanguage {
        name: "private.fixture.scene".into(),
        version: 1,
    }
}

#[test]
fn selected_languages_are_checked_before_any_factory_or_guest_evaluates() {
    use maka_plugin_core::{
        contract::Availability,
        package::{Declaration, EntryTarget},
    };
    use maka_ui_runtime::{NativeCapability, NativeExport};
    let timer = timer();
    for (name, version, disabled) in [
        ("unknown.scene", 1, false),
        ("private.fixture.scene", 2, false),
        ("unknown.scene", 1, true),
    ] {
        let (mut input, mut packages) = fixture(0);
        let index = packages
            .iter()
            .position(|p| p.manifest().id == "dynamic.app")
            .unwrap();
        let old = packages.remove(index);
        let declaration = old
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone();
        let mut other = declaration.entries[&EntryTarget::Tui].clone();
        other.imports.clear();
        other.exports.get_mut("App").unwrap().availability = Availability::Optional;
        other.scene = Some(maka_plugin_core::package::SceneLanguage {
            name: name.into(),
            version,
        });
        let extra = support::package(
            "later.component",
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, other)]),
                composition: None,
            },
            Some("throw new Error('later guest must not evaluate');".into()),
        );
        let mut node = support::node(&extra, 3);
        node.disabled = disabled;
        input.instances.insert("later".into(), node);
        packages.push(extra);
        let source = std::str::from_utf8(old.file("index.mjs").unwrap()).unwrap();
        let source = if disabled {
            source.to_owned()
        } else {
            format!("throw new Error('earlier guest must not evaluate');\n{source}")
        };
        let root = support::package("dynamic.app", declaration, Some(source));
        input.instances.get_mut("app").unwrap().package = root.application_ref().into();
        packages.push(root);
        let factory_calls = Rc::new(Cell::new(0));
        let observed = factory_calls.clone();
        let mut native = NativeRegistry::default()
            .with_scene_compiler(
                language(),
                |_| PreparedScene::new((), BTreeMap::from([("inc".into(), json!(true))]), vec![]),
                |prepared| prepared.finish::<(), ()>(Rc::new),
            )
            .unwrap();
        native
            .register("maka.ui.surface.tui", move |_, _| {
                observed.set(observed.get() + 1);
                Ok(BTreeMap::from([(
                    "Surface".into(),
                    NativeExport {
                        contract: maka_ui_runtime::surface_contract(PresentationTarget::Tui),
                        value: NativeCapability::Surface(PresentationTarget::Tui),
                    },
                )]))
            })
            .unwrap();
        let result = Application::new(
            &input,
            packages,
            timer.handle().clone(),
            maka_ui_js::execution(Default::default()),
            native,
            Default::default(),
        );
        if disabled {
            let mut app = result.unwrap();
            assert_eq!(factory_calls.get(), 1);
            let mount = app.mount_launch(json!({"count":7})).unwrap();
            app.dispatch(&mount, 0, "inc", json!(null)).unwrap();
            assert_eq!(app.frame(&mount).unwrap().state["count"], 8);
            app.close(&mount, Ok(())).unwrap();
            app.shutdown().unwrap();
        } else {
            let error = result.err().unwrap().to_string();
            assert!(
                error.contains(&format!("unsupported scene language: {name} v{version}")),
                "{error}"
            );
            assert_eq!(factory_calls.get(), 0);
        }
    }
    assert!(
        NativeRegistry::<()>::default()
            .with_scene_compiler(
                maka_plugin_core::package::SceneLanguage {
                    name: "invalid/name".into(),
                    version: 1
                },
                |_| PreparedScene::new((), BTreeMap::new(), vec![]),
                |prepared| prepared.finish::<(), ()>(Rc::new)
            )
            .is_err()
    );
}

#[test]
fn value_only_guest_needs_no_scene_compiler_and_cannot_publish_components() {
    use maka_plugin_core::{
        composition::{Environment, ExecutionLocation, Input},
        contract::Interface,
        package::{Declaration, EntryTarget},
    };
    let timer = timer();
    let (_, packages) = fixture(0);
    let root = packages
        .iter()
        .find(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let mut entry =
        root.manifest().application.as_ref().unwrap().entries[&EntryTarget::Tui].clone();
    entry.imports.clear();
    entry.scene = None;
    entry.exports.get_mut("App").unwrap().contract.interface = Interface::Value {
        schema: json!({"type":"integer"}).into(),
    };
    let contract = entry.exports["App"].contract.clone();
    for undeclared in [false, true] {
        let source = if undeclared {
            // Actual-export checking must reject this before it can ever mount.
            let component = root.manifest().application.as_ref().unwrap().entries
                [&EntryTarget::Tui]
                .exports["App"]
                .contract
                .clone();
            format!(
                "export function activate(){{return {{App:{{kind:'component',contract:{},component:['app']}}}};}}export const app={{mount(){{}},update(){{}},event(){{}}}};",
                serde_json::to_string(&component).unwrap()
            )
        } else {
            format!(
                "export function activate(){{return {{App:{{kind:'value',contract:{},value:42}}}};}}",
                serde_json::to_string(&contract).unwrap()
            )
        };
        let package = support::package(
            "plain.value",
            Declaration {
                entries: BTreeMap::from([(EntryTarget::Tui, entry.clone())]),
                composition: None,
            },
            Some(source),
        );
        let input = Input {
            environment: Environment {
                location: ExecutionLocation::Ui,
                presentation: Some(PresentationTarget::Tui),
            },
            instances: BTreeMap::from([("value".into(), support::node(&package, 1))]),
            recovery: None,
            launch: None,
        };
        let result = Application::<()>::new(
            &input,
            vec![package],
            timer.handle().clone(),
            maka_ui_js::execution(Default::default()),
            NativeRegistry::default(),
            Default::default(),
        );
        if undeclared {
            let error = result.err().unwrap().to_string();
            assert!(error.contains("undeclared or changed export"), "{error}");
        } else {
            result.unwrap().shutdown().unwrap();
        }
    }
}

#[test]
fn composed_render_budget_rejects_child_only_updates_against_retained_ancestors() {
    let timer = timer();
    let (input, packages) = support::composed_fixture(support::CHILD_UPDATE, support::PARENT_EVENT);
    let native = NativeRegistry::with_surface(PresentationTarget::Tui)
        .with_render_depth_limit(10)
        .with_scene_compiler(
            support::scene::language(PresentationTarget::Tui),
            |source| {
                let view = support::scene::prepare(source)?;
                let root = source["key"] == "root";
                let depth = if root {
                    if view.slots()[0].props["label"] == "Left 0" {
                        9
                    } else {
                        8
                    }
                } else if source["children"][1]["text"] == "11" {
                    2
                } else {
                    1
                };
                let slots = view
                    .slots()
                    .iter()
                    .map(|slot| (slot.key.clone(), depth))
                    .collect();
                view.with_render_depth(depth, slots)
            },
            support::scene::finish::<()>,
        )
        .unwrap();
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: std::time::Duration::from_millis(100),
            ..Default::default()
        }),
        native,
        Default::default(),
    )
    .unwrap();
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    let right = app.child_mount(&root, "right").unwrap().clone();
    // Each compiled scene is <=10. Only the full proposed path is 9+2=11.
    let error = app
        .dispatch(&left, 0, "inc", Value::Null)
        .unwrap_err()
        .to_string();
    assert!(error.contains("composed native render depth"), "{error}");
    assert_eq!(app.frame(&root).unwrap().revision, 0);
    assert_eq!(app.frame(&left).unwrap().revision, 0);
    assert_eq!(app.frame(&left).unwrap().state["count"], 10);
    assert_eq!(app.frame(&right).unwrap().revision, 0);
    assert_eq!(app.child_mount(&root, "left").unwrap(), &left);
    // A subsequent ordinary batch reduces the ancestor cost without replacing
    // child custody. The same previously rejected input can now commit at 10.
    app.dispatch(&root, 0, "parent", Value::Null).unwrap();
    assert_eq!(app.child_mount(&root, "left").unwrap(), &left);
    app.dispatch(&left, 1, "inc", Value::Null).unwrap();
    assert_eq!(app.frame(&left).unwrap().state["count"], 11);
    assert_eq!(app.frame(&left).unwrap().revision, 2);
    app.retire(&root).unwrap();
    for mount in app.retiring_mounts() {
        app.close(&mount, Ok(())).unwrap();
    }
    app.shutdown().unwrap();
}

#[test]
fn hidden_native_slot_retains_custody_without_consuming_render_depth() {
    let timer = timer();
    let (input, packages) = support::composed_fixture(support::CHILD_UPDATE, support::PARENT_EVENT);
    let native = NativeRegistry::with_surface(PresentationTarget::Tui)
        .with_render_depth_limit(10)
        .with_scene_compiler(
            support::scene::language(PresentationTarget::Tui),
            |source| {
                let view = support::scene::prepare(source)?;
                if source["key"] == "root" {
                    let hide = view.slots()[0].props["label"] == "Left 0";
                    let slots = view
                        .slots()
                        .iter()
                        .map(|slot| (slot.key.clone(), 9))
                        .collect();
                    view.with_hidden_slots(if hide {
                        std::collections::BTreeSet::from(["left".into()])
                    } else {
                        Default::default()
                    })?
                    .with_render_depth(9, slots)
                } else {
                    let depth = if source["children"][1]["text"] == "11" {
                        2
                    } else {
                        1
                    };
                    view.with_render_depth(depth, BTreeMap::new())
                }
            },
            support::scene::finish::<()>,
        )
        .unwrap();
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: std::time::Duration::from_millis(100),
            ..Default::default()
        }),
        native,
        Default::default(),
    )
    .unwrap();
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    // Hidden owners can settle native observations while retaining their state.
    app.dispatch(&left, 0, "inc", Value::Null).unwrap();
    assert_eq!(app.frame(&left).unwrap().state["count"], 11);
    let error = app
        .dispatch(&root, 0, "parent", Value::Null)
        .unwrap_err()
        .to_string();
    assert!(error.contains("composed native render depth"));
    assert_eq!(app.frame(&root).unwrap().revision, 0);
    assert_eq!(app.frame(&left).unwrap().revision, 1);
    assert_eq!(app.child_mount(&root, "left").unwrap(), &left);
    app.retire(&root).unwrap();
    for mount in app.retiring_mounts() {
        app.close(&mount, Ok(())).unwrap();
    }
    app.shutdown().unwrap();
}

#[test]
fn delayed_scene_preparation_does_not_expire_or_consume_the_next_callback_budget() {
    let timer = timer();
    let (input, packages) =
        support::fixture("return frame({count:state.count+1,revision:state.revision+1});");
    let (release, waiting) = std::sync::mpsc::channel();
    let waiting = std::sync::Mutex::new(Some(waiting));
    let native = NativeRegistry::with_surface(PresentationTarget::Tui)
        .with_scene_compiler(
            support::scene::language(PresentationTarget::Tui),
            move |source| {
                if let Some(waiting) = waiting.lock().unwrap().take() {
                    waiting.recv().unwrap();
                }
                support::scene::prepare(source)
            },
            support::scene::finish::<()>,
        )
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
        native,
        Default::default(),
    )
    .unwrap();
    let root = maka_ui_runtime::Application::mount_launch(&mut app, json!({"count":10})).unwrap();
    let waiting = timer.block_on(async { tokio::time::timeout(slice * 2, app.wait_idle()).await });
    release.send(()).unwrap();
    assert!(waiting.is_err());
    timer.block_on(app.wait_idle()).unwrap();
    assert_eq!(app.frame(&root).unwrap().state["count"], 10);
    app.dispatch(&root, 0, "inc", json!({})).unwrap();
    assert_eq!(app.frame(&root).unwrap().state["count"], 11);
    app.retire(&root).unwrap();
    app.close(&root, Ok(())).unwrap();
}
