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

//! Fit only a provider's summary input. Canonical history is never shortened.
use crate::RunInput;
use maka_agent_facts::context::{
    ContextBucket, ContextCounts, ModelPurpose, PreparedContextBreakdown,
};
use maka_agent_facts::language::Language;
use maka_agent_facts::model::ToolInput;
use maka_model::prompt::{AssistantPart, ContentPart, Message, ToolOutput};

/// A preflight estimate, never measured provider usage. Attachment encodings
/// are not text tokens; provider overflow is handled by fitting a smaller request.
pub(super) fn estimate(prompt: &[Message]) -> u64 {
    prompt_breakdown(prompt).estimated_tokens()
}

pub(super) fn breakdown(
    prompt: &[Message],
    tools: &[maka_model::ToolDefinition],
) -> PreparedContextBreakdown {
    let mut measured = prompt_breakdown(prompt);
    measured.counts.tools = tools.len() as u64;
    add_json(&mut measured.tool_schemas, tools);
    measured
}

fn prompt_breakdown(prompt: &[Message]) -> PreparedContextBreakdown {
    let mut measured = PreparedContextBreakdown::default();
    for message in prompt {
        measured.counts.messages += 1;
        let (bucket, counts) = match message {
            Message::System { .. } => (&mut measured.system, &mut measured.counts),
            Message::User { .. } => (&mut measured.user, &mut measured.counts),
            Message::Assistant { .. } => (&mut measured.assistant, &mut measured.counts),
            Message::Tool { .. } => (&mut measured.tool, &mut measured.counts),
        };
        bucket.estimated_tokens += 16;
        match message {
            Message::System { content, .. } => add_text(bucket, content),
            Message::User { content, .. } => {
                for part in content {
                    add_content(bucket, counts, part);
                }
            }
            Message::Assistant { content, .. } => {
                for part in content {
                    match part {
                        AssistantPart::Text { text, .. }
                        | AssistantPart::Reasoning { text, .. } => add_text(bucket, text),
                        AssistantPart::ToolCall {
                            tool_name, input, ..
                        } => {
                            counts.tool_calls += 1;
                            bucket.estimated_tokens += 16;
                            add_text(bucket, tool_name);
                            match input {
                                ToolInput::Json(value) => add_json(bucket, value),
                                ToolInput::InvalidJson { raw } => add_text(bucket, raw),
                            }
                        }
                        AssistantPart::ToolResult {
                            tool_name, output, ..
                        } => add_result(bucket, counts, tool_name, output),
                    }
                }
            }
            Message::Tool { content, .. } => {
                for part in content {
                    add_result(bucket, counts, &part.tool_name, &part.output);
                }
            }
        }
    }
    measured
}

fn add_text(bucket: &mut ContextBucket, text: &str) {
    add_bytes(bucket, text.len() as u64);
}
fn add_bytes(bucket: &mut ContextBucket, bytes: u64) {
    bucket.content_bytes += bytes;
    // Preserve each original estimator term's rounding, not ceil(sum(bytes)/3).
    bucket.estimated_tokens += bytes.div_ceil(3);
}
fn add_json(bucket: &mut ContextBucket, value: &(impl serde::Serialize + ?Sized)) {
    add_bytes(bucket, json_bytes(value));
}
fn add_content(bucket: &mut ContextBucket, counts: &mut ContextCounts, part: &ContentPart) {
    match part {
        ContentPart::Text { text, .. } => add_text(bucket, text),
        ContentPart::File { .. } => {
            counts.media += 1;
            bucket.estimated_tokens += 1024;
        }
    }
}
fn add_result(
    bucket: &mut ContextBucket,
    counts: &mut ContextCounts,
    name: &str,
    output: &ToolOutput,
) {
    counts.tool_results += 1;
    bucket.estimated_tokens += 16;
    add_text(bucket, name);
    match output {
        ToolOutput::Text(text) | ToolOutput::ErrorText(text) => add_text(bucket, text),
        ToolOutput::Json(value) | ToolOutput::ErrorJson(value) => add_json(bucket, value),
        ToolOutput::Content(parts) => {
            for part in parts {
                add_content(bucket, counts, part);
            }
        }
    }
}
pub(super) fn json_tokens(value: &(impl serde::Serialize + ?Sized)) -> u64 {
    json_bytes(value).div_ceil(3)
}
fn json_bytes(value: &(impl serde::Serialize + ?Sized)) -> u64 {
    struct Count(u64);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len() as u64);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value).expect("model prompt is serializable");
    count.0
}

/// Keep 4% of total context capacity (200k if unknown) available for summary
/// output, even when the full prefix must first be sent to the provider.
fn summary_output_reserve<W>(input: &RunInput<W>) -> u64 {
    input
        .context
        .as_ref()
        .and_then(|context| context.model_context_window)
        .unwrap_or(200_000)
        .saturating_mul(4)
        .div_ceil(100)
        .max(1)
}

/// The desired summary output may exceed the reserve; the final request caps
/// it to available capacity. This is not a minimum summary length.
pub(super) fn summary_output_budget<W>(input: &RunInput<W>) -> u64 {
    summary_output_reserve(input).max(input.main_output_limit.unwrap_or(8000))
}

