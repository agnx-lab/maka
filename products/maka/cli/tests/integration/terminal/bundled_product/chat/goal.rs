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

fn open(tui: &mut Pty) {
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"\x1b[F");
    tui.send(&[0x7f; 24]);
    tui.send(b"Goal");
    tui.wait_for("› Goal");
    tui.send(b"\r");
}
fn edit_below(tui: &mut Pty, label: &str, value: &str) {
    tui.wait_for(label);
    let screen = tui.screen.preview(usize::MAX).unwrap().screen;
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(label))
        .unwrap();
    let column = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 3;
    // Enter the content row inside the ordinary native field's border.
    tui.send(
        format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            column,
            row + 3,
            column,
            row + 3
        )
        .as_bytes(),
    );
    tui.send(b"\x1b[F");
    tui.send(&[0x7f; 12]);
    paste(tui, value);
}
#[test]
#[ignore = "real default product Goal approval, tool continuation, pause and process restart"]
fn default_product_goal_preserves_approval_and_resumes_original_objective_after_restart() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 4, 1);
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let ui = directory.path().join("ui");
    let host_guard = HostRootGuard::new(&ui.join("host"));
    let namespaces = RootNamespaces::for_current_account().unwrap();
    drop(RootOwner::create(&ui, &namespaces).unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(seed_connection(
        &ui.join("host"),
        &namespaces,
        &base,
        json!({}),
    ));
    let mut tui = Pty::spawn(&[
        "application",
        "launch",
        "--root",
        ui.to_str().unwrap(),
        "--workspace",
        workspace.to_str().unwrap(),
        "--trust-workspace",
        "--locale",
        "en",
    ]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"New conversation");
    tui.wait_for("› New conversation");
    tui.send(b"\r");
    tui.wait_for("Message…");
    click(&mut tui, "Message…");
    paste(&mut tui, "Retain this independent draft.");
    tui.wait_for("Retain this independent draft.");
    open(&mut tui);
    tui.wait_for("Objective");
    edit_below(&mut tui, "Objective", "Verify the Goal workflow 中😀");
    tui.wait_for("Verify the Goal workflow 中😀");
    edit_below(&mut tui, "Iterations", "2");
    click(&mut tui, "Save for later");
    tui.wait_for("Confirm Goal");
    tui.wait_for("fixture-model");
    tui.wait_for("Iterations: 2");
    evidence(&tui, "goal-approval");
    assert!(received.try_recv().is_err());
    click(&mut tui, "Confirm Goal");
    tui.wait_for("Ready to start");
    click(&mut tui, "Close");
    tui.wait_for("Retain this independent draft.");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    open(&mut tui);
    tui.wait_for("Verify the Goal workflow 中😀");
    tui.wait_for("Ready to start");
    click(&mut tui, "Resume Goal");
    click(&mut tui, "Confirm Goal");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(sent.to_string().contains("Verify the Goal workflow 中😀"));
    assert!(
        sent["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "GoalStatus")
    );
    // HTTP admission can precede the watch frame that adds the pending notice.
    // Click only after the visible controls have reached that execution phase.
    tui.wait_for("The current iteration is still settling.");
    evidence(&tui, "goal-before-pause");
    click(&mut tui, "Pause Goal");
    tui.wait_for("Paused");
    code_mode::tool_response(
        &mut socket,
        "GoalStatus",
        json!({"status":"progress","note":"Checkpoint attempted after user pause"}),
    );
    let (_, mut socket) = next_request(&mut tui, &received);
    delta(&mut socket, "The first iteration has settled.");
    complete(&mut socket);
    tui.wait_until(|s| s.contains("Paused") && !s.contains("still settling"));
    assert!(received.try_recv().is_err());
    evidence(&tui, "goal-paused");
    click(&mut tui, "Resume Goal");
    click(&mut tui, "Confirm Goal");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(sent.to_string().contains("Verify the Goal workflow 中😀"));
    code_mode::tool_response(
        &mut socket,
        "GoalStatus",
        json!({"status":"achieved","note":"Verified the original Goal after restart"}),
    );
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(sent.to_string().contains("recorded"));
    delta(&mut socket, "Goal verification complete.");
    complete(&mut socket);
    tui.wait_for("Achieved");
    tui.wait_for("2/2");
    evidence(&tui, "goal-achieved");
    click(&mut tui, "Close");
    tui.wait_for("Retain this independent draft.");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
}
