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

//! Imported authoring sources never enter runtime discovery until installed.
use super::*;
use maka_capabilities::call::Attachments;
use maka_capabilities::call::ReadResource;
use maka_capabilities::call::ResourceEncoding;
use maka_skills::publication::{
    self, Tree,
    receipt::{Destination, Operation, Outcome as Published},
};
use management::{Collection, operation, recorded, store_error};

impl management::Managed {
    pub async fn imported_snapshot(
        &self,
        scope: &Scope,
    ) -> Result<maka_skills::DiscoverySnapshot, String> {
        let access = self.imported_read.bind_scope(scope);
        maka_skills::scan(
            &[maka_skills::Source {
                root: access.location(),
                access,
                directory: "skills".into(),
                scope: maka_skills::SkillScope::Custom,
                source: maka_skills::SkillSource::Custom,
                reference_prefix: "imported".into(),
            }],
            &scope.cancellation,
        )
        .await
        .map_err(|_| "skills_discovery_unavailable".into())
    }
}
impl Library {
    pub(super) async fn import_source(
        &self,
        input: ImportSource,
        reference: &maka_plugin_core::operation::RecoveryRef,
        scope: &Scope,
        resources: Attachments,
    ) -> Result<ChangeResult, String> {
        self.import_operation(input, &operation(reference)?, scope, resources)
            .await
    }
    pub(super) async fn import_operation(
        &self,
        input: ImportSource,
        operation: &Operation,
        scope: &Scope,
        resources: Attachments,
    ) -> Result<ChangeResult, String> {
        let Some(managed) = &self.managed else {
            return Ok(ChangeResult::Rejected {
                reason: "management_disabled".into(),
            });
        };
        let _serial = tokio::select! {
            biased;
            _ = scope.cancellation.cancelled() => return Err("skills_cancelled".into()),
            guard = managed.serial.lock() => guard,
        };
        let publisher = managed.publisher(scope, Collection::Imported).await?;
        if let Some(outcome) = publisher.reserve(operation).await.map_err(store_error)? {
            return recorded(outcome);
        }
        publisher.recover().await.map_err(store_error)?;
        if let Some(outcome) = publisher.reserve(operation).await.map_err(store_error)? {
            return recorded(outcome);
        }
        // The original Call retains the complete immutable-resource copy until the
        // worker physically ends, including if the caller stops observing it.
        let mut ticket = scope
            .resources
            .reserve()
            .map_err(|_| "skills_call_closed")?;
        let cancellation = scope.cancellation.clone();
        let material = tokio::task::spawn_blocking(move || {
            ticket.start();
            let result = material(input, &resources, &cancellation);
            ticket.complete(Ok(()));
            result
        })
        .await
        .map_err(|_| "skills_import_worker_failed")?;
        if scope.cancellation.is_cancelled() {
            return Err("skills_cancelled".into());
        }
        let outcome = match material {
            Err(reason) => Published::Rejected {
                reason: reason.into(),
            },
            Ok((id, tree)) => {
                let occupied = managed.imported_snapshot(scope).await?;
                if occupied
                    .inventory
                    .iter()
                    .map(|entry| &entry.location.id)
                    .chain(occupied.rejected.iter().map(|entry| &entry.location.id))
                    .any(|other| other.eq_ignore_ascii_case(&id))
                {
                    Published::Rejected {
                        reason: "source_exists".into(),
                    }
                } else {
                    let destination = Destination::Source { id: id.clone() };
                    match publisher
                        .publish_operation(
                            &id,
                            None,
                            Some(&tree),
                            Some((operation, destination.clone())),
                            &scope.cancellation,
                        )
                        .await
                    {
                        Ok(()) => Published::Applied { destination },
                        Err(publication::Error::Conflict) => Published::Rejected {
                            reason: "source_exists".into(),
                        },
                        Err(error) => return Err(store_error(error)),
                    }
                }
            }
        };
        publisher
            .complete(operation, outcome.clone())
            .await
            .map_err(store_error)?;
        recorded(outcome)
    }
}
fn material(
    input: ImportSource,
    resources: &Attachments,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<(String, Tree), &'static str> {
    let id = resources.get("id").ok_or("invalid_source_id")?;
    let id = read(&**id, ResourceEncoding::Utf8, cancellation).map_err(|_| "invalid_source_id")?;
    let id = std::str::from_utf8(&id)
        .map_err(|_| "invalid_source_id")?
        .trim();
    if !maka_skills::safe_source_id(id) {
        return Err("invalid_source_id");
    }
    let file = resources.get("file").ok_or("invalid_source")?;
    if input.bytes != file.bytes() {
        return Err("invalid_source");
    }
    let bytes = read(&**file, ResourceEncoding::Bytes, cancellation)?;
    if maka_agent_facts::artifact::content_digest(&bytes) != input.digest {
        return Err("invalid_source");
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "invalid_skill")?;
    maka_skills::parse(text).map_err(|_| "invalid_skill")?;
    let mut tree = Tree::empty();
    tree.insert("SKILL.md", bytes)
        .map_err(|_| "invalid_source")?;
    Ok((id.into(), tree))
}
fn read(
    resource: &dyn ReadResource,
    encoding: ResourceEncoding,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<Vec<u8>, &'static str> {
    let expected = match encoding {
        ResourceEncoding::Bytes => maka_plugin_core::contract::binary_resource_type(),
        ResourceEncoding::Utf8 => maka_plugin_core::contract::text_resource_type(),
    };
    let length = resource.bytes();
    if resource.resource_type() != expected || resource.encoding() != encoding || length == 0 {
        return Err("invalid_source");
    }
    let mut bytes = Vec::with_capacity(length);
    for offset in (0..length).step_by(64 * 1024) {
        if cancellation.is_cancelled() {
            return Err("skills_cancelled");
        }
        let end = (offset + 64 * 1024).min(length);
        let chunk = resource.read(offset..end).map_err(|_| "invalid_source")?;
        if chunk.len() != end - offset {
            return Err("invalid_source");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
