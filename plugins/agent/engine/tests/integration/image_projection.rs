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

use crate::support::image_projection as support;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use maka_agent_engine::RunError;
use maka_agent_facts::event::EventWrite;
use maka_agent_facts::event::Fact;
use maka_agent_facts::event::InvocationOutcome;
use maka_agent_facts::event::RuntimeEvent;
use maka_agent_facts::input::InvocationInput;
use maka_agent_facts::tool_call::ToolCallIdentity;
use maka_agent_facts::tool_output::ImageOutput;
use maka_agent_facts::tool_output::ToolOutput;
use maka_event_log::EventLog;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use support::*;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const FOUR_MIB: usize = 4 * 1024 * 1024;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reopened_images_preserve_complete_accepted_artifacts_and_copied_authority() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events.sqlite");
        let log = EventLog::open(&path).await.unwrap();
        log.create_session("session", "create", &json!({}), 1).await.unwrap();
        for id in ["historical", "tool", "fits"] { artifact(&log, id, FOUR_MIB, true).await; }
        artifact(&log, "invalid", FOUR_MIB + 1, false).await;
        artifact(&log, "overflow", 8, true).await;
        artifact(&log, "accepted-50-mib", 50 * 1024 * 1024, true).await;
        let prior = identity("historical");
        let mut historical = attachment("historical");
        historical["bytes"] = json!(FOUR_MIB);
        let message = serde_json::from_value(json!({"text":"historical", "attachments":[historical]})).unwrap();
        let facts = [
            Fact::InvocationOpened { input: InvocationInput::Message { source_messages: Vec::new(), content: message, request_fingerprint: None }, configuration: None },
            Fact::ModelRequested { item_acceptance: false, effective_source_digest: None, purpose: maka_agent_facts::context::ModelPurpose::Main, context: None, context_breakdown: None, checkpoint_event_id: None, step_id: "prior-step".into(), model_id: "test".into(),
                source_scope: maka_agent_facts::event::LogScope::Session { id: "session".into() },
                source_high_water: 1, source_digest: "fixture".into(), input_digest: "fixture".into(), route_identity: "fixture".into() },
            Fact::ModelCompleted { step_id: "prior-step".into(), output: serde_json::from_value(json!({
                "parts":[{"kind":"tool_call","call":{"id":"read","name":"Read","input":{"kind":"json","data":{}},"provider_executed":false}}],
                "finish_reason":"tool-calls", "usage":{}})).unwrap() },
            Fact::ToolDispatched { presentation: None,
        title: None, operation_id: "prior-step:read".into(), call: ToolCallIdentity::provider("prior-step".into(), "read".into()), name: "Read".into(), input: json!({}) },
        ];
        for fact in facts { log.append(&EventWrite::plain(RuntimeEvent::new(prior.clone(), fact)).unwrap()).await.unwrap(); }
        log.append(&EventWrite::tool_success("prior-result".into(), std::time::SystemTime::now(), prior.clone(), "prior-step:read".into(),
            ToolOutput::Image(ImageOutput { detail: Some(maka_agent_facts::tool_output::ImageDetail::High), mime_type: "image/png".into(), reference: serde_json::from_value(attachment("tool")["ref"].clone()).unwrap() }).into()).unwrap().0).await.unwrap();
        log.append(&EventWrite::plain(RuntimeEvent::new(prior, Fact::InvocationEnded { outcome: InvocationOutcome::Completed })).unwrap()).await.unwrap();
        let revision = log.get_session::<Value>("session").await.unwrap().unwrap().revision;
        assert!(matches!(log.copy_session(maka_event_log::sessions::SessionCopy {
            source_session_id: "session".into(), target_session_id: "branch".into(),
            expected_source_revision: revision, purpose: maka_agent_facts::session::CopyPurpose::Branch { turn_id: None, side_conversation: false },
        }, &json!({}), 2).await.unwrap(), maka_event_log::sessions::SessionCopyResult::Committed(_)));
        log.close().await.unwrap();

        let log = Arc::new(EventLog::open(&path).await.unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let observed_log = log.clone();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let body = request(&mut socket).await;
            let prefix = observed_log.prefix(100, 128 * 1024).await.unwrap();
            assert!(prefix.events.iter().any(|stored| stored.event.invocation.invocation_id == "invocation-current"
                && matches!(stored.event.fact, Fact::ModelRequested { .. })));
            respond(&mut socket).await;
            body
        });
        let mut foreign = attachment("historical");
        foreign["ref"]["sessionId"] = json!("other");
        let mut workspace = attachment("workspace");
        workspace["ref"] = json!({"kind":"workspace_file", "relativePath":"historical"});
        let engine = engine(log.clone());
        engine.run(input(&base, json!([attachment("invalid"), attachment("missing"), foreign, workspace,
            attachment("fits"), attachment("overflow"), attachment("accepted-50-mib")])), CancellationToken::new()).await.unwrap();
        let body = server.await.unwrap();
        let mut urls = Vec::new();
        collect_images(&body["messages"], &mut urls);
        assert_eq!(urls.len(), 5, "prior user + typed Read + all three current images must reach the provider");
        for (url, size) in urls.into_iter().zip([FOUR_MIB, FOUR_MIB, FOUR_MIB, 8, 50 * 1024 * 1024]) {
            let data = url.strip_prefix("data:image/png;base64,").unwrap();
            let bytes = STANDARD.decode(data).unwrap();
            assert!(bytes.len() == size, "provider image length differs from the accepted artifact");
            assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
            assert!(bytes[8..].iter().all(|byte| *byte == 0), "provider image bytes differ from the accepted artifact");
        }
        let text = body["messages"].to_string();
        assert!(text.contains("\"detail\":\"high\""), "tool image detail reaches the provider after reopening");
        for name in ["invalid", "missing", "historical", "workspace"] {
            assert!(text.contains(&format!("Image attachment \\\"{name}\\\" could not be loaded")), "missing error for {name}");
        }
        assert!(text.contains("unsupported_mime"));
        assert!(!text.contains("per-request image budget"));
        assert!(text.contains("session_mismatch"));
        assert!(text.contains("unsupported_ref_kind"));
        let prefix = log.prefix(100, 128 * 1024).await.unwrap();
        let canonical = serde_json::to_string(&prefix).unwrap();
        assert!(!canonical.contains("data:image"), "projection must not persist image bytes");
        assert!(canonical.len() < 32 * 1024);
        use maka_event_log::artifacts::ArtifactDeletion;
        for (id, expected) in [("historical", ArtifactDeletion::Deleted), ("tool", ArtifactDeletion::Protected)] {
            assert_eq!(log.delete_user_artifact("session", id).await.unwrap(), expected, "{id}");
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let copied_request = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let body = request(&mut socket).await;
            respond(&mut socket).await;
            body
        });
        let mut copied = input(&base, json!([]));
        copied.invocation = identity("branch-current");
        copied.invocation.session_id = "branch".into();
        engine.run(copied, CancellationToken::new()).await.unwrap();
        let copied_request = copied_request.await.unwrap();
        let mut inherited_images = Vec::new();
        collect_images(&copied_request["messages"], &mut inherited_images);
        assert_eq!(inherited_images.len(), 2, "copy retains the deleted user upload and protected tool image through destination-owned references");
        let reference = log.resolve_history_artifact("branch", "session", "historical").await.unwrap().unwrap();
        assert!(copied_request["messages"].to_string().contains(&reference.resource_ref().unwrap()),
            "Read must receive the destination-owned resource URI");
        let tool_reference = log.resolve_history_artifact("branch", "session", "tool").await.unwrap().unwrap();
        let maka_agent_facts::attachment::StorageRef::SessionFile { relative_path, .. } = tool_reference else { panic!("owned tool image") };
        assert!(log.read_tool_result("branch", "prior-result").await.unwrap().unwrap().serialized_result.contains(&relative_path),
            "tool-result Read renders the copied image's locator, not the source artifact ID");
        let original = log.prefix(100, 128 * 1024).await.unwrap();
        let source_opening = original.events.iter().find(|e| e.event.invocation.invocation_id == "invocation-historical"
            && matches!(e.event.fact, Fact::InvocationOpened { .. })).unwrap();
        assert!(!serde_json::to_string(&source_opening.event).unwrap().contains("history-"),
            "rendering aliases cannot rewrite canonical input");
        engine.drain().await;
    }).await.expect("image projection must make bounded progress");
}

