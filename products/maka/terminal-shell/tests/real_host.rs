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

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use maka_plugin_core::operation::{OperationOutcome, Receipt};
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

async fn wait_session_search(view: &mut AppView) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            support::tick(view);
            if view
                .accessibility_snapshot()
                .iter()
                .any(|node| node.focused && node.label == "Search session titles")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Ctrl+O did not focus the native session-title Input");
}

async fn select_palette(view: &mut AppView, placeholder: &str, search: &str, choice: &str) {
    if placeholder == "Search sessions…" {
        wait_session_search(view).await;
    } else {
        wait_view(view, |screen| {
            screen.contains(placeholder) && screen.contains("Enter selects · Esc closes")
        })
        .await;
    }
    view.handle_event(Event::Paste(search.into()));
    assert_eq!(
        view.surface().input_context().unwrap().text.as_deref(),
        Some(search)
    );
    wait_view(view, |screen| {
        screen.contains(&format!("› {choice}"))
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

        support::click_text(&mut view, "MAKA");
        wait_view(&mut view, |screen| screen.contains("Search commands…") && screen.contains("Close · Esc")).await;
        support::click_text(&mut view, "Close · Esc");
        wait_view(&mut view, |screen| screen.contains("Make something remarkable.") && !screen.contains("Search commands…")).await;
        support::click_text(&mut view, "MAKA");
        wait_view(&mut view, |screen| screen.contains("Search commands…") && screen.contains("› Sessions")).await;
        support::click_text(&mut view, "› Sessions");
        wait_view(&mut view, |screen| screen.contains("Search sessions…") && screen.contains("› NSH real Agent") && !screen.contains("Loading…")).await;
        wait_session_search(&mut view).await;
        support::click_text(&mut view, "› NSH real Agent");
        wait_view(&mut view, |screen| screen.contains("NSH real Agent") && screen.contains("Enter sends")).await;
        view.handle_event(Event::Paste("Draft A stays in its own conversation".into()));
        support::wait_draft(&client, &fixture.session, "Draft A stays in its own conversation").await;

        // A historical Ctrl+O request must not replay when an inactive sidebar
        // remounts. The center keeps its native Input and exact saved draft.
        let composer = view.surface().focused().unwrap();
        view.resize(80, 24);
        let narrow = wait_view(&mut view, |screen| screen.contains("Draft A stays in its own conversation") && screen.contains("Enter sends") && !screen.contains("Search sessions…")).await;
        assert_eq!(view.surface().focused(), Some(composer));
        assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some("Draft A stays in its own conversation"));
        evidence("workbench-narrow-preserves-editor", &narrow);
        support::evidence_cells("workbench-narrow-preserves-editor", &view);
        view.resize(120, 40);
        let wide = wait_view(&mut view, |screen| screen.contains("Draft A stays in its own conversation") && screen.contains("Search sessions…") && screen.contains("Hide")).await;
        assert_eq!(view.surface().focused(), Some(composer));
        assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some("Draft A stays in its own conversation"));
        evidence("workbench-wide-preserves-editor", &wide);
        support::evidence_cells("workbench-wide-preserves-editor", &view);

        // Drag the real native divider in both directions without leaving the
        // editor or changing its draft. A viewport resize does not exercise it.
        for delta in [4_i16, -4] {
            let nodes = view.accessibility_snapshot();
            let divider = nodes.iter().find(|node| node.label == "Resize session browser").unwrap().frame;
            let editor = nodes.iter().find(|node| node.id == composer).unwrap().frame;
            let column = (divider.origin.x.0 / 8.0).round() as u16;
            let target = (column as i16 + delta) as u16;
            for (kind, column) in [
                (MouseEventKind::Down(MouseButton::Left), column),
                (MouseEventKind::Drag(MouseButton::Left), target),
                (MouseEventKind::Up(MouseButton::Left), target),
            ] {
                view.handle_event(Event::Mouse(MouseEvent {kind,column,row:1,modifiers:KeyModifiers::NONE}));
            }
            tokio::time::timeout(Duration::from_secs(20), async {
                loop {
                    support::tick(&mut view);
                    if view.accessibility_snapshot().iter().any(|node| node.label == "Resize session browser" && node.frame.origin.x.0 == divider.origin.x.0 + f32::from(delta) * 8.0) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }).await.expect("the physical browser divider drag did not resize the center");
            assert_eq!(view.surface().focused(), Some(composer), "divider drag moved editor focus");
            assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some("Draft A stays in its own conversation"));
            let resized = view.accessibility_snapshot().into_iter().find(|node| node.id == composer).unwrap().frame;
            assert_eq!(resized.origin.x.0, editor.origin.x.0 + f32::from(delta) * 8.0);
            assert_eq!(resized.size.width.0, editor.size.width.0 - f32::from(delta) * 8.0);
        }
        support::wait_draft(&client, &fixture.session, "Draft A stays in its own conversation").await;

        shortcut(&mut view, 'p');
        select_palette(&mut view, "Search commands…", "New con", "New conversation").await;
        wait_view(&mut view, |screen| screen.contains("Models") && screen.contains("Enter selects")).await;
        let catalog = client.query("agent", "list", json!({"filter":"all","position":{"kind":"start"}})).await.unwrap();
        let sessions = catalog["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 2);
        let original = sessions.iter().find(|session| session["id"] == fixture.session).unwrap();
        let created = sessions.iter().find(|session| session["id"] != fixture.session).unwrap();
        let created_id = created["id"].as_str().unwrap().to_owned();
        assert_eq!(created["workspace"], original["workspace"]);
        assert_eq!(created["workspace"]["path"], fixture.directory.path().join("workspace").canonicalize().unwrap().to_str().unwrap());
        wait_view(&mut view, |screen| screen.contains("Search models…")).await;
        view.handle_event(Event::Paste("model-alt".into()));
        wait_view(&mut view, |screen| screen.contains("fixture-model-alt") && screen.contains("1 model") && !screen.contains("Loading…")).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.contains("New conversation") && screen.contains("fixture-model-alt · Ctrl+G") && screen.contains("Enter sends") && !screen.contains("Close · Esc")).await;
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
        select_palette(&mut view, "Search sessions…", "New conversation", "New conversation").await;
        wait_view(&mut view, |screen| screen.contains("New conversation") && screen.contains("Message B uses the selected model") && screen.contains("Enter sends")).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        let mut request = fixture.next_request().await;
        assert_eq!(request.body["model"], "fixture-model-alt");
        assert!(request.body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "user" && message["content"].to_string().contains("Message B uses the selected model")));
        assert!(!request.body.to_string().contains("Draft A stays in its own conversation"));
        request.delta("REAL_WORKBENCH_RUN_TO_STOP").await;
        let writing = wait_view(&mut view, |screen| screen.contains("REAL_WORKBENCH_RUN_TO_STOP") && screen.contains("Writing response")).await;
        assert_eq!(writing.matches("REAL_WORKBENCH_RUN_TO_STOP").count(), 1);
        support::evidence_cells("workbench-streaming", &view);
        request.tool_prefix("todo_write", "{\"todos\":[").await;
        let preparing = wait_view(&mut view, |screen| screen.contains("Preparing tool · todo_write · {\"todos\":[")).await;
        evidence("workbench-tool-input-before-stop", &preparing);
        support::evidence_cells("workbench-tool-input-before-stop", &view);
        let observed = client.query("agent", "observe", json!({"sessionId":created_id})).await.unwrap();
        let invocation = observed["run"]["invocationId"].as_str().filter(|id| !id.is_empty()).unwrap();

        // A visible browser never takes the composer's native Enter or arrow
        // handling. Native Tab into a browser Button scopes Escape without a
        // JS focus mirror, and Enter must still activate that exact Button.
        shortcut(&mut view, 'o');
        wait_view(&mut view, |screen| screen.contains("Titles · match case") && screen.contains("Hide")).await;
        wait_session_search(&mut view).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)));
        assert!(view.accessibility_snapshot().iter().any(|node| node.focused && node.label == "Choose workspace scope"));
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.contains("Choose a workspace below")).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.contains("Titles · match case") && !screen.contains("Choose a workspace below")).await;
        for _ in 0..2 {
            view.handle_event(Event::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)));
        }
        assert!(view.accessibility_snapshot().iter().any(|node| node.focused && node.label == "Close session browser"));
        // The resize decision uses native subtree focus, including Tabbed
        // Buttons that never emitted an application focus callback.
        view.resize(80, 24);
        let transferred = wait_view(&mut view, |screen| screen.contains("Titles · match case") && screen.contains("Close · Esc") && !screen.contains("Hide")).await;
        wait_session_search(&mut view).await;
        evidence("workbench-focused-browser-transfers-to-dialog", &transferred);
        support::evidence_cells("workbench-focused-browser-transfers-to-dialog", &view);
        view.resize(120, 40);
        wait_view(&mut view, |screen| screen.contains("Titles · match case") && screen.contains("Hide") && !screen.contains("Close · Esc")).await;
        wait_session_search(&mut view).await;
        for _ in 0..2 {
            view.handle_event(Event::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT)));
        }
        assert!(view.accessibility_snapshot().iter().any(|node| node.focused && node.label == "Close session browser"));
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| !screen.contains("Enter selects · Esc closes") && screen.contains("Preparing tool · todo_write")).await;

        view.resize(80, 24);
        shortcut(&mut view, 'o');
        let narrow_browser = wait_view(&mut view, |screen| screen.contains("Titles · match case") && screen.contains("Close · Esc")).await;
        wait_session_search(&mut view).await;
        evidence("workbench-narrow-session-dialog", &narrow_browser);
        support::evidence_cells("workbench-narrow-session-dialog", &view);
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| !screen.contains("Enter selects · Esc closes") && screen.contains("Preparing tool · todo_write")).await;
        request.delta("BROWSER_ESCAPE_LEFT_THE_RUN_ALIVE").await;
        wait_view(&mut view, |screen| screen.contains("BROWSER_ESCAPE_LEFT_THE_RUN_ALIVE") && screen.contains("Preparing tool · todo_write")).await;
        let still_running = client.query("agent", "observe", json!({"sessionId":created_id})).await.unwrap();
        assert_eq!(still_running["run"]["invocationId"], invocation);
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
        view.resize(120, 40);
        wait_view(&mut view, |screen| screen.contains("BROWSER_ESCAPE_LEFT_THE_RUN_ALIVE") && screen.contains("Preparing tool · todo_write")).await;
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
        let stopped = wait_view(&mut view, |screen| screen.contains("Stopped") && screen.contains("REAL_WORKBENCH_RUN_TO_STOP") && screen.contains("Enter sends") && !screen.contains("Preparing tool") && !screen.contains("Running tool")).await;
        assert_eq!(stopped.matches("REAL_WORKBENCH_RUN_TO_STOP").count(), 1);
        evidence("workbench-stopped-clears-tool-input", &stopped);
        support::evidence_cells("workbench-stopped-clears-tool-input", &view);
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
        assert_eq!(client.load_draft("agent", &fixture.session, EDITOR).await.unwrap().unwrap().text, "Draft A stays in its own conversation");

        // The target belongs to the Host's second matching page. Physical
        // wheel scrolling must obtain it without a manual continuation action.
        for index in 0..33 {
            let created = support::mutate(&client, "create", json!({
                "title":format!("Browser page {index:02}"),"automaticTitle":false,"labels":[],
                "workspace":{"kind":"session","sessionId":fixture.session}
            })).await;
            assert_eq!(created["kind"], "created");
        }
        let first_page = client.query("agent", "list", json!({"filter":"all","scope":{"kind":"all"},"titleQuery":"Browser page","position":{"kind":"start"}})).await.unwrap();
        assert_eq!(first_page["sessions"].as_array().unwrap().len(), 32);
        let next_page = client.query("agent", "list", json!({"filter":"all","scope":{"kind":"all"},"titleQuery":"Browser page","position":{"kind":"after","revision":first_page["revision"],"cursor":first_page["after"]}})).await.unwrap();
        let target = &next_page["sessions"][0];
        let target_title = target["title"].as_str().unwrap();
        let target_id = target["id"].as_str().unwrap();
        assert!(!first_page["sessions"].as_array().unwrap().iter().any(|session| session["id"] == target_id));
        shortcut(&mut view, 'o');
        wait_view(&mut view, |screen| screen.contains("Titles · match case") && screen.contains("Enter selects · Esc closes")).await;
        wait_session_search(&mut view).await;
        view.handle_event(Event::Paste("Browser page".into()));
        wait_view(&mut view, |screen| screen.contains("Browser page") && !screen.contains("Loading…")).await;
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                support::tick(&mut view);
                let screen = view.snapshot();
                if screen.contains(target_title) {
                    break;
                }
                let row = first_page["sessions"].as_array().unwrap().iter()
                    .filter_map(|session| session["title"].as_str())
                    .find(|title| screen.contains(title))
                    .expect("the browser must show a matching session row before its next page");
                support::scroll_text(&mut view, row, MouseEventKind::ScrollDown);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("native browser scrolling did not load the second Host page");
        support::evidence_cells("workbench-automatic-browser-pagination", &view);

        // Read at the end before a real external metadata commit. Renaming
        // preserves this session's ID, timestamp and query membership, so a
        // catalog revision must not remove the tail or move its screen row.
        support::scroll_text(&mut view, target_title, MouseEventKind::ScrollDown);
        support::tick(&mut view);
        let tail_y = view.accessibility_snapshot().into_iter()
            .find(|node| node.label == format!("Open conversation {target_title}"))
            .unwrap().frame.origin.y.0;
        let original = client.query("agent", "observe", json!({"sessionId":target_id})).await.unwrap();
        let updated_title = format!("{target_title} v2");
        let renamed = support::mutate(&client, "metadata", json!({
            "sessionId":target_id,"expectedRevision":original["session"]["revision"],
            "title":updated_title,"labels":original["session"]["labels"]
        })).await;
        assert_eq!(renamed["kind"], "committed");
        assert_eq!(renamed["session"]["id"], target_id);
        assert_eq!(renamed["session"]["updatedAt"], original["session"]["updatedAt"]);
        // A new title proves the new revision reached this UI. No wheel or
        // other navigation action is allowed to recover the original row.
        wait_view(&mut view, |screen| screen.contains(&updated_title)).await;
        let tail_after = view.accessibility_snapshot().into_iter()
            .find(|node| node.label == format!("Open conversation {updated_title}"))
            .unwrap().frame.origin.y.0;
        assert_eq!(tail_after, tail_y);
        support::evidence_cells("workbench-browser-revision-preserves-tail", &view);
        let target_title = updated_title.as_str();
        support::click_text(&mut view, &format!("Open conversation {target_title}"));
        wait_view(&mut view, |screen| screen.lines().any(|line| line.contains("MAKA") && line.contains(target_title)) && screen.contains("Enter sends")).await;
        assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some(""));
        assert_eq!(client.load_draft("agent", &fixture.session, EDITOR).await.unwrap().unwrap().text, "Draft A stays in its own conversation");
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("real Host workbench navigation timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_agent_history_and_live_stream_render_and_the_app_sends_once() {
    // Later phases browse the outer history explicitly. Hit visible transcript
    // text with the existing physical wheel helper, never the focused List.
    async fn wheel_history_up_to(view: &mut AppView, marker: &str) -> String {
        let started = std::time::Instant::now();
        loop {
            support::tick(view);
            let screen = view.snapshot();
            if screen.contains(marker) {
                return screen;
            }
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "history wheel did not reveal {marker:?}:\n{screen}"
            );
            // These phases are flat or folded. Exclude the fixed checklist's
            // NATIVE_TODO labels so the pointer always hits transcript content.
            let text = screen
                .lines()
                .find(|line| {
                    [
                        "History ",
                        "NATIVE_THINK_SOURCE_",
                        "NATIVE_ACTIVITY_HEADER_",
                        "TITLE_",
                        "NATIVE_INTERMEDIATE_RESPONSE_OUTSIDE_PROCESS",
                        "REAL_PROVIDER_STREAM_VISIBLE",
                        "NEXT_INPUT_FOLDS_PREVIOUS_PROCESS",
                        "SECOND_POLICY_TURN_FINAL",
                        "> todo_write",
                        "Completed",
                    ]
                    .iter()
                    .any(|marker| line.contains(*marker))
                })
                .map(str::trim)
                .expect("the viewport must contain painted fixture transcript text");
            support::scroll_text(view, text, MouseEventKind::ScrollUp);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
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
        evidence("history", &history);
        let previous = view.surface().focused();
        assert!(view.surface().input_context().is_some(), "the history wheel must start with composer focus, so a missed pointer cannot scroll the focused List");
        // Wheel events over the conversation cross the actual history page boundary.
        let scrolling = std::time::Instant::now();
        loop {
            support::scroll_text(&mut view, "History ", MouseEventKind::ScrollUp);
            view.poll().unwrap();
            support::tick(&mut view);
            let screen = view.snapshot();
            if screen.contains("History 00 paragraph") {
                assert!(!screen.contains("Load earlier messages"));
                evidence("history-automatically-prepended", &screen);
                break;
            }
            assert!(scrolling.elapsed() < Duration::from_secs(20), "scrolling did not load original history:\n{screen}");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        shortcut(&mut view, 'l');
        wait_view(&mut view, |screen| screen.contains("History 15 paragraph 29") && screen.contains("[Source]") && screen.contains("Enter sends")).await;
        tokio::time::timeout(Duration::from_secs(20), async {
            while view.surface().focused() != previous {
                view.poll().unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("jump to latest did not restore composer focus");

        view.handle_event(Event::Paste("NSH user send, exactly once".into()));
        wait_view(&mut view, |screen| screen.contains("NSH user send, exactly once") && screen.contains("Send")).await;
        // Native editor text can paint before its change reaches the app and enables Send.
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                view.poll().unwrap();
                if view.accessibility_snapshot().iter().any(|node| node.label == "Send message" && !node.state.disabled) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("the presented Send button did not become enabled");
        support::click_text(&mut view, "Send");
        // AppView's engine continues independently while this awaits the real HTTP request.
        let mut request = fixture.next_request().await;
        assert!(request.body["messages"].as_array().unwrap().iter().any(|message| message["role"] == "user" && message["content"].to_string().contains("NSH user send, exactly once")));
        let thinking = std::iter::once("NATIVE_THINK_SOURCE_FIRST".to_owned())
            .chain((1..63).map(|index| format!("NATIVE_THINK_SOURCE_{index:02}")))
            .chain(["NATIVE_THINK_SOURCE_LAST".to_owned(), "\nNATIVE_ACTIVITY_HEADER_TAIL_INITIAL".to_owned()])
            .collect::<Vec<_>>()
            .join("\n");
        request.thinking(&thinking).await;
        let grouped = wait_view(&mut view, |screen| screen.contains("Analyzing the request · NATIVE_ACTIVITY_HEADER_TAIL_INITIAL") && !screen.contains("◇ Think")).await;
        evidence("standard-live-group-collapsed", &grouped);
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("On completion") && screen.contains("Verbose")).await;
        support::click_text(&mut view, "Compact");
        let compact = wait_view(&mut view, |screen| screen.contains("Details · Compact") && screen.contains("Analyzing the request") && !screen.contains("NATIVE_ACTIVITY_HEADER_TAIL_INITIAL") && !screen.contains("◇ Think")).await;
        evidence("compact-live-process", &compact);
        support::click_text(&mut view, "Detailed");
        let detailed = wait_view(&mut view, |screen| screen.contains("Details · Detailed") && screen.contains("◇ Think · NATIVE_THINK_SOURCE_FIRST") && !screen.contains("Analyzing the request")).await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Turn activity"));
        evidence("detailed-live-flat-process", &detailed);
        support::click_text(&mut view, "Verbose");
        wait_view(&mut view, |screen| screen.contains("Details · Verbose") && screen.contains("◇ Think · NATIVE_THINK_SOURCE_FIRST") && !screen.contains("Analyzing the request")).await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Turn activity"));
        support::click_text(&mut view, "Standard");
        wait_view(&mut view, |screen| screen.contains("Details · Standard") && screen.contains("Analyzing the request · NATIVE_ACTIVITY_HEADER_TAIL_INITIAL") && !screen.contains("◇ Think")).await;
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| !screen.contains("On completion")).await;
        // The one live group's button has a stable semantic label; its text
        // includes a disclosure glyph before the source-backed activity title.
        support::click_text(&mut view, "Process group");
        wait_view(&mut view, |screen| screen.contains("◇ Think · NATIVE_THINK_SOURCE_FIRST")).await;
        support::click_text(&mut view, "◇ Think · NATIVE_THINK_SOURCE_FIRST");
        // Either end of the actual expanded source must paint before keyboard scrolling.
        wait_view(&mut view, |screen| screen.contains("NATIVE_THINK_SOURCE_01") || screen.contains("NATIVE_THINK_SOURCE_62")).await;
        support::focus_activity(&mut view).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        let at_source_start = |screen: &str| screen.matches("NATIVE_THINK_SOURCE_FIRST").count() == 2 && !screen.contains("NATIVE_THINK_SOURCE_LAST");
        let source_top = wait_view(&mut view, &at_source_start).await;
        evidence("activity-source-top", &source_top);
        support::evidence_cells("activity-source-top", &view);

        // Keep focus outside the activity List so the wheel cannot pass via focus fallback.
        let activity_focus = view.surface().focused();
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT)));
        tokio::time::timeout(Duration::from_secs(20), async {
            while view.surface().focused().is_none() || view.surface().focused() == activity_focus {
                view.poll().unwrap();
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("history did not receive native focus");
        let header_row = source_top.lines().position(|line| line.contains("Analyzing the request")).expect("the activity header must be visible above its source");
        let source_row = source_top.lines().position(|line| line.contains("NATIVE_THINK_SOURCE_01")).unwrap();
        support::scroll_text(&mut view, "NATIVE_THINK_SOURCE_01", MouseEventKind::ScrollDown);
        let wheeled = wait_view(&mut view, |screen| screen.lines().position(|line| line.contains("NATIVE_THINK_SOURCE_01")) == Some(source_row - 3)).await;
        assert!(wheeled.contains("NATIVE_THINK_SOURCE_03"));
        assert_eq!(wheeled.lines().position(|line| line.contains("Analyzing the request")), Some(header_row), "the body wheel must not move the fixed turn header");
        evidence("activity-mouse-wheel", &wheeled);
        support::evidence_cells("activity-mouse-wheel", &view);
        support::focus_activity(&mut view).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        // Alt+Down explicitly reveals the group. Freeze its settled pose before
        // streaming; the earlier mouse-wheel assertions retain their own anchor.
        let cadence_source_top = wait_view(&mut view, &at_source_start).await;
        let cadence_header_row = cadence_source_top.lines().position(|line| line.contains("Analyzing the request")).expect("the navigated activity header must be visible above its source");
        let cadence_source_row = cadence_source_top.lines().position(|line| line.contains("NATIVE_THINK_SOURCE_01")).expect("the navigated original source must be visible");

        // Rapid paragraphs must update during the stream, at a bounded cadence,
        // while the expanded source and its reader anchor stay put.
        let cadence = std::time::Instant::now();
        let mut titles = vec![(Duration::ZERO, support::painted_text(&view, "⌃ Analyzing the request", cadence_header_row))];
        let mut paragraph = 0;
        while cadence.elapsed() < Duration::from_millis(650) {
            paragraph += 1;
            request.thinking(&format!("\n\nTITLE_{paragraph:02} 规划 e\u{301} {}", "界e\u{301}".repeat(48))).await;
            for _ in 0..4 {
                view.poll().unwrap();
                support::tick(&mut view);
                let screen = view.snapshot();
                assert!(at_source_start(&screen), "a title update moved the source reader:\n{screen}");
                assert_eq!(screen.lines().position(|line| line.contains("NATIVE_THINK_SOURCE_01")), Some(cadence_source_row), "a title update moved the original source row:\n{screen}");
                assert!(screen.lines().nth(cadence_header_row).unwrap().contains("Analyzing the request"), "the fixed activity header moved:\n{screen}");
                let title = support::painted_text(&view, "⌃ Analyzing the request", cadence_header_row);
                if title != titles.last().unwrap().1 {
                    titles.push((cadence.elapsed(), title));
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
        assert!(titles.len() >= 3, "continuous input postponed every intermediate title: {titles:?}");
        let latest = format!("TITLE_{paragraph:02} 规划 e\u{301}");
        wait_view(&mut view, |screen| at_source_start(screen) && screen.lines().nth(cadence_header_row).is_some_and(|line| line.contains(&latest))).await;
        let last = support::painted_text(&view, "⌃ Analyzing the request", cadence_header_row);
        assert!(last.ends_with('…') && !last.ends_with("e…"), "the long title must ellipsize without splitting a combining grapheme: {last}");
        if last != titles.last().unwrap().1 {
            titles.push((cadence.elapsed(), last));
        }
        // An interval-wide bound avoids confusing poll scheduling with the JS
        // presentation timestamp, while still catching a title for every delta.
        assert!(titles.len() <= cadence.elapsed().as_millis() as usize / 150 + 2, "the header changed faster than its minimum lifetime: {titles:?}");
        evidence("activity-title-cadence", &titles.iter().map(|(at, title)| format!("{}ms {title}", at.as_millis())).collect::<Vec<_>>().join("\n"));
        support::evidence_cells("activity-title-unicode", &view);

        request.thinking("\n\nNATIVE_ACTIVITY_HEADER_TAIL_APPENDED").await;
        let anchored = wait_view(&mut view, |screen| at_source_start(screen) && screen.matches("NATIVE_ACTIVITY_HEADER_TAIL_APPENDED").count() == 1).await;
        evidence("activity-append-keeps-source-top", &anchored);
        support::evidence_cells("activity-append-keeps-source-top", &view);

        let todos = json!([
            {"content":"NATIVE_TODO_DONE","status":"completed"},
            {"content":"NATIVE_TODO_ACTIVE","status":"in_progress"},
            {"content":"NATIVE_TODO_PENDING","status":"pending"}
        ]);
        assert!(request.body["tools"].as_array().unwrap().iter().any(|tool| tool["function"]["name"] == "todo_write"));
        let tool_input = json!({"todos":todos.clone()});
        let arguments = tool_input.to_string();
        let prefix = "{\"todos\":[";
        let suffix = arguments.strip_prefix(prefix).unwrap();
        request.delta("NATIVE_INTERMEDIATE_RESPONSE_OUTSIDE_PROCESS").await;
        // A reply closes only this contiguous process segment. The same native
        // source reader stays at its original row while later work gets a group.
        let cut = wait_view(&mut view, |screen| at_source_start(screen) && screen.contains("⌃ 1 thought")).await;
        assert_eq!(cut.lines().position(|line| line.contains("⌃ 1 thought")), Some(cadence_header_row));
        assert_eq!(cut.lines().position(|line| line.contains("NATIVE_THINK_SOURCE_01")), Some(cadence_source_row));
        evidence("intermediate-response-cuts-process-group", &cut);
        support::evidence_cells("intermediate-response-cuts-process-group", &view);
        // Leave the old source reader explicitly, then select the new segment
        // only after its actual preparation has reached the UI projection.
        shortcut(&mut view, 'l');
        let intermediate = wait_view(&mut view, |screen| screen.contains("NATIVE_INTERMEDIATE_RESPONSE_OUTSIDE_PROCESS")).await;
        evidence("intermediate-response-outside-process", &intermediate);
        support::evidence_cells("intermediate-response-outside-process", &view);
        request.tool_prefix("todo_write", prefix).await;
        let preparing = wait_view(&mut view, |screen| screen.contains("Preparing tool · todo_write") && screen.contains(prefix)).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)));
        let pending = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let pending_history = client.query("agent", "history", json!({"sessionId":fixture.session,"through":pending["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        assert!(pending_history["chunks"].as_array().unwrap().iter().all(|chunk| chunk["tool"]["call"]["name"] != "todo_write"), "an unfinished argument prefix must not become a canonical tool call");
        evidence("activity-preparing-tool-input", &preparing);
        support::evidence_cells("activity-preparing-tool-input", &view);
        request.tool_suffix(suffix).await;
        // A real tool result causes exactly one additional model HTTP request.
        let mut request = fixture.next_request().await;
        wait_view(&mut view, |screen| screen.contains("⌃ Working…") && !screen.contains("Running tool")).await;
        let complete_call = request.body["messages"].as_array().unwrap().iter()
            .filter_map(|message| message["tool_calls"].as_array()).flatten()
            .find(|call| call["id"] == "nsh-native-todo-call").unwrap();
        assert_eq!(complete_call["function"]["name"], "todo_write");
        assert_eq!(serde_json::from_str::<Value>(complete_call["function"]["arguments"].as_str().unwrap()).unwrap(), tool_input);
        let tool_result = request.body["messages"].as_array().unwrap().iter().find(|message| message["role"] == "tool" && message["tool_call_id"] == "nsh-native-todo-call").unwrap();
        assert_eq!(tool_result["content"], "1. [completed] \"NATIVE_TODO_DONE\"\n2. [in_progress] \"NATIVE_TODO_ACTIVE\"\n3. [pending] \"NATIVE_TODO_PENDING\"");
        let checklist = client.query("agent", "checklist", json!({"sessionId":fixture.session,"offset":0})).await.unwrap();
        assert_eq!(checklist["enabled"], true);
        assert_eq!(checklist["items"], todos);
        assert_eq!(checklist["total"], 3);
        assert_eq!(checklist["completed"], 1);
        assert_eq!(checklist["current"], todos[1]);
        assert!(checklist["nextOffset"].is_null());
        wait_view(&mut view, |screen| screen.contains("▸ Todo 1/3")).await;
        // The canonical tool segment now owns a different native activity List.
        support::focus_activity(&mut view).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.matches("> todo_write").count() == 1).await;
        support::click_text(&mut view, "▸ Todo 1/3");
        let checklist_visible = wait_view(&mut view, |screen| screen.contains("▾ Todo 1/3") && screen.contains("NATIVE_TODO_DONE") && screen.contains("NATIVE_TODO_PENDING")).await;
        evidence("real-tool-and-checklist", &checklist_visible);
        support::evidence_cells("real-tool-and-checklist", &view);

        request.delta("REAL_PROVIDER_STREAM_VISIBLE").await;
        let streaming = wait_view(&mut view, |screen| screen.contains("REAL_PROVIDER_STREAM_VISIBLE") && screen.contains("Writing response")).await;
        evidence("streaming", &streaming);
        request.delta(" and committed.").await;
        request.finish().await;
        fixture.idle(&client, &fixture.session).await;
        let committed = wait_view(&mut view, |screen| screen.contains("› Completed") && screen.matches("REAL_PROVIDER_STREAM_VISIBLE").count() == 1 && screen.contains("committed.") && !screen.contains("NATIVE_THINK_SOURCE_FIRST") && !screen.contains("NATIVE_ACTIVITY_HEADER_TAIL_APPENDED") && screen.contains("Enter sends")).await;
        evidence("committed", &committed);
        support::evidence_cells("committed", &view);

        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("On completion")).await;
        support::click_text(&mut view, "Compact");
        support::click_text(&mut view, "Details ·");
        shortcut(&mut view, 'l');
        wait_view(&mut view, |screen| screen.contains("Details · Compact") && screen.contains("› Completed") && !screen.contains("NATIVE_INTERMEDIATE_RESPONSE_OUTSIDE_PROCESS")).await;
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("On completion")).await;
        support::click_text(&mut view, "Detailed");
        support::click_text(&mut view, "Details ·");
        shortcut(&mut view, 'l');
        wait_view(&mut view, |screen| screen.contains("Details · Detailed") && screen.contains("› Completed")).await;
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("Next input")).await;
        support::click_text(&mut view, "Next input");
        support::click_text(&mut view, "Details ·");
        shortcut(&mut view, 'l');
        let deferred_tail = wait_view(&mut view, |screen| screen.contains("Details · Detailed") && screen.matches("REAL_PROVIDER_STREAM_VISIBLE").count() == 1).await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Turn activity"));
        evidence("detailed-next-input-final-outside-process", &deferred_tail);
        let deferred_body = wheel_history_up_to(&mut view, "NATIVE_INTERMEDIATE_RESPONSE_OUTSIDE_PROCESS").await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Turn activity"));
        evidence("detailed-next-input-intermediate-flat", &deferred_body);
        let deferred = wheel_history_up_to(&mut view, "⌃ Completed").await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Turn activity"));
        evidence("detailed-next-input-keeps-completed-process-flat", &deferred);
        shortcut(&mut view, 'l');
        tokio::time::timeout(Duration::from_secs(20), async {
            while view.surface().input_context().is_none() {
                support::tick(&mut view);
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("Jump latest did not focus the composer");
        view.handle_event(Event::Paste("NEXT_INPUT_FOLDS_PREVIOUS_PROCESS".into()));
        support::wait_draft(&client, &fixture.session, "NEXT_INPUT_FOLDS_PREVIOUS_PROCESS").await;
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                support::tick(&mut view);
                if view.accessibility_snapshot().iter().any(|node| node.label == "Send message" && !node.state.disabled) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("the next presented Send button did not become enabled");
        support::click_text(&mut view, "Send");
        let mut next = fixture.next_request().await;
        let next_user = wait_view(&mut view, |screen| screen.contains("NEXT_INPUT_FOLDS_PREVIOUS_PROCESS") && !screen.contains("NATIVE_INTERMEDIATE_RESPONSE_OUTSIDE_PROCESS")).await;
        evidence("next-input-message", &next_user);
        let next_input = wheel_history_up_to(&mut view, "› Completed").await;
        assert_eq!(next_input.matches("REAL_PROVIDER_STREAM_VISIBLE").count(), 1);
        assert!(!next_input.contains("NATIVE_INTERMEDIATE_RESPONSE_OUTSIDE_PROCESS"));
        evidence("next-input-folds-previous-process", &next_input);
        shortcut(&mut view, 'l');
        next.delta("SECOND_POLICY_TURN_FINAL").await;
        next.finish().await;
        fixture.idle(&client, &fixture.session).await;
        wait_view(&mut view, |screen| screen.contains("SECOND_POLICY_TURN_FINAL") && screen.contains("Enter sends")).await;

        // This older group is now closed: each manual opening starts at the
        // top. The row's small disclosure state still belongs to its source id.
        let older_turn = wheel_history_up_to(&mut view, "› Completed").await;
        assert_eq!(older_turn.matches("REAL_PROVIDER_STREAM_VISIBLE").count(), 1);
        support::click_text(&mut view, "› Completed");
        wait_view(&mut view, |screen| screen.contains("› 1 thought")).await;
        support::click_text(&mut view, "› 1 thought");
        let reopened = wait_view(&mut view, &at_source_start).await;
        evidence("completed-activity-source-reopened-at-top", &reopened);
        support::evidence_cells("completed-activity-source-reopened-at-top", &view);
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("Verbose") && screen.contains("Next input")).await;
        support::click_text(&mut view, "Compact");
        support::click_text(&mut view, "Details ·");
        let compact_settled = wait_view(&mut view, |screen| screen.contains("Details · Compact") && screen.contains("◇ Think") && !screen.contains("◇ Think · NATIVE_THINK_SOURCE_FIRST") && screen.matches("NATIVE_THINK_SOURCE_FIRST").count() == 1).await;
        evidence("compact-settled-thinking-without-preview", &compact_settled);
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("Standard") && screen.contains("Next input")).await;
        support::click_text(&mut view, "Standard");
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("Details · Standard") && at_source_start(screen)).await;
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("Detailed") && screen.contains("Next input")).await;
        support::click_text(&mut view, "Detailed");
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("Details · Detailed") && at_source_start(screen)).await;
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("Verbose") && screen.contains("Next input")).await;
        support::click_text(&mut view, "Verbose");
        support::click_text(&mut view, "Details ·");
        shortcut(&mut view, 'l');
        let verbose_tail = wait_view(&mut view, |screen| screen.contains("Details · Verbose") && screen.contains("SECOND_POLICY_TURN_FINAL")).await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Turn activity"));
        evidence("verbose-final-outside-process", &verbose_tail);
        let verbose_tool = wheel_history_up_to(&mut view, "> todo_write").await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Turn activity"));
        evidence("verbose-settled-tool-flat", &verbose_tool);
        let verbose = wheel_history_up_to(&mut view, "NATIVE_INTERMEDIATE_RESPONSE_OUTSIDE_PROCESS").await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Turn activity"));
        evidence("verbose-settled-process-flat", &verbose);
        shortcut(&mut view, 'l');
        wait_view(&mut view, |screen| screen.contains("Details · Verbose") && screen.contains("SECOND_POLICY_TURN_FINAL")).await;
        support::load_application(&mut view);
        let persisted = wait_view(&mut view, |screen| screen.contains("Details · Verbose") && screen.contains("SECOND_POLICY_TURN_FINAL") && screen.contains("Enter sends")).await;
        support::click_text(&mut view, "Details ·");
        wait_view(&mut view, |screen| screen.contains("Fold:")).await;
        assert!(view.accessibility_snapshot().iter().any(|node| node.label == "Fold work details: Next input" && node.state.selected));
        evidence("process-policy-survives-reload", &persisted);
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 19);
        assert!(client.pending_operations("agent", &fixture.session, EDITOR).await.unwrap().is_empty());
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("real Host/Agent UI integration timed out");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn streamed_markdown_keeps_inline_styles_and_the_original_selectable_source() {
    use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
    #[track_caller]
    fn wait_mode_layout(view: &mut AppView) -> impl std::future::Future<Output = ()> {
        let caller = std::panic::Location::caller();
        async move {
            let started = std::time::Instant::now();
            loop {
                support::tick(view);
                assert!(
                    view.last_error().is_none(),
                    "NSH app error at {caller}: {:?}",
                    view.last_error()
                );
                if !view.surface().animating() {
                    return;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(20),
                    "Markdown mode layout did not settle at {caller}:\n{}",
                    view.snapshot()
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }
    }
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::open().await;
        let client = fixture.client().await;
        let (mut view, bindings) = fixture.view(client.clone(), Some(fixture.session.clone()));
        wait_view(&mut view, |screen| screen.contains("Enter sends")).await;
        let sent = support::mutate(&client, "send", json!({
            "sessionId":fixture.session,"message":"Render the original Markdown", "model":fixture.selection
        })).await;
        assert_eq!(sent["kind"], "started");
        let mut request = fixture.next_request().await;
        let prefix = "# RICH_HEADING\n\n**RICH_BOLD** *RICH_ITALIC* ~~RICH_STRIKE~~ `RICH_CODE`";
        request.delta(prefix).await;
        let rendered = wait_view(&mut view, |screen| {
            screen.contains("RICH_HEADING") && screen.contains("RICH_STRIKE")
                && !screen.contains("# RICH_HEADING") && !screen.contains("**RICH_BOLD**")
        }).await;
        evidence("rich-response-live-inline", &rendered);
        support::evidence_cells("rich-response-live-inline", &view);
        // Inspect the actual terminal cells, rather than the Markdown DTO or
        // parser internals: these styles must survive nested NSH Text shaping.
        let viewport = view.surface().viewport();
        let area = Rect::new(0, 0, (viewport.width.0 / 8.0) as u16, (viewport.height.0 / 16.0) as u16);
        let mut buffer = Buffer::empty(area);
        view.render(area, &mut buffer);
        for (token, expected) in [
            ("RICH_HEADING", Modifier::BOLD),
            ("RICH_BOLD", Modifier::BOLD),
            ("RICH_ITALIC", Modifier::ITALIC),
            ("RICH_STRIKE", Modifier::CROSSED_OUT),
        ] {
            let cells = (0..area.height).find_map(|y| {
                (0..=area.width.saturating_sub(token.len() as u16)).find_map(|x| {
                    let text: String = (0..token.len()).map(|index| buffer[(x + index as u16, y)].symbol()).collect();
                    (text == token).then_some((x, y))
                })
            }).unwrap_or_else(|| panic!("formatted token {token} is not visible:\n{rendered}"));
            assert!((0..token.len()).all(|index| buffer[(cells.0 + index as u16, cells.1)].modifier.contains(expected)), "terminal cells for {token} lost {expected:?}");
        }
        let suffix = "\n\n- RICH_LIST\n\n> RICH_QUOTE\n\n```txt\nRICH_CODE_BLOCK\n```";
        request.delta(suffix).await;
        // The stream remains physically open: parsing cannot wait for final.
        wait_view(&mut view, |screen| screen.contains("RICH_CODE_BLOCK") && screen.contains("[Source]") && !screen.contains("```txt")).await;
        request.finish().await;
        fixture.idle(&client, &fixture.session).await;
        let complete = wait_view(&mut view, |screen| screen.contains("RICH_CODE_BLOCK") && screen.contains("[Source]") && screen.contains("Enter sends")).await;
        evidence("rich-response-complete", &complete);
        wait_mode_layout(&mut view).await;
        support::click_text(&mut view, "[Source]");
        let original = wait_view(&mut view, |screen| screen.contains("```txt") && screen.contains("RICH_CODE_BLOCK") && screen.contains("[Formatted]")).await;
        evidence("rich-response-original-source", &original);
        // Source has a different native height. Observe the original text
        // first, then let its scroll/layout motion finish before one click.
        wait_mode_layout(&mut view).await;
        support::click_text(&mut view, "[Formatted]");
        wait_view(&mut view, |screen| screen.contains("RICH_CODE_BLOCK") && screen.contains("[Source]") && !screen.contains("```txt")).await;
        let observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let history = client.query("agent", "history", json!({"sessionId":fixture.session,"through":observed["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        let response = history["chunks"].as_array().unwrap().iter().find(|chunk| chunk["role"] == "assistant" && chunk["field"] == "body").unwrap();
        assert_eq!(response["text"], format!("{prefix}{suffix}"));
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
        // Exceed the formatter's document budget during a real open stream.
        // The complete current source stays selectable instead of retaining
        // a stale rich prefix or returning a second native copy of the document.
        let sent = support::mutate(&client, "send", json!({
            "sessionId":fixture.session,"message":"Keep a partial code source literal", "model":fixture.selection
        })).await;
        assert_eq!(sent["kind"], "started");
        let mut request = fixture.next_request().await;
        let source = format!("```txt\n{}\n# RICH_TAIL_HEADING\n**RICH_TAIL_MARK**\n```", "filler line\n".repeat(24000));
        request.delta(&source).await;
        let over_budget = wait_view(&mut view, |screen| screen.contains("Long response shown as original source.") && screen.contains("**RICH_TAIL_MARK**") && screen.contains("[Source]")).await;
        evidence("rich-response-over-formatting-budget", &over_budget);
        wait_mode_layout(&mut view).await;
        support::click_text(&mut view, "[Source]");
        wait_view(&mut view, |screen| screen.contains("**RICH_TAIL_MARK**") && screen.contains("[Formatted]") && !screen.contains("Formatting unavailable")).await;
        request.finish().await;
        fixture.idle(&client, &fixture.session).await;
        drop(view);
        bindings.close().await.unwrap();

        // A fresh history tail omits the opening fence. Its heading-looking
        // lines must stay original source until offset zero has been loaded.
        let client = fixture.client().await;
        let observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let tail = client.query("agent", "history", json!({"sessionId":fixture.session,"through":observed["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        let response = tail["chunks"].as_array().unwrap().iter().find(|chunk| chunk["role"] == "assistant" && chunk["field"] == "body").unwrap();
        assert_eq!(response["totalBytes"].as_u64(), Some(source.len() as u64));
        assert!(source.ends_with(response["text"].as_str().unwrap()));
        assert!(response["offset"].as_u64().unwrap() > 0, "this history must omit the original opening fence");
        let (mut view, bindings) = fixture.view(client, Some(fixture.session.clone()));
        let literal = wait_view(&mut view, |screen| screen.contains("# RICH_TAIL_HEADING") && screen.contains("**RICH_TAIL_MARK**") && screen.contains("[Source]")).await;
        evidence("rich-response-incomplete-history", &literal);
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2);
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("real Host rich response workflow timed out");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn immutable_tool_images_render_and_preview_without_stopping_the_running_turn() {
    use base64::{
        Engine as _,
        engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    };
    use image::{ExtendedColorType, ImageEncoder, codecs::png};
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::registered_images().await;
        let client = fixture.client().await;
        let path = fixture.directory.path().join("workspace/scene.png");
        let pixels: Vec<u8> = (0..192 * 128).flat_map(|index| {
            if index % 192 < 64 { [255, 64, 32, 255] }
            else if index % 192 < 128 { [32, 220, 96, 255] }
            else { [64, 128, 255, 255] }
        }).collect();
        let mut bytes = Vec::new();
        png::PngEncoder::new_with_quality(&mut bytes, png::CompressionType::Uncompressed, png::FilterType::NoFilter)
            .write_image(&pixels, 192, 128, ExtendedColorType::Rgba8).unwrap();
        assert!(bytes.len() > 2 * 32768, "the real PNG must cross multiple Host page boundaries");
        std::fs::write(&path, &bytes).unwrap();
        support::mutate(&client, "send", json!({"sessionId":fixture.session,"message":"Read the image", "model":fixture.selection})).await;
        let request = fixture.next_request().await;
        request.tool("Read", json!({"path":"scene.png"})).await;
        let mut request = fixture.next_request().await;
        let image_url = request.body["messages"].as_array().unwrap().iter()
            .filter_map(|message| message["content"].as_array()).flatten()
            .find(|part| part["type"] == "image_url").unwrap_or_else(|| panic!("missing actual image in model request: {}", request.body["messages"]))["image_url"]["url"].as_str().unwrap();
        assert_eq!(STANDARD.decode(image_url.strip_prefix("data:image/png;base64,").unwrap()).unwrap(), bytes);
        // The live Read tool has settled. Neither viewer may reopen its original file.
        std::fs::write(&path, b"the workspace image was replaced").unwrap();
        let observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let history = client.query("agent", "history", json!({"sessionId":fixture.session,"through":observed["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        let result = history["chunks"].as_array().unwrap().iter()
            .find(|chunk| chunk["tool"]["result"]["image"] == true).unwrap()["tool"]["result"]["position"].clone();
        let mut terminal = support::pty::Pty::spawn(&fixture.directory.path().join("host"), Some(&fixture.session));
        terminal.wait_for("Enter sends").await;
        terminal.wait_for("› Working…").await;
        terminal.send(b"\x1b[1;3B");
        terminal.wait_for("> Read").await;
        terminal.click_text("> Read");
        let inline = terminal.wait_for("Open image · 192 × 128").await;
        assert!(inline.contains('▀'), "the actual PNG must paint in the production terminal");
        evidence("tool-image-inline", &inline);
        terminal.click_text("Open image");
        let preview = terminal.wait_for("Original image").await;
        assert!(preview.contains("Image preview") && preview.contains("192 × 128") && preview.contains('▀'));
        evidence("tool-image-preview", &preview);
        terminal.send(b"\x1b");
        terminal.wait_for("Open image · 192 × 128").await;
        // Native modal focus must return to its opener, so Enter opens it again.
        terminal.send(b"\r");
        terminal.wait_for("Original image").await;
        terminal.click_text("Close · Esc");
        terminal.wait_for("Open image · 192 × 128").await;
        request.delta("IMAGE_RUN_CONTINUES_AFTER_ESCAPE").await;
        terminal.wait_for("Writing response").await;
        // Closing the preview preserves its opener's reading position. Use
        // native activity scrolling to reveal the new response below the image.
        terminal.send(b"\x1b[F");
        terminal.wait_for("IMAGE_RUN_CONTINUES_AFTER_ESCAPE").await;
        request.finish().await;
        fixture.idle(&client, &fixture.session).await;
        terminal.send(b"\x11");
        terminal.finish().await;

        // A source selector cannot move retained image bytes to another Session.
        let roots = client.query("agent", "directories", json!({"kind":"roots"})).await.unwrap();
        let other = support::mutate(&client, "create", json!({"title":"Other image owner","labels":[],"workspace":{"kind":"directory","selection":roots["roots"][0]["selection"]}})).await;
        let other_id = other["session"]["id"].as_str().unwrap();
        let fence = client.query("agent", "observe", json!({"sessionId":other_id})).await.unwrap();
        let uri = format!("asset:/tool-image/{}", URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"sessionId":other_id,"through":fence["through"],"position":result,"offset":0})).unwrap()));
        let bindings = maka_terminal_shell::bindings::Bindings::new(client, tokio::runtime::Handle::current());
        let assets = bindings.assets();
        assert!(tokio::task::spawn_blocking(move || assets.read(&uri)).await.unwrap().is_err());
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("real Host tool image workflow timed out");
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

        support::load_application(&mut view);
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
        terminal.wait_for("Enter selects · Esc closes").await;
        terminal.click_text("Close");
        terminal.wait_for("Make something remarkable.").await;
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
        let blocked = terminal.wait_for("This message was not sent.").await;
        assert!(blocked.contains("Production PTY user message"));
        assert!(blocked.contains("Loading session history"));
        assert!(!blocked.contains("Enter sends"));
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1);
        evidence("production-pty-loading-keeps-draft", &blocked);
        gate.release();
        terminal.wait_for("Enter sends").await;
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1, "history recovery must not replay the early Enter");
        terminal.click_text("Send");
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
                "physical Send click never reached the real provider:\n{}",
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
        request.thinking("PRODUCTION_PTY_SAVED_POLICY_REASONING").await;
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
        terminal.click_text("Details · Standard");
        terminal.wait_for("On completion").await;
        terminal.click_text("Detailed");
        terminal.wait_for("Details · Detailed").await;
        terminal.click_text("Next input");
        let selected = terminal.wait_for("◇ Think · PRODUCTION_PTY_SAVED_POLICY_REASONING").await;
        assert!(selected.contains("REAL_PRODUCTION_PTY_STREAM"));
        evidence("production-pty-selected-process-policy", &selected);
        terminal.click_text("Details · Detailed");
        terminal.send(b"\x11");
        terminal.finish().await;
        drop(terminal);
        // This is a new OS process using the same selected Root, not an
        // AppView reload with a test-only data_dir. Detailed plus next-input
        // must keep the completed reasoning flat beside its original answer.
        let mut restarted = support::pty::Pty::spawn(&fixture.directory.path().join("host"), Some(&fixture.session));
        restarted.wait_for("Details · Detailed").await;
        let persisted = restarted.wait_for("◇ Think · PRODUCTION_PTY_SAVED_POLICY_REASONING").await;
        assert!(persisted.contains("REAL_PRODUCTION_PTY_STREAM") && persisted.contains("completed."));
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2, "loading presentation settings must not send another message");
        evidence("production-pty-process-policy-after-restart", &persisted);
        restarted.send(b"\x11");
        restarted.finish().await;
        drop(restarted);
        let reconnected = fixture.client().await;
        let observed = reconnected.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        assert_eq!(observed["session"]["id"], fixture.session, "quitting the shell must detach without stopping its service Host");
        reconnected.close().await.unwrap();
        drop(gate);
        fixture.finish().await;
    })
    .await
    .expect("production binary PTY verification timed out");
}
