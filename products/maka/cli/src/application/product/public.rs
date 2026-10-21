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
//! Product lifecycle operations admitted by the independently owned public Host.
use super::{HostError, Input, Package, RemoteObservation};
use maka_capabilities::call::{Attachments, ReadResource, ResourceEncoding};
use maka_host_runtime::application::{
    host::{Client, LocalCall},
    transport::wire::Reply,
};
use maka_plugin_core::{
    contract::{ResourceType, binary_resource_type, text_resource_type},
    deployment::{
        DeploymentRef,
        management::{
            DeploymentDocument, DeploymentDocumentChunk, ManagedDeployment, ManagementResult,
            PackageBundleExport, deployment_manager_contract,
        },
    },
    operation::{OperationCleanup, OperationOutcome, Receipt},
    package::PackageRef,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, ops::Range, path::PathBuf, sync::Arc};
use uuid::Uuid;

pub(super) struct Host {
    client: Client,
    domain: String,
    deployment: Uuid,
}
impl Host {
    pub(super) async fn connect(
        path: PathBuf,
        domain: String,
        deployment: Uuid,
    ) -> Result<Self, HostError> {
        if maka_event_log::root::resolve(&path)?.root_id() != domain {
            return Err("original product Host Root changed".into());
        }
        let client = crate::deployment::connect_local(path).await?;
        if client
            .catalogue()
            .select("management")?
            .description()
            .contract
            != deployment_manager_contract()
        {
            return Err("product Host management contract changed".into());
        }
        let host = Self {
            client,
            domain,
            deployment,
        };
        host.read().await?;
        Ok(host)
    }
    pub(super) fn into_client(self) -> Client {
        self.client
    }
    fn check_source(&self, source: &DeploymentRef) -> Result<(), HostError> {
        source.validate()?;
        if source.state_domain != self.domain || source.deployment != self.deployment {
            return Err("product management names a foreign Host deployment".into());
        }
        Ok(())
    }
    pub(super) async fn read(&self) -> Result<ManagedDeployment, HostError> {
        let head: ManagedDeployment = serde_json::from_value(self.query("read", json!({})).await?)?;
        self.check_source(&head.source.reference)?;
        Ok(head)
    }
    async fn query(&self, method: &str, input: Value) -> Result<Value, HostError> {
        let mut call = self
            .client
            .call("management", method.into(), input, BTreeMap::new())
            .await?;
        let result: Result<Value, HostError> = async {
            call.start().await?;
            match call.receive().await? {
                Reply::Finished { result, cleanup } => {
                    cleanup?;
                    Ok(result?)
                }
                Reply::Aborted { reason, cleanup } => {
                    cleanup?;
                    Err(reason.into())
                }
                _ => Err("product management query did not finish".into()),
            }
        }
        .await;
        settle(&mut call).await;
        result
    }
    pub(super) async fn document(
        &self,
        source: &DeploymentRef,
    ) -> Result<DeploymentDocument, HostError> {
        self.check_source(source)?;
        let mut text = String::new();
        loop {
            let chunk: DeploymentDocumentChunk = serde_json::from_value(
                self.query("readDocument", json!({"source":source,"offset":text.len()}))
                    .await?,
            )?;
            if chunk.source != *source
                || chunk.offset != text.len()
                || chunk.text.len() > chunk.total_bytes.saturating_sub(text.len())
            {
                return Err("product Host returned an invalid document continuation".into());
            }
            text.push_str(&chunk.text);
            match chunk.next {
                Some(next) if next == text.len() && !chunk.text.is_empty() => {}
                None if text.len() == chunk.total_bytes => break,
                _ => return Err("product Host returned an incomplete document".into()),
            }
        }
        Ok(serde_json::from_str(&text)?)
    }
    pub(super) async fn packages(
        &self,
        source: &DeploymentRef,
        references: &[PackageRef],
    ) -> Result<Vec<Package>, HostError> {
        self.check_source(source)?;
        let mut packages = Vec::new();
        for reference in references {
            let mut call = self
                .client
                .call(
                    "management",
                    "exportPackage".into(),
                    json!({"source":source,"package":reference}),
                    BTreeMap::new(),
                )
                .await?;
            let result = async {
                call.start().await?;
                if !matches!(call.receive().await?, Reply::Subscribed) {
                    return Err(HostError::from("product package export did not subscribe"));
                }
                let mut bytes = Vec::new();
                let mut total = None;
                loop {
                    match call.pull().await? {
                        Reply::Item { value } => {
                            let PackageBundleExport::Chunk {
                                offset,
                                total_bytes,
                                text,
                            } = serde_json::from_value(value)?
                            else {
                                return Err("product package export ended without cleanup".into());
                            };
                            if offset != bytes.len()
                                || total.is_some_and(|total| total != total_bytes)
                                || text.is_empty()
                                || text.len() > total_bytes.saturating_sub(bytes.len())
                            {
                                return Err("product package export continuation changed".into());
                            }
                            total = Some(total_bytes);
                            bytes.extend_from_slice(text.as_bytes());
                        }
                        Reply::Finished { result, cleanup } => {
                            cleanup?;
                            if !matches!(
                                serde_json::from_value::<PackageBundleExport>(result?)?,
                                PackageBundleExport::Complete {}
                            ) || total != Some(bytes.len())
                            {
                                return Err("product package export is incomplete".into());
                            }
                            let package = Package::from_bundle(&bytes)?;
                            if package.application_ref() != *reference {
                                return Err("product package export identity changed".into());
                            }
                            break Ok(package);
                        }
                        _ => return Err("product package export did not finish".into()),
                    }
                }
            }
            .await;
            settle(&mut call).await;
            packages.push(result?);
        }
        Ok(packages)
    }
    pub(super) async fn replace(
        &self,
        expected: u64,
        input: &Input,
        observations: &[RemoteObservation],
        packages: &[Package],
    ) -> Result<(), HostError> {
        let document = DeploymentDocument {
            input: input.clone(),
            remote_exports: observations.to_vec(),
            packages: packages.iter().map(Package::application_ref).collect(),
        };
        let bundles: Vec<Value> = packages
            .iter()
            .map(|package| serde_json::from_slice(&package.to_bundle()))
            .collect::<Result<_, _>>()?;
        let resources: Attachments = BTreeMap::from([
            (
                "document".into(),
                Arc::new(Bytes {
                    bytes: serde_json::to_vec(&document)?,
                    encoding: ResourceEncoding::Utf8,
                }) as Arc<dyn ReadResource>,
            ),
            (
                "packages".into(),
                Arc::new(Bytes {
                    bytes: serde_json::to_vec(&bundles)?,
                    encoding: ResourceEncoding::Bytes,
                }) as Arc<dyn ReadResource>,
            ),
        ]);
        let mut call = self
            .client
            .call(
                "management",
                "replace".into(),
                json!({"expectedRevision":expected}),
                resources,
            )
            .await?;
        let result = async {
            call.start().await?;
            if !matches!(call.receive().await?, Reply::Prepared { .. }) {
                return Err(HostError::from("product replacement was not prepared"));
            }
            let Reply::Mutation { reply, cleanup } = call.confirm().await? else {
                return Err("product replacement did not return a receipt".into());
            };
            cleanup?;
            let Receipt::Recorded {
                outcome: OperationOutcome::Returned { value },
                cleanup: OperationCleanup::Confirmed,
            } = reply.receipt
            else {
                return Err("product replacement outcome remains unconfirmed".into());
            };
            match serde_json::from_value::<ManagementResult<DeploymentRef>>(value)? {
                ManagementResult::Committed { value } => {
                    self.check_source(&value)?;
                    if value.revision != expected + 1 {
                        return Err("product replacement revision changed".into());
                    }
                    Ok(())
                }
                ManagementResult::Conflict { current_revision } => Err(format!(
                    "product replacement conflicts with revision {current_revision}"
                )
                .into()),
                ManagementResult::Rejected { reason, diagnostic } => Err(format!(
                    "product replacement rejected: {reason:?}: {}",
                    diagnostic.unwrap_or_default()
                )
                .into()),
            }
        }
        .await;
        settle(&mut call).await;
        result
    }
    pub(super) async fn ensure_initial(
        &self,
        input: &Input,
        observations: &[RemoteObservation],
        packages: &[Package],
    ) -> Result<(), HostError> {
        let source = DeploymentRef {
            state_domain: self.domain.clone(),
            deployment: self.deployment,
            revision: 1,
        };
        let document = self.document(&source).await?;
        if !same_document(&document, input, observations, packages)? {
            return Err(
                "original product initialization source conflicts with stored revision 1".into(),
            );
        }
        // Read the original immutable bytes as well as their declared digests.
        self.packages(&source, &document.packages).await?;
        Ok(())
    }
    pub(super) async fn ensure_restore(
        &self,
        expected: u64,
        input: &Input,
        observations: &[RemoteObservation],
        packages: &[Package],
        configuration: Option<(
            maka_host_runtime::application::host::Configuration,
            maka_host_runtime::application::host::Configuration,
        )>,
    ) -> Result<(), HostError> {
        let head = self.read().await?;
        let source = head.source.reference.clone();
        let document = DeploymentDocument {
            input: head.source.input,
            remote_exports: head.source.remote_exports,
            packages: head.packages,
        };
        let revision = head.source.reference.revision;
        let matches = same_document(&document, input, observations, packages)?;
        if revision != expected && !(matches && revision == expected + 1) {
            return Err("restoration conflicts with a later source revision".into());
        }
        if let Some((previous, configuration)) = configuration {
            // Authority is a separate durable step coordinated by the original
            // UI preparation. It must precede admitting the new source.
            self.client
                .configure(source, previous, configuration)
                .await?;
        }
        if matches {
            return Ok(());
        }
        self.replace(expected, input, observations, packages).await
    }
}
fn same_document(
    document: &DeploymentDocument,
    input: &Input,
    observations: &[RemoteObservation],
    packages: &[Package],
) -> Result<bool, HostError> {
    let mut actual = document.packages.clone();
    let mut desired: Vec<_> = packages.iter().map(Package::application_ref).collect();
    actual.sort();
    desired.sort();
    Ok(actual == desired
        && serde_json::to_value(&document.input)? == serde_json::to_value(input)?
        && serde_json::to_value(&document.remote_exports)? == serde_json::to_value(observations)?)
}
async fn settle(call: &mut LocalCall) {
    let mut reported = false;
    while !call.cleanup_confirmed() {
        if let Err(error) = call.close().await {
            if !reported {
                eprintln!("product Host call cleanup unconfirmed: {error}");
                reported = true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}
struct Bytes {
    bytes: Vec<u8>,
    encoding: ResourceEncoding,
}
impl ReadResource for Bytes {
    fn encoding(&self) -> ResourceEncoding {
        self.encoding
    }
    fn resource_type(&self) -> ResourceType {
        match self.encoding {
            ResourceEncoding::Utf8 => text_resource_type(),
            ResourceEncoding::Bytes => binary_resource_type(),
        }
    }
    fn bytes(&self) -> usize {
        self.bytes.len()
    }
    fn metadata(&self) -> Value {
        json!({})
    }
    fn read(&self, range: Range<usize>) -> Result<Vec<u8>, String> {
        if self.encoding == ResourceEncoding::Utf8 {
            let text = std::str::from_utf8(&self.bytes).map_err(|error| error.to_string())?;
            text.get(range)
                .map(|text| text.as_bytes().to_vec())
                .ok_or("invalid UTF-8 resource range".into())
        } else {
            self.bytes
                .get(range)
                .map(<[u8]>::to_vec)
                .ok_or("invalid binary resource range".into())
        }
    }
}
