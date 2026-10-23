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

//! Native provider sessions own wire state and use only Host-scoped transport.
mod chat_completions;
mod error;
mod events;
mod media;
mod prompt;
mod request;
use crate::{ModelError, ProviderFailureReason};
use maka_agent_facts::model::budget;
use maka_agent_ports::model::{Context, Emission, Lifetime, ProviderAdapter, Request, Session};
use maka_capabilities::http;
use maka_provider_protocol::{anthropic, chat_completions as chat, output::Event};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

const WINDOW: u32 = 32 * 1024 * 1024;
const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
struct Shared {
    input: Arc<Semaphore>,
    output: Arc<Semaphore>,
    without_usage: Mutex<HashSet<(u64, String)>>,
}
impl Default for Shared {
    fn default() -> Self {
        Self {
            input: Arc::new(Semaphore::new(WINDOW as usize)),
            output: Arc::new(Semaphore::new(WINDOW as usize)),
            without_usage: Mutex::default(),
        }
    }
}
#[derive(Default)]
pub(super) struct Adapter(Arc<Shared>);
impl ProviderAdapter for Adapter {
    fn enforces_idle_timeout(&self) -> bool {
        true
    }
    fn open(
        &self,
        _: Lifetime,
        _: CancellationToken,
    ) -> futures_util::future::BoxFuture<'_, Result<Arc<dyn Session>, ModelError>> {
        let shared = self.0.clone();
        Box::pin(async move { Ok(Arc::new(Native(shared)) as Arc<dyn Session>) })
    }
}
struct Native(Arc<Shared>);
impl Session for Native {
    fn stream(
        &self,
        request: Request,
        context: Context,
    ) -> futures_util::future::BoxFuture<'static, Result<(), ModelError>> {
        let shared = self.0.clone();
        Box::pin(async move {
            tokio::select! {
                biased;
                _ = context.cancellation.cancelled() => Err(ModelError::Cancelled),
                result = run(request, &context, shared) => result,
            }
        })
    }
}
enum Decoder {
    Anthropic(Box<anthropic::stream::Stream>),
    Chat(Box<chat::Stream>),
}
impl Decoder {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Event>, ModelError> {
        let mut next_id = || uuid::Uuid::new_v4().to_string();
        match self {
            Self::Anthropic(stream) => stream.push(bytes, &mut next_id, |_| {}).map_err(failure),
            Self::Chat(stream) => {
                let mut events = stream.push(bytes, &mut next_id).map_err(failure)?;
                if stream.terminal() {
                    events.extend(stream.finish(&mut next_id).map_err(failure)?);
                }
                Ok(events)
            }
        }
    }
    fn finish(&mut self) -> Result<Vec<Event>, ModelError> {
        match self {
            Self::Anthropic(stream) => Ok(stream.finish()),
            Self::Chat(stream) => stream
                .finish(&mut || uuid::Uuid::new_v4().to_string())
                .map_err(failure),
        }
    }
}
fn failure(error: maka_provider_protocol::Error) -> ModelError {
    ModelError::Adapter(error.to_string())
}

