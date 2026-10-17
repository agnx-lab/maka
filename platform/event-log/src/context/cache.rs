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

//! Owner-local certificates for exact committed history. No permissions or
//! rendered data are retained here.

use super::selection::Selection;
use crate::{StoreError, connection::WriterIngress};
use sqlx::{Decode, Sqlite, SqliteConnection, sqlite::SqliteOperation};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, AtomicU64, Ordering},
};

#[derive(Default)]
pub(crate) struct Cache {
    #[cfg(test)]
    anchor_hits: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    validated_links: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    stamps: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    raw_reuses: std::sync::atomic::AtomicUsize,
    generation: AtomicU64,
    frontier: AtomicI64,
    certificates: Mutex<HashMap<String, Certificate>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    version: i64,
    main_schema: i64,
    temp_schema: i64,
    failures: u64,
    generation: u64,
}

#[derive(Clone)]
pub(crate) struct Certificate {
    pub selection: Selection,
    pub stamp: Stamp,
    pub raw: Option<Raw>,
    pub checkpoint: Option<Checkpoint>,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Raw {
    pub through: u64,
    pub state: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct Checkpoint {
    pub id: String,
    pub sequence: u64,
    pub covered_through: u64,
}

impl Cache {
    /// Presence only selects an optimization attempt; fresh stamps and the
    /// transaction's real Selection still authenticate every consumed proof.
    pub(crate) fn has_proof(&self, session: &str) -> Option<bool> {
        self.certificates.lock().ok().map(|entries| {
            entries
                .get(session)
                .is_some_and(|entry| entry.raw.is_some() || entry.checkpoint.is_some())
        })
    }

    pub(crate) fn has_anchor(&self, session: &str) -> bool {
        self.certificates.lock().ok().is_some_and(|entries| {
            entries
                .get(session)
                .is_some_and(|entry| entry.checkpoint.is_some())
        })
    }

    pub(crate) fn has_raw(&self, session: &str) -> bool {
        self.certificates.lock().ok().is_some_and(|entries| {
            entries
                .get(session)
                .is_some_and(|entry| entry.raw.is_some())
        })
    }

    pub(crate) async fn install(
        self: &Arc<Self>,
        connection: &mut SqliteConnection,
    ) -> Result<(), StoreError> {
        let observed = self.clone();
        let mut handle = connection.lock_handle().await?;
        handle.set_preupdate_hook(move |change| {
            if change.database != "main" {
                return;
            }
            match change.table {
                "session_history_members" => observed.invalidate(),
                "event_log" => {
                    if change.operation != SqliteOperation::Insert {
                        observed.invalidate();
                        return;
                    }
                    let Ok(row) = change.get_new_row_id() else {
                        observed.invalidate();
                        return;
                    };
                    // Advance on every observed INSERT, even without a certificate.
                    // An out-of-order insertion can change an already pinned cut.
                    if row <= observed.frontier.fetch_max(row, Ordering::AcqRel) {
                        observed.invalidate();
                    }
                    let kind = change
                        .get_new_column_value(3)
                        .ok()
                        .and_then(|value| <&str as Decode<Sqlite>>::decode(value).ok());
                    if kind.is_none_or(|kind| kind == "tool_result_archived") {
                        observed.invalidate();
                    }
                }
                _ => {}
            }
        });
        Ok(())
    }

    pub(crate) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) async fn stamp(
        &self,
        connection: &mut SqliteConnection,
        failures: u64,
    ) -> Result<Stamp, StoreError> {
        #[cfg(test)]
        self.stamps.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the live SQLx handle remains locked during this synchronous
        // SQLite transaction-state observation.
        let autocommit = unsafe {
            rusqlite::ffi::sqlite3_get_autocommit(
                connection.lock_handle().await?.as_raw_handle().as_ptr(),
            )
        };
        if autocommit == 0 {
            self.invalidate();
            return Err(StoreError::OperationUnknown);
        }
        let stamp = self.current_stamp(connection, failures).await?;
        let maximum: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0) FROM event_log")
            .fetch_one(connection)
            .await?;
        // Only fresh writer boundaries rebase after rollback/external commits.
        self.frontier.store(maximum, Ordering::Release);
        Ok(stamp)
    }

