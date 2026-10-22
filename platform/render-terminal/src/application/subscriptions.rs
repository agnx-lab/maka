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

//! The production terminal task owner drains original streams after restoration.
use super::*;
use maka_plugin_core::contract::{Contract, Interface, Method, MethodMode, Transport};
use maka_ui_runtime::{NativeCapability, NativeExport, actions::*};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

struct State {
    waiting: Semaphore,
    closing: Semaphore,
    release: Semaphore,
    payload: Mutex<Option<std::sync::Weak<()>>>,
}
struct Service(Arc<State>);
impl NativeService for Service {
    fn call(&self, _: ServiceRequest) -> ServiceFuture {
        Box::pin(async { panic!("not a query") })
    }
    fn subscribe(&self, _: ServiceRequest, stop: CancellationToken) -> StreamFuture {
        let payload = Arc::new(());
        *self.0.payload.lock().unwrap() = Some(Arc::downgrade(&payload));
        pull(0, self.0.clone(), payload, stop)
    }
}
fn pull(
    index: usize,
    state: Arc<State>,
    payload: Arc<()>,
    stop: CancellationToken,
) -> StreamFuture {
    Box::pin(async move {
        if index < 2 && !stop.is_cancelled() {
            return StreamCompletion::Item {
                value: json!(index),
                next: pull(index + 1, state, payload, stop),
            };
        }
        state.waiting.add_permits(1);
        stop.cancelled().await;
        state.closing.add_permits(1);
        state.release.acquire().await.unwrap().forget();
        drop(payload);
        StreamCompletion::Finished(ServiceCompletion {
            result: Ok(json!(2)),
            cleanup: Ok(()),
        })
    })
}
#[test]
fn terminal_task_owner_delivers_each_item_then_drains_real_close_after_restoration() {
    let contract = Contract {
        name: "test.Stream".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Service {
            methods: BTreeMap::from([(
                "run".into(),
                Method {
                    progress: None,
                    mode: MethodMode::Subscription,
                    input: json!({}).into(),
                    output: json!({"type":"integer"}).into(),
                    resources: BTreeMap::new(),
                },
            )]),
        },
    };
    // Both raw input and stream feedback return the full committed demand.
    let (runtime, input, packages) = service_fixture(
        "const out=frame({count:state.count+1,revision:state.revision+1});out.subscriptions=[{key:'feed',binding:'service',member:0,method:'run',input:null,resources:{}}];return out;",
        &contract,
    );
    let state = Arc::new(State {
        waiting: Semaphore::new(0),
        closing: Semaphore::new(0),
        release: Semaphore::new(0),
        payload: Mutex::new(None),
    });
    let service = Arc::new(Service(state.clone()));
    let mut native = registry();
    native
        .register("test.service", move |_, _| {
            Ok(BTreeMap::from([(
                "Service".into(),
                NativeExport {
                    contract: contract.clone(),
                    value: NativeCapability::Service(service.clone()),
                },
            )]))
        })
        .unwrap();
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        native,
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    draw(&mut tui, 30, 12);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.waiting.available_permits() == 0 {
        tui.pump_settled().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 3);
    assert_eq!(tui.application.accepted_action_count(), 1);
    let payload = state.payload.lock().unwrap().as_ref().unwrap().clone();
    assert!(payload.upgrade().is_some());
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    while state.closing.available_permits() == 0 {
        tui.pump_settled().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!tui.can_shutdown());
    assert!(payload.upgrade().is_some());
    state.release.add_permits(1);
    while !tui.can_shutdown() {
        tui.pump_settled().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(payload.upgrade().is_none());
    assert_eq!(tui.application.accepted_action_count(), 0);
    tui.shutdown_settled().unwrap();
}
