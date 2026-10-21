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

#[path = "runtime/actions.rs"]
mod actions;
#[path = "runtime/events.rs"]
mod events;
#[path = "runtime/history.rs"]
mod history;
#[path = "runtime/native.rs"]
mod native;
#[path = "runtime/persistence.rs"]
mod persistence;
#[path = "runtime/scenes.rs"]
mod scenes;
#[path = "runtime/sdk.rs"]
mod sdk;
#[path = "runtime/subscriptions.rs"]
mod subscriptions;
#[path = "../examples/counter/support.rs"]
mod support;
#[path = "runtime/values.rs"]
mod values;
use maka_plugin_core::{
    composition::{PackageUse, PresentationTarget, Target},
    contract::{Cardinality, Import},
    package::Declaration,
};
#[path = "runtime/driver.rs"]
mod driver;
use driver::Application;
use maka_ui_runtime::surface_contract;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use support::{fixture, node, package};

fn app(handler: &str, timer: &tokio::runtime::Runtime) -> Application {
    let (input, packages) = fixture(handler);
    Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: Duration::from_millis(100),
            ..Default::default()
        }),
        crate::support::scene::registry::<()>(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap()
}
fn timer() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap()
}

#[test]
fn dynamic_components_publish_then_mount_independent_state_and_commit_input() {
    let timer = timer();
    let mut app = app(
        "return frame({count:state.count+1,revision:state.revision+1});",
        &timer,
    );
    let a = app.mount_launch(json!({"count":10})).unwrap();
    let b = app.mount_launch(json!({"count":20})).unwrap();
    app.dispatch(&a, 0, "inc", json!({})).unwrap();
    assert_eq!(app.frame(&a).unwrap().state["count"], 11);
    assert_eq!(app.frame(&b).unwrap().state["count"], 20);
    assert!(app.dispatch(&a, 0, "inc", json!({})).is_err());
    assert_eq!(app.frame(&a).unwrap().revision, 1);
    app.close(&a, Ok(())).unwrap();
    assert!(app.dispatch(&a, 1, "inc", json!({})).is_err());
    app.dispatch(&b, 0, "inc", json!({})).unwrap();
    assert_eq!(app.frame(&b).unwrap().state["count"], 21);
    app.close(&b, Ok(())).unwrap();
    app.shutdown().unwrap();
}

#[test]
fn invalid_batch_and_script_failure_keep_the_last_committed_view() {
    let timer = timer();
    for handler in [
        "const value=frame({count:999,revision:state.revision+1});value.frame.view.children.push(value.frame.view.children[0]);return value;",
        "const value=frame({count:999,revision:state.revision+1});throw new Error('failed batch');",
    ] {
        let mut app = app(handler, &timer);
        let mount = app.mount_launch(json!({"count":3})).unwrap();
        assert!(app.dispatch(&mount, 0, "inc", Value::Null).is_err());
        assert_eq!(app.frame(&mount).unwrap().state["count"], 3);
        assert_eq!(app.frame(&mount).unwrap().revision, 0);
        app.close(&mount, Ok(())).unwrap();
    }
}

#[test]
fn realm_failure_retires_all_its_mounts_and_unknown_cleanup_keeps_the_owner() {
    let timer = timer();
    let mut app = app("while(true){}", &timer);
    let a = app.mount_launch(json!({})).unwrap();
    let b = app.mount_launch(json!({})).unwrap();
    assert!(app.dispatch(&a, 0, "inc", Value::Null).is_err());
    assert!(app.is_retiring(&a));
    assert!(app.is_retiring(&b));
    assert!(
        app.close(&a, Err("native presentation close unknown".into()))
            .is_err()
    );
    assert!(app.frame(&a).is_ok());
    app.close(&a, Ok(())).unwrap();
    app.close(&b, Ok(())).unwrap();
    app.shutdown().unwrap();
}

