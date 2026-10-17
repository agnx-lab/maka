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
use futures_util::TryStreamExt;
use maka_agent_facts::event::StoredEvent;
use maka_presentation::ProjectionError;
use sqlx::{Row, Sqlite, SqliteConnection, query::Query, sqlite::SqliteArguments};

pub(super) async fn fence(tx: &mut SqliteConnection, through: u64) -> Result<i64, StoreError> {
    let through = i64::try_from(through).map_err(|_| ProjectionError::OutOfRange)?;
    let high: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sequence), 0) FROM event_log")
        .fetch_one(tx)
        .await?;
    if through > high {
        return Err(StoreError::InvalidTransition(
            "transcript fence exceeds committed log".into(),
        ));
    }
    Ok(through)
}

/// Select actual opening plus exactly one model step or invocation boundary.
/// No fabricated facts and no deserialization of earlier completed steps.
pub(super) async fn selected(
    tx: &mut SqliteConnection,
    invocation: &str,
    step: Option<&str>,
    through: i64,
    boundary: bool,
) -> Result<Vec<i64>, StoreError> {
    let mut executor_boundary = false;
    if boundary {
        let kind: String = sqlx::query_scalar(
            "SELECT kind FROM runtime_events WHERE sequence = ? AND invocation_id = ?",
        )
        .bind(through)
        .bind(invocation)
        .fetch_one(&mut *tx)
        .await?;
        executor_boundary = kind == "executor_observed";
        if matches!(
            kind.as_str(),
            "tool_dispatched" | "tool_rejected" | "tool_settled"
        ) {
            return super::tools::selected(tx, invocation, through).await;
        }
    }
    let external: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM runtime_events WHERE invocation_id = ?1 AND sequence <= ?2 AND kind = 'executor_started')"
    ).bind(invocation).bind(through).fetch_one(&mut *tx).await?;
    if external {
        let sql = if executor_boundary {
            // A tool boundary needs only its own activity. Preserve SQLite's
            // native JSON equality, including malformed non-string IDs.
            "SELECT sequence FROM runtime_events
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
             ORDER BY sequence"
        } else {
            "SELECT sequence FROM runtime_events
             WHERE invocation_id = ?1 AND sequence <= ?2 AND (
                 kind IN ('invocation_opened','executor_started')
                 OR (?3 AND sequence = ?2)
                 OR (kind IN ('executor_observed','executor_completed') AND (
                     NOT ?3 OR NOT EXISTS(SELECT 1 FROM runtime_events boundary WHERE boundary.sequence = ?2 AND boundary.kind = 'executor_observed')
                     OR json_extract(event_json, '$.fact.output.toolCallId') =
                         (SELECT json_extract(event_json, '$.fact.output.toolCallId') FROM runtime_events WHERE sequence = ?2))))
             ORDER BY sequence"
        };
        return collect_ids(
            tx,
            sqlx::query(sql)
                .bind(invocation)
                .bind(through)
                .bind(boundary),
        )
        .await;
    }
    // A failed/cancelled terminal can seal a request without a separate model
    // interruption (for example after a model-observation commit failure).
    let unresolved = if step.is_none() {
        pending_step(tx, invocation, through).await?
    } else {
        None
    };
    let step = step.or(unresolved.as_deref());
    let ids = serde_json::to_string(if boundary {
        std::slice::from_ref(&through)
    } else {
        &[]
    })?;
    read_step(tx, invocation, step, through, &ids, false).await
}

/// Merge only the opening, selected model step, and explicit boundary/ancestry.
/// UNION removes overlapping identities before replay.
pub(super) async fn read_step(
    tx: &mut SqliteConnection,
    invocation: &str,
    step: Option<&str>,
    through: i64,
    boundary_ids: &str,
    include_completed: bool,
) -> Result<Vec<i64>, StoreError> {
    // CAST matches the observation index; the original equality also preserves
    // selection semantics for malformed non-string step IDs.
    let query = sqlx::query(
        "SELECT sequence FROM runtime_events
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
ORDER BY sequence",
    )
    .bind(invocation)
    .bind(step)
    .bind(through)
    .bind(boundary_ids)
    .bind(include_completed);
    collect_ids(tx, query).await
}

/// Retain only unique canonical IDs; payloads are hydrated one at a time.
/// Selection and replay remain in the caller's original transaction.
async fn collect_ids(
    tx: &mut SqliteConnection,
    query: Query<'_, Sqlite, SqliteArguments>,
) -> Result<Vec<i64>, StoreError> {
    let mut ids = Vec::new();
    let mut records = query.fetch(tx);
    while let Some(row) = records.try_next().await? {
        ids.push(row.try_get(0)?);
    }
    Ok(ids)
}

/// Finish this short PK read before the caller resolves raw payloads or persists
/// derived rows using the same transaction. The view still retains its step.
pub(super) async fn fact(
    tx: &mut SqliteConnection,
    sequence: i64,
) -> Result<StoredEvent, StoreError> {
    let json: String =
        sqlx::query_scalar("SELECT event_json FROM runtime_events WHERE sequence = ?")
            .bind(sequence)
            .fetch_one(tx)
            .await?;
    Ok(StoredEvent {
        sequence: sequence_number(sequence)?,
        event: serde_json::from_str(&json)?,
    })
}

pub(super) async fn pending_step(
    tx: &mut SqliteConnection,
    invocation: &str,
    through: i64,
) -> Result<Option<String>, StoreError> {
    Ok(sqlx::query_scalar(
        "SELECT operation_id FROM runtime_events AS request
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
         ORDER BY sequence DESC LIMIT 1",
    )
    .bind(invocation)
    .bind(through)
    .fetch_optional(tx)
    .await?)
}
