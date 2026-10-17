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

//! Public IPC executes the same persisted application and owns caller intent.
#[allow(dead_code)]
mod support;
use maka_event_log::{
    EventLog,
    root::{RootNamespaces, RootOwner},
};
use maka_host_runtime::{
    application::{
        NativeRegistry,
        host::{Client, Host, discovery},
        transport::{Grant, HostEndpoint, Reply},
    },
    ipc::LocalListener,
};
use maka_plugin_core::operation::{OperationCleanup, OperationOutcome, Receipt};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn public_ipc_persists_original_gateway_intent_and_retires_original_owner() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let directory = tempfile::tempdir().unwrap();
        maka_event_log::root::private_directory(directory.path()).unwrap();
        let namespaces = RootNamespaces { ownership: directory.path().join("owners"), control: directory.path().join("control") };
        let root = directory.path().join("root");
        let store = Arc::new(EventLog::for_root(Arc::new(RootOwner::create(&root, &namespaces).unwrap())).await.unwrap());
        let deployment = Uuid::new_v4();
        let (input, packages) = support::host();
        let source = store.commit_application(deployment, 0, input, vec![], packages).await.unwrap();
        let host = Host::open(store.clone(), deployment, NativeRegistry::default(), Some(maka_host_js::factory(Default::default())), BTreeMap::from([
            ("business".into(), HostEndpoint { consumer: support::identity(13), import: "business".into(), grant: Grant { methods: ["read".into(), "write".into()].into(), recovery: false } }),
            ("receipts".into(), HostEndpoint { consumer: support::identity(13), import: "receipts".into(), grant: Grant { methods: ["read".into()].into(), recovery: true } }),
        ]), vec![]).await.unwrap();
        #[cfg(unix)]
        let address = directory.path().join("host.sock");
        #[cfg(windows)]
        let address = std::path::PathBuf::from(format!(r"\\.\pipe\maka-public-test-{}", Uuid::new_v4()));
        let listener = LocalListener::bind(&address).unwrap();
        let registration = host.publish_registration(&address, None).unwrap();
        let discovery = discovery::read_with_namespaces(&root, &namespaces).unwrap();
        let serving = tokio::spawn({ let host = host.clone(); async move { host.serve_local(listener, CancellationToken::new()).await } });
        let mut live = Client::status(&address, Some(host.epoch())).await.unwrap();
        discovery.verify(&live).unwrap();
        live.source.revision += 1;
        assert!(discovery.verify(&live).is_err());
        live.source.revision -= 1;
        live.generation = Some("another deployment".into());
        assert!(discovery.verify(&live).is_err());
        live.generation = None;
        live.pid = std::num::NonZeroU32::new(live.pid.get() + 1).unwrap();
        assert!(discovery.verify(&live).is_err());
        assert!(Client::connect(&address, Some(Uuid::new_v4())).await.is_err());
        let mut client = Client::connect(&address, Some(host.epoch())).await.unwrap();
        let mut other = Client::connect(&address, Some(host.epoch())).await.unwrap();
        assert_eq!(Client::status(&address, Some(host.epoch())).await.unwrap().sessions, 2);
        assert!(!Client::retire_with_options(&address, host.epoch(), false, false).await.unwrap());
        let mut write = client.call("business", "write".into(), Value::Null, Default::default()).await.unwrap();
        write.start().await.unwrap();
        assert!(matches!(write.receive().await.unwrap(), Reply::Prepared { .. }));
        let reference = write.reference().unwrap().clone();
        assert!(store.application_intent(deployment, support::identity(13), reference.operation).await.unwrap().is_none());
        assert!(matches!(write.confirm().await.unwrap(), Reply::Mutation { ref reply, cleanup: Ok(()) } if matches!(reply.receipt, Receipt::Recorded { outcome: OperationOutcome::Returned { .. }, cleanup: OperationCleanup::Confirmed })));
        assert!(write.cleanup_confirmed());
        let intent = store.application_intent(deployment, support::identity(13), reference.operation).await.unwrap().unwrap();
        assert_eq!(intent.reference, reference);
        assert_eq!(intent.caller, source);
        assert_eq!(intent.consumer, support::identity(13));
        // An already committed peer attestation cannot choose the public mode.
        let mut rejected = client.call("business", "write".into(), Value::Null, Default::default()).await.unwrap();
        rejected.start().await.unwrap();
        assert!(matches!(rejected.receive().await.unwrap(), Reply::Prepared { .. }));
        let recorded = store.record_application_intent(source.clone(), support::identity(13), rejected.reference().unwrap().clone(), Value::Null).await.unwrap();
        assert!(matches!(rejected.commit(recorded).await.unwrap(), Reply::Aborted { cleanup: Ok(()), .. }));
        assert!(rejected.cleanup_confirmed());
        client.close().await.unwrap();
        assert!(client.call("business", "read".into(), Value::Null, Default::default()).await.is_err());
        // Closing one physical session cannot revoke a different session.
        let mut read = other.call("business", "read".into(), Value::Null, Default::default()).await.unwrap();
        read.start().await.unwrap();
        assert!(matches!(read.receive().await.unwrap(), Reply::Finished { result: Ok(ref value), cleanup: Ok(()) } if value["effects"] == 1));
        assert!(read.cleanup_confirmed());
        other.close().await.unwrap();
        assert!(Client::retire(&address, host.epoch()).await.unwrap());
        serving.await.unwrap().unwrap();
        assert!(host.status().closed);
        assert!(discovery.verify(&host.status()).is_err());
        registration.remove().unwrap();
        assert_eq!(discovery::read_with_namespaces(&root, &namespaces).unwrap_err().kind(), std::io::ErrorKind::NotFound);
        drop((client, other, write, rejected, read, host, store));
        drop(RootOwner::open(&root, &namespaces).unwrap());
    }).await.unwrap();
}