#[test]
fn structural_imports_route_to_owned_providers_without_startup_deadlock() {
    use maka_plugin_core::composition::Template;
    let (mut input, mut packages) =
        fixture("return frame({count:state.count+1,revision:state.revision+1});");
    let mut children = std::mem::take(&mut input.instances);
    children.get_mut("app").unwrap().bindings.insert(
        "surface".into(),
        vec![Target::Import {
            name: "surface".into(),
        }],
    );
    let product = package(
        "product",
        Declaration {
            entries: BTreeMap::new(),
            composition: Some(Template {
                parameters: BTreeMap::new(),
                imports: BTreeMap::from([(
                    "surface".into(),
                    Import {
                        contract: surface_contract(PresentationTarget::Tui),
                        cardinality: Cardinality::One,
                    },
                )]),
                exports: BTreeMap::from([(
                    "App".into(),
                    Target::Instance {
                        path: vec!["app".into()],
                        export: "App".into(),
                    },
                )]),
                children,
            }),
        },
        None,
    );
    let mut root = node(&product, 100);
    root.use_package = PackageUse::Composition;
    root.bindings.insert(
        "surface".into(),
        vec![Target::Instance {
            path: vec!["product".into(), "surface".into()],
            export: "Surface".into(),
        }],
    );
    input.instances.insert("product".into(), root);
    input.launch = Some(Target::Instance {
        path: vec!["product".into()],
        export: "App".into(),
    });
    packages.push(product);
    let timer = timer();
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(Default::default()),
        crate::support::scene::registry::<()>(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap();
    let mount = app.mount_launch(json!({})).unwrap();
    app.dispatch(&mount, 0, "inc", Value::Null).unwrap();
    assert_eq!(app.frame(&mount).unwrap().state["count"], 1);
    assert!(app.shutdown().is_err());
    assert!(app.is_retiring(&mount));
    assert!(app.frame(&mount).is_ok());
    app.close(&mount, Ok(())).unwrap();
    app.shutdown().unwrap();
}

#[test]
fn invalid_terminal_surface_field_is_rejected_without_scene_commit() {
    let timer = timer();
    let mut app = app(
        "const next=frame({count:state.count+1,revision:state.revision+1});next.frame.surface.width=900;return next;",
        &timer,
    );
    let mount = app.mount_launch(json!({})).unwrap();
    assert!(app.dispatch(&mount, 0, "inc", Value::Null).is_err());
    assert_eq!(app.frame(&mount).unwrap().revision, 0);
    assert_eq!(
        app.frame(&mount).unwrap().surface,
        Some(maka_ui_runtime::Surface::Terminal {
            binding: "surface".into()
        })
    );
    app.close(&mount, Ok(())).unwrap();
    app.shutdown().unwrap();
}

fn composed(
    child_update: &str,
    parent_event: &str,
    timer: &tokio::runtime::Runtime,
) -> Application {
    let (input, packages) = support::composed_fixture(child_update, parent_event);
    Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: Duration::from_millis(200),
            ..Default::default()
        }),
        crate::support::scene::registry::<()>(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap()
}
fn close_all(app: &mut Application, root: &maka_plugin_core::runtime::Mount) {
    app.retire(root).unwrap();
    for mount in app.retiring_mounts() {
        app.close(&mount, Ok(())).unwrap();
    }
    app.shutdown().unwrap();
}

#[test]
fn imported_children_preserve_local_state_across_parent_props_updates_and_remount() {
    let timer = timer();
    let mut app = composed(support::CHILD_UPDATE, support::PARENT_EVENT, &timer);
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    let right = app.child_mount(&root, "right").unwrap().clone();
    assert_ne!(left, right);
    app.dispatch(&left, 0, "inc", Value::Null).unwrap();
    assert_eq!(app.frame(&left).unwrap().state["count"], 11);
    assert_eq!(app.frame(&right).unwrap().state["count"], 20);
    app.dispatch(&root, 0, "parent", Value::Null).unwrap();
    assert_eq!(*app.child_mount(&root, "left").unwrap(), left);
    assert_eq!(app.frame(&left).unwrap().state["count"], 11);
    assert_eq!(app.frame(&left).unwrap().state["label"], "Left 1");
    assert_eq!(app.frame(&left).unwrap().revision, 2);
    assert!(app.dispatch(&left, 1, "inc", Value::Null).is_err());
    app.dispatch(&root, 1, "toggle", Value::Null).unwrap();
    assert!(app.child_mount(&root, "right").is_err());
    assert!(app.is_retiring(&right));
    assert!(app.frame(&right).is_ok());
    assert!(
        app.close(&right, Err("native close unknown".into()))
            .is_err()
    );
    app.dispatch(&root, 2, "toggle", Value::Null).unwrap();
    let replacement = app.child_mount(&root, "right").unwrap().clone();
    assert_ne!(replacement, right);
    assert_eq!(app.frame(&replacement).unwrap().state["count"], 20);
    assert!(app.dispatch(&right, 1, "inc", Value::Null).is_err());
    app.close(&right, Ok(())).unwrap();
    close_all(&mut app, &root);
}

