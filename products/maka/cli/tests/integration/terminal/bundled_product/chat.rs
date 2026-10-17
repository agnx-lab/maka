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
use maka_agent_facts::configuration::CatalogMutationResult;
use std::path::Path;
use std::{
    net::{TcpListener, TcpStream},
    sync::{Arc, mpsc},
};
use uuid::Uuid;

mod attachments;
mod code_mode;
mod compaction;
mod computer;
mod followup;
mod goal;
mod graph;
mod health;
mod performance;
mod plan;
mod recap;
mod title;
mod todo;
mod trace;

async fn seed_connection(
    host: &Path,
    namespaces: &RootNamespaces,
    base: &str,
    model_overrides: serde_json::Value,
) -> String {
    let owner = Arc::new(RootOwner::create(host, namespaces).unwrap());
    let config = Arc::new(
        maka_config::ConfigurationStore::for_root(owner)
            .await
            .unwrap(),
    );
    let provider = json!({"packageId":"maka.providers","entryId":"maka.providers","scope":"profile","name":"openai-compatible"});
    let CatalogMutationResult::Committed { connection: Some(basis), .. } = config.create_connection(serde_json::from_value(json!({
            "expectedCatalogRevision":0,"connection":{"slug":"chat-fixture","name":"Chat fixture","provider":provider,"configuration":{"baseUrl":base},"enabled":true,"enabledModelIds":["fixture-model"],"modelOverrides":model_overrides}
        })).unwrap()).await.unwrap() else { panic!("connection commit") };
    let maka_config::oauth::enrollment::LoginPreparation::Ready(ticket) = config.prepare_oauth_login(serde_json::from_value(json!({
            "expectedCredential":null,"attemptId":Uuid::new_v4().to_string(),
            "target":{"kind":"existing","expected":{"connectionId":basis.connection_id,"revision":basis.revision,"provider":provider,"slug":"chat-fixture","configuration":{"baseUrl":base}},"configuration":{"baseUrl":base}},
            "authentication":{"method":"api-key","input":{"apiKey":"chat-fixture-key"}}
        })).unwrap()).await.unwrap() else { panic!("credential preparation") };
    assert!(ticket.claim().await.unwrap());
    assert!(matches!(
        ticket
            .complete(
                maka_agent_facts::provider::Credential {
                    secret: "chat-fixture-key".into(),
                    refresh_at: None
                },
                1
            )
            .await
            .unwrap(),
        maka_config::oauth::enrollment::LoginCompletion::Committed(_)
    ));
    assert!(matches!(config.set_default_target(serde_json::from_value(json!({"expectedCatalogRevision":config.catalog().await.unwrap().revision,"target":{"connectionId":basis.connection_id,"modelId":"fixture-model"}})).unwrap()).await.unwrap(),CatalogMutationResult::Committed{..}));
    config.shutdown().await.unwrap();
    basis.connection_id
}

fn request(socket: &mut TcpStream) -> serde_json::Value {
    socket
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let mut bytes = Vec::new();
    let boundary = loop {
        let mut buffer = [0; 4096];
        let count = socket.read(&mut buffer).unwrap();
        assert_ne!(count, 0);
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 512 * 1024);
        if let Some(at) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break at + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..boundary]);
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("authorization: bearer chat-fixture-key")
    );
    let length: usize = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() < boundary + length {
        let mut buffer = [0; 4096];
        let count = socket.read(&mut buffer).unwrap();
        assert_ne!(count, 0);
        bytes.extend_from_slice(&buffer[..count]);
    }
    serde_json::from_slice(&bytes[boundary..boundary + length]).unwrap()
}
fn is_title(value: &serde_json::Value) -> bool {
    value["messages"].as_array().unwrap().iter().any(|message| {
        message["role"] == "system"
            && message["content"]
                .as_str()
                .is_some_and(|text| text.starts_with("Generate a short session title"))
    })
}
struct ModelRequest {
    value: serde_json::Value,
    socket: TcpStream,
    received_at: Instant,
}

