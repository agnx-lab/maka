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
use serde_json::json;

#[test]
fn mcp_replay_preserves_opaque_results_without_unsupported_warnings() {
    use super::prompt::{PreparedPrompt, PromptOptions};
    use crate::input::{AssistantPart, Message, ToolCall, ToolOutput, ToolResult, ToolValue};
    use std::collections::BTreeMap;

    for is_error in [false, true] {
        // A hosted-tool spelling must not reinterpret an MCP server's payload.
        let payload = json!({"errorCode":"opaque", "content":[{"type":"text","text":"answer"}]});
        let messages = [Message::Assistant {
            parts: vec![
                AssistantPart::ToolCall(ToolCall {
                    id: "remote-1".into(),
                    name: "web_search".into(),
                    input: json!({"q":"native"}),
                    provider_executed: true,
                    extensions: Some(BTreeMap::from([(
                        "anthropic".into(),
                        json!({"type":"mcp-tool-use","serverName":"research"}),
                    )])),
                }),
                AssistantPart::ToolResult(ToolResult {
                    id: "remote-1".into(),
                    name: "web_search".into(),
                    output: ToolOutput {
                        value: if is_error {
                            ToolValue::ErrorJson(payload.clone())
                        } else {
                            ToolValue::Json(payload.clone())
                        },
                        extensions: None,
                    },
                    extensions: None,
                }),
            ],
            extensions: None,
        }];
        let names = BTreeMap::new();
        let mut cache = CacheBudget::default();
        let prompt = PreparedPrompt::encode(
            &messages,
            PromptOptions {
                send_reasoning: true,
                cache_namespace: "anthropic",
                tool_names: &names,
                toolsets: &names,
            },
            &mut cache,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&prompt).unwrap(),
            json!({"messages":[{
                "role":"assistant","content":[
                    {"type":"mcp_tool_use","id":"remote-1","name":"web_search","input":{"q":"native"},"server_name":"research"},
                    {"type":"mcp_tool_result","tool_use_id":"remote-1","content":payload,"is_error":is_error}
                ]
            }]})
        );
        assert!(prompt.warnings.is_empty());
        assert!(cache.warnings().is_empty());
    }
}

fn function(name: &str, cache: Option<CacheDirective>) -> Tool {
    Tool::Function(FunctionTool {
        name: name.into(),
        description: None,
        input_schema: json!({"type":"object"}),
        cache_control: cache,
        strict: Some(true),
        defer_loading: None,
        eager_input_streaming: None,
        allowed_callers: None,
        input_examples: None,
    })
}

#[test]
fn cache_capacity_is_shared_by_messages_and_definitions_but_not_requests() {
    let cache = Some(CacheDirective::Ephemeral { ttl: None });
    let mut budget = CacheBudget::default();
    assert_eq!(budget.reserve(cache, CacheSite::Reasoning), None);
    for site in [CacheSite::System, CacheSite::Text, CacheSite::ToolResult] {
        assert_eq!(budget.reserve(cache, site), cache);
    }
    let prepared = PreparedTools::build(
        [function("first", cache), function("second", cache)],
        None,
        None,
        ToolPolicy::default(),
        &mut budget,
    );
    let wire = prepared.wire_tools();
    assert_eq!(wire[0]["cache_control"], json!({"type":"ephemeral"}));
    assert!(wire[1].get("cache_control").is_none());
    assert_eq!(
        budget.warnings(),
        &[
            Warning::CacheNotAllowed(CacheSite::Reasoning),
            Warning::CacheLimit { attempted: 5 }
        ]
    );
    assert_eq!(
        CacheBudget::default().reserve(cache, CacheSite::System),
        cache
    );
}

#[test]
fn fallback_usage_excludes_the_failed_primary_and_advisor() {
    let usage: Usage = serde_json::from_value(json!({
        "input_tokens":80,"output_tokens":11,"cache_read_input_tokens":4,
        "cache_creation_input_tokens":2,"output_tokens_details":{"thinking_tokens":3},
        "iterations":[
            {"type":"message","input_tokens":900,"output_tokens":0},
            {"type":"fallback_message","input_tokens":80,"output_tokens":11},
            {"type":"advisor_message","input_tokens":50,"output_tokens":5}
        ]
    }))
    .unwrap();
    assert_eq!(
        usage.totals().unwrap(),
        TokenCounts {
            input: 86,
            uncached_input: 80,
            cache_read: 4,
            cache_write: 2,
            output: 11,
            reasoning: Some(3)
        }
    );
}

#[test]
fn compaction_accounting_counts_only_executor_work_and_rejects_overflow() {
    let mut usage: Usage = serde_json::from_value(json!({
        "input_tokens":1,"output_tokens":1,
        "iterations":[
            {"type":"compaction","input_tokens":17,"output_tokens":3},
            {"type":"message","input_tokens":6,"output_tokens":9},
            {"type":"advisor_message","input_tokens":100,"output_tokens":100}
        ]
    }))
    .unwrap();
    assert_eq!(
        usage.totals().unwrap(),
        TokenCounts {
            input: 23,
            uncached_input: 23,
            cache_read: 0,
            cache_write: 0,
            output: 12,
            reasoning: None
        }
    );
    usage.cache_read_input_tokens = Some(u64::MAX);
    assert!(matches!(usage.totals(), Err(Error::InvalidResponse { .. })));
}