#[test]
fn child_failure_rolls_back_the_entire_parent_update_and_all_sibling_frames() {
    let timer = timer();
    let mut app = composed(
        "if(props.label.startsWith('Right'))throw new Error('right update failed');return frame({...state,label:props.label,revision:state.revision+1});",
        support::PARENT_EVENT,
        &timer,
    );
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    let right = app.child_mount(&root, "right").unwrap().clone();
    assert!(app.dispatch(&root, 0, "parent", Value::Null).is_err());
    assert_eq!(app.frame(&root).unwrap().revision, 0);
    assert_eq!(app.frame(&left).unwrap().revision, 0);
    assert_eq!(app.frame(&left).unwrap().state["label"], "Left 0");
    assert_eq!(app.frame(&right).unwrap().revision, 0);
    assert!(app.is_retiring(&root));
    assert!(app.is_retiring(&left));
    close_all(&mut app, &root);
}

#[test]
fn invalid_child_props_do_not_publish_parent_or_prior_sibling_updates() {
    let timer = timer();
    let mut app = composed(
        support::CHILD_UPDATE,
        "const next=frame({...state,version:state.version+1,revision:state.revision+1});next.frame.view.children[0].children[1].props.label=42;return next;",
        &timer,
    );
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    assert!(app.dispatch(&root, 0, "parent", Value::Null).is_err());
    assert_eq!(app.frame(&root).unwrap().revision, 0);
    assert_eq!(app.frame(&left).unwrap().revision, 0);
    assert!(!app.is_retiring(&root));
    assert!(app.retiring_mounts().is_empty());
    close_all(&mut app, &root);
}

#[test]
fn sibling_callbacks_each_receive_their_execution_slice() {
    let timer = timer();
    let mut app = composed(
        "const end=Date.now()+125;while(Date.now()<end){}return frame({...state,label:props.label,revision:state.revision+1});",
        support::PARENT_EVENT,
        &timer,
    );
    let root = app.mount_launch(json!({})).unwrap();
    let left = app.child_mount(&root, "left").unwrap().clone();
    let right = app.child_mount(&root, "right").unwrap().clone();
    app.dispatch(&root, 0, "parent", Value::Null).unwrap();
    for mount in [&root, &left, &right] {
        assert_eq!(app.frame(mount).unwrap().revision, 1);
    }
    assert!(!app.is_retiring(&right));
    close_all(&mut app, &root);
}

