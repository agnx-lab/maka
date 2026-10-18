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
use serde_json::Value;

#[test]
#[ignore = "real default product, stopped PTY stdout and original model input"]
fn default_product_input_continues_while_the_original_stdout_owner_is_blocked() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 2, 0);
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
    tui.wait_until(|s| s.contains("Message…") && !s.contains("Restoring draft…"));
    click(&mut tui, "Message…");
    paste(&mut tui, "Prepare the original output baseline");
    tui.wait_for("Prepare the original output baseline");
    tui.send(b"\r");
    let (_, mut socket) = next_request(&mut tui, &received);
    delta(&mut socket, "The original model request completed.");
    complete(&mut socket);
    tui.wait_until(|s| {
        s.contains("Send")
            && !s.contains("Queue for next turn")
            && s.contains("The original model request completed.")
    });
    click(&mut tui, "Message…");
    let paused = tui.pause_output();
    // The large real resize produces a full ANSI frame. POLLOUT becoming false
    // and TIOCOUTQ remaining nonzero prove write-side congestion, independently
    // of the terminal emulator's display pause.
    tui.resize(240, 100);
    let queued = paused.wait_until_blocked();
    assert!(tui.child.try_wait().unwrap().is_none());
    paste(&mut tui, "Accepted while stdout is blocked 🙂");
    tui.send(b"\r");
    let ModelRequest {
        value: sent,
        mut socket,
        ..
    } = received
        .recv_timeout(Duration::from_secs(30))
        .expect("original input must reach the model before TCOON");
    assert!(
        sent.to_string()
            .contains("Accepted while stdout is blocked 🙂")
    );
    assert!(!is_title(&sent));
    eprintln!(
        "original PTY stdout backpressure: queued={queued}; original model request received before TCOON"
    );
    delta(
        &mut socket,
        "Input progressed during real output congestion.",
    );
    complete(&mut socket);
    drop(paused);
    tui.wait_until(|s| {
        s.contains("Input progressed during real output congestion.")
            && s.contains("Send")
            && !s.contains("Queue for next turn")
    });
    evidence(&tui, "stdout-congestion-resumed");
    // Resolve coordinates from a completed resumed frame, then verify that the
    // same native editor is usable after output/geometry promotion.
    click(&mut tui, "Message…");
    paste(&mut tui, "Native input after original output completion");
    tui.wait_for("Native input after original output completion");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
    assert!(!tui.screen.snapshot().unwrap().alternate_screen);
    drop(RootOwner::open(&host, &namespaces).unwrap());
}

