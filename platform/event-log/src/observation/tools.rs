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

use crate::{
    StoreError,
    turns::{InvocationState, TurnBoundary},
};
use sqlx::Row;

/// Original replay starts for active tool display, never effect authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolStreamSeed {
    pub tool_use_id: String,
    pub start_sequence: u64,
}

pub(super) async fn read(
    connection: &mut sqlx::SqliteConnection,
    root: Option<&TurnBoundary>,
) -> Result<Vec<ToolStreamSeed>, StoreError> {
    let Some(root) = root else {
        return Ok(Vec::new());
    };
    if matches!(root.state, InvocationState::Ended { .. })
        || matches!(
            root.input,
            maka_agent_facts::input::InvocationInput::ContextCompact { .. }
        )
    {
        return Ok(Vec::new());
    }
    // The caller's observation transaction owns the fence. Select identities
    // and replay starts only, never accumulated arguments or complete results.
    let rows = sqlx::query(SEEDS)
        .bind(&root.invocation.invocation_id)
        .fetch_all(connection)
        .await?;
    let mut result = Vec::new();
    for row in rows {
        let operation: String = row.try_get(1)?;
        result.push(ToolStreamSeed {
            tool_use_id: maka_agent_facts::tool_call::tool_use_id(
                &root.invocation.invocation_id,
                &operation,
            ),
            start_sequence: crate::sequence_number(row.try_get(0)?)?,
        });
    }
    Ok(result)
}

const SEEDS: &str = r#"
WITH starts AS (
    SELECT e.sequence, e.operation_id AS operation, NULL AS step, 'native' AS source
    FROM runtime_events e WHERE e.invocation_id = ?1 AND e.kind = 'tool_dispatched'
    UNION ALL
    SELECT e.sequence, json_extract(e.event_json,'$.fact.step_id') || ':' || json_extract(e.event_json,'$.fact.event.data.id'),
        json_extract(e.event_json,'$.fact.step_id'), 'model'
    FROM runtime_events e WHERE e.invocation_id = ?1 AND e.kind = 'model_observed'
      AND json_extract(e.event_json,'$.fact.event.kind') IN ('tool_input_started','tool_call')
      AND EXISTS (SELECT 1 FROM runtime_events request WHERE request.invocation_id=e.invocation_id
          AND request.kind='model_requested' AND request.operation_id=json_extract(e.event_json,'$.fact.step_id')
          AND json_extract(request.event_json,'$.fact.purpose')='main')
    UNION ALL
    SELECT e.sequence, json_extract(e.event_json,'$.fact.output.toolCallId'), NULL, 'executor'
    FROM runtime_events e WHERE e.invocation_id = ?1 AND e.kind='executor_observed'
      AND json_extract(e.event_json,'$.fact.output.type')='tool_start'
), active AS (
    SELECT operation, MIN(sequence) AS sequence FROM starts s
    WHERE (
        source IN ('native','model')
        AND NOT EXISTS (SELECT 1 FROM runtime_events done WHERE done.invocation_id=?1
            AND done.operation_id=s.operation AND done.kind IN ('tool_settled','tool_rejected'))
        AND (source='native' OR
            EXISTS (SELECT 1 FROM runtime_events admitted WHERE admitted.invocation_id=?1
                AND admitted.operation_id=s.operation AND admitted.kind='tool_dispatched') OR (
            NOT EXISTS (SELECT 1 FROM runtime_events result WHERE result.invocation_id=?1
                AND result.kind='model_observed' AND json_extract(result.event_json,'$.fact.event.kind')='provider_tool_result'
                AND json_extract(result.event_json,'$.fact.step_id') || ':' || json_extract(result.event_json,'$.fact.event.data.id')=s.operation)
            AND NOT EXISTS (SELECT 1 FROM runtime_events failed WHERE failed.invocation_id=?1
                AND failed.operation_id=s.step AND failed.kind='model_interrupted')
            AND (NOT EXISTS (SELECT 1 FROM runtime_events done WHERE done.invocation_id=?1
                    AND done.operation_id=s.step AND done.kind='model_completed') OR
                EXISTS (SELECT 1 FROM runtime_events ready WHERE ready.invocation_id=?1
                    AND ready.kind='model_observed' AND json_extract(ready.event_json,'$.fact.event.kind')='tool_call'
                    AND json_extract(ready.event_json,'$.fact.step_id') || ':' || json_extract(ready.event_json,'$.fact.event.data.id')=s.operation
                    AND NOT json_extract(ready.event_json,'$.fact.event.data.provider_executed')))
        ))
    ) OR (
        source='executor'
        AND NOT EXISTS (SELECT 1 FROM runtime_events done WHERE done.invocation_id=?1
            AND (done.kind='executor_completed' OR (done.kind='executor_observed'
                AND json_extract(done.event_json,'$.fact.output.type')='tool_result'
                AND json_extract(done.event_json,'$.fact.output.toolCallId')=s.operation)))
    )
    GROUP BY operation
)
SELECT sequence, operation FROM active ORDER BY sequence
"#;
