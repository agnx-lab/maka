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
use nsh_platform_tui::AppView;
use serde_json::{Value, json};
#[cfg(unix)]
use std::time::Instant;
use std::{sync::atomic::Ordering, time::Duration};
use support::{EDITOR, Fixture, ReplyGate, evidence, wait_view};
use uuid::Uuid;

fn shortcut(view: &mut AppView, key: char) {
    view.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Char(key),
        KeyModifiers::CONTROL,
    )));
}

async fn select_palette(view: &mut AppView, placeholder: &str, search: &str, choice: &str) {
    wait_view(view, |screen| {
        screen.contains(placeholder) && screen.contains("Enter selects · Esc closes")
    })
    .await;
    view.handle_event(Event::Paste(search.into()));
    wait_view(view, |screen| {
        screen.contains(choice)
            && screen.lines().any(|line| line.trim() == search)
            && screen.contains("Enter selects · Esc closes")
            && !screen.contains("Loading…")
    })
    .await;
    view.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_home_palette_creates_switches_models_and_stops_a_real_session_without_mixing_drafts() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::open().await;
        let client = fixture.client().await;
        let selected = support::mutate(&client, "selectBackend", json!({
            "sessionId":fixture.session,"backend":{"kind":"model","connectionId":fixture.selection["connectionId"],"modelId":"fixture-model","thinkingLevel":null}
        })).await;
        assert_eq!(selected["kind"], "committed");
        let (mut view, bindings) = fixture.view(client.clone(), None);
        let home = wait_view(&mut view, |screen| screen.contains("Make something remarkable.")).await;
        evidence("workbench-home", &home);
        support::evidence_cells("workbench-home", &view);

        shortcut(&mut view, 'p');
        select_palette(&mut view, "Search commands…", "Sess", "Sessions").await;
        select_palette(&mut view, "Search sessions…", "NSH real", "NSH real Agent").await;
        wait_view(&mut view, |screen| screen.contains("NSH real Agent") && screen.contains("Enter sends")).await;
        view.handle_event(Event::Paste("Draft A stays in its own conversation".into()));
        support::wait_draft(&client, &fixture.session, "Draft A stays in its own conversation").await;

        shortcut(&mut view, 'p');
        select_palette(&mut view, "Search commands…", "New con", "New conversation").await;
        wait_view(&mut view, |screen| screen.contains("Models") && screen.contains("Enter selects")).await;
        let catalog = client.query("agent", "list", json!({"filter":"all","position":{"kind":"start"}})).await.unwrap();
        let sessions = catalog["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 2);
        let created = sessions.iter().find(|session| session["id"] != fixture.session).unwrap();
        let created_id = created["id"].as_str().unwrap().to_owned();
        assert_eq!(created["workspace"]["path"], fixture.directory.path().join("workspace").to_str().unwrap());
        wait_view(&mut view, |screen| screen.contains("Search models…")).await;
        view.handle_event(Event::Paste("model-alt".into()));
        wait_view(&mut view, |screen| screen.contains("fixture-model-alt") && screen.contains("1 model") && !screen.contains("Loading…")).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.contains("New conversation") && screen.contains("fixture-model-alt") && screen.contains("Enter sends")).await;
        let observed = client.query("agent", "observe", json!({"sessionId":created_id})).await.unwrap();
        assert_eq!(observed["session"]["backend"], json!({"kind":"model","connectionId":fixture.selection["connectionId"],"modelId":"fixture-model-alt","thinkingLevel":null}));
        view.handle_event(Event::Paste("Message B uses the selected model".into()));
        support::wait_draft(&client, &created_id, "Message B uses the selected model").await;

        shortcut(&mut view, 'o');
        select_palette(&mut view, "Search sessions…", "NSH real", "NSH real Agent").await;
        let restored = wait_view(&mut view, |screen| screen.contains("NSH real Agent") && screen.contains("Draft A stays in its own conversation") && screen.contains("Enter sends")).await;
        assert!(!restored.contains("Message B uses the selected model"));
        evidence("workbench-draft-after-switch", &restored);
        support::evidence_cells("workbench-draft-after-switch", &view);
        shortcut(&mut view, 'o');
        select_palette(&mut view, "Search sessions…", &created_id, "New conversation").await;
        wait_view(&mut view, |screen| screen.contains("New conversation") && screen.contains("Message B uses the selected model") && screen.contains("Enter sends")).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        let mut request = fixture.next_request().await;
        assert_eq!(request.body["model"], "fixture-model-alt");
        assert!(request.body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "user" && message["content"].to_string().contains("Message B uses the selected model")));
        assert!(!request.body.to_string().contains("Draft A stays in its own conversation"));
        request.delta("REAL_WORKBENCH_RUN_TO_STOP").await;
        wait_view(&mut view, |screen| screen.contains("REAL_WORKBENCH_RUN_TO_STOP")).await;
        support::evidence_cells("workbench-streaming", &view);
        let observed = client.query("agent", "observe", json!({"sessionId":created_id})).await.unwrap();
        let invocation = observed["run"]["invocationId"].as_str().filter(|id| !id.is_empty()).unwrap();
        shortcut(&mut view, 'p');
        select_palette(&mut view, "Search commands…", "Sto", "Stop").await;
        // The SSE is still held. Only the UI's exact-run Stop may cancel it.
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let execution = client.query("agent", "execution", json!({"sessionId":created_id,"invocationId":invocation})).await.unwrap();
                if execution["execution"]["state"] == "cancelled" {
                    assert_eq!(execution["kind"], "found");
                    assert_eq!(execution["execution"]["invocationId"], invocation);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("the UI did not cancel its original execution");
        drop(request);
        fixture.idle(&client, &created_id).await;
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
        assert_eq!(client.load_draft("agent", &fixture.session, EDITOR).await.unwrap().unwrap().text, "Draft A stays in its own conversation");
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("real Host workbench navigation timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_agent_history_and_live_stream_render_and_the_app_sends_once() {
    tokio::time::timeout(Duration::from_secs(120), async {
        let mut fixture = Fixture::open().await;
        fixture.seed_history(16).await;
        let client = fixture.client().await;
        let observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let tail = client.query("agent", "history", json!({"sessionId":fixture.session,"through":observed["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        assert!(!tail["before"].is_null(), "the original history must exceed one page");
        let (mut view, bindings) = fixture.view(client.clone(), Some(fixture.session.clone()));
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
        fixture.idle(&client, &fixture.session).await;
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
        fixture.idle(&restarted, &fixture.session).await;
        assert_eq!(restarted.operation(id).await.unwrap().reference, original.reference);
        assert!(restarted.operation(id).await.unwrap().receipt.is_none(), "only the new UI's original-ref recovery may settle the journal");
        let (mut view, bindings) = fixture.view(restarted.clone(), Some(fixture.session.clone()));
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
        let mut terminal = support::pty::Pty::spawn(&fixture.directory.path().join("host"), None);
        let home = terminal.wait_for("Make something remarkable.").await;
        evidence("production-pty-home", &home);
        terminal.send(b"\x10");
        terminal.wait_for("Search commands…").await;
        terminal.paste("Sess");
        terminal.send(b"\r");
        terminal.wait_for("Search sessions…").await;
        terminal.paste("NSH real");
        terminal.wait_for("› NSH real Agent").await;
        terminal.send(b"\r");
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
        fixture.idle(&control, &fixture.session).await;
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