#[test]
fn forced_choice_fallback_restricts_the_available_tools() {
    let prepared = PreparedTools::build(
        [function("lookup", None), function("write", None)],
        Some(ToolChoice::Named("lookup".into())),
        Some(false),
        ToolPolicy {
            forced_choice: false,
            strict: false,
            ..ToolPolicy::default()
        },
        &mut CacheBudget::default(),
    );
    assert_eq!(
        prepared.wire_tools(),
        vec![json!({"name":"lookup","input_schema":{"type":"object"}})]
    );
    assert_eq!(
        serde_json::to_value(&prepared.choice).unwrap(),
        json!({"type":"auto","disable_parallel_tool_use":false})
    );
    assert_eq!(
        prepared.warnings.last(),
        Some(&Warning::ForcedToolChoiceIgnored {
            name: Some("lookup".into())
        })
    );
}

#[test]
fn dated_web_tool_capabilities_are_encoded_at_the_wire_boundary() {
    let options = json!({"maxUses":2,"allowedDomains":["example.org"],"useCache":false,"responseInclusion":"excluded"});
    let legacy = HostedTool::from_definition("anthropic.web_fetch_20250910", &options)
        .unwrap()
        .unwrap();
    let current = HostedTool::from_definition("anthropic.web_fetch_20260318", &options)
        .unwrap()
        .unwrap();
    assert_eq!(
        legacy.wire(),
        json!({"type":"web_fetch_20250910","name":"web_fetch","max_uses":2,"allowed_domains":["example.org"]})
    );
    assert_eq!(
        current.wire(),
        json!({"type":"web_fetch_20260318","name":"web_fetch","max_uses":2,"allowed_domains":["example.org"],"use_cache":false,"response_inclusion":"excluded"})
    );
    for budget in [json!(1000), json!(1024.5)] {
        assert!(
            HostedTool::from_definition(
                "anthropic.advisor_20260301",
                &json!({"model":"advisor","maxTokens":budget})
            )
            .is_err()
        );
    }
}

#[test]
fn constrained_schema_keeps_guidance_and_nested_object_boundaries() {
    let schema = json!({
        "type":"object","additionalProperties":true,"required":["count"],
        "properties":{
            "count":{"type":"integer","description":"Items","minimum":2,"maximum":7},
            "choice":{"oneOf":[{"type":"string"},{"type":"null"}],"default":null},
            "reference":{"$ref":"#/$defs/entry","title":"ignored"}
        }
    });
    assert_eq!(
        constrained_schema(schema),
        json!({
            "type":"object","additionalProperties":false,"required":["count"],
            "properties":{
                "count":{"type":"integer","description":"Items\nminimum: 2; maximum: 7."},
                "choice":{"anyOf":[{"type":"string"},{"type":"null"}],"default":null},
                "reference":{"$ref":"#/$defs/entry"}
            }
        })
    );
}

#[test]
fn response_sources_follow_documents_and_search_results_without_losing_citations() {
    use crate::output::{Part, Source, Stop};
    let raw = json!({"type":"message","id":"reply-1","model":"model-1","usage":{"input_tokens":7,"output_tokens":2},"stop_reason":"end_turn","content":[
        {"type":"web_fetch_tool_result","tool_use_id":"fetch-1","content":{"type":"web_fetch_result","url":"https://example.org/document","retrieved_at":null,"content":{"type":"document","title":null,"source":{"type":"text","media_type":"text/plain","data":"evidence"}}}},
        {"type":"text","text":"answer","citations":[
            {"type":"char_location","cited_text":"evidence","document_index":0,"document_title":null,"start_char_index":0,"end_char_index":8},
            {"type":"web_search_result_location","cited_text":"answer","url":"https://example.org/search","title":null,"encrypted_index":"citation-1"}
        ]},
        {"type":"web_search_tool_result","tool_use_id":"search-1","content":[{"type":"web_search_result","url":"https://example.org/search","title":null,"encrypted_content":"opaque"}]}
    ]});
    let mut ids = 0;
    let decoded = response::decode(
        raw.to_string().as_bytes(),
        response::Options::default(),
        &mut || {
            ids += 1;
            format!("source-{ids}")
        },
    )
    .unwrap();
    assert_eq!(decoded.finish.reason, Stop::Stop);
    assert_eq!(
        ids, 2,
        "search results suppress duplicate standalone web citations"
    );
    let Part::Text { citations, .. } = &decoded.parts[1] else {
        panic!("text after fetched result")
    };
    assert_eq!(
        citations.len(),
        2,
        "attachments survive standalone source suppression"
    );
    assert!(
        matches!(&citations[0].source, Source::Document { id, title, .. } if id == "0" && title == "https://example.org/document")
    );
    assert!(
        matches!(&decoded.parts[2], Part::Source(Source::Document { id, .. }) if id == "source-1")
    );
    assert!(matches!(&decoded.parts[4], Part::Source(Source::Url { id, .. }) if id == "source-2"));
}

