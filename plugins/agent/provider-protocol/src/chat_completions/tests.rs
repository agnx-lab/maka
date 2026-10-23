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
use crate::output::{Event, Part, Stop, TextKind};
use serde_json::json;

#[test]
fn done_stops_current_batch_and_finish_is_idempotent() {
    let wire = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"before\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"after\"}}]}\n\n"
    );
    let mut stream = Stream::new("vendor".into());
    let mut id = || "id".into();
    let events = stream.push(wire.as_bytes(), &mut id).unwrap();
    assert!(stream.terminal());
    let texts = events
        .iter()
        .filter_map(|v| {
            if let Event::TextDelta { text, .. } = v {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(texts, ["before"]);
    assert!(stream.push(wire.as_bytes(), &mut id).unwrap().is_empty());
    let finish = stream.finish(&mut id).unwrap();
    assert!(matches!(&finish[..],[Event::TextEnd{..},Event::Finish(v)] if v.reason==Stop::Stop));
    assert!(stream.finish(&mut id).unwrap().is_empty());
}
#[test]
fn fragment_boundaries_preserve_late_tool_names_and_reused_indices() {
    let records = [
        json!({"id":"response","choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"{\"x\":"}}]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":1,"id":"same","function":{"name":"first","arguments":"1}"}}]}}]}),
        json!({"choices":[{"delta":{"tool_calls":[{"index":1,"id":"same","function":{"name":"second","arguments":"{\"x\":2}"}}]},"finish_reason":"tool_calls"}]}),
    ];
    let wire = records
        .iter()
        .map(|r| format!("data: {r}\n\n"))
        .collect::<String>();
    let mut stream = Stream::new("vendor".into());
    let mut id = || "same".into();
    let mut events = vec![];
    for byte in wire.bytes() {
        events.extend(stream.push(&[byte], &mut id).unwrap());
    }
    events.extend(stream.finish(&mut id).unwrap());
    let calls = events
        .iter()
        .filter_map(|e| {
            if let Event::ToolCall(c) = e {
                Some((c.id.as_str(), c.name.as_str(), c.input.as_str()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        calls,
        [
            ("same", "first", "{\"x\":1}"),
            ("same-1", "second", "{\"x\":2}")
        ]
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, Event::Response { .. }))
            .count(),
        1
    );
}
#[test]
fn native_response_keeps_reasoning_signatures_and_extended_usage() {
    let body = json!({"id":"r","created":4,"choices":[{"message":{"role":"assistant","reasoning_content":"think","content":[{"type":"text","text":"answer"},{"type":"future","payload":1}],"tool_calls":[{"id":"call","function":{"name":"f","arguments":"{}"},"extra_content":{"google":{"thought_signature":"sig"}}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":4,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":6},"completion_tokens_details":{"reasoning_tokens":3},"vendor_count":9}});
    let response = generate(body.to_string().as_bytes(), "vendor", &mut || {
        "generated".into()
    })
    .unwrap();
    assert_eq!(response.created_at_unix_seconds, Some(4.0));
    assert!(
        matches!(&response.parts[..],[Part::Text{kind:TextKind::Reasoning,text,..},Part::Text{kind:TextKind::Text,..},Part::ToolCall(call)] if text=="think"&&call.extensions.as_ref().unwrap()["vendor"]["thoughtSignature"]=="sig")
    );
    let usage = response.finish.usage.unwrap();
    assert_eq!(usage.uncached_input, Some(-2));
    assert_eq!(usage.text, Some(0));
    assert_eq!(usage.raw.unwrap()["vendor_count"], 9);
}
#[test]
fn invalid_frames_do_not_fabricate_successful_completion() {
    let mut stream = Stream::new("vendor".into()).with_raw_records(true);
    let mut id = || "id".into();
    let events = stream
        .push(
            b"data: {\"choices\":[{\"delta\":{\"role\":\"user\"}}]}\n\n",
            &mut id,
        )
        .unwrap();
    assert!(matches!(&events[..], [Event::Raw(_), Event::Error(_)]));
    assert!(
        matches!(&stream.finish(&mut id).unwrap()[..],[Event::Finish(v)] if v.reason==Stop::Error)
    );
    let mut stream = Stream::new("vendor".into());
    assert!(
        matches!(&stream.finish(&mut id).unwrap()[..],[Event::Error(_),Event::Finish(v)] if v.reason==Stop::Error)
    );
}

#[test]
fn reasoning_observations_preserve_empty_carriers_and_prose_order() {
    use crate::output::ReasoningField;
    let records = [
        json!({"choices":[{"delta":{"reasoning_content":""}}]}),
        json!({"choices":[{"delta":{"content":"answer"}}]}),
        json!({"choices":[{"delta":{"reasoning":"","reasoning_content":"preferred"}}]}),
        json!({"choices":[{"delta":{"reasoning":"later"},"finish_reason":"stop"}]}),
    ];
    let wire = records
        .iter()
        .map(|r| format!("data: {r}\n\n"))
        .collect::<String>();
    let mut stream = Stream::new("vendor".into()).with_reasoning_observations(true);
    let mut events = stream.push(wire.as_bytes(), &mut || "id".into()).unwrap();
    events.extend(stream.finish(&mut || "id".into()).unwrap());
    let deltas = events
        .iter()
        .filter_map(|event| match event {
            Event::TextDelta {
                kind,
                text,
                reasoning_field,
                ..
            } => Some((*kind, text.as_str(), *reasoning_field)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        deltas,
        [
            (TextKind::Reasoning, "", Some(ReasoningField::Content)),
            (TextKind::Text, "answer", None),
            (
                TextKind::Reasoning,
                "preferred",
                Some(ReasoningField::Content)
            ),
            (
                TextKind::Reasoning,
                "later",
                Some(ReasoningField::Reasoning)
            ),
        ]
    );
    let lifecycle = events
        .iter()
        .filter_map(|event| match event {
            Event::TextStart { kind, .. } => Some((true, *kind)),
            Event::TextEnd { kind, .. } => Some((false, *kind)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        lifecycle,
        [
            (true, TextKind::Reasoning),
            (false, TextKind::Reasoning),
            (true, TextKind::Text),
            (false, TextKind::Text),
            (true, TextKind::Reasoning),
            (false, TextKind::Reasoning),
        ]
    );
}

#[test]
fn image_detail_survives_wire_encoding_without_affecting_video() {
    use crate::input::{File, FileSource, Message, UserPart};
    let options = request::PromptOptions {
        provider_namespace: "vendor".into(),
        metadata_namespace: "vendor".into(),
        multipart_tool_content: false,
    };
    for detail in [
        None,
        Some("auto"),
        Some("low"),
        Some("high"),
        Some("original"),
    ] {
        let image = File {
            media_type: "image/png".into(),
            filename: None,
            source: FileSource::Url("https://example.test/red.png".into()),
            extensions: detail.map(|detail| {
                serde_json::from_value(json!({"openai":{"imageDetail":detail}})).unwrap()
            }),
        };
        let output = messages(
            &[Message::User {
                parts: vec![UserPart::File(image)],
                extensions: None,
            }],
            &options,
        )
        .unwrap();
        let image = &output[0]["content"][0]["image_url"];
        assert_eq!(image["url"], "https://example.test/red.png");
        assert_eq!(
            image.get("detail"),
            detail.map(serde_json::Value::from).as_ref()
        );
    }
    let video = File {
        media_type: "video/mp4".into(),
        filename: None,
        source: FileSource::Url("https://example.test/red.mp4".into()),
        extensions: Some(serde_json::from_value(json!({"openai":{"imageDetail":"high"}})).unwrap()),
    };
    let output = messages(
        &[Message::User {
            parts: vec![UserPart::File(video)],
            extensions: None,
        }],
        &options,
    )
    .unwrap();
    assert_eq!(
        output[0]["content"][0],
        json!({"type":"video_url","video_url":{"url":"https://example.test/red.mp4"}})
    );
}
