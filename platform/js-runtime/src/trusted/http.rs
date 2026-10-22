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

use super::ops::Models;
use deno_core::{OpState, op2};
use deno_error::JsErrorBox;
use maka_agent_ports::model::Transport;
use maka_capabilities::http;
use serde::Serialize;
use serde_json::{Value, json, ser::Formatter};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    future::Future,
    io::{self, Write},
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Mutex as AsyncMutex, watch},
    time::Instant,
};
use tokio_util::sync::CancellationToken;

pub(super) struct Exchange {
    network: Arc<dyn Transport>,
    response: AsyncMutex<Option<http::Response>>,
    cancellation: CancellationToken,
    current: Mutex<CancellationToken>,
    idle_timeout: Duration,
    output: watch::Sender<OutputState>,
}

#[derive(Clone, Copy)]
struct OutputState {
    waiting: usize,
    resumed: Instant,
}

pub(super) struct OutputWait(watch::Sender<OutputState>);

impl Drop for OutputWait {
    fn drop(&mut self) {
        self.0.send_modify(|state| {
            state.waiting -= 1;
            if state.waiting == 0 {
                state.resumed = Instant::now();
            }
        });
    }
}

impl Exchange {
    pub fn new(
        network: Arc<dyn Transport>,
        cancellation: CancellationToken,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            network,
            response: AsyncMutex::new(None),
            current: Mutex::new(cancellation.child_token()),
            cancellation,
            idle_timeout,
            output: watch::channel(OutputState {
                waiting: 0,
                resumed: Instant::now(),
            })
            .0,
        }
    }

    pub fn wait_for_output(&self) -> OutputWait {
        self.output.send_modify(|state| state.waiting += 1);
        OutputWait(self.output.clone())
    }

    async fn wait<T>(&self, operation: impl Future<Output = T>) -> Result<T, JsErrorBox> {
        let started = Instant::now();
        let mut output = self.output.subscribe();
        tokio::pin!(operation);
        loop {
            let state = *output.borrow_and_update();
            // SDK streams may prefetch while the consumer's bounded queue is
            // full. Resume a fresh network idle interval only when local output
            // capacity returns; cancellation remains independently observable.
            let deadline = started.max(state.resumed) + self.idle_timeout;
            tokio::select! {
                biased;
                result = &mut operation => return Ok(result),
                _ = output.changed() => {},
                _ = tokio::time::sleep_until(deadline), if state.waiting == 0 =>
                    return Err(JsErrorBox::from_err(IdleTimeout)),
            }
        }
    }
    async fn start(
        &self,
        method: String,
        url: String,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    ) -> Result<Value, JsErrorBox> {
        self.close().await;
        let method: http::Method =
            serde_json::from_value(json!(method)).map_err(|_| failed("invalid HTTP method"))?;
        let head = matches!(method, http::Method::Head);
        let cancellation = self.cancellation.child_token();
        *self.current.lock().unwrap() = cancellation.clone();
        let mut slot = self.response.lock().await;
        let response = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(failed("HTTP request cancelled")),
            response = self.wait(self.network.request(
                http::Request { method, url, headers: headers.into_iter().collect(), body }
            )) => response?
                .map_err(|error| match error {
                    maka_agent_ports::model::Error::Provider(_) => JsErrorBox::from_err(TransportFailure),
                    other => JsErrorBox::generic(other.to_string()),
                })?,
        };
        let status = response.head.status;
        let headers: Vec<_> = response
            .head
            .headers
            .iter()
            .map(|(name, value)| {
                [
                    name.clone(),
                    value.iter().copied().map(char::from).collect(),
                ]
            })
            .collect();
        let has_body = !head && !matches!(status, 204 | 205 | 304);
        let result = json!({"status": status, "headers": headers, "hasBody": has_body});
        *slot = Some(response);
        Ok(result)
    }
    async fn chunk(&self) -> Result<Option<Vec<u8>>, JsErrorBox> {
        let cancellation = self.current.lock().unwrap().clone();
        let mut slot = self.response.lock().await;
        let Some(response) = slot.as_ref() else {
            return Ok(None);
        };
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(failed("HTTP request cancelled")),
            result = self.wait(response.body.next()) =>
                result.and_then(|result| result.map_err(|error| match error {
                http::Error::Failed(_) => JsErrorBox::from_err(TransportFailure),
                other => JsErrorBox::generic(other.to_string()),
            })),
        };
        if !matches!(result, Ok(Some(_)))
            && let Some(response) = slot.take()
        {
            let _ = response.body.close().await;
        }
        result
    }
    async fn close(&self) {
        self.current.lock().unwrap().cancel();
        if let Some(response) = self.response.lock().await.take() {
            let _ = response.body.close().await;
        }
    }
}
fn exchange(state: &OpState, id: u32) -> Result<Rc<Exchange>, JsErrorBox> {
    state
        .borrow::<Models>()
        .active
        .get(&id)
        .map(|model| model.http.clone())
        .ok_or_else(|| failed("HTTP model request closed"))
}
fn failed(message: &'static str) -> JsErrorBox {
    JsErrorBox::generic(message)
}
#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[property("code" = "MAKA_HTTP_TRANSPORT")]
#[error("model HTTP transport interrupted")]
struct TransportFailure;