pub(super) fn summary_budget<W>(input: &RunInput<W>) -> u64 {
    let context = input.context.as_ref();
    let total = context
        .and_then(|context| context.model_context_window)
        .unwrap_or(200_000);
    let window = context
        .and_then(|context| context.context_window)
        .unwrap_or(total);
    window.min(total.saturating_sub(summary_output_reserve(input)))
}

pub(super) fn needs_compaction<W>(input: &RunInput<W>, prompt: &[Message]) -> bool {
    estimate(prompt) > summary_budget(input)
}

/// Keep complete recent turns when possible. A single enormous turn becomes a
/// labelled head/tail excerpt, not an invalid partial tool-call/result sequence.
/// This is a summary-only rendering; the exact request digest records this input.
pub(super) fn fit_summary(prompt: &[Message], tokens: u64, language: Language) -> Vec<Message> {
    if estimate(prompt) <= tokens {
        return prompt.to_vec();
    }
    // Preserve instruction roles and keep the conversational anchor independent
    // of the System prefix. If the System alone cannot fit, provider overflow
    // will fail safely rather than silently truncating its instructions.
    let system_end = prompt
        .iter()
        .take_while(|message| matches!(message, Message::System { .. }))
        .count();
    let (system, prompt) = prompt.split_at(system_end);
    let tokens = tokens.saturating_sub(estimate(system));
    let bytes = usize::try_from(tokens.saturating_mul(3)).unwrap_or(usize::MAX);
    let notice = match language {
        Language::En => {
            "Conversation excerpts for summarization: some older text is omitted to fit this model. The complete original history remains stored locally. Preserve known goals and recent progress; do not infer facts from omitted text.\n\n"
        }
        Language::ZhCn => {
            "用于总结的对话节选：为适应模型，已省略部分较早文本，完整历史仍保存在本地。保留已知目标和近期进展，不得从省略内容推测事实。\n\n"
        }
        Language::ZhTw => {
            "用於總結的對話節錄：為符合模型容量，已省略部分較早文字，完整歷史仍保存在本機。保留已知目標和近期進展，不得從省略內容推測事實。\n\n"
        }
    };
    let available = bytes.saturating_sub(notice.len() + 100);
    // Preserve the original task or previous checkpoint as well as the recent tail.
    let head = prompt
        .first()
        .map(|message| serde_json::to_string(message).expect("model prompt"))
        .unwrap_or_default();
    let head = excerpt(&head, available / 4, language);
    let remaining = available.saturating_sub(head.len() + 32);
    let mut starts = vec![0];
    for (index, message) in prompt.iter().enumerate().skip(1) {
        if matches!(message, Message::User { .. }) {
            starts.push(index);
        }
    }
    starts.push(prompt.len());
    let mut first = prompt.len();
    let mut used = 0;
    for unit in starts.windows(2).rev() {
        let size = estimate(&prompt[unit[0]..unit[1]]).saturating_mul(3);
        if size > remaining.saturating_sub(used) as u64 {
            break;
        }
        used += size as usize;
        first = unit[0];
    }
    if first == prompt.len() {
        first = *starts.iter().rev().nth(1).unwrap_or(&0);
    }
    let tail = serde_json::to_string(&prompt[first..]).expect("model prompt");
    let earlier = match language {
        Language::En => "Earlier task or summary:",
        Language::ZhCn => "早期任务或摘要：",
        Language::ZhTw => "早期任務或摘要：",
    };
    let recent = match language {
        Language::En => "Recent conversation:",
        Language::ZhCn => "近期对话：",
        Language::ZhTw => "近期對話：",
    };
    let text = format!(
        "{notice}{earlier}\n{head}\n\n{recent}\n{}",
        excerpt(&tail, remaining, language)
    );
    let mut fitted = system.to_vec();
    fitted.push(Message::user(text));
    fitted
}

fn excerpt(text: &str, bytes: usize, language: Language) -> String {
    if text.len() <= bytes {
        return text.into();
    }
    let gap = match language {
        Language::En => "\n[… omitted from summary input …]\n",
        Language::ZhCn => "\n[… 已从总结输入中省略 …]\n",
        Language::ZhTw => "\n[… 已從總結輸入中省略 …]\n",
    };
    if bytes < gap.len() {
        return String::new();
    }
    let remaining = bytes - gap.len();
    let start = text.floor_char_boundary(remaining / 2);
    let end = text.ceil_char_boundary(text.len() - (remaining - remaining / 2));
    format!("{}{gap}{}", &text[..start], &text[end..])
}

/// The final request owns its output reserve, after all prompt/tool projections.
/// This cap never changes the configured model limit or the persisted history.
pub(super) fn output_limit<W>(
    input: &RunInput<W>,
    estimated_input_tokens: u64,
    requested: u64,
    purpose: ModelPurpose,
) -> u64 {
    let minimum = match purpose {
        ModelPurpose::Main => 1,
        ModelPurpose::Summary => summary_output_reserve(input),
    };
    input
        .context
        .as_ref()
        .and_then(|context| context.model_context_window)
        .map_or(requested, |window| {
            requested.min(window.saturating_sub(estimated_input_tokens).max(minimum))
        })
}