struct Blocked {
    entered: Arc<tokio::sync::Semaphore>,
    cancelled: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
}
impl maka_host_runtime::application::Handler for Blocked {
    fn call(
        &self,
        _: String,
        _: Value,
        context: maka_host_runtime::application::Context,
    ) -> futures_util::future::BoxFuture<'static, Result<Value, String>> {
        let entered = self.entered.clone();
        let cancelled = self.cancelled.clone();
        let release = self.release.clone();
        Box::pin(async move {
            entered.add_permits(1);
            context.scope().cancellation.cancelled().await;
            cancelled.add_permits(1);
            release.acquire().await.unwrap().forget();
            Ok(Value::Null)
        })
    }
}
impl maka_host_runtime::application::NativeOwner for Blocked {
    fn prepare_retirement(
        &self,
    ) -> Option<Box<dyn maka_host_runtime::application::NativeRetirement>> {
        Some(Box::new(()))
    }
    fn close(&self) -> futures_util::future::BoxFuture<'static, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lost_session_close_observer_cannot_release_accepted_work_or_root_early() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let directory = tempfile::tempdir().unwrap();
        maka_event_log::root::private_directory(directory.path()).unwrap();
        let namespaces = RootNamespaces { ownership: directory.path().join("owners"), control: directory.path().join("control") };
        let root = directory.path().join("root");
        let store = Arc::new(EventLog::for_root(Arc::new(RootOwner::create(&root, &namespaces).unwrap())).await.unwrap());
        let deployment = Uuid::new_v4();
        let (mut input, mut packages) = support::host();
        let package = packages.iter_mut().find(|package| package.manifest().id == "local.business").unwrap();
        let mut declaration = package.manifest().application.as_ref().unwrap().declaration().clone();
        declaration.entries.get_mut(&maka_plugin_core::package::EntryTarget::Host).unwrap().implementation = maka_plugin_core::package::Implementation::Native { implementation: "public.blocked".into() };
        *package = support::counter::package("local.business", declaration, None);
        input.instances.get_mut("business").unwrap().package = package.application_ref().into();
        let blocked = Arc::new(Blocked { entered: Arc::new(tokio::sync::Semaphore::new(0)), cancelled: Arc::new(tokio::sync::Semaphore::new(0)), release: Arc::new(tokio::sync::Semaphore::new(0)) });
        let mut native = NativeRegistry::default();
        native.insert("public.blocked".into(), Arc::new({ let blocked = blocked.clone(); move |_, _| { let blocked = blocked.clone(); Box::pin(async move { maka_host_runtime::application::NativeActivation {
            exports: Ok(BTreeMap::from([("Counter".into(), maka_plugin_core::runtime::Published { contract: support::business(), value: blocked.clone() as Arc<dyn maka_host_runtime::application::Handler> })])), owner: blocked,
        } }) } })).unwrap();
        store.commit_application(deployment, 0, input, vec![], packages).await.unwrap();
        let host = Host::open(store.clone(), deployment, native, Some(maka_host_js::factory(Default::default())), BTreeMap::from([
            ("business".into(), HostEndpoint { consumer: support::identity(13), import: "business".into(), grant: Grant { methods: ["write".into()].into(), recovery: false } }),
        ]), vec![]).await.unwrap();
        #[cfg(unix)]
        let address = directory.path().join("host.sock");
        #[cfg(windows)]
        let address = std::path::PathBuf::from(format!(r"\\.\pipe\maka-public-test-{}", Uuid::new_v4()));
        let listener = LocalListener::bind(&address).unwrap();
        let serving = tokio::spawn({ let host = host.clone(); async move { host.serve_local(listener, CancellationToken::new()).await } });
        let mut client = Client::connect(&address, Some(host.epoch())).await.unwrap();
        let mut call = client.call("business", "write".into(), Value::Null, Default::default()).await.unwrap();
        call.start().await.unwrap();
        assert!(matches!(call.receive().await.unwrap(), Reply::Prepared { .. }));
        let mut confirming = Box::pin(call.confirm());
        tokio::select! {
            permit = blocked.entered.acquire() => permit.unwrap().forget(),
            result = &mut confirming => panic!("accepted work ended before release: {result:?}"),
        }
        drop(confirming);
        assert!(!Client::retire(&address, host.epoch()).await.unwrap());
        let mut closing = Box::pin(client.close());
        tokio::select! {
            permit = blocked.cancelled.acquire() => permit.unwrap().forget(),
            result = &mut closing => panic!("session acknowledged before physical cleanup: {result:?}"),
        }
        drop(closing);
        drop(client);
        assert!(!Client::retire(&address, host.epoch()).await.unwrap());
        assert_eq!(RootOwner::open(&root, &namespaces).err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
        assert!(Client::retire_with_options(&address, host.epoch(), true, true).await.unwrap());
        host.wait_for_drain().await;
        assert!(!host.status().closed);
        assert!(!serving.is_finished());
        assert_eq!(RootOwner::open(&root, &namespaces).err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
        blocked.release.add_permits(1);
        serving.await.unwrap().unwrap();
        // The disconnected invocation still has no same-channel cleanup receipt.
        call.disconnect();
        assert!(!call.cleanup_confirmed());
        drop((call, host, store));
        drop(RootOwner::open(&root, &namespaces).unwrap());
    }).await.unwrap();
}

