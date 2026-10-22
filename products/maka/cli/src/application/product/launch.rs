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

//! Durable product recovery and shared Host installation operations.
use super::Sources;
use crate::application::HostError;
use base64::{Engine, engine::general_purpose::STANDARD};
use maka_event_log::{
    EventLog,
    applications::{LoadedDeployment, PreparedApplication},
    root::{RootNamespaces, RootOwner},
};
use maka_packages::Package;
use maka_plugin_core::{
    composition::Input,
    deployment::{DeploymentRef, RemoteObservation},
};
use maka_transport::{Configuration, HostLocation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preparation {
    #[serde(default)]
    pub(super) product: Option<super::update::Baseline>,
    #[serde(default)]
    pub(super) previous_connections: Option<Value>,
    #[serde(default)]
    pub(super) restore: BTreeMap<Uuid, u64>,
    pub(super) sources: Sources,
    pub(super) package_sets: super::PackageSets,
    // Exact immutable files, independent of whatever binary resumes this work.
    pub(super) packages: Vec<BTreeMap<String, String>>,
}

struct PreparedLaunch {
    deployment: Uuid,
    configuration: Value,
    preparation: Preparation,
    applications: [PreparedApplication; 3],
}

pub(super) async fn open(
    root: PathBuf,
    initialize: bool,
    expected_domain: Option<String>,
) -> Result<EventLog, HostError> {
    let owner = tokio::task::spawn_blocking(move || {
        let namespaces = RootNamespaces::for_current_account()?;
        if initialize {
            maka_event_log::root::initialize(&root, &namespaces)?;
        }
        let owner = RootOwner::open(&root, &namespaces)?;
        if expected_domain
            .as_deref()
            .is_some_and(|domain| owner.root_id() != domain)
        {
            return Err(std::io::Error::other("original product Host Root changed"));
        }
        Ok(owner)
    })
    .await??;
    Ok(maka_transport::open_store(Arc::new(owner)).await?)
}
pub(super) fn targets(
    configuration: &Configuration,
) -> Result<(PathBuf, String, Uuid, Uuid), HostError> {
    configuration.validate().map_err(HostError::from)?;
    let mut public = None;
    let mut management = None;
    for host in &configuration.hosts {
        match &host.location {
            HostLocation::Public { root, state_domain } if public.is_none() => {
                public = Some((
                    root.clone(),
                    state_domain
                        .clone()
                        .ok_or("product Host Root identity missing")?,
                    host.deployment,
                ))
            }
            HostLocation::Cohost {} if management.is_none() => management = Some(host.deployment),
            _ => return Err("product launch requires one public Host and one cohost".into()),
        }
    }
    let (path, domain, host) = public.ok_or("product Host is missing")?;
    Ok((
        path,
        domain,
        host,
        management.ok_or("product management Host is missing")?,
    ))
}
pub(super) async fn prepare(
    store: &EventLog,
) -> Result<
    (
        LoadedDeployment,
        Configuration,
        Vec<maka_host_runtime::application::host::Client>,
    ),
    HostError,
> {
    let record = store
        .application_launch()
        .await?
        .ok_or("product selection missing")?;
    let connections: Configuration = serde_json::from_value(record.configuration.clone())?;
    let (root, domain, host, _) = targets(&connections)?;
    if let Some(bytes) = record.pending {
        let mut preparation: Preparation = serde_json::from_slice(&bytes)?;
        let packages = decode_packages(std::mem::take(&mut preparation.packages))?;
        let applications = super::prepare_package_sets(
            &preparation.sources,
            &preparation.package_sets,
            &packages,
        )?;
        return finish(
            store,
            PreparedLaunch {
                deployment: record.deployment,
                configuration: record.configuration,
                preparation,
                applications,
            },
        )
        .await;
    }
    // These independent Roots can load together; wait for both so an error does
    // not cancel the other owner's in-flight validation or connection cleanup.
    let (public, loaded) = tokio::join!(
        super::public::Host::connect(root, domain, host),
        store.load_application(record.deployment),
    );
    let public = public?;
    let loaded = loaded?.ok_or("UI source missing")?;
    Ok((loaded, connections, vec![public.into_client()]))
}

async fn finish(
    store: &EventLog,
    prepared: PreparedLaunch,
) -> Result<
    (
        LoadedDeployment,
        Configuration,
        Vec<maka_host_runtime::application::host::Client>,
    ),
    HostError,
> {
    let PreparedLaunch {
        deployment,
        configuration,
        preparation,
        applications: [host_source, management_source, ui_source],
    } = prepared;
    if preparation
        .previous_connections
        .as_ref()
        .unwrap_or(&preparation.sources.connections)
        != &configuration
    {
        return Err("original launcher connection input changed".into());
    }
    let mut connections: Configuration = serde_json::from_value(configuration)?;
    let (root, domain, host, management) = targets(&connections)?;
    let host_input = host_source.input().clone();
    let host_packages = host_source.packages().to_vec();
    let bootstrap_verified = if preparation.restore.is_empty() {
        bootstrap_host(&root, &domain, host, &connections, host_source).await?
    } else {
        false
    };
    let public = super::public::Host::connect(root.clone(), domain.clone(), host).await?;
    let result = async {
        if preparation.restore.is_empty() {
            if !bootstrap_verified {
                public
                    .ensure_initial(&host_input, &[], &host_packages)
                    .await?;
            }
        } else {
            let previous = host_configuration(&connections)?;
            let desired = host_configuration(&serde_json::from_value(
                preparation.sources.connections.clone(),
            )?)?;
            let configuration =
                if serde_json::to_value(&previous)? == serde_json::to_value(&desired)? {
                    None
                } else {
                    Some((previous, desired))
                };
            public
                .ensure_restore(
                    *preparation
                        .restore
                        .get(&host)
                        .ok_or("restore revision missing")?,
                    &host_input,
                    &[],
                    &host_packages,
                    configuration,
                )
                .await?;
        }
        let loaded = if preparation.restore.is_empty() {
            ensure_prepared(store, management, management_source).await?;
            ensure_prepared(store, deployment, ui_source).await?
        } else {
            for (deployment, source) in [(management, management_source), (deployment, ui_source)] {
                super::restore::ensure(
                    store,
                    deployment,
                    *preparation
                        .restore
                        .get(&deployment)
                        .ok_or("restore revision missing")?,
                    source.input(),
                    source.remote_exports(),
                    source.packages(),
                )
                .await?;
            }
            store
                .load_application(deployment)
                .await?
                .ok_or("UI source missing")?
        };
        if let Some(baseline) = preparation.product {
            store
                .complete_application_product(
                    deployment,
                    preparation.sources.connections.clone(),
                    serde_json::to_value(baseline)?,
                )
                .await?;
            connections = serde_json::from_value(preparation.sources.connections.clone())?;
        } else {
            store.complete_application_launch(deployment).await?;
        }
        Ok::<_, HostError>(loaded)
    }
    .await;
    let loaded = result?;
    Ok((loaded, connections, vec![public.into_client()]))
}
fn host_configuration(
    configuration: &Configuration,
) -> Result<maka_host_runtime::application::host::Configuration, HostError> {
    let connection = configuration
        .hosts
        .iter()
        .find(|connection| matches!(&connection.location, HostLocation::Public { .. }))
        .ok_or("product public Host missing")?;
    let configuration = maka_host_runtime::application::host::Configuration::PublicHost {
        endpoints: connection.endpoints.clone(),
        native: connection.native.clone(),
    };
    configuration.validate()?;
    Ok(configuration)
}

/// True only after the original source, package bytes and selection are confirmed.
/// A busy Root has supplied no such proof; its live Host must still be checked.
async fn bootstrap_host(
    root: &std::path::Path,
    domain: &str,
    deployment: Uuid,
    connections: &Configuration,
    prepared: PreparedApplication,
) -> Result<bool, HostError> {
    let host_store = match open(root.into(), false, Some(domain.into())).await {
        Ok(store) => store,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock) =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    let connection = connections
        .hosts
        .iter()
        .find(|connection| matches!(&connection.location, HostLocation::Public { .. }))
        .ok_or("product public Host missing")?;
    let result = super::headless::save_product(
        &host_store,
        deployment,
        prepared,
        connection.endpoints.clone(),
        connection.native.clone(),
    )
    .await;
    // Public startup can acquire Root only after bootstrap has actually drained.
    host_store.drain().await;
    drop(host_store);
    result.map(|()| true)
}

pub(super) fn decode_packages(
    files: Vec<BTreeMap<String, String>>,
) -> Result<Vec<Package>, HostError> {
    files
        .into_iter()
        .map(|files| {
            let files = files
                .into_iter()
                .map(|(name, encoded)| {
                    let bytes = STANDARD.decode(encoded)?;
                    Ok::<_, HostError>((name, bytes))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            Ok(Package::new(files)?)
        })
        .collect()
}
pub(super) async fn ensure(
    store: &EventLog,
    deployment: Uuid,
    input: &Input,
    observations: &[RemoteObservation],
    packages: &[Package],
) -> Result<(), HostError> {
    ensure_prepared(
        store,
        deployment,
        PreparedApplication::new(input.clone(), observations.to_vec(), packages.to_vec())?,
    )
    .await
    .map(|_| ())
}

pub(super) async fn ensure_prepared(
    store: &EventLog,
    deployment: Uuid,
    prepared: PreparedApplication,
) -> Result<LoadedDeployment, HostError> {
    let input = prepared.input();
    let observations = prepared.remote_exports();
    let packages = prepared.packages();
    let reference = DeploymentRef {
        state_domain: store.owned_root()?.root_id().into(),
        deployment,
        revision: 1,
    };
    if let Some(original) = store.load_application_at(&reference).await? {
        let mut actual: Vec<_> = original
            .packages()
            .iter()
            .map(Package::application_ref)
            .collect();
        let mut expected: Vec<_> = packages.iter().map(Package::application_ref).collect();
        actual.sort();
        expected.sort();
        if serde_json::to_value(&original.source().input)? != serde_json::to_value(input)?
            || serde_json::to_value(&original.source().remote_exports)?
                != serde_json::to_value(observations)?
            || actual != expected
        {
            return Err(
                "original product initialization source conflicts with stored revision 1".into(),
            );
        }
        // Initialization compares revision one, but execution selects the current
        // head. Reuse the original only while it remains the observed head.
        let head = store
            .read_application_metadata(deployment)
            .await?
            .ok_or("UI source missing")?;
        if head.source.reference == original.source().reference {
            return Ok(original);
        }
        store
            .load_application_at(&head.source.reference)
            .await?
            .ok_or_else(|| "UI source missing".into())
    } else {
        Ok(store
            .commit_prepared_application(deployment, 0, prepared)
            .await?)
    }
}

#[cfg(test)]
mod tests;
