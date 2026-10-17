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

use super::{invalid, proof, read, safety, selection::Selection};
use crate::StoreError;
use maka_agent_facts::context::CheckpointMode;
use maka_agent_facts::context::CompactOutcome;
use maka_agent_facts::event::EventWrite;
use maka_agent_facts::event::Fact;
use maka_agent_facts::event::InvocationOutcome;
use maka_agent_facts::event::RuntimeEvent;
use sqlx::SqliteConnection;

pub(crate) async fn validate_append(
    connection: &mut SqliteConnection,
    event: &RuntimeEvent,
    verification: &mut super::cache::Verification,
) -> Result<(), StoreError> {
    super::request::validate(connection, event, verification).await?;
    match &event.fact {
        Fact::ContextCheckpointRecorded { checkpoint } => {
            safety::current_opening(
                connection,
                &event.invocation.session_id,
                Some(&event.invocation.invocation_id),
            )
            .await?;
            safety::require_safe(
                connection,
                &event.invocation.session_id,
                Some(&event.invocation.invocation_id),
            )
            .await?;
            safety::settled_boundary(connection, &event.invocation.invocation_id, i64::MAX as u64)
                .await?;
            let (_, opening) = proof::by_kind(
                connection,
                &event.invocation.invocation_id,
                "invocation_opened",
                None,
            )
            .await?;
            let selection = Selection::for_opening(connection, &opening).await?;
            let current = read::latest_record(connection, &selection, i64::MAX as u64)
                .await?
                .map(|record| record.event.id);
            if current != checkpoint.previous_checkpoint_id {
                return Err(invalid("checkpoint predecessor changed before adoption"));
            }
            verification.select(&selection);
            chain_cached(
                connection,
                event,
                i64::MAX as u64,
                false,
                None,
                verification,
            )
            .await?;
        }
        Fact::InvocationEnded { outcome } => {
            let compact: bool = sqlx::query_scalar(
                "SELECT json_extract(event_json, '$.fact.input.kind') = 'context_compact'
                 FROM runtime_events WHERE invocation_id = ? AND kind = 'invocation_opened'",
            )
            .bind(&event.invocation.invocation_id)
            .fetch_one(&mut *connection)
            .await?;
            match outcome {
                InvocationOutcome::ContextCompactFinished { outcome } => {
                    if !compact {
                        return Err(invalid("compact terminal belongs to ordinary invocation"));
                    }
                    if let CompactOutcome::Compacted { checkpoint_id } = outcome {
                        let (_, checkpoint) = proof::by_id(connection, checkpoint_id).await?;
                        if checkpoint.invocation != event.invocation
                            || !matches!(checkpoint.fact, Fact::ContextCheckpointRecorded { .. })
                        {
                            return Err(invalid("terminal references another checkpoint"));
                        }
                    }
                    let pending: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM runtime_events d WHERE invocation_id = ? AND kind IN ('tool_dispatched','model_requested')
                         AND NOT EXISTS(SELECT 1 FROM runtime_events t WHERE t.invocation_id = d.invocation_id AND t.operation_id = d.operation_id
                           AND t.kind IN ('tool_settled','model_completed','model_interrupted')))",
                    ).bind(&event.invocation.invocation_id).fetch_one(connection).await?;
                    if pending {
                        return Err(invalid("compact terminal has unresolved execution"));
                    }
                }
                InvocationOutcome::Completed if compact => {
                    return Err(invalid("compact requires a domain terminal"));
                }
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// Strictly decreasing coverage and record sequence make cycles impossible.
/// An optional current source cut must be at or after the first record.
pub(super) async fn chain_cached(
    connection: &mut SqliteConnection,
    first: &RuntimeEvent,
    first_sequence: u64,
    committed: bool,
    source: Option<(&Selection, u64)>,
    verification: &mut super::cache::Verification,
) -> Result<Option<String>, StoreError> {
    let mut event = first.clone();
    let mut sequence = first_sequence;
    let mut needs_terminal = committed;
    let mut sources: Vec<(Selection, Vec<(u64, String)>)> = Vec::new();
    loop {
        let Fact::ContextCheckpointRecorded { checkpoint } = &event.fact else {
            return Err(invalid("predecessor is not a checkpoint"));
        };
        if verification.anchor(&event.id, sequence, checkpoint.covered_through) {
            break;
        }
        #[cfg(test)]
        verification.validated_link();
        let selection = proof::validate(connection, &event, sequence, checkpoint).await?;
        if needs_terminal && matches!(checkpoint.mode, CheckpointMode::Standalone) {
            require_terminal(connection, &event).await?;
        }
        // A missing predecessor cannot silently discard the already adopted baseline.
        let latest = read::latest_record(connection, &selection, checkpoint.covered_through)
            .await?
            .map(|r| r.event.id);
        if latest != checkpoint.previous_checkpoint_id {
            return Err(invalid("predecessor is not the selected source baseline"));
        }
        // Coverage decreases along the chain. Only identical selections share
        // a scan; lineage predicates and copied-history ownership stay intact.
        let expected = (checkpoint.covered_through, checkpoint.source_digest.clone());
        if let Some((prior, prefixes)) = sources.last_mut()
            && *prior == selection
        {
            prefixes.push(expected);
        } else {
            sources.push((selection, vec![expected]));
        }
        let Some(id) = &checkpoint.previous_checkpoint_id else {
            break;
        };
        let (previous_sequence, previous) = proof::by_id(connection, id).await?;
        let Fact::ContextCheckpointRecorded {
            checkpoint: predecessor,
        } = &previous.fact
        else {
            return Err(invalid("predecessor has wrong kind"));
        };
        if previous.invocation.session_id != event.invocation.session_id
            || previous_sequence >= sequence
            || previous_sequence > checkpoint.covered_through
            || predecessor.covered_through >= checkpoint.covered_through
        {
            return Err(invalid(
                "checkpoint coverage must advance in the same Session",
            ));
        }
        event = previous;
        sequence = previous_sequence;
        needs_terminal = true;
    }
    let mut source_digest = None;
    for (selection, prefixes) in sources {
        let mut throughs: Vec<_> = prefixes.iter().rev().map(|(through, _)| *through).collect();
        // The current source and checkpoint prefixes share raw-byte hashing
        // only when their full selection (including lineage) is identical.
        let current =
            source.filter(|(selected, _)| **selected == selection && source_digest.is_none());
        if let Some((_, through)) = current {
            throughs.push(through);
        }
        let mut actual = verification
            .digests(connection, &selection, &throughs)
            .await?;
        if current.is_some() {
            // Current coverage follows the first checkpoint record, so its
            // digest is after every checkpoint source boundary in the stream.
            source_digest = actual.pop();
        }
        if actual
            .iter()
            .zip(prefixes.iter().rev())
            .any(|(actual, (_, expected))| actual != expected)
        {
            return Err(invalid("source digest changed"));
        }
    }
    if let Some((selection, through)) = source
        && source_digest.is_none()
    {
        source_digest = Some(
            verification
                .digests(connection, selection, &[through])
                .await?
                .pop()
                .expect("one requested prefix"),
        );
    }
    if committed {
        verification.adopt(first, first_sequence);
    }
    Ok(source_digest)
}

pub(crate) async fn require_terminal(
    connection: &mut SqliteConnection,
    checkpoint: &RuntimeEvent,
) -> Result<(), StoreError> {
    let (_, terminal) = proof::by_kind(
        connection,
        &checkpoint.invocation.invocation_id,
        "invocation_ended",
        None,
    )
    .await?;
    if terminal.invocation != checkpoint.invocation
        || !matches!(&terminal.fact,
        Fact::InvocationEnded { outcome: InvocationOutcome::ContextCompactFinished { outcome: CompactOutcome::Compacted { checkpoint_id } } } if checkpoint_id == &checkpoint.id)
    {
        return Err(invalid("checkpoint lacks its atomic compacted terminal"));
    }
    Ok(())
}

pub(crate) async fn validate_batch(
    connection: &mut SqliteConnection,
    events: &[EventWrite],
) -> Result<(), StoreError> {
    for write in events {
        if matches!(&write.event().fact, Fact::ContextCheckpointRecorded { checkpoint } if matches!(checkpoint.mode, CheckpointMode::Standalone))
        {
            require_terminal(connection, write.event()).await?;
        }
    }
    Ok(())
}
