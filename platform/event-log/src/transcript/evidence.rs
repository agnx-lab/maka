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

use crate::{StoreError, sequence_number};
use maka_agent_facts::event::StoredEvent;
use maka_presentation::ProjectionError;
use rusqlite::{Connection as RawConnection, OptionalExtension, params};
use sqlx::SqliteConnection;

const HIGH: &str = r#"SELECT COALESCE(MAX(sequence), 0) FROM event_log"#;
const KIND: &str = r#"SELECT kind FROM runtime_events WHERE sequence = ? AND invocation_id = ?"#;
const EXTERNAL: &str = r#"SELECT EXISTS(SELECT 1 FROM runtime_events WHERE invocation_id = ?1 AND sequence <= ?2 AND kind = 'executor_started')"#;
const EXEC_BOUNDARY: &str = r#"SELECT sequence FROM runtime_events
             WHERE invocation_id = ?1 AND sequence <= ?2 AND kind = 'invocation_opened'
             UNION
             SELECT sequence FROM runtime_events
             WHERE invocation_id = ?1 AND sequence <= ?2 AND kind = 'executor_started'
             UNION
             SELECT sequence FROM runtime_events
             WHERE invocation_id = ?1 AND sequence <= ?2 AND ?3 AND sequence = ?2
             UNION
             SELECT sequence FROM runtime_events
             WHERE invocation_id = ?1 AND sequence <= ?2
               AND kind IN ('executor_observed','executor_completed')
               AND json_extract(event_json, '$.fact.output.toolCallId') =
                   (SELECT json_extract(event_json, '$.fact.output.toolCallId') FROM runtime_events WHERE sequence = ?2)
             ORDER BY sequence"#;
const EXEC_REST: &str = r#"SELECT sequence FROM runtime_events
             WHERE invocation_id = ?1 AND sequence <= ?2 AND (
                 kind IN ('invocation_opened','executor_started')
                 OR (?3 AND sequence = ?2)
                 OR (kind IN ('executor_observed','executor_completed') AND (
                     NOT ?3 OR NOT EXISTS(SELECT 1 FROM runtime_events boundary WHERE boundary.sequence = ?2 AND boundary.kind = 'executor_observed')
                     OR json_extract(event_json, '$.fact.output.toolCallId') =
                         (SELECT json_extract(event_json, '$.fact.output.toolCallId') FROM runtime_events WHERE sequence = ?2))))
             ORDER BY sequence"#;
const STEP: &str = r#"SELECT sequence FROM runtime_events
WHERE invocation_id = ?1 AND sequence <= ?3 AND kind = 'invocation_opened'
UNION
SELECT sequence FROM runtime_events
WHERE invocation_id = ?1 AND sequence <= ?3
  AND sequence IN (SELECT value FROM json_each(?4))
UNION
SELECT sequence FROM runtime_events
WHERE invocation_id = ?1 AND sequence <= ?3
  AND ?2 IS NOT NULL AND kind = 'model_requested' AND operation_id = ?2
UNION
SELECT sequence FROM runtime_events
WHERE invocation_id = ?1 AND sequence <= ?3
  AND ?5 AND ?2 IS NOT NULL AND kind = 'model_completed' AND operation_id = ?2
UNION
SELECT sequence FROM runtime_events
WHERE invocation_id = ?1 AND sequence <= ?3 AND ?2 IS NOT NULL
  AND kind = 'model_observed'
  AND CAST(json_extract(event_json, '$.fact.step_id') AS TEXT) = ?2
  AND json_extract(event_json, '$.fact.step_id') = ?2
ORDER BY sequence"#;
const FACT: &str = r#"SELECT event_json FROM runtime_events WHERE sequence = ?"#;
const PENDING: &str = r#"SELECT operation_id FROM runtime_events AS request
         WHERE invocation_id = ?1 AND sequence <= ?2 AND kind = 'model_requested'
           AND EXISTS(SELECT 1 FROM runtime_events opening
               WHERE opening.invocation_id = request.invocation_id AND opening.kind = 'invocation_opened'
               AND json_extract(opening.event_json, '$.fact.input.kind') IN ('message', 'continuation', 'handoff'))
           AND json_extract(request.event_json, '$.fact.purpose') = 'main'
           AND NOT EXISTS(SELECT 1 FROM runtime_events AS boundary
               WHERE boundary.invocation_id = request.invocation_id
                 AND boundary.operation_id = request.operation_id
                 AND boundary.kind IN ('model_completed', 'model_interrupted')
                 AND boundary.sequence <= ?2)
         ORDER BY sequence DESC LIMIT 1"#;

