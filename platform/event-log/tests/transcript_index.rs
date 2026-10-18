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

use maka_agent_facts::event::Fact;
use maka_agent_facts::event::Invocation;
use maka_agent_facts::event::InvocationInput;
use maka_agent_facts::event::InvocationOutcome;
use maka_agent_facts::event::LogScope;
use maka_agent_facts::event::ModelInterruption;
use maka_agent_facts::event::RuntimeEvent;
use maka_agent_facts::history::RowPosition;
use maka_agent_facts::model::ModelEvent;
use maka_agent_facts::model::ModelPart;
use maka_agent_facts::model::ModelStep;
use maka_agent_facts::model::ModelUsage;
use maka_agent_facts::model::TextKind;
use maka_event_log::{EventLog, StoreError};
use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[path = "transcript_index/fixtures.rs"]
mod fixtures;
#[path = "transcript_index/history.rs"]
mod history;
#[path = "transcript_index/image.rs"]
mod image;
#[path = "transcript_index/interrupted_ancestry.rs"]
mod interrupted_ancestry;
#[path = "transcript_index/large_siblings.rs"]
mod large_siblings;
#[path = "transcript_index/observation_count.rs"]
mod observation_count;
#[path = "transcript_index/performance.rs"]
mod performance;
#[path = "transcript_index/performance_executor.rs"]
mod performance_executor;
#[path = "transcript_index/source.rs"]
mod source;
use fixtures::*;

#[tokio::test]
async fn completed_bytes_digest_identity_reopen_and_exact_rebuild() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("log.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    assert!(log.prepare_transcript("a", 0, 32).await.unwrap());
    assert!(
        log.transcript_headers(
            "a",
            &maka_event_log::transcript::TranscriptRead {
                through: 0,
                position: None,
                direction: maka_event_log::transcript::TranscriptDirection::Newer,
                limit: 1,
            }
        )
        .await
        .unwrap()
        .is_empty()
    );
    let (id, _) = partial(&log, "a", "hello😀").await;
    log.append(&event(
        "a",
        observe(
            "a",
            ModelEvent::PartFinished {
                id: "provider-part".into(),
                provider_options: None,
            },
        ),
    ))
    .await
    .unwrap();
    log.append(&event(
        "a",
        Fact::ModelCompleted {
            step_id: "step-a".into(),
            output: ModelStep {
                parts: vec![ModelPart::Text {
                    text_kind: TextKind::Text,
                    text: "hello😀".into(),
                    provider_options: None,
                }],
                finish_reason: maka_agent_facts::model::ModelFinishReason::Stop,
                usage: ModelUsage {
                    input_tokens: Some(9),
                    output_tokens: Some(3),
                    ..Default::default()
                },
                provider_options: None,
                response_id: None,
                model: None,
                timestamp: None,
            },
        },
    ))
    .await
    .unwrap();
    let through = end(&log, "a", InvocationOutcome::Completed).await;
    let wake = log.subscribe_commits();
    assert!(log.prepare_transcript("a", through, 32).await.unwrap());
    assert!(!wake.has_changed().unwrap());
    let db = Connection::open(&path).unwrap();
    let original = rows(&db, "a");
    assert_eq!(original.len(), 5);
    let assistant: Value = serde_json::from_slice(&original[2].1).unwrap();
    assert_eq!(assistant["id"], id);
    assert_eq!(assistant["modelId"], "real-model");
    for (_, bytes, digest, size) in &original {
        assert_eq!(*size, bytes.len() as i64);
        assert_eq!(
            *digest,
            format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
        );
    }
    // Derived progress may be discarded; replay must verify exactly the same rows.
    db.execute("DELETE FROM transcript_progress", []).unwrap();
    assert!(log.prepare_transcript("a", through, 32).await.unwrap());
    assert_eq!(rows(&db, "a"), original);
    log.close().await.unwrap();
    let reopened = EventLog::open(&path).await.unwrap();
    assert!(reopened.prepare_transcript("a", through, 32).await.unwrap());
    assert_eq!(rows(&db, "a"), original);
    // Corruption cannot silently replace an already published payload.
    db.execute(
        "UPDATE transcript_rows SET payload = X'00' WHERE source_sequence = ? AND item_index = ?",
        [
            original[1].0.source_sequence as i64,
            original[1].0.item_index as i64,
        ],
    )
    .unwrap();
    db.execute("DELETE FROM transcript_progress", []).unwrap();
    assert!(matches!(
        reopened.prepare_transcript("a", through, 32).await,
        Err(StoreError::TranscriptConflict)
    ));
    assert_eq!(progress(&db, "a"), 0);
}

