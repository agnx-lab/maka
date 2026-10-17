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

use super::*;
use maka_agent_ports::session::history::Page;
use maka_agent_ports::session::history::Position;
use maka_agent_ports::session::history::Role;
use std::collections::BTreeMap;

#[tokio::test]
async fn history_pages_preserve_utf8_nul_suffixes_archive_and_frozen_fences() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("log.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.create_session("a", "create", &serde_json::json!({}), 1)
        .await
        .unwrap();
    let text = format!("a{}\0suffix-needle", "🦀".repeat(10_000));
    let (_, old) = partial(&log, "a", &text).await;
    interrupt(&log, "a").await;
    log.append(&event("a", request("history"))).await.unwrap();
    let call = maka_agent_facts::model::ModelToolCall {
        id: "read".into(),
        name: "Read".into(),
        input: serde_json::json!({"path": "source.rs", "intent": "Inspect source"}).into(),
        provider_options: None,
        provider_executed: false,
    };
    log.append(&event(
        "a",
        Fact::ModelObserved {
            step_id: "step-history".into(),
            event: ModelEvent::ToolCall(call.clone()),
        },
    ))
    .await
    .unwrap();
    log.append(&event(
        "a",
        Fact::ModelCompleted {
            step_id: "step-history".into(),
            output: ModelStep {
                parts: vec![ModelPart::ToolCall { call }],
                finish_reason: maka_agent_facts::model::ModelFinishReason::ToolCalls,
                usage: ModelUsage::default(),
                provider_options: None,
                response_id: None,
                model: None,
                timestamp: None,
            },
        },
    ))
    .await
    .unwrap();
    log.append(&event(
        "a",
        Fact::ToolDispatched {
            presentation: None,
            title: None,
            operation_id: "step-history:read".into(),
            call: maka_agent_facts::tool_call::ToolCallIdentity::provider(
                "step-history".into(),
                "read".into(),
            ),
            name: "Read".into(),
            input: serde_json::json!({"path": "source.rs", "intent": "Inspect source"}),
        },
    ))
    .await
    .unwrap();
    let template = event(
        "a",
        Fact::InvocationEnded {
            outcome: InvocationOutcome::Completed,
        },
    );
    let (result, _) = maka_agent_facts::event::EventWrite::tool_success(
        "read-result".into(),
        template.event().recorded_at,
        template.event().invocation.clone(),
        "step-history:read".into(),
        serde_json::json!({"content": text, "nested": ["second\nline", 42, false]}).into(),
    )
    .unwrap();
    log.append(&result).await.unwrap();
    let through = end(
        &log,
        "a",
        InvocationOutcome::Cancelled {
            source: "user".into(),
        },
    )
    .await;
    assert!(log.prepare_transcript("a", through, 32).await.unwrap());
    log.set_session_archived::<Value>("a", true, 10)
        .await
        .unwrap();
    let archived = log
        .scoped_sessions::<Value>(
            maka_event_log::sessions::CatalogScope::Profile,
            None,
            None.into(),
            maka_event_log::sessions::CatalogFilter {
                archived_only: false,
                include_archived: true,
                pending_only: false,
            },
        )
        .await
        .unwrap();
    assert!(archived.sessions[0].archived);
    // Large tool payloads remain available to exact readers but occupy a single
    // metadata row in chat, in either paging direction.
    for reverse in [false, true] {
        let mut position = if reverse {
            Position::Tail
        } else {
            Position::Start
        };
        let mut calls = 0;
        let mut results = 0;
        loop {
            let Page::Ready {
                chunks,
                before,
                after,
                ..
            } = log
                .history_view(
                    "a",
                    through,
                    position,
                    false,
                    maka_agent_ports::session::history::HistoryView::Conversation,
                    4096,
                    Some(8),
                    64 * 1024,
                )
                .await
                .unwrap()
            else {
                panic!("prepared")
            };
            for chunk in chunks {
                if matches!(chunk.role, Role::ToolCall | Role::ToolResult) {
                    assert!(chunk.origin.is_none());
                    assert_eq!((chunk.offset, chunk.total_bytes), (0, 0));
                    assert!(chunk.text.is_empty());
                    assert!(chunk.tool.is_some());
                    if chunk.role == Role::ToolCall {
                        calls += 1;
                    } else {
                        results += 1;
                    }
                }
            }
            position = match if reverse { before } else { after } {
                Some(cursor) if reverse => Position::Before { cursor },
                Some(cursor) => Position::After { cursor },
                None => break,
            };
        }
        assert_eq!((calls, results), (1, 1));
    }
    let mut position = Position::Start;
    let mut messages: BTreeMap<String, (Role, String)> = BTreeMap::new();
    let mut pages = 0;
    loop {
        let page = log
            .history_text("a", through, position, false, 16 * 1024, 8, 1024 * 1024)
            .await
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() < 256 * 1024);
        let Page::Ready { chunks, after, .. } = page else {
            panic!("prepared")
        };
        for chunk in chunks {
            let timing = chunk.timing.as_ref().expect("original turn timing");
            let finished = timing.finished.as_ref().unwrap();
            assert!(finished.at >= timing.started_at);
            assert_eq!(
                finished.outcome,
                maka_agent_ports::session::history::TurnOutcome::Aborted
            );
            if let Some(tool) = &chunk.tool {
                assert_eq!(
                    tool.call.as_ref().map(|call| call.name.as_str()),
                    Some("Read")
                );
                match chunk.role {
                    Role::ToolCall => {
                        assert_eq!(tool.tool_use_id, chunk.message_id);
                        assert_eq!(tool.call.as_ref().unwrap().position, chunk.position);
                    }
                    Role::ToolResult => {
                        let result = tool.result.as_ref().unwrap();
                        assert_eq!(result.position, chunk.position);
                        assert!(!result.is_error);
                        assert!(!result.image);
                        assert!(messages.contains_key(&tool.tool_use_id));
                    }
                    _ => panic!("tool metadata on non-tool row"),
                }
            } else {
                assert!(matches!(chunk.role, Role::User | Role::Assistant));
            }
            let entry = messages
                .entry(chunk.message_id)
                .or_insert((chunk.role, String::new()));
            assert_eq!(entry.1.len() as u64, chunk.offset);
            entry.1.push_str(&chunk.text);
            assert!(entry.1.len() as u64 <= chunk.total_bytes);
        }
        pages += 1;
        assert!(pages < 20, "continuation must advance");
        let Some(cursor) = after else {
            break;
        };
        position = Position::After { cursor };
    }
    assert!(pages >= 3, "large message must be split, not clipped");
    assert_eq!(
        messages
            .values()
            .find(|(role, _)| *role == Role::Assistant)
            .unwrap()
            .1,
        text
    );
    assert_eq!(
        messages
            .values()
            .find(|(role, _)| *role == Role::ToolResult)
            .unwrap()
            .1,
        format!("{text}\nsecond\nline"),
        "JSON string values stay multiline; property names and non-text leaves are not searchable"
    );
    let Page::Ready { chunks, after, .. } = log
        .history_text("a", old, Position::Start, false, 16 * 1024, 8, 1024 * 1024)
        .await
        .unwrap()
    else {
        panic!("prepared")
    };
    assert!(after.is_none());
    assert!(
        chunks.iter().all(|chunk| chunk
            .timing
            .as_ref()
            .is_some_and(|time| time.finished.is_none())),
        "old fence excludes the later terminal event"
    );
    assert!(
        chunks.iter().all(|chunk| chunk.role == Role::User),
        "old fence excludes unfinished answer"
    );
    // Exercise the disposable cache with two owners of identical source positions.
    // A destination's cache must neither satisfy nor duplicate a source lookup.
    log.create_session("b", "second-owner", &serde_json::json!({}), 1)
        .await
        .unwrap();
    let cache = rusqlite::Connection::open(&path).unwrap();
    cache
        .execute(
            "INSERT INTO transcript_rows (source_sequence, item_index, session_id, turn_id, message_id, payload, digest, total_bytes)
         SELECT source_sequence, item_index, 'b', turn_id, message_id, payload, digest, total_bytes
         FROM transcript_rows WHERE session_id = 'a'",
            [],
        )
        .unwrap();
    cache
        .execute(
            "INSERT INTO transcript_progress SELECT 'b', through_sequence
         FROM transcript_progress WHERE session_id = 'a'",
            [],
        )
        .unwrap();
    cache
        .execute(
            "INSERT INTO transcript_text
         SELECT 'b', source_sequence, item_index, field, timestamp, role, CASE WHEN role IN ('user', 'assistant', 'tool_call', 'tool_result') THEN CAST('destination cache' AS BLOB) ELSE body END
         FROM transcript_text WHERE session_id = 'a'",
            [],
        )
        .unwrap();
    let Page::Ready {
        chunks: destination,
        ..
    } = log
        .history_text("b", old, Position::Start, false, 16 * 1024, 8, 1024 * 1024)
        .await
        .unwrap()
    else {
        panic!("prepared destination");
    };
    assert!(!destination.is_empty());
    assert!(
        destination
            .iter()
            .all(|chunk| chunk.text == "destination cache")
    );
    let Page::Ready { chunks: source, .. } = log
        .history_text("a", old, Position::Start, false, 16 * 1024, 8, 1024 * 1024)
        .await
        .unwrap()
    else {
        panic!("prepared source");
    };
    assert_eq!(
        serde_json::to_value(source).unwrap(),
        serde_json::to_value(chunks).unwrap()
    );
    cache
        .execute("DELETE FROM transcript_text WHERE session_id = 'b'", [])
        .unwrap();
    let rebuilt = log
        .history_text("b", old, Position::Start, false, 16 * 1024, 8, 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(rebuilt).unwrap(),
        serde_json::to_value(
            log.history_text("a", old, Position::Start, false, 16 * 1024, 8, 1024 * 1024)
                .await
                .unwrap()
        )
        .unwrap(),
    );
    drop(cache);
    log.close().await.unwrap();
    let log = EventLog::open(&path).await.unwrap();
    let Page::Ready { chunks, .. } = log
        .history_text(
            "a",
            through,
            Position::Start,
            false,
            16 * 1024,
            8,
            1024 * 1024,
        )
        .await
        .unwrap()
    else {
        panic!("prepared")
    };
    assert!(!chunks.is_empty(), "restart retains readable archive");
    log.close().await.unwrap();
}

