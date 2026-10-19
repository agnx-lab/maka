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

//! Optional bodies for selected directory rows. This never repairs metadata.
use super::read::sql_position;
use crate::StoreError;
use maka_agent_facts::history::RowPosition;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

pub(super) async fn ensure(
    db: &mut SqliteConnection,
    session: &str,
    position: RowPosition,
) -> Result<(), StoreError> {
    let (source, item) = sql_position(position)?;
    let (turn, message): (String, String) = sqlx::query_as(
        "SELECT turn_id,message_id FROM transcript_directory WHERE session_id=?1 AND source_sequence=?2 AND item_index=?3",
    ).bind(session).bind(source).bind(item).fetch_optional(&mut *db).await?
        .ok_or_else(|| StoreError::InvalidTransition("history row was not found".into()))?;
    let cached: Option<(String, String)> = sqlx::query_as(
        "SELECT turn_id,message_id FROM transcript_rows WHERE session_id=?1 AND source_sequence=?2 AND item_index=?3",
    ).bind(session).bind(source).bind(item).fetch_optional(&mut *db).await?;
    if let Some(identity) = cached {
        if identity != (turn, message) {
            return Err(StoreError::TranscriptConflict);
        }
        return Ok(());
    }
    // S is the cache unit: emission already owns every original row from this
    // selected producer. Retain these rows once, never replayed earlier sources.
    // Text remains demand-driven per visited row. A hit needs no sibling fill.
    let rows = super::emit::source(db, session, position.source_sequence).await?;
    if !rows.iter().any(|row| row.position == position) {
        return Err(StoreError::TranscriptConflict);
    }
    for row in rows {
        let (source, item) = sql_position(row.position)?;
        let identity: Option<(String,String)> = sqlx::query_as("SELECT turn_id,message_id FROM transcript_directory WHERE session_id=?1 AND source_sequence=?2 AND item_index=?3")
            .bind(session).bind(source).bind(item).fetch_optional(&mut *db).await?;
        if row.position.source_sequence != position.source_sequence
            || !matches!(identity,Some((turn,message)) if turn==row.message.turn_id && message==row.message.id)
        {
            return Err(StoreError::TranscriptConflict);
        }
        let payload = serde_json::to_vec(&row.message)?;
        let bytes = i64::try_from(payload.len()).map_err(|_| StoreError::PrefixTooLarge)?;
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(&payload)));
        sqlx::query("INSERT OR IGNORE INTO transcript_rows (session_id,source_sequence,item_index,turn_id,message_id,payload,digest,total_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)")
            .bind(session).bind(source).bind(item).bind(&row.message.turn_id).bind(&row.message.id).bind(&payload).bind(&digest).bind(bytes).execute(&mut *db).await?;
        let matches: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM transcript_rows WHERE session_id=?1 AND source_sequence=?2 AND item_index=?3 AND turn_id=?4 AND message_id=?5 AND payload=?6 AND digest=?7 AND total_bytes=?8)")
            .bind(session).bind(source).bind(item).bind(&row.message.turn_id).bind(&row.message.id).bind(payload).bind(digest).bind(bytes).fetch_one(&mut *db).await?;
        if !matches {
            return Err(StoreError::TranscriptConflict);
        }
    }
    Ok(())
}