#[tokio::test]
async fn interleaved_stream_preserves_order_and_excludes_other_session_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("log.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    let (id, _) = partial(&log, "a", "start\n").await;
    partial(&log, "b", "unrelated").await;
    let expected = format!(
        "start\n{}",
        (0..1024).map(|i| format!("{i}😀\n")).collect::<String>()
    );
    let events: Vec<_> = (0..1024)
        .flat_map(|i| {
            [("a", format!("{i}😀\n")), ("b", "not part of a".into())].map(|(session, text)| {
                event(
                    session,
                    observe(
                        session,
                        ModelEvent::PartDelta {
                            id: "provider-part".into(),
                            text,
                            provider_options: None,
                        },
                    ),
                )
            })
        })
        .collect();
    log.append_batch(&events).await.unwrap();
    let through = interrupt(&log, "a").await;
    assert!(log.prepare_transcript("a", through, 32).await.unwrap());

    let db = Connection::open(&path).unwrap();
    let projected = rows(&db, "a");
    assert_eq!(projected.len(), 3);
    let assistant: Value = serde_json::from_slice(&projected[2].1).unwrap();
    assert_eq!(assistant["id"], id);
    assert_eq!(assistant["text"], expected);
    assert_eq!(progress(&db, "a"), through as i64);

    let other = interrupt(&log, "b").await;
    assert!(log.prepare_transcript("b", other, 32).await.unwrap());
    let other_rows = rows(&db, "b");
    let assistant: Value = serde_json::from_slice(&other_rows[2].1).unwrap();
    assert_eq!(
        assistant["text"],
        format!("unrelated{}", "not part of a".repeat(1024))
    );
    assert_eq!(rows(&db, "a"), projected);
}