fn model_server(
    listener: TcpListener,
    requests: mpsc::Sender<ModelRequest>,
    main_requests: usize,
    title_requests: usize,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let mut titles = 0;
        for _ in 0..main_requests + title_requests {
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            let (mut socket, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "Expected remaining model requests"
                        );
                        let mut poll = libc::pollfd {
                            fd: listener.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        };
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        assert!(
                            unsafe { libc::poll(&mut poll, 1, remaining.as_millis() as i32) } >= 0
                        );
                    }
                    Err(error) => panic!("Model connection: {error}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            let value = request(&mut socket);
            let received_at = Instant::now();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            socket.flush().unwrap();
            if is_title(&value) {
                titles += 1;
                assert!(value["tools"].is_null() || value["tools"] == json!([]));
                delta(&mut socket, "Workspace conversation");
                complete(&mut socket);
            } else {
                requests
                    .send(ModelRequest {
                        value,
                        socket,
                        received_at,
                    })
                    .unwrap();
            }
        }
        assert_eq!(titles, title_requests);
    })
}
fn packet(socket: &mut TcpStream, value: serde_json::Value) {
    let data = format!("data: {value}\n\n");
    write!(socket, "{:x}\r\n{}\r\n", data.len(), data).unwrap();
    socket.flush().unwrap();
}
fn delta(socket: &mut TcpStream, text: &str) {
    packet(
        socket,
        json!({"id":"chat-fixture","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
    );
}
fn complete(socket: &mut TcpStream) {
    packet(
        socket,
        json!({"id":"chat-fixture","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}),
    );
    socket
        .write_all(b"e\r\ndata: [DONE]\n\n\r\n0\r\n\r\n")
        .unwrap();
    socket.flush().unwrap();
}
fn paste(tui: &mut Pty, text: &str) {
    tui.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
}
fn next_request(
    tui: &mut Pty,
    received: &mpsc::Receiver<ModelRequest>,
) -> (serde_json::Value, TcpStream) {
    let mut next = None;
    // Keep consuming the PTY while the model request is admitted. A real
    // terminal continuously reads stdout; blocking on the server channel can
    // fill its output buffer and prevent the next UI input from being handled.
    tui.wait_until_with_timeout(
        |_| {
            next = received.try_recv().ok();
            next.is_some()
        },
        Duration::from_secs(30),
    );
    let request = next.unwrap();
    (request.value, request.socket)
}
fn evidence(tui: &Pty, name: &str) {
    if let Ok(path) = std::env::var("MAKA_CHAT_UX_EVIDENCE") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            Path::new(&path).join(format!("{name}.txt")),
            tui.screen.snapshot().unwrap().screen,
        )
        .unwrap();
        std::fs::write(Path::new(&path).join(format!("{name}.ansi")), &tui.output).unwrap();
        std::fs::write(Path::new(&path).join("terminal.ansi"), &tui.output).unwrap();
    }
}
fn first_paragraph(screen: &str) -> Option<(usize, &str)> {
    screen.lines().enumerate().find_map(|(row, line)| {
        let start = line.find("Paragraph ")?;
        let end = line[start..].find("stable.")? + start + "stable.".len();
        Some((row, &line[start..end]))
    })
}

#[test]
#[ignore = "real bundled product, local streamed model and foreground PTY"]
fn bundled_chat_sends_follows_renders_markdown_and_scrolls_original_history() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = std::thread::spawn(move || {
        for _ in 0..9 {
            let (mut socket, _) = listener.accept().unwrap();
            let value = request(&mut socket);
            let received_at = Instant::now();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            socket.flush().unwrap();
            if requests
                .send(ModelRequest {
                    value,
                    socket,
                    received_at,
                })
                .is_err()
            {
                break;
            }
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let host = directory.path().join("host");
    let host_guard = HostRootGuard::new(&host);
    let ui = directory.path().join("ui");
    let configuration = directory.path().join("installation.json");
    let namespaces = RootNamespaces::for_current_account().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(seed_connection(&host, &namespaces, &base, json!({})));
    std::fs::write(&configuration, serde_json::to_vec(&json!({
        "presentation":"tui", "locale":"en", "scheme":"dark", "hostRoot":host,
        "agent":{"workspaceRoots":[{"label":"Workspace","path":workspace}],"tools":[],"permissions":{"access":"read_only","requestPermissions":true}}
    })).unwrap()).unwrap();
    let mut tui = Pty::spawn(&[
        "application",
        "launch",
        "--root",
        ui.to_str().unwrap(),
        "--configuration",
        configuration.to_str().unwrap(),
    ]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"New conversation");
    tui.wait_for("› New conversation");
    tui.send(b"\r");
    tui.wait_for("Message…");
    for turn in 0..8 {
        click(&mut tui, "Message…");
        let prompt = format!("Original prompt {turn:02}");
        paste(&mut tui, &prompt);
        tui.wait_for(&prompt);
        tui.send(b"\r");
        let (request, mut socket) = next_request(&mut tui, &received);
        assert_eq!(request["model"], "fixture-model");
        assert!(request.to_string().contains(&prompt));
        delta(
            &mut socket,
            &format!("# Answer {turn:02}\n\n**Rendered** original reply."),
        );
        tui.wait_for("Rendered");
        if turn == 0 {
            evidence(&tui, "streaming");
            let screen = tui.screen.snapshot().unwrap().screen;
            assert!(
                screen
                    .lines()
                    .any(|line| line.contains(&format!("❯ {prompt}"))),
                "sent message must appear in the transcript during streaming: {screen}"
            );
            assert!(
                !screen.contains("**Rendered**"),
                "live Markdown must render: {screen}"
            );
        }
        complete(&mut socket);
        tui.wait_until(|s| {
            s.contains("Send")
                && !s.contains("Queue for next turn")
                && s.contains(&format!("Answer {turn:02}"))
        });
    }
    tui.wait_until(|s| {
        s.lines()
            .filter(|line| line.contains("❯ Original prompt"))
            .count()
            >= 3
    });
    evidence(&tui, "history-tail");
    // Wheel targets the transcript independently of composer keyboard focus.
    // Message heights and history paging determine how many rows reach the start.
    let history_deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let previous = tui.screen.snapshot().unwrap().screen;
        if previous.contains("Original prompt 00") {
            break;
        }
        assert!(
            Instant::now() < history_deadline,
            "original history start was not reached:\n{previous}"
        );
        tui.send(b"\x1b[<64;70;10M");
        tui.wait_until_with_timeout(
            |screen| screen != previous,
            history_deadline.saturating_duration_since(Instant::now()),
        );
    }
    evidence(&tui, "history-start");
    click(&mut tui, "Message…");
    paste(&mut tui, "Continue from history");
    tui.send(b"\r");
    let (_, mut socket) = next_request(&mut tui, &received);
    delta(&mut socket, "# Continuing\n\n**Live** after history.");
    tui.wait_until(|screen| {
        screen.contains("Live")
            && screen
                .lines()
                .any(|line| line.contains("❯ Continue from history"))
    });
    evidence(&tui, "sent-from-history");
    let long = (0..80)
        .map(|row| {
            format!(
                "\n\nParagraph {row:02}: reading and streaming keep the original paragraph stable."
            )
        })
        .collect::<String>();
    delta(&mut socket, &long);
    tui.wait_for("Paragraph 79");
    for _ in 0..6 {
        let previous = tui.screen.snapshot().unwrap().screen;
        let paragraph = first_paragraph(&previous).unwrap();
        tui.send(b"\x1b[<64;70;10M");
        tui.wait_until(|screen| {
            first_paragraph(screen).is_some_and(|current| current != paragraph)
        });
    }
    tui.wait_until(|s| !s.contains("Paragraph 79"));
    let held = tui.screen.snapshot().unwrap().screen;
    evidence(&tui, "held-before");
    delta(&mut socket, "\n\nNewest streamed paragraph");
    // A queue state change has to reach the UI before checking the held viewport.
    complete(&mut socket);
    tui.wait_until(|s| s.contains("Send") && !s.contains("Queue for next turn"));
    let after = tui.screen.snapshot().unwrap().screen;
    evidence(&tui, "held-after");
    let (held_row, paragraph) = first_paragraph(&held).unwrap();
    assert!(
        after
            .lines()
            .nth(held_row)
            .is_some_and(|line| line.contains(paragraph)),
        "appended output must retain the read paragraph: {after}"
    );
    assert!(!after.contains("Newest streamed paragraph"));
    evidence(&tui, "held-history");
    click(&mut tui, "Latest messages");
    tui.wait_for("Newest streamed paragraph");
    evidence(&tui, "restored-following");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
    assert!(!tui.screen.snapshot().unwrap().alternate_screen);
    drop(RootOwner::open(&host, &namespaces).unwrap());
}

#[test]
#[ignore = "real Agent invocation continues after explicitly closing only its UI"]
fn ui_only_exit_preserves_accepted_agent_invocation_and_reopens_without_resend() {
    use maka_host_runtime::application::host::{Client, discovery};
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    const PROMPT: &str = "Finish this original invocation after the UI closes.";
    const ANSWER: &str = "Original background job finished.";
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 1, 0);
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let host = directory.path().join("host");
    let host_guard = HostRootGuard::new(&host);
    let ui = directory.path().join("ui");
    let configuration = directory.path().join("installation.json");
    let namespaces = RootNamespaces::for_current_account().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(seed_connection(&host, &namespaces, &base, json!({})));
    std::fs::write(
        &configuration,
        serde_json::to_vec(&json!({
            "presentation":"tui", "locale":"en", "scheme":"dark", "hostRoot":host,
            "agent":{
                "workspaceRoots":[{"label":"Workspace","path":workspace}], "tools":[],
                "automaticTitles":false,
                "permissions":{"access":"read_only","requestPermissions":true}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let mut tui = Pty::spawn(&[
        "application",
        "launch",
        "--root",
        ui.to_str().unwrap(),
        "--configuration",
        configuration.to_str().unwrap(),
    ]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"New conversation");
    tui.wait_for("› New conversation");
    tui.send(b"\r");
    tui.wait_for("Message…");
    click(&mut tui, "Message…");
    paste(&mut tui, PROMPT);
    tui.wait_for(PROMPT);
    tui.send(b"\r");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert_eq!(sent["model"], "fixture-model");
    assert!(sent.to_string().contains(PROMPT));
    assert!(!is_title(&sent));
    server.join().unwrap();

    // Receiving this HTTP request proves admission reached the real model
    // executor. Its answer is physically gated until the foreground has exited.
    let (release, gated) = mpsc::sync_channel::<()>(0);
    let responder = std::thread::spawn(move || {
        gated.recv_timeout(Duration::from_secs(60)).unwrap();
        delta(&mut socket, ANSWER);
        complete(&mut socket);
    });
    let original = discovery::read(&host).unwrap();
    let mut sql = runtime
        .block_on(SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(host.join(maka_event_log::root::ROOT_DATABASE))
                .read_only(true),
        ))
        .unwrap();
    let opened: Vec<String> = runtime.block_on(
        sqlx::query_scalar("SELECT invocation_id FROM event_log WHERE kind='invocation_opened' ORDER BY sequence")
            .fetch_all(&mut sql),
    ).unwrap();
    assert_eq!(opened.len(), 1);
    let invocation = &opened[0];
    let ended: i64 = runtime
        .block_on(
            sqlx::query_scalar(
                "SELECT COUNT(*) FROM event_log WHERE invocation_id=? AND kind='invocation_ended'",
            )
            .bind(invocation)
            .fetch_one(&mut sql),
        )
        .unwrap();
    assert_eq!(
        ended, 0,
        "the explicit model gate must still hold the original invocation"
    );
    tui.send(b"\x1b\x11");
    tui.finish();
    assert!(!tui.screen.snapshot().unwrap().alternate_screen);
    drop(RootOwner::open(&ui, &namespaces).unwrap());
    let error = RootOwner::open(&host, &namespaces)
        .err()
        .expect("business Host must remain resident");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    runtime.block_on(async {
        let status = Client::status(&original.endpoint, Some(original.host_epoch))
            .await
            .unwrap();
        original.verify(&status).unwrap();
        assert_eq!(
            status.sessions, 0,
            "foreground detachment must finish before releasing the answer"
        );
    });
    release.send(()).unwrap();
    responder.join().unwrap();

    // Observe durable completion before starting another UI. Reconnection must
    // only read this result, never resume or resend the accepted model request.
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let ended: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM event_log WHERE invocation_id=? AND kind='invocation_ended'")
                    .bind(invocation).fetch_one(&mut sql).await.unwrap();
                if ended == 1 { break; }
                tokio::task::yield_now().await;
            }
        }).await.expect("original invocation did not complete without its UI");
        let completed: Vec<String> = sqlx::query_scalar("SELECT event_json FROM event_log WHERE invocation_id=? AND kind='model_completed'")
            .bind(invocation).fetch_all(&mut sql).await.unwrap();
        assert_eq!(completed.len(), 1);
        assert!(completed[0].contains(ANSWER), "persisted output belongs to the original invocation");
    });
    let mut reopened = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    reopened.wait_until_with_timeout(|screen| screen.contains(ANSWER), Duration::from_secs(45));
    let current = discovery::read(&host).unwrap();
    assert_eq!(current.host_epoch, original.host_epoch);
    assert_eq!(current.source, original.source);
    let after: Vec<String> = runtime.block_on(
        sqlx::query_scalar("SELECT invocation_id FROM event_log WHERE kind='invocation_opened' ORDER BY sequence")
            .fetch_all(&mut sql),
    ).unwrap();
    assert_eq!(
        after, opened,
        "reopening must preserve the original invocation identity"
    );
    let attempts: i64 = runtime
        .block_on(
            sqlx::query_scalar("SELECT COUNT(*) FROM event_log WHERE kind='model_requested'")
                .fetch_one(&mut sql),
        )
        .unwrap();
    assert_eq!(
        attempts, 1,
        "UI reconnection must not resend the model request"
    );
    reopened.send(b"\x11");
    reopened.finish();
    runtime.block_on(sql.close()).unwrap();
    host_guard.stop();
    drop(RootOwner::open(&host, &namespaces).unwrap());
}
