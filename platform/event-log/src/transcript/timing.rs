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

use crate::StoreError;
use maka_agent_ports::session::history::TurnFinished;
use maka_agent_ports::session::history::TurnOutcome;
use maka_agent_ports::session::history::TurnTiming;
use sqlx::SqliteConnection;

// Two scalar index seeks, regardless of the size of a Turn's transcript. Start
// follows original sequence order, not the wall clock (which can move backward).
const SQL: &str = "WITH first AS (
    SELECT json_extract(payload, '$.ts') AS at FROM transcript_rows
    WHERE session_id = ?1 AND turn_id = ?2 AND source_sequence <= ?3
      AND json_extract(payload, '$.type') = 'turn_state'
      AND json_extract(payload, '$.status') = 'running'
    ORDER BY source_sequence, item_index LIMIT 1
), last AS (
    SELECT json_extract(payload, '$.ts') AS at,
        json_extract(payload, '$.status') AS status FROM transcript_rows
    WHERE session_id = ?1 AND turn_id = ?2 AND source_sequence <= ?3
      AND json_extract(payload, '$.type') = 'turn_state'
    ORDER BY source_sequence DESC, item_index DESC LIMIT 1
) SELECT first.at, last.at, last.status FROM first, last";

pub(super) async fn read(
    tx: &mut SqliteConnection,
    session: &str,
    turn: &str,
    through: i64,
) -> Result<Option<TurnTiming>, StoreError> {
    let row: Option<(Option<i64>, Option<i64>, String)> = sqlx::query_as(SQL)
        .bind(session)
        .bind(turn)
        .bind(through)
        .fetch_optional(tx)
        .await?;
    let Some((Some(start), Some(last), status)) = row else {
        return Ok(None);
    };
    const MAX: i64 = (1 << 53) - 1;
    if !(0..=MAX).contains(&start) || !(start..=MAX).contains(&last) {
        return Ok(None);
    }
    let outcome = match status.as_str() {
        "running" => None,
        "completed" => Some(TurnOutcome::Completed),
        "failed" => Some(TurnOutcome::Failed),
        "aborted" => Some(TurnOutcome::Aborted),
        _ => return Ok(None),
    };
    Ok(Some(TurnTiming {
        started_at: start as u64,
        finished: outcome.map(|outcome| TurnFinished {
            at: last as u64,
            outcome,
        }),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::{Connection, Row};

    #[tokio::test]
    async fn turn_boundaries_are_scoped_fenced_and_do_not_restart_at_handoff_or_tail() {
        let mut db = SqliteConnection::connect(":memory:").await.unwrap();
        sqlx::query("CREATE TABLE transcript_rows(session_id TEXT, turn_id TEXT, source_sequence INTEGER, item_index INTEGER, payload BLOB)")
            .execute(&mut db).await.unwrap();
        sqlx::raw_sql("CREATE INDEX transcript_turn_state ON transcript_rows (session_id, turn_id, source_sequence, item_index) WHERE json_extract(payload, '$.type') = 'turn_state';").execute(&mut db).await.unwrap();
        for (session, turn, sequence, status, at) in [
            ("a", "t", 1, "running", 1000),
            ("a", "t", 2, "running", 2000),
            ("a", "t", 3, "completed", 69000),
            ("b", "t", 1, "running", 900),
            ("b", "t", 3, "failed", 99000),
            ("a", "other", 1, "running", 500),
            ("a", "other", 3, "aborted", 90000),
            ("a", "missing", 3, "completed", 5000),
            ("a", "backward", 1, "running", 5000),
            ("a", "backward", 2, "completed", 4999),
        ] {
            sqlx::query("INSERT INTO transcript_rows VALUES (?,?,?,0,?)")
                .bind(session)
                .bind(turn)
                .bind(sequence)
                .bind(
                    serde_json::to_vec(&json!({"type":"turn_state","status":status,"ts":at}))
                        .unwrap(),
                )
                .execute(&mut db)
                .await
                .unwrap();
        }
        let active = read(&mut db, "a", "t", 2).await.unwrap().unwrap();
        assert_eq!(active.started_at, 1000);
        assert_eq!(active.finished, None);
        let done = read(&mut db, "a", "t", 3).await.unwrap().unwrap();
        assert_eq!(done.started_at, 1000);
        assert_eq!(
            done.finished,
            Some(TurnFinished {
                at: 69000,
                outcome: TurnOutcome::Completed
            })
        );
        assert_eq!(read(&mut db, "a", "t", 2).await.unwrap(), Some(active));
        for (session, turn, outcome) in [
            ("b", "t", TurnOutcome::Failed),
            ("a", "other", TurnOutcome::Aborted),
        ] {
            assert_eq!(
                read(&mut db, session, turn, 3)
                    .await
                    .unwrap()
                    .unwrap()
                    .finished
                    .unwrap()
                    .outcome,
                outcome
            );
        }
        for turn in ["absent", "missing", "backward"] {
            assert_eq!(read(&mut db, "a", turn, 3).await.unwrap(), None);
        }
        let sql = format!("EXPLAIN QUERY PLAN {SQL}");
        let plan = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind("a")
            .bind("t")
            .bind(3i64)
            .fetch_all(&mut db)
            .await
            .unwrap()
            .into_iter()
            .map(|row| row.get::<String, _>(3))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            plan.matches("USING INDEX transcript_turn_state").count(),
            2,
            "{plan}"
        );
    }
}