#[tokio::test]
async fn partial_freezes_at_interruption_and_old_fence_excludes_later_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("log.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    let (id, live) = partial(&log, "a", "partial😀").await;
    let interrupted = interrupt(&log, "a").await;
    let ended = end(
        &log,
        "a",
        InvocationOutcome::Cancelled {
            source: "user".into(),
        },
    )
    .await;
    let db = Connection::open(&path).unwrap();
    let invocation = event("a", opening()).event().invocation.clone();
    let overlay = log.active_transcript(&invocation, live).await.unwrap();
    assert_eq!(overlay.len(), 1);
    assert_eq!(overlay[0].id, id);
    assert!(
        log.active_transcript(&invocation, interrupted)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        log.active_transcript(&invocation, ended)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(log.prepare_transcript("a", live, 32).await.unwrap());
    let running = rows(&db, "a");
    assert_eq!(running.len(), 2);
    let state: Value = serde_json::from_slice(&running[1].1).unwrap();
    assert_eq!(state["type"], "turn_state");
    assert_eq!(
        state["status"], "running",
        "pending work is not interrupted"
    );
    assert_eq!(progress(&db, "a"), live as i64);
    assert!(log.prepare_transcript("a", interrupted, 32).await.unwrap());
    let frozen = rows(&db, "a");
    assert_eq!(frozen.len(), 3);
    let assistant: Value = serde_json::from_slice(&frozen[2].1).unwrap();
    assert_eq!(assistant["id"], id);
    assert_eq!(assistant["text"], "partial😀");
    assert!(log.prepare_transcript("a", ended, 32).await.unwrap());
    let final_rows = rows(&db, "a");
    assert_eq!(&final_rows[..3], frozen.as_slice());
    assert_eq!(final_rows.len(), 4); // no invented unknown usage
    let terminal: Value = serde_json::from_slice(&final_rows[3].1).unwrap();
    assert_eq!(terminal["status"], "aborted");
}

#[tokio::test]
async fn bounded_progress_and_session_isolation() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("log.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.append(&event("a", opening())).await.unwrap();
    log.append(&event("b", opening())).await.unwrap();
    let a_end = end(&log, "a", InvocationOutcome::Completed).await;
    let through = end(&log, "b", InvocationOutcome::Completed).await;
    let db = Connection::open(&path).unwrap();
    assert!(!log.prepare_transcript("a", through, 1).await.unwrap());
    assert_eq!(progress(&db, "a"), 1);
    assert!(log.prepare_transcript("a", through, 1).await.unwrap());
    assert_eq!(progress(&db, "a"), through as i64);
    assert_eq!(
        rows(&db, "a").last().unwrap().0,
        RowPosition {
            source_sequence: a_end,
            item_index: 0
        }
    );
    assert!(rows(&db, "b").is_empty());
    assert!(log.prepare_transcript("a", through + 1, 1).await.is_err());
    assert!(log.prepare_transcript("a", through, 33).await.is_err());
}

#[tokio::test]
async fn terminal_without_model_interruption_seals_only_committed_partial_evidence() {
    for outcome in [
        InvocationOutcome::Failed {
            model_failure: None,
            class: "event_commit".into(),
            message: None,
        },
        InvocationOutcome::Cancelled {
            source: "shutdown".into(),
        },
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("log.sqlite");
        let log = EventLog::open(&path).await.unwrap();
        let (id, live) = partial(&log, "a", "committed😀 prefix").await;
        let invocation = event("a", opening()).event().invocation.clone();
        let overlay = log.active_transcript(&invocation, live).await.unwrap();
        assert!(log.prepare_transcript("a", live, 32).await.unwrap());
        // A persistence fault may prevent ModelInterrupted while a later
        // terminal append succeeds. The source facts allow this boundary.
        let through = end(&log, "a", outcome.clone()).await;
        assert!(log.prepare_transcript("a", through, 32).await.unwrap());
        assert!(
            log.active_transcript(&invocation, through)
                .await
                .unwrap()
                .is_empty()
        );
        let db = Connection::open(&path).unwrap();
        let original = rows(&db, "a");
        assert_eq!(original.len(), 4); // user, running, factual partial, terminal; no usage
        let assistant: Value = serde_json::from_slice(&original[2].1).unwrap();
        assert_eq!(assistant["id"], id);
        let failed = matches!(outcome, InvocationOutcome::Failed { .. });
        let mut expected = serde_json::to_value(&overlay[0]).unwrap();
        if failed {
            expected["interrupted"] = Value::Bool(true);
        }
        assert_eq!(assistant, expected);
        let delivery = log
            .session_stream_events("a", live, through, 8, 4096)
            .await
            .unwrap();
        assert!(matches!(&delivery.events[0].fact,
            maka_event_log::observation::StreamFact::InvocationEnded { interrupted, .. }
            if if failed { interrupted == std::slice::from_ref(&id) } else { interrupted.is_empty() }));
        assert_eq!(
            original[2].0,
            RowPosition {
                source_sequence: through,
                item_index: 0
            }
        );
        let terminal: Value = serde_json::from_slice(&original[3].1).unwrap();
        assert_eq!(
            terminal["status"],
            if matches!(outcome, InvocationOutcome::Failed { .. }) {
                "failed"
            } else {
                "aborted"
            }
        );
        // Index reset and process reopen must not drop or relabel the prefix.
        db.execute("DELETE FROM transcript_progress", []).unwrap();
        log.close().await.unwrap();
        let reopened = EventLog::open(&path).await.unwrap();
        assert!(reopened.prepare_transcript("a", through, 32).await.unwrap());
        assert_eq!(rows(&db, "a"), original);
        assert_eq!(
            reopened.active_transcript(&invocation, live).await.unwrap(),
            overlay
        );
    }
}

#[tokio::test]
async fn oversized_text_and_invalid_tool_fail_without_committing_progress() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("log.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    partial(&log, "a", &"x".repeat(16 * 1024 * 1024)).await;
    let through = interrupt(&log, "a").await;
    let db = Connection::open(&path).unwrap();
    assert!(matches!(
        log.prepare_transcript("a", through, 32).await,
        Err(StoreError::Projection(
            maka_presentation::ProjectionError::TooLarge
        ))
    ));
    assert_eq!(progress(&db, "a"), 0);
    assert!(rows(&db, "a").is_empty());
    log.append(&event("b", opening())).await.unwrap();
    let through = log
        .append(&event(
            "b",
            Fact::ToolDispatched {
                presentation: None,
                title: None,
                operation_id: "tool".into(),
                call: maka_agent_facts::tool_call::ToolCallIdentity::standalone("tool-call".into()),
                name: "file".into(),
                input: Value::Null,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        log.prepare_transcript("b", through, 32).await,
        Err(StoreError::Projection(_))
    ));
    assert_eq!(progress(&db, "b"), 0);
    assert!(rows(&db, "b").is_empty());
}

#[tokio::test]
async fn completion_fanout_positions_preserve_usage_fences_sources_and_cold_rebuild() {
    use maka_agent_facts::model::ModelToolCall;
    use maka_agent_ports::session::history::{Page, Position, SourceField, SourcePosition};
    use maka_event_log::transcript::{TranscriptDirection, TranscriptRead};

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("fanout.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.create_session("fanout", "create", &serde_json::json!({}), 1)
        .await
        .unwrap();
    let opening_fact = log.append(&event("fanout", opening())).await.unwrap();
    log.append(&event("fanout", request("fanout")))
        .await
        .unwrap();
    let calls: Vec<_> = (0..256)
        .map(|index| ModelToolCall {
            id: format!("call-{index}"),
            name: "Read".into(),
            input: serde_json::json!({"index": index, "value": "last😀"}).into(),
            provider_options: None,
            provider_executed: false,
        })
        .collect();
    let observed: Vec<_> = calls
        .iter()
        .map(|call| {
            event(
                "fanout",
                observe("fanout", ModelEvent::ToolCall(call.clone())),
            )
        })
        .collect();
    let old_fence = *log.append_batch(&observed).await.unwrap().last().unwrap();
    let completion = log
        .append(&event(
            "fanout",
            Fact::ModelCompleted {
                step_id: "step-fanout".into(),
                output: ModelStep {
                    parts: calls
                        .iter()
                        .cloned()
                        .map(|call| ModelPart::ToolCall { call })
                        .collect(),
                    finish_reason: maka_agent_facts::model::ModelFinishReason::ToolCalls,
                    usage: ModelUsage {
                        input_tokens: Some(7),
                        output_tokens: Some(11),
                        ..Default::default()
                    },
                    provider_options: None,
                    response_id: None,
                    model: None,
                    timestamp: None,
                },
            },
        ))
        .await
        .unwrap();
    let neighboring = log
        .append(&event(
            "fanout",
            Fact::ToolDispatched {
                presentation: None,
                title: None,
                operation_id: "step-fanout:call-255".into(),
                call: maka_agent_facts::tool_call::ToolCallIdentity::provider(
                    "step-fanout".into(),
                    "call-255".into(),
                ),
                name: "Read".into(),
                input: serde_json::json!({"index":255,"value":"last😀"}),
            },
        ))
        .await
        .unwrap();
    assert_eq!(neighboring, completion + 1);
    let template = event(
        "fanout",
        Fact::InvocationEnded {
            outcome: InvocationOutcome::Completed,
        },
    );
    let success = maka_agent_facts::event::EventWrite::tool_success(
        template.event().id.clone(),
        template.event().recorded_at,
        template.event().invocation.clone(),
        "step-fanout:call-255".into(),
        serde_json::json!({"answer":"literal😀result"}).into(),
    )
    .unwrap()
    .0;
    let result_fact = log.append(&success).await.unwrap();
    let through = end(&log, "fanout", InvocationOutcome::Completed).await;
    while !log.prepare_transcript("fanout", through, 1).await.unwrap() {}
    let db = Connection::open(&path).unwrap();
    let original = rows(&db, "fanout");
    let completed: Vec<_> = original
        .iter()
        .filter(|row| row.0.source_sequence == completion)
        .collect();
    assert_eq!(completed.len(), 257);
    for (index, row) in completed.iter().enumerate() {
        assert_eq!(
            row.0,
            RowPosition {
                source_sequence: completion,
                item_index: index as u64
            }
        );
    }
    let usage: Value = serde_json::from_slice(&completed[256].1).unwrap();
    assert_eq!(usage["type"], "token_usage");
    assert_eq!(usage["input"], 7);
    assert_eq!(usage["output"], 11);
    let next = original
        .iter()
        .find(|row| row.0.source_sequence == neighboring)
        .unwrap();
    assert_eq!(
        next.0,
        RowPosition {
            source_sequence: neighboring,
            item_index: 0
        }
    );
    assert!(completed[256].0 < next.0);
    let old = log
        .transcript_headers(
            "fanout",
            &TranscriptRead {
                through: old_fence,
                position: None,
                direction: TranscriptDirection::Newer,
                limit: 256,
            },
        )
        .await
        .unwrap();
    assert_eq!(old.len(), 2);
    assert!(
        old.iter()
            .all(|row| row.position.source_sequence <= old_fence)
    );
    let anchored = log
        .transcript_headers(
            "fanout",
            &TranscriptRead {
                through,
                position: Some(RowPosition {
                    source_sequence: completion,
                    item_index: 255,
                }),
                direction: TranscriptDirection::Older,
                limit: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        anchored.iter().map(|row| row.position).collect::<Vec<_>>(),
        vec![
            RowPosition {
                source_sequence: completion,
                item_index: 255
            },
            RowPosition {
                source_sequence: completion,
                item_index: 254
            },
        ]
    );
    let beyond_fence = log
        .transcript_headers(
            "fanout",
            &TranscriptRead {
                through: old_fence,
                position: Some(RowPosition {
                    source_sequence: completion,
                    item_index: 255,
                }),
                direction: TranscriptDirection::Older,
                limit: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        beyond_fence
            .iter()
            .map(|row| row.position)
            .collect::<Vec<_>>(),
        vec![
            RowPosition {
                source_sequence: opening_fact,
                item_index: 1
            },
            RowPosition {
                source_sequence: opening_fact,
                item_index: 0
            },
        ]
    );
    let first = log
        .history_text(
            "fanout",
            through,
            Position::Event {
                sequence: completion,
            },
            false,
            8192,
            1,
            65536,
        )
        .await
        .unwrap();
    let Page::Ready { chunks, .. } = first else {
        panic!("prepared")
    };
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        chunks[0].position,
        RowPosition {
            source_sequence: completion,
            item_index: 0
        }
    );
    let call_position = RowPosition {
        source_sequence: completion,
        item_index: 255,
    };
    let result_position = RowPosition {
        source_sequence: result_fact,
        item_index: 0,
    };
    let tool_id = maka_presentation::tool_message_id("inv-fanout", "step-fanout:call-255");
    assert_eq!(
        log.tool_source_position("fanout", through, &tool_id)
            .await
            .unwrap(),
        call_position
    );
    let sources = [
        (
            call_position,
            SourceField::Arguments,
            r#"{"index":255,"value":"last😀"}"#,
        ),
        (
            result_position,
            SourceField::Result,
            r#"{"answer":"literal😀result"}"#,
        ),
    ];
    for (position, field, expected) in sources {
        let mut text = String::new();
        loop {
            let source = log
                .history_source(
                    "fanout",
                    through,
                    position,
                    field,
                    None,
                    SourcePosition::After {
                        offset: text.len() as u64,
                    },
                    4,
                    65536,
                )
                .await
                .unwrap();
            assert_eq!(source.position, position);
            assert_eq!(source.offset, text.len() as u64);
            text.push_str(&source.text);
            if text.len() as u64 == source.total_bytes {
                break;
            }
        }
        assert_eq!(text, expected);
    }
    for position in [
        call_position,
        RowPosition {
            source_sequence: completion,
            item_index: 257,
        },
    ] {
        assert!(
            log.history_source(
                "fanout",
                old_fence,
                position,
                SourceField::Arguments,
                None,
                SourcePosition::Start,
                8192,
                65536
            )
            .await
            .is_err()
        );
    }
    assert!(
        log.history_source(
            "fanout",
            through,
            RowPosition {
                source_sequence: completion,
                item_index: 257
            },
            SourceField::Arguments,
            None,
            SourcePosition::Start,
            8192,
            65536
        )
        .await
        .is_err()
    );
    for reverse in [false, true] {
        let mut position = if reverse {
            Position::Tail
        } else {
            Position::Start
        };
        let mut found = std::collections::BTreeMap::new();
        loop {
            let Page::Ready {
                chunks,
                before,
                after,
                ..
            } = log
                .history_text("fanout", through, position, false, 8192, 17, 65536)
                .await
                .unwrap()
            else {
                panic!("prepared")
            };
            for chunk in chunks {
                if chunk.role == maka_agent_ports::session::history::Role::ToolCall {
                    assert_eq!(chunk.position.source_sequence, completion);
                    assert!(found.insert(chunk.position, chunk.message_id).is_none());
                }
            }
            let Some(cursor) = (if reverse { before } else { after }) else {
                break;
            };
            position = if reverse {
                Position::Before { cursor }
            } else {
                Position::After { cursor }
            };
        }
        assert_eq!(found.len(), 256);
        for index in 0..256 {
            assert_eq!(
                found[&RowPosition {
                    source_sequence: completion,
                    item_index: index
                }],
                maka_presentation::tool_message_id(
                    "inv-fanout",
                    &format!("step-fanout:call-{index}")
                )
            );
        }
    }
    // A search continuation is an exclusive pair boundary, even when no row has that pair.
    let search = log
        .search_transcript(
            "fanout",
            maka_event_log::transcript::search::TranscriptSearch {
                through,
                after: Some(RowPosition {
                    source_sequence: completion,
                    item_index: 257,
                }),
                query: "literal".into(),
                case_sensitive: true,
                whole_word: false,
                include_internal: false,
                max_matches: 16,
            },
        )
        .await
        .unwrap();
    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].position, result_position);
    let canonical = log.prefix(1024, 8 * 1024 * 1024).await.unwrap().digest;
    log.close().await.unwrap();
    for table in ["transcript_text", "transcript_rows", "transcript_progress"] {
        db.execute(&format!("DELETE FROM {table}"), []).unwrap();
    }
    let reopened = EventLog::open(&path).await.unwrap();
    while !reopened
        .prepare_transcript("fanout", through, 32)
        .await
        .unwrap()
    {}
    assert_eq!(rows(&db, "fanout"), original);
    assert_eq!(
        reopened.prefix(1024, 8 * 1024 * 1024).await.unwrap().digest,
        canonical
    );
    for (position, field, expected) in sources {
        let source = reopened
            .history_source(
                "fanout",
                through,
                position,
                field,
                None,
                SourcePosition::Start,
                8192,
                65536,
            )
            .await
            .unwrap();
        assert_eq!(source.position, position);
        assert_eq!(source.text, expected);
    }
    reopened.close().await.unwrap();
}
