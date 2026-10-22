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

//! A Session's Turns from its durable transcript: `session.turns.query`
//! and its Turn index, `session.turn_landmarks.query`.
//!
//! Source: `packages/runtime-host/src/protocol/session-turns.ts`
//! (`SESSION_TURNS_OPERATION_SPECS`, `decodeSessionTurnsQueryInput`,
//! `decodeSessionTurnsQueryResult`, `mergeSessionTurnContributions`,
//! `projectSessionTurnContribution`, `decodeSessionTurnLandmarksQueryInput`,
//! `decodeSessionTurnLandmarksQueryResult`). Maka Desktop reads every page
//! (`listSessionTurns` in apps/desktop/src/main/runtime-host-client.ts) and
//! forks a side chat through the latest completed Turn
//! (`latestSettledTurnId` in
//! apps/desktop/src/renderer/features/workbar/tools/side-chat/quote-companion-core.ts).
//! It reads the Turn index (`listTurnLandmarks` there) for the prompt rail,
//! once per Session while history lies outside the transcript it holds
//! (apps/desktop/src/renderer/features/conversation/controller/transcript-reading-position-controller.tsx).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{Operation, StoredMessage, TurnStatus};

/// `SESSION_TURN_QUERY_MAX_CONTRIBUTIONS`: the most contributions one page
/// asks for.
pub const SESSION_TURNS_QUERY_MAX_CONTRIBUTIONS: u32 = 128;

/// `SessionTurnsQueryInput` (exactly these four fields; `throughSequence`
/// is `null` on the first page, then the first page's watermark).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
#[non_exhaustive]
pub struct SessionTurnsQueryInput {
    pub session_id: String,
    pub through_sequence: Option<u64>,
    pub position: u64,
    /// 1 to [`SESSION_TURNS_QUERY_MAX_CONTRIBUTIONS`].
    pub max_contributions: u32,
}

impl SessionTurnsQueryInput {
    /// The first page of `session_id`'s Turns, as many as a page holds.
    pub fn first(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            through_sequence: None,
            position: 0,
            max_contributions: SESSION_TURNS_QUERY_MAX_CONTRIBUTIONS,
        }
    }

    /// The page after `page`, at the same watermark; `None` after the last.
    pub fn after(&self, page: &SessionTurnsQueryResult) -> Option<Self> {
        let position = page.next_position.filter(|next| *next > self.position)?;
        Some(Self { through_sequence: page.through_sequence, position, ..self.clone() })
    }
}

/// The Turn state a contribution saw last (`latestState`): a `turn_state`
/// row and its sequence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
#[non_exhaustive]
pub struct SessionTurnState {
    pub sequence: u64,
    /// Always a `turn_state` row on a well-formed page.
    pub message: StoredMessage,
}

impl SessionTurnState {
    /// The recorded status, when the row is a `turn_state`.
    pub fn status(&self) -> Option<&TurnStatus> {
        match &self.message {
            StoredMessage::TurnState(state) => Some(&state.status),
            _ => None,
        }
    }
}

/// `SessionTurnContribution`: what one page knows of a Turn. A Turn can
/// span pages; [`latest_completed_turn`] merges them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
#[non_exhaustive]
pub struct SessionTurnContribution {
    pub turn_id: String,
    pub first_sequence: u64,
    pub latest_state: Option<SessionTurnState>,
    /// At most 256 UTF-8 bytes.
    pub user_prompt_preview: Option<String>,
}

/// `SessionTurnsQueryResult`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
#[non_exhaustive]
pub struct SessionTurnsQueryResult {
    pub session_id: String,
    pub through_sequence: Option<u64>,
    pub contributions: Vec<SessionTurnContribution>,
    /// `null` on the last page.
    pub next_position: Option<u64>,
}

/// `session.turns.query` (mode `query`). Errors include `not_found`.
#[derive(Debug)]
pub enum SessionTurnsQuery {}

impl Operation for SessionTurnsQuery {
    const NAME: &'static str = "session.turns.query";
    type Input = SessionTurnsQueryInput;
    type Output = SessionTurnsQueryResult;
}

/// `SESSION_TURN_LANDMARK_MAX_ITEMS`: the most landmarks one query asks
/// for and one answer carries.
pub const SESSION_TURN_LANDMARK_MAX_ITEMS: u32 = 64;

/// `SessionTurnLandmarksQueryInput` (exactly these three fields; `turnId`
/// is `null` to sample the Session, as Desktop's
/// `listTurnLandmarks(sessionId, null)` asks with `maxLandmarks: 64`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
#[non_exhaustive]
pub struct SessionTurnLandmarksQueryInput {
    pub session_id: String,
    /// 1 to [`SESSION_TURN_LANDMARK_MAX_ITEMS`].
    pub max_landmarks: u32,
    /// Look up this one Turn instead of sampling the Session.
    pub turn_id: Option<String>,
}

