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
use crate::trusted::ops::{Models, Output};
use deno_core::{JsRuntime, RuntimeOptions};
use futures_util::future::BoxFuture;
use maka_agent_ports::model::{Connect, Error, Socket};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Network {
    requests: Mutex<Vec<http::Request>>,
    closed: Arc<AtomicUsize>,
}
impl Transport for Network {
    fn identity(&self) -> u64 {
        1
    }
    fn request(&self, request: http::Request) -> BoxFuture<'_, Result<http::Response, Error>> {
        self.requests.lock().unwrap().push(request);
        Box::pin(async {
            Ok(http::Response {
                head: http::Head {
                    status: 204,
                    url: "https://fixture.invalid/model".into(),
                    headers: Vec::new(),
                },
                body: Arc::new(Body(self.closed.clone())),
            })
        })
    }
    fn connect(&self, _: Connect) -> BoxFuture<'_, Result<Arc<dyn Socket>, Error>> {
        panic!("JSON HTTP must not open a WebSocket")
    }
}
struct Body(Arc<AtomicUsize>);
impl http::Body for Body {
    fn next(&self) -> BoxFuture<'_, Result<Option<Vec<u8>>, http::Error>> {
        Box::pin(async { Ok(None) })
    }
    fn cancel(&self) {}
    fn close(&self) -> BoxFuture<'_, Result<(), http::Error>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn json_op_preserves_shaped_values_and_original_request_authority() {
    crate::initialize_platform();
    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![crate::trusted::ops::maka_trusted::init()],
        ..Default::default()
    });
    let network = Arc::new(Network::default());
    let cancellation = CancellationToken::new();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);
    let mut models = Models::default();
    for (id, token) in [(1, cancellation.clone()), (2, cancelled)] {
        models.active.insert(
            id,
            Output {
                http: Rc::new(Exchange::new(network.clone(), token.clone())),
                sender: sender.clone(),
                cancellation: token,
                endpoint: "fixture".into(),
            },
        );
    }
    runtime.op_state().borrow_mut().put(models);
    let promise = runtime
        .execute_script(
            "json-http-contract",
            r#"
        (async () => {
          const ops = Deno.core.ops;
          const send = (id, body) => ops.op_http_json_start(
            id, 'POST', 'https://fixture.invalid/model', {'x-fixture': 'present'}, body);
          const refused = async (action, message) => {
            let error;
            try { await action(); } catch (caught) { error = caught; }
            if (!error || error.code === 'MAKA_HTTP_TRANSPORT' ||
                (message && !error.message.includes(message))) {
              throw new Error('Expected local JSON/authority rejection');
            }
          };
          await refused(() => send(77, {}), 'HTTP model request closed');
          await refused(() => send(1, {value: 1n}));
          await refused(() => send(2, {}), 'HTTP request cancelled');
          const body = {
            omitted: undefined,
            nested: {present: true, omitted: undefined},
            array: [undefined, null, NaN, Infinity, -Infinity],
            text: '界😀\u0000"\\\n',
            number: 1.25,
          };
          const first = await send(1, body);
          if (first.status !== 204) throw new Error('HTTP response lost');
          await ops.op_http_close(1);
          // Closing an HTTP attempt must not cancel the owning model or its retry.
          const second = await send(1, {retry: true});
          if (second.status !== 204) throw new Error('HTTP retry rejected');
          await ops.op_http_close(1);
          const numbers = await send(1, [
            4294967296, 10000000000, 9007199254740991, 9007199254740992,
            1000000000000000100, 1e21, 1e-6, 1e-7, -0, -10000000000,
          ]);
          if (numbers.status !== 204) throw new Error('Numeric body rejected');
          await ops.op_http_close(1);
        })()
    "#,
        )
        .unwrap();
    let resolving = runtime.resolve(promise);
    runtime
        .with_event_loop_promise(resolving, Default::default())
        .await
        .unwrap();
    assert!(!cancellation.is_cancelled());
    assert_eq!(network.closed.load(Ordering::SeqCst), 3);
    let requests = network.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(matches!(requests[0].method, http::Method::Post));
    assert_eq!(requests[0].url, "https://fixture.invalid/model");
    assert_eq!(
        requests[0].headers,
        [("x-fixture".into(), "present".into())]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[0].body).unwrap(),
        json!({
            "nested": {"present": true},
            "array": [null, null, null, null, null],
            "text": "界😀\0\"\\\n",
            "number": 1.25,
        })
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&requests[1].body).unwrap(),
        json!({"retry": true})
    );
    assert_eq!(
        requests[2].body.as_slice(),
        b"[4294967296,10000000000,9007199254740991,9007199254740992,1000000000000000100,1e+21,0.000001,1e-7,0,-10000000000]"
    );
}