#[derive(Debug, thiserror::Error, deno_error::JsError)]
#[class(generic)]
#[property("code" = "MAKA_MODEL_IDLE_TIMEOUT")]
#[error("model stream idle timeout exceeded")]
struct IdleTimeout;

#[op2]
#[serde]
pub(super) async fn op_http_json_start(
    state: Rc<RefCell<OpState>>,
    id: u32,
    #[string] method: String,
    #[string] url: String,
    #[serde] headers: BTreeMap<String, String>,
    #[serde] body: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let exchange = exchange(&state.borrow(), id)?;
    let encoded = encode_json(&body, &exchange.cancellation)?;
    drop(body);
    exchange.start(method, url, headers, encoded).await
}

#[op2]
#[buffer]
pub(super) async fn op_http_chunk(
    state: Rc<RefCell<OpState>>,
    id: u32,
) -> Result<Option<Vec<u8>>, JsErrorBox> {
    let exchange = exchange(&state.borrow(), id)?;
    exchange.chunk().await
}

#[op2]
pub(super) async fn op_http_close(state: Rc<RefCell<OpState>>, id: u32) -> Result<(), JsErrorBox> {
    let exchange = exchange(&state.borrow(), id)?;
    exchange.close().await;
    Ok(())
}

struct JsonWriter<'a> {
    bytes: Vec<u8>,
    cancellation: &'a CancellationToken,
}
impl Write for JsonWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.cancellation.is_cancelled() {
            return Err(io::Error::other("HTTP request cancelled"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct JsonFormatter;
impl Formatter for JsonFormatter {
    fn write_f64<W>(&mut self, writer: &mut W, value: f64) -> io::Result<()>
    where
        W: Write + ?Sized,
    {
        let mut buffer = ryu_js::Buffer::new();
        writer.write_all(buffer.format_finite(value).as_bytes())
    }
}

fn encode_json(body: &Value, cancellation: &CancellationToken) -> Result<Vec<u8>, JsErrorBox> {
    if cancellation.is_cancelled() {
        return Err(failed("HTTP request cancelled"));
    }
    let mut writer = JsonWriter {
        bytes: Vec::new(),
        cancellation,
    };
    let result = body.serialize(&mut serde_json::Serializer::with_formatter(
        &mut writer,
        JsonFormatter,
    ));
    // Use the model token, not the previous HTTP child's token: close() cancels
    // that child even when a normal provider-policy retry is about to start.
    if cancellation.is_cancelled() {
        return Err(failed("HTTP request cancelled"));
    }
    result.map_err(|_| failed("HTTP JSON encoding failed"))?;
    Ok(writer.bytes)
}

#[cfg(test)]
mod tests;