impl SessionTurnLandmarksQueryInput {
    /// Up to [`SESSION_TURN_LANDMARK_MAX_ITEMS`] of `session_id`'s Turns,
    /// spread across the Session.
    pub fn sample(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            max_landmarks: SESSION_TURN_LANDMARK_MAX_ITEMS,
            turn_id: None,
        }
    }
}

/// `SessionTurnLandmark`: where one Turn lies in its Session and what its
/// prompt says. A sampled landmark always has a label: the Host leaves out
/// a Turn whose prompt is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
#[non_exhaustive]
pub struct SessionTurnLandmark {
    pub turn_id: String,
    /// No row of the Turn sits before this sequence.
    pub sequence: u64,
    /// Nor after this one.
    pub last_sequence: u64,
    /// The prompt (`displayText ?? text`), trimmed and cut to at most 96
    /// UTF-8 bytes.
    pub label: String,
}

/// `SessionTurnLandmarksQueryResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, serde(deny_unknown_fields))]
#[non_exhaustive]
pub struct SessionTurnLandmarksQueryResult {
    pub session_id: String,
    pub through_sequence: Option<u64>,
    /// Oldest first; at most [`SESSION_TURN_LANDMARK_MAX_ITEMS`].
    pub landmarks: Vec<SessionTurnLandmark>,
}

/// `session.turn_landmarks.query` (mode `query`). Errors include
/// `not_found`.
#[derive(Debug)]
pub enum SessionTurnLandmarksQuery {}

impl Operation for SessionTurnLandmarksQuery {
    const NAME: &'static str = "session.turn_landmarks.query";
    type Input = SessionTurnLandmarksQueryInput;
    type Output = SessionTurnLandmarksQueryResult;
}