fn large_history_fixture(model_base: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
    use maka_agent_facts::import::{Content, Record, Source};
    use maka_event_log::{EventLog, sessions::PluginSession};
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let ui = directory.path().join("ui");
    let host = ui.join("host");
    let host_guard = HostRootGuard::new(&host);
    let namespaces = RootNamespaces::for_current_account().unwrap();
    drop(RootOwner::create(&ui, &namespaces).unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    if let Some(base) = model_base {
        runtime.block_on(seed_connection(&host, &namespaces, base, json!({})));
    }
    let args = [
        "application",
        "launch",
        "--root",
        ui.to_str().unwrap(),
        "--workspace",
        workspace.to_str().unwrap(),
        "--trust-workspace",
        "--locale",
        "en",
    ];
    let mut bootstrap = Pty::spawn(&args);
    bootstrap.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    bootstrap.send(b"\x11");
    // First restore the terminal, then explicitly retire the business Host.
    let closing = Instant::now();
    while bootstrap.child.try_wait().unwrap().is_none() {
        bootstrap.read();
        assert!(
            closing.elapsed() < Duration::from_secs(15),
            "bootstrap did not exit"
        );
    }
    bootstrap.finish();
    host_guard.stop();
    runtime.block_on(async {
        let owner = Arc::new(RootOwner::open(&host, &namespaces).unwrap());
        let (workspace, observed) = maka_fs_tools::directory::capture(&workspace).unwrap();
        let directory = maka_fs_tools::directory::open(&workspace, &observed).unwrap();
        let identity = maka_fs_tools::workspace::ensure_directory_identity(
            workspace.clone(), directory, owner.canonical_path().to_owned(),
            owner.control_directory().to_owned(), None,
        ).await.unwrap();
        let published = maka_fs_tools::workspace::directory::PublishedDirectory::open(&workspace).unwrap();
        let log = EventLog::for_root(owner).await.unwrap();
        use sqlx::Connection;
        let mut db = sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new().filename(host.join(maka_event_log::root::ROOT_DATABASE)).read_only(true)).await.unwrap();
        let deployments: Vec<String> = sqlx::query_scalar("SELECT id FROM application_deployments").fetch_all(&mut db).await.unwrap();
        db.close().await.unwrap();
        let mut prefix = None;
        for deployment in deployments {
            let deployment = Uuid::parse_str(&deployment).unwrap();
            let source = log.read_application_metadata(deployment).await.unwrap().unwrap();
            if let Some(agent) = source.source.input.instances.get("agent") {
                let head = log.application_head(deployment).await.unwrap().unwrap();
                prefix = Some(maka_host_runtime::storage::instance_prefix(&head.state_domain, deployment, &maka_plugin_core::composition::InstanceId(vec![agent.key])));
            }
        }
        let session = format!("{}{}", prefix.expect("default Agent namespace"), Uuid::new_v4().simple());
        let mut records = Vec::new();
        for message in 0..32 {
            for (suffix, content) in [
                ("user", Content::User { text: format!("Question {message:02}") }),
                ("assistant", Content::Assistant {
                    text: (0..30).map(|row| format!("Scroll row {message:02}-{row:02}: a paragraph with **bold** and `code` in a long conversation.\n\n")).collect(),
                    model: None, thinking: None,
                }),
            ] {
                records.push(Record {
                    source_message_id: format!("{message}-{suffix}"),
                    source_turn_id: format!("turn-{message}"),
                    timestamp: Some(1), content,
                });
            }
        }
        log.import_session(
            &PluginSession {
                session_id: session,
                creator: maka_capabilities::storage::Namespace::new(maka_agent_api::IMPLEMENTATION, maka_plugin_core::scope::Scope::Profile).unwrap(),
                fingerprint: "scroll-fixture".into(), managed: false, authority_session_id: None,
            },
            &Source { adapter: "fixture".into(), session_id: "scroll-source".into() },
            &json!({"title":"Scroll fixture","titleIsManual":true,"labels":[],
                "workspace":{"path":published.path(),"directoryIdentity":published.identity().unwrap(),"identity":identity},
                "permissions":{"access":"workspace_write","requestPermissions":true},"boundary_revision":0}),
            records, 1,
        ).await.unwrap();
        log.close().await.unwrap();
    });
    (directory, ui)
}