async fn run(request: Request, context: &Context, shared: Arc<Shared>) -> Result<(), ModelError> {
    let size = budget::reservation(&request, WINDOW)?;
    let _input = shared
        .input
        .clone()
        .acquire_many_owned(size)
        .await
        .map_err(|_| ModelError::Cancelled)?;
    let mut prepared = request::prepare(request)?;
    let route = (context.transport.identity(), prepared.url.clone());
    let remembered = prepared.compatible
        && shared
            .without_usage
            .lock()
            .expect("usage cache")
            .contains(&route);
    if remembered {
        prepared
            .body
            .as_object_mut()
            .expect("request object")
            .remove("stream_options");
    }
    let mut events = events::Events::new(prepared.anthropic);
    let mut retried = remembered;
    loop {
        let response = tokio::time::timeout(
            context.idle_timeout,
            context.transport.request(http::Request {
                method: http::Method::Post,
                url: prepared.url.clone(),
                headers: prepared.headers.clone(),
                body: serde_json::to_vec(&prepared.body)
                    .map_err(|e| ModelError::Adapter(e.to_string()))?,
            }),
        )
        .await
        .map_err(|_| ModelError::TimedOut)?
        .map_err(|e| events.evidence.transport(e))?;
        let body = BodyGuard(response.body);
        context.events.progress();
        let status = response.head.status;
        let sse = header(&response.head, "content-type").is_some_and(|mime| {
            mime.split(';')
                .next()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("text/event-stream"))
        });
        if !(200..300).contains(&status) || !sse {
            let retry_after = retry_after(&response.head);
            let read = read_body(&body.0, &response.head, context).await;
            let bytes = match read {
                Ok(bytes) => bytes,
                Err(ModelError::Provider(failure))
                    if failure.reason() == ProviderFailureReason::Network
                        && !(200..300).contains(&status) =>
                {
                    return Err(events.evidence.provider(
                        error::http_error(status, &[]),
                        prepared.anthropic,
                        retry_after,
                    ));
                }
                Err(error) => return Err(error),
            };
            if prepared.compatible
                && !retried
                && status == 400
                && prepared.body.get("stream_options").is_some()
            {
                let text = String::from_utf8_lossy(&bytes).to_ascii_lowercase();
                if text.contains("stream_options") || text.contains("include_usage") {
                    {
                        let mut cache = shared.without_usage.lock().expect("usage cache");
                        if cache.len() >= 1024 {
                            cache.clear();
                        }
                        cache.insert(route.clone());
                    }
                    prepared
                        .body
                        .as_object_mut()
                        .expect("request object")
                        .remove("stream_options");
                    retried = true;
                    body.0
                        .close()
                        .await
                        .map_err(|e| ModelError::Adapter(e.to_string()))?;
                    continue;
                }
            }
            return Err(events.evidence.provider(
                error::http_error(status, &bytes),
                prepared.anthropic,
                retry_after,
            ));
        }
        // Release request storage after dispatch. Admission remains reserved
        // through physical stream settlement; output has its own byte window.
        drop(prepared.body);
        loop {
            let chunk = tokio::time::timeout(context.idle_timeout, body.0.next())
                .await
                .map_err(|_| ModelError::TimedOut)?
                .map_err(|error| match error {
                    http::Error::Denied => ModelError::Cancelled,
                    http::Error::Failed(_) => events.evidence.interrupted(
                        ProviderFailureReason::Network,
                        "model HTTP transport interrupted",
                    ),
                    other => ModelError::Adapter(other.to_string()),
                })?;
            match chunk {
                Some(bytes) => {
                    context.events.progress();
                    // Bound the vector of decoded events even if an HTTP peer
                    // combines many records into one transport chunk.
                    for bytes in bytes.chunks(16 * 1024) {
                        let decoded = prepared.decoder.push(bytes)?;
                        emit(decoded, &mut events, context, &shared).await?;
                        if events.finished {
                            body.0
                                .close()
                                .await
                                .map_err(|e| ModelError::Adapter(e.to_string()))?;
                            return Ok(());
                        }
                    }
                }
                None => {
                    emit(prepared.decoder.finish()?, &mut events, context, &shared).await?;
                    return if events.finished {
                        Ok(())
                    } else {
                        Err(events.evidence.interrupted(
                            ProviderFailureReason::StreamTruncated,
                            "model stream ended without finish",
                        ))
                    };
                }
            }
        }
    }
}
async fn emit(
    decoded: Vec<Event>,
    events: &mut events::Events,
    context: &Context,
    shared: &Shared,
) -> Result<(), ModelError> {
    for event in decoded {
        if let Some(event) = events.push(event)? {
            let size = budget::bytes(&event, WINDOW)?;
            let reservation = shared
                .output
                .clone()
                .acquire_many_owned(size)
                .await
                .map_err(|_| ModelError::Cancelled)?;
            context
                .events
                .emit(Emission {
                    event,
                    reservation: Some(Arc::new(reservation)),
                })
                .await?;
        }
        if events.finished {
            break;
        }
    }
    Ok(())
}
struct BodyGuard(Arc<dyn http::Body>);
impl Drop for BodyGuard {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
fn header<'a>(head: &'a http::Head, name: &str) -> Option<&'a str> {
    head.headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| std::str::from_utf8(value).ok())
}
async fn read_body(
    body: &Arc<dyn http::Body>,
    head: &http::Head,
    context: &Context,
) -> Result<Vec<u8>, ModelError> {
    let limit = || ModelError::Adapter("provider response body exceeds 8 MiB".into());
    if header(head, "content-length")
        .and_then(|n| n.parse::<usize>().ok())
        .is_some_and(|n| n > RESPONSE_LIMIT)
    {
        return Err(limit());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = tokio::time::timeout(context.idle_timeout, body.next())
        .await
        .map_err(|_| ModelError::TimedOut)?
        .map_err(|error| match error {
            http::Error::Denied => ModelError::Cancelled,
            http::Error::Failed(_) => ModelError::Provider(crate::ProviderFailure::new(
                ProviderFailureReason::Network,
                "error body interrupted",
                true,
                None,
            )),
            other => ModelError::Adapter(other.to_string()),
        })?
    {
        context.events.progress();
        if chunk.len() > RESPONSE_LIMIT - bytes.len() {
            return Err(limit());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn retry_after(head: &http::Head) -> Option<u64> {
    let delay = if let Some(ms) = header(head, "retry-after-ms") {
        ms.parse::<f64>().ok()?
    } else {
        let seconds = header(head, "retry-after")?;
        if let Ok(seconds) = seconds.parse::<f64>() {
            seconds * 1000.0
        } else {
            let date = jiff::fmt::rfc2822::parse(seconds).ok()?;
            let now = jiff::Timestamp::now().as_millisecond();
            (date.timestamp().as_millisecond() - now) as f64
        }
    };
    (delay.is_finite() && delay > 0.0 && delay <= 2_147_483_647.0).then_some(delay.ceil() as u64)
}
