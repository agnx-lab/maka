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

use super::*;
use futures_util::future::BoxFuture;
use maka_event_log::{
    EventLog,
    root::{RootNamespaces, RootOwner, private_directory},
};
use maka_host_runtime::{
    application::{
        Context, Handler, NativeActivation, NativeRegistry,
        host::Host,
        transport::{Grant, HostEndpoint},
    },
    ipc::LocalListener,
};
use maka_packages::Package;
use maka_plugin_core::{
    composition::InstanceId,
    contract::Contract,
    operation::{OperationCleanup, OperationOutcome, Receipt},
    runtime::Published,
};
use serde_json::json;
use sqlx::SqlitePool;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use tokio::sync::Notify;

mod drafts;
mod settings;

#[derive(Default)]
struct Counter {
    effects: AtomicUsize,
    blocked: AtomicBool,
    started: Notify,
    release: Notify,
    watches_closed: AtomicUsize,
    messages: std::sync::Mutex<Vec<Value>>,
    secrets: std::sync::Mutex<Vec<Vec<u8>>>,
}
struct CounterHandler(Arc<Counter>);
impl Handler for CounterHandler {
    fn call(
        &self,
        method: String,
        input: Value,
        context: Context,
    ) -> BoxFuture<'static, Result<Value>> {
        let owner = self.0.clone();
        Box::pin(async move {
            match method.as_str() {
                "read" => Ok(json!({"sent":owner.effects.load(Ordering::SeqCst)})),
                "send" | "selectBackend" | "create" | "discoverModels" | "saveCredential" => {
                    owner.messages.lock().unwrap().push(input.clone());
                    if method == "saveCredential" {
                        let secret = &context.attachments()["secret"];
                        owner
                            .secrets
                            .lock()
                            .unwrap()
                            .push(secret.read(0..secret.bytes())?);
                    }
                    if input.get("message").and_then(Value::as_str) == Some("reject") {
                        return Ok(
                            json!({"kind":"rejected","sessionId":input["sessionId"],"reason":"skills_unavailable"}),
                        );
                    }
                    let value = owner.effects.fetch_add(1, Ordering::SeqCst) + 1;
                    owner.started.notify_one();
                    if owner.blocked.load(Ordering::SeqCst) {
                        owner.release.notified().await;
                    }
                    if input.get("message").is_some() {
                        Ok(
                            json!({"kind":"started","sessionId":input["sessionId"],"messageId":format!("message-{value}"),"invocationId":format!("invocation-{value}")}),
                        )
                    } else {
                        Ok(json!({"sent":value}))
                    }
                }
                "watch" => {
                    context
                        .emit(json!({"sent":owner.effects.load(Ordering::SeqCst)}))
                        .await?;
                    context.scope().cancellation.cancelled().await;
                    owner.watches_closed.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"closed":true}))
                }
                _ => Err("unknown counter method".into()),
            }
        })
    }
}
struct Fixture {
    directory: tempfile::TempDir,
    address: PathBuf,
    host: Arc<Host>,
    counter: Arc<Counter>,
    serving: tokio::task::JoinHandle<Result<()>>,
}
impl Fixture {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        private_directory(directory.path()).unwrap();
        let root = Arc::new(
            RootOwner::create(
                &directory.path().join("host"),
                &RootNamespaces {
                    ownership: directory.path().join("owners"),
                    control: directory.path().join("control"),
                },
            )
            .unwrap(),
        );
        let store = Arc::new(EventLog::for_root(root).await.unwrap());
        let contract: Contract = serde_json::from_value(json!({
            "name":"test.Counter","version":1,"transport":"remote","interface":{"kind":"service","methods":{
                "read":{"mode":"query","input":{},"output":{},"resources":{}},
                "send":{"mode":"mutation","input":{},"output":{},"resources":{}},
                "selectBackend":{"mode":"mutation","input":{},"output":{},"resources":{}},
                "stop":{"mode":"mutation","input":{},"output":{},"resources":{}},
                "respond":{"mode":"mutation","input":{},"output":{},"resources":{}},
                "create":{"mode":"mutation","input":{},"output":{},"resources":{}},
                "discoverModels":{"mode":"mutation","input":{},"output":{},"resources":{}},
                "saveCredential":{"mode":"mutation","input":{},"output":{},"resources":{"secret":{"type":{"name":"maka.text.utf8","version":1},"optional":false}}},
                "watch":{"mode":"subscription","input":{},"output":{},"resources":{}}
            }}
        })).unwrap();
        let package = |id: &str, implementation: &str, imports: Value, exports: Value| {
            Package::new(BTreeMap::from([(
                "maka.extension.json".into(),
                serde_json::to_vec(&json!({
                    "schemaVersion":1,"id":id,"application":{"entries":{"host":{
                        "implementation":{"kind":"native","implementation":implementation},
                        "configuration":{"type":"object"},"imports":imports,"exports":exports
                    }}}
                }))
                .unwrap(),
            )]))
            .unwrap()
        };
        let business = package(
            "test.business",
            "test.counter",
            json!({}),
            json!({"Counter":{"contract":contract,"availability":"required"}}),
        );
        let reader = package(
            "test.receipts",
            "maka.operations.receipts",
            json!({}),
            json!({"Receipts":{"contract":receipt_reader_contract(),"availability":"required"}}),
        );
        let gateway = package(
            "test.gateway",
            "maka.application.gateway",
            json!({
                "agent":{"contract":contract,"cardinality":{"kind":"one"}},
                "settings":{"contract":contract,"cardinality":{"kind":"one"}},
                "setup":{"contract":contract,"cardinality":{"kind":"one"}},
                "receipts":{"contract":receipt_reader_contract(),"cardinality":{"kind":"one"}}
            }),
            json!({}),
        );
        let node = |package: &Package, id: u128, bindings: Value| {
            json!({
                "key":Uuid::from_u128(id),"package":{"source":"pinned","package":package.application_ref()},
                "usePackage":"entry","bindings":bindings
            })
        };
        let input = serde_json::from_value(json!({
            "environment":{"location":"host"},"instances":{
                "business":node(&business,1,json!({})),"reader":node(&reader,2,json!({})),
                "gateway":node(&gateway,3,json!({
                    "agent":[{"kind":"instance","path":["business"],"export":"Counter"}],
                    "settings":[{"kind":"instance","path":["business"],"export":"Counter"}],
                    "setup":[{"kind":"instance","path":["business"],"export":"Counter"}],
                    "receipts":[{"kind":"instance","path":["reader"],"export":"Receipts"}]
                }))
            }
        }))
        .unwrap();
        let deployment = Uuid::new_v4();
        store
            .commit_application(
                deployment,
                0,
                input,
                vec![],
                vec![business, reader, gateway],
            )
            .await
            .unwrap();
        let counter = Arc::new(Counter::default());
        let mut native = NativeRegistry::default();
        let handler = counter.clone();
        native
            .insert(
                "test.counter".into(),
                Arc::new(move |_, _| {
                    let handler = handler.clone();
                    let contract = contract.clone();
                    Box::pin(async move {
                        NativeActivation {
                            exports: Ok(BTreeMap::from([(
                                "Counter".into(),
                                Published {
                                    contract,
                                    value: Arc::new(CounterHandler(handler)) as Arc<dyn Handler>,
                                },
                            )])),
                            owner: Arc::new(()),
                        }
                    })
                }),
            )
            .unwrap();
        let host = Host::open(
            store,
            deployment,
            native,
            None,
            BTreeMap::from([
                (
                    "agent".into(),
                    HostEndpoint {
                        consumer: InstanceId(vec![Uuid::from_u128(3)]),
                        import: "agent".into(),
                        grant: Grant {
                            methods: [
                                "read".into(),
                                "send".into(),
                                "selectBackend".into(),
                                "stop".into(),
                                "respond".into(),
                                "watch".into(),
                            ]
                            .into(),
                            recovery: false,
                        },
                    },
                ),
                (
                    "settings".into(),
                    HostEndpoint {
                        consumer: InstanceId(vec![Uuid::from_u128(3)]),
                        import: "settings".into(),
                        grant: Grant {
                            methods: ["create".into(), "saveCredential".into()].into(),
                            recovery: false,
                        },
                    },
                ),
                (
                    "setup".into(),
                    HostEndpoint {
                        consumer: InstanceId(vec![Uuid::from_u128(3)]),
                        import: "setup".into(),
                        grant: Grant {
                            methods: ["discoverModels".into()].into(),
                            recovery: false,
                        },
                    },
                ),
                (
                    "receipts".into(),
                    HostEndpoint {
                        consumer: InstanceId(vec![Uuid::from_u128(3)]),
                        import: "receipts".into(),
                        grant: Grant {
                            methods: ["read".into()].into(),
                            recovery: true,
                        },
                    },
                ),
            ]),
            vec![],
        )
        .await
        .unwrap();
        #[cfg(unix)]
        let address = directory.path().join("host.sock");
        #[cfg(windows)]
        let address = PathBuf::from(format!(r"\\.\pipe\maka-app-client-{}", Uuid::new_v4()));
        let listener = LocalListener::bind(&address).unwrap();
        let serving = tokio::spawn({
            let host = host.clone();
            async move { host.serve_local(listener, CancellationToken::new()).await }
        });
        Self {
            directory,
            address,
            host,
            counter,
            serving,
        }
    }
    async fn client(&self) -> Client {
        let host = HostClient::connect(&self.address, Some(self.host.epoch()))
            .await
            .unwrap();
        let journal = Journal::open(&self.directory.path().join("client.db"))
            .await
            .unwrap();
        Client::new(host, journal, "receipts".into()).unwrap()
    }
    async fn finish(self) {
        self.host.shutdown().await.unwrap();
        self.serving.await.unwrap().unwrap();
    }
}
async fn send(client: &Client, id: Uuid) -> Mutation {
    client
        .mutation(
            id,
            "agent",
            "send",
            json!({"text":"private draft text"}),
            Attachments::new(),
            Some("session-one".into()),
            json!({"draftRevision":7}),
        )
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_original_receipt_survives_cancelled_observer_disconnect_and_restart_without_replay()
 {
    tokio::time::timeout(Duration::from_secs(20),async {
        let fixture = Fixture::new().await;
        fixture.counter.blocked.store(true,Ordering::SeqCst);
        let client = fixture.client().await;
        assert_eq!(client.control(Uuid::new_v4(),"send",Value::Null).await.err().unwrap(),"unsupported agent control method");
        let unrelated = client.mutation(Uuid::new_v4(),"agent","selectBackend",Value::Null,Attachments::new(),None,Value::Null).await.unwrap();
        assert_eq!(client.operation_for_control(unrelated.id()).await.unwrap_err(),"original client operation belongs to another control owner");
        unrelated.close().await.unwrap();
        assert!(client.pending_controls().await.unwrap().is_empty());
        for (method, input, expected) in [
            ("stop", json!({"sessionId":"session-one","invocationId":"run-A"}), json!({"kind":"agent-control","invocationId":"run-A"})),
            ("respond", json!({"sessionId":"session-one","requestId":"interaction-A"}), json!({"kind":"agent-control","requestId":"interaction-A"})),
        ] {
            assert_eq!(client.control(Uuid::new_v4(),method,json!({"sessionId":"session-one"})).await.err().unwrap(),"agent control requires a valid invocationId or requestId");
            let control = client.control(Uuid::new_v4(),method,input).await.unwrap();
            let original = client.operation_for_control(control.id()).await.unwrap();
            assert_eq!(original.session_id.as_deref(),Some("session-one"));
            assert_eq!(original.correlation,expected);
            let mut extra = expected.clone();
            extra["extra"] = json!(true);
            for correlation in [json!({"kind":"agent-control"}), extra] {
                let unrelated = client.mutation(Uuid::new_v4(),"agent",method,Value::Null,Attachments::new(),Some("session-one".into()),correlation).await.unwrap();
                assert_eq!(client.operation_for_control(unrelated.id()).await.unwrap_err(),"original client operation belongs to another control owner");
                unrelated.close().await.unwrap();
            }
            let pending = client.pending_controls().await.unwrap();
            assert_eq!(pending.iter().map(|operation| operation.id).collect::<Vec<_>>(),[control.id()]);
            assert_eq!(pending[0].correlation,expected);
            assert!(client.abandon_unprepared(control.id()).await.unwrap().abandoned);
            assert_eq!(client.operation_for_control(control.id()).await.unwrap().correlation,expected);
            assert!(client.pending_controls().await.unwrap().is_empty());
            control.close().await.unwrap();
        }
        let id = Uuid::new_v4();
        let mutation = client.control(id,"selectBackend",json!({"sessionId":"session-one","text":"private draft text"})).await.unwrap();
        assert_eq!(client.pending_controls().await.unwrap().iter().map(|operation| operation.id).collect::<Vec<_>>(),[id]);
        let original = mutation.prepare().await.unwrap();
        assert_eq!(original.session_id.as_deref(),Some("session-one"));
        assert_eq!(original.correlation,json!({"kind":"agent-control"}));
        assert_eq!(client.operation(id).await.unwrap().reference,original.reference);
        assert_eq!(client.query("agent","read",Value::Null).await.unwrap(),json!({"sent":0}));
        assert!(client.mutation(id,"agent","send",Value::Null,Attachments::new(),None,Value::Null).await.is_err());
        let mut confirmation = Box::pin(mutation.confirm());
        tokio::select! {
            result = &mut confirmation => panic!("gated Host returned early: {result:?}"),
            () = fixture.counter.started.notified() => {},
        }
        drop(confirmation);
        assert!(mutation.confirm().await.is_err(),"the same owner cannot confirm twice");
        mutation.disconnect().await;
        assert!(mutation.close().await.is_err(),"disconnect is not a cleanup acknowledgement");
        assert_eq!(client.pending_requests().await,1);
        fixture.counter.release.notify_one();
        loop {
            let reply = client.recover(id).await.unwrap();
            if let Receipt::Recorded {outcome:OperationOutcome::Returned {value},cleanup:OperationCleanup::Confirmed} = reply.receipt {
                assert_eq!(value,json!({"sent":1}));break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(client.pending_requests().await,1,"receipt recovery cannot settle a lost physical request");
        assert!(client.pending_controls().await.unwrap().is_empty());
        drop(mutation);
        assert_eq!(client.pending_requests().await,1,"dropping a UI handle cannot discard shell custody");
        client.close().await.unwrap();
        assert_eq!(client.pending_requests().await,0);
        let restarted = fixture.client().await;
        let recovered = restarted.recover(id).await.unwrap();
        assert_eq!(recovered.reference,original.reference.unwrap());
        assert_eq!(restarted.operation_for_control(id).await.unwrap().id,id);
        assert!(matches!(recovered.receipt,Receipt::Recorded {outcome:OperationOutcome::Returned {ref value},..} if value==&json!({"sent":1})));
        assert_eq!(restarted.query("agent","read",Value::Null).await.unwrap(),json!({"sent":1}));
        assert!(restarted.mutation(id,"agent","send",Value::Null,Attachments::new(),None,Value::Null).await.is_err());
        assert!(!serde_json::to_string(&restarted.operation(id).await.unwrap()).unwrap().contains("private draft text"));
        restarted.close().await.unwrap();
        fixture.finish().await;
    }).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_client_persistence_never_confirms_the_prepared_host_mutation() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = Fixture::new().await;
        let client = fixture.client().await;
        let mutation = send(&client, Uuid::new_v4()).await;
        let connection = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(fixture.directory.path().join("client.db")),
        )
        .await
        .unwrap();
        sqlx::query("DROP TABLE operations")
            .execute(&connection)
            .await
            .unwrap();
        assert!(mutation.prepare().await.is_err());
        assert!(mutation.confirm().await.is_err());
        assert_eq!(
            client.query("agent", "read", Value::Null).await.unwrap(),
            json!({"sent":0})
        );
        assert_eq!(client.pending_requests().await, 0);
        client.close().await.unwrap();
        fixture.finish().await;
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_a_sql_blocked_preparation_fences_confirmation_even_after_the_writer_is_released()
 {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = Fixture::new().await;
        let client = fixture.client().await;
        let id = Uuid::new_v4();
        let mutation = send(&client, id).await;
        let blocker = SqlitePool::connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(fixture.directory.path().join("client.db")),
        )
        .await
        .unwrap();
        let mut connection = blocker.acquire().await.unwrap();
        let transaction = sqlx::Connection::begin_with(&mut *connection, "BEGIN IMMEDIATE")
            .await
            .unwrap();
        let mut preparing = Box::pin(mutation.prepare());
        tokio::select! {
            result = &mut preparing => panic!("SQL writer is held: {result:?}"),
            () = client.0.journal.1.notified() => {},
        }
        drop(preparing);
        assert_eq!(client.pending_requests().await, 1);
        transaction.commit().await.unwrap();
        // A cancelled SQL observer may or may not have committed. Neither state
        // gives the interrupted Rust owner permission to confirm or start again.
        assert!(mutation.confirm().await.is_err());
        assert!(mutation.prepare().await.is_err());
        mutation.close().await.unwrap();
        assert_eq!(client.pending_requests().await, 0);
        assert_eq!(
            client.query("agent", "read", Value::Null).await.unwrap(),
            json!({"sent":0})
        );
        assert!(
            client
                .mutation(
                    id,
                    "agent",
                    "send",
                    Value::Null,
                    Attachments::new(),
                    None,
                    Value::Null
                )
                .await
                .is_err()
        );
        client.close().await.unwrap();
        fixture.finish().await;
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn subscriptions_and_successful_calls_release_owners_and_scope_close_settles_abandoned_handles()
 {
    tokio::time::timeout(Duration::from_secs(20),async {
        let fixture = Fixture::new().await;
        let client = fixture.client().await;
        assert_eq!(client.query("agent","read",Value::Null).await.unwrap(),json!({"sent":0}));
        let subscription = client.subscribe("agent","watch",Value::Null).await.unwrap();
        assert!(matches!(subscription.next().await.unwrap(),Some(SubscriptionEvent::Item(ref value)) if value==&json!({"sent":0})));
        subscription.close().await.unwrap();
        assert!(subscription.next().await.unwrap().is_none());
        assert_eq!(client.pending_requests().await,0);
        let abandoned = client.subscribe("agent","watch",Value::Null).await.unwrap();
        assert!(matches!(abandoned.next().await.unwrap(),Some(SubscriptionEvent::Item(ref value)) if value==&json!({"sent":0})));
        drop(abandoned);
        assert_eq!(client.pending_requests().await,1);
        client.close().await.unwrap();
        assert_eq!(client.pending_requests().await,0);
        assert_eq!(fixture.counter.watches_closed.load(Ordering::SeqCst),2);
        fixture.finish().await;
    }).await.unwrap();
}
