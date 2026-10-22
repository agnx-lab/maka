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

//! AI SDK wire representation stays behind this adapter.

use crate::{ModelError, ToolDefinition};
use maka_agent_facts::model::request::Request as ModelRequest;
use maka_agent_ports::model::Context;
use maka_agent_ports::model::Lifetime;
use maka_agent_ports::model::ProviderAdapter;
use maka_agent_ports::model::Request;
use maka_agent_ports::model::Session;
use maka_js_runtime::trusted::TrustedRuntime;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::mpsc;

mod chat;
mod prompt;

fn serialize_tools<S: serde::Serializer>(
    tools: &[ToolDefinition],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    #[serde(tag = "type", rename_all = "kebab-case")]
    enum SdkTool<'a> {
        Function(&'a ToolDefinition),
        Provider {
            id: &'a str,
            name: &'a str,
            args: &'a Value,
        },
    }
    serializer.collect_seq(tools.iter().map(|tool| match &tool.provider {
        Some(provider) => SdkTool::Provider {
            id: &provider.id,
            name: &tool.name,
            args: &provider.args,
        },
        None => SdkTool::Function(tool),
    }))
}

pub(super) async fn stream(
    runtime: TrustedRuntime,
    mut request: ModelRequest,
    context: Context,
) -> Result<(), ModelError> {
    let Context {
        events: sender,
        cancellation,
        idle_timeout,
        transport: network,
    } = context;
    request.prompt = crate::prompt::notification_fallback(request.prompt, request.language);
    audio_fallback(
        &mut request.prompt,
        request.language,
        matches!(request.provider.kind, crate::ProviderKind::Anthropic),
    );
    if matches!(
        request.provider.kind,
        crate::ProviderKind::OpenaiChat | crate::ProviderKind::OpenaiCompatible { .. }
    ) {
        request.prompt = chat::project(request.prompt, request.language);
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct SdkRequest {
        provider: maka_agent_facts::model::request::Provider,
        #[serde(serialize_with = "prompt::serialize")]
        prompt: Vec<crate::prompt::Message>,
        #[serde(serialize_with = "serialize_tools")]
        tools: Vec<ToolDefinition>,
        provider_options: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_output_tokens: Option<u64>,
    }
    let request = SdkRequest {
        provider: request.provider,
        prompt: request.prompt,
        tools: request.tools,
        provider_options: request.provider_options,
        max_output_tokens: request.max_output_tokens,
    };
    let (events, mut receiver) = mpsc::channel(1);
    let execution = runtime.model(request, events, cancellation.clone(), network, idle_timeout);
    tokio::pin!(execution);
    let mut normalizer = crate::events::Normalizer::default();
    let forward = async {
        while let Some(event) = receiver.recv().await {
            sender.progress();
            let (value, reservation) = event.map_err(ModelError::from)?.into_parts();
            let event = normalizer.push(value)?;
            if let Some(event) = event {
                let event = maka_agent_ports::model::Emission {
                    event,
                    reservation: Some(std::sync::Arc::new(reservation)),
                };
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(ModelError::Cancelled),
                    sent = sender.emit(event) => sent?,
                }
            }
        }
        Ok(normalizer)
    };
    tokio::pin!(forward);
    // Both futures are owned here: returning an adapter error also cancels and
    // settles the SDK worker before releasing the Host's execution permit.
    tokio::select! {
        result = &mut execution => {
            result.map_err(ModelError::from)?;
            forward.await?.end()
        }
        result = &mut forward => {
            match result {
                Err(error) => {
                    cancellation.cancel();
                    let _ = execution.await;
                    Err(error)
                }
                Ok(normalizer) => {
                    execution.await.map_err(ModelError::from)?;
                    normalizer.end()
                },
            }
        }
    }
}

// The locked Messages SDK has no audio content type. Chat SDKs support WAV/MP3.
// Preserve an explicit observation instead of dropping media or fabricating bytes.
fn audio_fallback(
    messages: &mut [crate::prompt::Message],
    language: maka_agent_facts::language::Language,
    anthropic: bool,
) {
    use crate::prompt::{ContentPart, FileData, Message, ToolOutput};
    use maka_agent_facts::language::Language;
    fn project(parts: &mut [ContentPart], language: Language, anthropic: bool) {
        for part in parts {
            if let ContentPart::File {
                media_type,
                data: FileData::Data(data),
                ..
            } = part
                && media_type.starts_with("audio/")
                && (anthropic
                    || !matches!(
                        media_type.as_str(),
                        "audio/wav" | "audio/mpeg" | "audio/mp3"
                    ))
            {
                let length = data.len();
                *part = ContentPart::text(match language {
                    Language::En => format!(
                        "Audio input ({media_type}, {length} base64 characters) is not supported by this model adapter. The audio remains in the tool evidence."
                    ),
                    Language::ZhCn => format!(
                        "此模型适配器不支持音频输入（{media_type}，{length} 个 base64 字符）。音频仍保留在工具证据中。"
                    ),
                    Language::ZhTw => format!(
                        "此模型介接器不支援音訊輸入（{media_type}，{length} 個 base64 字元）。音訊仍保留在工具證據中。"
                    ),
                });
            }
        }
    }
    for message in messages {
        match message {
            Message::User { content, .. } => project(content, language, anthropic),
            Message::Tool { content, .. } => {
                for result in content {
                    if let ToolOutput::Content(parts) = &mut result.output {
                        project(parts, language, anthropic);
                    }
                }
            }
            _ => {}
        }
    }
}

pub struct Adapter(pub TrustedRuntime);
impl ProviderAdapter for Adapter {
    fn enforces_idle_timeout(&self) -> bool {
        // The native HTTP bridge times actual network waits. Input reservations
        // and output delivery may wait for local capacity without provider silence.
        true
    }

    fn open(
        &self,
        _: Lifetime,
        _: tokio_util::sync::CancellationToken,
    ) -> futures_util::future::BoxFuture<'_, Result<Arc<dyn Session>, ModelError>> {
        let session = Sdk(self.0.clone());
        Box::pin(async { Ok(Arc::new(session) as Arc<dyn Session>) })
    }
}
struct Sdk(TrustedRuntime);
impl Session for Sdk {
    fn stream(
        &self,
        request: Request,
        context: Context,
    ) -> futures_util::future::BoxFuture<'static, Result<(), ModelError>> {
        Box::pin(stream(self.0.clone(), request, context))
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;
    use crate::prompt::{ContentPart, FileData, Message};
    use maka_agent_facts::language::Language;

    #[test]
    fn unsupported_audio_reports_original_metadata_in_captured_language() {
        for (language, expected) in [
            (
                Language::ZhCn,
                "不支持音频输入（audio/ogg，8 个 base64 字符）",
            ),
            (
                Language::ZhTw,
                "不支援音訊輸入（audio/ogg，8 個 base64 字元）",
            ),
        ] {
            let mut messages = vec![Message::User {
                content: vec![
                    ContentPart::text("Original user 原文"),
                    ContentPart::File {
                        data: FileData::Data("YXVkaW8=".into()),
                        media_type: "audio/ogg".into(),
                        provider_options: None,
                    },
                ],
                provider_options: None,
            }];
            audio_fallback(&mut messages, language, false);
            let Message::User { content, .. } = &messages[0] else {
                panic!("user message")
            };
            assert_eq!(content[0], ContentPart::text("Original user 原文"));
            let ContentPart::Text { text, .. } = &content[1] else {
                panic!("unsupported audio observation")
            };
            assert!(text.contains(expected));
        }
    }
}
