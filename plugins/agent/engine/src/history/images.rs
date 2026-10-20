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

use base64::{Engine as _, engine::general_purpose::STANDARD};
use maka_agent_facts::attachment::AttachmentRef;
use maka_agent_facts::attachment::StorageRef;
use maka_agent_facts::attachment::sniff_binary_mime;
use maka_agent_facts::event::StoredEvent;
use maka_agent_facts::tool_output::AudioOutput;
use maka_agent_facts::tool_output::ImageOutput;
use maka_event_log::EventLog;
use maka_model::prompt::{ContentPart, FileData, Message, ToolOutput};
use tokio_util::sync::CancellationToken;

use crate::RunError;

pub(super) fn no_vision(language: Language) -> &'static str {
    match language {
        Language::En => "Image was read, but the selected model does not support image input.",
        Language::ZhCn => "已读取图片，但所选模型不支持图片输入。",
        Language::ZhTw => "已讀取圖片，但所選模型不支援圖片輸入。",
    }
}

/// Targets borrow canonical evidence and are emitted alongside their messages.
/// Arbitrary JSON tool results cannot authorize a storage read.
pub(super) enum Target<'a> {
    Audio {
        message: usize,
        part: usize,
        audio: &'a AudioOutput,
    },
    User {
        message: usize,
        image: &'a AttachmentRef,
    },
    Tool {
        message: usize,
        part: usize,
        image: &'a ImageOutput,
    },
}

enum ImageRead {
    Bytes(Vec<u8>),
    Unavailable(&'static str),
}

#[allow(clippy::too_many_arguments)] // Model projection needs both the replay cut and admission policy.
pub(crate) async fn materialize_replay(
    log: &EventLog,
    events: &[maka_event_log::context::ContextEvent],
    anchor: Option<&StoredEvent>,
    session: &str,
    vision: bool,
    cancellation: &CancellationToken,
    replay: Option<super::Replay<'_>>,
    prior_unknown: bool,
    language: Language,
) -> Result<Vec<Message>, RunError> {
    let mut targets = Vec::new();
    let selected = anchor
        .into_iter()
        .map(super::EventRef::Canonical)
        .chain(events.iter().map(super::EventRef::from));
    let resources =
        super::resources::Resources::load(log, session, selected.clone(), cancellation).await?;
    let mut messages = super::build(
        selected,
        &resources,
        &mut targets,
        vision,
        replay,
        prior_unknown,
        language,
    )?;
    for target in targets {
        if cancellation.is_cancelled() {
            return Err(RunError::Cancelled);
        }
        let (reference, mime) = match &target {
            Target::Audio { audio, .. } => (&audio.reference, &audio.mime_type),
            Target::User { image, .. } => (&image.storage_ref, &image.mime_type),
            Target::Tool { image, .. } => (&image.reference, &image.mime_type),
        };
        let audio = matches!(target, Target::Audio { .. });
        let read = match resources.resolve(reference) {
            Some(reference) => read(log, session, reference, audio).await?,
            None => ImageRead::Unavailable("session_mismatch"),
        };
        if cancellation.is_cancelled() {
            return Err(RunError::Cancelled);
        }
        let part = match read {
            ImageRead::Bytes(bytes) => ContentPart::File {
                data: FileData::Data(STANDARD.encode(bytes)),
                media_type: mime.clone(),
                provider_options: match &target {
                    Target::Tool { image, .. } => image
                        .detail
                        .map(|detail| serde_json::json!({"openai":{"imageDetail":detail}})),
                    Target::User { .. } | Target::Audio { .. } => None,
                },
            },
            ImageRead::Unavailable(reason) => match &target {
                Target::User { image, .. } => ContentPart::text(match language {
                    Language::En => format!(
                        "Image attachment \"{}\" could not be loaded: {reason}.",
                        image.name
                    ),
                    Language::ZhCn => format!("无法加载图片附件 \"{}\"：{reason}。", image.name),
                    Language::ZhTw => format!("無法載入圖片附件 \"{}\"：{reason}。", image.name),
                }),
                Target::Audio { .. } => ContentPart::text(match language {
                    Language::En => {
                        format!("Audio could not be loaded from artifact storage: {reason}.")
                    }
                    Language::ZhCn => format!("无法从工件存储加载音频：{reason}。"),
                    Language::ZhTw => format!("無法從成品儲存載入音訊：{reason}。"),
                }),
                Target::Tool { .. } => ContentPart::text(match language {
                    Language::En => {
                        format!("Image could not be loaded from artifact storage: {reason}.")
                    }
                    Language::ZhCn => format!("无法从工件存储加载图片：{reason}。"),
                    Language::ZhTw => format!("無法從成品儲存載入圖片：{reason}。"),
                }),
            },
        };
        match target {
            Target::User { message, .. } => {
                let Message::User { content, .. } = &mut messages[message] else {
                    unreachable!("user image target comes from the user message builder");
                };
                content.push(part);
            }
            Target::Audio {
                message,
                part: index,
                ..
            }
            | Target::Tool {
                message,
                part: index,
                ..
            } => {
                let Message::Tool { content, .. } = &mut messages[message] else {
                    unreachable!("tool image target comes from the tool message builder");
                };
                let ToolOutput::Content(parts) = &mut content[0].output else {
                    unreachable!("image target comes from a structured content output");
                };
                parts[index] = part;
            }
        }
    }
    if cancellation.is_cancelled() {
        return Err(RunError::Cancelled);
    }
    Ok(messages)
}

async fn read(
    log: &EventLog,
    session: &str,
    reference: &StorageRef,
    audio: bool,
) -> Result<ImageRead, RunError> {
    let StorageRef::SessionFile {
        session_id,
        relative_path,
    } = reference
    else {
        return Ok(ImageRead::Unavailable("unsupported_ref_kind"));
    };
    if session_id != session {
        return Ok(ImageRead::Unavailable("session_mismatch"));
    }
    if maka_agent_facts::interaction::entity_id(relative_path).is_err() {
        return Ok(ImageRead::Unavailable("not_found"));
    }
    let Some(bytes) = log
        .with_artifact(
            maka_event_log::artifacts::ArtifactReadScope::Session(session),
            relative_path,
            |_, reader| {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes)?;
                Ok::<_, maka_event_log::StoreError>(bytes)
            },
        )
        .await?
        .transpose()?
    else {
        return Ok(ImageRead::Unavailable("not_found"));
    };
    if !audio && sniff_binary_mime(&bytes).is_none() {
        return Ok(ImageRead::Unavailable("unsupported_mime"));
    }
    Ok(ImageRead::Bytes(bytes))
}
