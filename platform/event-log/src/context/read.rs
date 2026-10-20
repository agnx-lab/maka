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

use super::selection::Selection;
use super::{
    ContextBaseline, ModelContextSource, boundary, invalid, latest_main, proof, safety, validate,
};
use crate::{EventLog, StoreError};
use maka_agent_facts::context::CheckpointMode;
use maka_agent_facts::event::Fact;
use maka_agent_facts::event::RuntimeEvent;
use maka_agent_facts::event::StoredEvent;
use sqlx::{Connection, SqliteConnection};

impl EventLog {
    /// Check an explicit user's fresh message against all prior facts. The
    /// returned bit only marks whether a sealed unknown dispatch is present.
    pub async fn check_manual_message_history(&self, session: &str) -> Result<bool, StoreError> {
        self.validate_root()?;
        crate::sessions::validate_id(session)?;
        let session = session.to_owned();
        self.connection
            .read_interruptible(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    let pending =
                        safety::require_manual_message_safe(&mut tx, &session, None).await?;
                    tx.commit().await?;
                    Ok(pending)
                })
            })
            .await
    }

    pub async fn read_model_context(
        &self,
        session: &str,
        current: Option<&str>,
    ) -> Result<ModelContextSource, StoreError> {
        self.context_source(session, current, None, false).await
    }

    pub async fn read_manual_message_context(
        &self,
        session: &str,
        current: &str,
    ) -> Result<ModelContextSource, StoreError> {
        self.context_source(session, Some(current), None, true)
            .await
    }

    pub async fn prepare_context_compaction(
        &self,
        session: &str,
        current: Option<&str>,
        mode: &CheckpointMode,
    ) -> Result<ModelContextSource, StoreError> {
        self.context_source(session, current, Some(mode.clone()), false)
            .await
    }

    async fn context_source(
        &self,
        session: &str,
        current: Option<&str>,
        mode: Option<CheckpointMode>,
        manual_message: bool,
    ) -> Result<ModelContextSource, StoreError> {
        self.validate_root()?;
        crate::sessions::validate_id(session)?;
        let (session, current) = (session.to_owned(), current.map(str::to_owned));
        let cache = self.context_cache.clone();
        let writer = self.connection.writer();
        self.connection
            .read_interruptible(move |connection| {
                Box::pin(async move {
                    let attempt = match cache.has_proof(&session) {
                        Some(true) => true,
                        Some(false) => super::cache::history_hint(connection, &session).await,
                        None => false,
                    };
                    let observation = if attempt {
                        Some(super::cache::observe(cache.clone(), &writer, session.clone()).await?)
                    } else {
                        None
                    };
                    let mut tx = connection.begin().await?;
                    let opening =
                        safety::current_opening(&mut tx, &session, current.as_deref()).await?;
                    if manual_message {
                        let Some((_, event)) = &opening else {
                            return Err(invalid("manual message has no active opening"));
                        };
                        if !matches!(
                            &event.fact,
                            Fact::InvocationOpened {
                                input: maka_agent_facts::input::InvocationInput::Message { .. },
                                ..
                            }
                        ) {
                            return Err(invalid(
                                "manual message context requires a message opening",
                            ));
                        }
                        safety::require_manual_message_safe(&mut tx, &session, current.as_deref())
                            .await?;
                    } else {
                        safety::require_safe(&mut tx, &session, current.as_deref()).await?;
                    }
                    let selection = match &opening {
                        Some((_, event)) => Selection::for_opening(&mut tx, event).await?,
                        None => Selection::session(&session),
                    };
                    let (origin, mut verification) = if let Some((origin, certificate)) =
                        observation
                    {
                        let (confirmed, _) =
                            super::cache::observe(cache.clone(), &writer, session.clone()).await?;
                        let certificate = certificate.filter(|_| confirmed == origin);
                        let verification = super::cache::Verification::new(
                            cache.clone(),
                            origin.clone(),
                            certificate,
                        );
                        (Some(origin), verification)
                    } else {
                        (None, super::cache::Verification::default())
                    };
                    let high_water = if let Some(mode) = &mode {
                        if let Some((_, event)) = &opening {
                            boundary::admit_summary(
                                &mut tx,
                                &event.invocation.invocation_id,
                                None,
                                i64::MAX as u64,
                            )
                            .await?;
                            safety::settled_boundary(
                                &mut tx,
                                &event.invocation.invocation_id,
                                i64::MAX as u64,
                            )
                            .await?;
                        }
                        boundary::source_fence(
                            &mut tx,
                            &selection,
                            opening.as_ref(),
                            mode,
                            i64::MAX as u64,
                        )
                        .await?
                    } else {
                        selection.high_water(&mut tx, i64::MAX as u64).await?
                    };
                    let before = i64::MAX as u64;
                    let latest_main =
                        latest_main::read_selected(&mut tx, &selection, high_water).await?;
                    let source = materialize_cached(
                        &mut tx,
                        &selection,
                        high_water,
                        before,
                        usize::MAX,
                        usize::MAX,
                        (latest_main, &mut verification),
                    )
                    .await?;
                    tx.commit().await?;
                    if let Some(origin) = origin {
                        super::cache::promote(cache, &writer, origin, verification.candidate())
                            .await?;
                    }
                    Ok(source)
                })
            })
            .await
    }
}

