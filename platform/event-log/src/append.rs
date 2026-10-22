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

use maka_agent_facts::event::CommitError;
use maka_agent_facts::event::CommitFuture;
use maka_agent_facts::event::EventSink;
use maka_agent_facts::event::EventWrite;
use maka_agent_facts::event::Fact;
use sqlx::{Connection, SqliteConnection};

use crate::{EventLog, StoreError, sequence_number};

pub(crate) mod history;
pub(crate) mod observations;

impl EventLog {
    /// Commit ordered facts atomically, validating each against the preceding
    /// facts in this transaction. Exact replays return their original sequence
    /// until Session removal completes, after which all writes are rejected.
    /// This must not enclose effects: live dispatch and outcome remain separate.
    pub async fn append_batch(&self, events: &[EventWrite]) -> Result<Vec<u64>, CommitError> {
        if !events.is_empty()
            && events
                .iter()
                .all(|write| matches!(write.event().fact, Fact::ModelObserved { .. }))
        {
            self.validate_root().map_err(commit_error)?;
            return self
                .connection
                .writer()
                .observations(events.to_vec(), self.commits.clone())
                .await;
        }
        self.append_batch_checked(events, None)
            .await
            .map_err(commit_error)
    }

    /// A selected native executor checks its original capability grant in the
    /// same transaction as T1. Revocation cannot race a separate grant read.
    /// This compares existing authority; it neither creates a grant nor runs effects.
    pub async fn append_capability_dispatch(
        &self,
        event: &EventWrite,
        target: &maka_agent_facts::interaction::GrantTarget,
        base_revision: u64,
    ) -> Result<u64, CommitError> {
        if !matches!(event.event().fact, Fact::ToolDispatched { .. }) {
            return Err(CommitError::Rejected(
                "capability admission requires a tool dispatch".into(),
            ));
        }
        self.append_batch_checked(
            std::slice::from_ref(event),
            Some((target.clone(), base_revision)),
        )
        .await
        .map(|sequences| sequences[0])
        .map_err(commit_error)
    }

    async fn append_batch_checked(
        &self,
        events: &[EventWrite],
        capability: Option<(maka_agent_facts::interaction::GrantTarget, u64)>,
    ) -> Result<Vec<u64>, StoreError> {
        self.validate_root()?;
        let events = events.to_vec();
        let commits = self.commits.clone();
        let cache = self.context_cache.clone();
        let ingress = self.connection.writer();
        self.connection
            .run(move |connection| {
                Box::pin(async move {
                    let context = if let Some(consumer) =
                        events.iter().find(|write| match &write.event().fact {
                            Fact::ContextCheckpointRecorded { .. } => true,
                            Fact::ModelRequested {
                                purpose,
                                checkpoint_event_id,
                                source_scope,
                                ..
                            } => {
                                checkpoint_event_id.is_some()
                                    || *purpose == maka_agent_facts::context::ModelPurpose::Summary
                                    || (matches!(
                                        source_scope,
                                        maka_agent_facts::event::LogScope::Lineage { .. }
                                    ) && cache.has_raw(&write.event().invocation.session_id))
                            }
                            _ => false,
                        }) {
                        let origin = cache.stamp(connection, ingress.failures()).await?;
                        let certificate =
                            cache.get(&consumer.event().invocation.session_id, &origin)?;
                        Some((origin, certificate))
                    } else {
                        None
                    };
                    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
                    let (origin, mut verification) = if let Some((origin, certificate)) = context {
                        let certificate = if cache
                            .confirm_append(&mut transaction, &origin, ingress.failures())
                            .await?
                        {
                            certificate
                        } else {
                            None
                        };
                        let verification = crate::context::cache::Verification::new(
                            cache.clone(),
                            origin.clone(),
                            certificate,
                        );
                        (Some(origin), verification)
                    } else {
                        (None, crate::context::cache::Verification::default())
                    };
                    let mut sequences = Vec::with_capacity(events.len());
                    let mut last_inserted = None;
                    for event in &events {
                        if let Some((target, revision)) = &capability
                            && crate::interactions::capability_grant(
                                &mut transaction,
                                &event.event().invocation.session_id,
                                target,
                            )
                            .await?
                            .is_none_or(|grant| grant.base_revision != Some(*revision))
                        {
                            return Err(StoreError::PermissionRevoked);
                        }
                        let result =
                            Self::append_verified(&mut transaction, event, &mut verification)
                                .await?;
                        let sequence = match result {
                            AppendResult::Existing(sequence) => sequence,
                            AppendResult::Inserted(sequence) => {
                                last_inserted = Some(sequence);
                                sequence
                            }
                        };
                        sequences.push(sequence);
                    }
                    crate::context::validate_batch(&mut transaction, &events).await?;
                    if let Some(sequence) = last_inserted {
                        transaction
                            .commit()
                            .await
                            .map_err(StoreError::CommitUnknown)?;
                        commits.send_replace(sequence);
                        // Optional cache work cannot change a confirmed commit's outcome.
                        if let Some(origin) = origin
                            && let Some(candidate) = verification.candidate()
                        {
                            match cache.stamp(connection, ingress.failures()).await {
                                Ok(stamp) if stamp == origin => {
                                    if cache.publish(candidate).is_err() {
                                        cache.invalidate();
                                    }
                                }
                                Ok(_) => {}
                                Err(_) => cache.invalidate(),
                            }
                        }
                    } else {
                        transaction.rollback().await?;
                    }
                    drop(events);
                    Ok(sequences)
                })
            })
            .await
    }

    pub(crate) async fn append_in_transaction(
        transaction: &mut SqliteConnection,
        write: &EventWrite,
    ) -> Result<AppendResult, StoreError> {
        Self::append_verified(
            transaction,
            write,
            &mut crate::context::cache::Verification::default(),
        )
        .await
    }

