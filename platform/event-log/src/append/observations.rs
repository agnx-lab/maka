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

//! Adjacent accepted observation requests share one durable receipt boundary.
//! Savepoints isolate domain rejection; database failures retire the original owner.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use maka_agent_facts::event::{CommitError, EventWrite};
use sqlx::{Connection, SqliteConnection};
use tokio::sync::{oneshot, watch};

use super::{AppendResult, commit_error};
use crate::{EventLog, StoreError};

pub(crate) struct Request {
    events: Vec<EventWrite>,
    commits: watch::Sender<u64>,
    reply: oneshot::Sender<Result<Vec<u64>, CommitError>>,
    failures: Arc<AtomicU64>,
    outcome: Outcome,
}

enum Outcome {
    Waiting,
    Writing,
    Prepared {
        sequences: Vec<u64>,
        inserted: Option<u64>,
    },
    Rejected(CommitError),
}

impl Request {
    pub(crate) fn new(
        events: Vec<EventWrite>,
        commits: watch::Sender<u64>,
        reply: oneshot::Sender<Result<Vec<u64>, CommitError>>,
        failures: Arc<AtomicU64>,
    ) -> Self {
        Self {
            events,
            commits,
            reply,
            failures,
            outcome: Outcome::Waiting,
        }
    }
}

struct Aborted {
    message: String,
    unknown: bool,
}

pub(crate) async fn commit(
    connection: &mut SqliteConnection,
    mut requests: Vec<Request>,
    validate: &(impl Fn() -> Result<(), StoreError> + Sync),
) -> bool {
    let result = transaction(connection, &mut requests, validate).await;
    for request in requests {
        let outcome = match request.outcome {
            Outcome::Rejected(error) => Err(error),
            Outcome::Prepared {
                sequences,
                inserted,
            } if result.is_ok() => {
                if let Some(sequence) = inserted {
                    request.commits.send_replace(sequence);
                }
                Ok(sequences)
            }
            state => {
                let failure = result.as_ref().expect_err("unprepared observation receipt");
                if failure.unknown && !matches!(state, Outcome::Waiting) {
                    Err(CommitError::OutcomeUnknown(failure.message.clone()))
                } else {
                    Err(CommitError::Rejected(failure.message.clone()))
                }
            }
        };
        if outcome.is_err() {
            request.failures.fetch_add(1, Ordering::AcqRel);
        }
        let _ = request.reply.send(outcome);
    }
    result.is_ok()
}

async fn transaction(
    connection: &mut SqliteConnection,
    requests: &mut [Request],
    validate: &(impl Fn() -> Result<(), StoreError> + Sync),
) -> Result<(), Aborted> {
    validate().map_err(|error| Aborted {
        message: error.to_string(),
        unknown: false,
    })?;
    let mut transaction = connection
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| Aborted {
            message: error.to_string(),
            unknown: false,
        })?;
    let prepared = prepare(&mut transaction, requests, validate).await;
    match prepared {
        Ok(true) => transaction.commit().await.map_err(|error| Aborted {
            message: error.to_string(),
            unknown: true,
        }),
        Ok(false) => transaction.rollback().await.map_err(|error| Aborted {
            message: error.to_string(),
            unknown: true,
        }),
        Err(error) => {
            // No commit was attempted. Only a confirmed full rollback establishes
            // rejection; a dropped transaction is never a rollback receipt.
            let rollback = transaction.rollback().await;
            Err(Aborted {
                message: match &rollback {
                    Ok(()) => error.to_string(),
                    Err(rollback) => format!("{error}; rollback unconfirmed: {rollback}"),
                },
                unknown: rollback.is_err(),
            })
        }
    }
}

async fn prepare(
    connection: &mut SqliteConnection,
    requests: &mut [Request],
    validate: &(impl Fn() -> Result<(), StoreError> + Sync),
) -> Result<bool, StoreError> {
    let mut inserted_any = false;
    for request in requests {
        validate()?;
        request.outcome = Outcome::Writing;
        // Raw savepoints deliberately do not add SQLx Transaction depth. Any
        // SQL/rollback/release failure abandons the whole group and this owner.
        sqlx::query("SAVEPOINT observation_request")
            .execute(&mut *connection)
            .await?;
        match prepare_request(connection, &request.events).await {
            Ok((sequences, inserted)) => {
                sqlx::query("RELEASE observation_request")
                    .execute(&mut *connection)
                    .await?;
                inserted_any |= inserted.is_some();
                request.outcome = Outcome::Prepared {
                    sequences,
                    inserted,
                };
            }
            Err(error) if isolated_rejection(&error) => {
                sqlx::query("ROLLBACK TO observation_request")
                    .execute(&mut *connection)
                    .await?;
                sqlx::query("RELEASE observation_request")
                    .execute(&mut *connection)
                    .await?;
                request.outcome = Outcome::Rejected(commit_error(error));
            }
            Err(error) => return Err(error),
        }
    }
    validate()?;
    Ok(inserted_any)
}

async fn prepare_request(
    connection: &mut SqliteConnection,
    events: &[EventWrite],
) -> Result<(Vec<u64>, Option<u64>), StoreError> {
    let mut verification = crate::context::cache::Verification::default();
    let mut sequences = Vec::with_capacity(events.len());
    let mut inserted = None;
    for event in events {
        sequences.push(
            match EventLog::append_verified(connection, event, &mut verification).await? {
                AppendResult::Existing(sequence) => sequence,
                AppendResult::Inserted(sequence) => {
                    inserted = Some(sequence);
                    sequence
                }
            },
        );
    }
    crate::context::validate_batch(connection, events).await?;
    Ok((sequences, inserted))
}

fn isolated_rejection(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::Sealed
            | StoreError::ImportedInvocation
            | StoreError::EventConflict
            | StoreError::SessionRetired
            | StoreError::PermissionRevoked
            | StoreError::InvalidTransition(_)
            | StoreError::HistoryTurnMissing
            | StoreError::HistoryTurnSplit
            | StoreError::HistoryUnsettled
            | StoreError::HistoryProviderEffectUnresolved
            | StoreError::MaterialCollected
    )
}

#[cfg(test)]
mod tests;