/// Authenticate the selected context before sealing a handoff.
pub(crate) async fn validate_handoff_context(
    connection: &mut SqliteConnection,
    opening: &maka_agent_facts::event::RuntimeEvent,
) -> Result<(), StoreError> {
    let selection = Selection::for_opening(connection, opening).await?;
    let high = selection.high_water(connection, i64::MAX as u64).await?;
    materialize_selected(
        connection,
        &selection,
        high,
        i64::MAX as u64,
        usize::MAX,
        usize::MAX,
        super::LatestMainContext::TraceUnavailable,
    )
    .await?;
    Ok(())
}

pub(super) async fn materialize_selected(
    connection: &mut SqliteConnection,
    selection: &Selection,
    high_water: u64,
    before: u64,
    max_events: usize,
    max_bytes: usize,
    latest_main: super::LatestMainContext,
) -> Result<ModelContextSource, StoreError> {
    materialize_cached(
        connection,
        selection,
        high_water,
        before,
        max_events,
        max_bytes,
        (latest_main, &mut super::cache::Verification::default()),
    )
    .await
}

async fn materialize_cached(
    connection: &mut SqliteConnection,
    selection: &Selection,
    high_water: u64,
    before: u64,
    max_events: usize,
    max_bytes: usize,
    (latest_main, verification): (super::LatestMainContext, &mut super::cache::Verification),
) -> Result<ModelContextSource, StoreError> {
    let (baseline, source_evidence) =
        latest_with_evidence(connection, selection, high_water, verification).await?;
    let anchor = if let Some(ContextBaseline { checkpoint, .. }) = &baseline {
        if let CheckpointMode::MidTurn { anchor_event_id } = &checkpoint.mode {
            let (sequence, event) = proof::by_id(connection, anchor_event_id).await?;
            Some(StoredEvent { sequence, event })
        } else {
            None
        }
    } else {
        None
    };
    let anchor_bytes = anchor
        .as_ref()
        .map(|a| serde_json::to_vec(&a.event).map(|b| b.len()))
        .transpose()?
        .unwrap_or(0);
    let remaining_bytes = max_bytes
        .checked_sub(anchor_bytes)
        .ok_or(StoreError::PrefixTooLarge)?;
    let remaining_events = max_events
        .checked_sub(usize::from(anchor.is_some()))
        .ok_or(StoreError::PrefixTooLarge)?;
    let after = baseline
        .as_ref()
        .map_or(0, |b| b.checkpoint.covered_through);
    let tail = super::tail::read(
        connection,
        selection,
        after,
        high_water,
        before,
        remaining_events,
        remaining_bytes,
    )
    .await?;
    let effective_source_digest =
        crate::archive::digest_selected(connection, selection, &source_evidence, before).await?;
    Ok(ModelContextSource {
        source_evidence,
        effective_source_digest,
        baseline,
        anchor,
        tail,
        latest_main,
    })
}

