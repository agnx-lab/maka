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

//! Separate prototype database; does not open or migrate old Maka State Roots.

mod append;
mod application_inspection;
pub mod applications;
pub mod archive;
pub mod artifacts;
pub mod bundle;
mod catalog_changes;
pub mod connection;
pub mod context;
mod continuation;
pub mod diagnostics;
pub mod effects;
mod executor;
mod handoff;
pub use handoff::PendingHandoff;
pub mod composition;
pub mod interactions;
pub mod message_admissions;
mod message_identity;
pub mod message_interrupts;
pub mod message_queue;
pub mod message_resolution;
pub mod message_sources;
mod model_items;
pub mod observation;
pub mod plugins;
mod prefix;
pub mod projects;
mod read_resources;
pub mod recovery;
pub mod root;
pub mod run_source;
mod schema;
pub mod sessions;
pub mod shell_runs;
mod sqlite_functions;
pub mod steering;
mod steering_delivery;
pub mod title;
mod tool_calls;
mod tool_payloads;
pub mod transcript;
pub mod turns;
pub mod usage;
pub mod workspace;

use std::path::Path;
use std::sync::Arc;

use connection::{ConnectionAuthority, OwnedConnection};
use sqlx::sqlite::SqliteConnectOptions;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(
        "Resource prefix is ambiguous in this Session; use a longer prefix or the full reference"
    )]
    AmbiguousReadResource,
    #[error(transparent)]
    Project(#[from] projects::ProjectError),
    #[error(transparent)]
    Archive(#[from] archive::ArchiveError),
    #[error("event log already has a writer")]
    WriterBusy,
    #[error("not a supported Rust prototype event log; old Maka databases are not migrated")]
    UnsupportedDatabase,
    #[error("invocation is sealed")]
    Sealed,
    #[error("imported invocation is historical evidence, not locally accepted work")]
    ImportedInvocation,
    #[error("event identity conflicts with committed content")]
    EventConflict,
    #[error("session create request conflicts with committed configuration")]
    SessionConflict,
    #[error("artifact identity conflicts with committed metadata or payload")]
    ArtifactConflict,
    #[error("artifact chunk offset is out of range")]
    ArtifactOffset,
    #[error("shell resource identity already exists")]
    ShellConflict,
    #[error("shell resource not found in this session")]
    ShellNotFound,
    #[error("session not found")]
    SessionNotFound,
    #[error("session retired")]
    SessionRetired,
    #[error("permission grant is no longer current")]
    PermissionRevoked,
    #[error("session has an unsealed invocation")]
    SessionBusy,
    #[error("invalid event transition: source Turn does not exist")]
    HistoryTurnMissing,
    #[error("invalid event transition: history cut splits a logical Turn")]
    HistoryTurnSplit,
    #[error("invalid event transition: Session contains unsealed or unresolved prior execution")]
    HistoryUnsettled,
    #[error("invalid event transition: Session contains an unresolved prior provider tool effect")]
    HistoryProviderEffectUnresolved,
    #[error("invalid event transition: managed Session history requires its owner's lifecycle")]
    ManagedSession,
    #[error("catalog revision changed from {expected} to {actual}")]
    RevisionConflict { expected: String, actual: String },
    #[error("invalid event transition: {0}")]
    InvalidTransition(String),
    #[error("prefix exceeds the requested event or byte limit")]
    PrefixTooLarge,
    #[error("conversation material in this scope was collected after Session removal")]
    MaterialCollected,
    #[error("derived transcript identity conflicts with its committed bytes")]
    TranscriptConflict,
    #[error(transparent)]
    Projection(#[from] maka_presentation::ProjectionError),
    #[error("event log mutex poisoned")]
    Poisoned,
    #[error("event commit outcome unknown: {0}")]
    CommitUnknown(sqlx::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("database initialization worker failed")]
    Initialization,
    #[error("database connection closed before accepting the operation")]
    ConnectionClosed,
    #[error("accepted database operation outcome is unknown")]
    OperationUnknown,
    #[error("database close failed: {0}")]
    CloseFailed(String),
    #[error("database startup failed: {cause}; cleanup unconfirmed: {cleanup}")]
    StartupCleanupFailed {
        cause: Box<StoreError>,
        cleanup: Box<StoreError>,
    },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub struct EventLog {
    context_cache: Arc<context::cache::Cache>,
    // The independent owner drains SQLx and closes it before releasing its lease.
    connection: OwnedConnection,
    root_owner: Option<Arc<root::RootOwner>>,
    commits: tokio::sync::watch::Sender<u64>,
    shell_changes: tokio::sync::broadcast::Sender<shell_runs::ShellChange>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OpenPurpose {
    Execution,
    ApplicationInspection,
}

impl EventLog {
    pub async fn for_root(owner: Arc<root::RootOwner>) -> Result<Self, StoreError> {
        owner.validate_current()?;
        let path = owner.canonical_path().join(root::ROOT_DATABASE);
        Self::open_owned(&path, Some(owner), OpenPurpose::Execution).await
    }

    /// Inspect saved application state without migrations, derived initialization
    /// or stored member/byte changes. Retains the original Root and database lease until
    /// SQL close. Only application SELECT APIs using the owned connection belong
    /// to this opening; it is not a general-purpose read-only EventLog.
    pub async fn for_application_inspection(
        owner: Arc<root::RootOwner>,
    ) -> Result<Self, StoreError> {
        owner.validate_current()?;
        let path = owner.canonical_path().join(root::ROOT_DATABASE);
        Self::open_owned(&path, Some(owner), OpenPurpose::ApplicationInspection).await
    }

    /// Reuse the actual owned Root for another trusted native store. This is a
    /// writer lease, never a guest path or serialized authorization token.
    pub fn owned_root(&self) -> Result<Arc<root::RootOwner>, StoreError> {
        self.validate_root()?;
        self.root_owner.clone().ok_or_else(|| {
            StoreError::InvalidTransition("store requires an owned State Root".into())
        })
    }

    fn validate_root(&self) -> Result<(), StoreError> {
        if let Some(owner) = &self.root_owner {
            owner.validate_current()?;
        }
        Ok(())
    }

    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        Self::open_owned(path, None, OpenPurpose::Execution).await
    }

    async fn open_owned(
        path: &Path,
        root_owner: Option<Arc<root::RootOwner>>,
        purpose: OpenPurpose,
    ) -> Result<Self, StoreError> {
        // A symlink alias must share the same lease and SQLite sidecars.
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if purpose == OpenPurpose::ApplicationInspection {
                    return Err(error.into());
                }
                if path.symlink_metadata().is_ok() {
                    return Err(StoreError::InvalidTransition(
                        "dangling database symlink is unsupported".into(),
                    ));
                }
                let name = path.file_name().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "missing database filename",
                    )
                })?;
                let parent = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                parent.canonicalize()?.join(name)
            }
            Err(error) => return Err(error.into()),
        };
        // Different hard-link names cannot share WAL sidecars safely.
        #[cfg(unix)]
        if let Ok(metadata) = path.metadata() {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err(StoreError::InvalidTransition(
                    "hard-linked event log is unsupported".into(),
                ));
            }
        }
        // A distinct stable lock file avoids colliding with SQLite's own
        // byte-range locks on the database. Never unlink it while in use.
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".writer.lock");
        let acquired = match purpose {
            OpenPurpose::Execution => root::FileLease::acquire(Path::new(&lock_path)),
            OpenPurpose::ApplicationInspection => {
                root::FileLease::acquire_existing(Path::new(&lock_path))
            }
        };
        let lease = match acquired {
            Ok(lease) => Arc::new(lease),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(StoreError::WriterBusy);
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        let options = match purpose {
            OpenPurpose::Execution => SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
            OpenPurpose::ApplicationInspection => application_inspection::options(&path)?,
        };
        let connection = OwnedConnection::open(
            options,
            ConnectionAuthority::Writer {
                lease,
                root: root_owner.clone(),
            },
            match purpose {
                OpenPurpose::Execution => |connection| {
                    Box::pin(async move {
                        schema::initialize_connection(connection).await?;
                        transcript::directory::initialize(connection).await
                    })
                },
                OpenPurpose::ApplicationInspection => application_inspection::initialize,
            },
        )
        .await?;
        let context_cache = Arc::new(context::cache::Cache::default());
        let install = context_cache.clone();
        let high_water = connection
            .run(move |connection| {
                Box::pin(async move {
                    if purpose == OpenPurpose::Execution {
                        install.install(connection).await?;
                    }
                    sequence_number(
                        sqlx::query_scalar::<_, i64>(
                            "SELECT COALESCE(MAX(sequence), 0) FROM event_log",
                        )
                        .fetch_one(connection)
                        .await?,
                    )
                })
            })
            .await;
        let high_water = match high_water {
            Ok(value) => value,
            Err(cause) => {
                // Startup errors have the same real close boundary as normal shutdown.
                if let Err(cleanup) = connection.shutdown().await {
                    return Err(StoreError::StartupCleanupFailed {
                        cause: Box::new(cause),
                        cleanup: Box::new(cleanup),
                    });
                }
                return Err(cause);
            }
        };
        Ok(Self {
            context_cache,
            connection,
            root_owner,
            commits: tokio::sync::watch::channel(high_water).0,
            shell_changes: tokio::sync::broadcast::channel(256).0,
        })
    }

    /// Finish every accepted operation and close SQLite before releasing its lease.
    pub async fn close(self) -> Result<(), StoreError> {
        self.connection.close().await
    }

    /// Close ingress and drain while existing observers still hold log handles.
    pub async fn shutdown(&self) -> Result<(), StoreError> {
        self.connection.shutdown().await
    }

    /// Wait for this SQL owner's real close acknowledgement. A failed close
    /// retains its Root and reports the failure; it never confirms process exit.
    pub async fn drain(&self) {
        let mut reported = std::collections::BTreeSet::new();
        loop {
            match self.shutdown().await {
                Ok(()) => return,
                Err(error) => {
                    let error = error.to_string();
                    if reported.insert(error.clone()) {
                        eprintln!("SQL cleanup unconfirmed: {error:?}");
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                }
            }
        }
    }

    /// A coalescible wakeup, not a replacement for committed facts. Register
    /// before reading a bootstrap snapshot, then catch up from its SQL cursor.
    pub fn subscribe_commits(&self) -> tokio::sync::watch::Receiver<u64> {
        self.commits.subscribe()
    }

    /// Register before reading catalog state. Hints include metadata writes;
    /// only a re-read of the canonical catalog revision establishes a change.
    pub fn subscribe_catalog_changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.connection.subscribe_changes()
    }
}

fn sequence_number(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value)
        .map_err(|_| StoreError::InvalidTransition("negative ledger sequence".into()))
}
