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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn readable_tool_cards_preserve_paged_originals_across_workspace_changes_and_resize() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::files("readText").await;
        let client = fixture.client().await;
        let path = fixture.directory.path().join("workspace/recorded.txt");
        let original = format!("起点_原始\n{}终点_原始\n", "中文😀 retained line\n".repeat(700));
        assert!(original.len() > 8192, "the real result must require multiple source pages");
        std::fs::write(&path, &original).unwrap();
        let sent = support::mutate(&client, "send", json!({
            "sessionId":fixture.session, "message":"Read the recorded document", "model":fixture.selection,
        })).await;
        assert_eq!(sent["kind"], "started", "{sent}");
        fixture.next_request().await.tool("WorkspaceDocument", json!({"path":"recorded.txt"})).await;
        let mut continuation = fixture.next_request().await;
        let result: Value = serde_json::from_str(continuation.body["messages"].as_array().unwrap()
            .iter().rev().find(|message| message["role"] == "tool").unwrap()["content"]
            .as_str().unwrap()).unwrap();
        assert_eq!(result["kind"], "completed", "{result}");
        assert_eq!(result["value"]["content"], original);
        // The card must use the original Host source rather than reopen this path.
        std::fs::write(&path, "CHANGED_CURRENT_WORKSPACE").unwrap();

        let (mut view, bindings) = fixture.view(fixture.client().await, Some(fixture.session.clone()));
        view.resize(120, 40);
        wait_view(&mut view, |screen| screen.contains("Enter sends")).await;
        support::focus_activity(&mut view).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        support::click_button(&mut view, "Toggle tool WorkspaceDocument").await;
        let readable = wait_view(&mut view, |screen| screen.contains("Read lines") && screen.contains("起点_原始")).await;
        let text = view.accessibility_snapshot().into_iter()
            .find(|node| node.label.contains("1 │ 起点_原始") && node.label.contains("终点_原始"))
            .expect("the readable native Text must include both ends of the original paged result");
        assert!(!text.label.contains("CHANGED_CURRENT_WORKSPACE"));
        evidence("tool-read-paged-original", &readable);

        // This switch shares the mounted reader; the literal JSON is still the exact result.
        support::click_button(&mut view, "Toggle original tool details").await;
        wait_view(&mut view, |screen| screen.contains("Original result")).await;
        let raw = view.accessibility_snapshot().into_iter().find_map(|node| {
            serde_json::from_str::<Value>(&node.label).ok().filter(|value| value == &result)
        });
        assert_eq!(raw, Some(result));
        view.resize(80, 24);
        support::focus_activity(&mut view).await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.contains("v WorkspaceDocument")).await;
        tab_to_label(&mut view, "Toggle tool WorkspaceDocument").await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        let collapsed = wait_view(&mut view, |screen| screen.contains("> WorkspaceDocument") && !screen.contains("Original result")).await;
        evidence("tool-read-narrow-collapsed", &collapsed);
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.contains("Original result")).await;
        support::click_button(&mut view, "Toggle original tool details").await;
        let narrow = wait_view(&mut view, |screen| screen.contains("Read lines") && screen.contains("起点_原始")).await;
        evidence("tool-read-narrow-reopened", &narrow);
        view.resize(120, 40);
        continuation.delta("TOOL_CARD_TURN_CONTINUES").await;
        continuation.finish().await;
        fixture.idle(&client, &fixture.session).await;
        assert!(fixture.store.pending_interactions(&fixture.session).await.unwrap().is_empty());
        drop(view);
        bindings.close().await.unwrap();
        client.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("real Host readable tool card workflow timed out");
}
