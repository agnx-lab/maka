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

//! Explicit Host sources retain their composition while selected package pins advance.
use super::super::{
    Package, selected,
    update::{Outcome, Scope},
};
use super::{HostError, Input, Preparation, Roots, Selection, launch};
use base64::{Engine, engine::general_purpose::STANDARD};
use maka_event_log::{EventLog, applications::launch::Launch};
use maka_plugin_core::{
    composition::ExecutionLocation,
    package::{PackageRef, PackageSelection},
};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) async fn synchronize(
    store: &EventLog,
    apply: bool,
) -> Result<Outcome, HostError> {
    let record = store
        .application_launch()
        .await?
        .ok_or("public Host selection missing")?;
    let selection: Selection = serde_json::from_value(record.configuration.clone())?;
    selection.validate()?;
    if let Some(pending) = &record.pending {
        let preparation: Preparation = serde_json::from_slice(pending)?;
        let packages = launch::decode_packages(preparation.packages.clone())?;
        validate_target(&preparation.input, &packages)?;
        let previous = check_pending(store, &record, &preparation, &packages).await?;
        let initializing = preparation.expected_revision == 0;
        let mut outcome = Outcome {
            scope: if initializing {
                Scope::Initialization
            } else {
                Scope::SelectedPackages
            },
            changed: true,
            applied: false,
            changes: if initializing {
                Vec::new()
            } else {
                changes(previous.as_ref(), &preparation.input)
            },
            ..Default::default()
        };
        if apply {
            // Recover only this frozen operation, even under a newer executable.
            super::prepare(store, Roots::Saved).await?;
            outcome.applied = true;
        }
        return Ok(outcome);
    }
    let version = tokio::task::spawn_blocking(super::super::bundle_fingerprint).await??;
    let packages = tokio::task::spawn_blocking(super::super::packages).await??;
    let authoring = super::super::authoring()?;
    super::super::verified_fingerprint(&authoring, &packages, &version)?;
    synchronize_packages(store, record, apply, packages).await
}