pub(crate) async fn latest_selected(
    connection: &mut SqliteConnection,
    selection: &Selection,
    through: u64,
    verification: &mut super::cache::Verification,
) -> Result<Option<ContextBaseline>, StoreError> {
    verification.select(selection);
    let Some(StoredEvent { sequence, event }) =
        latest_record(connection, selection, through).await?
    else {
        return Ok(None);
    };
    validate::chain_cached(connection, &event, sequence, true, None, verification).await?;
    let Fact::ContextCheckpointRecorded { checkpoint } = event.fact else {
        return Err(invalid("checkpoint kind mismatch"));
    };
    Ok(Some(ContextBaseline {
        event_id: event.id,
        checkpoint,
    }))
}

/// Validate the baseline and authenticate the current source in the same scan
/// wherever checkpoint coverage uses the exact current selection.
pub(super) async fn latest_with_evidence(
    connection: &mut SqliteConnection,
    selection: &Selection,
    through: u64,
    verification: &mut super::cache::Verification,
) -> Result<(Option<ContextBaseline>, super::SourceEvidence), StoreError> {
    verification.select(selection);
    let Some(StoredEvent { sequence, event }) =
        latest_record(connection, selection, through).await?
    else {
        return Ok((
            None,
            super::SourceEvidence {
                scope: selection.scope.clone(),
                high_water: through,
                digest: verification
                    .digests(connection, selection, &[through])
                    .await?
                    .pop()
                    .expect("one requested prefix"),
            },
        ));
    };
    let digest = validate::chain_cached(
        connection,
        &event,
        sequence,
        true,
        Some((selection, through)),
        verification,
    )
    .await?
    .expect("requested current source digest");
    let Fact::ContextCheckpointRecorded { checkpoint } = event.fact else {
        return Err(invalid("checkpoint kind mismatch"));
    };
    Ok((
        Some(ContextBaseline {
            event_id: event.id,
            checkpoint,
        }),
        super::SourceEvidence {
            scope: selection.scope.clone(),
            high_water: through,
            digest,
        },
    ))
}

/// Select without traversing predecessors, so chain validation stays iterative.
pub(crate) async fn latest_record(
    connection: &mut SqliteConnection,
    selection: &Selection,
    through: u64,
) -> Result<Option<StoredEvent>, StoreError> {
    let filter = Selection::predicate("c", "?3");
    let eligible = format!(
        "c.kind='context_checkpoint_recorded' AND c.sequence <= ?1 AND {filter}
         AND NOT EXISTS(SELECT 1 FROM runtime_events r WHERE r.invocation_id=c.invocation_id
           AND r.kind='model_requested' AND r.operation_id=json_extract(c.event_json,'$.fact.checkpoint.summary_step_id')
           AND json_extract(r.event_json,'$.fact.source_scope.kind')='lineage'
           AND (?3 IS NULL OR c.invocation_id NOT IN (SELECT value FROM json_each(?3,'$.runs'))))"
    );
    // Limit each indexed branch before combining: a wide UNION view would sort
    // every old body even when the newest native checkpoint is already known.
    let row: Option<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT * FROM (
           SELECT c.sequence,c.event_json FROM runtime_events c
           WHERE json_extract(c.event_json,'$.invocation.session_id')=?2 AND {eligible}
           ORDER BY c.sequence DESC LIMIT 1)
         UNION ALL SELECT * FROM (
           SELECT c.sequence,c.event_json FROM session_history_members h
           JOIN runtime_events c ON c.sequence=h.sequence
           WHERE h.session_id=?2 AND {eligible} ORDER BY h.sequence DESC LIMIT 1)
         ORDER BY sequence DESC LIMIT 1"
    )))
    .bind(through as i64)
    .bind(&selection.session)
    .bind(&selection.lineage)
    .fetch_optional(&mut *connection)
    .await?;
    let Some((sequence, json)) = row else {
        return Ok(None);
    };
    let event: RuntimeEvent = serde_json::from_str(&json)?;
    Ok(Some(StoredEvent {
        sequence: crate::sequence_number(sequence)?,
        event,
    }))
}
