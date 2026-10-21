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

//! Default upgrades and explicit three-way synchronization of customized products.
use super::{HostError, Product, Sources, launch, packages};
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::Args;
use maka_event_log::EventLog;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Baseline {
    pub(super) product: Product,
    /// Exact authoring code and package bytes last compared to this baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) version: Option<String>,
    pub(super) installation: Value,
    /// Standard launcher inputs regenerate defaults; explicit configurations stay explicit.
    pub(super) default_options: Option<Value>,
    pub(super) sources: Sources,
}

#[derive(Args)]
pub(crate) struct Update {
    /// Saved product State Root (default: ~/.maka/application). Close its Host or UI first.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Apply the previewed merge only when there are no conflicts.
    #[arg(long)]
    apply: bool,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Scope {
    #[default]
    Defaults,
    SelectedPackages,
    Initialization,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Outcome {
    pub(super) scope: Scope,
    pub(super) customized: bool,
    pub(super) changed: bool,
    pub(super) applied: bool,
    pub(super) changes: Vec<String>,
    pub(super) conflicts: Vec<String>,
}
impl Update {
    pub(crate) async fn run(self) -> Result<(), HostError> {
        let root = super::root_path(self.root)?;
        let store = if self.apply {
            launch::open(root, false, None).await?
        } else {
            super::super::open_application_inspection(root).await?
        };
        let result = async {
            let record = store
                .application_launch()
                .await?
                .ok_or("product selection missing")?;
            let outcome = if record.configuration.get("kind").is_some() {
                let selection: maka_host_runtime::application::host::Configuration =
                    serde_json::from_value(record.configuration.clone())?;
                selection.validate()?;
                super::headless::update::synchronize(&store, self.apply).await?
            } else {
                let mut outcome = synchronize(&store, true, self.apply).await?;
                if self.apply && outcome.changed && outcome.conflicts.is_empty() {
                    launch::prepare(&store, None, Product::Agent).await?;
                    outcome.applied = true;
                }
                outcome
            };
            println!("{}", serde_json::to_string(&outcome)?);
            if self.apply && !outcome.conflicts.is_empty() {
                return Err("default update has conflicts; no update was written".into());
            }
            Ok(())
        }
        .await;
        store.drain().await;
        result
    }
}

pub(super) async fn begin(store: &EventLog) -> Result<bool, HostError> {
    let outcome = synchronize(store, false, true).await?;
    Ok(outcome.changed && !outcome.customized)
}

async fn synchronize(store: &EventLog, custom: bool, apply: bool) -> Result<Outcome, HostError> {
    let version = tokio::task::spawn_blocking(super::bundle_fingerprint).await??;
    synchronize_version(store, custom, apply, version).await
}

async fn synchronize_version(
    store: &EventLog,
    custom: bool,
    apply: bool,
    version: String,
) -> Result<Outcome, HostError> {
    let record = store
        .application_launch()
        .await?
        .ok_or("product selection missing")?;
    if record.pending.is_some() {
        return Err("finish the prepared product operation before checking for updates".into());
    }
    let original = store.application_product().await?.ok_or(
        "saved UI product has no default-update baseline; update its composition explicitly",
    )?;
    if !custom && original.get("version").and_then(Value::as_str) == Some(version.as_str()) {
        return Ok(Outcome::default());
    }
    let mut baseline: Baseline = serde_json::from_value(original.clone())?;
    let connections = serde_json::from_value(record.configuration.clone())?;
    let (path, domain, host_id, management) = launch::targets(&connections)?;
    let host = super::public::Host::connect(path, domain.clone(), host_id).await?;
    async {
        let mut revisions = BTreeMap::new();
        let head = host.read().await?;
        revisions.insert(host_id, head.source.reference.revision);
        let mut current = vec![head];
        for deployment in [management, record.deployment] {
            let target = store;
            let head = target
                .read_application_metadata(deployment)
                .await?
                .ok_or("product source missing")?;
            revisions.insert(deployment, head.source.reference.revision);
            current.push(head);
        }
        let local = Sources {
            host: current[0].source.input.clone(),
            management_host: current[1].source.input.clone(),
            ui: current[2].source.input.clone(),
            remote_exports: current[2].source.remote_exports.clone(),
            connections: record.configuration.clone(),
        };
        let base = serde_json::to_value(&baseline.sources)?;
        let local_value = serde_json::to_value(&local)?;
        let mut outcome = Outcome {
            customized: local_value != base,
            ..Default::default()
        };
        if outcome.customized && !custom {
            return Ok(outcome);
        }
        if baseline.version.as_ref() == Some(&version) {
            return Ok(outcome);
        }
        let packages = tokio::task::spawn_blocking(packages).await??;
        let authoring = super::authoring()?;
        let version = super::verified_fingerprint(&authoring, &packages, &version)?;
        let configuration = match &baseline.default_options {
            Some(options) => {
                let mut configuration =
                    super::installation_with_source(options.clone(), &authoring).await?;
                // Product identities survive changes to the default feature set.
                // The author allocates identities only for newly introduced instances.
                for key in ["keys", "hostDeployment", "managementDeployment"] {
                    configuration[key] = baseline.installation[key].clone();
                }
                configuration
            }
            None => baseline.installation.clone(),
        };
        let (mut next, installation, _) =
            super::author_with_source(baseline.product, configuration, &packages, &authoring)
                .await?;
        for endpoint in next.connections["hosts"]
            .as_array_mut()
            .ok_or("product connections missing")?
        {
            if endpoint["location"]["kind"] == "public" {
                endpoint["location"]["stateDomain"] = Value::String(domain.clone());
            }
        }
        let next_value = serde_json::to_value(&next)?;
        if next_value == base {
            if apply {
                baseline.version = Some(version);
                store
                    .compare_and_set_application_product(original, serde_json::to_value(&baseline)?)
                    .await?;
            }
            return Ok(outcome);
        }
        outcome.changed = true;
        let merged = merge(
            Some(&base),
            Some(&local_value),
            Some(&next_value),
            "",
            &mut outcome,
        )
        .ok_or("product merge removed its root")?;
        if !outcome.conflicts.is_empty() {
            return Ok(outcome);
        }
        let sources: Sources = serde_json::from_value(merged)?;
        // Neither automatic nor explicit synchronization relocates installation data.
        if launch::targets(&serde_json::from_value(sources.connections.clone())?)?
            != launch::targets(&connections)?
        {
            return Err("default update changed installation identity".into());
        }
        // Unchanged defaults only need source metadata. Read and validate old
        // package bytes when an actual merge needs to retain user-selected code.
        let host_inventory = host
            .packages(&current[0].source.reference, &current[0].packages)
            .await?;
        let host_required: BTreeSet<_> = super::selected(
            &local.host,
            &current[0].source.remote_exports,
            &host_inventory,
        )?
        .iter()
        .map(|package| package.application_ref())
        .collect();
        let mut inventories = vec![(host_inventory, host_required)];
        for head in [&current[1], &current[2]] {
            let loaded = store
                .load_application_at(&head.source.reference)
                .await?
                .ok_or("product source missing")?;
            let required = loaded
                .plan()
                .instances()
                .values()
                .map(|instance| instance.package.clone())
                .collect();
            inventories.push((loaded.packages().to_vec(), required));
        }
        let mut available = BTreeMap::new();
        for package in inventories
            .iter()
            .flat_map(|(inventory, _)| inventory.iter())
            .chain(packages.values())
        {
            available.insert(package.application_ref(), package.clone());
        }
        let available: Vec<_> = available.into_values().collect();
        let mut selected = BTreeMap::new();
        let mut package_sets = super::PackageSets::default();
        // Catalogs belong to each source, even when immutable bytes are shared.
        for ((input, observations, references), (old_inventory, old_required)) in [
            (&sources.host, &[][..], &mut package_sets.host),
            (
                &sources.management_host,
                &[][..],
                &mut package_sets.management_host,
            ),
            (
                &sources.ui,
                sources.remote_exports.as_slice(),
                &mut package_sets.ui,
            ),
        ]
        .into_iter()
        .zip(&inventories)
        {
            let mut inventory: BTreeMap<_, _> = old_inventory
                .iter()
                .filter(|package| !old_required.contains(&package.application_ref()))
                .map(|package| (package.application_ref(), package.clone()))
                .collect();
            for package in super::selected(input, observations, &available)? {
                inventory.insert(package.application_ref(), package);
            }
            let inventory: Vec<_> = inventory.into_values().collect();
            maka_event_log::applications::validate_application(input, observations, &inventory)?;
            *references = inventory
                .iter()
                .map(|package| package.application_ref())
                .collect();
            for package in inventory {
                selected.insert(package.application_ref(), package);
            }
        }
        if selected.len() > 128 {
            return Err("updated product exceeds the prepared package budget".into());
        }
        if apply {
            let preparation = launch::Preparation {
                product: Some(Baseline {
                    product: baseline.product,
                    version: Some(version),
                    installation,
                    default_options: baseline.default_options,
                    sources: next,
                }),
                previous_connections: Some(record.configuration),
                restore: revisions,
                sources,
                package_sets,
                packages: selected
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
        }
        Ok::<_, HostError>(outcome)
    }
    .await
}

/// Objects merge by field. Ordered lists and scalar values remain atomic; an
/// ambiguous edit is reported rather than silently changing user intent.
fn merge(
    base: Option<&Value>,
    local: Option<&Value>,
    next: Option<&Value>,
    path: &str,
    outcome: &mut Outcome,
) -> Option<Value> {
    if local == next || next == base {
        return local.cloned();
    }
    if let (Some(Value::Object(base)), Some(Value::Object(local)), Some(Value::Object(next))) =
        (base, local, next)
    {
        let keys: BTreeSet<_> = base.keys().chain(local.keys()).chain(next.keys()).collect();
        let mut result = serde_json::Map::new();
        for key in keys {
            let escaped = key.replace('~', "~0").replace('/', "~1");
            if let Some(value) = merge(
                base.get(key),
                local.get(key),
                next.get(key),
                &format!("{path}/{escaped}"),
                outcome,
            ) {
                result.insert(key.clone(), value);
            }
        }
        Some(Value::Object(result))
    } else if local == base {
        outcome.changes.push(path.into());
        next.cloned()
    } else {
        outcome.conflicts.push(path.into());
        local.cloned()
    }
}

#[cfg(test)]
mod tests;