fn collect_images<'a>(value: &'a Value, urls: &mut Vec<&'a str>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_images(value, urls);
            }
        }
        Value::Object(fields) => {
            if let Some(url) = fields
                .get("image_url")
                .and_then(|value| value["url"].as_str())
            {
                urls.push(url);
            }
            for value in fields.values() {
                collect_images(value, urls);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn cancelled_image_turn_does_not_commit_or_send_a_model_request() {
    let directory = tempfile::tempdir().unwrap();
    let log = Arc::new(
        EventLog::open(&directory.path().join("events.sqlite"))
            .await
            .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let engine = engine(log.clone());
    assert!(matches!(
        engine
            .run(input(&base, json!([attachment("missing")])), cancellation)
            .await,
        Err(RunError::Cancelled)
    ));
    engine.drain().await;
    assert!(
        !log.prefix(100, 128 * 1024)
            .await
            .unwrap()
            .events
            .iter()
            .any(|stored| matches!(stored.event.fact, Fact::ModelRequested { .. }))
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(20), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prepared_context_breakdown_measures_projected_roles_and_freezes_literal_counts() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("events.sqlite");
        let log = Arc::new(EventLog::open(&path).await.unwrap());
        log.create_session("session", "create", &json!({}), 1).await.unwrap();
        artifact(&log, "tool", 8, true).await;
        let prior = identity("historical");
        for fact in [
            Fact::InvocationOpened {
                input: InvocationInput::Message { source_messages: Vec::new(), content: "é".into(), request_fingerprint: None },
                configuration: None,
            },
            Fact::ModelRequested {
                item_acceptance: false, effective_source_digest: None,
                purpose: maka_agent_facts::context::ModelPurpose::Main,
                context: None, context_breakdown: None, checkpoint_event_id: None,
                step_id: "prior-step".into(), model_id: "test".into(),
                source_scope: maka_agent_facts::event::LogScope::Session { id: "session".into() },
                source_high_water: 1, source_digest: "fixture".into(), input_digest: "fixture".into(), route_identity: "fixture".into(),
            },
            Fact::ModelCompleted { step_id: "prior-step".into(), output: serde_json::from_value(json!({
                "parts":[{"kind":"tool_call","call":{"id":"read","name":"Read","input":{"kind":"json","data":{}},"provider_executed":false}}],
                "finish_reason":"tool-calls", "usage":{}})).unwrap() },
            Fact::ToolDispatched { presentation: None, title: None,
                operation_id: "prior-step:read".into(), call: ToolCallIdentity::provider("prior-step".into(), "read".into()),
                name: "Read".into(), input: json!({}),
            },
        ] {
            log.append(&EventWrite::plain(RuntimeEvent::new(prior.clone(), fact)).unwrap()).await.unwrap();
        }
        log.append(&EventWrite::tool_success("prior-result".into(), std::time::SystemTime::now(), prior.clone(), "prior-step:read".into(),
            ToolOutput::Image(ImageOutput { detail: None, mime_type: "image/png".into(), reference: serde_json::from_value(attachment("tool")["ref"].clone()).unwrap() }).into()).unwrap().0).await.unwrap();
        log.append(&EventWrite::plain(RuntimeEvent::new(prior, Fact::InvocationEnded { outcome: InvocationOutcome::Completed })).unwrap()).await.unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let body = request(&mut socket).await;
            respond(&mut socket).await;
            body
        });
        let effect = Arc::new(crate::support::agent_loop::Effect { log: log.clone(), count: Arc::new(std::sync::atomic::AtomicUsize::new(0)) });
        let mut input = crate::support::agent_loop::input(&base, "current", effect);
        input.configuration.cwd = if cfg!(windows) {
            r"C:\selected\image"
        } else {
            "/selected/image"
        }.into();
        input.supports_vision = true;
        input.configuration.system_prompt = Some(maka_agent_facts::execution::SystemPrompt { text: "系".into(), policy_revision: 1, sources: Vec::new() });
        input.context = Some(maka_agent_facts::context::ModelRequestContext {
            provider_id: "fixture".into(), context_window: Some(1300), model_context_window: Some(1300), declared_window: None,
        });
        let maka_agent_engine::RunWork::Message { message, .. } = &mut input.work else { unreachable!() };
        *message = "😀".into();
        let engine = engine(log.clone());
        engine.run(input, CancellationToken::new()).await.unwrap();
        engine.drain().await;
        let body = server.await.unwrap();
        let prefix = log.prefix(100, 128 * 1024).await.unwrap();
        let measured = prefix.events.iter().find_map(|event| match &event.event.fact {
            Fact::ModelRequested { context_breakdown, .. } if event.event.invocation.invocation_id == "invocation-current" => context_breakdown.clone(),
            _ => None,
        }).unwrap();
        drop(engine);
        Arc::try_unwrap(log).ok().unwrap().close().await.unwrap();
        let reopened = EventLog::open(&path).await.unwrap();
        let persisted = reopened.prefix(100, 128 * 1024).await.unwrap();
        reopened.close().await.unwrap();

        assert_eq!(body["messages"].as_array().unwrap().len(), 6);
        assert_eq!(body["messages"][0]["role"], "system");
        let system = body["messages"][0]["content"].as_str().unwrap();
        assert!(system.starts_with("系"));
        let system_bytes = system.len() as u64;
        let system_tokens = 16 + system_bytes.div_ceil(3);
        let total_tokens = 1190 + system_tokens;
        let output_tokens = 1300 - total_tokens;
        assert_eq!(body["messages"][3]["role"], "tool");
        assert_eq!(body["messages"][3]["content"], r#"[{"type":"text","text":"Image read successfully."}]"#);
        assert_eq!(body["messages"][4]["role"], "user");
        assert_eq!(body["messages"][4]["content"][0]["text"], "Images from tool Read (toolCallId: read):");
        assert!(body["messages"][4]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(body["tools"][0]["function"]["name"], "echo");
        assert_eq!(serde_json::to_value(&measured).unwrap(), json!({
            "system":{"contentBytes":system_bytes,"estimatedTokens":system_tokens},
            "user":{"contentBytes":47,"estimatedTokens":1089},
            "assistant":{"contentBytes":6,"estimatedTokens":35},
            "tool":{"contentBytes":28,"estimatedTokens":42},
            "toolSchemas":{"contentBytes":70,"estimatedTokens":24},
            "counts":{"messages":6,"media":1,"toolCalls":1,"toolResults":1,"tools":1}
        }));
        // Original estimator: tool message 16 + result 16 + ceil(Read's 4/3)
        // + ceil(the retained 24-byte success text / 3) = 42; no fallback text.
        // System text uses UTF-8 bytes: 16 message tokens + ceil(bytes / 3).
        // The other projected roles and schemas retain their original 1190 tokens.
        assert_eq!(measured.estimated_tokens(), total_tokens);
        assert_eq!(body["max_tokens"], output_tokens, "1300 capacity minus the complete projected input heuristic");
        assert!(persisted.events.iter().any(|event| matches!(&event.event.fact,
            Fact::ModelRequested { context_breakdown: Some(value), .. } if value == &measured)));
    }).await.unwrap();
}

#[tokio::test]
async fn missing_image_guidance_uses_the_admitted_language_and_preserves_attachment_identity() {
    use maka_agent_facts::language::Language;
    for (language, guidance) in [
        (Language::En, "could not be loaded"),
        (Language::ZhCn, "无法加载图片附件"),
        (Language::ZhTw, "無法載入圖片附件"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let log = Arc::new(
            EventLog::open(&directory.path().join("events.sqlite"))
                .await
                .unwrap(),
        );
        log.create_session("session", "create", &json!({}), 1)
            .await
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = request(&mut socket).await;
            respond(&mut socket).await;
            request
        });
        let mut image = attachment("missing_image");
        image["name"] = "missing-原图.png".into();
        let mut run = input(&base, json!([image]));
        run.configuration.language = language;
        let worker = engine(log.clone());
        worker.run(run, CancellationToken::new()).await.unwrap();
        worker.drain().await;
        let request = server.await.unwrap();
        let user = request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "user")
            .unwrap();
        let text = user["content"].to_string();
        assert!(text.contains(guidance));
        assert!(text.contains("missing-原图.png"));
        assert!(text.contains("not_found"));
        assert!(text.contains("current"));
        assert!(text.contains("image/png"));
        log.shutdown().await.unwrap();
    }
}
