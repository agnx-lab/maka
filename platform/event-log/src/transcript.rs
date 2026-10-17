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

//! Disposable message index; authority stays in the owner-typed canonical ledger.
mod active;
mod compaction;
pub(crate) use active::interrupted_messages;
mod evidence;
mod history;
mod image;
mod model;
pub mod navigation;
mod prompt;
mod read;
pub mod search;
mod source;
mod timing;
mod tool_metadata;
mod tools;
use crate::{EventLog, StoreError};
use maka_agent_facts::event::Fact;
use maka_agent_facts::event::ToolOutcome;
use maka_presentation::{InvocationView, MAX_TOOL_ROW_BYTES, ProjectionError, Row};
pub use read::*;
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row as SqlRow, SqliteConnection};

const MAX_BOUNDARIES: usize = 32;
const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

impl EventLog {
    /// Prepare at most 32 immutable boundaries through a fixed raw-log fence.
    /// False means another bounded call is needed. Oversize/unsupported evidence
    /// is an error, never an indefinitely preparing state. No commit notice is
    /// emitted: these writes carry no execution facts.
    pub async fn prepare_transcript(
        &self,
        session: &str,
        through: u64,
        max_boundaries: usize,
    ) -> Result<bool, StoreError> {
        crate::sessions::validate_id(session)?;
        if !(1..=MAX_BOUNDARIES).contains(&max_boundaries) {
            return Err(StoreError::InvalidTransition(
                "invalid transcript boundary limit".into(),
            ));
        }
        read::sql_number(through)?;
        self.validate_root()?;
        let session = session.to_owned();
        self.connection
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    let fence = evidence::fence(&mut tx, through).await?;
                    let progress: Option<i64> = sqlx::query_scalar(
                        "SELECT through_sequence FROM transcript_progress WHERE session_id = ?",
                    )
                    .bind(&session)
                    .fetch_optional(&mut *tx)
                    .await?;
                    if progress.is_some_and(|progress| progress >= fence) {
                        return Ok(true);
                    }
                    let progress = progress.unwrap_or(0);
                    // Read only small boundary headers here. Canonical payloads are selected
                    // and budget-checked separately, one boundary at a time.
                    let boundaries = {
                        // The shared history view requires a whole-remainder sort here.
                        // Merge the two ordered ownership paths before applying the limit.
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
                        // Only the static predicate is interpolated; all inputs remain bound.
                        let records = sqlx::query(sqlx::AssertSqlSafe(sql))
                        .bind(&session)
                        .bind(progress)
                        .bind(fence)
                        .bind((max_boundaries + 1) as i64)
                        .fetch_all(&mut *tx)
                        .await?;
                        records
                            .into_iter()
                            .map(|row| {
                                Ok((
                                    row.try_get::<i64, _>(0)?,
                                    row.try_get::<String, _>(1)?,
                                    row.try_get::<Option<String>, _>(2)?,
                                    row.try_get::<String, _>(3)?,
                                ))
                            })
                            .collect::<Result<Vec<_>, sqlx::Error>>()?
                    };
                    let ready = boundaries.len() <= max_boundaries;
                    let mut processed = progress;
                    for (sequence, invocation, step, kind) in boundaries.iter().take(max_boundaries) {
                        if kind == "context_checkpoint_recorded" {
                            let row = compaction::project(&mut tx, *sequence).await?;
                            persist(&mut tx, &session, row).await?;
                            processed = *sequence;
                            continue;
                        }
                        let facts = evidence::selected(
                            &mut tx,
                            invocation,
                            step.as_deref(),
                            *sequence,
                            true,
                        )
                        .await?;
                        let mut view = InvocationView::new(MAX_TEXT_BYTES)?;
                        for fact in facts {
                            let resolved = if let Fact::ToolSettled {
                                outcome: ToolOutcome::Succeeded { raw, .. },
                                ..
                            } = &fact.event.fact
                            {
                                // Raw's Json/Mcp/Text/Image wrappers exceed their ToolContent
                                // by 2/1/3/12 bytes. The required full Message envelope is
                                // larger than all four differences, so this lower bound can
                                // reject before hydrating up to 64 MiB. Exact row sizing still
                                // runs in presentation; this is not payload verification.
                                if raw.bytes > MAX_TOOL_ROW_BYTES as u64 {
                                    return Err(ProjectionError::TooLarge.into());
                                }
                                Some(
                                    crate::tool_payloads::resolve_in_transaction(
                                        &mut tx,
                                        &fact.event,
                                    )
                                    .await?,
                                )
                            } else {
                                None
                            };
                            for mut row in view.push_with_tool_output(&fact, resolved.as_ref())? {
                                if fact.event.invocation.session_id != session {
                                    owned_references(&mut tx, &session, &mut row.message).await?;
                                }
                                persist(&mut tx, &session, row).await?;
                            }
                        }
                        processed = *sequence;
                    }
                    let next = if ready { fence } else { processed };
                    sqlx::query(
                        "INSERT INTO transcript_progress VALUES (?1, ?2)
             ON CONFLICT(session_id) DO UPDATE SET through_sequence = excluded.through_sequence",
                    )
                    .bind(&session)
                    .bind(next)
                    .execute(&mut *tx)
                    .await?;
                    tx.commit().await?;
                    Ok(ready)
                })
            })
            .await
    }
}

async fn owned_references(
    tx: &mut SqliteConnection,
    session: &str,
    message: &mut maka_presentation::Message,
) -> Result<(), StoreError> {
    use maka_agent_facts::attachment::StorageRef;
    use maka_presentation::{Content, ToolContent};
    let references: Vec<_> = match &mut message.content {
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
    for reference in references {
        if let StorageRef::SessionFile {
            session_id,
            relative_path,
        } = reference
        {
            *reference = crate::artifacts::history::resolve(tx, session, session_id, relative_path)
                .await?
                .ok_or(StoreError::TranscriptConflict)?;
        }
    }
    Ok(())
}

async fn persist(tx: &mut SqliteConnection, session: &str, row: Row) -> Result<(), StoreError> {
    let bytes = serde_json::to_vec(&row.message)?;
    let (source_sequence, item_index) = read::sql_position(row.position)?;
    let size = i64::try_from(bytes.len()).map_err(|_| StoreError::PrefixTooLarge)?;
    let digest = format!("sha256:{}", hex::encode(Sha256::digest(&bytes)));
    sqlx::query(
        "INSERT OR IGNORE INTO transcript_rows
         (source_sequence, item_index, session_id, turn_id, message_id, payload, digest, total_bytes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(source_sequence)
    .bind(item_index)
    .bind(session)
    .bind(&row.message.turn_id)
    .bind(&row.message.id)
    .bind(&bytes)
    .bind(&digest)
    .bind(size)
    .execute(&mut *tx)
    .await?;
    // Neither a conflicting identity nor a payload update is an idempotent replay.
    let matches: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM transcript_rows
         WHERE source_sequence = ?1 AND item_index = ?2 AND session_id = ?3 AND turn_id = ?4 AND message_id = ?5
         AND payload = ?6 AND digest = ?7 AND total_bytes = ?8)",
    )
    .bind(source_sequence)
    .bind(item_index)
    .bind(session)
    .bind(&row.message.turn_id)
    .bind(&row.message.id)
    .bind(&bytes)
    .bind(&digest)
    .bind(size)
    .fetch_one(&mut *tx)
    .await?;
    if !matches {
        return Err(StoreError::TranscriptConflict);
    }
    Ok(())
}