async fn synchronize_packages(
    store: &EventLog,
    record: Launch,
    apply: bool,
    bundled: BTreeMap<String, Package>,
) -> Result<Outcome, HostError> {
    let loaded = store
        .load_application(record.deployment)
        .await?
        .ok_or("public Host source missing")?;
    if !loaded.source().remote_exports.is_empty() {
        return Err("public Host source cannot use remote observations".into());
    }
    validate_target(&loaded.source().input, loaded.packages())?;
    let mut replacements = BTreeMap::new();
    for package in bundled.values() {
        let reference = package.application_ref();
        if replacements
            .insert(reference.id.clone(), reference)
            .is_some()
        {
            return Err("current bundle contains duplicate package IDs".into());
        }
    }
    let mut next = loaded.source().input.clone();
    for node in next.instances.values_mut() {
        if let PackageSelection::Pinned { package } = &mut node.package {
            replace(package, &replacements);
        }
        for override_ in &mut node.overrides {
            if let Some(package) = &mut override_.package {
                replace(package, &replacements);
            }
        }
    }
    let changes = changes(Some(&loaded.source().input), &next);
    let mut outcome = Outcome {
        scope: Scope::SelectedPackages,
        changed: !changes.is_empty(),
        changes,
        ..Default::default()
    };
    if !outcome.changed {
        return Ok(outcome);
    }
    let mut available = BTreeMap::new();
    for package in loaded.packages().iter().chain(bundled.values()) {
        available.insert(package.application_ref(), package.clone());
    }
    let old_required: BTreeSet<_> = loaded
        .plan()
        .instances()
        .values()
        .map(|instance| instance.package.clone())
        .collect();
    let mut inventory: BTreeMap<_, _> = loaded
        .packages()
        .iter()
        .filter(|package| !old_required.contains(&package.application_ref()))
        .map(|package| (package.application_ref(), package.clone()))
        .collect();
    for package in selected(&next, &[], &available.into_values().collect::<Vec<_>>())? {
        inventory.insert(package.application_ref(), package);
    }
    let packages: Vec<_> = inventory.into_values().collect();
    validate_target(&next, &packages)?;
    if apply {
        let preparation = Preparation {
            expected_revision: loaded.source().reference.revision,
            input: next,
            packages: packages
                .iter()
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
        super::prepare(store, Roots::Saved).await?;
        outcome.applied = true;
    }
    Ok(outcome)
}

pub(super) fn validate_target(input: &Input, packages: &[Package]) -> Result<(), HostError> {
    if input.environment.location != ExecutionLocation::Host {
        return Err("public Host update requires a local Host source".into());
    }
    maka_event_log::applications::validate_application(input, &[], packages)?;
    Ok(())
}

fn replace(package: &mut PackageRef, replacements: &BTreeMap<String, PackageRef>) {
    if let Some(next) = replacements.get(&package.id) {
        *package = next.clone();
    }
}
fn pins(input: &Input) -> BTreeMap<String, &PackageRef> {
    let mut pins = BTreeMap::new();
    for (name, node) in &input.instances {
        let name = name.replace('~', "~0").replace('/', "~1");
        if let PackageSelection::Pinned { package } = &node.package {
            pins.insert(format!("/instances/{name}/package"), package);
        }
        for (index, override_) in node.overrides.iter().enumerate() {
            if let Some(package) = &override_.package {
                pins.insert(
                    format!("/instances/{name}/overrides/{index}/package"),
                    package,
                );
            }
        }
    }
    pins
}
fn changes(previous: Option<&Input>, next: &Input) -> Vec<String> {
    let previous = previous.map(pins).unwrap_or_default();
    pins(next)
        .into_iter()
        .filter_map(|(path, package)| {
            let old = previous.get(&path).copied();
            (old != Some(package)).then(|| {
                let old = old.map_or_else(
                    || "(unselected)".into(),
                    |old| format!("{}@{}", old.id, old.revision),
                );
                format!("{path}: {old} -> {}@{}", package.id, package.revision)
            })
        })
        .collect()
}

async fn check_pending(
    store: &EventLog,
    record: &Launch,
    preparation: &Preparation,
    packages: &[Package],
) -> Result<Option<Input>, HostError> {
    let current = store.load_application(record.deployment).await?;
    if preparation.expected_revision == 0 {
        if let Some(current) = current {
            let mut reference = current.source().reference.clone();
            reference.revision = 1;
            let original = store
                .load_application_at(&reference)
                .await?
                .ok_or("original public Host source missing")?;
            if !matches_target(&original, &preparation.input, packages) {
                return Err(
                    "original product initialization source conflicts with stored revision 1"
                        .into(),
                );
            }
        }
        return Ok(None);
    }
    let current = current.ok_or("public Host source missing")?;
    let revision = current.source().reference.revision;
    if revision != preparation.expected_revision
        && !(revision
            == preparation
                .expected_revision
                .checked_add(1)
                .ok_or("public Host revision exhausted")?
            && matches_target(&current, &preparation.input, packages))
    {
        return Err("prepared Host update conflicts with a later source revision".into());
    }
    let mut reference = current.source().reference.clone();
    reference.revision = preparation.expected_revision;
    let previous = store
        .load_application_at(&reference)
        .await?
        .ok_or("prepared Host update source missing")?;
    Ok(Some(previous.source().input.clone()))
}
fn matches_target(
    loaded: &maka_event_log::applications::LoadedDeployment,
    input: &Input,
    packages: &[Package],
) -> bool {
    let mut actual: Vec<_> = loaded
        .packages()
        .iter()
        .map(Package::application_ref)
        .collect();
    let mut desired: Vec<_> = packages.iter().map(Package::application_ref).collect();
    actual.sort();
    desired.sort();
    actual == desired
        && loaded.source().remote_exports.is_empty()
        && loaded.source().input == *input
}

#[cfg(test)]
mod tests;
