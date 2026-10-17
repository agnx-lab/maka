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

use super::selection::Selection;
use super::{SourceEvidence, invalid};
use crate::{StoreError, sequence_number};
use sqlx::SqliteConnection;

mod scan;
pub(crate) use scan::register;

/// The same scoped digest as complete prefix(), without retaining source rows.
/// Even a single old giant observation is hashed in bounded SQL BLOB slices.
pub(super) async fn evidence(
    connection: &mut SqliteConnection,
    session: &str,
    through: u64,
) -> Result<SourceEvidence, StoreError> {
    selected(connection, &Selection::session(session), through).await
}

pub(crate) async fn selected(
    connection: &mut SqliteConnection,
    selection: &Selection,
    through: u64,
) -> Result<SourceEvidence, StoreError> {
    let digest = digests(connection, selection, &[through])
        .await?
        .pop()
        .expect("one requested prefix");
    Ok(SourceEvidence {
        scope: selection.scope.clone(),
        high_water: through,
        digest,
    })
}

/// Hash one exact selection once for strictly increasing source boundaries.
/// Each boundary must select a complete row (or the empty prefix at zero).
/// Only hash states and digest strings are retained, never the source payloads.
pub(super) async fn digests(
    connection: &mut SqliteConnection,
    selection: &Selection,
    throughs: &[u64],
) -> Result<Vec<String>, StoreError> {
    Ok(scan(connection, selection, throughs, None).await?.0)
}

pub(super) async fn scan(
    connection: &mut SqliteConnection,
    selection: &Selection,
    throughs: &[u64],
    raw: Option<&super::cache::Raw>,
) -> Result<(Vec<String>, Option<super::cache::Raw>), StoreError> {
    if throughs.is_empty() {
        return Ok((Vec::new(), None));
    }
    let session = selection
        .session
        .as_deref()
        .ok_or_else(|| invalid("context requires a Session"))?;
    let result: String = sqlx::query_scalar("SELECT context_source_digests(?, ?, ?, ?, ?)")
        .bind(session)
        .bind(&selection.lineage)
        .bind(serde_json::to_vec(&selection.scope)?)
        .bind(serde_json::to_string(throughs)?)
        .bind(raw.map(serde_json::to_string).transpose()?)
        .fetch_one(connection)
        .await?;
    let (digests, raw) =
        serde_json::from_str::<Result<(Vec<String>, super::cache::Raw), String>>(&result)?
            .map_err(StoreError::InvalidTransition)?;
    Ok((digests, Some(raw)))
}

pub(super) async fn high_water(
    connection: &mut SqliteConnection,
    session: &str,
    before: i64,
) -> Result<u64, StoreError> {
    Selection::session(session)
        .high_water(connection, sequence_number(before.saturating_sub(1))?)
        .await
}

#[cfg(test)]
mod tests;