/// Live readers retain their original transaction and reducer boundaries.
/// Only evidence selection is delegated to the one synchronous policy core.
pub(super) async fn selected(
    tx: &mut SqliteConnection,
    invocation: &str,
    step: Option<&str>,
    through: i64,
    boundary: bool,
) -> Result<Vec<i64>, StoreError> {
    let invocation = invocation.to_owned();
    let step = step.map(str::to_owned);
    tx.run_on_worker(move |handle| {
        // SAFETY: the worker exclusively borrows its handle for this callback.
        // This non-owning wrapper/statements are destroyed before returning IDs.
        let db = unsafe { RawConnection::from_handle(handle.as_ptr()) }?;
        db.set_prepared_statement_cache_capacity(100);
        selected_on_handle(&db, &invocation, step.as_deref(), through, boundary)
    })
    .await?
}

pub(super) async fn fence(tx: &mut SqliteConnection, through: u64) -> Result<i64, StoreError> {
    let through = i64::try_from(through).map_err(|_| ProjectionError::OutOfRange)?;
    let high: i64 = sqlx::query_scalar(HIGH).fetch_one(tx).await?;
    checked_fence(through, high)
}

fn checked_fence(through: i64, high: i64) -> Result<i64, StoreError> {
    if through > high {
        return Err(StoreError::InvalidTransition(
            "transcript fence exceeds committed log".into(),
        ));
    }
    Ok(through)
}

pub(super) async fn fact(
    tx: &mut SqliteConnection,
    sequence: i64,
) -> Result<StoredEvent, StoreError> {
    let json: String = sqlx::query_scalar(FACT)
        .bind(sequence)
        .fetch_one(tx)
        .await?;
    decoded_fact(sequence, &json)
}

pub(super) fn fact_on_handle(db: &RawConnection, sequence: i64) -> Result<StoredEvent, StoreError> {
    let json: String = db
        .prepare_cached(FACT)?
        .query_row([sequence], |row| row.get(0))?;
    decoded_fact(sequence, &json)
}

fn decoded_fact(sequence: i64, json: &str) -> Result<StoredEvent, StoreError> {
    Ok(StoredEvent {
        sequence: sequence_number(sequence)?,
        event: serde_json::from_str(json)?,
    })
}

pub(super) async fn pending_step(
    tx: &mut SqliteConnection,
    invocation: &str,
    through: i64,
) -> Result<Option<String>, StoreError> {
    Ok(sqlx::query_scalar(PENDING)
        .bind(invocation)
        .bind(through)
        .fetch_optional(tx)
        .await?)
}

fn pending_step_on_handle(
    db: &RawConnection,
    invocation: &str,
    through: i64,
) -> Result<Option<String>, StoreError> {
    Ok(db
        .prepare_cached(PENDING)?
        .query_row(params![invocation, through], |row| row.get(0))
        .optional()?)
}

fn ids(
    db: &RawConnection,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<Vec<i64>, StoreError> {
    Ok(db
        .prepare_cached(sql)?
        .query_map(parameters, |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}
/// Select the actual opening, one step and its exact boundary evidence.
/// This is the only selection policy for both preparation and live readers.
pub(super) fn selected_on_handle(
    db: &RawConnection,
    invocation: &str,
    step: Option<&str>,
    through: i64,
    boundary: bool,
) -> Result<Vec<i64>, StoreError> {
    let mut executor_boundary = false;
    if boundary {
        let kind: String = db
            .prepare_cached(KIND)?
            .query_row(params![through, invocation], |row| row.get(0))?;
        executor_boundary = kind == "executor_observed";
        if matches!(
            kind.as_str(),
            "tool_dispatched" | "tool_rejected" | "tool_settled"
        ) {
            return super::tools::selected(db, invocation, through);
        }
    }
    let external: bool = db
        .prepare_cached(EXTERNAL)?
        .query_row(params![invocation, through], |row| row.get(0))?;
    if external {
        return ids(
            db,
            if executor_boundary {
                EXEC_BOUNDARY
            } else {
                EXEC_REST
            },
            params![invocation, through, boundary],
        );
    }
    let unresolved: Option<String> = if step.is_none() {
        pending_step_on_handle(db, invocation, through)?
    } else {
        None
    };
    let step = step.or(unresolved.as_deref());
    let boundaries = serde_json::to_string(if boundary {
        std::slice::from_ref(&through)
    } else {
        &[]
    })?;
    read_step(db, invocation, step, through, &boundaries, false)
}
/// UNION removes overlaps; CAST uses the index while original JSON equality
/// preserves malformed non-string step-ID selection semantics.
pub(super) fn read_step(
    db: &RawConnection,
    invocation: &str,
    step: Option<&str>,
    through: i64,
    boundaries: &str,
    completed: bool,
) -> Result<Vec<i64>, StoreError> {
    ids(
        db,
        STEP,
        params![invocation, step, through, boundaries, completed],
    )
}
