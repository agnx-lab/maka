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
use sqlx::{Connection, SqliteConnection};

// The ordinary retained database is at most 32 MiB. One active producer may
// exceed that target so tiny fragment/sibling reads do not repeatedly emit it.
// This does not bound a complete operation's projection or live journal peak.
const RETAINED_PAGES: i64 = 8192;

pub(crate) async fn initialize(db: &mut SqliteConnection) -> Result<(), StoreError> {
    sqlx::raw_sql(
        "PRAGMA temp_store=FILE;
         ATTACH DATABASE '' AS transcript_cache;
         PRAGMA transcript_cache.page_size=4096;
         PRAGMA transcript_cache.auto_vacuum=FULL;
         PRAGMA transcript_cache.cache_size=-4096;
         PRAGMA transcript_cache.synchronous=OFF;
         PRAGMA transcript_cache.journal_mode=DELETE;
         CREATE TABLE transcript_cache.rows (
             session_id TEXT NOT NULL, source_sequence INTEGER NOT NULL CHECK(source_sequence>=1),
             item_index INTEGER NOT NULL CHECK(item_index>=0), turn_id TEXT NOT NULL,
             message_id TEXT NOT NULL, payload BLOB NOT NULL, digest TEXT NOT NULL,
             total_bytes INTEGER NOT NULL CHECK(total_bytes>0),
             PRIMARY KEY(session_id,source_sequence,item_index), UNIQUE(session_id,message_id)
         );
         CREATE TABLE transcript_cache.text (
             session_id TEXT NOT NULL, source_sequence INTEGER NOT NULL CHECK(source_sequence>=1),
             item_index INTEGER NOT NULL CHECK(item_index>=0),
             field TEXT NOT NULL CHECK(field IN ('body','thinking')),
             timestamp INTEGER NOT NULL, role TEXT NOT NULL, body BLOB NOT NULL,
             PRIMARY KEY(session_id,source_sequence,item_index,field)
         );
         CREATE TABLE transcript_cache.active_source (
             singleton INTEGER PRIMARY KEY CHECK(singleton=1),
             session_id TEXT NOT NULL, source_sequence INTEGER NOT NULL
         );",
    )
    .execute(&mut *db)
    .await?;
    Ok(())
}

/// Keep every ensured row pinned until the original operation has consumed it,
/// including final history boundaries. Accepted work stays on the writer owner.
pub(super) fn run<'c, T: Send + 'c>(
    db: &'c mut SqliteConnection,
    read: impl for<'a> FnOnce(
        &'a mut SqliteConnection,
    ) -> crate::connection::BoxFuture<'a, Result<T, StoreError>>
    + Send
    + 'c,
) -> crate::connection::BoxFuture<'c, Result<T, StoreError>> {
    Box::pin(async move {
        let mut tx = db.begin().await?;
        match read(&mut tx).await {
            Ok(value) => {
                tx.commit().await?;
                trim(db).await?;
                Ok(value)
            }
            Err(error) => {
                // Preserve the canonical read failure. No maintenance follows an
                // unsuccessful read or a rollback with an uncertain result.
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    })
}

async fn trim(db: &mut SqliteConnection) -> Result<(), StoreError> {
    let pages: i64 = sqlx::query_scalar("PRAGMA transcript_cache.page_count")
        .fetch_one(&mut *db)
        .await?;
    if pages <= RETAINED_PAGES {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    sqlx::raw_sql(
        "DELETE FROM transcript_cache.text WHERE NOT EXISTS (
             SELECT 1 FROM transcript_cache.active_source a
             WHERE a.session_id=text.session_id AND a.source_sequence=text.source_sequence
         );
         DELETE FROM transcript_cache.rows WHERE NOT EXISTS (
             SELECT 1 FROM transcript_cache.active_source a
             WHERE a.session_id=rows.session_id AND a.source_sequence=rows.source_sequence
         );",
    )
    .execute(&mut *tx)
    .await?;
    // FULL auto-vacuum reclaims free database pages at this commit, including
    // an opened temporary backing file. DELETE alone would retain high-water.
    tx.commit().await?;
    Ok(())
}

async fn retain_source(
    db: &mut SqliteConnection,
    session: &str,
    source: i64,
) -> Result<(), StoreError> {
    sqlx::query("INSERT INTO transcript_cache.active_source VALUES(1,?1,?2)
        ON CONFLICT(singleton) DO UPDATE SET session_id=excluded.session_id,source_sequence=excluded.source_sequence
        WHERE active_source.session_id!=excluded.session_id OR active_source.source_sequence!=excluded.source_sequence")
        .bind(session).bind(source).execute(db).await?;
    Ok(())
}

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
        "SELECT turn_id,message_id FROM transcript_cache.rows WHERE session_id=?1 AND source_sequence=?2 AND item_index=?3",
    ).bind(session).bind(source).bind(item).fetch_optional(&mut *db).await?;
    if let Some(identity) = cached {
        if identity != (turn, message) {
            return Err(StoreError::TranscriptConflict);
        }
        retain_source(db, session, source).await?;
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
        sqlx::query("INSERT OR IGNORE INTO transcript_cache.rows (session_id,source_sequence,item_index,turn_id,message_id,payload,digest,total_bytes) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)")
            .bind(session).bind(source).bind(item).bind(&row.message.turn_id).bind(&row.message.id).bind(&payload).bind(&digest).bind(bytes).execute(&mut *db).await?;
        let matches: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM transcript_cache.rows WHERE session_id=?1 AND source_sequence=?2 AND item_index=?3 AND turn_id=?4 AND message_id=?5 AND payload=?6 AND digest=?7 AND total_bytes=?8)")
            .bind(session).bind(source).bind(item).bind(&row.message.turn_id).bind(&row.message.id).bind(payload).bind(digest).bind(bytes).fetch_one(&mut *db).await?;
        if !matches {
            return Err(StoreError::TranscriptConflict);
        }
    }
    retain_source(db, session, source).await?;
    Ok(())
}

#[cfg(test)]
#[path = "bodycache_tests.rs"]
mod tests;
