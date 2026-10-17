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
use maka_agent_facts::import::Content;
use maka_agent_facts::import::Record;
use maka_agent_facts::import::Source;
use maka_capabilities::storage::Namespace;
use maka_event_log::sessions::PluginSession;
use maka_plugin_core::scope::Scope as StorageScope;

#[tokio::test]
async fn history_search_options_match_persisted_unicode_literals_and_original_preview_offsets() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let fixture = Fixture::new("http://127.0.0.1:9/v1", None).await;
        let seed = fixture.create("Unicode search").await;
        let configuration = fixture.owner.session(&seed).await.unwrap().configuration;
        let session = format!("{}{}", fixture.owner.prefix, Uuid::new_v4().simple());
        let owner = PluginSession {
            session_id: session.clone(),
            creator: Namespace::new("unicode-search.fixture", StorageScope::Profile).unwrap(),
            fingerprint: "unicode-search-import".into(),
            managed: false,
            authority_session_id: None,
        };
        let texts = [
            format!("xNeedle {} NeEdLe valid", "界".repeat(150)),
            "literal .* token".into(),
            "literal ab token".into(),
            "CAFÉ accent".into(),
            "ΟΣ uppercase".into(),
            "ος final".into(),
            "οσ medial".into(),
            "MARK\u{301} MARK_ MARK\u{200d} MARK界".into(),
            "mark".into(),
            format!("{} CAFÉ original-tail", "Ⱥ".repeat(400)),
        ];
        let records = texts.into_iter().enumerate().map(|(i, text)| Record {
            source_message_id: format!("unicode-{i}"),
            source_turn_id: format!("unicode-turn-{i}"),
            timestamp: Some(i as u64 + 1),
            content: Content::User { text },
        }).collect();
        fixture.store.import_session(&owner,
            &Source { adapter: "fixture".into(), session_id: "unicode-original".into() },
            &configuration, records, now()).await.unwrap();
        let through = *fixture.store.subscribe_commits().borrow();
        for (query, case_sensitive, whole_word, expected) in [
            ("NEEDLE", false, true, vec![format!("…{} NeEdLe valid", "界".repeat(21))]),
            ("NEEDLE", true, true, vec![]),
            (".*", false, true, vec!["literal .* token".into()]),
            ("café", false, true, vec!["CAFÉ accent".into(), format!("…{} CAFÉ original-tail", "Ⱥ".repeat(32))]),
            ("ος", false, true, vec!["ΟΣ uppercase".into(), "ος final".into(), "οσ medial".into()]),
            ("mark", false, true, vec!["mark".into()]),
            ("mark", false, false, vec!["MARK\u{301} MARK_ MARK\u{200d} MARK界".into(), "mark".into()]),
        ] {
            let mut actual = Vec::new();
            let mut after: Option<maka_agent_ports::session::history::RowPosition> = None;
            let mut finished = false;
            for _ in 0..32 {
                let page = fixture.query("search", json!({"sessionId":session,"through":through,"after":after,"query":query,"caseSensitive":case_sensitive,"wholeWord":whole_word,"includeInternal":false})).await.unwrap();
                assert_eq!(page["through"], through);
                if page["kind"] == "preparing" { continue; }
                for hit in page["matches"].as_array().unwrap() {
                    assert_eq!(hit["field"], "body");
                    actual.push(hit["preview"].as_str().unwrap().to_owned());
                }
                let Some(next) = serde_json::from_value::<Option<maka_agent_ports::session::history::RowPosition>>(page["nextAfter"].clone()).unwrap() else { finished = true; break; };
                assert!(after.is_none_or(|after| next > after));
                after = Some(next);
            }
            assert!(finished, "{query}");
            assert_eq!(actual, expected, "query={query} caseSensitive={case_sensitive} wholeWord={whole_word}");
        }
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn history_search_preserves_empty_batches_and_exact_cursors_with_original_authority() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let fixture = Fixture::new("http://127.0.0.1:9/v1", None).await;
        let seed = fixture.create("Search").await;
        let configuration = fixture.owner.session(&seed).await.unwrap().configuration;
        let session = format!("{}{}", fixture.owner.prefix, Uuid::new_v4().simple());
        let plugin = PluginSession {
            session_id: session.clone(),
            creator: Namespace::new("search.fixture", StorageScope::Profile).unwrap(),
            fingerprint: "search-import".into(),
            managed: false,
            authority_session_id: None,
        };

        let records = (0..112).map(|i| Record {
            source_message_id: format!("m{i}"),
            source_turn_id: format!("turn-{i}"),
            timestamp: Some(i + 1),
            content: Content::User { text: if i < 72 { format!("Earlier {i}") } else {
                format!("中😀 match {i} {}", "界".repeat(300))
            } },
        }).collect();
        fixture.store.import_session(&plugin, &Source {
            adapter: "fixture".into(), session_id: "source".into(),
        }, &configuration, records, 1).await.unwrap();

        let through = *fixture.store.subscribe_commits().borrow();
        let request = json!({"sessionId":session,"through":through,"after":null,"query":"中😀","caseSensitive":true,"wholeWord":false,"includeInternal":false});
        let mut input = request.clone();
        let mut matches = Vec::new();
        let mut empty_continuation = false;
        let mut preparing = false;
        let mut ended = false;
        for _ in 0..32 {
            let page = fixture.query("search", input.clone()).await.unwrap();
            assert_eq!(page["through"], through);
            assert!(serde_json::to_vec(&page).unwrap().len() <= 16 * 1024);
            if page["kind"] == "preparing" { preparing = true; continue; }
            let found = page["matches"].as_array().unwrap();
            assert!(found.len() <= 16);
            for item in found {
                assert!(input["after"].is_null() || serde_json::from_value::<maka_agent_ports::session::history::RowPosition>(item["cursor"]["position"].clone()).unwrap() > serde_json::from_value(input["after"].clone()).unwrap());
                assert_eq!(item["cursor"]["offset"], 0);
                let preview = item["preview"].as_str().unwrap();
                assert!(preview.len() <= 384 && preview.contains("中😀"));
                matches.push(item.clone());
            }
            let Some(after) = serde_json::from_value::<Option<maka_agent_ports::session::history::RowPosition>>(page["nextAfter"].clone()).unwrap() else { ended = true; break; };
            empty_continuation |= found.is_empty();
            assert!(input["after"].is_null() || after > serde_json::from_value(input["after"].clone()).unwrap());
            assert!(after.source_sequence <= through);
            input["after"] = json!(after);
        }
        assert!(preparing && empty_continuation && ended);
        assert_eq!(matches.len(), 40);
        assert!(matches.windows(2).all(|pair| serde_json::from_value::<maka_agent_ports::session::history::RowPosition>(pair[0]["cursor"]["position"].clone()).unwrap() < serde_json::from_value(pair[1]["cursor"]["position"].clone()).unwrap()));
        let located = fixture.query("history", json!({"sessionId":session,"through":through,
            "position":{"kind":"after","cursor":matches[17]["cursor"]}})).await.unwrap();
        assert_eq!(located["chunks"][0]["position"], matches[17]["cursor"]["position"]);
        assert!(located["chunks"][0]["text"].as_str().unwrap().starts_with("中😀 match 89 "));
        for (key, value) in [
            ("sessionId", json!("foreign_session")),
            ("through", json!(through + 1)),
            ("after", json!({"sourceSequence":through + 1,"itemIndex":0})),
            ("query", json!("")),
            ("query", json!("界".repeat(171))),
        ] {
            let mut invalid = request.clone(); invalid[key] = value;
            assert!(fixture.query("search", invalid).await.is_err(), "{key}");
        }
        let denied = fixture.app.client().start(
            fixture.binding(2).await, "search".into(), request,
            scope(&fixture.issuer, false), Attachments::new().into(),
        ).await;
        assert!(denied.is_err());
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn search_hits_read_the_exact_field_when_history_body_is_absent_or_different() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let fixture = Fixture::new("http://127.0.0.1:9/v1", None).await;
        let seed = fixture.create("Fields").await;
        let configuration = fixture.owner.session(&seed).await.unwrap().configuration;
        let session = format!("{}{}", fixture.owner.prefix, Uuid::new_v4().simple());
        let owner = PluginSession {
            session_id: session.clone(),
            creator: Namespace::new("source.fixture", StorageScope::Profile).unwrap(),
            fingerprint: "search-source-fields".into(), managed: false, authority_session_id: None,
        };

        let body = format!("user-body-hit {}", "中😀\0\n".repeat(1400));
        let thought = format!("thinking-hit {}", "中😀\0\n".repeat(1400));
        let args = json!({"argument-key-hit": "原始😀\"\n".repeat(1400)});
        let result = json!({"nested": {"result-key-hit": "原始输出😀".repeat(1400)}});
        let records = [
            Content::User { text: body.clone() },
            Content::Assistant { text: String::new(), model: None, thinking: Some(thought.clone()) },
            Content::Assistant { text: "body-hit-after-empty-thought".into(), model: None, thinking: Some("other-thinking-hit".into()) },
            Content::ToolCall { call_id: "call".into(), name: "tool-name-hit".into(), input: Some(args.clone()) },
            Content::ToolResult { call_id: "call".into(), output: result.clone(), is_error: false },
        ].into_iter().enumerate().map(|(i, content)| Record {
            source_message_id: format!("record-{i}"), source_turn_id: "turn".into(), timestamp: Some(1), content,
        }).collect();
        fixture.store.import_session(&owner, &Source {
            adapter: "fixture".into(), session_id: "original".into(),
        }, &configuration, records, 1).await.unwrap();

        let through = *fixture.store.subscribe_commits().borrow();
        for include in [false, true] {
            let mut position = json!({"kind":"start"});
            let mut thinking = String::new();
            let mut finished = false;
            for _ in 0..64 {
                let page = fixture.query("history", json!({"sessionId":session,"through":through,"position":position,"includeThinking":include})).await.unwrap();
                assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
                if page["kind"] == "preparing" { continue; }
                for chunk in page["chunks"].as_array().unwrap() {
                    if chunk["field"] == "thinking" {
                        assert!(include);
                        assert_eq!(chunk["role"], "assistant");
                        thinking.push_str(chunk["text"].as_str().unwrap());
                    } else { assert_eq!(chunk["field"], "body"); }
                }
                if page["after"].is_null() { finished = true; break; }
                position = json!({"kind":"after","cursor":page["after"]});
            }
            assert!(finished);
            assert_eq!(thinking, if include { thought.clone() + "other-thinking-hit" } else { String::new() });
        }
        for (query, field, expected, is_tool) in [
            ("user-body-hit", "body", body, false),
            ("THINKING-HIT 中", "thinking", thought, false),
            ("body-hit-after", "body", "body-hit-after-empty-thought".to_owned(), false),
            ("other-thinking-hit", "thinking", "other-thinking-hit".to_owned(), false),
            ("tool-name-hit", "tool_name", "tool-name-hit".to_owned(), true),
            ("argument-key-hit", "arguments", serde_json::to_string(&args).unwrap(), true),
            ("RESULT-KEY-HIT", "result", serde_json::to_string(&result).unwrap(), true),
        ] {
            let mut hit = None;
            for _ in 0..8 {
                let page = fixture.query("search", json!({"sessionId":session,"through":through,"after":null,"query":query,"caseSensitive":!matches!(field, "thinking" | "result"),"wholeWord":false,"includeInternal":false})).await.unwrap();
                if page["kind"] == "preparing" { continue; }
                assert_eq!(page["matches"].as_array().unwrap().len(), 1, "{query}");
                hit = Some(page["matches"][0].clone()); break;
            }
            let hit = hit.unwrap();
            assert_eq!(hit["field"], field);
            let sequence = hit["cursor"]["position"].clone();
            for reverse in [false, true] {
                let mut position = json!({"kind":if reverse {"tail"} else {"start"}});
                let mut text = String::new();
                let mut finished = false;
                for _ in 0..16 {
                    let page = fixture.query("source", json!({"sessionId":session,"through":through,
                        "source":{"kind":"row","position":sequence},"field":field,"position":position})).await.unwrap();
                    assert_eq!(page["kind"], "ready");
                    assert_eq!(page["through"], through);
                    let source = &page["source"];
                    let role = if query == "user-body-hit" { "user" } else if is_tool {
                        if field == "result" { "tool_result" } else { "tool_call" }
                    } else { "assistant" };
                    let fields = match role {
                        "user" => json!(["body"]),
                        "assistant" => json!(["thinking", "body"]),
                        "tool_call" => json!(["tool_name", "body", "arguments"]),
                        _ => json!(["body", "result"]),
                    };
                    assert_eq!(source["role"], role);
                    assert_eq!(source["timestamp"], 1);
                    assert_eq!(source["fields"], fields);
                    if !reverse && source["offset"] == 0 {
                        for other in fields.as_array().unwrap() {
                            let related = fixture.query("source", json!({"sessionId":session,"through":through,
                                "source":{"kind":"row","position":sequence},"field":other,"position":{"kind":"start"}})).await.unwrap();
                            assert_eq!(related["source"]["position"], sequence);
                            assert_eq!(related["source"]["role"], role);
                            assert_eq!(related["source"]["fields"], fields);
                            assert_eq!(related["source"]["timestamp"], source["timestamp"]);
                            if query == "THINKING-HIT 中" && other == "body" { assert_eq!(related["source"]["text"], ""); }
                        }
                    }
                    assert_eq!(source["position"], sequence);
                    assert_eq!(source["field"], field);
                    assert_eq!(source["totalBytes"], expected.len());
                    assert_eq!(source["tool"].is_object(), is_tool);
                    let part = source["text"].as_str().unwrap();
                    assert!(part.len() <= 8192 && serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
                    if reverse { text.insert_str(0, part); } else { text.push_str(part); }
                    let offset = source["offset"].as_u64().unwrap() + if reverse {0} else {part.len() as u64};
                    if offset == if reverse {0} else {expected.len() as u64} { finished = true; break; }
                    position = json!({"kind":if reverse {"before"} else {"after"},"offset":offset});
                }
                assert!(finished);
                assert_eq!(text, expected);
            }
            if is_tool {
                let body = fixture.query("source", json!({"sessionId":session,"through":through,
                    "source":{"kind":"row","position":sequence},"field":"body","position":{"kind":"start"}})).await.unwrap();
                let page = fixture.query("history", json!({"sessionId":session,"through":through,
                    "position":{"kind":"after","cursor":hit["cursor"]}})).await.unwrap();
                assert_eq!(body["source"]["field"], "body");
                assert_eq!(body["source"]["format"], "text");
                assert_eq!(body["source"]["position"], sequence);
                if body["source"]["totalBytes"] == 0 {
                    assert_eq!(body["source"]["text"], "");
                    assert_eq!(body["source"]["tool"]["call"]["position"], body["source"]["position"]);
                    assert_eq!(body["source"]["tool"]["call"]["name"], "tool-name-hit");
                    // A zero offset is already the end of an empty projected body.
                    // Exact source must retain the row even when history advances.
                    assert_ne!(page["chunks"][0]["position"], sequence);
                } else {
                    assert!(page["chunks"][0]["text"].as_str().unwrap().starts_with(body["source"]["text"].as_str().unwrap()), "history and source have independent page sizes but the same text prefix");
                    for field in ["position", "timestamp", "offset", "totalBytes", "tool"] {
                        assert_eq!(body["source"][field], page["chunks"][0][field], "tool body {field}");
                    }
                }
            }
            if query == "THINKING-HIT 中" {
                let page = fixture.query("history", json!({"sessionId":session,"through":through,"position":{"kind":"after","cursor":hit["cursor"]}})).await.unwrap();
                assert_ne!(page["chunks"][0]["position"], sequence, "body projection must not be mistaken for the search source");
                for (key, value) in [("sessionId", json!("foreign")), ("through", json!(0)), ("field", json!("arguments"))] {
                    let mut input = json!({"sessionId":session,"through":through,"source":{"kind":"row","position":sequence},"field":field,"position":{"kind":"start"}});
                    input[key] = value;
                    assert!(fixture.query("source", input).await.is_err(), "{key}");
                }
                let denied = fixture.app.client().start(fixture.binding(2).await, "source".into(),
                    json!({"sessionId":session,"through":through,"source":{"kind":"row","position":sequence},"field":field,"position":{"kind":"start"}}),
                    scope(&fixture.issuer, false), Attachments::new().into()).await;
                assert!(denied.is_err());
            }
        }
        fixture.app.shutdown().await.unwrap();
        fixture.config.shutdown().await.unwrap();
    }).await.unwrap();
}

