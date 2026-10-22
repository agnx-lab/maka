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

fn link_node(view: &AppView, label: &str) -> AccessibleNode {
    // The text carrier may share the label; only the Link is an activation target.
    let mut links = view.accessibility_snapshot().into_iter().filter(|node| {
        node.role == nsh_core::AccessibilityRole::Link && node.label == label
    });
    let link = links
        .next()
        .unwrap_or_else(|| panic!("no native Link has label {label:?}"));
    assert!(links.next().is_none(), "ambiguous native Link {label:?}");
    assert!(!link.state.disabled, "native Link {label:?} is disabled");
    link
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn markdown_links_open_original_workspace_files_without_replacing_the_draft() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let mut fixture = Fixture::open().await;
        let client = fixture.client().await;
        let original = "REAL_LINK_CONTENT 中文 👩🏽‍💻 e\u{301}\r\nOriginal trailing spaces  \n";
        let directory = fixture.directory.path().join("workspace").join("docs");
        std::fs::create_dir(&directory).unwrap();
        let file = directory.join("linked 你好.txt");
        std::fs::write(&file, original).unwrap();
        let (mut view, bindings) = fixture.view(client.clone(), Some(fixture.session.clone()));
        wait_editable_composer(&mut view).await;
        let sent = support::mutate(&client, "send", json!({
            "sessionId":fixture.session,"message":"LINK_REQUEST","model":fixture.selection
        })).await;
        assert_eq!(sent["kind"], "started");
        let mut request = fixture.next_request().await;
        let source = "[**OPEN** *REPORT*](./docs/linked%20%E4%BD%A0%E5%A5%BD.txt#L2)\n\n[OPEN_REFERENCE][report]\n\n[UNSAFE_LINK](javascript:alert%281%29)\n\n[report]: ./docs/linked%20%E4%BD%A0%E5%A5%BD.txt\n";
        request.delta(source).await;
        request.finish().await;
        fixture.idle(&client, &fixture.session).await;
        wait_enabled_node(&mut view, "OPEN REPORT").await;
        let links = view.accessibility_snapshot().into_iter().filter(|node| node.role == nsh_core::AccessibilityRole::Link).collect::<Vec<_>>();
        assert_eq!(links.iter().filter(|node| node.label == "OPEN REPORT").count(), 1, "one styled Markdown link has one native target");
        wait_editable_composer(&mut view).await;
        let composer = view.surface().focused().unwrap();
        view.handle_event(Event::Paste("UNSENT_LINK_DRAFT 你好".into()));
        support::wait_draft(&client, &fixture.session, "UNSENT_LINK_DRAFT 你好").await;

        let unsafe_link = link_node(&view, "UNSAFE_LINK");
        support::click_node(&mut view, unsafe_link);
        wait_view(&mut view, |screen| screen.contains("Only HTTP(S) links") && screen.contains("Could not open link")).await;
        assert!(!view.accessibility_snapshot().iter().any(|node| node.label == "Close workspace preview"));
        let report = link_node(&view, "OPEN REPORT");
        let report_id = report.id;
        support::click_node(&mut view, report);
        wait_view(&mut view, |screen| screen.contains("REAL_LINK_CONTENT") && screen.contains("Original trailing spaces")).await;
        evidence("markdown-link-real-workspace-file", &view.snapshot());
        std::fs::write(&file, "REFRESHED_LINK_CONTENT 中文\n").unwrap();
        wait_view(&mut view, |screen| screen.contains("The workspace may have changed")).await;
        support::click_button(&mut view, "Refresh current file").await;
        wait_view(&mut view, |screen| screen.contains("REFRESHED_LINK_CONTENT") && !screen.contains("REAL_LINK_CONTENT")).await;
        support::click_button(&mut view, "Close workspace preview").await;
        wait_view_state(&mut view, |view, screen| screen.contains("UNSENT_LINK_DRAFT 你好") && !view.accessibility_snapshot().iter().any(|node| node.label == "Close workspace preview") && view.accessibility_snapshot().iter().any(|node| node.id == report_id && node.role == nsh_core::AccessibilityRole::Link && node.focused && node.label == "OPEN REPORT")).await;
        assert!(view.accessibility_snapshot().iter().any(|node| node.id == composer && node.label == "Message composer"));

        // Use the original native Tab/Enter route, not a synthetic handler call.
        wait_enabled_node(&mut view, "OPEN_REFERENCE").await;
        let reference = link_node(&view, "OPEN_REFERENCE");
        for _ in 0..=view.accessibility_snapshot().len() {
            if view.surface().focused() == Some(reference.id) {
                break;
            }
            view.handle_event(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
        }
        assert_eq!(view.surface().focused(), Some(reference.id), "native Tab must reach the original Link target");
        view.handle_event(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        wait_view(&mut view, |screen| screen.contains("REFRESHED_LINK_CONTENT")).await;
        support::click_button(&mut view, "Close workspace preview").await;
        wait_view_state(&mut view, |view, screen| screen.contains("UNSENT_LINK_DRAFT 你好") && !view.accessibility_snapshot().iter().any(|node| node.label == "Close workspace preview") && view.accessibility_snapshot().iter().any(|node| node.id == reference.id && node.role == nsh_core::AccessibilityRole::Link && node.focused && node.label == "OPEN_REFERENCE")).await;
        support::wait_draft(&client, &fixture.session, "UNSENT_LINK_DRAFT 你好").await;
        let observed = client.query("agent", "observe", json!({"sessionId":fixture.session})).await.unwrap();
        let history = client.query("agent", "history", json!({"sessionId":fixture.session,"through":observed["through"],"position":{"kind":"tail"},"view":"conversation"})).await.unwrap();
        let response = history["chunks"].as_array().unwrap().iter().find(|chunk| chunk["role"] == "assistant" && chunk["field"] == "body").unwrap();
        assert_eq!(response["text"], source, "rendered link labels never replace the original canonical source");
        assert_eq!(fixture.request_count.load(Ordering::SeqCst), 1, "opening links must not invoke a model");
        drop(view);
        bindings.close().await.unwrap();
        fixture.finish().await;
    }).await.expect("real Host Markdown link workflow timed out");
}