#[tokio::test]
async fn tool_metadata_preserves_empty_calls_and_non_text_results_through_frozen_windows() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("tools.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    for session in ["numeric", "unknown", "image", "failed", "rejected", "spoof"] {
        log.create_session(session, session, &serde_json::json!({}), 1)
            .await
            .unwrap();
        log.append(&event(session, opening())).await.unwrap();
        log.append(&event(session, request(session))).await.unwrap();
        let call = maka_agent_facts::model::ModelToolCall {
            id: "count".into(),
            name: "Count".into(),
            input: serde_json::json!({"count": 42}).into(),
            provider_options: None,
            provider_executed: false,
        };
        let step = format!("step-{session}");
        log.append(&event(
            session,
            observe(session, ModelEvent::ToolCall(call.clone())),
        ))
        .await
        .unwrap();
        log.append(&event(
            session,
            Fact::ModelCompleted {
                step_id: step.clone(),
                output: ModelStep {
                    parts: vec![ModelPart::ToolCall { call }],
                    finish_reason: maka_agent_facts::model::ModelFinishReason::ToolCalls,
                    usage: ModelUsage::default(),
                    provider_options: None,
                    response_id: None,
                    model: None,
                    timestamp: None,
                },
            },
        ))
        .await
        .unwrap();
        let operation = format!("{step}:count");
        let call_fence = log
            .append(&event(
                session,
                if session == "rejected" {
                    Fact::ToolRejected {
                        operation_id: operation.clone(),
                        call: maka_agent_facts::tool_call::ToolCallIdentity::provider(
                            step.clone(),
                            "count".into(),
                        ),
                        name: "Count".into(),
                        input: serde_json::json!({"count": 42}).into(),
                        reason: maka_agent_facts::tool_call::ToolRejection::Unavailable,
                    }
                } else {
                    Fact::ToolDispatched {
                        presentation: None,
                        title: None,
                        operation_id: operation.clone(),
                        call: maka_agent_facts::tool_call::ToolCallIdentity::provider(
                            step,
                            "count".into(),
                        ),
                        name: "Count".into(),
                        input: serde_json::json!({"count": 42}),
                    }
                },
            ))
            .await
            .unwrap();
        if !matches!(session, "unknown" | "failed" | "rejected") {
            let template = event(
                session,
                Fact::InvocationEnded {
                    outcome: InvocationOutcome::Completed,
                },
            );
            let output = if session == "image" {
                maka_agent_facts::tool_output::ToolOutput::Image(
                    maka_agent_facts::tool_output::ImageOutput {
                        detail: None,
                        mime_type: "image/png".into(),
                        reference: maka_agent_facts::attachment::StorageRef::WorkspaceFile {
                            relative_path: "unread-image.png".into(),
                        },
                    },
                )
            } else if session == "spoof" {
                serde_json::json!({"count": 42, "ready": true, "settlement": "failed"}).into()
            } else {
                serde_json::json!({"count": 42, "ready": true}).into()
            };
            let (result, _) = maka_agent_facts::event::EventWrite::tool_success(
                format!("{session}-result"),
                template.event().recorded_at,
                template.event().invocation.clone(),
                operation,
                output.into(),
            )
            .unwrap();
            log.append(&result).await.unwrap();
        } else if session != "rejected" {
            log.append(&event(
                session,
                Fact::ToolSettled {
                    operation_id: operation,
                    outcome: if session == "failed" {
                        maka_agent_facts::event::ToolOutcome::Failed {
                            message: "Recorded failure".into(),
                        }
                    } else {
                        maka_agent_facts::event::ToolOutcome::Unknown {
                            message: "Result unknown; do not repeat".into(),
                            operation: None,
                        }
                    },
                },
            ))
            .await
            .unwrap();
        }
        let through = end(&log, session, InvocationOutcome::Completed).await;
        assert!(log.prepare_transcript(session, through, 32).await.unwrap());
        let Page::Ready { chunks, .. } = log
            .history_text(
                session,
                call_fence,
                Position::Start,
                false,
                8192,
                8,
                64 * 1024,
            )
            .await
            .unwrap()
        else {
            panic!("ready")
        };
        assert!(chunks.iter().any(|chunk| chunk.role == Role::ToolCall
            && chunk.text.is_empty()
            && chunk.tool.as_ref().is_some_and(|tool| {
                tool.call.as_ref().is_some_and(|call| {
                    call.name == "Count" && call.dispatched == (session != "rejected")
                }) && (tool.result.is_none() || session == "rejected")
            })));
        assert!(
            session == "rejected" || chunks.iter().all(|chunk| chunk.role != Role::ToolResult),
            "old fence excludes settled result"
        );
        for reverse in [false, true] {
            let mut position = if reverse {
                Position::Tail
            } else {
                Position::Start
            };
            let mut tools = BTreeMap::new();
            for _ in 0..16 {
                let Page::Ready {
                    chunks,
                    before,
                    after,
                    ..
                } = log
                    .history_text(session, through, position, false, 8192, 1, 64 * 1024)
                    .await
                    .unwrap()
                else {
                    panic!("ready")
                };
                assert!(chunks.len() <= 1);
                for chunk in chunks {
                    if matches!(chunk.role, Role::ToolCall | Role::ToolResult) {
                        use maka_agent_ports::session::history::SourceField;
                        use maka_agent_ports::session::history::SourceFormat;
                        use maka_agent_ports::session::history::SourcePosition;
                        let field = if chunk.role == Role::ToolCall {
                            SourceField::Arguments
                        } else {
                            SourceField::Result
                        };
                        let source = log
                            .history_source(
                                session,
                                through,
                                chunk.position,
                                field,
                                None,
                                SourcePosition::Start,
                                8192,
                                64 * 1024,
                            )
                            .await
                            .unwrap();
                        assert_eq!(source.tool.as_ref().unwrap(), chunk.tool.as_ref().unwrap());
                        assert_eq!(
                            chunk
                                .tool
                                .as_ref()
                                .unwrap()
                                .result
                                .as_ref()
                                .unwrap()
                                .settlement,
                            Some(match session {
                                "unknown" => maka_agent_facts::tool_call::ToolSettlement::Unknown,
                                "failed" => maka_agent_facts::tool_call::ToolSettlement::Failed,
                                "rejected" => maka_agent_facts::tool_call::ToolSettlement::Rejected,
                                _ => maka_agent_facts::tool_call::ToolSettlement::Returned,
                            })
                        );

                        if chunk.role == Role::ToolResult {
                            assert!(
                                session == "rejected"
                                    || log
                                        .history_source(
                                            session,
                                            call_fence,
                                            chunk.position,
                                            field,
                                            None,
                                            SourcePosition::Start,
                                            8192,
                                            64 * 1024
                                        )
                                        .await
                                        .is_err()
                            );
                            if matches!(session, "numeric" | "spoof") {
                                assert_eq!(source.format, SourceFormat::Json);
                                assert_eq!(
                                    serde_json::from_str::<Value>(&source.text).unwrap(),
                                    if session == "spoof" {
                                        serde_json::json!({"count":42,"ready":true,"settlement":"failed"})
                                    } else {
                                        serde_json::json!({"count":42,"ready":true})
                                    }
                                );
                            } else if session == "image" {
                                assert_eq!(source.format, SourceFormat::ImageDescriptor);
                                let descriptor: Value = serde_json::from_str(&source.text).unwrap();
                                assert_eq!(descriptor["kind"], "image");
                                assert_eq!(descriptor["mimeType"], "image/png");
                                assert!(source.text.contains("unread-image.png"));
                                assert!(!temp.path().join("unread-image.png").exists());
                            } else if session == "unknown" {
                                assert_eq!(source.format, SourceFormat::Text);
                                assert_eq!(source.text, "Result unknown; do not repeat");
                            }
                        }
                    }
                    if let Some(tool) = chunk.tool {
                        tools.insert(chunk.role as u8, (chunk.text, tool));
                    }
                }
                let next = if reverse { before } else { after };
                let Some(cursor) = next else { break };
                position = if reverse {
                    Position::Before { cursor }
                } else {
                    Position::After { cursor }
                };
            }
            assert_eq!(
                tools.len(),
                2,
                "empty tool rows must not disappear or stall paging"
            );
            let (text, tool) = &tools[&(Role::ToolResult as u8)];
            let call = tool.call.as_ref().unwrap();
            let result = tool.result.as_ref().unwrap();
            assert_eq!(call.name, "Count");
            assert_eq!(
                result.is_error,
                matches!(session, "unknown" | "failed" | "rejected")
            );
            assert_eq!(result.image, session == "image");
            assert_eq!(text.is_empty(), matches!(session, "numeric" | "image"));
            let (_, call_tool) = &tools[&(Role::ToolCall as u8)];
            assert_eq!(tool, call_tool);
        }
    }
    let original_events =
        serde_json::to_vec(&log.prefix(1000, 1024 * 1024).await.unwrap()).unwrap();
    log.close().await.unwrap();
    let db = Connection::open(&path).unwrap();
    let canonical = rows(&db, "numeric");
    db.execute_batch("DELETE FROM transcript_rows WHERE json_extract(payload, '$.type') = 'tool_activity';
        UPDATE transcript_rows SET payload = json_remove(payload, '$.settlement') WHERE json_extract(payload, '$.type') = 'tool_result';
        DELETE FROM _sqlx_migrations WHERE version = 15;").unwrap();
    drop(db);
    let log = EventLog::open(&path).await.unwrap();
    assert_eq!(
        serde_json::to_vec(&log.prefix(1000, 1024 * 1024).await.unwrap()).unwrap(),
        original_events
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM transcript_rows", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);
    let through = *log.subscribe_commits().borrow();
    assert!(
        log.prepare_transcript("numeric", through, 32)
            .await
            .unwrap()
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(rows(&db, "numeric"), canonical);
    drop(db);
    let Page::Ready { chunks, .. } = log
        .history_text(
            "numeric",
            through,
            Position::Tail,
            false,
            8192,
            8,
            64 * 1024,
        )
        .await
        .unwrap()
    else {
        panic!("ready")
    };
    assert!(
        chunks
            .iter()
            .any(|chunk| chunk.tool.as_ref().is_some_and(|tool| tool
                .call
                .as_ref()
                .is_some_and(|call| call.name == "Count")
                && tool.result.as_ref().is_some_and(|result| !result.is_error)))
    );
    log.close().await.unwrap();
}

#[tokio::test]
async fn oversized_tool_metadata_returns_accepted_window_without_consuming_its_boundary() {
    use maka_agent_facts::import::Content;
    use maka_agent_facts::import::Record;
    use maka_agent_facts::import::Source;
    use maka_capabilities::storage::Namespace;
    use maka_plugin_core::scope::Scope;
    let temp = tempfile::tempdir().unwrap();
    let log = EventLog::open(&temp.path().join("tool-budget.sqlite"))
        .await
        .unwrap();
    let owner = maka_event_log::sessions::PluginSession {
        session_id: "budget".into(),
        creator: Namespace::new("history.fixture", Scope::Profile).unwrap(),
        fingerprint: "tool-budget".into(),
        managed: false,
        authority_session_id: None,
    };

    let records = [
        Content::User {
            text: "accepted prefix".into(),
        },
        Content::ToolCall {
            call_id: "call".into(),
            name: "name".repeat(1024),
            input: None,
        },
        Content::Assistant {
            text: "accepted suffix".into(),
            model: None,
            thinking: None,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(index, content)| Record {
        source_message_id: format!("m{index}"),
        source_turn_id: "turn".into(),
        timestamp: Some(1),
        content,
    })
    .collect::<Vec<_>>();
    log.import_session(
        &owner,
        &Source {
            adapter: "fixture".into(),
            session_id: "foreign".into(),
        },
        &serde_json::json!({}),
        records,
        1,
    )
    .await
    .unwrap();

    let through = *log.subscribe_commits().borrow();
    assert!(log.prepare_transcript("budget", through, 32).await.unwrap());
    for reverse in [false, true] {
        let position = if reverse {
            Position::Tail
        } else {
            Position::Start
        };
        let Page::Ready {
            chunks,
            before,
            after,
            ..
        } = log
            .history_text("budget", through, position, false, 8192, 8, 1024)
            .await
            .unwrap()
        else {
            panic!("ready")
        };
        assert_eq!(chunks.len(), 1);
        assert_eq!(
            chunks[0].text,
            if reverse {
                "accepted suffix"
            } else {
                "accepted prefix"
            }
        );
        let cursor = if reverse { before } else { after }.unwrap();
        let position = if reverse {
            Position::Before { cursor }
        } else {
            Position::After { cursor }
        };
        assert!(
            log.history_text("budget", through, position, false, 8192, 8, 1024)
                .await
                .is_err(),
            "indivisible first tool metadata must fail without being consumed"
        );
        let Page::Ready { chunks, .. } = log
            .history_text("budget", through, position, false, 8192, 1, 64 * 1024)
            .await
            .unwrap()
        else {
            panic!("ready")
        };
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].role, Role::ToolCall);
        assert!(chunks[0].tool.is_some());
        assert!(chunks[0].text.is_empty());
    }
    log.close().await.unwrap();
}

#[tokio::test]
async fn bounded_history_windows_cover_sparse_rows_both_directions_and_validate_real_positions() {
    use maka_agent_facts::import::Content;
    use maka_agent_facts::import::Record;
    use maka_agent_facts::import::Source;
    use maka_agent_ports::session::history::Cursor;
    use maka_capabilities::storage::Namespace;
    use maka_plugin_core::scope::Scope;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("windows.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    let owner = maka_event_log::sessions::PluginSession {
        session_id: "window".into(),
        creator: Namespace::new("history.fixture", Scope::Profile).unwrap(),
        fingerprint: "window-import".into(),
        managed: false,
        authority_session_id: None,
    };

    let mut records = Vec::new();
    let mut expected = Vec::new();
    for index in 0..40 {
        let content = if index % 4 == 2 {
            let text = format!("{index} a🦀中é\0tail\r\n{}", "🌏".repeat(index % 5 + 1));
            expected.push(text.clone());
            Content::Assistant {
                text,
                model: None,
                thinking: None,
            }
        } else if index % 4 == 1 {
            Content::User {
                text: String::new(),
            }
        } else {
            Content::Note {
                text: "bookkeeping is not conversation text".into(),
            }
        };
        records.push(Record {
            source_message_id: format!("m{index}"),
            source_turn_id: "turn".into(),
            timestamp: Some(1),
            content,
        });
    }
    log.import_session(
        &owner,
        &Source {
            adapter: "fixture".into(),
            session_id: "foreign".into(),
        },
        &serde_json::json!({}),
        records,
        1,
    )
    .await
    .unwrap();

    let through = *log.subscribe_commits().borrow();
    while !log.prepare_transcript("window", through, 32).await.unwrap() {}
    for position in [Position::Start, Position::Tail] {
        let page = log
            .history_view(
                "window",
                through,
                position,
                false,
                maka_agent_ports::session::history::HistoryView::Conversation,
                32 * 1024,
                None,
                64 * 1024,
            )
            .await
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        let Page::Ready {
            chunks,
            before,
            after,
            ..
        } = page
        else {
            panic!("prepared")
        };
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.text.clone())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(chunks.len(), 10);
        assert!(before.is_none() && after.is_none());

        let text_budget = if matches!(position, Position::Tail) {
            &expected[expected.len() - 6..]
        } else {
            &expected[..6]
        }
        .iter()
        .map(String::len)
        .sum();
        let bounded = log
            .history_view(
                "window",
                through,
                position,
                false,
                maka_agent_ports::session::history::HistoryView::Conversation,
                text_budget,
                None,
                64 * 1024,
            )
            .await
            .unwrap();
        let encoded = serde_json::to_vec(&bounded).unwrap();
        assert!(encoded.len() > 1024);
        for budget in [encoded.len() - 1, encoded.len(), encoded.len() + 1] {
            let limited = log
                .history_view(
                    "window",
                    through,
                    position,
                    false,
                    maka_agent_ports::session::history::HistoryView::Conversation,
                    text_budget,
                    None,
                    budget,
                )
                .await
                .unwrap();
            let actual = serde_json::to_vec(&limited).unwrap();
            assert!(actual.len() <= budget);
            let Page::Ready { chunks, .. } = limited else {
                panic!("prepared")
            };
            let count = if budget < encoded.len() { 5 } else { 6 };
            assert_eq!(chunks.len(), count);
            let texts = chunks
                .iter()
                .map(|chunk| chunk.text.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                texts,
                if matches!(position, Position::Tail) {
                    &expected[expected.len() - count..]
                } else {
                    &expected[..count]
                }
            );
            if budget >= encoded.len() {
                assert_eq!(actual, encoded);
            }
        }
    }
    let mut canonical = None;
    for budget in [4, 5, 7, 13, 128 * 1024] {
        for reverse in [false, true] {
            let db = Connection::open(&path).unwrap();
            db.execute("DELETE FROM transcript_text", []).unwrap();
            drop(db);
            let mut position = if reverse {
                Position::Tail
            } else {
                Position::Start
            };
            let mut collected: BTreeMap<RowPosition, (u64, String)> = BTreeMap::new();
            let mut previous = None;
            let mut done = false;
            for _ in 0..512 {
                let Page::Ready {
                    chunks,
                    before,
                    after,
                    through: actual,
                } = log
                    .history_text("window", through, position, false, budget, 2, 1024 * 1024)
                    .await
                    .unwrap()
                else {
                    panic!("prepared");
                };
                assert_eq!(actual, through);
                assert!(chunks.len() <= 2);
                assert!(chunks.iter().map(|chunk| chunk.text.len()).sum::<usize>() <= budget);
                assert!(
                    chunks
                        .windows(2)
                        .all(|pair| pair[0].position < pair[1].position)
                );
                for chunk in chunks {
                    let entry = collected.entry(chunk.position).or_insert_with(|| {
                        (if reverse { chunk.total_bytes } else { 0 }, String::new())
                    });
                    if reverse {
                        assert_eq!(chunk.offset + chunk.text.len() as u64, entry.0);
                        entry.1.insert_str(0, &chunk.text);
                        entry.0 = chunk.offset;
                    } else {
                        assert_eq!(chunk.offset, entry.0);
                        entry.1.push_str(&chunk.text);
                        entry.0 += chunk.text.len() as u64;
                    }
                    assert!(
                        chunk.position.source_sequence <= through,
                        "row positions retain the original raw event fence"
                    );
                }
                let cursor = if reverse { before } else { after };
                let Some(cursor) = cursor else {
                    done = true;
                    break;
                };
                assert_ne!(
                    previous,
                    Some(cursor),
                    "empty pages and cache rebuilding must progress"
                );
                previous = Some(cursor);
                position = if reverse {
                    Position::Before { cursor }
                } else {
                    Position::After { cursor }
                };
            }
            assert!(done, "budget {budget}, reverse {reverse}");
            let texts = collected
                .values()
                .map(|(_, text)| text.clone())
                .collect::<Vec<_>>();
            assert_eq!(texts, expected);
            let sequences = collected.keys().copied().collect::<Vec<_>>();
            assert!(
                sequences
                    .windows(2)
                    .all(|pair| pair[1].source_sequence > pair[0].source_sequence)
            );
            canonical.get_or_insert(collected);
        }
    }
    let first = *canonical.as_ref().unwrap().keys().next().unwrap();
    // The first message starts with two ASCII digits plus a space and then 'a'.
    // Find every exact byte boundary rather than deriving one from a JS index.
    let text = &canonical.as_ref().unwrap()[&first].1;
    for offset in 0..=text.len() {
        for reverse in [false, true] {
            let cursor = Cursor {
                field: Default::default(),
                position: first,
                offset: offset as u64,
            };
            let result = log
                .history_text(
                    "window",
                    through,
                    if reverse {
                        Position::Before { cursor }
                    } else {
                        Position::After { cursor }
                    },
                    false,
                    7,
                    2,
                    1024 * 1024,
                )
                .await;
            assert_eq!(
                result.is_ok(),
                text.is_char_boundary(offset),
                "offset {offset}, reverse {reverse}"
            );
        }
    }
    for cursor in [
        Cursor {
            field: Default::default(),
            position: RowPosition {
                item_index: first.item_index + 1,
                ..first
            },
            offset: 0,
        },
        Cursor {
            field: Default::default(),
            position: first,
            offset: text.len() as u64 + 1,
        },
        Cursor {
            field: Default::default(),
            position: RowPosition {
                source_sequence: 0,
                item_index: 0,
            },
            offset: 0,
        },
    ] {
        assert!(
            log.history_text(
                "window",
                through,
                Position::After { cursor },
                false,
                7,
                2,
                1024 * 1024
            )
            .await
            .is_err()
        );
    }
    log.create_session("other", "other", &serde_json::json!({}), 1)
        .await
        .unwrap();
    let high = *log.subscribe_commits().borrow();
    assert!(log.prepare_transcript("other", high, 32).await.unwrap());
    assert!(
        log.history_text(
            "other",
            high,
            Position::After {
                cursor: Cursor {
                    field: Default::default(),
                    position: first,
                    offset: 0
                }
            },
            false,
            7,
            2,
            1024 * 1024
        )
        .await
        .is_err()
    );
    let Page::Ready { chunks, after, .. } = log
        .history_text(
            "window",
            through,
            Position::Event { sequence: through },
            false,
            7,
            2,
            1024 * 1024,
        )
        .await
        .unwrap()
    else {
        panic!("prepared");
    };
    assert!(chunks.is_empty());
    assert!(after.is_none());
    log.close().await.unwrap();
}