    async fn current_stamp(
        &self,
        connection: &mut SqliteConnection,
        failures: u64,
    ) -> Result<Stamp, StoreError> {
        let version = sqlx::query_scalar("PRAGMA main.data_version")
            .fetch_one(&mut *connection)
            .await?;
        let main_schema = sqlx::query_scalar("PRAGMA main.schema_version")
            .fetch_one(&mut *connection)
            .await?;
        let temp_schema = sqlx::query_scalar("PRAGMA temp.schema_version")
            .fetch_one(connection)
            .await?;
        Ok(Stamp {
            version,
            main_schema,
            temp_schema,
            failures,
            generation: self.generation.load(Ordering::Acquire),
        })
    }

    /// BEGIN IMMEDIATE has pinned and locked the append snapshot. Confirm the
    /// original autocommit observation; never re-label its certificate here.
    pub(crate) async fn confirm_append(
        &self,
        connection: &mut SqliteConnection,
        origin: &Stamp,
        failures: u64,
    ) -> Result<bool, StoreError> {
        Ok(origin == &self.current_stamp(connection, failures).await?)
    }

    pub(crate) fn get(
        &self,
        session: &str,
        stamp: &Stamp,
    ) -> Result<Option<Certificate>, StoreError> {
        let mut certificates = self
            .certificates
            .lock()
            .map_err(|_| StoreError::OperationUnknown)?;
        if certificates
            .get(session)
            .is_some_and(|certificate| &certificate.stamp != stamp)
        {
            certificates.remove(session);
        }
        Ok(certificates.get(session).cloned())
    }

    pub(crate) fn publish(&self, candidate: Certificate) -> Result<(), StoreError> {
        let Some(session) = candidate.selection.session.clone() else {
            return Ok(());
        };
        let mut certificates = self
            .certificates
            .lock()
            .map_err(|_| StoreError::OperationUnknown)?;
        if let Some(current) = certificates.get(&session)
            && current.stamp == candidate.stamp
            && current.selection == candidate.selection
            && (current
                .checkpoint
                .as_ref()
                .map_or(0, |value| value.sequence)
                > candidate
                    .checkpoint
                    .as_ref()
                    .map_or(0, |value| value.sequence)
                || current.raw.as_ref().map_or(0, |value| value.through)
                    > candidate.raw.as_ref().map_or(0, |value| value.through))
        {
            return Ok(());
        }
        certificates.insert(session, candidate);
        Ok(())
    }
}

pub(crate) async fn observe(
    cache: Arc<Cache>,
    writer: &WriterIngress,
    session: String,
) -> Result<(Stamp, Option<Certificate>), StoreError> {
    let ingress = writer.clone();
    writer
        .run(move |connection| {
            Box::pin(async move {
                let stamp = cache.stamp(connection, ingress.failures()).await?;
                let certificate = cache.get(&session, &stamp)?;
                Ok((stamp, certificate))
            })
        })
        .await
}

/// Transaction-local scratch. Only an authenticated reader handshake or the
/// audited append entry constructs this; ordinary run validators use Default.
#[derive(Default)]
pub(crate) struct Verification {
    cache: Option<Arc<Cache>>,
    origin: Option<Stamp>,
    candidate: Option<Certificate>,
}

impl Verification {
    pub(crate) fn new(cache: Arc<Cache>, origin: Stamp, certificate: Option<Certificate>) -> Self {
        Self {
            cache: Some(cache),
            origin: Some(origin),
            candidate: certificate,
        }
    }

    fn check(&mut self) {
        if let (Some(cache), Some(origin), Some(candidate)) =
            (&self.cache, &self.origin, &mut self.candidate)
            && cache.generation.load(Ordering::Acquire) != origin.generation
        {
            candidate.raw = None;
            candidate.checkpoint = None;
        }
    }