struct PeerHost {
    directory: tempfile::TempDir,
    namespaces: RootNamespaces,
    store: Arc<EventLog>,
    host: Arc<Host>,
    address: std::path::PathBuf,
    serving: tokio::task::JoinHandle<Result<(), String>>,
    selections: BTreeMap<String, HostEndpoint>,
}
impl PeerHost {
    async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        maka_event_log::root::private_directory(directory.path()).unwrap();
        let namespaces = RootNamespaces {
            ownership: directory.path().join("owners"),
            control: directory.path().join("control"),
        };
        let store = Arc::new(
            EventLog::for_root(Arc::new(
                RootOwner::create(&directory.path().join("host"), &namespaces).unwrap(),
            ))
            .await
            .unwrap(),
        );
        let (input, packages) = support::host();
        let deployment = Uuid::new_v4();
        store
            .commit_application(deployment, 0, input, vec![], packages)
            .await
            .unwrap();
        let selections = BTreeMap::from([
            (
                "business".into(),
                HostEndpoint {
                    consumer: support::identity(13),
                    import: "business".into(),
                    grant: Grant {
                        methods: ["read".into(), "write".into()].into(),
                        recovery: false,
                    },
                },
            ),
            (
                "receipts".into(),
                HostEndpoint {
                    consumer: support::identity(13),
                    import: "receipts".into(),
                    grant: Grant {
                        methods: ["read".into()].into(),
                        recovery: true,
                    },
                },
            ),
        ]);
        let host = Host::open(
            store.clone(),
            deployment,
            NativeRegistry::default(),
            Some(maka_host_js::factory(Default::default())),
            selections.clone(),
            vec![],
        )
        .await
        .unwrap();
        #[cfg(unix)]
        let address = directory.path().join("host.sock");
        #[cfg(windows)]
        let address =
            std::path::PathBuf::from(format!(r"\\.\pipe\maka-peer-test-{}", Uuid::new_v4()));
        let listener = LocalListener::bind(&address).unwrap();
        let serving = tokio::spawn({
            let host = host.clone();
            async move { host.serve_local(listener, CancellationToken::new()).await }
        });
        Self {
            directory,
            namespaces,
            store,
            host,
            address,
            serving,
            selections,
        }
    }
    async fn ui(
        &self,
        name: &str,
        target: maka_plugin_core::composition::PresentationTarget,
    ) -> (
        Arc<EventLog>,
        maka_plugin_core::deployment::DeploymentRef,
        maka_plugin_core::composition::InstanceId,
    ) {
        let store = Arc::new(
            EventLog::for_root(Arc::new(
                RootOwner::create(&self.directory.path().join(name), &self.namespaces).unwrap(),
            ))
            .await
            .unwrap(),
        );
        let (input, packages, observations) = support::ui(target);
        let consumer = maka_plugin_core::composition::InstanceId(vec![input.instances["app"].key]);
        let source = store
            .commit_application(Uuid::new_v4(), 0, input, observations, packages)
            .await
            .unwrap();
        (store, source, consumer)
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quit_keeps_other_clients_and_stops_after_the_last_client() {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let fixture = PeerHost::start().await;
        let epoch = Some(fixture.host.epoch());
        let mut detached = Client::connect(&fixture.address, epoch).await.unwrap();
        detached.close().await.unwrap();
        let mut first = Client::connect(&fixture.address, epoch).await.unwrap();
        let mut second = Client::connect(&fixture.address, epoch).await.unwrap();
        first.quit().await.unwrap();
        let mut read = second
            .call("business", "read".into(), Value::Null, Default::default())
            .await
            .unwrap();
        read.start().await.unwrap();
        assert!(matches!(read.receive().await.unwrap(), Reply::Finished { result: Ok(ref value), cleanup: Ok(()) } if value["effects"] == 0));
        second.quit().await.unwrap();
        fixture.serving.await.unwrap().unwrap();
        drop(fixture.host);
        drop(fixture.store);
        drop(RootOwner::open(&fixture.directory.path().join("host"), &fixture.namespaces).unwrap());
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_quits_stop_the_host() {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let fixture = PeerHost::start().await;
        let epoch = Some(fixture.host.epoch());
        let mut first = Client::connect(&fixture.address, epoch).await.unwrap();
        let mut second = Client::connect(&fixture.address, epoch).await.unwrap();
        let (first, second) = tokio::join!(first.quit(), second.quit());
        first.unwrap();
        second.unwrap();
        fixture.serving.await.unwrap().unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_captures_the_replacement_host_registration() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let fixture = PeerHost::start().await;
        let mut original = Client::connect(&fixture.address, Some(fixture.host.epoch())).await.unwrap();
        let before = original.catalogue().describe("business").unwrap().registration_epoch.unwrap();
        let (mut input, packages) = support::host();
        let provider = Uuid::new_v4();
        input.instances.get_mut("business").unwrap().key = provider;
        fixture.store.commit_application(fixture.host.source().deployment, 1, input, vec![], packages).await.unwrap();
        let mut connected = loop {
            let mut connected = Client::connect(&fixture.address, Some(fixture.host.epoch())).await.unwrap();
            let observed = connected.catalogue().describe("business").unwrap();
            if observed.registration_epoch.is_some_and(|epoch| epoch != before) {
                assert_eq!(observed.provider.0, vec![provider]);
                break connected;
            }
            connected.close().await.unwrap();
            tokio::task::yield_now().await;
        };
        let mut read = connected.call("business", "read".into(), Value::Null, Default::default()).await.unwrap();
        read.start().await.unwrap();
        assert!(matches!(read.receive().await.unwrap(), Reply::Finished { result: Ok(ref value), cleanup: Ok(()) } if value["effects"] == 0));
        original.close().await.unwrap();
        connected.close().await.unwrap();
        assert!(Client::retire(&fixture.address, fixture.host.epoch()).await.unwrap());
        fixture.serving.await.unwrap().unwrap();
    }).await.unwrap();
}

