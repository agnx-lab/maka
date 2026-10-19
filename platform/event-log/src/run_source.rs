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

use crate::{EventLog, StoreError, sequence_number};
use futures_util::TryStreamExt;
use maka_agent_facts::configuration::validation::MAX_SAFE_INTEGER;
use maka_agent_facts::continuation::RunBoundary;
use maka_agent_facts::event::Fact;
use maka_agent_facts::event::RuntimeEvent;
use maka_agent_facts::event::StoredEvent;
use sha2::{Digest, Sha256};
use sqlx::{Connection, Row, SqliteConnection};

/// Canonical evidence for one physical Run cut, retaining only its endpoints.
/// Boundary high-water counts Run-local events; endpoint sequences are root positions.
#[derive(Debug)]
pub struct RunSource {
    pub boundary: RunBoundary,
    pub opening: StoredEvent,
    pub last: StoredEvent,
}

impl EventLog {
    /// Authenticate identity, the exact Run-local cut and original bytes in one snapshot.
    /// A requested cut must exist; it is never silently clamped to the latest event.
    /// An unsealed prefix is readable evidence, not proof that resuming it is safe.
    pub async fn run_source(
        &self,
        session: &str,
        run: &str,
        up_to: Option<u64>,
    ) -> Result<Option<RunSource>, StoreError> {
        self.validate_root()?;
        crate::sessions::validate_id(session)?;
        crate::sessions::validate_id(run)?;
        if up_to.is_some_and(|n| n == 0 || n > MAX_SAFE_INTEGER) {
            return Err(invalid("Run high-water is not a positive safe integer"));
        }
        let (session, run) = (session.to_owned(), run.to_owned());
        self.connection
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    let Some(invocation) =
                        crate::turns::run_invocation(&mut tx, &session, &run).await?
                    else {
                        return Ok(None);
                    };
                    let through = if let Some(up_to) = up_to {
                        sqlx::query_scalar::<_, i64>(
                            "SELECT sequence FROM runtime_events WHERE invocation_id = ?
                     ORDER BY sequence LIMIT 1 OFFSET ?",
                        )
                        .bind(&invocation)
                        .bind((up_to - 1) as i64)
                        .fetch_optional(&mut *tx)
                        .await?
                        .ok_or_else(|| invalid("requested Run prefix is unavailable"))?
                    } else {
                        sqlx::query_scalar::<_, i64>(
                            "SELECT MAX(sequence) FROM runtime_events WHERE invocation_id = ?",
                        )
                        .bind(&invocation)
                        .fetch_one(&mut *tx)
                        .await?
                    };
                    let source = read(&mut tx, &session, &run, &invocation, through).await?;
                    tx.commit().await?;
                    Ok(Some(source))
                })
            })
            .await
    }
}

pub(crate) async fn read(
    connection: &mut SqliteConnection,
    session: &str,
    run: &str,
    invocation_id: &str,
    through: i64,
) -> Result<RunSource, StoreError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM runtime_events WHERE invocation_id = ? AND sequence <= ?",
    )
    .bind(invocation_id)
    .bind(through)
    .fetch_one(&mut *connection)
    .await?;
    let high_water = sequence_number(count)?;
    if high_water == 0 || high_water > MAX_SAFE_INTEGER {
        return Err(invalid(
            "Run source high-water is not a positive safe integer",
        ));
    }
    let mut opening: Option<StoredEvent> = None;
    let mut last = None;
    let mut observed = 0u64;
    let mut digest = Sha256::new();
    digest.update(b"maka.run-prefix.v1\0");
    digest.update(high_water.to_be_bytes());
    let mut rows = sqlx::query(
        "SELECT sequence, event_json,
         (SELECT length(payload) FROM tool_result_payloads WHERE event_id = runtime_events.event_id)
         FROM runtime_events WHERE invocation_id = ? AND sequence <= ? ORDER BY sequence",
    )
    .bind(invocation_id)
    .bind(through)
    .fetch(&mut *connection);
    while let Some(row) = rows.try_next().await? {
        let sequence = sequence_number(row.try_get(0)?)?;
        let json: &str = row.try_get(1)?;
        let event: RuntimeEvent = serde_json::from_str(json)?;
        crate::tool_payloads::verify_binding(&event, row.try_get(2)?)?;
        match &opening {
            None => {
                if !matches!(event.fact, Fact::InvocationOpened { .. })
                    || event.invocation.session_id != session
                    || event.invocation.run_id != run
                    || event.invocation.invocation_id != invocation_id
                {
                    return Err(invalid("Run prefix lacks its exact canonical opening"));
                }
                let identity = serde_json::to_vec(&event.invocation)?;
                digest.update((identity.len() as u64).to_be_bytes());
                digest.update(identity);
            }
            Some(opening)
                if opening.event.invocation != event.invocation
                    || matches!(event.fact, Fact::InvocationOpened { .. }) =>
            {
                return Err(invalid("Run prefix identity changed"));
            }
            Some(_) => {}
        }
        digest.update(sequence.to_be_bytes());
        digest.update((json.len() as u64).to_be_bytes());
        digest.update(json.as_bytes());
        let stored = StoredEvent { sequence, event };
        if opening.is_none() {
            opening = Some(StoredEvent {
                sequence,
                event: stored.event.clone(),
            });
        }
        last = Some(stored);
        observed = observed
            .checked_add(1)
            .ok_or_else(|| invalid("Run source event count overflow"))?;
    }
    if observed != high_water {
        return Err(invalid(
            "Run source event count changed during authentication",
        ));
    }
    let opening = opening.ok_or_else(|| invalid("Run source is empty"))?;
    Ok(RunSource {
        boundary: RunBoundary {
            invocation: opening.event.invocation.clone(),
            high_water,
            digest: format!("sha256:{}", hex::encode(digest.finalize())),
        },
        opening,
        last: last.ok_or_else(|| invalid("Run source is empty"))?,
    })
}

fn invalid(reason: &str) -> StoreError {
    StoreError::InvalidTransition(reason.into())
}