#[test]
fn json_encoding_stops_at_the_next_write_after_model_cancellation() {
    let cancellation = CancellationToken::new();
    let mut writer = JsonWriter {
        bytes: Vec::new(),
        cancellation: &cancellation,
    };
    writer.write_all(b"[").unwrap();
    cancellation.cancel();
    let error = serde_json::to_writer(&mut writer, &json!({"not_sent": true})).unwrap_err();
    assert_eq!(error.io_error_kind(), Some(io::ErrorKind::Other));
    assert_eq!(writer.bytes, b"[");
    assert_eq!(
        encode_json(&json!({"not_sent": true}), &cancellation)
            .unwrap_err()
            .to_string(),
        "HTTP request cancelled"
    );
}

#[test]
#[ignore = "allocates 553,041,312 JSON string bytes to measure native encoding cancellation"]
fn large_json_encoding_cancellation_measurement() {
    use std::{
        sync::Barrier,
        time::{Duration, Instant},
    };

    let generation_started = Instant::now();
    let body = Value::Array(
        (0..8)
            .map(|_| Value::String("A".repeat(69_130_164)))
            .collect(),
    );
    let generation_finished = Instant::now();
    let cancellation = CancellationToken::new();
    let encoding_ready = Arc::new(Barrier::new(2));
    let cancel = cancellation.clone();
    let ready = encoding_ready.clone();
    let cancelling = std::thread::spawn(move || {
        ready.wait();
        std::thread::sleep(Duration::from_millis(10));
        let cancelled_at = Instant::now();
        cancel.cancel();
        cancelled_at
    });
    let mut writer = JsonWriter {
        bytes: Vec::new(),
        cancellation: &cancellation,
    };
    let encoding_started = Instant::now();
    encoding_ready.wait();
    let result = body.serialize(&mut serde_json::Serializer::with_formatter(
        &mut writer,
        JsonFormatter,
    ));
    let encoding_finished = Instant::now();
    let cancelled_at = cancelling.join().unwrap();
    let written = writer.bytes.len();
    let error_kind = result
        .as_ref()
        .err()
        .and_then(serde_json::Error::io_error_kind);
    // This measures native encoding only; V8 conversion and real image HTTP
    // delivery have separate tests. A single scan/write is not preemptible.
    println!(
        "{}",
        json!({
            "case": "large_json_encoding_cancellation_measurement",
            "leafCount": 8,
            "leafCharacters": 69_130_164,
            "totalCharacters": 553_041_312,
            "generationSeconds": generation_finished.duration_since(generation_started).as_secs_f64(),
            "encodingSeconds": encoding_finished.duration_since(encoding_started).as_secs_f64(),
            "cancelAfterEncodingStartSeconds": cancelled_at.duration_since(encoding_started).as_secs_f64(),
            "returnAfterCancelSeconds": encoding_finished.checked_duration_since(cancelled_at).map(|elapsed| elapsed.as_secs_f64()),
            "writtenBytes": written,
            "errorKind": error_kind.map(|kind| format!("{kind:?}")),
        })
    );
    assert!(cancelled_at >= encoding_started);
    assert!(
        encoding_finished >= cancelled_at,
        "encoding finished before cancellation"
    );
    assert!(
        written > 0,
        "cancellation happened before any encoding output"
    );
    assert!(
        written < 553_041_312,
        "cancellation did not interrupt the aggregate body"
    );
    assert_eq!(error_kind, Some(io::ErrorKind::Other));
}