#[tokio::test]
async fn history_text_requires_the_actual_prepared_raw_fence() {
    let temp = tempfile::tempdir().unwrap();
    let log = EventLog::open(&temp.path().join("fence.sqlite"))
        .await
        .unwrap();
    log.create_session("a", "create", &serde_json::json!({}), 1)
        .await
        .unwrap();
    let (_, through) = partial(&log, "a", "answer").await;
    assert!(log.prepare_transcript("a", through, 32).await.unwrap());
    let later = interrupt(&log, "a").await;
    assert!(later > through);
    assert!(
        log.history_text("a", later, Position::Start, false, 16, 2, 1024 * 1024)
            .await
            .is_err(),
        "reads require preparation through the requested raw fence"
    );
    assert!(log.prepare_transcript("a", later, 32).await.unwrap());
    let Page::Ready { chunks, .. } = log
        .history_text("a", later, Position::Tail, false, 16, 2, 1024 * 1024)
        .await
        .unwrap()
    else {
        panic!("prepared");
    };
    assert!(chunks.iter().any(|chunk| chunk.role == Role::Assistant));
    log.close().await.unwrap();
}

#[tokio::test]
async fn empty_projection_boundaries_obey_the_exact_json_window_budget() {
    use maka_agent_facts::attachment::AttachmentKind;
    use maka_agent_facts::attachment::AttachmentRef;
    use maka_agent_facts::attachment::StorageRef;
    use maka_agent_facts::input::DeliveredMessage;
    use maka_agent_facts::message::MessageDisposition;
    use maka_agent_facts::message::Placement;
    use maka_agent_facts::message::RootSourceMessage;
    let temp = tempfile::tempdir().unwrap();
    let log = EventLog::open(&temp.path().join("wire.sqlite"))
        .await
        .unwrap();
    log.create_session("a", "create", &serde_json::json!({}), 1)
        .await
        .unwrap();
    log.append(&event("a", opening())).await.unwrap();
    let mut target = 0;
    for index in 0..55 {
        let mut content: maka_agent_facts::input::MessageInput = "".into();
        if index == 37 {
            content.attachments = Some(vec![AttachmentRef {
                kind: AttachmentKind::Other,
                name: "a".into(),
                mime_type: "application/octet-stream".into(),
                bytes: 1,
                storage_ref: StorageRef::WorkspaceFile {
                    relative_path: "a".repeat(1100),
                },
            }]);
        }
        let message = DeliveredMessage {
            origin: None,
            message_id: format!("empty-{index}"),
            submitted_content_digest: content.content_digest().unwrap(),
            content: content.clone(),
        };
        let source = RootSourceMessage {
            message: message.clone(),
            unprepared_content: content,
            submitted_placement: Placement::CurrentTurn,
            disposition: MessageDisposition::Steering,
            submitted_intent: None,
        };
        source.validate().unwrap();
        let sequence = log
            .append(&event(
                "a",
                Fact::MessageSteered {
                    message: Box::new(message),
                    source: Some(Box::new(source)),
                },
            ))
            .await
            .unwrap();
        if index == 37 {
            target = sequence;
            // The attachment body now ends at a two-digit byte offset. Jump
            // two sequence digits so consuming the next empty row still grows
            // the wire cursor even when that offset returns to zero. Other
            // Sessions advance the real log without adding rows to this one.
            let gap = (0..1000)
                .map(|index| event(&format!("gap-{index}"), opening()))
                .collect::<Vec<_>>();
            log.append_batch(&gap).await.unwrap();
        }
    }
    let through = end(&log, "a", InvocationOutcome::Completed).await;
    while !log.prepare_transcript("a", through, 32).await.unwrap() {}
    let Page::Ready { chunks, .. } = log
        .history_text(
            "a",
            through,
            Position::Event { sequence: target },
            false,
            4,
            1,
            1024 * 1024,
        )
        .await
        .unwrap()
    else {
        panic!("prepared");
    };
    let cursor = maka_agent_ports::session::history::Cursor {
        field: Default::default(),
        position: chunks[0].position,
        offset: 0,
    };
    assert!(cursor.position.source_sequence < 100);
    let baseline = log
        .history_text(
            "a",
            through,
            Position::After { cursor },
            false,
            32,
            1,
            1024 * 1024,
        )
        .await
        .unwrap();
    let budget = serde_json::to_vec(&baseline).unwrap().len();
    assert!(budget >= 1024);
    let page = log
        .history_text(
            "a",
            through,
            Position::After { cursor },
            false,
            32,
            8,
            budget,
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_vec(&page).unwrap().len(), budget);
    let Page::Ready {
        after: Some(cursor),
        chunks,
        ..
    } = page
    else {
        panic!("boundary retained");
    };
    assert_eq!(chunks.len(), 1);
    assert!(
        cursor.position.source_sequence < 100,
        "oversized empty-row boundary was not consumed"
    );
    let mut cursor = cursor;
    let mut ended = false;
    for _ in 0..4 {
        let next = log
            .history_text(
                "a",
                through,
                Position::After { cursor },
                false,
                4,
                8,
                budget,
            )
            .await
            .unwrap();
        assert!(serde_json::to_vec(&next).unwrap().len() <= budget);
        let Page::Ready { chunks, after, .. } = next else {
            panic!("prepared");
        };
        assert!(chunks.is_empty());
        let Some(after) = after else {
            ended = true;
            break;
        };
        assert!(after.position > cursor.position);
        cursor = after;
    }
    assert!(ended, "empty projection windows reach the actual end");
    log.close().await.unwrap();
}

