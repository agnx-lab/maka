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

use super::super::test_public::PublishedHost;
use super::*;
use maka_event_log::applications::launch::Launch as Record;
use maka_plugin_core::composition::{Environment, ExecutionLocation, PresentationTarget};
use serde_json::json;

fn input() -> Input {
    Input {
        environment: Environment {
            location: ExecutionLocation::Host,
            presentation: None,
        },
        instances: BTreeMap::new(),
        launch: None,
        recovery: None,
    }
}
async fn pending(store: &EventLog, host: &EventLog) -> (Uuid, Uuid, Uuid) {
    let deployment = Uuid::new_v4();
    let host_id = Uuid::new_v4();
    let management = Uuid::new_v4();
    let endpoint = json!({"service":{"consumer":[Uuid::new_v4()],"import":"service","grant":{"methods":["read"],"recovery":true}}});
    let owner = host.owned_root().unwrap();
    let connections = json!({"hosts":[
        {"name":"local","location":{"kind":"public","root":owner.canonical_path(),"stateDomain":owner.root_id()},"deployment":host_id,"endpoints":endpoint},
        {"name":"manager","location":{"kind":"cohost"},"deployment":management,"endpoints":endpoint}
    ]});
    let selection = connections.clone();
    let preparation = Preparation {
        product: None,
        previous_connections: None,
        restore: BTreeMap::new(),
        package_sets: super::super::PackageSets::default(),
        sources: Sources {
            host: input(),
            management_host: input(),
            ui: input(),
            remote_exports: vec![],
            connections,
        },
        packages: vec![],
    };
    store
        .begin_application_launch(Record {
            deployment,
            configuration: selection,
            pending: Some(serde_json::to_vec(&preparation).unwrap()),
        })
        .await
        .unwrap();
    (deployment, host_id, management)
}

#[tokio::test]
async fn partial_initialization_resumes_original_inputs_and_preserves_new_heads() {
    let directory = tempfile::tempdir().unwrap();
    let ui_path = directory.path().join("ui");
    let host_path = directory.path().join("host");
    let store = open(ui_path.clone(), true, None).await.unwrap();
    let host = Arc::new(open(host_path.clone(), true, None).await.unwrap());
    let (ui, host_id, management) = pending(&store, &host).await;
    ensure(&host, host_id, &input(), &[], &[]).await.unwrap();
    let mut edited = input();
    edited.environment = Environment {
        location: ExecutionLocation::Ui,
        presentation: Some(PresentationTarget::Tui),
    };
    host.commit_application(host_id, 1, edited, vec![], vec![])
        .await
        .unwrap();
    ensure(&store, management, &input(), &[], &[])
        .await
        .unwrap();
    ensure(&store, ui, &input(), &[], &[]).await.unwrap();
    let mut newer_ui = input();
    newer_ui.environment = Environment {
        location: ExecutionLocation::Ui,
        presentation: Some(PresentationTarget::Tui),
    };
    store
        .commit_application(ui, 1, newer_ui.clone(), vec![], vec![])
        .await
        .unwrap();
    let serving = PublishedHost::start(host.clone(), host_id).await;
    store.drain().await;
    drop(store);
    let store = open(ui_path.clone(), false, None).await.unwrap();
    let (selected, _, clients) = prepare(&store).await.unwrap();
    selected.check_store(&store).unwrap();
    assert_eq!(selected.source().reference.deployment, ui);
    assert_eq!(selected.source().reference.revision, 2);
    assert_eq!(selected.source().input, newer_ui);
    assert!(
        store
            .application_launch()
            .await
            .unwrap()
            .unwrap()
            .pending
            .is_none()
    );
    assert_eq!(
        store
            .load_application(ui)
            .await
            .unwrap()
            .unwrap()
            .source()
            .reference
            .revision,
        2
    );
    assert_eq!(
        store
            .load_application(management)
            .await
            .unwrap()
            .unwrap()
            .source()
            .reference
            .revision,
        1
    );
    assert_eq!(
        host.load_application(host_id)
            .await
            .unwrap()
            .unwrap()
            .source()
            .reference
            .revision,
        2
    );
    drop(clients);
    store
        .commit_application(ui, 2, input(), vec![], vec![])
        .await
        .unwrap();
    let (current, _, _) = prepare(&store).await.unwrap();
    assert_eq!(current.source().reference.revision, 3);
    assert_eq!(
        store
            .load_application(ui)
            .await
            .unwrap()
            .unwrap()
            .source()
            .reference
            .revision,
        3
    );
    serving.stop().await;
    drop(host);
    store.drain().await;
}

