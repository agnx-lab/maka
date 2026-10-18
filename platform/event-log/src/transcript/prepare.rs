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

//! One bounded disposable projection batch on the original SQLite worker.
use super::{MAX_TEXT_BYTES, evidence, read};
use crate::StoreError;
use maka_agent_facts::event::{Fact, ToolOutcome};
use maka_presentation::{InvocationView, MAX_TOOL_ROW_BYTES, ProjectionError, Row};
use rusqlite::{Connection as RawConnection, OptionalExtension, params};
use sha2::{Digest, Sha256};

const PROGRESS: &str = r#"SELECT through_sequence FROM transcript_progress WHERE session_id = ?"#;
const PROGRESS_WRITE: &str = r#"INSERT INTO transcript_progress VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET through_sequence = excluded.through_sequence"#;
const INSERT_ROW: &str = r#"INSERT OR IGNORE INTO transcript_rows
         (source_sequence, item_index, session_id, turn_id, message_id, payload, digest, total_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#;
const MATCH_ROW: &str = r#"SELECT EXISTS(SELECT 1 FROM transcript_rows
         WHERE source_sequence = ?1 AND item_index = ?2 AND session_id = ?3 AND turn_id = ?4 AND message_id = ?5
         AND payload = ?6 AND digest = ?7 AND total_bytes = ?8)"#;

/// Return (ready, wrote_progress); a ready existing index keeps SQLx's original
/// read-only transaction-drop path rather than committing an empty batch.
pub(super) fn batch(
    db: &RawConnection,
    session: &str,
    through: u64,
    max_boundaries: usize,
) -> Result<(bool, bool), StoreError> {
    let fence = evidence::fence_on_handle(db, through)?;
    let progress: Option<i64> = db
        .prepare_cached(PROGRESS)?
        .query_row([session], |row| row.get(0))
        .optional()?;
    if progress.is_some_and(|value| value >= fence) {
        return Ok((true, false));
    }
    let progress = progress.unwrap_or(0);
    let eligible = "(e.kind = 'context_checkpoint_recorded' OR NOT EXISTS (SELECT 1 FROM runtime_events opening
                       WHERE opening.invocation_id = e.invocation_id
                         AND opening.kind = 'invocation_opened'
                         AND json_extract(opening.event_json, '$.fact.input.kind') = 'context_compact'))
                   AND (e.kind NOT IN ('model_completed', 'model_interrupted') OR
                       EXISTS (SELECT 1 FROM runtime_events request
             JOIN runtime_events opening ON opening.invocation_id = request.invocation_id
             AND opening.kind = 'invocation_opened'
             WHERE request.kind = 'model_requested' AND request.invocation_id = e.invocation_id
             AND request.operation_id = e.operation_id
             AND json_extract(opening.event_json, '$.fact.input.kind') IN ('message', 'continuation', 'handoff')
             AND json_extract(request.event_json, '$.fact.purpose') = 'main'))
                   AND (e.kind IN ('message_imported', 'invocation_opened', 'message_steered', 'model_completed',
                                'model_interrupted', 'invocation_ended', 'context_checkpoint_recorded',
                                'tool_dispatched', 'tool_rejected', 'tool_settled', 'executor_completed')
                       OR (e.kind = 'executor_observed' AND json_extract(e.event_json, '$.fact.output.type') IN ('tool_start','tool_result')))";
    let sql = format!(
                            "SELECT e.sequence AS sequence, e.invocation_id, e.operation_id, e.kind
                             FROM runtime_events e
                             WHERE json_extract(e.event_json, '$.invocation.session_id') = ?1
                               AND e.sequence > ?2 AND e.sequence <= ?3 AND {eligible}
                             UNION ALL
                             SELECT h.sequence AS sequence, e.invocation_id, e.operation_id, e.kind
                             FROM session_history_members h CROSS JOIN runtime_events e ON e.sequence = h.sequence
                             WHERE h.session_id = ?1 AND h.sequence > ?2 AND h.sequence <= ?3 AND {eligible}
                             ORDER BY sequence LIMIT ?4"
                        );

    let boundaries = db
        .prepare_cached(&sql)?
        .query_map(
            params![session, progress, fence, (max_boundaries + 1) as i64],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let ready = boundaries.len() <= max_boundaries;
    let mut processed = progress;
    let mut retained: Option<(&str, Vec<i64>, InvocationView)> = None;
    for (sequence, invocation, step, kind) in boundaries.iter().take(max_boundaries) {
        if kind == "context_checkpoint_recorded" {
            drop(retained.take());
            persist(db, session, super::compaction::project(db, *sequence)?)?;
            processed = *sequence;
            continue;
        }
        let ids = evidence::selected_on_handle(db, invocation, step.as_deref(), *sequence, true)?;
        let (mut view, prefix) = match retained.take() {
            Some((prior, previous, view))
                if prior == invocation.as_str() && ids.starts_with(&previous) =>
            {
                (view, previous.len())
            }
            old => {
                // A changed selection must not inherit evidence absent from this boundary.
                drop(old);
                (InvocationView::new(MAX_TEXT_BYTES)?, 0)
            }
        };
        for &sequence in ids.iter().skip(prefix) {
            let fact = evidence::fact_on_handle(db, sequence)?;
            let resolved = if let Fact::ToolSettled {
                outcome: ToolOutcome::Succeeded { raw, .. },
                ..
            } = &fact.event.fact
            {
                // Raw wrapper overhead is smaller than the required Message envelope,
                // so this bound rejects before hydration; presentation checks exact size.
                if raw.bytes > MAX_TOOL_ROW_BYTES as u64 {
                    return Err(ProjectionError::TooLarge.into());
                }
                Some(crate::tool_payloads::resolve_in_transaction(
                    db,
                    &fact.event,
                )?)
            } else {
                None
            };
            for mut row in view.push_with_tool_output(&fact, resolved.as_ref())? {
                if fact.event.invocation.session_id != session {
                    owned_references(db, session, &mut row.message)?;
                }
                persist(db, session, row)?;
            }
        }
        retained = Some((invocation.as_str(), ids, view));
        processed = *sequence;
    }
    db.prepare_cached(PROGRESS_WRITE)?
        .execute(params![session, if ready { fence } else { processed }])?;
    Ok((ready, true))
}