#[test]
#[ignore = "manual default product history scroll profiling in a real PTY"]
fn default_product_scrolls_large_imported_history() {
    let (_directory, ui) = large_history_fixture(None);
    let host_guard = HostRootGuard::new(&ui.join("host"));
    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    eprintln!("scroll fixture PID {}", tui.child.id());
    tui.wait_until_with_timeout(|s| s.contains("Scroll fixture"), Duration::from_secs(60));
    click(&mut tui, "Scroll fixture");
    tui.wait_until_with_timeout(|s| s.contains("Scroll row 31-29"), Duration::from_secs(60));
    let ready = Instant::now();
    while ready.elapsed() < Duration::from_millis(500) {
        tui.read();
    }
    let first_row = |screen: &str| {
        screen.lines().enumerate().find_map(|(index, line)| {
            let at = line.find("Scroll row ")?;
            Some((index, line[at..at + 16].to_owned()))
        })
    };
    let rounds = std::env::var("MAKA_SCROLL_PROFILE_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    // Start away from the tail: showing/hiding the "Scroll to end" control
    // changes viewport height and is not an extra wheel movement.
    for _ in 0..20 {
        let before = first_row(&tui.screen.snapshot().unwrap().screen).unwrap();
        tui.send(b"\x1b[<64;70;10M");
        tui.wait_until(|screen| first_row(screen).is_some_and(|row| row != before));
    }
    let ready = Instant::now();
    while ready.elapsed() < Duration::from_millis(500) {
        tui.read();
    }
    let baseline = first_row(&tui.screen.snapshot().unwrap().screen).unwrap();
    eprintln!("scroll baseline: {baseline:?}");
    for _ in 0..rounds {
        let before = first_row(&tui.screen.snapshot().unwrap().screen).unwrap();
        let started = Instant::now();
        tui.send(b"\x1b[<64;70;10M");
        tui.wait_until(|screen| first_row(screen).is_some_and(|row| row != before));
        eprintln!("history wheel to visible row: {:?}", started.elapsed());
    }
    let ready = Instant::now();
    while ready.elapsed() < Duration::from_millis(500) {
        tui.read();
    }
    let scrolled = first_row(&tui.screen.snapshot().unwrap().screen).unwrap();
    eprintln!("scroll target: {scrolled:?}");
    for (event, expected) in [
        (b"\x1b[<65;70;10M", baseline),
        (b"\x1b[<64;70;10M", scrolled),
    ] {
        // Keep consuming output while input arrives independently at 100 Hz,
        // so the fixture itself cannot cause PTY output backpressure.
        let mut input = tui.master.as_ref().unwrap().try_clone().unwrap();
        let last_input = std::thread::scope(|scope| {
            let sender = scope.spawn(move || {
                use std::io::Write;
                let mut last = Instant::now();
                for _ in 0..rounds {
                    std::thread::sleep(Duration::from_millis(10));
                    input.write_all(event).unwrap();
                    last = Instant::now();
                }
                last
            });
            tui.wait_until(|screen| first_row(screen).as_ref() == Some(&expected));
            sender.join().unwrap()
        });
        let latency = last_input.elapsed();
        eprintln!("history 100 Hz wheel burst stop to final row: {latency:?}");
        assert!(
            latency < Duration::from_millis(150),
            "wheel backlog after stopping: {latency:?}"
        );
        let settled = Instant::now();
        while settled.elapsed() < Duration::from_millis(250) {
            tui.read();
            if tui.frames.ready() {
                assert_eq!(
                    first_row(&tui.screen.snapshot().unwrap().screen).as_ref(),
                    Some(&expected)
                );
            }
        }
        assert_eq!(
            first_row(&tui.screen.snapshot().unwrap().screen).as_ref(),
            Some(&expected)
        );
    }
    evidence(&tui, "scroll-profile");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
}

#[test]
#[ignore = "real default product sends over large imported history"]
fn default_product_sends_over_large_imported_history() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let rounds = std::env::var("MAKA_SEND_PROFILE_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2);
    let server = model_server(listener, requests, rounds, 0);
    let (_directory, ui) = large_history_fixture(Some(&base));
    let host_guard = HostRootGuard::new(&ui.join("host"));
    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    eprintln!("send fixture PID {}", tui.child.id());
    tui.wait_until_with_timeout(|s| s.contains("Scroll fixture"), Duration::from_secs(60));
    let opening = Instant::now();
    click(&mut tui, "Scroll fixture");
    tui.wait_until_with_timeout(
        |s| {
            s.contains("Scroll row 31-29")
                && s.contains("Message…")
                && s.contains("fixture-model")
                && !s.contains("Restoring draft…")
        },
        Duration::from_secs(60),
    );
    eprintln!(
        "large history open to composer ready: {:?}",
        opening.elapsed()
    );
    for turn in 0..rounds {
        let prompt = format!("Persistent history send {turn}");
        click(&mut tui, "Message…");
        paste(&mut tui, &prompt);
        tui.wait_for(&prompt);
        let started = Instant::now();
        eprintln!("send profiling turn {turn} PID {}", tui.child.id());
        tui.send(b"\r");
        let transcript = format!("❯ {prompt}");
        let mut request = None;
        let mut visible_at = None;
        while request.is_none() || visible_at.is_none() {
            tui.read();
            if request.is_none() {
                request = received.try_recv().ok();
            }
            if visible_at.is_none()
                && tui.frames.ready()
                && tui.screen.snapshot().unwrap().screen.contains(&transcript)
            {
                visible_at = Some(Instant::now());
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "send did not reach model and transcript"
            );
            assert!(
                tui.child.try_wait().unwrap().is_none(),
                "UI exited during send"
            );
        }
        let ModelRequest {
            value: sent,
            mut socket,
            received_at,
        } = request.unwrap();
        assert!(sent.to_string().contains(&prompt));
        eprintln!(
            "large history Enter to model request: {:?}",
            received_at.duration_since(started)
        );
        eprintln!(
            "large history Enter to visible message: {:?}",
            visible_at.unwrap().duration_since(started)
        );
        let observed = Instant::now();
        while turn < 2 && observed.elapsed() < Duration::from_secs(6) {
            tui.read();
            if tui.frames.ready() {
                assert!(
                    tui.screen.snapshot().unwrap().screen.contains(&transcript),
                    "submitted message disappeared"
                );
            }
            assert!(
                tui.child.try_wait().unwrap().is_none(),
                "UI exited while awaiting model response"
            );
        }
        let reply = format!("Completed history send {turn}");
        delta(&mut socket, &reply);
        complete(&mut socket);
        tui.wait_until(|s| {
            s.contains(&reply)
                && s.contains("Message…")
                && s.contains("Send")
                && !s.contains("Queue for next turn")
        });
        assert!(tui.screen.snapshot().unwrap().screen.contains(&transcript));
    }
    evidence(&tui, "large-history-send");
    let draft = "Unsent draft after history benchmark";
    click(&mut tui, "Message…");
    paste(&mut tui, draft);
    tui.wait_for(draft);
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
    for reopen in 0..2 {
        let mut restored = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
        eprintln!("draft restore fixture PID {}", restored.child.id());
        restored.wait_until_with_timeout(|s| s.contains("Scroll fixture"), Duration::from_secs(60));
        let opening = Instant::now();
        click(&mut restored, "Scroll fixture");
        restored.wait_until_with_timeout(
            |s| s.contains(draft) && !s.contains("Restoring draft…"),
            Duration::from_secs(60),
        );
        eprintln!(
            "large history open to saved draft ready ({rounds} sends, reopen {reopen}): {:?}",
            opening.elapsed()
        );
        evidence(&restored, "large-history-draft-restored");
        restored.send(b"\x11");
        restored.finish();
        host_guard.stop();
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            use sqlx::{Connection, Row};
            let mut db = sqlx::SqliteConnection::connect_with(&sqlx::sqlite::SqliteConnectOptions::new()
                .filename(ui.join(maka_event_log::root::ROOT_DATABASE)).read_only(true)).await.unwrap();
            let saved = sqlx::query("SELECT metadata_json, content FROM application_ui_checkpoints WHERE format='maka.ui.editor@1/maka.agent.draft@2' AND length(content)>0").fetch_all(&mut db).await.unwrap();
            assert_eq!(saved.len(), 1, "exactly one unsent draft remains");
            let saved = &saved[0];
            let metadata: Value = serde_json::from_str(&saved.get::<String, _>("metadata_json")).unwrap();
            let scope: Value = serde_json::from_str(metadata["metadata"]["historyScope"].as_str().unwrap_or_else(|| panic!("saved draft has no checked caller scope: {metadata}"))).unwrap();
            let (count, through): (i64, i64) = sqlx::query_as("SELECT COUNT(*), MAX(cursor) FROM application_intents WHERE deployment=? AND consumer=? AND json_extract(recovery_json, '$.format')='maka.agent.draft-send@2'")
                .bind(scope[1].as_str().unwrap()).bind(serde_json::to_string(&scope[2]).unwrap())
                .fetch_one(&mut db).await.unwrap();
            assert_eq!(count, rounds as i64);
            assert_eq!(metadata["metadata"]["historyAfter"], through);
            assert_eq!(saved.get::<Vec<u8>, _>("content"), draft.as_bytes());
            db.close().await.unwrap();
        });
    }
}
