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

//! Reduce exact source bytes on SQLite's worker, returning only digest cuts.
//! Serialized state is temporary, owner-local evidence, never a wire fact.

use crate::context::cache::Raw;
use crate::context::{invalid, selection::Selection};
use crate::{StoreError, sequence_number};
use rusqlite::{Connection, functions::FunctionFlags, params, types::ValueRef};
use sha2::{
    Digest, Sha256,
    digest::common::hazmat::{SerializableState, SerializedState},
};

pub(crate) fn register(connection: &Connection) -> Result<(), StoreError> {
    connection.create_scalar_function(
        "context_source_digests",
        5,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DIRECTONLY,
        |context| {
            let session: String = context.get(0)?;
            let lineage: Option<String> = context.get(1)?;
            let scope: Vec<u8> = context.get(2)?;
            let cuts: String = context.get(3)?;
            let cuts: Vec<u64> = serde_json::from_str(&cuts)
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            let raw: Option<String> = context.get(4)?;
            let raw: Option<Raw> = raw
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))?;
            // SAFETY: synchronous read-only use on SQLite's current worker and
            // transaction. No handle, row, or reference escapes the callback;
            // it never awaits, changes threads, or recursively invokes this UDF.
            let connection = unsafe { context.get_connection() }?;
            let result = match digests(
                &connection,
                &session,
                lineage.as_deref(),
                &scope,
                &cuts,
                raw.as_ref(),
            ) {
                Ok(digests) => Ok(digests),
                // Keep domain failures typed across the SQLx/UDF boundary.
                Err(StoreError::InvalidTransition(message)) => Err(message),
                Err(StoreError::Sqlite(error)) => return Err(error),
                Err(error) => return Err(rusqlite::Error::UserFunctionError(Box::new(error))),
            };
            serde_json::to_string(&result)
                .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(error)))
        },
    )?;
    Ok(())
}

fn digests(
    connection: &Connection,
    session: &str,
    lineage: Option<&str>,
    scope: &[u8],
    throughs: &[u64],
    raw: Option<&Raw>,
) -> Result<(Vec<String>, Raw), StoreError> {
    let through = *throughs
        .last()
        .ok_or_else(|| invalid("missing source cut"))?;
    let raw = raw.filter(|raw| throughs[0] >= raw.through);
    let after = raw.map_or(0, |raw| raw.through);
    let mut digest = if let Some(raw) = raw {
        let state = SerializedState::<Sha256>::try_from(raw.state.as_slice())
            .map_err(|_| invalid("invalid internal SHA state"))?;
        Sha256::deserialize(&state).map_err(|_| invalid("invalid internal SHA state"))?
    } else {
        let mut digest = Sha256::new();
        digest.update(b"maka.log-prefix.v1\0");
        digest.update((scope.len() as u64).to_be_bytes());
        digest.update(scope);
        digest
    };
    let mut boundaries = throughs.iter().copied().peekable();
    let mut result = Vec::new();
    let mut finish = |digest: &Sha256, sequence: u64| -> Result<(), StoreError> {
        if let Some(&boundary) = boundaries.peek() {
            if sequence > boundary {
                return Err(invalid("coverage is not a selected source high-water"));
            }
            if sequence == boundary {
                let mut prefix = digest.clone();
                prefix.update(boundary.to_be_bytes());
                result.push(format!("sha256:{}", hex::encode(prefix.finalize())));
                boundaries.next();
            }
        }
        Ok(())
    };
    finish(&digest, after)?;
    let filter = Selection::predicate("e", "?4");
    let payload = "length(CAST(e.event_json AS BLOB)),
        CASE WHEN length(CAST(e.event_json AS BLOB)) <= 65536 THEN CAST(e.event_json AS BLOB) END";
    let sql = format!(
        "SELECT e.sequence AS sequence, {payload} FROM runtime_events e
         WHERE e.sequence > ?1 AND e.sequence <= ?2
           AND json_extract(e.event_json,'$.invocation.session_id') = ?3 AND {filter}
         UNION ALL SELECT h.sequence AS sequence, {payload} FROM session_history_members h
         JOIN runtime_events e ON e.sequence = h.sequence
         WHERE h.session_id = ?3 AND h.sequence > ?1 AND h.sequence <= ?2 AND {filter}
         ORDER BY sequence"
    );
    let mut statement = connection.prepare(&sql)?;
    let mut slices = connection.prepare(
        "SELECT substr(CAST(event_json AS BLOB), ?, 65536) FROM runtime_events WHERE sequence = ?",
    )?;
    let mut previous = after as i64;
    let mut rows = statement.query(params![after as i64, through as i64, session, lineage])?;
    while let Some(row) = rows.next()? {
        let sequence: i64 = row.get(0)?;
        let bytes: i64 = row.get(1)?;
        if sequence <= previous {
            return Err(invalid("source selection repeats or reorders a record"));
        }
        let bytes = sequence_number(bytes)?;
        digest.update(sequence_number(sequence)?.to_be_bytes());
        digest.update(bytes.to_be_bytes());
        previous = sequence;
        match row.get_ref(2)? {
            ValueRef::Blob(payload) => digest.update(payload),
            ValueRef::Null => {
                // Nested reads stay on this worker and snapshot. Keep the
                // outer cursor so even oversized duplicate rows are checked.
                let mut offset = 0u64;
                while offset < bytes {
                    let mut parts = slices.query(params![(offset + 1) as i64, sequence])?;
                    let part = parts
                        .next()?
                        .ok_or_else(|| invalid("source bytes disappeared"))?;
                    let slice = part
                        .get_ref(0)?
                        .as_blob()
                        .map_err(|_| invalid("source bytes are not a BLOB"))?;
                    if slice.is_empty() {
                        return Err(invalid("source bytes disappeared"));
                    }
                    offset += slice.len() as u64;
                    digest.update(slice);
                }
            }
            _ => return Err(invalid("source bytes are not a BLOB")),
        }
        finish(&digest, sequence_number(sequence)?)?;
    }
    if boundaries.peek().is_some() {
        return Err(invalid("coverage is not a selected source high-water"));
    }
    Ok((
        result,
        Raw {
            through,
            state: digest.serialize().to_vec(),
        },
    ))
}
