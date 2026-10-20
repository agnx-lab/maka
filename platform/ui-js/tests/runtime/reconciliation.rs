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
use crate::{Declaration, PackageUse, node, package};
use maka_plugin_core::{composition::InstanceId, runtime::Phase};

#[test]
fn committed_ui_addition_retains_original_mount_frame_and_then_replacement_waits_for_ack() {
    let timer = timer();
    let dir = tempfile::tempdir().unwrap();
    let store = store(&timer, dir.path());
    let deployment = Uuid::new_v4();
    let handler = "return frame({...state,text:event.event.text,count:state.count+1,revision:state.revision+1});";
    let (mut input, mut packages) = support::fixture(handler);
    let structural = package(
        "structural.empty",
        Declaration {
            entries: BTreeMap::new(),
            composition: Some(maka_plugin_core::composition::Template {
                parameters: BTreeMap::new(),
                imports: BTreeMap::new(),
                exports: BTreeMap::new(),
                children: BTreeMap::new(),
            }),
        },
        None,
    );
    let mut structural_node = node(&structural, 101);
    structural_node.use_package = PackageUse::Composition;
    input
        .instances
        .insert("structural".into(), structural_node.clone());
    packages.push(structural.clone());
    let original = timer
        .block_on(store.commit_application(deployment, 0, input.clone(), vec![], packages.clone()))
        .unwrap();
    let mut app = start(&timer, store.clone(), deployment);
    let root = app.mount_launch(json!({"count":7})).unwrap();
    app.dispatch(&root, 0, "inc", json!({"text":"你好🙂"}))
        .unwrap();
    let state = app.frame(&root).unwrap().state.clone();
    let recipe = app.frame(&root).unwrap().view.view.clone();
    let original_activation = app
        .deployment_status()
        .instances
        .into_iter()
        .find(|s| s.activation.instance() == &InstanceId(vec![Uuid::from_u128(2)]))
        .unwrap()
        .activation;
    let selected = packages
        .iter()
        .find(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let added = support::package(
        "installed.extra",
        selected
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone(),
        Some(
            std::str::from_utf8(selected.file("index.mjs").unwrap())
                .unwrap()
                .to_owned(),
        ),
    );
    let mut desired = input.clone();
    let mut node = desired.instances["app"].clone();
    node.key = Uuid::from_u128(9);
    node.package = added.application_ref().into();
    desired.instances.insert("extra".into(), node);
    let mut bytes = packages.clone();
    bytes.push(added);
    let stale = app
        .prepare_update(
            timer
                .block_on(app.load_committed_update(original.clone()).unwrap())
                .unwrap(),
        )
        .unwrap();
    let next = timer
        .block_on(store.commit_application(deployment, 1, desired, vec![], bytes))
        .unwrap();
    let load = app.load_committed_update(next.clone()).unwrap();
    // This mutable access must remain possible while the immutable load is pending.
    assert_eq!(app.frame(&root).unwrap().state, state);
    app.capture_actions(&root, |_| unreachable!("no queued capture"))
        .unwrap();
    let update = timer.block_on(load).unwrap();
    let prepared = app.prepare_update(update).unwrap();
    let foreign = app
        .prepare_update(
            timer
                .block_on(app.load_committed_update(next.clone()).unwrap())
                .unwrap(),
        )
        .unwrap();
    let mut other = start(&timer, store.clone(), deployment);
    assert!(other.adopt_prepared(foreign, vec![]).is_err());
    other.shutdown().unwrap();
    drop(other);
    app.adopt_prepared(prepared, vec![]).unwrap();
    app.advance_deployment().unwrap();
    assert_eq!(app.deployment_status().effective, Some(next.clone()));
    assert_eq!(app.frame(&root).unwrap().state, state);
    assert!(Rc::ptr_eq(&app.frame(&root).unwrap().view.view, &recipe));
    assert!(!app.is_retiring(&root));
    assert!(
        app.deployment_status()
            .instances
            .iter()
            .any(|s| s.activation == original_activation && s.phase == Phase::Active)
    );
    assert!(
        timer
            .block_on(app.adopt_committed(original, vec![]))
            .is_err()
    );
    assert!(app.adopt_prepared(stale, vec![]).is_err());
    let (mut input, mut packages) =
        support::fixture("return frame({count:state.count+100,revision:state.revision+1});");
    input.instances.insert("structural".into(), structural_node);
    packages.push(structural);
    input
        .instances
        .get_mut("surface")
        .unwrap()
        .configuration
        .values = BTreeMap::from([("replacement".into(), json!(true))]);
    let latest = timer
        .block_on(store.commit_application(deployment, 2, input, vec![], packages))
        .unwrap();
    timer
        .block_on(app.adopt_committed(latest.clone(), vec![]))
        .unwrap();
    app.advance_deployment().unwrap();
    assert!(app.is_retiring(&root));
    assert_eq!(app.frame(&root).unwrap().state, state);
    assert_eq!(app.deployment_status().effective, Some(next));
    assert!(app.deployment_status().converging);
    assert!(app.mount_launch(json!({})).is_err());
    assert!(
        app.dispatch(&root, 1, "inc", json!({"text":"must reject"}))
            .is_err()
    );
    app.close(&root, Ok(())).unwrap();
    app.advance_deployment().unwrap();
    assert_eq!(app.deployment_status().effective, Some(latest));
    assert_eq!(
        app.live_source_revisions(),
        std::collections::BTreeSet::from([3]),
        "unchanged original structural epoch must not retain consumer authority"
    );
    let fresh = app.mount_launch(json!({"count":7})).unwrap();
    assert_ne!(fresh, root);
    app.dispatch(&fresh, 0, "inc", json!({})).unwrap();
    assert_eq!(app.frame(&fresh).unwrap().state["count"], 107);
    app.close(&fresh, Ok(())).unwrap();
    app.shutdown().unwrap();
    drop(app);
    timer
        .block_on(Arc::try_unwrap(store).ok().unwrap().close())
        .unwrap();
}

#[test]
fn invalid_ui_candidate_does_not_retire_original_product() {
    let timer = timer();
    let dir = tempfile::tempdir().unwrap();
    let store = store(&timer, dir.path());
    let deployment = Uuid::new_v4();
    let (input, packages) =
        support::fixture("return frame({count:state.count+1,revision:state.revision+1});");
    let original = timer
        .block_on(store.commit_application(deployment, 0, input.clone(), vec![], packages.clone()))
        .unwrap();
    let mut app = start(&timer, store.clone(), deployment);
    let root = app.mount_launch(json!({"count":7})).unwrap();
    let old = packages
        .iter()
        .find(|p| p.manifest().id == "dynamic.app")
        .unwrap();
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
        .implementation = maka_plugin_core::package::Implementation::Native {
        implementation: "unavailable.native".into(),
    };
    declaration
        .entries
        .get_mut(&maka_plugin_core::package::EntryTarget::Tui)
        .unwrap()
        .scene = None;
    let invalid = support::package("invalid.app", declaration, None);
    let mut desired = input;
    desired.instances.get_mut("app").unwrap().package = invalid.application_ref().into();
    let mut bytes = packages;
    bytes.push(invalid);
    let invalid = timer
        .block_on(store.commit_application(deployment, 1, desired, vec![], bytes))
        .unwrap();
    assert!(
        timer
            .block_on(app.adopt_committed(invalid, vec![]))
            .is_err()
    );
    assert_eq!(app.source(), Some(&original));
    assert!(!app.is_retiring(&root));
    app.dispatch(&root, 0, "inc", json!({})).unwrap();
    assert_eq!(app.frame(&root).unwrap().state["count"], 8);
    app.close(&root, Ok(())).unwrap();
    app.shutdown().unwrap();
    drop(app);
    timer
        .block_on(Arc::try_unwrap(store).ok().unwrap().close())
        .unwrap();
}

#[test]
fn retained_current_domain_failure_cannot_publish_false_convergence_and_explicit_revision_recovers()
{
    let timer = timer();
    let dir = tempfile::tempdir().unwrap();
    let store = store(&timer, dir.path());
    let deployment = Uuid::new_v4();
    let (input, packages) = support::fixture("throw Error('actual retained callback failed');");
    let original = timer
        .block_on(store.commit_application(deployment, 0, input.clone(), vec![], packages.clone()))
        .unwrap();
    let mut app = start(&timer, store.clone(), deployment);
    let root = app.mount_launch(json!({"count":7})).unwrap();
    let mut desired = input;
    let mut extra = desired.instances["surface"].clone();
    extra.key = Uuid::from_u128(99);
    desired.instances.insert("extra".into(), extra);
    let next = timer
        .block_on(store.commit_application(deployment, 1, desired, vec![], packages))
        .unwrap();
    timer
        .block_on(app.adopt_committed(next.clone(), vec![]))
        .unwrap();
    assert!(!app.is_retiring(&root));
    assert!(app.dispatch(&root, 0, "inc", json!({})).is_err());
    assert!(app.is_retiring(&root));
    app.close(&root, Ok(())).unwrap();
    app.advance_deployment().unwrap();
    let status = app.deployment_status();
    assert!(
        status
            .error
            .unwrap()
            .contains("actual retained callback failed")
    );
    assert!(status.converging);
    assert_eq!(status.desired, Some(next));
    assert_eq!(status.effective, Some(original));
    let (input, packages) =
        support::fixture("return frame({count:state.count+20,revision:state.revision+1});");
    let repaired = timer
        .block_on(store.commit_application(deployment, 2, input, vec![], packages))
        .unwrap();
    timer
        .block_on(app.adopt_committed(repaired.clone(), vec![]))
        .unwrap();
    app.advance_deployment().unwrap();
    assert_eq!(app.deployment_status().effective, Some(repaired));
    assert!(app.deployment_status().error.is_none());
    let fresh = app.mount_launch(json!({"count":7})).unwrap();
    app.dispatch(&fresh, 0, "inc", json!({})).unwrap();
    assert_eq!(app.frame(&fresh).unwrap().state["count"], 27);
    app.close(&fresh, Ok(())).unwrap();
    app.shutdown().unwrap();
    drop(app);
    timer
        .block_on(Arc::try_unwrap(store).ok().unwrap().close())
        .unwrap();
}
