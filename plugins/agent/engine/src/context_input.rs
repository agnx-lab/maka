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
            "Conversation excerpts for summarization: some text is omitted to fit this model. The complete original history remains stored locally.\n\n"
        }
        Language::ZhCn => {
            "用于总结的对话节选：为适应模型，已省略部分文本，完整原始历史仍保存在本地。\n\n"
        }
        Language::ZhTw => {
            "用於總結的對話節錄：為符合模型容量，已省略部分文字，完整原始歷史仍儲存在本機。\n\n"
        }
    };
    let available = bytes.saturating_sub(notice.len() + 100);
    // Preserve the original task or previous checkpoint as well as the recent tail.
    let head_budget = available / 4;
    let mut head = media_excerpt(
        &prompt[..prompt.len().min(1)],
        system_end,
        head_budget,
        language,
        true,
    );
    let head_size: usize = head
        .iter()
        .map(|part| match part {
            ContentPart::Text { text, .. } => text.len(),
            ContentPart::File { .. } => 1024 * 3,
        })
        .sum();
    let mut remaining = available.saturating_sub(head_size + 32);
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
    if first == 0 {
        // The tail owns all media accounting when it covers the anchor too.
        // A text-only anchor avoids duplicate payloads or false omission notices.
        head = media_excerpt(
            &prompt[..prompt.len().min(1)],
            system_end,
            head_budget,
            language,
            false,
        );
        let head_size: usize = head
            .iter()
            .map(|part| match part {
                ContentPart::Text { text, .. } => text.len(),
                ContentPart::File { .. } => unreachable!("text-only anchor"),
            })
            .sum();
        remaining = available.saturating_sub(head_size + 32);
    }
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
    let mut content = vec![ContentPart::text(format!("{notice}{earlier}\n"))];
    content.extend(head);
    content.push(ContentPart::text(format!("\n\n{recent}\n")));
    content.extend(media_excerpt(
        &prompt[first..],
        system_end + first,
        remaining,
        language,
        true,
    ));
    // Keep the original text-only wire shape when no native media was selected.
    let mut merged = Vec::new();
    for part in content {
        match (merged.last_mut(), part) {
            (Some(ContentPart::Text { text, .. }), ContentPart::Text { text: next, .. }) => {
                text.push_str(&next);
            }
            (_, part) => merged.push(part),
        }
    }
    let mut fitted = system.to_vec();
    fitted.push(Message::User {
        content: merged,
        provider_options: None,
    });
    fitted
}

/// Render historical protocol facts as text, but keep file encodings out of that
/// text even when only a head/tail excerpt survives. Original media parts retain
/// their payload, media type and provider metadata when selected.
fn media_excerpt(
    messages: &[Message],
    first: usize,
    bytes: usize,
    language: Language,
    include_media: bool,
) -> Vec<ContentPart> {
    let mut files = Vec::new();
    let mut rendered = String::new();
    for (offset, message) in messages.iter().enumerate() {
        let mut message = message.clone();
        let source = format!("request-source message position {}", first + offset);
        match &mut message {
            Message::System { .. } => {}
            Message::User { content, .. } => extract_media(content, &source, &mut files, language),
            Message::Assistant { content, .. } => {
                for (index, part) in content.iter_mut().enumerate() {
                    if let AssistantPart::ToolResult {
                        output: ToolOutput::Content(parts),
                        ..
                    } = part
                    {
                        extract_media(
                            parts,
                            &format!("{source}, assistant part {index}"),
                            &mut files,
                            language,
                        );
                    }
                }
            }
            Message::Tool { content, .. } => {
                for result in content {
                    if let ToolOutput::Content(parts) = &mut result.output {
                        extract_media(
                            parts,
                            &format!(
                                "{source}, tool {} ({})",
                                result.tool_name, result.tool_call_id
                            ),
                            &mut files,
                            language,
                        );
                    }
                }
            }
        }
        // Every File has been replaced before serialization; raw tool inputs,
        // result kinds and historical role/provenance remain available as text.
        rendered.push_str(&serde_json::to_string(&message).expect("model prompt"));
        rendered.push('\n');
    }
    if !include_media {
        return vec![ContentPart::text(excerpt(&rendered, bytes, language))];
    }
    // Share the excerpt budget between textual evidence and native media. Media
    // uses the same 1024-token estimate as add_content, never encoded byte length.
    let media_budget = bytes.saturating_sub(rendered.len().min(bytes / 4));
    let mut remaining_media = media_budget;
    let mut omitted_labels = Vec::new();
    let mut selected = Vec::new();
    for (label, file) in files.iter().rev() {
        let size = 1024 * 3 + label.len();
        if size <= remaining_media {
            remaining_media -= size;
            selected.push((label.clone(), file.clone()));
        } else {
            omitted_labels.push(label.as_str());
        }
    }
    selected.reverse();
    omitted_labels.reverse();
    let media_bytes = media_budget - remaining_media;
    let omitted = files.len() - selected.len();
    let mut omission = if omitted == 0 {
        String::new()
    } else {
        let last = first + messages.len().saturating_sub(1);
        match language {
            Language::En => format!(
                "\n[{omitted} media parts omitted from summary input; request-source message positions {first}–{last}. The complete original history remains stored locally.]\n"
            ),
            Language::ZhCn => format!(
                "\n[总结输入省略了 {omitted} 个媒体部分；请求来源消息位置 {first}–{last}。完整原始历史仍保存在本地。]\n"
            ),
            Language::ZhTw => format!(
                "\n[總結輸入省略了 {omitted} 個媒體部分；請求來源訊息位置 {first}–{last}。完整原始歷史仍儲存在本機。]\n"
            ),
        }
    };
    if omitted != 0 {
        let labels_budget = (bytes / 4).min(bytes.saturating_sub(media_bytes + omission.len()));
        omission.push_str(&excerpt(&omitted_labels.concat(), labels_budget, language));
    }
    let mut content = vec![ContentPart::text(excerpt(
        &rendered,
        bytes.saturating_sub(media_bytes + omission.len()),
        language,
    ))];
    if !omission.is_empty() {
        content.push(ContentPart::text(omission));
    }
    for (label, file) in selected {
        content.push(ContentPart::text(label));
        content.push(file);
    }
    content
}

