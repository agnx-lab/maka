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

mod support;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use maka_plugin_core::operation::{OperationOutcome, Receipt};
use nsh_engine::Bundle;
use serde_json::{Value, json};
#[cfg(unix)]
use std::time::Instant;
use std::{sync::atomic::Ordering, time::Duration};
use support::{EDITOR, Fixture, ReplyGate, evidence, wait_view};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_agent_history_and_live_stream_render_and_the_app_sends_once() {
    tokio::time::timeout(Duration::from_secs(120), async {
        let mut fixture = Fixture::open().await;
        fixture.seed_history(16).await;
        let client = fixture.client().await;
        let observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let tail = client.query("agent", "history", json!({"sessionId":fixture.session,"through":observed["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        assert!(!tail["before"].is_null(), "the original history must exceed one page");
        let (mut view, bindings) = fixture.view(client.clone());
        let history = wait_view(&mut view, |screen| screen.contains("History 15 paragraph") && screen.contains("Enter sends")).await;
        assert!(history.contains("original provider text"));
        assert!(history.contains("Load earlier messages"));
        evidence("history", &history);

        view.handle_event(Event::Paste("NSH user send, exactly once".into()));
        wait_view(&mut view, |screen| screen.contains("NSH user send, exactly once")).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        // AppView's engine continues independently while this awaits the real HTTP request.
        let mut request = fixture.next_request().await;
        assert!(request.body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "user" && message["content"].to_string().contains("NSH user send, exactly once")));
        request.delta("REAL_PROVIDER_STREAM_VISIBLE").await;
        let streaming = wait_view(&mut view, |screen| screen.contains("REAL_PROVIDER_STREAM_VISIBLE")).await;
        evidence("streaming", &streaming);
        request.delta(" and committed.").await;
        request.finish().await;
        fixture.idle(&client).await;
        let committed = wait_view(&mut view, |screen| screen.contains("REAL_PROVIDER_STREAM_VISIBLE") && screen.contains("committed.") && screen.contains("Enter sends")).await;
        evidence("committed", &committed);
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 17);
        assert!(client.pending_operations("agent", &fixture.session, EDITOR).await.unwrap().is_empty());
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("real Host/Agent UI integration timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lost_confirmation_recovers_its_original_reference_and_reload_keeps_a_later_draft() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::open().await;
        fixture.seed_history(1).await;
        let gate = ReplyGate::start(fixture.directory.path(), fixture.address.clone());
        let client = fixture.client_at(&gate.address).await;
        let first = client.save_draft("agent", &fixture.session, EDITOR, None, "Original accepted message A".into()).await.unwrap();
        let id = Uuid::new_v4();
        let mutation = client.send_draft(id, "agent", &fixture.session, EDITOR, first.revision, json!({"model":fixture.selection})).await.unwrap();
        let original = mutation.prepare().await.unwrap();
        assert!(original.reference.is_some());
        assert_eq!(client.operation(id).await.unwrap().reference, original.reference);

        gate.arm();
        let mut confirmation = Box::pin(mutation.confirm());
        tokio::select! {
            result = &mut confirmation => panic!("the real send receipt escaped its physical gate: {result:?}"),
            () = gate.held.notified() => {},
        }
        // The Agent returned a real receipt, but this observer has not received it.
        // A newer durable editor revision must remain independent of that attempt.
        let later = client.save_draft("agent", &fixture.session, EDITOR, Some(first.revision), "Later unsent draft B survives recovery".into()).await.unwrap();
        assert!(later.revision > first.revision);
        drop(confirmation);
        assert!(mutation.confirm().await.is_err(), "the same attempt cannot confirm twice");
        mutation.disconnect().await;
        gate.release();
        let mut request = fixture.next_request().await;
        assert!(request.body.to_string().contains("Original accepted message A"));
        request.delta("REAL_RECOVERED_PROVIDER_ANSWER").await;
        request.finish().await;
        client.close().await.unwrap();

        let restarted = fixture.client().await;
        fixture.idle(&restarted).await;
        assert_eq!(restarted.operation(id).await.unwrap().reference, original.reference);
        assert!(restarted.operation(id).await.unwrap().receipt.is_none(), "only the new UI's original-ref recovery may settle the journal");
        let (mut view, bindings) = fixture.view(restarted.clone());
        let recovered = wait_view(&mut view, |screen| screen.contains("Later unsent draft B survives recovery") && screen.contains("REAL_RECOVERED_PROVIDER_ANSWER") && screen.contains("Message accepted.")).await;
        evidence("recovered-original-receipt", &recovered);
        let settled = restarted.operation(id).await.unwrap();
        assert_eq!(settled.reference, original.reference);
        assert!(matches!(settled.receipt, Some(Receipt::Recorded {outcome:OperationOutcome::Returned {ref value},..}) if value["kind"] == "started" && value["sessionId"] == fixture.session));
        assert_eq!(restarted.load_draft("agent", &fixture.session, EDITOR).await.unwrap().unwrap().text, later.text);
        assert!(restarted.pending_operations("agent", &fixture.session, EDITOR).await.unwrap().is_empty());
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2, "recovering a receipt must never send A or draft B again");

        view.load(Bundle::source(maka_terminal_shell::APPLICATION.into()).into()).unwrap();
        let reloaded = wait_view(&mut view, |screen| screen.contains("Later unsent draft B survives recovery") && screen.contains("REAL_RECOVERED_PROVIDER_ANSWER") && screen.contains("Enter sends")).await;
        evidence("reloaded-draft", &reloaded);
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2);
        let history = restarted.query("agent", "history", json!({"sessionId":fixture.session,"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        assert_eq!(history["chunks"].as_array().unwrap().iter().filter(|chunk| chunk["role"] == "user" && chunk["text"] == "Original accepted message A").count(), 1);
        assert!(restarted.mutation(id, "agent", "send", Value::Null, Default::default(), None, Value::Null).await.is_err());
        drop(view);
        bindings.close().await.unwrap();
        drop(gate);
        fixture.finish().await;
    }).await.expect("real original-receipt recovery timed out");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_binary_sends_renders_provider_text_and_restores_its_actual_terminal() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::registered().await;
        fixture.seed_history(1).await;
        let gate = ReplyGate::history(fixture.directory.path(), fixture.address.clone());
        gate.arm();
        fixture.register_at(&gate.address);
        let mut terminal =
            support::pty::Pty::spawn(&fixture.directory.path().join("host"), &fixture.session);
        let mut held = Box::pin(gate.held.notified());
        let awaiting = Instant::now();
        loop {
            terminal.read();
            tokio::select! {() = &mut held => break, () = tokio::time::sleep(Duration::from_millis(2)) => {}}
            assert!(awaiting.elapsed() < Duration::from_secs(20), "real first history did not reach its gate");
        }
        drop(held);
        let loading = terminal.wait_for("Loading session history").await;
        assert!(!loading.contains("Enter sends"), "the shell advertised submission before history was ready");
        terminal.paste("Production PTY user message");
        terminal.wait_for("Production PTY user message").await;
        terminal.send(b"\r");
        let blocked = terminal.wait_for("This Enter did not send a message.").await;
        assert!(blocked.contains("Production PTY user message"));
        assert!(blocked.contains("Loading session history"));
        assert!(!blocked.contains("Enter sends"));
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
        evidence("production-pty-loading-keeps-draft", &blocked);
        gate.release();
        terminal.wait_for("Enter sends").await;
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1, "history recovery must not replay the early Enter");
        terminal.send(b"\r");
        // A real terminal keeps reading stdout while waiting for the provider.
        // This also exercises the production LocalEngine presentation acknowledgements.
        let awaiting = Instant::now();
        let mut request = loop {
            terminal.read();
            if let Ok(request) = fixture.requests.try_recv() {
                break request;
            }
            assert!(
                awaiting.elapsed() < Duration::from_secs(20),
                "physical Enter never reached the real provider:\n{}",
                terminal.snapshot()
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        assert!(
            request
                .body
                .to_string()
                .contains("Production PTY user message")
        );
        request.delta("REAL_PRODUCTION_PTY_STREAM").await;
        let streamed = terminal.wait_for("REAL_PRODUCTION_PTY_STREAM").await;
        evidence("production-pty-streaming", &streamed);
        request.delta(" completed.").await;
        request.finish().await;
        let committed = terminal.wait_for("completed.").await;
        evidence("production-pty-committed", &committed);
        let control = fixture.client().await;
        fixture.idle(&control).await;
        control.close().await.unwrap();
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2);
        terminal.send(b"\x11");
        terminal.finish().await;
        drop(terminal);
        drop(gate);
        fixture.finish().await;
    })
    .await
    .expect("production binary PTY verification timed out");
}