#[tokio::test]
async fn conflicting_original_commit_keeps_pending_record_and_does_not_create_ui() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(directory.path().join("ui"), true, None).await.unwrap();
    let host = Arc::new(
        open(directory.path().join("host"), true, None)
            .await
            .unwrap(),
    );
    let (ui, host_id, _) = pending(&store, &host).await;
    let mut conflicting = input();
    conflicting.environment = Environment {
        location: ExecutionLocation::Ui,
        presentation: Some(PresentationTarget::Tui),
    };
    host.commit_application(host_id, 0, conflicting, vec![], vec![])
        .await
        .unwrap();
    let serving = PublishedHost::start(host.clone(), host_id).await;
    let error = prepare(&store).await.err().unwrap();
    assert!(error.to_string().contains("conflicts"), "{error}");
    assert!(
        store
            .application_launch()
            .await
            .unwrap()
            .unwrap()
            .pending
            .is_some()
    );
    assert!(store.load_application(ui).await.unwrap().is_none());
    serving.stop().await;
    drop(host);
    store.drain().await;
}

#[tokio::test]
async fn replacement_host_root_cannot_resume_original_initialization() {
    let directory = tempfile::tempdir().unwrap();
    let host_path = directory.path().join("host");
    let store = open(directory.path().join("ui"), true, None).await.unwrap();
    let host = open(host_path.clone(), true, None).await.unwrap();
    let (ui, _, _) = pending(&store, &host).await;
    host.drain().await;
    drop(host);
    // Remove only this disposable test Root, then create a genuinely different identity.
    std::fs::remove_dir_all(&host_path).unwrap();
    drop(RootOwner::create(&host_path, &RootNamespaces::for_current_account().unwrap()).unwrap());
    let database = host_path.join(maka_event_log::root::ROOT_DATABASE);
    assert!(!database.exists());
    let error = prepare(&store).await.err().unwrap();
    assert!(error.to_string().contains("Root changed"), "{error}");
    assert!(
        !database.exists(),
        "a foreign Root must be rejected before opening or recovering its store"
    );
    assert!(store.load_application(ui).await.unwrap().is_none());
    assert!(
        store
            .application_launch()
            .await
            .unwrap()
            .unwrap()
            .pending
            .is_some()
    );
    store.drain().await;
}

#[test]
fn explicit_source_package_sets_keep_catalogs_and_reject_missing_or_duplicate_refs() {
    let package = Package::new(BTreeMap::from([(
        "maka.extension.json".into(),
        serde_json::to_vec(&json!({
            "schemaVersion":1,"id":"example.installed",
            "application":{"entries":{"host":{
                "implementation":{"kind":"native","implementation":"example.Component"},
                "configuration":{"type":"object","additionalProperties":false},
                "imports":{},"exports":{}
            }}}
        }))
        .unwrap(),
    )]))
    .unwrap();
    let sources = Sources {
        host: input(),
        management_host: input(),
        ui: input(),
        remote_exports: vec![],
        connections: json!({}),
    };
    let sets = super::super::PackageSets {
        host: vec![package.application_ref()],
        management_host: vec![],
        ui: vec![],
    };
    let applications =
        super::super::prepare_package_sets(&sources, &sets, std::slice::from_ref(&package))
            .unwrap();
    assert_eq!(
        applications[0]
            .packages()
            .iter()
            .map(Package::application_ref)
            .collect::<Vec<_>>(),
        vec![package.application_ref()]
    );
    assert!(applications[1].packages().is_empty());
    assert!(applications[2].packages().is_empty());
    assert!(
        applications[0].input().instances.is_empty(),
        "installed inventory need not execute"
    );

    let mut missing = super::super::PackageSets::from_applications(&applications);
    let mut unavailable = package.application_ref();
    unavailable.id = "example.missing".into();
    missing.management_host.push(unavailable);
    let error =
        super::super::prepare_package_sets(&sources, &missing, std::slice::from_ref(&package))
            .err()
            .unwrap();
    assert_eq!(
        error.to_string(),
        "prepared package set refers to missing bytes"
    );

    let mut duplicated = super::super::PackageSets::from_applications(&applications);
    duplicated.host.push(package.application_ref());
    let error =
        super::super::prepare_package_sets(&sources, &duplicated, std::slice::from_ref(&package))
            .err()
            .unwrap();
    assert_eq!(error.to_string(), "duplicate prepared package reference");
    let error = super::super::prepare_package_sets(&sources, &sets, &[package.clone(), package])
        .err()
        .unwrap();
    assert_eq!(error.to_string(), "duplicate prepared package bytes");
}
