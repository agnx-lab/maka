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

//! Host-owned model I/O shared by native and JavaScript adapters.
use futures_util::{SinkExt, StreamExt, future::BoxFuture};
use maka_agent_facts::model::error::ProviderFailure;
use maka_agent_facts::model::error::ProviderFailureReason;
use maka_agent_ports::model::Connect;
use maka_agent_ports::model::Error;
use maka_agent_ports::model::Frame;
use maka_agent_ports::model::Socket;
use maka_agent_ports::model::Transport;
use maka_capabilities::http;
use std::{
    collections::{HashMap, hash_map::RandomState},
    hash::BuildHasher,
    sync::{Arc, Mutex},
};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

/// The pool owns connections; this handle admits I/O only while its call lives.
pub(super) struct Call {
    pub transport: Arc<dyn Transport>,
    pub events: Arc<dyn maka_agent_ports::model::Events>,
    pub cancellation: CancellationToken,
}
impl Transport for Call {
    fn identity(&self) -> u64 {
        self.transport.identity()
    }
    fn request(&self, request: http::Request) -> BoxFuture<'_, Result<http::Response, Error>> {
        Box::pin(async move {
            let response = tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Err(Error::Cancelled),
                response = self.transport.request(request) => response?,
            };
            self.events.progress();
            Ok(http::Response {
                head: response.head,
                body: Arc::new(CallBody {
                    body: response.body,
                    events: self.events.clone(),
                    cancellation: self.cancellation.clone(),
                }),
            })
        })
    }
    fn connect(&self, request: Connect) -> BoxFuture<'_, Result<Arc<dyn Socket>, Error>> {
        Box::pin(async move {
            let socket = tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Err(Error::Cancelled),
                socket = self.transport.connect(request) => socket?,
            };
            self.events.progress();
            // Accepted sockets belong to the adapter session and may outlive
            // this request. Each new call supplies its own cancellation context.
            Ok(socket)
        })
    }
}
struct CallBody {
    body: Arc<dyn http::Body>,
    events: Arc<dyn maka_agent_ports::model::Events>,
    cancellation: CancellationToken,
}
impl http::Body for CallBody {
    fn next(&self) -> BoxFuture<'_, Result<Option<Vec<u8>>, http::Error>> {
        Box::pin(async move {
            let result = tokio::select! {
                biased;
                _ = self.cancellation.cancelled() => return Err(http::Error::Denied),
                result = self.body.next() => result?,
            };
            self.events.progress();
            Ok(result)
        })
    }
    fn cancel(&self) {
        self.body.cancel();
    }
    fn close(&self) -> BoxFuture<'_, Result<(), http::Error>> {
        self.body.close()
    }
}

