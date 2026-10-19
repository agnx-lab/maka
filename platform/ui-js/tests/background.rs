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

#[path = "../examples/counter/support.rs"]
mod support;
use maka_plugin_core::composition::PresentationTarget;
use maka_ui_runtime::Application;
use serde_json::json;
use std::time::{Duration, Instant};

fn ready(
    handler: &str,
) -> (
    tokio::runtime::Runtime,
    Application,
    maka_plugin_core::runtime::Mount,
) {
    ready_with_registry(
        handler,
        support::scene::registry::<()>(PresentationTarget::Tui),
    )
}
fn ready_with_registry(
    handler: &str,
    native: maka_ui_runtime::NativeRegistry<()>,
) -> (
    tokio::runtime::Runtime,
    Application,
    maka_plugin_core::runtime::Mount,
) {
    let scheduler = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let (input, packages) = support::fixture(handler);
    let mut app = Application::new(
        &input,
        packages,
        scheduler.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: Duration::from_millis(500),
            ..Default::default()
        }),
        native,
        Default::default(),
    )
    .unwrap();
    scheduler.block_on(app.wait_ready()).unwrap();
    let mount = app.mount_launch(json!({"count":3})).unwrap();
    scheduler.block_on(app.wait_idle()).unwrap();
    (scheduler, app, mount)
}
#[test]
fn slow_guest_keeps_owner_available_and_only_prepared_commit_publishes_state() {
    let (scheduler, mut app, mount) = ready(
        "const until=Date.now()+100;while(Date.now()<until){};return frame({count:state.count+1,revision:state.revision+1});",
    );
    app.defer_native_commit();
    let at = Instant::now();
    app.dispatch(&mount, 0, "inc", json!({})).unwrap();
    assert!(
        at.elapsed() < Duration::from_millis(30),
        "foreground waited for JavaScript: {:?}",
        at.elapsed()
    );
    assert_eq!(app.frame(&mount).unwrap().state["count"], 3);
    assert!(app.logical_pending());
    scheduler.block_on(app.wait_prepared(&mount)).unwrap();
    assert_eq!(app.frame(&mount).unwrap().state["count"], 3);
    assert_eq!(app.prepared_frame(&mount).unwrap().state["count"], 4);
    app.commit_ui(&mount, |_, candidate| {
        assert_eq!(candidate.changed().count(), 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(app.frame(&mount).unwrap().state["count"], 4);
    app.close(&mount, Ok(())).unwrap();
    scheduler.block_on(async {
        loop {
            match app.shutdown() {
                Ok(()) => break,
                Err(maka_ui_runtime::Error::Core(maka_plugin_core::Error::CleanupPending)) => {
                    tokio::task::yield_now().await
                }
                Err(error) => panic!("{error}"),
            }
        }
    });
}

#[test]
fn large_shared_state_survives_candidate_rejection_and_retired_worker_completion() {
    use std::sync::{Arc, Condvar, Mutex};

    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let preparing = gate.clone();
    let native = support::scene::registry::<()>(PresentationTarget::Tui)
        .with_scene_compiler(
            support::scene::language(PresentationTarget::Tui),
            move |source| {
                let (locked, changed) = &*preparing;
                let mut held = locked.lock().unwrap();
                while *held {
                    held = changed.wait(held).unwrap();
                }
                support::scene::prepare(source)
            },
            support::scene::finish::<()>,
        )
        .unwrap();
    let (scheduler, mut app, mount) = ready_with_registry(
        "return frame({...state,count:state.count+1,revision:state.revision+1,payload:event.event.payload??state.payload});",
        native,
    );
    let payload = "shared-state-".repeat(100_000);
    app.dispatch(&mount, 0, "inc", json!({"payload":payload}))
        .unwrap();
    scheduler.block_on(app.wait_idle()).unwrap();
    let committed = app.frame(&mount).unwrap().state.clone();
    assert_eq!(committed["count"], 4);
    assert_eq!(committed["payload"].as_str().unwrap().len(), 1_300_000);

    app.defer_native_commit();
    *gate.0.lock().unwrap() = true;
    app.dispatch(&mount, 1, "inc", json!({})).unwrap();
    // The actual accepted invocation retains this allocation in addition to
    // the committed frame and this observer. State size does not add a copy.
    assert!(Arc::strong_count(&committed) >= 3);
    *gate.0.lock().unwrap() = false;
    gate.1.notify_all();
    scheduler.block_on(app.wait_prepared(&mount)).unwrap();
    assert_eq!(app.prepared_frame(&mount).unwrap().state["count"], 5);
    app.commit_ui(&mount, |_, _| {
        Err(maka_ui_runtime::Error::Invalid(
            "reject shared candidate".into(),
        ))
    })
    .unwrap_err();
    assert!(Arc::ptr_eq(&committed, &app.frame(&mount).unwrap().state));
    assert_eq!(app.frame(&mount).unwrap().revision, 1);
    assert_eq!(committed["count"], 4);

    app.dispatch(&mount, 1, "inc", json!({})).unwrap();
    scheduler.block_on(app.wait_prepared(&mount)).unwrap();
    app.commit_ui(&mount, |_, _| Ok(())).unwrap();
    assert_eq!(app.frame(&mount).unwrap().state["count"], 5);
    assert_eq!(
        app.frame(&mount).unwrap().state["payload"],
        committed["payload"]
    );

    let retained = Arc::downgrade(&app.frame(&mount).unwrap().state);
    *gate.0.lock().unwrap() = true;
    app.dispatch(&mount, 2, "inc", json!({})).unwrap();
    app.close(&mount, Ok(())).unwrap();
    assert!(
        retained.upgrade().is_some(),
        "accepted work still owns its input"
    );
    *gate.0.lock().unwrap() = false;
    gate.1.notify_all();
    assert!(scheduler.block_on(app.wait_idle()).is_err());
    assert!(
        retained.upgrade().is_none(),
        "settled work releases its input"
    );
    scheduler.block_on(std::future::poll_fn(|cx| {
        app.set_waker(cx.waker().clone());
        match app.shutdown() {
            Ok(()) => std::task::Poll::Ready(()),
            Err(maka_ui_runtime::Error::Core(maka_plugin_core::Error::CleanupPending)) => {
                std::task::Poll::Pending
            }
            Err(error) => panic!("{error}"),
        }
    }));
}
#[test]
fn retired_mount_waits_for_actual_guest_completion_and_rejects_late_candidate() {
    let (scheduler, mut app, mount) = ready(
        "const until=Date.now()+100;while(Date.now()<until){};return frame({count:999,revision:state.revision+1});",
    );
    app.dispatch(&mount, 0, "inc", json!({})).unwrap();
    app.retire(&mount).unwrap();
    // Native close and accepted execution settle independently. The original
    // provider/Job remains retained after its physical surface is actually gone.
    app.close(&mount, Ok(())).unwrap();
    assert!(!app.can_shutdown());
    assert!(scheduler.block_on(app.wait_idle()).is_err());
    assert!(app.frame(&mount).is_err());
    scheduler.block_on(async {
        loop {
            match app.shutdown() {
                Ok(()) => break,
                Err(maka_ui_runtime::Error::Core(maka_plugin_core::Error::CleanupPending)) => {
                    tokio::task::yield_now().await
                }
                Err(error) => panic!("{error}"),
            }
        }
    });
}

#[test]
fn original_receipts_survive_rejected_candidate_and_drain_in_order_before_retirement() {
    use maka_ui_runtime::{budget::NativeBudget, publication::Observations};
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    let (scheduler, mut app, mount) = ready(
        "const until=Date.now()+100;while(Date.now()<until){};return frame({...state,count:state.count+event.event.sequence,receipts:[...(state.receipts??[]),event.event],revision:state.revision+1});",
    );
    app.defer_native_commit();
    let budget = NativeBudget::default();
    let queue = Observations::new(budget.clone(), app.wake_callback());
    queue.adopt(queue.prepare([("inc", 4096); 2]).unwrap());
    let first = json!({"owner":"original-editor","sequence":1,"outcome":"applied"});
    let second = json!({"owner":"original-editor","sequence":2,"outcome":"conflict"});
    let mut first_ack = queue.capture("inc", first.clone(), false).unwrap().unwrap();
    let second_ack = queue
        .capture("inc", second.clone(), false)
        .unwrap()
        .unwrap();
    let advance = |app: &mut Application| {
        let revision = app.frame(&mount).unwrap().revision;
        queue.advance(
            (revision, revision),
            &mut (),
            |key, event, _| app.dispatch_observation(&mount, revision, key, event),
            |_, _, _| {},
        )
    };
    let at = Instant::now();
    assert!(advance(&mut app).is_empty());
    assert!(at.elapsed() < Duration::from_millis(30));
    scheduler.block_on(app.wait_prepared(&mount)).unwrap();
    assert!(
        !app.commit_ui(&mount, |_, _| Err(maka_ui_runtime::Error::Invalid(
            "native preparation rejected".into()
        )))
        .unwrap()
    );
    assert_eq!(app.frame(&mount).unwrap().state["count"], 3);
    assert_eq!(advance(&mut app).len(), 1);
    assert!(queue.pending());
    let mut cx = Context::from_waker(Waker::noop());
    assert!(
        Pin::new(&mut first_ack).poll(&mut cx).is_pending(),
        "candidate rejection is not publication or retirement"
    );
    assert!(budget.usage().0 > 0);
    // A real independent business input changes the basis. Rejected guest work
    // cannot have mutated the authoritative Rust state or require a rollback.
    app.dispatch(&mount, 0, "inc", json!({"sequence":0}))
        .unwrap();
    scheduler.block_on(app.wait_prepared(&mount)).unwrap();
    app.commit_ui(&mount, |_, _| Ok(())).unwrap();
    for _ in 0..2 {
        assert!(advance(&mut app).is_empty());
        scheduler.block_on(app.wait_prepared(&mount)).unwrap();
        app.commit_ui(&mount, |_, _| Ok(())).unwrap();
    }
    assert!(advance(&mut app).is_empty());
    scheduler.block_on(first_ack).unwrap();
    scheduler.block_on(second_ack).unwrap();
    assert_eq!(
        app.frame(&mount).unwrap().state["receipts"],
        json!([{"sequence":0},first,second])
    );
    assert!(!queue.pending());
    assert_eq!(budget.usage(), (0, 0, 0));
    // Retirement resolves the original publisher ACK, releases its bytes and
    // still waits for the accepted worker execution's actual completion.
    let late = queue
        .capture("inc", json!({"sequence":3,"outcome":"applied"}), false)
        .unwrap()
        .unwrap();
    assert!(advance(&mut app).is_empty());
    app.retire(&mount).unwrap();
    queue.close();
    assert_eq!(budget.usage(), (0, 0, 0));
    app.close(&mount, Ok(())).unwrap();
    assert!(!app.can_shutdown());
    scheduler.block_on(app.wait_idle()).unwrap();
    assert!(matches!(
        scheduler.block_on(late),
        Err(maka_ui_runtime::Error::Core(
            maka_plugin_core::Error::Retired
        ))
    ));
    assert!(app.frame(&mount).is_err());
    scheduler
        .block_on(std::future::poll_fn(|cx| {
            app.set_waker(cx.waker().clone());
            match app.shutdown() {
                Err(maka_ui_runtime::Error::Core(maka_plugin_core::Error::CleanupPending)) => {
                    Poll::Pending
                }
                result => Poll::Ready(result),
            }
        }))
        .unwrap();
}

#[test]
fn a_slow_deployment_loads_modules_sequentially() {
    use maka_plugin_core::package::{EntryTarget, Implementation, Realm};
    use std::task::Poll;
    let scheduler = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let (mut input, mut packages) = support::fixture("");
    let original = packages.pop().unwrap();
    let mut declaration = original
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    declaration
        .entries
        .get_mut(&EntryTarget::Tui)
        .unwrap()
        .implementation = Implementation::JavaScript {
        module: "index.mjs".into(),
        realm: Realm::Shared,
    };
    let source = format!(
        "/*{}*/\nconst until=Date.now()+100;while(Date.now()<until){{}};\n{}",
        "x".repeat(800_000),
        std::str::from_utf8(original.file("index.mjs").unwrap()).unwrap()
    );
    let app = support::package("dynamic.app", declaration, Some(source));
    input.instances.get_mut("app").unwrap().package = app.application_ref().into();
    for index in 1..12 {
        let mut node = support::node(&app, index + 2);
        node.bindings = input.instances["app"].bindings.clone();
        input
            .instances
            .insert(format!("additional{index:02}"), node);
    }
    packages.push(app);
    let mut app = Application::new(
        &input,
        packages,
        scheduler.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: Duration::from_millis(500),
            ..Default::default()
        }),
        support::scene::registry::<()>(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap();
    scheduler.block_on(app.wait_ready()).unwrap();
    let status = app.deployment_status();
    assert!(!status.converging);
    assert!(status.error.is_none());
    let mount = app.mount_launch(json!({"count":3})).unwrap();
    scheduler.block_on(app.wait_idle()).unwrap();
    assert_eq!(app.frame(&mount).unwrap().state["count"], 3);
    app.close(&mount, Ok(())).unwrap();
    scheduler
        .block_on(std::future::poll_fn(|cx| {
            app.set_waker(cx.waker().clone());
            match app.shutdown() {
                Err(maka_ui_runtime::Error::Core(maka_plugin_core::Error::CleanupPending)) => {
                    Poll::Pending
                }
                result => Poll::Ready(result),
            }
        }))
        .unwrap();
}

#[test]
fn shutdown_drains_a_large_composition_behind_its_original_busy_worker() {
    use maka_plugin_core::package::{EntryTarget, Implementation, Realm};
    let scheduler = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let (mut input, mut packages) = support::fixture(
        "const until=Date.now()+200;while(Date.now()<until){};return frame({...state,revision:state.revision+1});",
    );
    let original = packages.pop().unwrap();
    let mut declaration = original
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    declaration
        .entries
        .get_mut(&EntryTarget::Tui)
        .unwrap()
        .implementation = Implementation::JavaScript {
        module: "index.mjs".into(),
        realm: Realm::Shared,
    };
    let package = support::package(
        "dynamic.app",
        declaration,
        Some(
            std::str::from_utf8(original.file("index.mjs").unwrap())
                .unwrap()
                .to_owned(),
        ),
    );
    let mut root = input.instances["app"].clone();
    root.package = package.application_ref().into();
    input.instances.insert("app".into(), root.clone());
    for index in 0..96 {
        let mut instance = root.clone();
        instance.key = uuid::Uuid::from_u128(100 + index);
        input.instances.insert(format!("module-{index}"), instance);
    }
    packages.push(package);
    let mut app = Application::new(
        &input,
        packages,
        scheduler.handle().clone(),
        maka_ui_js::execution(maka_js_runtime::plugin::Limits {
            synchronous_slice: Duration::from_secs(2),
            ..Default::default()
        }),
        support::scene::registry::<()>(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap();
    scheduler.block_on(app.wait_ready()).unwrap();
    let mount = app.mount_launch(json!({})).unwrap();
    scheduler.block_on(app.wait_idle()).unwrap();
    app.dispatch(&mount, 0, "inc", json!({})).unwrap();
    app.close(&mount, Ok(())).unwrap();
    assert!(matches!(
        app.shutdown(),
        Err(maka_ui_runtime::Error::Core(
            maka_plugin_core::Error::CleanupPending
        ))
    ));
    // Native release cannot acknowledge accepted JavaScript or module teardown.
    assert!(!app.can_shutdown());
    assert!(scheduler.block_on(app.wait_idle()).is_err());
    scheduler.block_on(async {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match app.shutdown() {
                    Ok(()) => break,
                    Err(maka_ui_runtime::Error::Core(maka_plugin_core::Error::CleanupPending)) => {
                        tokio::task::yield_now().await
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        })
        .await
        .unwrap();
    });
}