fn owned_references(
    db: &RawConnection,
    session: &str,
    message: &mut maka_presentation::Message,
) -> Result<(), StoreError> {
    use maka_agent_facts::attachment::StorageRef;
    use maka_presentation::{Content, ToolContent};
    let refs: Vec<_> = match &mut message.content {
        Content::User { attachments, .. } => attachments
            .iter_mut()
            .flatten()
            .map(|a| &mut a.storage_ref)
            .collect(),
        Content::ToolResult {
            content: ToolContent::Image { reference, .. },
            ..
        } => vec![reference],
        _ => Vec::new(),
    };
    for reference in refs {
        if let StorageRef::SessionFile {
            session_id,
            relative_path,
        } = reference
        {
            let id: Option<String> = db
                .prepare_cached(crate::artifacts::history::RESOLVE_SQL)?
                .query_row(params![session, &*session_id, &*relative_path], |row| {
                    row.get(0)
                })
                .optional()?;
            *reference = id
                .map(|relative_path| StorageRef::SessionFile {
                    session_id: session.into(),
                    relative_path,
                })
                .ok_or(StoreError::TranscriptConflict)?;
        }
    }
    Ok(())
}

fn persist(db: &RawConnection, session: &str, row: Row) -> Result<(), StoreError> {
    let bytes = serde_json::to_vec(&row.message)?;
    let (sequence, item) = read::sql_position(row.position)?;
    let size = i64::try_from(bytes.len()).map_err(|_| StoreError::PrefixTooLarge)?;
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    db.prepare_cached(INSERT_ROW)?.execute(params![
        sequence,
        item,
        session,
        &row.message.turn_id,
        &row.message.id,
        &bytes,
        &digest,
        size
    ])?;
    let matches: bool = db.prepare_cached(MATCH_ROW)?.query_row(
        params![
            sequence,
            item,
            session,
            &row.message.turn_id,
            &row.message.id,
            &bytes,
            &digest,
            size
        ],
        |row| row.get(0),
    )?;
    if !matches {
        return Err(StoreError::TranscriptConflict);
    }
    Ok(())
}