    async fn append_verified(
        transaction: &mut SqliteConnection,
        write: &EventWrite,
        verification: &mut crate::context::cache::Verification,
    ) -> Result<AppendResult, StoreError> {
        let event = write.event();
        crate::recovery::require_local(transaction, &event.invocation.invocation_id).await?;
        let removal =
            crate::sessions::removal::read(transaction, &event.invocation.session_id).await?;
        if removal == Some(crate::sessions::SessionRetirement::Removed) {
            return Err(StoreError::SessionRetired);
        }
        let json = serde_json::to_string(event)?;
        let previous: Option<(i64, String)> =
            sqlx::query_as("SELECT sequence, event_json FROM runtime_events WHERE event_id = ?")
                .bind(&event.id)
                .fetch_optional(&mut *transaction)
                .await?;
        if let Some((sequence, previous)) = previous {
            if previous != json {
                return Err(StoreError::EventConflict);
            }
            crate::tool_payloads::verify_replay(transaction, write).await?;
            crate::composition::verify_replay(transaction, write).await?;
            crate::usage::valuation::verify_replay(transaction, write).await?;
            crate::archive::verify_replay(transaction, event).await?;
            return Ok(AppendResult::Existing(sequence_number(sequence)?));
        }
        if removal.is_some()
            && matches!(
                event.fact,
                Fact::ModelRequested { .. }
                    | Fact::ToolDispatched { .. }
                    | Fact::ExecutorStarted { .. }
                    | Fact::MessageSteered { .. }
            )
        {
            return Err(StoreError::SessionRetired);
        }
        let id = &event.invocation.invocation_id;
        history::validate_cached(transaction, event, verification).await?;
        if matches!(event.fact, Fact::InvocationOpened { .. }) {
            crate::sessions::retain(transaction, &event.invocation.session_id).await?;
            let archived: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM session_control WHERE id = ? AND archived = 1)",
            )
            .bind(&event.invocation.session_id)
            .fetch_one(&mut *transaction)
            .await?;
            if archived {
                return Err(StoreError::InvalidTransition(
                    "cannot admit an archived session".into(),
                ));
            }
            if crate::sessions::has_active_invocation(transaction, &event.invocation.session_id)
                .await?
            {
                return Err(StoreError::InvalidTransition(
                    "session already has an unsealed invocation".into(),
                ));
            }
        }
        crate::handoff::validate_admission(transaction, event).await?;
        crate::message_identity::validate(transaction, event).await?;
        crate::message_admissions::consume(transaction, event).await?;
        if matches!(event.fact, Fact::InvocationEnded { .. }) {
            crate::interactions::lifecycle::require_closed(transaction, &event.invocation).await?;
        }
        let inserted = sqlx::query(
            "INSERT INTO event_log (event_id, invocation_id, kind, operation_id, event_json)
             SELECT ?1, ?2, ?3, ?4, ?5 WHERE NOT EXISTS (
                 SELECT 1 FROM session_message_sources WHERE owner_session_id = ?6 AND message_id = ?1
                 UNION ALL SELECT 1 FROM message_admissions WHERE session_id = ?6 AND message_id = ?1)",
        )
        .bind(&event.id)
        .bind(id)
        .bind(event.fact.kind())
        .bind(event.fact.operation_id())
        .bind(json)
        .bind(&event.invocation.session_id)
        .execute(&mut *transaction)
        .await?;
        if inserted.rows_affected() != 1 {
            return Err(StoreError::EventConflict);
        }
        let sequence = sequence_number(inserted.last_insert_rowid())?;
        crate::message_sources::insert(transaction, event).await?;
        crate::tool_payloads::insert(transaction, write).await?;
        crate::artifacts::freeze_tool_media(transaction, event, sequence).await?;
        crate::composition::insert(transaction, write).await?;
        crate::usage::valuation::append(transaction, write).await?;
        if matches!(event.fact, Fact::InvocationEnded { .. }) {
            crate::sessions::read_state::mark_unread(
                transaction,
                &event.invocation.session_id,
                &event.invocation.invocation_id,
            )
            .await?;
        }
        if matches!(
            event.fact,
            Fact::InvocationOpened { .. }
                | Fact::MessageSteered { .. }
                | Fact::ModelCompleted { .. }
                | Fact::ModelInterrupted { .. }
                | Fact::InvocationEnded { .. }
        ) {
            crate::sessions::project_execution(transaction, sequence).await?;
            crate::sessions::advance_revision(transaction, &event.invocation.session_id).await?;
        }
        crate::transcript::directory::record_source(
            transaction,
            &event.invocation.session_id,
            sequence,
        )
        .await?;
        Ok(AppendResult::Inserted(sequence))
    }
}

fn commit_error(error: StoreError) -> CommitError {
    match error {
        StoreError::CommitUnknown(error) => CommitError::OutcomeUnknown(error.to_string()),
        StoreError::OperationUnknown => CommitError::OutcomeUnknown(error.to_string()),
        StoreError::SessionRetired => CommitError::Retired,
        StoreError::PermissionRevoked => CommitError::PermissionRevoked,
        other => CommitError::Rejected(other.to_string()),
    }
}

pub(crate) enum AppendResult {
    Existing(u64),
    Inserted(u64),
}

impl EventLog {
    pub async fn append(&self, event: &EventWrite) -> Result<u64, CommitError> {
        Ok(self.append_batch(std::slice::from_ref(event)).await?[0])
    }
}

impl EventSink for EventLog {
    fn commit(self: std::sync::Arc<Self>, event: EventWrite) -> CommitFuture {
        Box::pin(async move { self.append(&event).await })
    }
}
