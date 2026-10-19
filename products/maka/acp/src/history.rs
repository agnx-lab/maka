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

use crate::{Error, client::Client};
use maka_agent_api::contract::{HistoryChunk, HistoryPage, HistoryPosition};
use serde_json::json;
use std::collections::HashSet;

/// Freeze the original raw-event fence, including while the durable view prepares.
pub async fn read(
    client: &Client,
    session: &str,
    mut through: Option<u64>,
    after: Option<u64>,
) -> Result<Vec<HistoryChunk>, Error> {
    let mut position = after.map_or(HistoryPosition::Start, |sequence| HistoryPosition::Event {
        sequence: sequence + 1,
    });
    let mut history = Vec::new();
    let mut cursors = HashSet::new();
    loop {
        let page: HistoryPage = client.query("agent", "history", json!({"sessionId":session,"through":through,"position":position,"includeThinking":true})).await?;
        let HistoryPage::Ready {
            through: fence,
            chunks,
            after: next,
            ..
        } = page;
        freeze(&mut through, fence)?;
        history.extend(chunks);
        let Some(cursor) = next else {
            return Ok(history);
        };
        if !cursors.insert(serde_json::to_string(&cursor)?) {
            return Err("History cursor repeated".into());
        }
        position = HistoryPosition::After { cursor };
    }
}
fn freeze(through: &mut Option<u64>, fence: u64) -> Result<(), Error> {
    if through.is_some_and(|through| through != fence) {
        return Err("History fence changed during replay".into());
    }
    *through = Some(fence);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_pagination_keeps_the_original_fence() {
        let mut fence = None;
        freeze(&mut fence, 42).unwrap();
        freeze(&mut fence, 42).unwrap();
        assert!(freeze(&mut fence, 43).is_err());
        assert_eq!(fence, Some(42));
    }
}