#[derive(Default)]
pub(super) struct Pool {
    routes: Mutex<HashMap<maka_network::Policy, Arc<Network>>>,
    identity: RandomState,
}
impl Pool {
    pub fn get(&self, policy: &maka_network::Policy) -> Result<Arc<dyn Transport>, Error> {
        let mut routes = self.routes.lock().unwrap();
        if let Some(network) = routes.get(policy) {
            return Ok(network.clone());
        }
        let network = Arc::new(Network {
            policy: policy.clone(),
            client: policy
                .client_builder()
                .build()
                .map_err(|_| Error::Adapter("model network initialization failed".into()))?,
            identity: self.identity.hash_one(policy),
        });
        // Settings edits cannot accumulate an unbounded pool of obsolete clients.
        if routes.len() >= 16 {
            routes.clear();
        }
        routes.insert(policy.clone(), network.clone());
        Ok(network)
    }
}
struct Network {
    policy: maka_network::Policy,
    client: reqwest::Client,
    identity: u64,
}
fn headers(values: Vec<(String, String)>) -> Result<reqwest::header::HeaderMap, Error> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in values {
        let name = name
            .parse::<reqwest::header::HeaderName>()
            .map_err(|_| Error::Adapter("invalid model HTTP header".into()))?;
        let bytes = value
            .chars()
            .map(u8::try_from)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Error::Adapter("invalid model HTTP header".into()))?;
        let value = reqwest::header::HeaderValue::from_bytes(&bytes)
            .map_err(|_| Error::Adapter("invalid model HTTP header".into()))?;
        headers
            .try_append(name, value)
            .map_err(|_| Error::Adapter("model HTTP headers cannot be represented".into()))?;
    }
    Ok(headers)
}
impl Transport for Network {
    fn identity(&self) -> u64 {
        self.identity
    }
    fn request(&self, request: http::Request) -> BoxFuture<'_, Result<http::Response, Error>> {
        Box::pin(async move {
            let method = match request.method {
                http::Method::Get => reqwest::Method::GET,
                http::Method::Head => reqwest::Method::HEAD,
                http::Method::Post => reqwest::Method::POST,
                http::Method::Put => reqwest::Method::PUT,
                http::Method::Patch => reqwest::Method::PATCH,
                http::Method::Delete => reqwest::Method::DELETE,
                http::Method::Options => reqwest::Method::OPTIONS,
            };
            let request_headers = headers(request.headers)?;
            let url = reqwest::Url::parse(&request.url)
                .map_err(|_| Error::Adapter("invalid model HTTP URL".into()))?;
            self.policy
                .check_request_headers(&url, &request_headers, !request.body.is_empty(), true)
                .map_err(|_| Error::Adapter("model HTTP headers cannot be represented".into()))?;
            let response = self
                .client
                .request(method, request.url)
                .headers(request_headers)
                .body(request.body)
                .send()
                .await
                .map_err(failure)?;
            let head = http::Head {
                status: response.status().as_u16(),
                url: response.url().to_string(),
                headers: response
                    .headers()
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec()))
                    .collect(),
            };
            Ok(http::Response {
                head,
                body: Arc::new(Body {
                    response: AsyncMutex::new(Some(response)),
                    cancelled: CancellationToken::new(),
                }),
            })
        })
    }
    fn connect(&self, request: Connect) -> BoxFuture<'_, Result<Arc<dyn Socket>, Error>> {
        Box::pin(async move {
            let mut url = reqwest::Url::parse(&request.url)
                .map_err(|_| Error::Adapter("invalid model WebSocket URL".into()))?;
            let scheme = match url.scheme() {
                "ws" | "http" => "http",
                "wss" | "https" => "https",
                _ => return Err(Error::Adapter("invalid model WebSocket scheme".into())),
            };
            url.set_scheme(scheme)
                .map_err(|_| Error::Adapter("invalid model WebSocket scheme".into()))?;
            let request_headers = headers(request.headers)?;
            let socket = maka_network::connect_websocket(
                &self.policy,
                self.policy.client_builder(),
                url.as_str(),
                request_headers,
                usize::MAX,
            )
            .await
            .map_err(|_| Error::Adapter("model WebSocket connection failed".into()))?;
            let (write, read) = socket.split();
            Ok(Arc::new(Connection {
                write: AsyncMutex::new(Some(write)),
                read: AsyncMutex::new(Some(read)),
                cancelled: CancellationToken::new(),
            }) as Arc<dyn Socket>)
        })
    }
}
struct Body {
    response: AsyncMutex<Option<reqwest::Response>>,
    cancelled: CancellationToken,
}
impl http::Body for Body {
    fn next(&self) -> BoxFuture<'_, Result<Option<Vec<u8>>, http::Error>> {
        Box::pin(async move {
            let mut response = self.response.lock().await;
            let Some(body) = response.as_mut() else {
                return Ok(None);
            };
            let result = tokio::select! {
                biased;
                _ = self.cancelled.cancelled() => return Err(http::Error::Denied),
                result = body.chunk() => result,
            };
            match result {
                Ok(Some(bytes)) => Ok(Some(bytes.to_vec())),
                Ok(None) => {
                    response.take();
                    Ok(None)
                }
                Err(error) => {
                    response.take();
                    Err(match failure(error) {
                        Error::Provider(_) => {
                            http::Error::Failed("model HTTP stream interrupted".into())
                        }
                        _ => http::Error::Invalid("invalid model HTTP stream".into()),
                    })
                }
            }
        })
    }
    fn cancel(&self) {
        self.cancelled.cancel();
    }
    fn close(&self) -> BoxFuture<'_, Result<(), http::Error>> {
        self.cancel();
        Box::pin(async move {
            self.response.lock().await.take();
            Ok(())
        })
    }
}
type Wire = tokio_tungstenite::tungstenite::Message;
struct Connection {
    write: AsyncMutex<Option<futures_util::stream::SplitSink<maka_network::Socket, Wire>>>,
    read: AsyncMutex<Option<futures_util::stream::SplitStream<maka_network::Socket>>>,
    cancelled: CancellationToken,
}
impl Socket for Connection {
    fn send(&self, frame: Frame) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(async move {
            let frame = match frame {
                Frame::Text(text) => Wire::Text(text.into()),
                Frame::Binary(bytes) => Wire::Binary(bytes.into()),
            };
            let mut write = self.write.lock().await;
            let write = write.as_mut().ok_or(Error::Cancelled)?;
            tokio::select! {
                biased;
                _ = self.cancelled.cancelled() => Err(Error::Cancelled),
                result = write.send(frame) => result.map_err(|cause| websocket_failure("send", cause)),
            }
        })
    }
    fn receive(&self) -> BoxFuture<'_, Result<Option<Frame>, Error>> {
        Box::pin(async move {
            let mut read = self.read.lock().await;
            let Some(read) = read.as_mut() else {
                return Ok(None);
            };
            loop {
                let next = tokio::select! {
                    biased;
                    _ = self.cancelled.cancelled() => return Err(Error::Cancelled),
                    next = read.next() => next,
                };
                match next {
                    Some(Ok(Wire::Text(text))) => return Ok(Some(Frame::Text(text.to_string()))),
                    Some(Ok(Wire::Binary(bytes))) => {
                        return Ok(Some(Frame::Binary(bytes.to_vec())));
                    }
                    Some(Ok(Wire::Ping(_))) => {
                        if let Some(write) = self.write.lock().await.as_mut() {
                            tokio::select! {
                                biased;
                                _ = self.cancelled.cancelled() => return Err(Error::Cancelled),
                                result = write.flush() => result.map_err(|cause| websocket_failure("pong", cause))?,
                            }
                        }
                    }
                    Some(Ok(Wire::Pong(_))) => {}
                    Some(Ok(Wire::Close(close))) => {
                        let interrupted = close.as_ref().is_none_or(|close| {
                            matches!(u16::from(close.code), 1000 | 1001 | 1009 | 1011..=1014)
                        });
                        let detail = close.map_or_else(
                            || "without a close status".into(),
                            |close| {
                                format!("with code {}: {}", u16::from(close.code), close.reason)
                            },
                        );
                        let message = format!("model WebSocket closed {detail}");
                        if !interrupted {
                            return Err(Error::Adapter(message));
                        }
                        return Err(Error::Provider(ProviderFailure::new(
                            ProviderFailureReason::StreamTruncated,
                            message,
                            false, // The protocol decoder, not socket I/O, decides replay safety.
                            None,
                        )));
                    }
                    None => return Ok(None),
                    Some(Err(cause)) => return Err(websocket_failure("receive", cause)),
                    Some(Ok(_)) => {
                        return Err(Error::Adapter(
                            "model WebSocket returned an unsupported frame".into(),
                        ));
                    }
                }
            }
        })
    }
    fn close(&self) -> BoxFuture<'_, Result<(), Error>> {
        self.cancelled.cancel();
        Box::pin(async move {
            self.write.lock().await.take();
            self.read.lock().await.take();
            Ok(())
        })
    }
}
fn websocket_failure(operation: &str, cause: tokio_tungstenite::tungstenite::Error) -> Error {
    use tokio_tungstenite::tungstenite::{Error as WsError, error::ProtocolError};
    let transient = match &cause {
        WsError::ConnectionClosed
        | WsError::AlreadyClosed
        | WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake) => true,
        WsError::Io(cause) => transient_io(cause.kind()),
        _ => false,
    };
    let message = format!("model WebSocket {operation} failed: {cause}");
    if transient {
        Error::Provider(ProviderFailure::new(
            ProviderFailureReason::Network,
            message,
            false,
            None,
        ))
    } else {
        Error::Adapter(message)
    }
}

