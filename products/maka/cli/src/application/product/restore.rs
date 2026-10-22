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

//! Restore the installation baseline with the existing durable launcher preparation.
use super::{HostError, Input, Package, RemoteObservation, Sources, launch};
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::Args;
use maka_event_log::EventLog;
use maka_plugin_core::deployment::DeploymentRef;
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

#[derive(Args)]
pub(crate) struct Restore {
    /// UI State Root (default: ~/.maka/application). Close its UI first.
    #[arg(long)]
    root: Option<PathBuf>,
}
impl Restore {
    pub(crate) async fn run(self) -> Result<(), HostError> {
        let root = super::root_path(self.root)?;
        let store = launch::open(root, None).await?;
        let result = restore(&store).await;
        store.drain().await;
        println!("{}", serde_json::to_string(&result?)?);
        Ok(())
    }
}
async fn restore(store: &EventLog) -> Result<DeploymentRef, HostError> {
    let record = store
        .application_launch()
        .await?
        .ok_or("root has no saved product")?;
    // An interrupted initialization/restoration must retain its exact preparation.
    if let Some(pending) = &record.pending {
        let preparation: launch::Preparation = serde_json::from_slice(pending)?;
        let restoring = !preparation.restore.is_empty();
        launch::prepare(store).await?;
        if restoring {
            return Ok(store
                .read_application_metadata(record.deployment)
                .await?
                .ok_or("UI source missing")?
                .source
                .reference);
        }
        // Finishing initialization preserves later heads. The requested restore
        // must still replace those heads with the installation baseline below.
    }
    let connections = serde_json::from_value(record.configuration.clone())?;
    let (path, domain, host_id, management) = launch::targets(&connections)?;
    let host = super::public::Host::connect(path, domain, host_id).await?;
    let result = async {
        let mut revisions = BTreeMap::new();
        let current = host.read().await?;
        revisions.insert(host_id, current.source.reference.revision);
        let mut baseline = current.source.reference;
        baseline.revision = 1;
        let original = host.document(&baseline).await?;
        let mut packages = BTreeMap::new();
        for package in host.packages(&baseline, &original.packages).await? {
            packages.insert(package.application_ref(), package);
        }
        let mut originals = vec![original];
        for deployment in [management, record.deployment] {
            let target = store;
            let current = target
                .read_application_metadata(deployment)
                .await?
                .ok_or("product source missing")?;
            let mut baseline = current.source.reference.clone();
            baseline.revision = 1;
            let original = target
                .load_application_at(&baseline)
                .await?
                .ok_or("installation baseline missing")?;
            revisions.insert(deployment, current.source.reference.revision);
            for package in original.packages() {
                packages.insert(package.application_ref(), package.clone());
            }
            originals.push(
                maka_plugin_core::deployment::management::DeploymentDocument {
                    input: original.source().input.clone(),
                    remote_exports: original.source().remote_exports.clone(),
                    packages: original
                        .packages()
                        .iter()
                        .map(Package::application_ref)
                        .collect(),
                },
            );
        }
        let ui = originals.pop().unwrap();
        let management = originals.pop().unwrap();
        let host = originals.pop().unwrap();
        let package_sets = super::PackageSets {
            host: host.packages,
            management_host: management.packages,
            ui: ui.packages,
        };
        let sources = Sources {
            host: host.input,
            management_host: management.input,
            ui: ui.input,
            remote_exports: ui.remote_exports,
            connections: record.configuration,
        };
        let inventory: Vec<_> = packages.values().cloned().collect();
        super::prepare_package_sets(&sources, &package_sets, &inventory)?;
        let preparation = launch::Preparation {
            product: None,
            previous_connections: None,
            restore: revisions,
            sources,
            package_sets,
            packages: packages
                .values()
                .map(|package| {
                    package
                        .files()
                        .iter()
                        .map(|(path, bytes)| (path.clone(), STANDARD.encode(bytes)))
                        .collect()
                })
                .collect(),
        };
        store
            .begin_application_restore(record.deployment, serde_json::to_vec(&preparation)?)
            .await?;
        Ok::<_, HostError>(())
    }
    .await;
    result?;
    launch::prepare(store).await?;
    Ok(store
        .read_application_metadata(record.deployment)
        .await?
        .ok_or("UI source missing")?
        .source
        .reference)
}

