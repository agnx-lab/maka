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

use super::{ArchiveError, accepted, target};
use crate::{
    StoreError,
    context::{SourceEvidence, selection::Selection},
};
use maka_agent_facts::archive::projection_digest;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;

pub(crate) async fn digest_selected(
    connection: &mut SqliteConnection,
    selection: &Selection,
    source: &SourceEvidence,
    before: u64,
) -> Result<String, StoreError> {
    let mut hash = Sha256::new();
    hash.update(b"maka.effective-context.v1\0");
    hash.update(source.digest.as_bytes());
    fold_selected(connection, selection, source.high_water, before, &mut hash).await?;
    Ok(format!("sha256:{}", hex::encode(hash.finalize())))
}

pub(crate) async fn validate_selected(
    connection: &mut SqliteConnection,
    selection: &Selection,
    high_water: u64,
    before: u64,
) -> Result<(), StoreError> {
    fold_selected(
        connection,
        selection,
        high_water,
        before,
        &mut Sha256::new(),
    )
    .await
}

async fn fold_selected(
    connection: &mut SqliteConnection,
    selection: &Selection,
    high_water: u64,
    before: u64,
    hash: &mut Sha256,
) -> Result<(), StoreError> {
    let session = selection.session.as_deref().ok_or(ArchiveError::Corrupt)?;
    let archive_filter = Selection::predicate("a", "?4");
    let target_filter = Selection::predicate("t", "?5");
    let pinned_filter = Selection::predicate("t", "?4");
    let broken: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS(SELECT 1 FROM runtime_events a LEFT JOIN runtime_events t
           ON t.event_id=json_extract(a.event_json,'$.fact.placeholder.identity.runtime_event_id')
         WHERE a.kind='tool_result_archived' AND a.sequence < ?1 AND json_extract(a.event_json,'$.invocation.session_id')=?2
           AND {archive_filter} AND (t.sequence IS NULL OR t.kind!='tool_settled'
             OR NOT EXISTS(SELECT 1 FROM session_history_events owned WHERE owned.owner_session_id=?3 AND owned.sequence=t.sequence)))
         OR EXISTS(SELECT 1 FROM session_history_members h
           JOIN runtime_events t ON t.sequence=h.sequence
           LEFT JOIN runtime_events a ON a.sequence=h.archive_sequence
           WHERE h.session_id=?2 AND h.sequence<=?5 AND h.archive_sequence IS NOT NULL AND {pinned_filter}
             AND (t.kind!='tool_settled' OR a.kind IS NOT 'tool_result_archived' OR a.sequence<=t.sequence
               OR json_extract(a.event_json,'$.fact.placeholder.identity.runtime_event_id') IS NOT t.event_id))"
    ))).bind(before as i64).bind(session).bind(session).bind(&selection.lineage)
        .bind(high_water as i64).fetch_one(&mut *connection).await?;
    if broken {
        return Err(ArchiveError::Corrupt.into());
    }
    let archive_filter = Selection::archive_predicate("t", "a", "?1", "?5");
    let branches = target::SOURCES.map(|source| format!(
        "SELECT t.sequence AS sequence,t.event_id,json_extract(a.event_json,'$.invocation.session_id'),a.sequence+1
         FROM ({source}) t CROSS JOIN runtime_events a
         WHERE a.kind='tool_result_archived'
           AND CAST(json_extract(a.event_json,'$.fact.placeholder.identity.runtime_event_id') AS TEXT)=t.event_id
           AND t.owner_session_id=?2 AND t.sequence>?3 AND t.sequence<=?4
           AND {archive_filter} AND {target_filter}"
    ));
    let query = format!(
        "{} ORDER BY sequence LIMIT 129",
        branches.join(" UNION ALL ")
    );
    let mut after = 0_i64;
    loop {
        let rows: Vec<(i64, String, String, i64)> =
            sqlx::query_as(sqlx::AssertSqlSafe(query.clone()))
                .bind(before as i64)
                .bind(session)
                .bind(after)
                .bind(high_water as i64)
                .bind(&selection.lineage)
                .fetch_all(&mut *connection)
                .await?;
        if rows.is_empty() {
            break;
        }
        // The extra row makes duplicate detection independent of page edges.
        if rows.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err(ArchiveError::Corrupt.into());
        }
        for (sequence, id, source_session, archive_before) in rows.into_iter().take(128) {
            if sequence <= after {
                return Err(ArchiveError::Corrupt.into());
            }
            let (_, placeholder) = accepted::find(
                connection,
                &source_session,
                &id,
                crate::sequence_number(archive_before)?,
            )
            .await?
            .ok_or(ArchiveError::Corrupt)?;
            let replacement = placeholder
                .to_model_projection()
                .map_err(|_| ArchiveError::Corrupt)?;
            let replacement_digest =
                projection_digest(&replacement).map_err(|_| ArchiveError::Corrupt)?;
            let encoded = serde_json::to_vec(&(sequence, &id, &placeholder, replacement_digest))?;
            hash.update((encoded.len() as u64).to_le_bytes());
            hash.update(encoded);
            after = sequence;
        }
    }
    Ok(())
}

pub(crate) async fn affected_after(
    connection: &mut SqliteConnection,
    selection: &Selection,
    coverage: u64,
    after: u64,
) -> Result<bool, StoreError> {
    let archive_filter = Selection::predicate("a", "?4");
    let target_filter = Selection::predicate("t", "?4");
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS(SELECT 1 FROM runtime_events a LEFT JOIN runtime_events t
         ON t.event_id=json_extract(a.event_json,'$.fact.placeholder.identity.runtime_event_id')
         WHERE a.kind='tool_result_archived' AND json_extract(a.event_json,'$.invocation.session_id')=?
           AND a.sequence > ? AND {archive_filter} AND (t.sequence IS NULL OR (t.sequence <= ?3 AND {target_filter})))"
    ))).bind(&selection.session).bind(after as i64).bind(coverage as i64).bind(&selection.lineage).fetch_one(connection).await?)
}

pub(crate) async fn validate_summary(
    connection: &mut SqliteConnection,
    source: &SourceEvidence,
    expected: &str,
    requested: u64,
) -> Result<(), StoreError> {
    let selection = Selection::resolve(connection, &source.scope).await?;
    if selection.session.is_none() {
        return Err(ArchiveError::Corrupt.into());
    }
    if affected_after(connection, &selection, source.high_water, requested).await? {
        return Err(StoreError::InvalidTransition(
            "summary effective source changed after its request".into(),
        ));
    }
    if digest_selected(connection, &selection, source, requested).await? == expected {
        Ok(())
    } else {
        Err(StoreError::InvalidTransition(
            "summary effective source digest mismatch".into(),
        ))
    }
}