fn transient_io(kind: std::io::ErrorKind) -> bool {
    use std::io::ErrorKind;
    matches!(
        kind,
        ErrorKind::ConnectionRefused
            | ErrorKind::UnexpectedEof
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::NotConnected
            | ErrorKind::BrokenPipe
            | ErrorKind::TimedOut
            | ErrorKind::HostUnreachable
            | ErrorKind::NetworkUnreachable
            | ErrorKind::NetworkDown
    )
}

pub(super) fn failure(error: reqwest::Error) -> Error {
    use std::error::Error as _;
    let mut transient = error.is_timeout() || error.is_dns();
    let mut cause = error.source();
    while let Some(error) = cause {
        if let Some(error) = error.downcast_ref::<hyper::Error>() {
            transient |= error.is_closed() || error.is_incomplete_message();
        }
        if let Some(error) = error.downcast_ref::<std::io::Error>() {
            transient |= transient_io(error.kind());
        }
        cause = error.source();
    }
    if transient {
        Error::Provider(ProviderFailure::new(
            ProviderFailureReason::Network,
            "model HTTP transport interrupted",
            true,
            None,
        ))
    } else {
        Error::Adapter("model HTTP request failed".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unrepresentable_model_headers_fail_without_panicking() {
        let values = (0..32_768)
            .map(|index| (format!("x-field-{index}"), "value".into()))
            .collect();
        let result = headers(values).and_then(|headers| {
            maka_network::Policy::default()
                .check_request_headers(
                    &reqwest::Url::parse("https://example.com/").unwrap(),
                    &headers,
                    true,
                    true,
                )
                .map_err(|_| Error::Adapter("model HTTP headers cannot be represented".into()))?;
            Ok(headers)
        });
        assert!(
            matches!(result, Err(Error::Adapter(message)) if message == "model HTTP headers cannot be represented")
        );
        let values = headers(vec![("x-safe".into(), "kept".into())]).unwrap();
        assert_eq!(values["x-safe"], "kept");
    }

    #[test]
    fn socket_write_failures_preserve_transient_network_evidence() {
        use tokio_tungstenite::tungstenite::{Error as WsError, error::ProtocolError};
        let error = websocket_failure(
            "send",
            WsError::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
        );
        let Error::Provider(failure) = error else {
            panic!("broken pipe must remain a network failure")
        };
        assert_eq!(failure.reason(), ProviderFailureReason::Network);
        assert!(
            failure.message().contains("send failed")
                && failure
                    .message()
                    .to_ascii_lowercase()
                    .contains("broken pipe")
        );
        assert!(
            !failure.replay_safe(),
            "only the protocol decoder can authorize replay"
        );
        assert!(
            matches!(
                websocket_failure("send", WsError::Protocol(ProtocolError::SendAfterClosing)),
                Error::Adapter(_)
            ),
            "protocol misuse is not a retryable network outage"
        );
    }
}