    pub(crate) fn select(&mut self, selection: &Selection) {
        self.check();
        if let Some(origin) = &self.origin
            && self
                .candidate
                .as_ref()
                .is_none_or(|candidate| candidate.selection != *selection)
        {
            self.candidate = Some(Certificate {
                selection: selection.clone(),
                stamp: origin.clone(),
                raw: None,
                checkpoint: None,
            });
        }
    }

    pub(crate) fn anchor(&mut self, id: &str, sequence: u64, covered_through: u64) -> bool {
        self.check();
        let matched = self
            .candidate
            .as_ref()
            .and_then(|candidate| candidate.checkpoint.as_ref())
            .is_some_and(|anchor| {
                anchor.id == id
                    && anchor.sequence == sequence
                    && anchor.covered_through == covered_through
            });
        #[cfg(test)]
        if matched && let Some(cache) = &self.cache {
            cache.anchor_hits.fetch_add(1, Ordering::Relaxed);
        }
        matched
    }

    #[cfg(test)]
    pub(super) fn validated_link(&self) {
        if let Some(cache) = &self.cache {
            cache.validated_links.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn adopt(&mut self, event: &maka_agent_facts::event::RuntimeEvent, sequence: u64) {
        self.check();
        if let Some(candidate) = &mut self.candidate
            && let maka_agent_facts::event::Fact::ContextCheckpointRecorded { checkpoint } =
                &event.fact
        {
            candidate.checkpoint = Some(Checkpoint {
                id: event.id.clone(),
                sequence,
                covered_through: checkpoint.covered_through,
            });
        }
    }

    pub(crate) async fn digests(
        &mut self,
        connection: &mut SqliteConnection,
        selection: &Selection,
        throughs: &[u64],
    ) -> Result<Vec<String>, StoreError> {
        self.check();
        let candidate = self
            .candidate
            .as_mut()
            .filter(|candidate| candidate.selection == *selection);
        let raw = candidate
            .as_ref()
            .and_then(|candidate| candidate.raw.as_ref());
        #[cfg(test)]
        if raw.is_some_and(|raw| throughs.first().is_some_and(|cut| *cut >= raw.through))
            && let Some(cache) = &self.cache
        {
            cache.raw_reuses.fetch_add(1, Ordering::Relaxed);
        }
        let (digests, raw) = super::evidence::scan(connection, selection, throughs, raw).await?;
        if let Some(candidate) = candidate
            && raw.as_ref().is_some_and(|raw| {
                candidate
                    .raw
                    .as_ref()
                    .is_none_or(|old| raw.through >= old.through)
            })
        {
            candidate.raw = raw;
        }
        Ok(digests)
    }

    pub(crate) fn candidate(self) -> Option<Certificate> {
        self.candidate
            .filter(|candidate| candidate.raw.is_some() || candidate.checkpoint.is_some())
    }
}

/// A disposable autocommit hint, never evidence about the work snapshot.
pub(crate) async fn history_hint(connection: &mut SqliteConnection, session: &str) -> bool {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runtime_events e
        WHERE json_extract(e.event_json, '$.invocation.session_id')=?1
          AND e.kind <> 'invocation_opened')
        OR EXISTS(SELECT 1 FROM session_history_members WHERE session_id=?1)",
    )
    .bind(session)
    .fetch_one(connection)
    .await
    .unwrap_or(false)
}

pub(crate) async fn promote(
    cache: Arc<Cache>,
    writer: &WriterIngress,
    origin: Stamp,
    candidate: Option<Certificate>,
) -> Result<(), StoreError> {
    let Some(candidate) = candidate else {
        return Ok(());
    };
    let ingress = writer.clone();
    writer
        .run(move |connection| {
            Box::pin(async move {
                if cache.stamp(connection, ingress.failures()).await? == origin {
                    cache.publish(candidate)?;
                }
                Ok(())
            })
        })
        .await
}

#[cfg(test)]
mod tests;