#[tokio::test]
async fn message_material_labels_and_time_survive_exact_paging_and_text_cache_upgrade() {
    use maka_agent_facts::attachment::AttachmentKind;
    use maka_agent_facts::attachment::AttachmentRef;
    use maka_agent_facts::attachment::StorageRef;
    use maka_agent_facts::input::DirectoryReference;
    use maka_agent_facts::input::MessageInput;
    use maka_agent_ports::session::history::SourceField;
    use maka_agent_ports::session::history::SourcePosition;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("details.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    let attachment = |name: &str, bytes| AttachmentRef {
        kind: AttachmentKind::Other,
        name: name.into(),
        mime_type: "application/octet-stream".into(),
        bytes,
        storage_ref: StorageRef::WorkspaceFile {
            relative_path: "not-opened".into(),
        },
    };
    let directory = DirectoryReference {
        host_id: "original-host".into(),
        path: "src/中文".into(),
    };
    let mut complete: MessageInput = "hidden model text".into();
    complete.display_text = Some("Visible 中\0tail".into());
    complete.attachments = Some(vec![
        attachment("notes.txt", 512),
        attachment("report.txt", 2048),
        attachment("image.png", 1572864),
    ]);
    complete.directory_references = Some(vec![directory.clone()]);
    let mut folder: MessageInput = "hidden model text".into();
    folder.display_text = Some(String::new());
    folder.directory_references = Some(vec![directory]);
    let mut file: MessageInput = "".into();
    file.attachments = Some(vec![attachment("empty.txt", 0)]);
    let cases = [
        (
            "a",
            complete,
            "Visible 中\0tail\n↳ notes.txt · 512 B\n↳ report.txt · 2.0 KiB\n↳ image.png · 1.5 MiB\n▱ src/中文",
        ),
        ("b", folder, "▱ src/中文"),
        ("c", file, "↳ empty.txt · 0 B"),
    ];
    let timestamp = 1_700_000_000_000u64;
    let mut sources = Vec::new();
    for (session, content, expected) in cases {
        log.create_session(session, "create", &serde_json::json!({}), 1)
            .await
            .unwrap();
        let mut original = event(
            session,
            Fact::InvocationOpened {
                configuration: None,
                input: InvocationInput::Message {
                    source_messages: Vec::new(),
                    content,
                    request_fingerprint: None,
                },
            },
        )
        .event()
        .clone();
        original.recorded_at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(timestamp);
        let through = log
            .append(&maka_agent_facts::event::EventWrite::plain(original).unwrap())
            .await
            .unwrap();
        assert!(log.prepare_transcript(session, through, 32).await.unwrap());
        let Page::Ready { chunks, .. } = log
            .history_text(session, through, Position::Start, false, 8192, 1, 64 * 1024)
            .await
            .unwrap()
        else {
            panic!("ready")
        };
        assert_eq!(chunks.len(), 1, "material-only prompts remain visible");
        assert_eq!(chunks[0].text, expected);
        assert_eq!(chunks[0].timestamp, timestamp);
        let sequence = chunks[0].position;
        for budget in [4, 19, 8192] {
            for reverse in [false, true] {
                let mut position = if reverse {
                    Position::Tail
                } else {
                    Position::Start
                };
                let mut actual = String::new();
                let mut pages = 0;
                loop {
                    pages += 1;
                    assert!(pages < expected.len() + 8, "bounded windows must progress");
                    let Page::Ready {
                        chunks,
                        before,
                        after,
                        ..
                    } = log
                        .history_text(session, through, position, false, budget, 1, 64 * 1024)
                        .await
                        .unwrap()
                    else {
                        panic!("ready")
                    };
                    assert!(chunks.len() <= 1);
                    for chunk in &chunks {
                        assert_eq!(chunk.timestamp, timestamp);
                        assert_eq!(chunk.total_bytes, expected.len() as u64);
                        let source = log
                            .history_source(
                                session,
                                through,
                                sequence,
                                SourceField::Body,
                                None,
                                if reverse {
                                    SourcePosition::Before {
                                        offset: chunk.offset + chunk.text.len() as u64,
                                    }
                                } else {
                                    SourcePosition::After {
                                        offset: chunk.offset,
                                    }
                                },
                                budget,
                                64 * 1024,
                            )
                            .await
                            .unwrap();
                        assert_eq!(source.text, chunk.text);
                        assert_eq!(source.offset, chunk.offset);
                        assert_eq!(source.timestamp, chunk.timestamp);
                        if reverse {
                            actual.insert_str(0, &chunk.text);
                        } else {
                            actual.push_str(&chunk.text);
                        }
                    }
                    let Some(cursor) = (if reverse { before } else { after }) else {
                        break;
                    };
                    let next = if reverse {
                        Position::Before { cursor }
                    } else {
                        Position::After { cursor }
                    };
                    assert_ne!(next, position);
                    position = next;
                }
                assert_eq!(actual, expected);
            }
        }
        sources.push((session, through, sequence, expected));
    }
    assert!(
        !temp.path().join("not-opened").exists(),
        "display paths perform no I/O"
    );
    let before = serde_json::to_vec(&log.prefix(10, 65536).await.unwrap()).unwrap();
    log.close().await.unwrap();
    let db = Connection::open(&path).unwrap();
    let canonical = sources
        .iter()
        .map(|(session, ..)| rows(&db, session))
        .collect::<Vec<_>>();
    db.execute(
        "UPDATE transcript_text SET body = CAST('old derived text' AS BLOB)",
        [],
    )
    .unwrap();
    db.execute("DELETE FROM _sqlx_migrations WHERE version = 9", [])
        .unwrap();
    drop(db);
    let log = EventLog::open(&path).await.unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM transcript_text", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        sources
            .iter()
            .map(|(session, ..)| rows(&db, session))
            .collect::<Vec<_>>(),
        canonical
    );
    assert_eq!(
        serde_json::to_vec(&log.prefix(10, 65536).await.unwrap()).unwrap(),
        before
    );
    for (session, through, sequence, expected) in sources {
        let Page::Ready { chunks, .. } = log
            .history_text(session, through, Position::Start, false, 8192, 1, 64 * 1024)
            .await
            .unwrap()
        else {
            panic!("ready")
        };
        assert_eq!(chunks[0].text, expected);
        assert_eq!(chunks[0].position, sequence);
        assert_eq!(chunks[0].timestamp, timestamp);
    }
    drop(db);
    log.close().await.unwrap();
}