#[test]
fn hosted_boundary_rejects_wrong_discriminators_and_document_media_pairs() {
    for content in [
        json!({"type":"something_else","url":"https://example.org","retrieved_at":null,"content":{"type":"document","title":null,"source":{"type":"text","media_type":"text/plain","data":"x"}}}),
        json!({"type":"web_fetch_result","url":"https://example.org","retrieved_at":null,"content":{"type":"not_document","title":null,"source":{"type":"text","media_type":"text/plain","data":"x"}}}),
        json!({"type":"web_fetch_result","url":"https://example.org","retrieved_at":null,"content":{"type":"document","title":null,"source":{"type":"text","media_type":"application/pdf","data":"x"}}}),
    ] {
        let raw = json!({"type":"message","usage":{"input_tokens":0,"output_tokens":0},"content":[{"type":"web_fetch_tool_result","tool_use_id":"x","content":content}]});
        assert!(
            response::decode(
                raw.to_string().as_bytes(),
                response::Options::default(),
                &mut || "unused".into()
            )
            .is_err(),
            "accepted {raw}"
        );
    }
}

#[test]
fn stream_eof_reuses_only_completed_accounting_and_spliced_messages_latch() {
    use crate::output::{Event, Stop};
    fn feed(stream: &mut stream::Stream, value: serde_json::Value) -> Vec<Event> {
        stream.value(value, &mut || "unused".into())
    }
    let mut stream = stream::Stream::new(response::Options::default());
    feed(
        &mut stream,
        json!({"type":"message_start","message":{"id":"one","usage":{"input_tokens":3}}}),
    );
    feed(
        &mut stream,
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4}}),
    );
    feed(&mut stream, json!({"type":"message_stop"}));
    feed(
        &mut stream,
        json!({"type":"message_start","message":{"id":"two","usage":{"input_tokens":99}}}),
    );
    feed(
        &mut stream,
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
    );
    feed(
        &mut stream,
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}),
    );
    let events = stream.finish();
    let [Event::Finish(finish)] = events.as_slice() else {
        panic!("interrupted finish")
    };
    assert_eq!(finish.reason, Stop::Interrupted);
    assert_eq!(finish.usage.as_ref().unwrap().input, Some(3));
    assert_eq!(finish.usage.as_ref().unwrap().output, Some(4));
    assert!(stream.finish().is_empty());

    let mut stream = stream::Stream::new(response::Options::default());
    let start = json!({"type":"message_start","message":{"id":"one","usage":{"input_tokens":3}}});
    feed(&mut stream, start.clone());
    assert!(
        feed(&mut stream, start).is_empty(),
        "duplicate start is idempotent"
    );
    let events = feed(
        &mut stream,
        json!({"type":"message_start","message":{"id":"other","usage":{"input_tokens":7}}}),
    );
    assert!(matches!(
        events.as_slice(),
        [Event::Error(Error::InvalidResponse { .. })]
    ));
    assert!(feed(&mut stream, json!({"type":"message_stop"})).is_empty());
    assert!(stream.finish().is_empty());
}

#[test]
fn json_tool_mode_suppresses_prose_and_preserves_explicit_null_citations() {
    use crate::output::{Event, Part};
    let mut stream = stream::Stream::new(response::Options {
        json_tool: true,
        ..response::Options::default()
    });
    for value in [
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me answer"}}),
        json!({"type":"content_block_stop","index":0}),
    ] {
        assert!(stream.value(value, &mut || "unused".into()).is_empty());
    }
    assert!(
        stream.finish().is_empty(),
        "suppressed prose is not interrupted output"
    );
    let citation = json!({"type":"content_block_location","cited_text":"proof","document_index":0,"document_title":null,"file_id":null,"start_block_index":0,"end_block_index":1});
    let raw = json!({"type":"message","usage":{"input_tokens":0,"output_tokens":1},"content":[{"type":"text","text":"answer","citations":[citation]}]});
    let response = response::decode(
        raw.to_string().as_bytes(),
        response::Options::default(),
        &mut || "unused".into(),
    )
    .unwrap();
    let Part::Text {
        extensions: Some(metadata),
        ..
    } = &response.parts[0]
    else {
        panic!("citation metadata")
    };
    assert_eq!(metadata["anthropic"]["citations"][0], citation);

    let mut stream = stream::Stream::new(response::Options::default());
    stream.value(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}), &mut || "unused".into());
    let errors = stream.value(json!({"type":"error","error":{"type":"rate_limit_error","message":"retry later","statusCode":503,"isRetryable":false,"data":null}}), &mut || "unused".into());
    let [Event::ProviderError(error)] = errors.as_slice() else {
        panic!("provider error")
    };
    assert_eq!(error.status, Some(503));
    assert_eq!(error.retryable, Some(false));
    assert_eq!(error.data, serde_json::Value::Null);
    assert!(
        stream.finish().is_empty(),
        "an error must not produce a second interrupted finish"
    );
}