#[test]
fn invalid_child_initialization_rolls_back_all_private_mounts_without_native_ack() {
    let (mut input, mut packages) =
        support::composed_fixture(support::CHILD_UPDATE, support::PARENT_EVENT);
    let counter = packages.pop().unwrap();
    let mut files = counter.files().clone();
    let source = String::from_utf8(files.remove("index.mjs").unwrap()).unwrap();
    // The left mount initializes successfully before the right fails validation.
    files.insert(
        "index.mjs".into(),
        source
            .replace(
                "surface:null",
                "surface:state.label.startsWith('Right')?{kind:'terminal',binding:'surface'}:null",
            )
            .into_bytes(),
    );
    let counter = maka_packages::Package::new(files).unwrap();
    input.instances.get_mut("counter").unwrap().package = counter.application_ref().into();
    packages.push(counter);
    let timer = timer();
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(Default::default()),
        crate::support::scene::registry::<()>(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap();
    assert!(app.mount_launch(json!({})).is_err());
    assert!(app.retiring_mounts().is_empty());
    // No mount escaped to a renderer; shutdown needs no invented native receipt.
    app.shutdown().unwrap();
}

#[test]
fn a_component_without_a_real_required_callback_cannot_publish() {
    let (mut input, mut packages) = fixture("");
    let root = packages.pop().unwrap();
    let mut files = root.files().clone();
    files
        .get_mut("index.mjs")
        .unwrap()
        .extend_from_slice(b"\napp.update = null;\n");
    let root = maka_packages::Package::new(files).unwrap();
    input.instances.get_mut("app").unwrap().package = root.application_ref().into();
    packages.push(root);
    let timer = timer();
    assert!(
        Application::new(
            &input,
            packages,
            timer.handle().clone(),
            maka_ui_js::execution(Default::default()),
            crate::support::scene::registry::<()>(PresentationTarget::Tui),
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn dispatch_at_a_deep_mount_preserves_the_complete_acyclic_component_tree() {
    use maka_plugin_core::package::{EntryTarget, Realm};
    let (mut input, mut packages) =
        support::composed_fixture(support::CHILD_UPDATE, support::PARENT_EVENT);
    let terminal = packages.pop().unwrap();
    let root = packages.pop().unwrap();
    let source = std::str::from_utf8(root.file("index.mjs").unwrap())
        .unwrap()
        .replace("initial:10", "initial:31")
        .replace("right:true", "right:false");
    let root = package(
        "dynamic.app",
        root.manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone(),
        Some(source),
    );
    input.instances.get_mut("app").unwrap().package = root.application_ref().into();

    let mut declaration = terminal
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let entry = declaration.entries.get_mut(&EntryTarget::Tui).unwrap();
    if let maka_plugin_core::package::Implementation::JavaScript { realm, .. } =
        &mut entry.implementation
    {
        *realm = Realm::Shared;
    }
    entry.imports.insert(
        "counter".into(),
        Import {
            contract: entry.exports["Counter"].contract.clone(),
            cardinality: Cardinality::One,
        },
    );
    let source = std::str::from_utf8(terminal.file("index.mjs").unwrap())
        .unwrap()
        .replace(
            "children:[",
            "children:[...(state.count>0?[{kind:'component',key:'nested',binding:'counter',member:0,on:{},props:{label:state.label,initial:state.count-1}}]:[]),",
        );
    let branch = package("dynamic.branch", declaration, Some(source));
    // Acyclic providers: each component imports the next instance. Initially
    // 32 mounts are visible; the deepest event attempts to add mount 33.
    for level in 1..=33 {
        let name = if level == 1 {
            "counter".to_owned()
        } else {
            format!("counter{level}")
        };
        let mut instance = node(if level == 33 { &terminal } else { &branch }, level + 2);
        if level < 33 {
            instance.bindings.insert(
                "counter".into(),
                vec![Target::Instance {
                    path: vec![format!("counter{}", level + 1)],
                    export: "Counter".into(),
                }],
            );
        }
        input.instances.insert(name, instance);
    }
    packages.extend([root, branch, terminal]);
    let timer = timer();
    let mut app = Application::new(
        &input,
        packages,
        timer.handle().clone(),
        maka_ui_js::execution(Default::default()),
        crate::support::scene::registry::<()>(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap();
    let root = app.mount_launch(json!({})).unwrap();
    let mut deepest = app.child_mount(&root, "left").unwrap().clone();
    for _ in 1..32 {
        deepest = app.child_mount(&deepest, "nested").unwrap().clone();
    }
    assert_eq!(app.frame(&deepest).unwrap().state["count"], 0);
    app.dispatch(&deepest, 0, "inc", Value::Null).unwrap();
    assert_eq!(app.frame(&deepest).unwrap().revision, 1);
    assert_eq!(app.frame(&deepest).unwrap().state["count"], 1);
    let nested = app.child_mount(&deepest, "nested").unwrap();
    assert_eq!(app.frame(nested).unwrap().revision, 0);
    assert_eq!(app.frame(nested).unwrap().state["count"], 0);
    assert!(app.retiring_mounts().is_empty());
    close_all(&mut app, &root);
}
