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

//! The prompt rail in the window: the task's conversation has one, with a
//! tick per prompt that scrolls the prompt to the top, and the strip and
//! the composer under it keep its reading column.

use base64::Engine as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Pixels, TestAppContext, px, size};
use serde_json::{Value, json};
use shared::domain_element_id;
use transcript_model::ItemKey;

use crate::chrome_tests::row;
use crate::tests::{EPOCH, Harness, ScriptedHost, session};

/// Task `s1`'s `subscription.open`: three settled turns in its tail, each
/// reply taller than the window.
fn open_with_three_turns() -> Value {
    let reply: String = (1..=40).map(|n| format!("Paragraph {n}.\n\n")).collect();
    let rows: Vec<Value> = (1..=3)
        .flat_map(|n| {
            let turn = format!("t{n}");
            [
                json!({"type": "user", "id": turn, "turnId": turn, "ts": 1,
                       "text": format!("Prompt {n}")}),
                json!({"type": "assistant", "id": format!("{turn}-a"), "turnId": turn, "ts": 2,
                       "text": reply, "contentOrder": ["text"], "modelId": "m"}),
                json!({"type": "turn_state", "id": format!("{turn}-end"), "turnId": turn,
                       "ts": 3, "status": "completed"}),
            ]
        })
        .collect();
    let fragments: Vec<Value> = rows
        .iter()
        .enumerate()
        .rev()
        .map(|(ix, row)| {
            let bytes = serde_json::to_vec(row).expect("row");
            json!({
                "sequence": ix + 1, "byteOffset": 0, "totalBytes": bytes.len(),
                "payloadDigest": null,
                "data": base64::engine::general_purpose::STANDARD.encode(&bytes)
            })
        })
        .collect();
    json!({
        "hostEpoch": EPOCH, "subscriptionId": "sub-s1", "nextSequence": 1,
        "snapshot": {
            "schemaVersion": 5,
            "session": {"sessionId": "s1", "metadataRevision": 1, "status": "active",
                        "createdAt": 1, "isArchived": false},
            "projectionRevision": 1, "rootTurn": null, "goal": null,
            "queue": {"hostEpoch": EPOCH, "queueRevision": 0, "steering": [], "followup": []},
            "interactions": {"pending": []}
        },
        "activeAssistantStreams": [],
        "transcript": {"durable": {
            "kind": "page", "sessionId": "s1", "direction": "older",
            "throughSequence": rows.len(), "rawBytes": 0, "fragments": fragments,
            "nextCursor": null, "endsAtTurnBoundary": true
        }}
    })
}

fn tick(turn: &str) -> gpui_kit::ElementId {
    domain_element_id("prompt-rail-tick", turn)
}

#[gpui_kit::test]
fn the_task_conversation_jumps_by_prompt(cx: &mut TestAppContext) {
    let transport = ScriptedHost::new(vec![session("s1", "Alpha", "/work/demo", "active")]);
    transport.reply("subscription.open", Ok(open_with_three_turns()));
    let harness = Harness::with_transport(transport, cx);
    harness.with_window(cx, |window, cx| window.click(row("s1"), cx));
    // The rail takes what a frame showed in the next.
    harness.with_window(cx, |_, _| {});
    harness.with_window(cx, |window, cx| {
        assert_eq!(window.find("prompt-rail").label(), Some("Jump by prompt"));
        for n in 1..=3 {
            let label = format!("Jump to prompt: Prompt {n}");
            assert_eq!(window.find(tick(&format!("t{n}"))).label(), Some(label.as_str()));
        }
        window.click(tick("t1"), cx);
    });
    let user = conversation::item_element_id("t1", &ItemKey::User("t1".into()));
    harness.with_window(cx, |window, _| {
        let transcript = window.find("conversation-transcript").bounds();
        let prompt = window.find(user).bounds();
        // At the list's top line: inside the 1 px border and 8 px padding.
        let gap = prompt.top() - transcript.top();
        assert!(gap > px(8.) && gap < px(10.), "Prompt 1 at the top: {gap:?}");
    });
    assert!(
        harness.transport.requests("session.turn_landmarks.query").is_empty(),
        "the tail holds every Turn"
    );
}

fn near(actual: Pixels, expected: Pixels, what: &str) {
    assert!((actual - expected).abs() < px(1.), "{what}: {actual:?}, expected {expected:?}");
}

#[gpui_kit::test]
fn the_strip_and_the_composer_keep_the_transcripts_column(cx: &mut TestAppContext) {
    let transport = ScriptedHost::new(vec![session("s1", "Alpha", "/work/demo", "active")]);
    transport.reply("subscription.open", Ok(open_with_three_turns()));
    let harness = Harness::with_transport(transport, cx);
    harness.with_window(cx, |window, cx| window.click(row("s1"), cx));
    // Prompt 1 in view, its row filling the column.
    harness.with_window(cx, |_, _| {});
    harness.with_window(cx, |window, cx| window.click(tick("t1"), cx));
    let user = conversation::item_element_id("t1", &ItemKey::User("t1".into()));
    // A wide pane centres the column; a narrower one keeps the rail's room
    // right of it; the narrowest has even gutters.
    for (width, rail) in [(1200., Some(false)), (760., Some(true)), (580., None)] {
        cx.simulate_window_resize(harness.window.into(), size(px(width), px(700.)));
        // The column takes what a frame measured in the next.
        harness.with_window(cx, |_, _| {});
        harness.with_window(cx, |_, _| {});
        harness.with_window(cx, |window, _| {
            let transcript = window.find("conversation-transcript").bounds();
            let column = window.find(user.clone()).bounds();
            let left = column.left() - transcript.left() - px(1.);
            let right = transcript.right() - px(1.) - column.right();
            match rail {
                Some(false) => near(left, right, &format!("centred at {width}")),
                Some(true) => {
                    near(left, px(24.), &format!("the left gutter at {width}"));
                    near(right, px(53.), &format!("the rail's room at {width}"));
                }
                None => {
                    near(left, px(24.), &format!("the left gutter at {width}"));
                    near(right, px(24.), &format!("the right gutter at {width}"));
                }
            }
            assert_eq!(window.try_find("prompt-rail").is_some(), rail.is_some(), "at {width}");
            for id in ["context-strip", "composer"] {
                let bounds = window.find(id).bounds();
                near(bounds.left(), column.left(), &format!("{id}'s left at {width}"));
                near(bounds.right(), column.right(), &format!("{id}'s right at {width}"));
            }
        });
    }
}
