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

use crate::{InvalidDocument, SkillDocument};
use crate::{SkillScope, SkillSource};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use tokio_util::sync::CancellationToken;

pub(crate) mod origin;
pub(crate) mod source;
mod sources;
pub use origin::{Origin, OriginFailure, OriginStatus};
pub use sources::{
    BundledSource, SourceCatalog, SourceCatalogError, governance_catalog, safe_source_id,
    source_catalog,
};

#[derive(Clone)]
pub struct Source {
    pub root: PathBuf,
    pub access: maka_capabilities::filesystem::ReadDirectory,
    pub directory: PathBuf,
    pub scope: SkillScope,
    pub source: SkillSource,
    pub reference_prefix: String,
}

impl Source {
    /// The Host supplies all roots, including home; the library never searches ambient home.
    pub fn standard(
        cwd: Option<&maka_capabilities::filesystem::ReadDirectory>,
        workspace: &maka_capabilities::filesystem::ReadDirectory,
        home: Option<&maka_capabilities::filesystem::ReadDirectory>,
    ) -> Vec<Self> {
        crate::api::LocationId::ALL
            .into_iter()
            .filter_map(|id| {
                let root = match id.scope() {
                    SkillScope::Project => cwd?,
                    SkillScope::User => home?,
                    SkillScope::Workspace => workspace,
                    SkillScope::Custom => unreachable!("standard location"),
                };
                Some(Self::at(
                    root,
                    id.directory(),
                    id.scope(),
                    id.source(),
                    id.reference(),
                ))
            })
            .collect()
    }

    fn at(
        access: &maka_capabilities::filesystem::ReadDirectory,
        directory: &str,
        scope: SkillScope,
        source: SkillSource,
        prefix: &str,
    ) -> Self {
        Self {
            root: access.location(),
            access: access.clone(),
            directory: directory.into(),
            scope,
            source,
            reference_prefix: prefix.into(),
        }
    }