pub(super) async fn ensure(
    store: &EventLog,
    deployment: Uuid,
    expected: u64,
    input: &Input,
    observations: &[RemoteObservation],
    packages: &[Package],
) -> Result<(), HostError> {
    let current = store
        .load_application(deployment)
        .await?
        .ok_or("restore source missing")?;
    let mut actual: Vec<_> = current
        .packages()
        .iter()
        .map(Package::application_ref)
        .collect();
    let mut desired: Vec<_> = packages.iter().map(Package::application_ref).collect();
    actual.sort();
    desired.sort();
    let matches = actual == desired
        && serde_json::to_value(&current.source().input)? == serde_json::to_value(input)?
        && serde_json::to_value(&current.source().remote_exports)?
            == serde_json::to_value(observations)?;
    if matches
        && (current.source().reference.revision == expected
            || current.source().reference.revision == expected + 1)
    {
        return Ok(());
    }
    if current.source().reference.revision != expected {
        return Err("restoration conflicts with a later source revision".into());
    }
    store
        .commit_application(
            deployment,
            expected,
            input.clone(),
            observations.to_vec(),
            packages.to_vec(),
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_public::PublishedHost;
    use super::*;
    use maka_event_log::applications::launch::Launch;
    use maka_plugin_core::composition::{Environment, ExecutionLocation};
    use serde_json::json;
    use std::sync::Arc;

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
    fn unused_package(id: &str) -> Package {
        Package::new(BTreeMap::from([(
            "maka.extension.json".into(),
            serde_json::to_vec(&json!({
                "schemaVersion":1,"id":id,
                "application":{"entries":{"host":{
                    "implementation":{"kind":"native","implementation":"example.Component"},
                    "configuration":{"type":"object","additionalProperties":false},
                    "imports":{},"exports":{}
                }}}
            }))
            .unwrap(),
        )]))
        .unwrap()
    }

    #[tokio::test]
    async fn restore_keeps_each_original_source_inventory_without_catalog_leakage() {
        let directory = tempfile::tempdir().unwrap();
        let store = launch::create(directory.path().join("ui")).await.unwrap();
        let host_path = directory.path().join("host");
        let host = Arc::new(launch::create(host_path.clone()).await.unwrap());
        let host_id = Uuid::new_v4();
        let manager = Uuid::new_v4();
        let ui = Uuid::new_v4();
        let root = host.owned_root().unwrap();
        let endpoint = json!({"service":{"consumer":[Uuid::new_v4()],"import":"service","grant":{"methods":["read"],"recovery":true}}});
        let configuration = json!({"hosts":[
            {"name":"local","location":{"kind":"public","root":root.canonical_path(),"stateDomain":root.root_id()},"deployment":host_id,"endpoints":endpoint},
            {"name":"manager","location":{"kind":"cohost"},"deployment":manager,"endpoints":endpoint}
        ]});
        drop(root);
        let originals = [
            unused_package("example.original-host"),
            unused_package("example.original-manager"),
            unused_package("example.original-ui"),
        ];
        let later = unused_package("example.installed-later");
        let mut selected = None;
        for ((target, deployment), original) in
            [(host.as_ref(), host_id), (&store, manager), (&store, ui)]
                .into_iter()
                .zip(&originals)
        {
            target
                .commit_application(deployment, 0, input(), vec![], vec![original.clone()])
                .await
                .unwrap();
            let head = target
                .commit_application(
                    deployment,
                    1,
                    input(),
                    vec![],
                    vec![original.clone(), later.clone()],
                )
                .await
                .unwrap();
            if deployment == ui {
                selected = Some(head);
            }
        }
        store
            .select_committed_application_launch(selected.unwrap(), configuration.clone())
            .await
            .unwrap();
        let serving = PublishedHost::start(host.clone(), host_id).await;
        let restored = restore(&store).await.unwrap();
        let mut actual = Vec::new();
        for (target, deployment) in [(host.as_ref(), host_id), (&store, manager), (&store, ui)] {
            let loaded = target.load_application(deployment).await.unwrap().unwrap();
            assert_eq!(loaded.source().reference.revision, 3);
            actual.push(
                loaded
                    .packages()
                    .iter()
                    .map(Package::application_ref)
                    .collect::<Vec<_>>(),
            );
        }
        let record = store.application_launch().await.unwrap().unwrap();
        serving.stop().await;
        drop(host);
        store.drain().await;
        assert_eq!(restored.deployment, ui);
        assert_eq!(record.configuration, configuration);
        assert!(record.pending.is_none());
        assert_eq!(
            actual,
            originals.map(|package| vec![package.application_ref()])
        );
    }

    #[tokio::test]
    async fn restore_preserves_original_identity_history_and_resumes_without_replay() {
        let directory = tempfile::tempdir().unwrap();
        let store = launch::create(directory.path().join("ui")).await.unwrap();
        let host_path = directory.path().join("host");
        let host = Arc::new(launch::create(host_path.clone()).await.unwrap());
        let ui = Uuid::new_v4();
        let host_id = Uuid::new_v4();
        let manager = Uuid::new_v4();
        let owner = host.owned_root().unwrap();
        let endpoint = json!({"service":{"consumer":[Uuid::new_v4()],"import":"service","grant":{"methods":["read"],"recovery":true}}});
        let configuration = json!({"hosts":[
            {"name":"local","location":{"kind":"public","root":owner.canonical_path(),"stateDomain":owner.root_id()},"deployment":host_id,"endpoints":endpoint},
            {"name":"manager","location":{"kind":"cohost"},"deployment":manager,"endpoints":endpoint}
        ]});
        drop(owner);
        for (target, id) in [(host.as_ref(), host_id), (&store, manager), (&store, ui)] {
            target
                .commit_application(id, 0, input(), vec![], vec![])
                .await
                .unwrap();
            // A different valid environment is sufficient to distinguish desired source.
            let mut changed = input();
            changed.environment.location = ExecutionLocation::Ui;
            changed.environment.presentation =
                Some(maka_plugin_core::composition::PresentationTarget::Tui);
            target
                .commit_application(id, 1, changed, vec![], vec![])
                .await
                .unwrap();
        }
        store
            .begin_application_launch(Launch {
                deployment: ui,
                configuration: configuration.clone(),
                pending: Some(
                    serde_json::to_vec(&launch::Preparation {
                        product: None,
                        previous_connections: None,
                        restore: BTreeMap::new(),
                        package_sets: super::super::PackageSets::default(),
                        sources: Sources {
                            host: input(),
                            management_host: input(),
                            ui: input(),
                            remote_exports: vec![],
                            connections: configuration.clone(),
                        },
                        packages: vec![],
                    })
                    .unwrap(),
                ),
            })
            .await
            .unwrap();
        let serving = PublishedHost::start(host.clone(), host_id).await;
        let restored = restore(&store).await.unwrap();
        assert_eq!(restored.deployment, ui);
        assert_eq!(restored.revision, 3);
        let record = store.application_launch().await.unwrap().unwrap();
        assert_eq!(record.configuration, configuration);
        assert!(record.pending.is_none());
        let old = store
            .load_application_at(&DeploymentRef {
                revision: 2,
                ..restored.clone()
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            old.source().input.environment.location,
            ExecutionLocation::Ui
        );
        assert_eq!(restore(&store).await.unwrap(), restored); // Already restored: no new revision.

        let preparation = launch::Preparation {
            product: None,
            previous_connections: None,
            restore: BTreeMap::from([(host_id, 3), (manager, 3), (ui, 3)]),
            package_sets: super::super::PackageSets::default(),
            sources: Sources {
                host: input(),
                management_host: input(),
                ui: input(),
                remote_exports: vec![],
                connections: configuration,
            },
            packages: vec![],
        };
        store
            .begin_application_restore(ui, serde_json::to_vec(&preparation).unwrap())
            .await
            .unwrap();
        // Simulate a committed first target whose acknowledgement was lost.
        host.commit_application(host_id, 3, input(), vec![], vec![])
            .await
            .unwrap();
        assert_eq!(restore(&store).await.unwrap(), restored);
        assert_eq!(
            host.read_application_metadata(host_id)
                .await
                .unwrap()
                .unwrap()
                .source
                .reference
                .revision,
            4
        );
        let public = super::super::public::Host::connect(
            host_path,
            host.owned_root().unwrap().root_id().into(),
            host_id,
        )
        .await
        .unwrap();
        assert!(
            public
                .ensure_restore(2, &input(), &[], &[], None)
                .await
                .is_err()
        );
        drop(public);
        // A pending launcher restore must survive rejection of its first source.
        let mut conflicting = preparation;
        conflicting.restore.insert(host_id, 2);
        store
            .begin_application_restore(ui, serde_json::to_vec(&conflicting).unwrap())
            .await
            .unwrap();
        let error = restore(&store).await.unwrap_err();
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
        assert_eq!(
            store
                .read_application_metadata(ui)
                .await
                .unwrap()
                .unwrap()
                .source
                .reference,
            restored
        );
        assert_eq!(
            host.read_application_metadata(host_id)
                .await
                .unwrap()
                .unwrap()
                .source
                .reference
                .revision,
            4
        );
        serving.stop().await;
        drop(host);
        store.drain().await;
    }
}
