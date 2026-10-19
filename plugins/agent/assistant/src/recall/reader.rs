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

use super::search::{History, Session};
use maka_agent_facts::tools::ToolError;
use maka_agent_ports::session::catalog;
use maka_agent_ports::session::history::Chunk;
use maka_agent_ports::session::history::Page;
use maka_agent_ports::session::history::Position;
use maka_agent_ports::session::history::Read;
use maka_capabilities::call::Scope;
use std::{collections::VecDeque, sync::Arc};

pub(super) struct Reader {
    history: Arc<dyn History>,
    call: Scope,
    input: Read,
    chunks: VecDeque<Chunk>,
    done: bool,
    assembling: Option<Chunk>,
}
impl Reader {
    pub fn new(
        history: Arc<dyn History>,
        call: Scope,
        session: String,
        through: Option<u64>,
    ) -> Self {
        Self {
            history,
            call,
            input: Read {
                include_thinking: false,
                session_id: session,
                through,
                position: Position::Start,
            },
            chunks: VecDeque::new(),
            done: false,
            assembling: None,
        }
    }
    pub fn through(&self) -> Option<u64> {
        self.input.through
    }

    pub async fn next(&mut self) -> Result<Option<Chunk>, ToolError> {
        loop {
            if self.call.cancellation.is_cancelled() {
                return Err(failed("Recall cancelled"));
            }
            if let Some(chunk) = self.chunks.pop_front() {
                if chunk.total_bytes > 64 * 1024 * 1024 {
                    return Err(failed(
                        "History message exceeds Recall's 64 MiB processing capacity",
                    ));
                }
                if let Some(current) = &mut self.assembling {
                    if current.position != chunk.position
                        || current.text.len() as u64 != chunk.offset
                        || current.total_bytes != chunk.total_bytes
                    {
                        return Err(failed("History continuation changed"));
                    }
                    current.text.push_str(&chunk.text);
                } else {
                    if chunk.offset != 0 {
                        return Err(failed("History message starts mid-text"));
                    }
                    self.assembling = Some(chunk);
                }
                if self
                    .assembling
                    .as_ref()
                    .is_some_and(|value| value.text.len() as u64 == value.total_bytes)
                {
                    return Ok(self.assembling.take());
                }
                continue;
            }
            if self.done {
                if self.assembling.is_some() {
                    return Err(failed("History ended before its message"));
                }
                return Ok(None);
            }
            let Page::Ready {
                through,
                chunks,
                after,
                ..
            } = self
                .history
                .read(self.call.clone(), self.input.clone())
                .await
                .map_err(failed)?;
            self.input.through = Some(through);
            if let Some(cursor) = after {
                self.input.position = Position::After { cursor };
            }
            self.done = after.is_none();
            self.chunks = chunks.into();

            tokio::task::yield_now().await;
        }
    }
}

pub(super) async fn sessions(
    history: &dyn History,
    call: &Scope,
    selected: Option<&str>,
) -> Result<(Vec<Session>, bool), ToolError> {
    let mut input = catalog::List {
        include_archived: true,
        ..Default::default()
    };
    let mut all = Vec::new();
    let mut total = 0;
    loop {
        let page = history
            .list(call.clone(), input.clone())
            .await
            .map_err(failed)?;
        for session in page.entries {
            if selected.is_none_or(|id| id == session.id) {
                total += 1;
                all.push(session);
            }
        }
        // Retain the same recent-Session ceiling as TS, without holding the full catalog.
        all.sort_by(|a, b| {
            b.last_message_at
                .cmp(&a.last_message_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        all.truncate(200);
        if page.next_cursor.is_none() {
            return Ok((all, total <= 200));
        }
        input.revision = Some(page.revision);
        input.cursor = page.next_cursor;
    }
}
pub(super) fn failed(error: impl std::fmt::Display) -> ToolError {
    ToolError::Failed(error.to_string())
}
