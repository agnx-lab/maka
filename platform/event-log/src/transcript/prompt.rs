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

use crate::{EventLog, StoreError};
use maka_agent_facts::history::RowPosition;
use maka_agent_ports::session::history::Prompt;
use sqlx::{Connection, Row};

const QUERY: &str = "SELECT source_sequence, item_index,
    COALESCE(navigation_preview(COALESCE(json_extract(payload, '$.displayText'),
        json_extract(payload, '$.text'), ''), 4096), '[attachments]')
    FROM transcript_rows
    WHERE session_id = ?1 AND (source_sequence, item_index) <= (?2, ?3) AND json_extract(payload, '$.type') = 'user'
    ORDER BY source_sequence DESC, item_index DESC LIMIT 1";

impl EventLog {
    /// Inclusive reading anchor, fenced to an already prepared transcript.
    /// The partial index skips answer/tool bodies regardless of session size.
    pub async fn history_prompt(
        &self,
        session: &str,
        through: u64,
        position: RowPosition,
    ) -> Result<Option<Prompt>, StoreError> {
        self.validate_root()?;
        crate::sessions::validate_id(session)?;
        super::read::sql_number(through)?;
        if position.source_sequence > through {
            return Err(StoreError::InvalidTransition(
                "prompt anchor exceeds history fence".into(),
            ));
        }
        let (source_sequence, item_index) = super::read::sql_position(position)?;
        let session = session.to_owned();
        self.connection
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    super::read::prepared(&mut tx, &session, through).await?;
                    sqlx::query(QUERY)
                        .bind(session)
                        .bind(source_sequence)
                        .bind(item_index)
                        .fetch_optional(&mut *tx)
                        .await?
                        .map(|row| {
                            Ok(Prompt {
                                position: super::read::row_position(
                                    row.try_get(0)?,
                                    row.try_get(1)?,
                                )?,
                                text: row.try_get(2)?,
                            })
                        })
                        .transpose()
                })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{Connection, params};
    #[test]
    fn preceding_prompt_uses_index_and_does_not_read_other_sessions_or_later_prompts() {
        let db = Connection::open_in_memory().unwrap();
        super::super::navigation::register(&db).unwrap();
        db.execute_batch(
            "CREATE TABLE transcript_rows(session_id TEXT, source_sequence INTEGER, item_index INTEGER, payload BLOB);",
        )
        .unwrap();
        db.execute_batch("CREATE INDEX transcript_user_prompt ON transcript_rows (session_id, source_sequence, item_index) WHERE json_extract(payload, '$.type') = 'user';").unwrap();
        for (session, sequence, payload) in [
            (
                "a",
                1,
                serde_json::json!({"type":"user","text":"First question"}),
            ),
            (
                "a",
                2,
                serde_json::json!({"type":"assistant","text":"x".repeat(1024 * 1024)}),
            ),
            (
                "b",
                3,
                serde_json::json!({"type":"user","text":"Other session"}),
            ),
            ("a", 4, serde_json::json!({"type":"user","text":"?"})),
        ] {
            db.execute(
                "INSERT INTO transcript_rows VALUES(?1,?2,0,?3)",
                params![session, sequence, serde_json::to_vec(&payload).unwrap()],
            )
            .unwrap();
        }
        let plan: String = db
            .prepare(&format!("EXPLAIN QUERY PLAN {QUERY}"))
            .unwrap()
            .query_map(params!["a", 3, 0], |row| row.get::<_, String>(3))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            plan.contains("USING INDEX transcript_user_prompt"),
            "{plan}"
        );
        for (anchor, expected) in [(3, (1, "First question")), (4, (4, "?"))] {
            let actual: (i64, i64, String) = db
                .query_row(QUERY, params!["a", anchor, 0], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .unwrap();
            assert_eq!(actual, (expected.0, 0, expected.1.into()));
        }
        db.execute(
            "INSERT INTO transcript_rows VALUES('a',4,300,?1)",
            params![
                serde_json::to_vec(&serde_json::json!({"type":"user","text":"Later within fact"}))
                    .unwrap()
            ],
        )
        .unwrap();
        for (index, expected_index, expected_text) in
            [(299, 0, "?"), (300, 300, "Later within fact")]
        {
            let actual: (i64, i64, String) = db
                .query_row(QUERY, params!["a", 4, index], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
                })
                .unwrap();
            assert_eq!(actual, (4, expected_index, expected_text.into()));
        }
    }
}