#[tokio::test]
async fn thinking_windows_preserve_field_order_bytes_and_explicit_body_only_reads() {
    use maka_agent_facts::import::Content;
    use maka_agent_facts::import::Record;
    use maka_agent_facts::import::Source;
    use maka_agent_ports::session::history::Cursor;
    use maka_agent_ports::session::history::HistoryField;
    use maka_capabilities::storage::Namespace;
    use maka_plugin_core::scope::Scope;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("thinking.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    let owner = maka_event_log::sessions::PluginSession {
        session_id: "thinking".into(),
        creator: Namespace::new("history.fixture", Scope::Profile).unwrap(),
        fingerprint: "thinking-import".into(),
        managed: false,
        authority_session_id: None,
    };

    let thinking = "reason 中😀\0\r\n".repeat(101);
    let body = "answer é🦀\0tail".repeat(13);
    let records = [
        Content::User {
            text: "question".into(),
        },
        Content::Assistant {
            text: body.clone(),
            thinking: Some(thinking.clone()),
            model: Some("imported-evidence-model".into()),
        },
        Content::Assistant {
            text: String::new(),
            thinking: Some("thinking only 🦀".into()),
            model: None,
        },
        Content::Assistant {
            text: "body with empty thinking".into(),
            thinking: Some(String::new()),
            model: None,
        },
        Content::Assistant {
            text: "body without thinking".into(),
            thinking: None,
            model: None,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(index, content)| Record {
        source_message_id: format!("m{index}"),
        source_turn_id: "turn".into(),
        timestamp: Some(123),
        content,
    })
    .collect();
    log.import_session(
        &owner,
        &Source {
            adapter: "fixture".into(),
            session_id: "foreign".into(),
        },
        &serde_json::json!({}),
        records,
        1,
    )
    .await
    .unwrap();

    let through = *log.subscribe_commits().borrow();
    while !log
        .prepare_transcript("thinking", through, 32)
        .await
        .unwrap()
    {}
    let Page::Ready { chunks, .. } = log
        .history_text(
            "thinking",
            through,
            Position::Start,
            true,
            8192,
            8,
            64 * 1024,
        )
        .await
        .unwrap()
    else {
        panic!("ready")
    };
    let imported_message = chunks
        .iter()
        .find(|chunk| chunk.text == body)
        .unwrap()
        .message_id
        .clone();
    let imported_model = maka_agent_ports::session::history::HistoryModel::Imported {
        model_id: "imported-evidence-model".into(),
    };
    let expected = vec![
        "question".to_owned(),
        thinking.clone(),
        body.clone(),
        "thinking only 🦀".into(),
        "body with empty thinking".into(),
        "body without thinking".into(),
    ];
    let mut thinking_cursor = None;
    for include in [false, true] {
        for reverse in [false, true] {
            for budget in [4, 13, 8192] {
                let db = Connection::open(&path).unwrap();
                db.execute("DELETE FROM transcript_text", []).unwrap();
                drop(db);
                let mut position = if reverse {
                    Position::Tail
                } else {
                    Position::Start
                };
                let mut last = None;
                let mut collected = BTreeMap::<(RowPosition, u8), (u64, String)>::new();
                let mut done = false;
                for _ in 0..2048 {
                    let page = log
                        .history_text("thinking", through, position, include, budget, 2, 64 * 1024)
                        .await
                        .unwrap();
                    assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
                    let Page::Ready {
                        chunks,
                        before,
                        after,
                        ..
                    } = page
                    else {
                        panic!("ready")
                    };
                    assert!(chunks.len() <= 2);
                    assert!(chunks.iter().map(|c| c.text.len()).sum::<usize>() <= budget);
                    let keys = chunks
                        .iter()
                        .map(|c| {
                            (
                                c.position,
                                if c.field == HistoryField::Thinking {
                                    0
                                } else {
                                    1
                                },
                            )
                        })
                        .collect::<Vec<_>>();
                    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
                    for (chunk, key) in chunks.into_iter().zip(keys) {
                        assert_eq!(
                            chunk.model.as_ref(),
                            (chunk.message_id == imported_message).then_some(&imported_model)
                        );
                        if chunk.field == HistoryField::Thinking {
                            assert!(include);
                            assert_eq!(chunk.role, Role::Assistant);
                            assert!(chunk.tool.is_none() && chunk.attachments.is_empty());
                            thinking_cursor.get_or_insert(Cursor {
                                position: chunk.position,
                                field: chunk.field,
                                offset: 0,
                            });
                        }
                        let entry = collected.entry(key).or_insert((
                            if reverse { chunk.total_bytes } else { 0 },
                            String::new(),
                        ));
                        if reverse {
                            assert_eq!(chunk.offset + chunk.text.len() as u64, entry.0);
                            entry.1.insert_str(0, &chunk.text);
                            entry.0 = chunk.offset;
                        } else {
                            assert_eq!(chunk.offset, entry.0);
                            entry.1.push_str(&chunk.text);
                            entry.0 += chunk.text.len() as u64;
                        }
                    }
                    let Some(cursor) = (if reverse { before } else { after }) else {
                        done = true;
                        break;
                    };
                    assert_ne!(last, Some(cursor), "every field boundary must advance");
                    last = Some(cursor);
                    position = if reverse {
                        Position::Before { cursor }
                    } else {
                        Position::After { cursor }
                    };
                }
                assert!(done);
                let actual = collected
                    .into_values()
                    .map(|(_, text)| text)
                    .collect::<Vec<_>>();
                let wanted = if include {
                    expected.clone()
                } else {
                    vec![
                        expected[0].clone(),
                        body.clone(),
                        expected[4].clone(),
                        expected[5].clone(),
                    ]
                };
                assert_eq!(
                    actual, wanted,
                    "include={include} reverse={reverse} budget={budget}"
                );
            }
        }
    }
    let cursor = thinking_cursor.unwrap();
    assert!(
        log.history_text(
            "thinking",
            through,
            Position::After { cursor },
            false,
            8192,
            2,
            64 * 1024
        )
        .await
        .is_err(),
        "thinking cursor cannot silently change a body-only read"
    );
    let Page::Ready { chunks, .. } = log
        .history_text(
            "thinking",
            through,
            Position::After { cursor },
            true,
            8192,
            2,
            64 * 1024,
        )
        .await
        .unwrap()
    else {
        panic!("ready")
    };
    assert_eq!(chunks[0].text, thinking);
    assert_eq!(chunks[1].text, body);
    let body_cursor = Cursor {
        field: HistoryField::Body,
        ..cursor
    };
    let Page::Ready { chunks, .. } = log
        .history_text(
            "thinking",
            through,
            Position::Before {
                cursor: body_cursor,
            },
            true,
            8192,
            1,
            64 * 1024,
        )
        .await
        .unwrap()
    else {
        panic!("ready")
    };
    assert_eq!(chunks[0].field, HistoryField::Thinking);
    assert_eq!(chunks[0].text, thinking);
    let source = log
        .history_source(
            "thinking",
            through,
            cursor.position,
            maka_agent_ports::session::history::SourceField::Thinking,
            None,
            maka_agent_ports::session::history::SourcePosition::Start,
            8192,
            64 * 1024,
        )
        .await
        .unwrap();
    assert_eq!(source.text, thinking);
    assert_eq!(source.model.as_ref(), Some(&imported_model));
    assert!(
        log.history_text(
            "thinking",
            through,
            Position::After {
                cursor: Cursor {
                    offset: 8,
                    ..cursor
                }
            },
            true,
            8192,
            2,
            64 * 1024
        )
        .await
        .is_err(),
        "Thinking offsets must not split UTF-8"
    );
    log.close().await.unwrap();
    // Reopen an actual pre-field cache shape. Migration may rebuild derived
    // text, but must preserve canonical identities, fences and both fields.
    let db = Connection::open(&path).unwrap();
    db.execute_batch("ALTER TABLE transcript_text RENAME TO field_cache;
        CREATE TABLE transcript_text AS SELECT session_id, source_sequence AS sequence, timestamp, role, body FROM field_cache WHERE field = 'body';
        DROP TABLE field_cache;
        DELETE FROM _sqlx_migrations WHERE version IN (10, 31);").unwrap();
    drop(db);
    let reopened = EventLog::open(&path).await.unwrap();
    while !reopened
        .prepare_transcript("thinking", through, 32)
        .await
        .unwrap()
    {}
    let Page::Ready {
        chunks,
        through: restored_fence,
        ..
    } = reopened
        .history_text(
            "thinking",
            through,
            Position::After { cursor },
            true,
            8192,
            2,
            64 * 1024,
        )
        .await
        .unwrap()
    else {
        panic!("ready")
    };
    assert_eq!(restored_fence, through);
    assert_eq!(chunks[0].text, thinking);
    assert_eq!(chunks[1].text, body);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn failed_turn_and_retry_diagnostics_are_visible_with_exact_sources_and_fences() {
    use maka_agent_ports::session::history::{
        HistoryView, SourceField, SourcePosition, SourceRole,
    };

    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("failures.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.create_session("failed", "create", &serde_json::json!({}), 1)
        .await
        .unwrap();
    log.append(&event("failed", opening())).await.unwrap();
    log.append(&event("failed", request("first")))
        .await
        .unwrap();
    let retry_fence = log
        .append(&event(
            "failed",
            Fact::ModelInterrupted {
                model_failure: None,
                step_id: "step-first".into(),
                status: ModelInterruption::RetryableFailure,
                diagnostic: Some("Connection lost 中😀; retry permitted".into()),
            },
        ))
        .await
        .unwrap();
    log.append(&event("failed", request("second")))
        .await
        .unwrap();
    log.append(&event(
        "failed",
        Fact::ModelInterrupted {
            model_failure: None,
            step_id: "step-second".into(),
            status: ModelInterruption::Failed,
            diagnostic: Some("Provider rejected request".into()),
        },
    ))
    .await
    .unwrap();
    // A terminal failure with no message still exposes its recorded class.
    let through = end(
        &log,
        "failed",
        InvocationOutcome::Failed {
            model_failure: None,
            class: "provider_rejected".into(),
            message: None,
        },
    )
    .await;
    assert!(log.prepare_transcript("failed", through, 32).await.unwrap());
    for view in [HistoryView::Text, HistoryView::Conversation] {
        let mut position = Position::Start;
        let mut failures = BTreeMap::<RowPosition, String>::new();
        for _ in 0..64 {
            let Page::Ready { chunks, after, .. } = log
                .history_view("failed", through, position, false, view, 17, Some(1), 8192)
                .await
                .unwrap()
            else {
                panic!("prepared history")
            };
            for chunk in chunks {
                if chunk.role != Role::Failure {
                    continue;
                }
                let source = log
                    .history_source(
                        "failed",
                        through,
                        chunk.position,
                        SourceField::Body,
                        None,
                        SourcePosition::After {
                            offset: chunk.offset,
                        },
                        chunk.text.len().max(4),
                        8192,
                    )
                    .await
                    .unwrap();
                assert_eq!(source.role, SourceRole::Failure);
                assert!(source.fields.contains(&SourceField::Body));
                assert_eq!(source.text, chunk.text);
                failures
                    .entry(chunk.position)
                    .or_default()
                    .push_str(&chunk.text);
            }
            let Some(cursor) = after else { break };
            position = Position::After { cursor };
        }
        assert_eq!(
            failures.values().map(String::as_str).collect::<Vec<_>>(),
            [
                "Connection lost 中😀; retry permitted",
                "Provider rejected request",
                "provider_rejected",
            ]
        );
        let Page::Ready { chunks, .. } = log
            .history_view(
                "failed",
                retry_fence,
                Position::Tail,
                false,
                view,
                8192,
                Some(16),
                8192,
            )
            .await
            .unwrap()
        else {
            panic!("prepared history")
        };
        assert_eq!(
            chunks
                .iter()
                .filter(|chunk| chunk.role == Role::Failure)
                .map(|chunk| chunk.text.as_str())
                .collect::<Vec<_>>(),
            ["Connection lost 中😀; retry permitted"]
        );
    }
    log.close().await.unwrap();
    // Simulate the prior cache projection, then apply the diagnostic-body upgrade.
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE transcript_text SET body = x'' WHERE role IN ('turn_state', 'model_interruption')",
        [],
    )
    .unwrap();
    db.execute("DELETE FROM _sqlx_migrations WHERE version = 21", [])
        .unwrap();
    drop(db);
    let log = EventLog::open(&path).await.unwrap();
    let Page::Ready { chunks, .. } = log
        .history_text("failed", through, Position::Tail, false, 8192, 16, 8192)
        .await
        .unwrap()
    else {
        panic!("prepared history")
    };
    assert_eq!(
        chunks
            .iter()
            .filter(|chunk| chunk.role == Role::Failure)
            .count(),
        3
    );
}

#[tokio::test]
async fn notification_origin_survives_every_history_and_source_fragment_within_json_budgets() {
    use maka_agent_facts::input::DeliveredMessage;
    use maka_agent_facts::message::RootSourceMessage;
    use maka_agent_facts::message::{MessageDisposition, MessageOrigin, Placement};
    use maka_agent_ports::session::history::{SourceField, SourcePosition};
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("notification.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.create_session("notice", "create", &serde_json::json!({}), 1)
        .await
        .unwrap();
    let text = "通知😀\0suffix\n".repeat(300);
    let content: maka_agent_facts::input::MessageInput = text.clone().into();
    let origin = MessageOrigin::WorkhubGraph {
        graph_id: "agent_graph_actual".into(),
    };
    let source = RootSourceMessage {
        unprepared_content: content.clone(),
        message: DeliveredMessage {
            message_id: "notification".into(),
            submitted_content_digest: content.content_digest().unwrap(),
            content: content.clone(),
            origin: Some(origin.clone()),
        },
        submitted_placement: Placement::CurrentTurn,
        disposition: MessageDisposition::TurnStarted,
        submitted_intent: None,
    };
    let opening = event(
        "notice",
        Fact::InvocationOpened {
            configuration: None,
            input: InvocationInput::Message {
                content,
                source_messages: vec![source.clone()],
                request_fingerprint: None,
            },
        },
    );
    log.admit_message(
        maka_event_log::message_admissions::PendingMessageAdmission {
            execution: maka_event_log::message_admissions::PendingExecution {
                owner: "maka-runtime-host".into(),
                basis: serde_json::Value::Null,
            },
            steering_invocation: None,
            required_tools: Default::default(),
            invocation: opening.event().invocation.clone(),
            source,
            admitted_at: 1,
        },
    )
    .await
    .unwrap();
    log.append(&opening).await.unwrap();
    let through = log
        .append(&event(
            "notice",
            Fact::InvocationEnded {
                outcome: InvocationOutcome::Completed,
            },
        ))
        .await
        .unwrap();
    assert!(log.prepare_transcript("notice", through, 32).await.unwrap());
    log.close().await.unwrap();
    let log = EventLog::open(&path).await.unwrap();
    let mut position = Position::Start;
    let mut assembled = String::new();
    let mut fragments = 0;
    let mut source_sequence = None;
    let mut pages = 0;
    loop {
        let page = log
            .history_text("notice", through, position, false, 256, 2, 1536)
            .await
            .unwrap();
        assert!(serde_json::to_vec(&page).unwrap().len() <= 1536);
        let Page::Ready { chunks, after, .. } = page else {
            panic!("prepared")
        };
        for chunk in chunks {
            assert_eq!(chunk.role, Role::User);
            assert_eq!(chunk.message_id, "notification");
            assert_eq!(chunk.origin.as_ref(), Some(&origin));
            assert_eq!(chunk.offset, assembled.len() as u64);
            source_sequence = Some(chunk.position);
            assembled.push_str(&chunk.text);
            fragments += 1;
        }
        pages += 1;
        assert!(pages < 100, "history continuation must advance");
        let Some(cursor) = after else {
            break;
        };
        position = Position::After { cursor };
    }
    assert!(fragments > 2);
    assert_eq!(assembled, text);
    let sequence = source_sequence.unwrap();
    let mut offset = 0;
    let mut assembled = String::new();
    loop {
        let source = log
            .history_source(
                "notice",
                through,
                sequence,
                SourceField::Body,
                None,
                SourcePosition::After { offset },
                256,
                1536,
            )
            .await
            .unwrap();
        assert_eq!(source.origin.as_ref(), Some(&origin));
        assert!(serde_json::to_vec(&source).unwrap().len() <= 1536);
        assert_eq!(source.offset, offset);
        assert!(!source.text.is_empty());
        assembled.push_str(&source.text);
        offset += source.text.len() as u64;
        if offset == source.total_bytes {
            break;
        }
    }
    assert_eq!(assembled, text);
    let source = log
        .history_source(
            "notice",
            through,
            sequence,
            SourceField::Body,
            None,
            SourcePosition::Start,
            1024,
            4096,
        )
        .await
        .unwrap();
    let exact_budget = serde_json::to_vec(&source).unwrap().len();
    assert!(exact_budget > 1024);
    assert!(
        log.history_source(
            "notice",
            through,
            sequence,
            SourceField::Body,
            None,
            SourcePosition::Start,
            1024,
            exact_budget - 1
        )
        .await
        .is_err()
    );
    assert_eq!(
        log.history_source(
            "notice",
            through,
            sequence,
            SourceField::Body,
            None,
            SourcePosition::Start,
            1024,
            exact_budget
        )
        .await
        .unwrap()
        .origin,
        Some(origin)
    );
    log.close().await.unwrap();
}

#[tokio::test]
async fn response_models_follow_exact_steps_fences_and_retained_copied_history() {
    use maka_agent_facts::event::EventWrite;
    use maka_agent_ports::session::history::{
        HistoryField, HistoryModel, SourceField, SourcePosition,
    };
    use maka_event_log::sessions::{SessionCopy, SessionRetirement};
    type Expected = BTreeMap<String, (HistoryField, String, HistoryModel)>;
    async fn step(
        log: &EventLog,
        invocation: &Invocation,
        id: &str,
        requested: &str,
        reported: Option<&str>,
        parts: &[(TextKind, &str)],
    ) -> (u64, Expected) {
        let write = |fact| EventWrite::plain(RuntimeEvent::new(invocation.clone(), fact)).unwrap();
        let mut request = request(id);
        let Fact::ModelRequested { model_id, .. } = &mut request else {
            unreachable!()
        };
        *model_id = requested.into();
        log.append(&write(request)).await.unwrap();
        let metadata = HistoryModel::Native {
            requested_model_id: requested.into(),
            reported_model_id: reported.map(str::to_owned),
        };
        let mut expected = Expected::new();
        let mut accepted = Vec::new();
        for (index, (kind, text)) in parts.iter().enumerate() {
            let part_id = format!("{id}-{index}");
            let observed = |event| Fact::ModelObserved {
                step_id: format!("step-{id}"),
                event,
            };
            let start = write(observed(ModelEvent::PartStarted {
                id: part_id.clone(),
                text_kind: *kind,
                provider_options: None,
            }));
            expected.insert(
                start.event().id.clone(),
                (
                    if *kind == TextKind::Thinking {
                        HistoryField::Thinking
                    } else {
                        HistoryField::Body
                    },
                    (*text).into(),
                    metadata.clone(),
                ),
            );
            log.append(&start).await.unwrap();
            for event in [
                ModelEvent::PartDelta {
                    id: part_id.clone(),
                    text: (*text).into(),
                    provider_options: None,
                },
                ModelEvent::PartFinished {
                    id: part_id,
                    provider_options: None,
                },
            ] {
                log.append(&write(observed(event))).await.unwrap();
            }
            accepted.push(ModelPart::Text {
                text_kind: *kind,
                text: (*text).into(),
                provider_options: None,
            });
        }
        log.append(&write(Fact::ModelObserved {
            step_id: format!("step-{id}"),
            event: ModelEvent::ResponseMetadata {
                id: None,
                model: reported.map(str::to_owned),
                timestamp: None,
            },
        }))
        .await
        .unwrap();
        log.append(&write(Fact::ModelObserved {
            step_id: format!("step-{id}"),
            event: ModelEvent::Finished {
                reason: maka_agent_facts::model::ModelFinishReason::Stop,
                usage: ModelUsage::default(),
                provider_options: None,
            },
        }))
        .await
        .unwrap();
        let through = log
            .append(&write(Fact::ModelCompleted {
                step_id: format!("step-{id}"),
                output: ModelStep {
                    parts: accepted,
                    finish_reason: maka_agent_facts::model::ModelFinishReason::Stop,
                    usage: ModelUsage::default(),
                    provider_options: None,
                    response_id: None,
                    model: reported.map(str::to_owned),
                    timestamp: None,
                },
            }))
            .await
            .unwrap();
        (through, expected)
    }
    async fn read(log: &EventLog, session: &str, through: u64, expected: &Expected) {
        assert!(log.prepare_transcript(session, through, 32).await.unwrap());
        for reverse in [false, true] {
            let mut position = if reverse {
                Position::Tail
            } else {
                Position::Start
            };
            let mut actual = BTreeMap::<String, String>::new();
            let mut done = false;
            for _ in 0..128 {
                let page = log
                    .history_text(session, through, position, true, 37, 2, 4096)
                    .await
                    .unwrap();
                assert!(serde_json::to_vec(&page).unwrap().len() <= 4096);
                let Page::Ready {
                    chunks,
                    before,
                    after,
                    ..
                } = page
                else {
                    panic!("prepared")
                };
                for chunk in chunks {
                    if chunk.role != Role::Assistant {
                        assert!(chunk.model.is_none());
                        continue;
                    }
                    let (field, text, model) = &expected[&chunk.message_id];
                    assert_eq!(chunk.field, *field);
                    assert_eq!(chunk.model.as_ref(), Some(model));
                    let assembled = actual.entry(chunk.message_id.clone()).or_default();
                    if reverse {
                        assembled.insert_str(0, &chunk.text);
                    } else {
                        assembled.push_str(&chunk.text);
                    }
                    // Independently read the same literal field starting inside the message.
                    let source = log
                        .history_source(
                            session,
                            through,
                            chunk.position,
                            if *field == HistoryField::Thinking {
                                SourceField::Thinking
                            } else {
                                SourceField::Body
                            },
                            None,
                            SourcePosition::After {
                                offset: chunk.offset,
                            },
                            19,
                            4096,
                        )
                        .await
                        .unwrap();
                    assert_eq!(source.model.as_ref(), Some(model));
                    assert_eq!(
                        source.text,
                        text[chunk.offset as usize..][..source.text.len()]
                    );
                }
                let Some(cursor) = (if reverse { before } else { after }) else {
                    done = true;
                    break;
                };
                position = if reverse {
                    Position::Before { cursor }
                } else {
                    Position::After { cursor }
                };
            }
            assert!(done, "bounded pages must finish");
            assert_eq!(
                actual,
                expected
                    .iter()
                    .map(|(id, (_, text, _))| (id.clone(), text.clone()))
                    .collect()
            );
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("response-models.sqlite");
    let log = EventLog::open(&path).await.unwrap();
    log.create_session("models", "create", &serde_json::json!({}), 1)
        .await
        .unwrap();
    let first = Invocation {
        session_id: "models".into(),
        turn_id: "one-turn".into(),
        run_id: "first-run".into(),
        invocation_id: "first-invocation".into(),
    };
    let write = |invocation: &Invocation, fact| {
        EventWrite::plain(RuntimeEvent::new(invocation.clone(), fact)).unwrap()
    };
    log.append(&write(&first, opening())).await.unwrap();
    let body = "first body 中😀\0tail\n".repeat(12);
    let thinking = "first reasoning 🦀\n".repeat(8);
    let (first_fence, first_expected) = step(
        &log,
        &first,
        "one",
        "requested-alias",
        Some("reported-concrete"),
        &[(TextKind::Thinking, &thinking), (TextKind::Text, &body)],
    )
    .await;
    let (_, second_expected) = step(
        &log,
        &first,
        "two",
        "second-request",
        None,
        &[(TextKind::Text, "second step without reported identity")],
    )
    .await;
    let copied_fence = log
        .append(&write(
            &first,
            Fact::InvocationEnded {
                outcome: InvocationOutcome::Cancelled {
                    source: "user".into(),
                },
            },
        ))
        .await
        .unwrap();
    let mut copied_expected = first_expected.clone();
    copied_expected.extend(second_expected);
    let revision = log
        .get_session::<Value>("models")
        .await
        .unwrap()
        .unwrap()
        .revision;
    log.copy_session(
        SessionCopy {
            source_session_id: "models".into(),
            target_session_id: "model-copy".into(),
            expected_source_revision: revision,
            purpose: maka_agent_facts::session::CopyPurpose::Branch {
                turn_id: None,
                side_conversation: false,
            },
        },
        &serde_json::json!({}),
        2,
    )
    .await
    .unwrap();
    let successor = Invocation {
        run_id: "successor-run".into(),
        invocation_id: "successor-invocation".into(),
        ..first
    };
    log.append(&write(&successor, opening())).await.unwrap();
    let (_, successor_expected) = step(
        &log,
        &successor,
        "three",
        "resumed-request",
        Some("resumed-reported"),
        &[(TextKind::Text, "same turn, distinct physical invocation")],
    )
    .await;
    let through = log
        .append(&write(
            &successor,
            Fact::InvocationEnded {
                outcome: InvocationOutcome::Completed,
            },
        ))
        .await
        .unwrap();
    let mut all_expected = copied_expected.clone();
    all_expected.extend(successor_expected);
    read(&log, "models", first_fence, &first_expected).await;
    read(&log, "models", through, &all_expected).await;
    read(&log, "model-copy", through, &copied_expected).await;
    let revision = log
        .get_session::<Value>("models")
        .await
        .unwrap()
        .unwrap()
        .revision;
    log.begin_session_removal("models", revision).await.unwrap();
    assert_eq!(
        log.finish_session_retirement("models").await.unwrap(),
        SessionRetirement::Removed
    );
    assert_eq!(
        log.collect_session_material(None).await.unwrap(),
        maka_event_log::sessions::MaterialCollection::Retained("models".into())
    );
    log.close().await.unwrap();
    let reopened = EventLog::open(&path).await.unwrap();
    read(&reopened, "model-copy", copied_fence, &copied_expected).await;
    reopened.close().await.unwrap();
}