async fn peer_call(
    client: &Client,
    source: &maka_plugin_core::deployment::DeploymentRef,
    consumer: &maka_plugin_core::composition::InstanceId,
    method: &str,
) -> maka_host_runtime::application::host::LocalCall {
    let (reader, writer) = tokio::io::split(client.open_call_channel().await.unwrap());
    maka_host_runtime::application::transport::Call::new(
        reader,
        writer,
        client.catalogue().select("business").unwrap(),
        maka_host_runtime::application::transport::Source {
            deployment: source.clone(),
            consumer: consumer.clone(),
            mount: Uuid::new_v4(),
            request: Uuid::new_v4(),
        },
        method.into(),
        Value::Null,
        Default::default(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_peer_sessions_preserve_ui_journals_grants_and_independent_revision_approvals() {
    use maka_plugin_core::composition::PresentationTarget;
    tokio::time::timeout(Duration::from_secs(30), async {
        let fixture = PeerHost::start().await;
        let (ui, source, consumer) = fixture.ui("tui", PresentationTarget::Tui).await;
        let (other_ui, other_source, other_consumer) = fixture.ui("second_ui", PresentationTarget::Tui).await;
        let epoch = Some(fixture.host.epoch());
        let mut client = Client::connect_peer(&fixture.address, epoch, source.clone(), [consumer.clone()].into(), fixture.selections.clone()).await.unwrap();
        let mut readonly = fixture.selections.clone();
        readonly.get_mut("business").unwrap().grant.methods.remove("write");
        let mut other = Client::connect_peer(&fixture.address, epoch, other_source.clone(), [other_consumer.clone()].into(), readonly.clone()).await.unwrap();
        for invalid in [
            { let mut selected = readonly.clone(); selected.get_mut("business").unwrap().consumer = support::identity(999); selected },
            { let mut selected = readonly.clone(); selected.get_mut("business").unwrap().import = "receipts".into(); selected },
            { let mut selected = readonly.clone(); selected.get_mut("business").unwrap().grant.recovery = true; selected },
            { let mut selected = readonly.clone(); selected.get_mut("business").unwrap().grant.methods.insert("unpublished".into()); selected },
        ] {
            assert!(Client::connect_peer(&fixture.address, epoch, source.clone(), [consumer.clone()].into(), invalid).await.is_err());
        }
        assert!(client.call("business", "write".into(), Value::Null, Default::default()).await.is_err());
        let mut wrong_consumer = peer_call(&client, &source, &support::identity(999), "read").await;
        assert!(wrong_consumer.start().await.is_err());
        assert!(wrong_consumer.cleanup_confirmed());
        let mut wrong_source = peer_call(&client, &other_source, &other_consumer, "read").await;
        assert!(wrong_source.start().await.is_err());
        assert!(wrong_source.cleanup_confirmed());
        let mut denied = peer_call(&other, &other_source, &other_consumer, "write").await;
        assert!(denied.start().await.is_err());
        assert!(denied.cleanup_confirmed());
        let mut owned_confirmation = peer_call(&client, &source, &consumer, "write").await;
        owned_confirmation.start().await.unwrap();
        assert!(matches!(owned_confirmation.receive().await.unwrap(), Reply::Prepared { .. }));
        assert!(matches!(owned_confirmation.confirm().await.unwrap(), Reply::Aborted { cleanup: Ok(()), .. }));
        let mut write = peer_call(&client, &source, &consumer, "write").await;
        write.start().await.unwrap();
        assert!(matches!(write.receive().await.unwrap(), Reply::Prepared { .. }));
        let reference = write.reference().unwrap().clone();
        assert!(ui.application_intent(source.deployment, consumer.clone(), reference.operation).await.unwrap().is_none());
        let recorded = ui.record_application_intent(source.clone(), consumer.clone(), reference.clone(), Value::Null).await.unwrap();
        assert_eq!(recorded.intent().caller, source);
        assert!(matches!(write.commit(recorded).await.unwrap(), Reply::Mutation { ref reply, cleanup: Ok(()) } if matches!(reply.receipt, Receipt::Recorded { outcome: OperationOutcome::Returned { .. }, cleanup: OperationCleanup::Confirmed })));
        assert!(write.cleanup_confirmed());
        assert_eq!(ui.application_intent(source.deployment, consumer.clone(), reference.operation).await.unwrap().unwrap().reference, reference);
        assert!(fixture.store.application_deployment_intents(fixture.host.source().deployment, 0, 10).await.unwrap().is_empty());
        let current = ui.load_application_at(&source).await.unwrap().unwrap();
        let (current, packages, _, _) = current.into_parts();
        let next = ui.commit_application(source.deployment, 1, current.input, current.remote_exports, packages).await.unwrap();
        let captured = client.capture().await.unwrap();
        assert_eq!(client.epochs().await.unwrap(), captured.epochs());
        assert!(client.check_approval(&next, &[consumer.clone()].into(), &[1].into(), &Some(captured.clone())).is_ok());
        assert!(client.check_approval(&other_source, &[consumer.clone()].into(), &[1].into(), &Some(captured.clone())).is_err());
        let mut altered = serde_json::to_value(&captured).unwrap();
        altered["endpoints"]["business"]["provider"] = serde_json::to_value(support::identity(999)).unwrap();
        let altered = serde_json::from_value(altered).unwrap();
        assert!(client.approve(next.clone(), [consumer.clone()].into(), [1].into(), Some(altered)).await.is_err());
        // A rejected comparison consumes that exact captured owner; an old catalogue cannot recapture it.
        assert!(client.approve(next.clone(), [consumer.clone()].into(), [1].into(), Some(captured)).await.is_err());
        let captured = client.capture().await.unwrap();
        client.approve(next.clone(), [consumer.clone()].into(), [1].into(), Some(captured)).await.unwrap();
        let mut retained = peer_call(&client, &source, &consumer, "read").await;
        retained.start().await.unwrap();
        assert!(matches!(retained.receive().await.unwrap(), Reply::Finished { result: Ok(ref value), .. } if value["effects"] == 1));
        client.approve(next.clone(), Default::default(), [2].into(), None).await.unwrap();
        let mut revoked = peer_call(&client, &source, &consumer, "read").await;
        assert!(revoked.start().await.is_err());
        assert!(revoked.cleanup_confirmed());
        let recaptured = client.capture().await.unwrap();
        assert!(client.approve(source.clone(), [consumer.clone()].into(), [2].into(), Some(recaptured)).await.is_err());
        client.close().await.unwrap();
        let mut read = peer_call(&other, &other_source, &other_consumer, "read").await;
        read.start().await.unwrap();
        assert!(matches!(read.receive().await.unwrap(), Reply::Finished { result: Ok(ref value), cleanup: Ok(()) } if value["effects"] == 1));
        other.close().await.unwrap();
        assert!(Client::retire(&fixture.address, fixture.host.epoch()).await.unwrap());
        fixture.serving.await.unwrap().unwrap();
        ui.shutdown().await.unwrap();
        other_ui.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_peer_control_never_appends_another_command_to_its_anchor() {
    use maka_host_runtime::application::{
        host::protocol::{Connected, Opening, Request, Response, SessionControl, VERSION},
        transport::wire,
    };
    use maka_plugin_core::composition::PresentationTarget;
    tokio::time::timeout(Duration::from_secs(30), async {
        let fixture = PeerHost::start().await;
        let (ui, source, consumer) = fixture.ui("ui", PresentationTarget::Tui).await;
        let mut probe = Client::connect_peer(&fixture.address, Some(fixture.host.epoch()), source.clone(), [consumer.clone()].into(), fixture.selections.clone()).await.unwrap();
        let catalogue = probe.catalogue().clone();
        probe.close().await.unwrap();
        // Exercise a partially written command on the actual Host. Unix retains
        // the read half; Windows closes the whole pipe without a cleanup receipt.
        #[cfg(windows)]
        let mut survivor = Client::connect_peer(&fixture.address, Some(fixture.host.epoch()), source.clone(), [consumer.clone()].into(), fixture.selections.clone()).await.unwrap();
        let mut actual = wire::Reader::new(maka_host_runtime::ipc::connect(&fixture.address).await.unwrap());
        wire::write(&mut actual.inner, &Opening {
            client_instance: None, version: VERSION, epoch: Some(fixture.host.epoch()),
            request: Request::ConnectPeer { source: source.clone(), consumers: [consumer.clone()].into(), endpoints: fixture.selections.clone() },
        }).await.unwrap();
        assert!(matches!(actual.read::<Response>().await.unwrap(), Response::Connected(_)));
        wire::write(&mut actual.inner, &SessionControl::Capture).await.unwrap();
        {
            use tokio::io::AsyncWriteExt;
            actual.inner.write_all(&[0, 0, 0, 8, b'{']).await.unwrap();
            actual.inner.shutdown().await.unwrap();
        }
        #[cfg(unix)]
        {
            assert!(matches!(actual.read::<Response>().await.unwrap(), Response::Captured(Ok(_))));
            assert!(matches!(actual.read::<Response>().await.unwrap(), Response::Closed));
        }
        #[cfg(windows)]
        {
            assert!(actual.read::<Response>().await.is_err());
            while fixture.host.status().sessions != 1 {
                tokio::task::yield_now().await;
            }
            assert!(!fixture.host.status().closing);
            let mut read = peer_call(&survivor, &source, &consumer, "read").await;
            read.start().await.unwrap();
            assert!(matches!(read.receive().await.unwrap(), Reply::Finished { result: Ok(ref value), cleanup: Ok(()) } if value["effects"] == 0));
            survivor.close().await.unwrap();
        }

        for quitting in [false, true] {
        #[cfg(unix)]
        let address = fixture.directory.path().join(if quitting { "quit-control.sock" } else { "control.sock" });
        #[cfg(windows)]
        let address = std::path::PathBuf::from(format!(r"\\.\pipe\maka-peer-control-test-{}", Uuid::new_v4()));
        let mut listener = LocalListener::bind(&address).unwrap();
        let (received, command_received) = tokio::sync::oneshot::channel();
        let (quit_received, independent_quit_received) = tokio::sync::oneshot::channel();
        let (release_ack, ack_released) = tokio::sync::oneshot::channel();
        let session = Uuid::new_v4();
        let catalogue = catalogue.clone();
        let epoch = fixture.host.epoch();
        let connected_source = fixture.host.source().clone();
        let connected_consumers = fixture.selections.iter().map(|(name, selection)| (name.clone(), selection.consumer.clone())).collect();
        let fake = tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            let mut reader = wire::Reader::new(stream);
            assert!(matches!(reader.read::<Opening>().await.unwrap().request, Request::ConnectPeer { .. }));
            wire::write(&mut reader.inner, &Response::Connected(Connected {
                version: VERSION, epoch, session, source: connected_source,
                consumers: connected_consumers, catalogue: catalogue.clone(),
            })).await.unwrap();
            assert!(matches!(reader.read::<SessionControl>().await.unwrap(), SessionControl::Capture));
            received.send(()).unwrap();
            if quitting {
                let stream = tokio::select! {
                    stream = listener.accept() => stream.unwrap(),
                    _ = reader.read::<SessionControl>() => panic!("quit appended to or closed the lost anchor"),
                };
                let mut control = wire::Reader::new(stream);
                let opening = control.read::<Opening>().await.unwrap();
                assert_eq!(opening.epoch, Some(epoch));
                assert!(matches!(opening.request, Request::Quit { session: actual } if actual == session));
                // Old anchor replies cannot acknowledge the independent quit.
                wire::write(&mut reader.inner, &Response::Captured(Ok(catalogue))).await.unwrap();
                wire::write(&mut reader.inner, &Response::Closed).await.unwrap();
                quit_received.send(()).unwrap();
                ack_released.await.unwrap();
                wire::write(&mut control.inner, &Response::Closed).await.unwrap();
                assert!(reader.read::<SessionControl>().await.is_err());
            } else {
                // EOF proves no later control frame was appended after lost observation.
                assert!(reader.read::<SessionControl>().await.is_err());
                #[cfg(unix)]
                {
                    // Unix retains the read half for original replies and final cleanup.
                    wire::write(&mut reader.inner, &Response::Captured(Ok(catalogue))).await.unwrap();
                    wire::write(&mut reader.inner, &Response::Closed).await.unwrap();
                }
            }
        });
        let mut client = Client::connect_peer(&address, Some(epoch), source.clone(), [consumer.clone()].into(), fixture.selections.clone()).await.unwrap();
        let mut capture = Box::pin(client.capture());
        tokio::select! {
            result = &mut capture => panic!("capture finished without its acknowledgement: {result:?}"),
            result = command_received => result.unwrap(),
        }
        drop(capture);
        assert!(client.epochs().await.is_err());
        assert!(client.capture().await.is_err());
        assert!(client.approve(source.clone(), [consumer.clone()].into(), Default::default(), None).await.is_err());
        assert!(client.open_call_channel().await.is_err());
        if quitting {
            let mut quit = Box::pin(client.quit());
            tokio::select! {
                result = &mut quit => panic!("quit completed before its independent acknowledgement: {result:?}"),
                result = independent_quit_received => result.unwrap(),
            }
            assert!(futures_util::poll!(quit.as_mut()).is_pending(), "old anchor replies acknowledged the independent quit");
            // Only this independently gated Closed response confirms cleanup.
            release_ack.send(()).unwrap();
            quit.await.unwrap();
            client.quit().await.unwrap();
            client.close().await.unwrap();
        } else {
            #[cfg(unix)]
            {
                client.close().await.unwrap();
                client.close().await.unwrap();
            }
            #[cfg(windows)]
            {
                assert!(client.close().await.is_err());
                assert!(client.close().await.is_err());
            }
        }
        assert!(client.open_call_channel().await.is_err());
        drop(client);
        fake.await.unwrap();
        }
        assert!(Client::retire(&fixture.address, fixture.host.epoch()).await.unwrap());
        fixture.serving.await.unwrap().unwrap();
        ui.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discovered_connections_release_ui_roots_without_stopping_the_shared_host() {
    use maka_plugin_core::composition::PresentationTarget;
    use maka_transport::{Configuration, Connections, HostLocation};
    tokio::time::timeout(Duration::from_secs(30), async {
        let fixture = PeerHost::start().await;
        let registration = fixture.host.publish_registration(&fixture.address, None).unwrap();
        let (ui, source, _) = fixture.ui("tui-connections", PresentationTarget::Tui).await;
        let configuration = || Configuration { hosts: vec![maka_transport::Host {
            name: "local".into(),
            location: HostLocation::Public { root: fixture.directory.path().join("host"), state_domain: Some(fixture.host.root_id().into()) },
            deployment: fixture.host.source().deployment,
            endpoints: fixture.selections.clone(), native: vec![],
        }] };
        for invalid in 0..3 {
            let mut input = configuration();
            match invalid {
                0 => input.hosts[0].deployment = Uuid::new_v4(),
                1 => input.hosts[0].location = HostLocation::Public { root: fixture.directory.path().join("host"), state_domain: Some("0".repeat(64)) },
                _ => input.hosts[0].location = HostLocation::Public { root: fixture.directory.path().join("never-create"), state_domain: None },
            }
            let mut connections = Connections::prepare(ui.clone(), source.deployment, PresentationTarget::Tui, input, fixture.namespaces.clone(), std::path::PathBuf::from("unused-local-startup-helper"), NativeRegistry::default(), Some(maka_host_js::factory(Default::default()))).await.unwrap();
            assert!(connections.start().await.is_err());
            connections.close().await.unwrap();
        }
        assert!(!fixture.directory.path().join("never-create").exists());
        let mut connections = Connections::prepare(ui.clone(), source.deployment, PresentationTarget::Tui, configuration(), fixture.namespaces.clone(), std::path::PathBuf::from("unused-local-startup-helper"), NativeRegistry::default(), Some(maka_host_js::factory(Default::default()))).await.unwrap();
        connections.start().await.unwrap();
        let frozen = connections.registrations();
        assert_eq!(frozen.len(), 2);
        let (second_ui, second_source, _) = fixture.ui("second_ui-connections", PresentationTarget::Tui).await;
        let mut other = Connections::prepare(second_ui.clone(), second_source.deployment, PresentationTarget::Tui, configuration(), fixture.namespaces.clone(), std::path::PathBuf::from("unused-local-startup-helper"), NativeRegistry::default(), Some(maka_host_js::factory(Default::default()))).await.unwrap();
        other.start().await.unwrap();
        assert_eq!(fixture.host.status().sessions, 2);
        connections.close().await.unwrap();
        ui.shutdown().await.unwrap();
        drop(ui);
        drop(RootOwner::open(&fixture.directory.path().join("tui-connections"), &fixture.namespaces).unwrap());
        while fixture.host.status().sessions != 1 {
            tokio::task::yield_now().await;
        }
        assert!(!fixture.host.status().closing);
        // Retained registrations carry no anchor lease after the owning UI closed.
        assert_eq!(frozen.len(), 2);
        assert_eq!(other.approve_source(&second_ui, second_source.clone(), Default::default()).await.unwrap().len(), 2);
        let mut observer = Client::connect(&fixture.address, Some(fixture.host.epoch())).await.unwrap();
        let mut read = observer.call("business", "read".into(), Value::Null, Default::default()).await.unwrap();
        read.start().await.unwrap();
        assert!(matches!(read.receive().await.unwrap(), Reply::Finished { result: Ok(ref value), cleanup: Ok(()) } if value["effects"] == 0));
        observer.close().await.unwrap();
        other.close().await.unwrap();
        second_ui.shutdown().await.unwrap();
        drop(second_ui);
        drop(RootOwner::open(&fixture.directory.path().join("second_ui-connections"), &fixture.namespaces).unwrap());
        assert!(Client::retire(&fixture.address, fixture.host.epoch()).await.unwrap());
        fixture.serving.await.unwrap().unwrap();
        registration.remove().unwrap();
    }).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn quitting_an_externally_stopped_host_requires_its_original_root_release() {
    use maka_plugin_core::composition::PresentationTarget;
    use maka_transport::{Configuration, Connections, ExitMode, HostLocation};
    tokio::time::timeout(Duration::from_secs(30), async {
        for replacement in [false, true] {
        let fixture = PeerHost::start().await;
        let registration = fixture.host.publish_registration(&fixture.address, None).unwrap();
        let (ui, source, _) = fixture.ui("external-stop-ui", PresentationTarget::Tui).await;
        let ui_root = fixture.directory.path().join("external-stop-ui");
        let host_root = fixture.directory.path().join("host");
        let original_epoch = fixture.host.epoch();
        let deployment = fixture.host.source().deployment;
        let configuration = Configuration { hosts: vec![maka_transport::Host {
            name: "local".into(),
            location: HostLocation::Public { root: host_root.clone(), state_domain: Some(fixture.host.root_id().into()) },
            deployment: fixture.host.source().deployment,
            endpoints: fixture.selections.clone(), native: vec![],
        }] };
        let mut connections = Connections::prepare(ui.clone(), source.deployment, PresentationTarget::Tui,
            configuration, fixture.namespaces.clone(), std::path::PathBuf::from("unused-local-startup-helper"),
            NativeRegistry::default(), Some(maka_host_js::factory(Default::default()))).await.unwrap();
        connections.start().await.unwrap();
        assert_eq!(fixture.host.status().sessions, 1);
        assert!(Client::retire_with_options(&fixture.address, fixture.host.epoch(), true, true).await.unwrap());
        fixture.serving.await.unwrap().unwrap();
        assert!(fixture.host.status().closed);
        // SQL cleanup and the closed socket do not release retained Root owners.
        ui.shutdown().await.unwrap();
        drop(ui);
        assert_eq!(RootOwner::open(&host_root, &fixture.namespaces).err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
        assert!(connections.close_with(ExitMode::Quit).await.is_err());
        assert_eq!(RootOwner::open(&ui_root, &fixture.namespaces).err().unwrap().kind(), std::io::ErrorKind::WouldBlock);
        // Releasing the actual original owners makes the same close retry safe.
        registration.remove().unwrap();
        drop((fixture.host, fixture.store));
        drop(RootOwner::open(&host_root, &fixture.namespaces).unwrap());
        let replacement = if replacement {
            let store = Arc::new(EventLog::for_root(Arc::new(RootOwner::open(&host_root, &fixture.namespaces).unwrap())).await.unwrap());
            let host = Host::open(store, deployment, NativeRegistry::default(),
                Some(maka_host_js::factory(Default::default())), fixture.selections.clone(), vec![]).await.unwrap();
            assert_ne!(host.epoch(), original_epoch);
            #[cfg(unix)]
            let address = fixture.address.clone();
            #[cfg(windows)]
            let address = std::path::PathBuf::from(format!(r"\\.\pipe\maka-replacement-test-{}", Uuid::new_v4()));
            let listener = LocalListener::bind(&address).unwrap();
            let registration = host.publish_registration(&address, None).unwrap();
            let serving = tokio::spawn({ let host = host.clone(); async move { host.serve_local(listener, CancellationToken::new()).await } });
            Some((host, address, registration, serving))
        } else {
            None
        };
        connections.close_with(ExitMode::Quit).await.unwrap();
        drop(RootOwner::open(&ui_root, &fixture.namespaces).unwrap());
        if let Some((host, address, registration, serving)) = replacement {
            // The old UI's quit may confirm its original owner disappeared, but
            // cannot retire the new incarnation now holding the same Root.
            let mut client = Client::connect(&address, Some(host.epoch())).await.unwrap();
            let mut read = client.call("business", "read".into(), Value::Null, Default::default()).await.unwrap();
            read.start().await.unwrap();
            assert!(matches!(read.receive().await.unwrap(), Reply::Finished { result: Ok(ref value), cleanup: Ok(()) } if value["effects"] == 0));
            assert!(!Client::status(&address, Some(host.epoch())).await.unwrap().closing);
            client.close().await.unwrap();
            assert!(Client::retire(&address, host.epoch()).await.unwrap());
            serving.await.unwrap().unwrap();
            registration.remove().unwrap();
            drop((read, client, host));
            drop(RootOwner::open(&host_root, &fixture.namespaces).unwrap());
        }
        }
    }).await.unwrap();
}

struct NativeCaller;
impl maka_host_runtime::application::NativeOwner for NativeCaller {
    fn prepare_retirement(
        &self,
    ) -> Option<Box<dyn maka_host_runtime::application::NativeRetirement>> {
        Some(Box::new(()))
    }
    fn close(&self) -> futures_util::future::BoxFuture<'static, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}
fn native_caller() -> (
    NativeRegistry,
    tokio::sync::mpsc::UnboundedReceiver<maka_host_runtime::application::NativeServices>,
) {
    let (send, receive) = tokio::sync::mpsc::unbounded_channel();
    let mut native = NativeRegistry::default();
    native
        .insert(
            "test.native-caller".into(),
            Arc::new(move |context, _| {
                send.send(context.services()).unwrap();
                Box::pin(async {
                    maka_host_runtime::application::NativeActivation {
                        exports: Ok(BTreeMap::new()),
                        owner: Arc::new(NativeCaller),
                    }
                })
            }),
        )
        .unwrap();
    (native, receive)
}
async fn native_read(
    services: &maka_host_runtime::application::NativeServices,
) -> Result<Value, String> {
    let request = services
        .request(
            maka_capabilities::call::Identity::Remote {
                request_id: Uuid::new_v4(),
            },
            CancellationToken::new(),
        )
        .unwrap();
    let result = match request.call(
        "business",
        0,
        "read".into(),
        Value::Null,
        Default::default(),
    ) {
        Ok(call) => call.await,
        Err(error) => Err(error),
    };
    request.finish().await.unwrap();
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configured_native_grants_apply_live_survive_source_update_and_root_restart() {
    use maka_host_runtime::application::{host::Configuration, transport::NativeGrant};
    tokio::time::timeout(Duration::from_secs(30), async {
        let directory = tempfile::tempdir().unwrap();
        maka_event_log::root::private_directory(directory.path()).unwrap();
        let namespaces = RootNamespaces {
            ownership: directory.path().join("owners"),
            control: directory.path().join("control"),
        };
        let root = directory.path().join("host");
        let store = Arc::new(
            EventLog::for_root(Arc::new(RootOwner::create(&root, &namespaces).unwrap()))
                .await
                .unwrap(),
        );
        let deployment = Uuid::new_v4();
        let (mut input, mut packages) = support::host();
        let gateway = packages
            .iter_mut()
            .find(|package| package.manifest().id == "local.gateway")
            .unwrap();
        let mut declaration = gateway
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone();
        declaration
            .entries
            .get_mut(&maka_plugin_core::package::EntryTarget::Host)
            .unwrap()
            .implementation = maka_plugin_core::package::Implementation::Native {
            implementation: "test.native-caller".into(),
        };
        *gateway = support::counter::package("local.gateway", declaration, None);
        input.instances.get_mut("gateway").unwrap().package = gateway.application_ref().into();
        let source = store
            .commit_application(deployment, 0, input.clone(), vec![], packages.clone())
            .await
            .unwrap();
        let endpoints = BTreeMap::from([(
            "business".into(),
            HostEndpoint {
                consumer: support::identity(13),
                import: "business".into(),
                grant: Grant {
                    methods: ["read".into()].into(),
                    recovery: false,
                },
            },
        )]);
        let previous = Configuration::PublicHost {
            endpoints: endpoints.clone(),
            native: vec![],
        };
        let desired = Configuration::PublicHost {
            endpoints: endpoints.clone(),
            native: vec![NativeGrant {
                origin: support::identity(13),
                consumer: support::identity(13),
                provider: support::identity(11),
                export: "Counter".into(),
                methods: ["read".into()].into(),
                recovery: false,
            }],
        };
        store
            .select_committed_application_launch(
                source.clone(),
                serde_json::to_value(&previous).unwrap(),
            )
            .await
            .unwrap();
        let (native, mut activations) = native_caller();
        let host = Host::open(
            store.clone(),
            deployment,
            native,
            Some(maka_host_js::factory(Default::default())),
            endpoints,
            vec![],
        )
        .await
        .unwrap();
        #[cfg(unix)]
        let address = directory.path().join("before.sock");
        #[cfg(windows)]
        let address =
            std::path::PathBuf::from(format!(r"\\.\pipe\maka-native-before-{}", Uuid::new_v4()));
        let listener = LocalListener::bind(&address).unwrap();
        let serving = tokio::spawn({
            let host = host.clone();
            async move { host.serve_local(listener, CancellationToken::new()).await }
        });
        let mut client = Client::connect(&address, Some(host.epoch())).await.unwrap();
        let services = activations.recv().await.unwrap();
        assert_eq!(
            native_read(&services).await.unwrap_err(),
            "native service invocation is not granted"
        );
        // A stale source or unrelated previous selection must not publish grants.
        let mut future = source.clone();
        future.revision += 1;
        assert!(
            client
                .configure(future, previous.clone(), desired.clone())
                .await
                .is_err()
        );
        assert!(
            client
                .configure(source.clone(), desired.clone(), desired.clone())
                .await
                .is_err()
        );
        assert_eq!(
            store
                .application_launch()
                .await
                .unwrap()
                .unwrap()
                .configuration,
            serde_json::to_value(&previous).unwrap()
        );
        assert_eq!(
            native_read(&services).await.unwrap_err(),
            "native service invocation is not granted"
        );
        client
            .configure(source.clone(), previous.clone(), desired.clone())
            .await
            .unwrap();
        // Retry the exact command after losing its acknowledgement.
        client
            .configure(source.clone(), previous.clone(), desired.clone())
            .await
            .unwrap();
        assert_eq!(
            native_read(&services).await.unwrap(),
            serde_json::json!({"effects":0})
        );
        let next = store
            .commit_application(deployment, 1, input, vec![], packages)
            .await
            .unwrap();
        assert_eq!(next.revision, 2);
        assert!(
            client
                .configure(source, desired.clone(), previous)
                .await
                .is_err()
        );
        assert_eq!(
            native_read(&services).await.unwrap(),
            serde_json::json!({"effects":0})
        );
        let saved = store.application_launch().await.unwrap().unwrap();
        assert_eq!(saved.configuration, serde_json::to_value(&desired).unwrap());
        assert!(saved.pending.is_none());
        client.close().await.unwrap();
        assert!(Client::retire(&address, host.epoch()).await.unwrap());
        serving.await.unwrap().unwrap();
        drop((client, services, host, store));
        // Reopen from the durable launcher selection, as a new Host process does.
        let store = Arc::new(
            EventLog::for_root(Arc::new(RootOwner::open(&root, &namespaces).unwrap()))
                .await
                .unwrap(),
        );
        let saved = store.application_launch().await.unwrap().unwrap();
        let Configuration::PublicHost {
            endpoints,
            native: grants,
        } = serde_json::from_value(saved.configuration).unwrap();
        let (native, mut activations) = native_caller();
        let host = Host::open(
            store.clone(),
            deployment,
            native,
            Some(maka_host_js::factory(Default::default())),
            endpoints,
            grants,
        )
        .await
        .unwrap();
        #[cfg(unix)]
        let address = directory.path().join("after.sock");
        #[cfg(windows)]
        let address =
            std::path::PathBuf::from(format!(r"\\.\pipe\maka-native-after-{}", Uuid::new_v4()));
        let listener = LocalListener::bind(&address).unwrap();
        let serving = tokio::spawn({
            let host = host.clone();
            async move { host.serve_local(listener, CancellationToken::new()).await }
        });
        let mut client = Client::connect(&address, Some(host.epoch())).await.unwrap();
        assert_eq!(host.source(), &next);
        let services = activations.recv().await.unwrap();
        assert_eq!(
            native_read(&services).await.unwrap(),
            serde_json::json!({"effects":0})
        );
        client.close().await.unwrap();
        assert!(Client::retire(&address, host.epoch()).await.unwrap());
        serving.await.unwrap().unwrap();
        drop((client, services, host, store));
        drop(RootOwner::open(&root, &namespaces).unwrap());
    })
    .await
    .unwrap();
}
