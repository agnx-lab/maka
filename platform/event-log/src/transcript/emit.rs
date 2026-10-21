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

//! Exact own-source rows from the existing presentation reducer.
use super::{evidence, read};
use crate::StoreError;
use maka_agent_facts::event::{Fact, ToolOutcome};
use maka_presentation::{InvocationView, Row};
use rusqlite::{Connection as RawConnection, OptionalExtension, params};
use sqlx::SqliteConnection;

pub(super) const ELIGIBLE: &str = r#"(e.kind = 'context_checkpoint_recorded' OR NOT EXISTS (SELECT 1 FROM runtime_events opening
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
                       OR (e.kind = 'model_observed'
                           AND json_extract(e.event_json, '$.fact.event.kind') IN ('part_finished','source','tool_call','provider_tool_result')
                           AND json_type(e.event_json, '$.fact.step_id') = 'text'
                           AND EXISTS (SELECT 1 FROM runtime_events request
                               JOIN runtime_events opening ON opening.invocation_id = request.invocation_id AND opening.kind = 'invocation_opened'
                               WHERE request.kind = 'model_requested' AND request.invocation_id = e.invocation_id
                                 AND request.sequence < e.sequence
                                 AND request.operation_id = CAST(json_extract(e.event_json, '$.fact.step_id') AS TEXT)
                                 AND json_extract(e.event_json, '$.fact.step_id') = request.operation_id
                                 AND json_extract(request.event_json, '$.fact.purpose') = 'main'
                                 AND json_extract(request.event_json, '$.fact.item_acceptance') = 1
                                 AND json_extract(opening.event_json, '$.fact.input.kind') IN ('message','continuation','handoff')))
                       OR (e.kind = 'executor_observed' AND json_extract(e.event_json, '$.fact.output.type') IN ('tool_start','tool_result')))"#;

pub(super) async fn source(
    connection: &mut SqliteConnection,
    session: &str,
    source: u64,
) -> Result<Vec<Row>, StoreError> {
    let sequence = read::sql_number(source)?;
    let session = session.to_owned();
    connection
        .run_on_worker(move |handle| {
            // SAFETY: exclusive worker-local borrow; no handle-backed values escape.
            let db = unsafe { RawConnection::from_handle(handle.as_ptr()) }?;
            db.set_prepared_statement_cache_capacity(100);
            source_on_handle(&db, &session, sequence)
        })
        .await?
}

pub(super) fn source_on_handle(
    db: &RawConnection,
    session: &str,
    source: i64,
) -> Result<Vec<Row>, StoreError> {
    let sql = format!(
        "SELECT e.invocation_id, CASE WHEN e.kind = 'model_observed' THEN json_extract(e.event_json, '$.fact.step_id') ELSE e.operation_id END, e.kind FROM runtime_events e WHERE e.sequence = ?1 AND (e.event_session = ?2 OR EXISTS(SELECT 1 FROM session_history_members h WHERE h.session_id = ?2 AND h.sequence = e.sequence)) AND {ELIGIBLE}"
    );
    let candidate: Option<(String, Option<String>, String)> = db
        .prepare_cached(&sql)?
        .query_row(params![source, session], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .optional()?;
    let Some((invocation, step, kind)) = candidate else {
        return Ok(Vec::new());
    };
    if kind == "context_checkpoint_recorded" {
        return Ok(vec![super::compaction::project(db, source)?]);
    }
    let ids = evidence::selected_on_handle(db, &invocation, step.as_deref(), source, true)?;
    let mut view = InvocationView::new();
    let mut own = Vec::new();
    for sequence in ids {
        let fact = evidence::fact_on_handle(db, sequence)?;
        let resolved = if let Fact::ToolSettled {
            outcome: ToolOutcome::Succeeded { .. },
            ..
        } = &fact.event.fact
        {
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
            if row.position.source_sequence == source as u64 {
                own.push(row);
            }
        }
    }
    Ok(own)
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
