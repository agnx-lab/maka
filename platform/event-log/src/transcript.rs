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

//! Disposable message index; authority stays in the owner-typed canonical ledger.
mod active;
mod compaction;
pub(crate) use active::interrupted_messages;
mod evidence;
mod history;
mod image;
mod model;
pub mod navigation;
mod prepare;
mod prompt;
mod read;
pub mod search;
mod source;
mod timing;
mod tool_metadata;
mod tools;
use crate::{EventLog, StoreError};
pub use read::*;
use sqlx::Connection;

const MAX_BOUNDARIES: usize = 32;
const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;

impl EventLog {
    /// Prepare at most 32 immutable boundaries through a fixed raw-log fence.
    /// False means another bounded call is needed. Oversize/unsupported evidence
    /// is an error, never an indefinitely preparing state. No commit notice is
    /// emitted: these writes carry no execution facts.
    pub async fn prepare_transcript(
        &self,
        session: &str,
        through: u64,
        max_boundaries: usize,
    ) -> Result<bool, StoreError> {
        crate::sessions::validate_id(session)?;
        if !(1..=MAX_BOUNDARIES).contains(&max_boundaries) {
            return Err(StoreError::InvalidTransition(
                "invalid transcript boundary limit".into(),
            ));
        }
        read::sql_number(through)?;
        self.validate_root()?;
        let session = session.to_owned();
        self.connection
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    let (ready, write) = tx
                        .run_on_worker(move |handle| {
                            // SAFETY: exclusive worker-local borrow of the original handle.
                            // All temporary statements/wrapper drop before the owned result.
                            let borrowed =
                                unsafe { rusqlite::Connection::from_handle(handle.as_ptr()) }?;
                            borrowed.set_prepared_statement_cache_capacity(100);
                            prepare::batch(&borrowed, &session, through, max_boundaries)
                        })
                        .await??;
                    if write {
                        tx.commit().await?;
                    }
                    Ok(ready)
                })
            })
            .await
    }
}