#[tokio::test]
async fn reading_prompt_is_indexed_fenced_and_keeps_original_session_authority() {
    let fixture = Fixture::new("http://127.0.0.1:9/v1", None).await;
    let seed = fixture.create("Reading context").await;
    let configuration = fixture.owner.session(&seed).await.unwrap().configuration;
    let session = format!("{}{}", fixture.owner.prefix, Uuid::new_v4().simple());

    fixture
        .store
        .import_session(
            &PluginSession {
                session_id: session.clone(),
                creator: Namespace::new("prompt.fixture", StorageScope::Profile).unwrap(),
                fingerprint: "prompt-import".into(),
                managed: false,
                authority_session_id: None,
            },
            &Source {
                adapter: "fixture".into(),
                session_id: "source".into(),
            },
            &configuration,
            vec![
                Record {
                    source_message_id: "u1".into(),
                    source_turn_id: "t1".into(),
                    timestamp: Some(1),
                    content: Content::User {
                        text: "First question".into(),
                    },
                },
                Record {
                    source_message_id: "a1".into(),
                    source_turn_id: "t1".into(),
                    timestamp: Some(2),
                    content: Content::Assistant {
                        text: "answer\n".repeat(100_000),
                        model: None,
                        thinking: None,
                    },
                },
                Record {
                    source_message_id: "u2".into(),
                    source_turn_id: "t2".into(),
                    timestamp: Some(3),
                    content: Content::User { text: "?".into() },
                },
            ],
            1,
        )
        .await
        .unwrap();

    let through = *fixture.store.subscribe_commits().borrow();
    let read = json!({"sessionId":session,"through":through,"position":{"kind":"start"},"view":"conversation"});
    let first = loop {
        let page = fixture.query("history", read.clone()).await.unwrap();
        if page["kind"] == "ready" {
            break page;
        }
    };
    let first_user = first["chunks"][0]["position"].clone();
    let answer = first["chunks"][1]["position"].clone();
    assert_eq!(
        fixture
            .query(
                "historyPrompt",
                json!({"sessionId":session,"through":through,"position":answer})
            )
            .await
            .unwrap(),
        json!({"position":first_user,"text":"First question"})
    );
    let last = fixture.query("history", json!({"sessionId":session,"through":through,"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
    let last_user = last["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|chunk| chunk["role"] == "user")
        .unwrap()["position"]
        .clone();
    assert_eq!(
        fixture
            .query(
                "historyPrompt",
                json!({"sessionId":session,"through":through,"position":last_user})
            )
            .await
            .unwrap(),
        json!({"position":last_user,"text":"?"})
    );
    assert!(
        fixture
            .query(
                "historyPrompt",
                json!({"sessionId":session,"through":first_user["sourceSequence"],"position":last_user})
            )
            .await
            .is_err()
    );
    assert!(
        fixture
            .query(
                "historyPrompt",
                json!({"sessionId":"foreign-session","through":through,"position":answer})
            )
            .await
            .is_err()
    );
}
