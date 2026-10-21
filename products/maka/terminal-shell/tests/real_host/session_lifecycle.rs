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

async fn activate(view: &mut AppView, label: &str) {
    tab_to_label(view, label).await;
    view.handle_event(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
}

async fn fill_title(view: &mut AppView, title: &str) {
    wait_focused_input(view, "Conversation title").await;
    shortcut(view, 'a');
    view.handle_event(Event::Paste(title.into()));
    let (done, committed) = tokio::sync::oneshot::channel();
    view.control()
        .unwrap()
        .advance_clock(Duration::ZERO, move || {
            let _ = done.send(());
        });
    tokio::time::timeout(Duration::from_secs(20), committed)
        .await
        .unwrap()
        .unwrap();
}

async fn actions(view: &mut AppView, title: &str) {
    shortcut(view, 'o');
    wait_enabled_node(view, &format!("Actions for {title}")).await;
    support::click_label(view, &format!("Actions for {title}"));
    wait_enabled_node(view, "Fork conversation").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_session_lifecycle_keeps_original_revisions_history_and_separate_drafts() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::open().await;
        fixture.seed_history(1).await;
        let client = fixture.client().await;
        let (mut view, bindings) = fixture.view(client.clone(), Some(fixture.session.clone()));
        wait_view(&mut view, |screen| screen.contains("Enter sends") && screen.contains("NSH real Agent")).await;
        tab_to_label(&mut view, "Message composer").await;
        view.handle_event(Event::Paste("Original draft stays with its owner".into()));
        support::wait_draft(&client, &fixture.session, "Original draft stays with its owner").await;

        // The search's keyboard action opens the highlighted original row.
        shortcut(&mut view, 'o');
        wait_focused_input(&mut view, "Search session titles").await;
        wait_enabled_node(&mut view, "Actions for NSH real Agent").await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)));
        activate(&mut view, "Rename conversation").await;
        fill_title(&mut view, "Renamed original").await;
        let observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let external = support::mutate(&client, "metadata", json!({
            "sessionId":fixture.session,"expectedRevision":observed["session"]["revision"],
            "title":"Externally revised","labels":["external-label"]
        })).await;
        assert_eq!(external["kind"], "committed");
        activate(&mut view, "Save conversation title").await;
        wait_view(&mut view, |screen| screen.contains("Review its current details") && screen.contains("Externally revised")).await;
        assert_eq!(client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap()["session"]["title"], "Externally revised");
        // Refreshing a conflicting revision keeps the typed name. A second
        // explicit save keeps the newly observed labels rather than erasing them.
        activate(&mut view, "Save conversation title").await;
        wait_view(&mut view, |screen| screen.contains("Renamed original") && !screen.contains("Rename conversation")).await;
        let renamed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        assert_eq!(renamed["session"]["title"], "Renamed original");
        assert_eq!(renamed["session"]["labels"], json!(["external-label"]));

        actions(&mut view, "Renamed original").await;
        support::click_label(&mut view, "Archive conversation");
        wait_view(&mut view, |screen| screen.contains("This conversation is archived.")).await;
        assert_eq!(client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap()["session"]["archived"], true);
        support::wait_draft(&client, &fixture.session, "Original draft stays with its owner").await;
        shortcut(&mut view, 'o');
        wait_enabled_node(&mut view, "Show archived conversations").await;
        support::click_label(&mut view, "Show archived conversations");
        wait_enabled_node(&mut view, "Actions for Renamed original").await;
        support::click_label(&mut view, "Actions for Renamed original");
        wait_enabled_node(&mut view, "Restore conversation").await;
        support::click_label(&mut view, "Restore conversation");
        wait_view(&mut view, |screen| !screen.contains("This conversation is archived.") && screen.contains("Enter sends")).await;
        assert_eq!(client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap()["session"]["archived"], false);
        support::click_label(&mut view, "Show active conversations");

        actions(&mut view, "Renamed original").await;
        support::click_label(&mut view, "Fork conversation");
        fill_title(&mut view, "Independent fork").await;
        activate(&mut view, "Create conversation fork").await;
        wait_view(&mut view, |screen| screen.contains("Independent fork") && !screen.contains("Fork conversation") && screen.contains("Enter sends")).await;
        let listed = client.query("agent", "list", json!({"filter":"all","scope":{"kind":"all"},"titleQuery":"Independent fork","position":{"kind":"start"}})).await.unwrap();
        assert_eq!(listed["sessions"].as_array().unwrap().len(), 1);
        let fork = &listed["sessions"][0];
        let fork_id = fork["id"].as_str().unwrap().to_owned();
        assert_eq!(fork["branch"]["sessionId"], fixture.session);
        assert_eq!(fork["labels"], json!(["external-label"]));
        let fork_observed = client.query("agent", "observe", json!({"sessionId":fork_id})).await.unwrap();
        let history = client.query("agent", "history", json!({"sessionId":fork_id,"through":fork_observed["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        assert!(history.to_string().contains("History question 00"), "the fork must copy the real original history: {history}");
        tab_to_label(&mut view, "Message composer").await;
        assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some(""));
        view.handle_event(Event::Paste("Independent fork draft".into()));
        support::wait_draft(&client, &fork_id, "Independent fork draft").await;
        shortcut(&mut view, 'o');
        wait_enabled_node(&mut view, "Open conversation Renamed original").await;
        support::click_label(&mut view, "Open conversation Renamed original");
        wait_view(&mut view, |screen| screen.contains("Original draft stays with its owner") && screen.contains("Enter sends")).await;
        tab_to_label(&mut view, "Message composer").await;
        assert_eq!(view.surface().input_context().unwrap().text.as_deref(), Some("Original draft stays with its owner"));

        actions(&mut view, "Renamed original").await;
        support::click_label(&mut view, "Delete conversation");
        wait_enabled_node(&mut view, "Confirm delete conversation").await;
        assert_eq!(client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap()["session"]["id"], fixture.session, "opening confirmation must not delete the source");
        activate(&mut view, "Confirm delete conversation").await;
        wait_view(&mut view, |screen| screen.contains("Make something remarkable.") && !screen.contains("Delete conversation?")).await;
        let remaining = client.query("agent", "list", json!({"filter":"all","scope":{"kind":"all"},"titleQuery":"","position":{"kind":"start"}})).await.unwrap();
        assert_eq!(remaining["sessions"].as_array().unwrap().len(), 1);
        assert_eq!(remaining["sessions"][0]["id"], fork_id);
        support::wait_draft(&client, &fork_id, "Independent fork draft").await;
        assert!(fixture.directory.path().join("workspace").is_dir(), "deleting a conversation must retain its workspace");
        assert!(client.pending_controls().await.unwrap().is_empty());
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1, "lifecycle actions must not replay any real model request");
        evidence("session-lifecycle-final", &view.snapshot());
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.unwrap();
}
