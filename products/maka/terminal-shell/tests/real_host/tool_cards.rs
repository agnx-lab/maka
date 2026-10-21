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
        // Cross the 8192 UTF-8-byte source boundary while staying inside the
        // real tool's 7500 UTF-16-unit model page, including its JSON envelope.
        let line = format!("{}😀\n", "中文".repeat(10));
        let original = format!("起点_原始\n{}终点_原始\n", line.repeat(200));
        assert!(original.len() > 8192, "the real result must require multiple source pages");
        std::fs::write(&path, &original).unwrap();
        let sent = support::mutate(&client, "send", json!({
            "sessionId":fixture.session, "message":"Read the recorded document", "model":fixture.selection,
        })).await;
        assert_eq!(sent["kind"], "started", "{sent}");
        fixture.next_request().await.tool("WorkspaceDocument", json!({"path":"recorded.txt"})).await;
        let mut continuation = fixture.next_request().await;
        let tool_result = continuation.body["messages"].as_array().unwrap()
            .iter().rev().find(|message| message["role"] == "tool").unwrap()["content"]
            .as_str().unwrap();
        let result: Value = serde_json::from_str(tool_result)
            .unwrap_or_else(|error| panic!("actual readText tool result was not JSON: {error}: {tool_result}"));
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

        // Large originals scroll inside the card while its disclosure and
        // original-details action remain visible in the turn's own viewport.
        support::scroll_snapshot_text(&mut view, &readable, "起点_原始", MouseEventKind::ScrollDown);
        wait_view(&mut view, |screen| screen.contains("v WorkspaceDocument") && screen.contains("Original details") && !screen.contains("起点_原始")).await;
        tab_to_label(&mut view, "Tool details").await;
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
        let tail = wait_view(&mut view, |screen| screen.contains("终点_原始")).await;
        assert!(tail.contains("v WorkspaceDocument") && tail.contains("Original details"));
        evidence("tool-read-local-scroll-tail", &tail);
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.contains("Read lines") && screen.contains("起点_原始")).await;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tool_cards_show_recorded_search_changes_and_business_failures_without_inventing_diffs() {
    tokio::time::timeout(Duration::from_secs(90), async {
        for (method, input, marker) in [
            ("grep", json!({"pattern":"needle","path":"."}), "Matching lines"),
            ("write", json!({"path":"new.txt","content":"新文件_记录\n"}), "Recorded write"),
            ("edit", json!({"path":"original.txt","old_string":"needle","new_string":"替换_记录"}), "Recorded edit"),
            ("readText", json!({"path":"missing.txt"}), "Tool failed"),
        ] {
            let mut fixture = Fixture::files(method).await;
            let client = fixture.client().await;
            let path = fixture.directory.path().join("workspace/original.txt");
            std::fs::write(&path, "中文 needle\n").unwrap();
            let sent = support::mutate(&client, "send", json!({
                "sessionId":fixture.session,"message":"Use the selected Files method","model":fixture.selection,
            })).await;
            assert_eq!(sent["kind"], "started", "{method}: {sent}");
            fixture.next_request().await.tool("WorkspaceDocument", input).await;
            let continuation = fixture.next_request().await;
            let tool_result = continuation.body["messages"].as_array().unwrap()
                .iter().rev().find(|message| message["role"] == "tool").unwrap()["content"]
                .as_str().unwrap();
            let result: Value = serde_json::from_str(tool_result)
                .unwrap_or_else(|error| panic!("actual {method} tool result was not JSON: {error}: {tool_result}"));
            if method == "readText" {
                assert_eq!(result["kind"], "failed", "{result}");
            } else {
                assert_eq!(result["kind"], "completed", "{method}: {result}");
            }
            if method == "write" {
                assert_eq!(std::fs::read_to_string(fixture.directory.path().join("workspace/new.txt")).unwrap(), "新文件_记录\n");
            } else if method == "edit" {
                assert_eq!(std::fs::read_to_string(&path).unwrap(), "中文 替换_记录\n");
            }
            // Results are historical facts, including a real business-level failure.
            std::fs::write(&path, "LATER_WORKSPACE_CONTENT").unwrap();
            let (mut view, bindings) = fixture.view(fixture.client().await, Some(fixture.session.clone()));
            view.resize(120, 40);
            wait_view(&mut view, |screen| screen.contains("Enter sends")).await;
            support::focus_activity(&mut view).await;
            view.handle_event(Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
            support::click_button(&mut view, "Toggle tool WorkspaceDocument").await;
            let screen = wait_view(&mut view, |screen| screen.contains(marker)).await;
            let labels = view.accessibility_snapshot().into_iter().map(|node| node.label).collect::<Vec<_>>();
            assert!(!labels.iter().any(|label| label.contains("LATER_WORKSPACE_CONTENT")));
            match method {
                "grep" => assert!(labels.iter().any(|label| label.contains("中文 needle"))),
                "write" => {
                    assert!(labels.iter().any(|label| label == "Requested file contents"));
                    assert!(labels.iter().any(|label| label == "新文件_记录\n"));
                    assert!(!labels.iter().any(|label| label == "Applied diff"));
                }
                "edit" => {
                    assert!(labels.iter().any(|label| label == "Requested removal"));
                    assert!(labels.iter().any(|label| label == "Requested replacement"));
                    assert!(!labels.iter().any(|label| label == "Applied diff"));
                }
                "readText" => {
                    assert!(labels.iter().any(|label| label == result["message"].as_str().unwrap()));
                    assert!(labels.iter().any(|label| label == "Error"));
                }
                _ => unreachable!(),
            }
            evidence(&format!("tool-card-{method}"), &screen);
            continuation.finish().await;
            fixture.idle(&client, &fixture.session).await;
            drop(view);
            bindings.close().await.unwrap();
            client.close().await.unwrap();
            fixture.finish().await;
        }
    }).await.expect("actual Files tool card workflow timed out");
}