fn extract_media(
    parts: &mut [ContentPart],
    source: &str,
    files: &mut Vec<(String, ContentPart)>,
    language: Language,
) {
    for (index, part) in parts.iter_mut().enumerate() {
        if let ContentPart::File { media_type, .. } = part {
            let label = match language {
                Language::En => format!(
                    "[Media: {source}, content part {index}, {media_type}; original in locally stored history]\n"
                ),
                Language::ZhCn => format!(
                    "[媒体：{source}，内容部分 {index}，{media_type}；原件保存在本地历史中]\n"
                ),
                Language::ZhTw => format!(
                    "[媒體：{source}，內容部分 {index}，{media_type}；原件儲存在本機歷史中]\n"
                ),
            };
            let file = std::mem::replace(part, ContentPart::text(&label));
            files.push((label, file));
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use maka_model::prompt::FileData;

    #[test]
    fn overlapping_anchor_and_tail_account_for_media_once() {
        let file = ContentPart::File {
            data: FileData::Data("distinctive-media-payload".into()),
            media_type: "image/png".into(),
            provider_options: Some(serde_json::json!({"detail":"high"})),
        };
        let system = Message::System {
            content: "Summary instructions".into(),
            provider_options: None,
        };
        let prompt = vec![
            system.clone(),
            Message::User {
                content: vec![ContentPart::text("Original task and image"), file.clone()],
                provider_options: None,
            },
            Message::Assistant {
                content: vec![AssistantPart::Text {
                    text: format!(
                        "{}Recent progress; effect remains unknown",
                        "older log ".repeat(5000)
                    ),
                    provider_options: None,
                }],
                provider_options: None,
            },
        ];
        for (tokens, expected_files) in [(2000, 1), (500, 0)] {
            let fitted = fit_summary(&prompt, tokens, Language::En);
            assert!(estimate(&fitted) <= tokens);
            assert_eq!(fitted[0], system);
            let Message::User { content, .. } = &fitted[1] else {
                panic!("historical excerpt");
            };
            let files: Vec<_> = content
                .iter()
                .filter(|part| matches!(part, ContentPart::File { .. }))
                .collect();
            assert_eq!(files.len(), expected_files);
            let text: String = content
                .iter()
                .filter_map(|part| match part {
                    ContentPart::Text { text, .. } => Some(text.as_str()),
                    ContentPart::File { .. } => None,
                })
                .collect();
            assert!(!text.contains("distinctive-media-payload"));
            if expected_files == 1 {
                assert_eq!(files[0], &file);
                assert!(!text.contains("media parts omitted"));
            } else {
                assert!(text.contains("1 media parts omitted"));
                assert!(text.contains("request-source message position"));
                assert!(text.contains("image/png"));
            }
        }
    }
}
