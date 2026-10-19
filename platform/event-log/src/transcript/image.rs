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

use super::read::{sql_number, sql_position};
use crate::{EventLog, StoreError, artifacts::ImageChunk};
use maka_agent_facts::attachment::StorageRef;
use maka_agent_facts::history::RowPosition;
use maka_agent_facts::tool_output::ImageOutput;
use sqlx::Connection;

impl EventLog {
    /// Resolve only the immutable image selected by this canonical row/fence.
    pub async fn tool_image(
        &self,
        session: &str,
        through: u64,
        position: RowPosition,
        offset: u64,
        limit: usize,
    ) -> Result<ImageChunk, StoreError> {
        self.validate_root()?;
        crate::sessions::validate_id(session)?;
        sql_number(through)?;
        if position.source_sequence > through || limit == 0 || limit > 32 * 1024 {
            return Err(invalid("invalid tool image window"));
        }
        let (source_sequence, item_index) = sql_position(position)?;
        sql_number(offset)?;
        let session = session.to_owned();
        self.connection
            .run(move |connection| {
                Box::pin(async move {
                    let mut tx = connection.begin().await?;
                    super::directory::check_read(&mut tx, &session, through).await?;
                    super::bodycache::ensure(&mut tx, &session, position).await?;
                    let raw: String = sqlx::query_scalar(
                        "SELECT json_remove(json_extract(payload, '$.content'), '$.kind')
                 FROM transcript_rows WHERE session_id = ?1 AND source_sequence = ?2 AND item_index = ?3
                 AND json_extract(payload, '$.type') = 'tool_result'
                 AND json_extract(payload, '$.content.kind') = 'image'
                 AND length(CAST(json_extract(payload, '$.content') AS BLOB)) <= 16384",
                    )
                    .bind(&session)
                    .bind(source_sequence)
                    .bind(item_index)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or_else(|| invalid("canonical tool image not found"))?;
                    let image: ImageOutput = serde_json::from_str(&raw)?;
                    let StorageRef::SessionFile {
                        session_id,
                        relative_path,
                    } = image.reference
                    else {
                        return Err(invalid("tool image has no immutable artifact"));
                    };
                    if session_id != session {
                        return Err(invalid("tool image belongs to another Session"));
                    }
                    let chunk = crate::artifacts::read_image(
                        &mut tx,
                        &session,
                        &relative_path,
                        &image.mime_type,
                        offset,
                        limit,
                        position.source_sequence,
                    )
                    .await?;
                    tx.commit().await?;
                    Ok(chunk)
                })
            })
            .await
    }
}
fn invalid(message: &str) -> StoreError {
    StoreError::InvalidTransition(message.into())
}