/// The id of the latest Turn that completed, in the order the Turns began:
/// Desktop's `latestSettledTurnId` over `listSessionTurns`. Contributions
/// of one Turn merge as `mergeSessionTurnContributions` merges them (the
/// earliest first sequence, the state with the highest sequence); a Turn
/// whose ending is on no page has no status and does not count, as
/// `projectSessionTurnContribution` drops it. Failed, stopped and running
/// Turns do not count either: a side chat forked through one would read as
/// carrying on unfinished work.
pub fn latest_completed_turn(contributions: &[SessionTurnContribution]) -> Option<String> {
    let mut merged: HashMap<&str, (u64, Option<&SessionTurnState>)> = HashMap::new();
    for contribution in contributions {
        let entry = merged
            .entry(contribution.turn_id.as_str())
            .or_insert((contribution.first_sequence, None));
        entry.0 = entry.0.min(contribution.first_sequence);
        if let Some(state) = &contribution.latest_state
            && entry.1.is_none_or(|current| state.sequence > current.sequence)
        {
            entry.1 = Some(state);
        }
    }
    merged
        .into_iter()
        .filter(|(_, (_, state))| {
            state.and_then(SessionTurnState::status) == Some(&TurnStatus::Completed)
        })
        .max_by_key(|(_, (first, _))| *first)
        .map(|(turn_id, _)| turn_id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn contribution(turn_id: &str, first: u64, state: Option<(u64, &str)>) -> Value {
        json!({
            "turnId": turn_id, "firstSequence": first,
            "latestState": state.map(|(sequence, status)| json!({
                "sequence": sequence,
                "message": {"type": "turn_state", "id": format!("end-{turn_id}-{sequence}"),
                            "turnId": turn_id, "ts": 1, "status": status}
            })),
            "userPromptPreview": null
        })
    }

    fn decode(values: Vec<Value>) -> Vec<SessionTurnContribution> {
        values.into_iter().map(|value| serde_json::from_value(value).expect("decode")).collect()
    }

    #[test]
    fn the_latest_completed_turn_skips_failed_running_and_unfinished_ones() {
        let contributions = decode(vec![
            contribution("t1", 0, Some((3, "completed"))),
            contribution("t2", 4, Some((9, "completed"))),
            contribution("t3", 10, Some((12, "failed"))),
            contribution("t4", 13, Some((14, "running"))),
            contribution("t5", 15, None),
        ]);
        assert_eq!(latest_completed_turn(&contributions).as_deref(), Some("t2"));
        let none = decode(vec![contribution("t1", 0, Some((2, "aborted")))]);
        assert_eq!(latest_completed_turn(&none), None);
        assert_eq!(latest_completed_turn(&[]), None);
    }

    #[test]
    fn a_turn_split_across_pages_takes_its_latest_state() {
        // The first page saw t2 running; the second, its end.
        let contributions = decode(vec![
            contribution("t1", 0, Some((3, "completed"))),
            contribution("t2", 4, Some((5, "running"))),
            contribution("t2", 6, Some((8, "completed"))),
        ]);
        assert_eq!(latest_completed_turn(&contributions).as_deref(), Some("t2"));
        // A later page's older state does not replace a newer one.
        let contributions = decode(vec![
            contribution("t1", 0, Some((3, "completed"))),
            contribution("t2", 4, Some((8, "failed"))),
            contribution("t2", 4, Some((5, "completed"))),
        ]);
        assert_eq!(latest_completed_turn(&contributions).as_deref(), Some("t1"));
    }

    #[test]
    fn pages_follow_the_watermark_until_the_last() {
        let first = SessionTurnsQueryInput::first("s1");
        assert_eq!(
            serde_json::to_value(&first).expect("encode"),
            json!({"sessionId": "s1", "throughSequence": null, "position": 0,
                   "maxContributions": 128})
        );
        let page: SessionTurnsQueryResult = serde_json::from_value(json!({
            "sessionId": "s1", "throughSequence": 40, "contributions": [], "nextPosition": 128
        }))
        .expect("decode");
        let next = first.after(&page).expect("another page");
        assert_eq!((next.through_sequence, next.position), (Some(40), 128));
        let last: SessionTurnsQueryResult = serde_json::from_value(json!({
            "sessionId": "s1", "throughSequence": 40, "contributions": [], "nextPosition": null
        }))
        .expect("decode");
        assert_eq!(next.after(&last), None);
        // A position that does not move on ends the reading.
        let stuck: SessionTurnsQueryResult = serde_json::from_value(json!({
            "sessionId": "s1", "throughSequence": 40, "contributions": [], "nextPosition": 128
        }))
        .expect("decode");
        assert_eq!(next.after(&stuck), None);
    }

    // The landmark shapes below are the ones the Host's own tests produce:
    // packages/runtime-host/src/__tests__/session-turns.test.ts (a full
    // sampled index of 64, each label cut to 96 bytes) and
    // session-transcript-reader.test.ts (a sample of three prompts, and a
    // looked-up Turn whose prompt is empty).

    #[test]
    fn a_landmark_query_samples_the_session_or_looks_up_one_turn() {
        assert_eq!(
            serde_json::to_value(SessionTurnLandmarksQueryInput::sample("s1")).expect("encode"),
            json!({"sessionId": "s1", "maxLandmarks": 64, "turnId": null})
        );
        let lookup: SessionTurnLandmarksQueryInput = serde_json::from_value(
            json!({"sessionId": "s1", "maxLandmarks": 1, "turnId": "turn-first"}),
        )
        .expect("decode");
        assert_eq!(lookup.turn_id.as_deref(), Some("turn-first"));
    }

    #[test]
    fn landmark_answers_round_trip() {
        let sampled = json!({
            "sessionId": "session-1", "throughSequence": 47,
            "landmarks": [
                {"turnId": "turn-0", "sequence": 0, "lastSequence": 7, "label": "prompt 0"},
                {"turnId": "turn-2", "sequence": 16, "lastSequence": 23, "label": "prompt 2"},
                {"turnId": "turn-4", "sequence": 32, "lastSequence": 47, "label": "prompt 4"}
            ]
        });
        let result: SessionTurnLandmarksQueryResult =
            serde_json::from_value(sampled.clone()).expect("decode");
        assert_eq!(serde_json::to_value(&result).expect("encode"), sampled);
        let labels: Vec<&str> = result.landmarks.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(labels, ["prompt 0", "prompt 2", "prompt 4"]);

        let looked_up = json!({
            "sessionId": "session-1", "throughSequence": 55,
            "landmarks": [{"turnId": "turn-first", "sequence": 8, "lastSequence": 55, "label": ""}]
        });
        let result: SessionTurnLandmarksQueryResult =
            serde_json::from_value(looked_up.clone()).expect("decode");
        assert_eq!(serde_json::to_value(&result).expect("encode"), looked_up);
        assert_eq!((result.landmarks[0].sequence, result.landmarks[0].last_sequence), (8, 55));

        // An empty Session has no watermark and no landmarks.
        let empty = json!({"sessionId": "session-1", "throughSequence": null, "landmarks": []});
        let result: SessionTurnLandmarksQueryResult =
            serde_json::from_value(empty).expect("decode");
        assert_eq!((result.through_sequence, result.landmarks.len()), (None, 0));
    }

    #[test]
    fn a_full_index_takes_the_largest_sequences() {
        // `Number.MAX_SAFE_INTEGER - index`, the Host test's sequences.
        let max_safe = (1_u64 << 53) - 1;
        let landmarks: Vec<Value> = (0..u64::from(SESSION_TURN_LANDMARK_MAX_ITEMS))
            .map(|index| {
                json!({"turnId": format!("{index:t<128}"), "sequence": max_safe - index,
                       "lastSequence": max_safe - index, "label": "\u{0}".repeat(96)})
            })
            .collect();
        let full = json!({"sessionId": "session-1", "throughSequence": 1000,
                          "landmarks": landmarks});
        let result: SessionTurnLandmarksQueryResult =
            serde_json::from_value(full.clone()).expect("decode");
        assert_eq!(result.landmarks.len(), 64);
        assert_eq!(result.landmarks[63].sequence, max_safe - 63);
        assert_eq!(serde_json::to_value(&result).expect("encode"), full);
    }
}
