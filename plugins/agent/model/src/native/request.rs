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

use super::{Decoder, failure, prompt};
use crate::{ModelError, ProviderKind};
use maka_agent_ports::model::{Credentials, Request};
use maka_provider_protocol::{anthropic, chat_completions};
use serde_json::Value;
use std::collections::BTreeMap;

pub(super) struct Prepared {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
    pub decoder: Decoder,
    pub compatible: bool,
    pub anthropic: bool,
}
pub(super) fn prepare(mut request: Request) -> Result<Prepared, ModelError> {
    let is_anthropic = matches!(request.provider.kind, ProviderKind::Anthropic);
    let compatible = matches!(request.provider.kind, ProviderKind::OpenaiCompatible { .. });
    request.prompt = crate::prompt::notification_fallback(request.prompt, request.language);
    super::media::audio_fallback(&mut request.prompt, request.language, is_anthropic);
    if !is_anthropic {
        request.prompt = super::chat_completions::project(request.prompt, request.language);
    }
    let messages = prompt::messages(request.prompt)?;
    let mut headers = BTreeMap::from([("content-type".to_owned(), "application/json".to_owned())]);
    let (mut body, decoder, suffix) = if is_anthropic {
        let tools = request
            .tools
            .into_iter()
            .map(|tool| {
                let definition = if let Some(provider) = tool.provider {
                    let hosted =
                        anthropic::HostedTool::from_definition(&provider.id, &provider.args)
                            .map_err(failure)?;
                    match hosted {
                        Some(
                            hosted @ (anthropic::HostedTool::CodeExecution(_)
                            | anthropic::HostedTool::WebFetch(..)
                            | anthropic::HostedTool::WebSearch(..)
                            | anthropic::HostedTool::ToolSearch(_)
                            | anthropic::HostedTool::Advisor(_)),
                        ) => anthropic::Tool::Hosted(hosted),
                        _ => {
                            return Err(ModelError::Adapter(format!(
                                "Unsupported provider-executed tool: {}",
                                provider.id
                            )));
                        }
                    }
                } else {
                    anthropic::Tool::Function(anthropic::FunctionTool {
                        name: tool.name.clone(),
                        description: Some(tool.description),
                        input_schema: tool.input_schema,
                        cache_control: None,
                        strict: None,
                        defer_loading: None,
                        eager_input_streaming: None,
                        allowed_callers: None,
                        input_examples: None,
                    })
                };
                Ok(anthropic::request::ToolBinding {
                    name: tool.name,
                    definition,
                })
            })
            .collect::<Result<Vec<_>, ModelError>>()?;
        let choice = (!tools.is_empty()).then_some(anthropic::ToolChoice::Auto);
        let options = anthropic::request::options::ProviderOptions::parse(
            request.provider_options.get("anthropic"),
            None,
        )
        .map_err(failure)?;
        let prepared = anthropic::request::prepare(
            anthropic::request::Request {
                model: request.provider.model,
                messages: &messages,
                tools,
                choice,
                sampling: anthropic::request::Sampling {
                    max_output_tokens: request.max_output_tokens,
                    ..Default::default()
                },
                reasoning: Default::default(),
                format: Default::default(),
                options,
                custom_namespace_used: false,
                streaming: true,
                header_betas: vec![
                    "interleaved-thinking-2025-05-14".into(),
                    "fine-grained-tool-streaming-2025-05-14".into(),
                ],
            },
            &Default::default(),
        )
        .map_err(failure)?;
        headers.insert("anthropic-version".into(), "2023-06-01".into());
        if !prepared.betas.is_empty() {
            headers.insert("anthropic-beta".into(), prepared.betas.join(","));
        }
        (
            serde_json::to_value(prepared.body).map_err(|e| ModelError::Adapter(e.to_string()))?,
            Decoder::Anthropic(Box::new(anthropic::stream::Stream::new(prepared.response))),
            "messages",
        )
    } else {
        let namespace = match request.provider.kind {
            ProviderKind::OpenaiCompatible { name } => name,
            ProviderKind::OpenaiChatCompletions => "openai".into(),
            _ => return Err(ModelError::Adapter("unsupported native protocol".into())),
        };
        let tools = request
            .tools
            .into_iter()
            .map(|tool| {
                if tool.provider.is_some() {
                    return Err(ModelError::Adapter(
                        "Chat Completions does not support provider-executed tools".into(),
                    ));
                }
                Ok(chat_completions::request::Tool::Function {
                    name: tool.name,
                    description: Some(tool.description),
                    schema: tool.input_schema,
                    strict: None,
                })
            })
            .collect::<Result<Vec<_>, ModelError>>()?;
        let tool_choice =
            (!tools.is_empty()).then_some(chat_completions::request::ToolChoice::Auto);
        let prepared = chat_completions::request::prepare(
            chat_completions::request::Request {
                model: request.provider.model,
                messages: &messages,
                tools,
                tool_choice,
                sampling: chat_completions::request::Sampling {
                    max_output_tokens: request.max_output_tokens,
                    ..Default::default()
                },
                format: Default::default(),
                reasoning: None,
                extensions: prompt::extensions(Some(request.provider_options))?,
                streaming: true,
            },
            &chat_completions::request::Config {
                provider_namespace: namespace,
                include_usage: true,
                ..Default::default()
            },
        )
        .map_err(failure)?;
        (
            prepared.body,
            Decoder::Chat(Box::new(
                chat_completions::Stream::new(prepared.metadata_namespace)
                    .with_reasoning_observations(compatible),
            )),
            "chat/completions",
        )
    };
    if let Some(overlay) = request.provider.body_overlay {
        for (key, value) in overlay {
            if body.get(&key).is_some() {
                return Err(ModelError::Adapter(format!(
                    "Extra request body conflicts with a generated field: {key}"
                )));
            }
            body[&key] = value;
        }
    }
    match request.provider.auth {
        Credentials::ApiKey(key) => {
            headers.insert(
                if is_anthropic {
                    "x-api-key"
                } else {
                    "authorization"
                }
                .into(),
                if is_anthropic {
                    key
                } else {
                    format!("Bearer {key}")
                },
            );
        }
        Credentials::RequestHeaders(auth) => merge_headers(
            &mut headers,
            auth,
            "Authentication header conflicts with a protocol header",
        )?,
    }
    merge_headers(
        &mut headers,
        request.provider.headers,
        "Custom request header conflicts with a generated header",
    )?;
    let mut url = reqwest::Url::parse(&request.provider.base_url)
        .map_err(|_| ModelError::Adapter("invalid provider endpoint".into()))?;
    url.set_path(&format!("{}/{suffix}", url.path().trim_end_matches('/')));
    Ok(Prepared {
        url: url.into(),
        headers: headers.into_iter().collect(),
        body,
        decoder,
        compatible,
        anthropic: is_anthropic,
    })
}
fn merge_headers(
    target: &mut BTreeMap<String, String>,
    headers: BTreeMap<String, String>,
    message: &str,
) -> Result<(), ModelError> {
    for (name, value) in headers {
        let normalized = name.to_ascii_lowercase();
        if target.get(&normalized).is_some_and(|old| old != &value) {
            return Err(ModelError::Adapter(format!("{message}: {name}")));
        }
        target.insert(normalized, value);
    }
    Ok(())
}
