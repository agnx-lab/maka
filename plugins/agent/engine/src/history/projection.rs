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

use maka_agent_facts::language::Language;

use super::{EventRef, images, operation_id, user};
use crate::RunError;
use maka_agent_facts::context::ModelPurpose;
use maka_agent_facts::context::resolve_model_purpose;
use maka_agent_facts::event::Fact;
use maka_agent_facts::event::ToolOutcome;
use maka_agent_facts::input::InvocationInput;
use maka_agent_facts::model::ModelPart;
use maka_agent_facts::model::TextKind;
use maka_agent_facts::model::assembly::StepBuilder;
use maka_agent_facts::tool_call::ToolOrigin;
use maka_model::prompt::{AssistantPart, Message, ToolOutput};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

pub(super) fn build<'a>(
    events: impl Iterator<Item = EventRef<'a>> + Clone,
    resources: &super::resources::Resources<'_>,
    images: &mut Vec<images::Target<'a>>,
    vision: bool,
    replay: Option<super::Replay<'a>>,
    prior_unknown: bool,
    language: Language,
) -> Result<Vec<Message>, RunError> {
    let cuts = replay.map(|policy| super::replay::Cuts::new(events.clone(), policy));
    let mut messages = Vec::new();
    // Leading foreign assistant history is retained for display, but cannot
    // become an assistant-prefill request. Only a real user opens model input.
    let mut has_user = false;
    let mut calls = HashMap::new();
    let mut custom_operations = HashSet::new();
    let mut dispatched = HashSet::new();
    let mut cells = HashMap::new();
    let mut notifications = Vec::new();
    let pending: HashSet<_> = if prior_unknown {
        let settled: HashSet<_> = events
            .clone()
            .filter_map(EventRef::canonical)
            .filter_map(|stored| {
                if let Fact::ToolSettled { operation_id, .. } = &stored.event.fact {
                    Some(operation_id.as_str())
                } else {
                    None
                }
            })
            .collect();
        events
            .clone()
            .filter_map(EventRef::canonical)
            .filter_map(|stored| {
                if let Fact::ToolDispatched { operation_id, .. } = &stored.event.fact {
                    (!settled.contains(operation_id.as_str())).then_some(operation_id.as_str())
                } else {
                    None
                }
            })
            .collect()
    } else {
        HashSet::new()
    };
    let mut invocations: HashMap<_, _> = events
        .clone()
        .filter_map(EventRef::canonical)
        .filter_map(|stored| match &stored.event.fact {
            Fact::InvocationOpened { input, .. } => Some((
                &stored.event.invocation.invocation_id,
                InvocationSpan {
                    input,
                    requests: HashMap::new(),
                },
            )),
            _ => None,
        })
        .collect();
    for event in events.clone() {
        let stored = match event {
            EventRef::Canonical(stored) => stored,
            EventRef::ModelItems(items) => {
                let request = invocations
                    .get_mut(&items.invocation.invocation_id)
                    .and_then(|invocation| invocation.requests.get_mut(&items.step_id))
                    .ok_or_else(|| {
                        RunError::ReconciliationRequired("accepted items lack request".into())
                    })?;
                if !request.item_acceptance {
                    return Err(RunError::ReconciliationRequired(
                        "item projection on a legacy request".into(),
                    ));
                }
                request.builder = None;
                request.end = items.sequence;
                request.accepted.extend(
                    items
                        .items
                        .iter()
                        .map(|item| (item.sequence, item.index, Cow::Borrowed(&item.part))),
                );
                continue;
            }
            EventRef::Archived(_) => continue,
        };
        if let Fact::ModelRequested {
            step_id,
            purpose,
            source_high_water,
            item_acceptance,
            ..
        } = &stored.event.fact
        {
            let invocation = invocations
                .get_mut(&stored.event.invocation.invocation_id)
                .ok_or_else(|| {
                    RunError::ReconciliationRequired("model request lacks canonical opening".into())
                })?;
            let resolved = resolve_model_purpose(invocation.input, *purpose)
                .map_err(|reason| RunError::ReconciliationRequired(reason.into()))?;
            invocation.requests.insert(
                step_id,
                RequestSpan {
                    purpose: resolved,
                    source: *source_high_water,
                    end: u64::MAX,
                    item_acceptance: *item_acceptance,
                    builder: if *item_acceptance {
                        Some(StepBuilder::for_step(step_id)?)
                    } else {
                        None
                    },
                    accepted: Vec::new(),
                },
            );
        }
        match &stored.event.fact {
            Fact::ModelObserved { step_id, event } => {
                if let Some(request) = invocations
                    .get_mut(&stored.event.invocation.invocation_id)
                    .and_then(|invocation| invocation.requests.get_mut(step_id))
                    && let Some(builder) = request.builder.as_mut()
                {
                    builder.push(event.clone())?;
                    if let Some((index, part)) = builder.accepted_item() {
                        request
                            .accepted
                            .push((stored.sequence, index, Cow::Owned(part.clone())));
                    }
                }
            }
            Fact::ModelCompleted { step_id, .. } | Fact::ModelInterrupted { step_id, .. } => {
                if let Some(request) = invocations
                    .get_mut(&stored.event.invocation.invocation_id)
                    .and_then(|invocation| invocation.requests.get_mut(step_id))
                {
                    request.end = stored.sequence;
                    if let Some(builder) = request.builder.take()
                        && let Fact::ModelCompleted { output, .. } = &stored.event.fact
                        && builder.finish()? != *output
                    {
                        return Err(RunError::ReconciliationRequired(
                            "model completion differs from accepted observations".into(),
                        ));
                    }
                    request.accepted.sort_by_key(|(_, index, _)| *index);
                }
            }
            Fact::InvocationEnded { .. } => {
                if let Some(invocation) =
                    invocations.get_mut(&stored.event.invocation.invocation_id)
                {
                    for request in invocation.requests.values_mut() {
                        if request.end == u64::MAX {
                            request.end = stored.sequence;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for event in events {
        let stored = match event {
            EventRef::Canonical(stored) => stored,
            EventRef::ModelItems(items) => {
                if calls.is_empty() {
                    flush_notifications(&mut notifications, items.sequence, &mut messages);
                }
                continue;
            }
            EventRef::Archived(archived) => {
                let (id, name) = calls.remove(&archived.operation_id).ok_or_else(|| {
                    RunError::ReconciliationRequired("archived result lacks provider call".into())
                })?;
                if !dispatched.remove(&archived.operation_id) {
                    return Err(RunError::ReconciliationRequired(
                        "archived result lacks dispatch".into(),
                    ));
                }
                let maka_agent_facts::tool_output::DurableToolProjection::Json { value } =
                    &archived.replacement
                else {
                    return Err(RunError::ReconciliationRequired(
                        "archive replacement is not a JSON page".into(),
                    ));
                };
                // Store verification retains the canonical codec; only current
                // presentation describes the capability needed for continuation.
                let mut value = value.clone();
                value["readInstructions"] = match language { Language::En => "Use page.next only with a selected Session tool-result reader; without one, the remainder is unavailable.", Language::ZhCn => "仅使用已选定的会话工具结果读取工具读取 page.next；没有读取工具时，其余内容不可用。", Language::ZhTw => "僅使用已選定的工作階段工具結果讀取工具讀取 page.next；沒有讀取工具時，其餘內容無法取得。" }.into();
                let output = if archived.is_error {
                    ToolOutput::ErrorJson(value)
                } else {
                    ToolOutput::Json(value)
                };
                messages.push(tool_message(
                    id,
                    name,
                    output,
                    custom_operations.contains(&archived.operation_id),
                ));
                if calls.is_empty() {
                    flush_notifications(&mut notifications, archived.sequence, &mut messages);
                }
                continue;
            }
        };
        let invocation = invocations.get(&stored.event.invocation.invocation_id);
        if invocation.is_some_and(|invocation| {
            matches!(invocation.input, InvocationInput::ContextCompact { .. })
        }) {
            continue;
        }
        match &stored.event.fact {
            Fact::ToolNotified {
                operation_id,
                model_text,
                ..
            } => {
                let (parent, id) = cells.get(operation_id.as_str()).ok_or_else(|| {
                    RunError::ReconciliationRequired("notification lacks its Code Mode cell".into())
                })?;
                let mut message = Message::notification(*id, model_text.clone());
                if custom_operations.contains(*parent) {
                    mark_custom(&mut message);
                }
                let release = invocation
                    .into_iter()
                    .flat_map(|invocation| invocation.requests.values())
                    .filter_map(|request| {
                        (request.purpose == ModelPurpose::Main
                            && request.source < stored.sequence
                            && stored.sequence <= request.end)
                            .then_some(request.end)
                    })
                    .max()
                    .unwrap_or(stored.sequence);
                notifications.push((release, message));
            }
            Fact::MessageImported { record, .. } => {
                use maka_agent_facts::import::Content;
                match &record.content {
                    Content::User { text } if record.is_conversation() => {
                        has_user = true;
                        messages.push(Message::user(text.clone()));
                    }
                    Content::Assistant { text, .. } if has_user && record.is_conversation() => {
                        messages.push(Message::Assistant {
                            content: vec![AssistantPart::Text {
                                text: text.clone(),
                                provider_options: None,
                            }],
                            provider_options: None,
                        });
                    }
                    // Foreign tool observations are not executable provider
                    // call/result pairs. Thinking and notes stay in the ledger/UI.
                    _ => {}
                }
            }
            Fact::InvocationOpened { input, .. } => {
                if input.inherited_claim().is_some() {
                    continue;
                }
                let InvocationInput::Message { content, .. } = input else {
                    return Err(RunError::ReconciliationRequired(
                        "invocation has no model-history input".into(),
                    ));
                };
                has_user = true;
                messages.push(user::project(
                    content,
                    resources,
                    false,
                    messages.len(),
                    images,
                    vision,
                    language,
                )?);
            }
            Fact::MessageSteered { message, .. } => {
                has_user = true;
                message
                    .validate()
                    .map_err(|reason| RunError::ReconciliationRequired(reason.into()))?;
                messages.push(user::project(
                    &message.content,
                    resources,
                    true,
                    messages.len(),
                    images,
                    vision,
                    language,
                )?);
            }
            Fact::ExecutorCompleted { text } => {
                messages.push(Message::Assistant {
                    content: vec![AssistantPart::Text {
                        text: text.clone(),
                        provider_options: None,
                    }],
                    provider_options: None,
                });
            }
            Fact::ModelCompleted { step_id, .. }
            | Fact::ModelRequested {
                step_id,
                item_acceptance: true,
                ..
            } => {
                let request = invocation.and_then(|invocation| invocation.requests.get(step_id));
                if matches!(&stored.event.fact, Fact::ModelCompleted { .. })
                    && request.is_some_and(|request| request.item_acceptance)
                {
                    if calls.is_empty() {
                        flush_notifications(&mut notifications, stored.sequence, &mut messages);
                    }
                    continue;
                }
                match request.map(|request| request.purpose) {
                    Some(ModelPurpose::Summary) => continue,
                    Some(ModelPurpose::Main) => {}
                    None => {
                        return Err(RunError::ReconciliationRequired(
                            "model completion lacks request purpose".into(),
                        ));
                    }
                }
                let parts: Vec<_> = match &stored.event.fact {
                    Fact::ModelCompleted { output, .. } => output
                        .parts
                        .iter()
                        .enumerate()
                        .map(|(index, part)| (stored.sequence, index, part))
                        .collect(),
                    _ => request
                        .unwrap()
                        .accepted
                        .iter()
                        .map(|(event, index, part)| (*event, *index, part.as_ref()))
                        .collect(),
                };
                let mut content = Vec::new();
                for (accepted_at, index, part) in parts {
                    let value =
                        match part {
                            // Citations are preserved in the canonical result/UI, not
                            // fabricated as additional model-authored prompt text.
                            ModelPart::Source { .. } => continue,
                            ModelPart::Text {
                                text_kind,
                                text,
                                provider_options,
                            } => match text_kind {
                                TextKind::Thinking => AssistantPart::Reasoning {
                                    text: text.clone(),
                                    provider_options: provider_options.clone(),
                                },
                                TextKind::Text => AssistantPart::Text {
                                    text: text.clone(),
                                    provider_options: provider_options.clone(),
                                },
                            },
                            ModelPart::ToolCall { call } => {
                                if !call.provider_executed {
                                    if call.provider_options.as_ref().is_some_and(|value| {
                                        value["openai"]["toolKind"] == "custom"
                                    }) {
                                        custom_operations.insert(operation_id(step_id, &call.id));
                                    }
                                    calls.insert(
                                        operation_id(step_id, &call.id),
                                        (&call.id, &call.name),
                                    );
                                }
                                AssistantPart::ToolCall {
                                    tool_call_id: call.id.clone(),
                                    tool_name: call.name.clone(),
                                    input: call.input.clone(),
                                    provider_executed: Some(call.provider_executed),
                                    provider_options: call.provider_options.clone(),
                                }
                            }
                            ModelPart::ToolResult {
                                id,
                                name,
                                output,
                                is_error,
                                provider_options,
                            } => AssistantPart::ToolResult {
                                tool_call_id: id.clone(),
                                tool_name: name.clone(),
                                output: if *is_error {
                                    ToolOutput::ErrorJson(output.clone())
                                } else {
                                    ToolOutput::Json(output.clone())
                                },
                                provider_options: provider_options.clone(),
                            },
                        };
                    if cuts
                        .as_ref()
                        .map(|cuts| {
                            cuts.allows_at(
                                accepted_at,
                                &stored.event.invocation,
                                step_id,
                                index,
                                part,
                            )
                        })
                        .transpose()?
                        .unwrap_or(true)
                    {
                        content.push(value);
                    }
                }
                if !content.is_empty() {
                    messages.push(Message::Assistant {
                        content,
                        provider_options: None,
                    });
                }
            }
            Fact::ToolDispatched {
                operation_id: operation,
                call,
                name,
                ..
            } => match &call.origin {
                ToolOrigin::Provider { step_id } => {
                    if operation_id(step_id, &call.tool_call_id) != *operation
                        || calls.get(operation) != Some(&(&call.tool_call_id, name))
                        || !dispatched.insert(operation)
                    {
                        return Err(RunError::ReconciliationRequired(
                            "provider tool dispatch identity mismatch".into(),
                        ));
                    }
                    if pending.contains(operation.as_str()) {
                        let (id, name) = calls.remove(operation).ok_or_else(|| {
                            RunError::ReconciliationRequired(
                                "unknown tool call lacks accepted model output".into(),
                            )
                        })?;
                        dispatched.remove(operation);
                        messages.push(tool_message(id, name, ToolOutput::ErrorText(
                            match language { Language::En => "outcome_unknown: The tool was dispatched, but no durable result was recorded. Its effect may have happened. Inspect current state before repeating it.", Language::ZhCn => "outcome_unknown: 工具已派发，但未记录持久结果；操作可能已生效。重复操作前先检查当前状态。", Language::ZhTw => "outcome_unknown: 工具已派發，但未記錄持久結果；操作可能已生效。重複操作前先檢查目前狀態。" }.into(),
                        ), custom_operations.contains(operation)));
                    }
                }
                ToolOrigin::CodeCell {
                    parent_operation_id,
                    parent_tool_call_id,
                    ..
                } => {
                    if calls.contains_key(operation) {
                        return Err(RunError::ReconciliationRequired(
                            "hidden cell aliases provider call".into(),
                        ));
                    }
                    cells.insert(
                        operation.as_str(),
                        (parent_operation_id.as_str(), parent_tool_call_id.as_str()),
                    );
                }
                ToolOrigin::CodeMode { .. }
                | ToolOrigin::HostSdk { .. }
                | ToolOrigin::Standalone => {
                    if calls.contains_key(operation) {
                        return Err(RunError::ReconciliationRequired(
                            "hidden tool operation aliases provider call".into(),
                        ));
                    }
                }
            },
            Fact::ToolRejected {
                operation_id: operation,
                call,
                name,
                input,
                reason,
            } => match &call.origin {
                ToolOrigin::Provider { step_id } => {
                    if operation_id(step_id, &call.tool_call_id) != *operation
                        || calls.get(operation) != Some(&(&call.tool_call_id, name))
                        || dispatched.contains(operation)
                    {
                        return Err(RunError::ReconciliationRequired(
                            "provider tool rejection identity mismatch".into(),
                        ));
                    }
                    let (id, name) = calls.remove(operation).expect("validated accepted call");
                    messages.push(tool_message(
                        id,
                        name,
                        ToolOutput::ErrorText(match input.invalid_json_diagnostic() {
                            Some(diagnostic) => format!("{reason}\n{diagnostic}"),
                            None => reason.to_string(),
                        }),
                        custom_operations.contains(operation),
                    ));
                }
                ToolOrigin::CodeMode { .. }
                | ToolOrigin::CodeCell { .. }
                | ToolOrigin::HostSdk { .. }
                | ToolOrigin::Standalone => {
                    if calls.contains_key(operation) {
                        return Err(RunError::ReconciliationRequired(
                            "hidden tool rejection aliases provider call".into(),
                        ));
                    }
                }
            },
            Fact::ToolSettled {
                operation_id,
                outcome,
            } => {
                if let Some((id, name)) = calls.remove(operation_id) {
                    if !dispatched.remove(operation_id) {
                        return Err(RunError::ReconciliationRequired(
                            "provider tool result lacks matching dispatch".into(),
                        ));
                    }
                    let output = match outcome {
                        ToolOutcome::Succeeded {
                            model_projection, ..
                        } => super::output::project(
                            model_projection,
                            messages.len(),
                            images,
                            vision,
                            language,
                        ),
                        ToolOutcome::Failed { message } | ToolOutcome::Unknown { message, .. } => {
                            ToolOutput::ErrorText(message.clone())
                        }
                    };
                    messages.push(tool_message(
                        id,
                        name,
                        output,
                        custom_operations.contains(operation_id),
                    ));
                }
            }
            _ => {}
        }
        // The request's source cut, not its commit timestamp, proves whether
        // it saw a notification. This also preserves frozen physical retries.
        if calls.is_empty() {
            flush_notifications(&mut notifications, stored.sequence, &mut messages);
        }
    }
    if !calls.is_empty() {
        return Err(RunError::ReconciliationRequired(
            "model tool calls have no committed outcome".into(),
        ));
    }
    Ok(messages)
}

fn tool_message(id: &str, name: &str, output: ToolOutput, custom: bool) -> Message {
    let mut message = Message::tool(id, name, output);
    if custom {
        mark_custom(&mut message);
    }
    message
}

fn mark_custom(message: &mut Message) {
    if let Message::Tool { content, .. } = message {
        for part in content {
            let options = part
                .provider_options
                .get_or_insert_with(|| serde_json::json!({}));
            options["openai"] = serde_json::json!({"toolKind":"custom"});
        }
    }
}

struct InvocationSpan<'a> {
    input: &'a InvocationInput,
    requests: HashMap<&'a String, RequestSpan<'a>>,
}

struct RequestSpan<'a> {
    purpose: ModelPurpose,
    source: u64,
    end: u64,
    item_acceptance: bool,
    builder: Option<StepBuilder>,
    accepted: Vec<(u64, usize, Cow<'a, ModelPart>)>,
}

fn flush_notifications(
    pending: &mut Vec<(u64, Message)>,
    through: u64,
    messages: &mut Vec<Message>,
) {
    messages.extend(
        pending
            .extract_if(.., |(release, _)| *release <= through)
            .map(|(_, message)| message),
    );
}
