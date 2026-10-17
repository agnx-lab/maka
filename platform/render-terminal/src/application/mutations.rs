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

//! Presenter close successors; actual Host effects are tested at the Host boundary.
use super::*;
use maka_event_log::{
    EventLog,
    applications::{intents::RecordedIntent, operations::OperationRequest},
    root::{RootNamespaces, RootOwner},
};
use maka_plugin_core::{
    composition::InstanceId,
    contract::{Contract, Interface, Method, MethodMode, Transport},
};
use maka_ui_runtime::{
    NativeCapability, NativeExport,
    actions::{
        CleanupFuture, MutationFuture, NativeMutation, NativeService, ReferenceFuture,
        ServiceFuture, ServiceRequest,
    },
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct Owner {
    request: OperationRequest,
    prepare: Arc<tokio::sync::Semaphore>,
    close: Arc<tokio::sync::Semaphore>,
    closing: Arc<tokio::sync::Semaphore>,
    payload: Mutex<Option<Arc<()>>>,
}
impl NativeMutation for Owner {
    fn prepare(self: Arc<Self>) -> ReferenceFuture {
        Box::pin(async move {
            self.prepare.acquire().await.unwrap().forget();
            self.request.reference().map_err(|e| e.to_string())
        })
    }
    fn dispatch(
        self: Arc<Self>,
        _: RecordedIntent,
        _progress: tokio::sync::mpsc::Sender<serde_json::Value>,
    ) -> MutationFuture {
        Box::pin(async { panic!("retired terminal must not dispatch a mutation") })
    }
    fn cancel(&self) {
        self.prepare.add_permits(1);
    }
    fn close(self: Arc<Self>) -> CleanupFuture {
        Box::pin(async move {
            self.closing.add_permits(1);
            self.close.acquire().await.unwrap().forget();
            self.payload.lock().unwrap().take();
            Ok(())
        })
    }
}
struct Service {
    contract: Contract,
    prepare: Arc<tokio::sync::Semaphore>,
    close: Arc<tokio::sync::Semaphore>,
    closing: Arc<tokio::sync::Semaphore>,
    payload: std::sync::mpsc::Sender<std::sync::Weak<()>>,
}
impl NativeService for Service {
    fn call(&self, _: ServiceRequest) -> ServiceFuture {
        Box::pin(async { panic!("query is not exported") })
    }
    fn mutation(&self, request: ServiceRequest) -> Result<Arc<dyn NativeMutation>, String> {
        let payload = Arc::new(());
        self.payload.send(Arc::downgrade(&payload)).unwrap();
        let original = OperationRequest {
            source: request.source.deployment().unwrap().clone(),
            namespace: InstanceId(vec![Uuid::from_u128(99)]),
            consumer: request.source.consumer().instance().clone(),
            export: "Service".into(),
            method: request.method,
            contract: self.contract.clone(),
            operation: request.source.request_id(),
            input: request.input,
            capture: None,
            origin: maka_capabilities::call::Identity::Remote {
                request_id: request.source.request_id(),
            },
        };
        Ok(Arc::new(Owner {
            request: original,
            prepare: self.prepare.clone(),
            close: self.close.clone(),
            closing: self.closing.clone(),
            payload: Mutex::new(Some(payload)),
        }))
    }
}
#[test]
fn terminal_restoration_during_preparation_runs_close_successor_before_root_release() {
    let contract = Contract {
        name: "test.Service".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Service {
            methods: BTreeMap::from([(
                "run".into(),
                Method {
                    progress: None,
                    mode: MethodMode::Mutation,
                    input: json!({}),
                    output: json!({}),
                    resources: BTreeMap::new(),
                },
            )]),
        },
    };
    let (runtime, input, packages) = service_fixture(
        "const out=frame({count:state.count+1,revision:state.revision+1});if(event.kind==='raw')out.actions.push({key:'finished',binding:'service',member:0,method:'run',input:{},resources:{}});return out;",
        &contract,
    );
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    let ns = RootNamespaces {
        ownership: dir.path().join("owners"),
        control: dir.path().join("control"),
    };
    let store = Arc::new(
        runtime
            .block_on(EventLog::for_root(Arc::new(
                RootOwner::create(&root, &ns).unwrap(),
            )))
            .unwrap(),
    );
    let deployment = Uuid::new_v4();
    let source = runtime
        .block_on(store.commit_application(deployment, 0, input, vec![], packages))
        .unwrap();
    let weak_store = Arc::downgrade(&store);
    let prepare = Arc::new(tokio::sync::Semaphore::new(0));
    let close = Arc::new(tokio::sync::Semaphore::new(0));
    let closing = Arc::new(tokio::sync::Semaphore::new(0));
    let (tx, rx) = std::sync::mpsc::channel();
    let service = Arc::new(Service {
        contract: contract.clone(),
        prepare: prepare.clone(),
        close: close.clone(),
        closing: closing.clone(),
        payload: tx,
    });
    let mut native = registry();
    native
        .register("test.service", move |_, _values| {
            Ok(BTreeMap::from([(
                "Service".into(),
                NativeExport {
                    contract: contract.clone(),
                    value: NativeCapability::Service(service.clone()),
                },
            )]))
        })
        .unwrap();
    let mut tui = runtime
        .block_on(Presenter::start_from_store(
            store,
            source.clone(),
            runtime.handle().clone(),
            native,
            json!({}),
            vec![],
            maka_ui_js::execution(Default::default()),
        ))
        .unwrap();
    draw(&mut tui, 30, 12);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    tui.pump_settled().unwrap();
    let payload = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(tui.application.accepted_action_count(), 0);
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    assert!(!tui.can_shutdown());
    assert!(payload.upgrade().is_some());
    assert!(RootOwner::open(&root, &ns).is_err());
    prepare.add_permits(1);
    let deadline = Instant::now() + Duration::from_secs(5);
    while closing.available_permits() == 0 {
        tui.pump_settled().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(!tui.can_shutdown());
    assert!(payload.upgrade().is_some());
    assert!(RootOwner::open(&root, &ns).is_err());
    let caller = weak_store.upgrade().unwrap();
    let intents = runtime
        .block_on(caller.application_intents(
            source.deployment,
            InstanceId(vec![Uuid::from_u128(2)]),
            0,
            128,
        ))
        .unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].caller, source);
    runtime.block_on(caller.shutdown()).unwrap();
    drop(caller);
    close.add_permits(1);
    while !tui.jobs.is_empty() {
        tui.pump_settled().unwrap();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(payload.upgrade().is_none());
    assert!(tui.can_shutdown());
    tui.shutdown_settled().unwrap();
    assert!(weak_store.upgrade().is_none());
    assert!(RootOwner::open(&root, &ns).is_ok());
}