    fn location(&self, id: String, precedence: usize) -> SkillLocation {
        SkillLocation {
            reference: format!("{}:{id}", self.reference_prefix),
            path: self.root.join(&self.directory).join(&id),
            discovery_root: self.root.clone(),
            id,
            scope: self.scope.clone(),
            source: self.source.clone(),
            precedence,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillLocation {
    pub reference: String,
    pub id: String,
    pub path: PathBuf,
    pub discovery_root: PathBuf,
    pub scope: SkillScope,
    pub source: SkillSource,
    pub precedence: usize,
}

#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    pub location: SkillLocation,
    pub document: SkillDocument,
    pub content_sha256: String,
    pub shadowed_by: Option<String>,
    pub resources: crate::Resources,
}

#[derive(Debug, Clone)]
pub struct RejectedSkill {
    pub location: SkillLocation,
    pub document: InvalidDocument,
    pub content_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryFailure {
    BlockedPath,
    ReadFailed,
}

#[derive(Debug, Clone)]
pub struct DiscoveryDiagnostic {
    pub path: PathBuf,
    pub scope: SkillScope,
    pub source: SkillSource,
    pub precedence: usize,
    pub reason: DiscoveryFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanError {
    Cancelled,
    Unavailable,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cancelled => "skill discovery cancelled",
            Self::Unavailable => "skill discovery worker is unavailable",
        })
    }
}
impl std::error::Error for ScanError {}

#[derive(Debug, Default)]
pub struct DiscoverySnapshot {
    pub inventory: Vec<DiscoveredSkill>,
    pub rejected: Vec<RejectedSkill>,
    pub diagnostics: Vec<DiscoveryDiagnostic>,
}

/// Query-only facts; ordinary runtime discovery does not inspect installation locks.
#[derive(Debug, Default)]
pub struct QuerySnapshot {
    pub discovery: DiscoverySnapshot,
    /// Exact bytes belonging to this query's revision; never reopened by pathname.
    pub(crate) contents: BTreeMap<String, Box<[u8]>>,
    pub origins: BTreeMap<String, Origin>,
    pub occupied: BTreeSet<String>,
    pub empty: Vec<SkillLocation>,
}

async fn scan_with_origins(
    sources: &[Source],
    cancellation: &CancellationToken,
) -> Result<QuerySnapshot, ScanError> {
    let mut query = QuerySnapshot::default();
    query.discovery = scan_impl(sources, cancellation, Some(&mut query), false).await?;
    Ok(query)
}

/// Owned blocking I/O; callers must join it before releasing execution ownership.
/// Preferences and capability gating are applied later, against this same snapshot.
pub async fn scan(
    sources: &[Source],
    cancellation: &CancellationToken,
) -> Result<DiscoverySnapshot, ScanError> {
    scan_impl(sources, cancellation, None, false).await
}

/// Include the immutable assets from this build after the selected filesystem sources.
/// Their paths are logical resource names; no filesystem capability is created.
pub async fn scan_with_bundled(
    sources: &[Source],
    cancellation: &CancellationToken,
) -> Result<DiscoverySnapshot, ScanError> {
    scan_impl(sources, cancellation, None, true).await
}

/// Prepare immutable build assets during service activation, before requests
/// need their catalog. No user files or filesystem capabilities are retained.
pub fn prepare_bundled() -> Result<(), ScanError> {
    sources::bundled()
        .map(|_| ())
        .map_err(|_| ScanError::Unavailable)
}

async fn scan_impl(
    sources: &[Source],
    cancellation: &CancellationToken,
    query: Option<&mut QuerySnapshot>,
    bundled: bool,
) -> Result<DiscoverySnapshot, ScanError> {
    let mut state = Scan {
        snapshot: DiscoverySnapshot::default(),
        query: query.as_ref().map(|_| QuerySnapshot::default()),
    };
    for (precedence, source) in sources.iter().enumerate() {
        check_cancelled(cancellation)?;
        let source = source.clone();
        let access = source.access.clone();
        let cancellation = cancellation.clone();
        state = access
            .with_reader(move |reader| {
                state.source(&source, &reader, precedence, &cancellation)?;
                Ok::<_, ScanError>(state)
            })
            .await
            .map_err(|error| match error {
                maka_capabilities::filesystem::ReadError::Retired => ScanError::Cancelled,
                _ => ScanError::Unavailable,
            })??;
    }
    check_cancelled(cancellation)?;
    if bundled {
        for source in sources::bundled().map_err(|_| ScanError::Unavailable)? {
            check_cancelled(cancellation)?;
            let root = PathBuf::from("bundled");
            state.snapshot.inventory.push(DiscoveredSkill {
                location: SkillLocation {
                    reference: format!("bundled:{}", source.id),
                    id: source.id.into(),
                    path: root.join(source.id),
                    discovery_root: root,
                    scope: SkillScope::Custom,
                    source: SkillSource::Maka,
                    precedence: sources.len(),
                },
                document: source.document,
                content_sha256: source.content_sha256,
                shadowed_by: None,
                resources: source.resources.capture(),
            });
        }
    }
    state.snapshot.resolve_precedence();
    if let (Some(destination), Some(updated)) = (query, state.query) {
        *destination = updated;
    }
    Ok(state.snapshot)
}

struct Scan {
    snapshot: DiscoverySnapshot,
    query: Option<QuerySnapshot>,
}
impl Scan {
    fn source(
        &mut self,
        source: &Source,
        reader: &maka_capabilities::filesystem::Reader<'_>,
        precedence: usize,
        cancellation: &CancellationToken,
    ) -> Result<(), ScanError> {
        let Self { snapshot, query } = self;
        let mut captured = match source::Captured::open(source, reader) {
            Ok(captured) => captured,
            Err(reason) => {
                snapshot.diagnostic(
                    source,
                    precedence,
                    source.root.join(&source.directory),
                    reason,
                );
                return Ok(());
            }
        };
        let entries = match captured.entries(cancellation) {
            Ok(entries) => entries,
            Err(source::EntriesError::Scan(error)) => return Err(error),
            Err(source::EntriesError::Read(reason)) => {
                snapshot.diagnostic(
                    source,
                    precedence,
                    source.root.join(&source.directory),
                    reason,
                );
                return Ok(());
            }
        };
        let publication =
            source.scope == SkillScope::Workspace && source.source == SkillSource::Legacy;
        if publication && let Some(query) = query.as_mut() {
            query.occupied.extend(
                entries
                    .iter()
                    .filter_map(|name| name.to_str().map(str::to_lowercase)),
            );
        }
        for name in entries {
            check_cancelled(cancellation)?;
            let path = source.root.join(&source.directory).join(&name);
            let Some(id) = name.to_str() else {
                snapshot.diagnostic(source, precedence, path, DiscoveryFailure::BlockedPath);
                continue;
            };
            let read =
                match captured.read_skill(&name, publication && query.is_some(), cancellation) {
                    Ok(Some(read)) => read,
                    Ok(None) => continue,
                    Err(source::ReadError::Cancelled) => return Err(ScanError::Cancelled),
                    Err(source::ReadError::Failure(reason)) => {
                        snapshot.diagnostic(source, precedence, path, reason);
                        continue;
                    }
                };
            let location = source.location(id.into(), precedence);
            let source::SkillRead::Document { bytes, origin } = read else {
                if publication && let Some(query) = query.as_mut() {
                    query.empty.push(location);
                }
                continue;
            };
            if let Some(origin) = origin
                && let Some(query) = query.as_mut()
            {
                query.origins.insert(location.reference.clone(), origin);
            }
            let reference = location.reference.clone();
            match crate::parse(&String::from_utf8_lossy(&bytes)) {
                Ok(document) => snapshot.inventory.push(DiscoveredSkill {
                    location,
                    document,
                    content_sha256: maka_agent_facts::artifact::content_digest(&bytes),
                    shadowed_by: None,
                    resources: if query.is_none() {
                        crate::Resources::capture(
                            reader,
                            &source.directory.join(&name),
                            cancellation,
                        )?
                    } else {
                        Default::default()
                    },
                }),
                Err(document) => snapshot.rejected.push(RejectedSkill {
                    location,
                    document: *document,
                    content_sha256: maka_agent_facts::artifact::content_digest(&bytes),
                }),
            }
            if let Some(query) = query.as_mut() {
                query.contents.insert(reference, bytes.into_boxed_slice());
            }
        }

        Ok(())
    }
}

impl DiscoverySnapshot {
    fn diagnostic(
        &mut self,
        source: &Source,
        precedence: usize,
        path: PathBuf,
        reason: DiscoveryFailure,
    ) {
        self.diagnostics.push(DiscoveryDiagnostic {
            path,
            scope: source.scope.clone(),
            source: source.source.clone(),
            precedence,
            reason,
        });
    }
}

pub(crate) fn check_cancelled(token: &CancellationToken) -> Result<(), ScanError> {
    if token.is_cancelled() {
        Err(ScanError::Cancelled)
    } else {
        Ok(())
    }
}
