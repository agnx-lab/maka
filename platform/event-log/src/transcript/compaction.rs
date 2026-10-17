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
use maka_agent_facts::history::RowPosition;
use maka_presentation::{Content, Message, Row};
use sqlx::{Row as SqlRow, SqliteConnection};

/// Project only adoption metadata, without hydrating the private summary body.
pub(super) async fn project(tx: &mut SqliteConnection, sequence: i64) -> Result<Row, StoreError> {
    let row = sqlx::query(
        "SELECT c.event_id, json_extract(c.event_json, '$.invocation.turn_id') AS turn_id,
           catalog_time(json_extract(c.event_json, '$.recorded_at')) AS adopted_at,
           (SELECT catalog_time(json_extract(r.event_json, '$.recorded_at'))
            FROM runtime_events r
            WHERE r.invocation_id = c.invocation_id AND r.kind = 'model_requested'
              AND json_extract(r.event_json, '$.fact.purpose') = 'summary'
              AND r.sequence <= adopted.sequence
              AND json_extract(r.event_json, '$.fact.source_high_water') = json_extract(adopted.event_json, '$.fact.source_high_water')
              AND json_extract(r.event_json, '$.fact.source_digest') = json_extract(adopted.event_json, '$.fact.source_digest')
              AND NOT EXISTS (SELECT 1 FROM runtime_events boundary
                WHERE boundary.invocation_id = c.invocation_id AND boundary.sequence > r.sequence AND boundary.sequence < c.sequence
                  AND boundary.kind = 'context_checkpoint_recorded')
            ORDER BY r.sequence LIMIT 1) AS started_at
         FROM runtime_events c JOIN runtime_events adopted
           ON adopted.invocation_id = c.invocation_id AND adopted.kind = 'model_requested'
           AND adopted.operation_id = json_extract(c.event_json, '$.fact.checkpoint.summary_step_id')
         WHERE c.sequence = ? AND c.kind = 'context_checkpoint_recorded'"
    ).bind(sequence).fetch_one(tx).await?;
    let ts = sequence_number(row.try_get("adopted_at")?)?;
    let started = sequence_number(row.try_get("started_at")?)?;
    Ok(Row {
        position: RowPosition {
            source_sequence: sequence_number(sequence)?,
            item_index: 0,
        },
        message: Message {
            id: row.try_get("event_id")?,
            turn_id: row.try_get("turn_id")?,
            ts,
            content: Content::ContextCompacted {
                duration_ms: ts.saturating_sub(started),
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::Connection;
    use std::time::{Duration, UNIX_EPOCH};

    #[tokio::test]
    async fn concurrent_main_does_not_end_summary_timing_but_new_source_does() {
        for new_source in [false, true] {
            let mut db = SqliteConnection::connect(":memory:").await.unwrap();
            crate::sqlite_functions::register(&mut db).await.unwrap();
            sqlx::query("CREATE TABLE runtime_events(sequence INTEGER, invocation_id TEXT, kind TEXT, operation_id TEXT, event_id TEXT, event_json TEXT)").execute(&mut db).await.unwrap();
            for (sequence, at, kind, step, fact) in [
                (
                    1,
                    1000,
                    "model_requested",
                    "summary",
                    json!({"purpose":"summary","source_high_water":1,"source_digest":"source"}),
                ),
                (
                    2,
                    1500,
                    "model_requested",
                    "main",
                    json!({"purpose":"main","source_high_water":2,"source_digest":"main"}),
                ),
                (
                    3,
                    3000,
                    "model_requested",
                    "retry",
                    json!({"purpose":"summary","source_high_water":if new_source {2} else {1},"source_digest":if new_source {"new-source"} else {"source"}}),
                ),
                (
                    4,
                    5500,
                    "context_checkpoint_recorded",
                    "",
                    json!({"checkpoint":{"summary_step_id":"retry"}}),
                ),
            ] {
                let value = json!({"invocation":{"turn_id":"turn"}, "recorded_at":UNIX_EPOCH + Duration::from_millis(at), "fact":fact});
                sqlx::query("INSERT INTO runtime_events VALUES (?, 'invocation', ?, ?, ?, ?)")
                    .bind(sequence)
                    .bind(kind)
                    .bind(step)
                    .bind(format!("event-{sequence}"))
                    .bind(value.to_string())
                    .execute(&mut db)
                    .await
                    .unwrap();
            }
            let row = project(&mut db, 4).await.unwrap();
            assert_eq!(row.message.id, "event-4");
            assert_eq!(row.message.ts, 5500);
            assert_eq!(
                row.message.content,
                Content::ContextCompacted {
                    duration_ms: if new_source { 2500 } else { 4500 }
                }
            );
        }
    }
}
