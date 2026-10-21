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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn final_actions_use_original_turn_accounting_and_fork_before_a_later_active_turn() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::open().await;
        let client = fixture.client().await;
        let original = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let metadata = support::mutate(&client, "metadata", json!({
            "sessionId":fixture.session,"expectedRevision":original["session"]["revision"],
            "title":"NSH real Agent","labels":["original-label"]
        })).await;
        assert_eq!(metadata["kind"], "committed");
        let first = support::mutate(&client, "send", json!({
            "sessionId":fixture.session,"message":"First complete turn 中文","model":fixture.selection
        })).await;
        assert_eq!(first["kind"], "started");
        let mut first_request = fixture.next_request().await;
        first_request.delta("# FINAL_A\n\n**完整原文 中文😀**").await;
        first_request.finish().await;
        fixture.idle(&client, &fixture.session).await;
        let first_observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let first_history = client.query("agent", "history", json!({"sessionId":fixture.session,"through":first_observed["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        let final_row = first_history["chunks"].as_array().unwrap().iter().find(|row| row["finalOutput"] == true).unwrap();
        let first_turn = final_row["turnId"].as_str().unwrap().to_owned();
        assert_eq!(final_row["timing"]["finished"]["outcome"], "completed");
        let (mut view, bindings) = fixture.view(client.clone(), Some(fixture.session.clone()));
        wait_view(&mut view, |screen| screen.contains("FINAL_A") && screen.contains("Fork from here") && screen.contains("fixture-model")).await;
        activate(&mut view, "View this turn's model usage").await;
        let usage = wait_view(&mut view, |screen| screen.contains("1 main model request in this turn") && screen.contains("Output tokens")).await;
        for (label, expected) in [("Input tokens", "1"), ("Output tokens", "2"), ("Reasoning tokens", "Unavailable")] {
            assert!(usage.lines().any(|line| line.contains(label) && line.split_whitespace().last() == Some(expected)), "{label} did not show the original provider counter:\n{usage}");
        }
        evidence("final-actions-original-main-usage", &usage);
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| !screen.contains("Main model usage")).await;
        assert!(view.accessibility_snapshot().iter().any(|node| node.focused && node.label == "View this turn's model usage"));
        tab_to_label(&mut view, "Message composer").await;
        view.handle_event(Event::Paste("父会话草稿 stays here".into()));
        support::wait_draft(&client, &fixture.session, "父会话草稿 stays here").await;

        let second = support::mutate(&client, "send", json!({
            "sessionId":fixture.session,"message":"LATER_B must not be copied","model":fixture.selection
        })).await;
        assert_eq!(second["kind"], "started");
        let mut second_request = fixture.next_request().await;
        second_request.delta("ACTIVE_B is still streaming").await;
        wait_view(&mut view, |screen| screen.contains("ACTIVE_B") && screen.contains("Fork from here")).await;
        support::click_button(&mut view, "Fork from this turn").await;
        let forked = wait_view(&mut view, |screen| screen.contains("Fork of NSH real Agent") && screen.contains("FINAL_A") && !screen.contains("ACTIVE_B")).await;
        let listed = client.query("agent", "list", json!({"filter":"all","scope":{"kind":"all"},"titleQuery":"Fork of NSH real Agent","position":{"kind":"start"}})).await.unwrap();
        assert_eq!(listed["sessions"].as_array().unwrap().len(), 1);
        let fork = &listed["sessions"][0];
        let fork_id = fork["id"].as_str().unwrap().to_owned();
        assert_ne!(fork_id, fixture.session);
        assert_eq!(fork["branch"]["sessionId"], fixture.session);
        assert_eq!(fork["branch"]["turnId"], first_turn);
        assert_eq!(fork["labels"], json!(["original-label"]));
        let current = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        assert_eq!(current["run"]["invocationId"], second["invocationId"]);
        let child = client.query("agent", "observe", json!({"sessionId":fork_id})).await.unwrap();
        let copied = client.query("agent", "history", json!({"sessionId":fork_id,"through":child["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        assert!(copied.to_string().contains("FINAL_A"));
        assert!(!copied.to_string().contains("LATER_B"));
        assert!(!copied.to_string().contains("ACTIVE_B"));
        support::wait_draft(&client, &fixture.session, "父会话草稿 stays here").await;
        support::wait_draft(&client, &fork_id, "").await;
        evidence("final-actions-fork-excludes-later-active-turn", &forked);

        view.resize(80, 24);
        wait_view(&mut view, |screen| screen.contains("FINAL_A") && screen.contains("Usage")).await;
        activate(&mut view, "View this turn's model usage").await;
        let copied_usage = wait_view(&mut view, |screen| screen.contains("No model usage is recorded for this turn")).await;
        assert!(!copied_usage.contains("Input tokens"), "copied history must not invent provider counters:\n{copied_usage}");
        evidence("final-actions-copied-history-usage-unavailable-80x24", &copied_usage);
        activate(&mut view, "Close turn usage").await;
        tab_to_label(&mut view, "Message composer").await;
        view.handle_event(Event::Paste("独立分支草稿".into()));
        support::wait_draft(&client, &fork_id, "独立分支草稿").await;
        support::wait_draft(&client, &fixture.session, "父会话草稿 stays here").await;
        second_request.finish().await;
        fixture.idle(&client, &fixture.session).await;
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 2, "actions must not replay model calls");
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("original final-action flow did not settle");
}
