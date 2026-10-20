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
    tui.wait_until(|screen| !screen.contains("› Plan"));
    tui.send(b"Plan");
    click(tui, "› Plan");
}

#[test]
#[ignore = "real default Plan review, approved native effect, history and process restart"]
fn default_product_plan_requires_review_and_restores_frozen_proposal_and_completion() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 3, 1);
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
        json!({"fixture-model":{"codeMode":true,"applyPatch":false}}),
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
    open(&mut tui);
    click(&mut tui, "Switch to Planning mode");
    tui.wait_for("Switch to Agent mode");
    click(&mut tui, "Close");
    click(&mut tui, "Message…");
    paste(&mut tui, "Plan a reviewed file change 中😀");
    tui.send(b"\r");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(sent.to_string().contains("Inspect and discuss only"));
    assert!(
        sent["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["function"]["name"] == "SubmitPlan")
    );
    code_mode::tool_response(
        &mut socket,
        "SubmitPlan",
        json!({
            "title":"Reviewed file change 中😀", "overview":"Write only after approval.",
            "steps":[{"id":"write","title":"Write the file","description":"Create reviewed.txt with the approved content.","files":["reviewed.txt"],"complexity":"low"}],
            "risks":["The file must not exist before approval."]
        }),
    );
    tui.wait_for("Awaiting approval");
    assert!(!workspace.join("reviewed.txt").exists());
    assert!(received.try_recv().is_err());
    tui.wait_until(|s| !s.contains("Restoring draft"));
    click(&mut tui, "Message…");
    paste(&mut tui, "Keep this independent draft.");
    tui.wait_for("Keep this independent draft.");
    open(&mut tui);
    tui.wait_for("Reviewed file change 中😀");
    evidence(&tui, "plan-proposed");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();

    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    open(&mut tui);
    tui.wait_for("Reviewed file change 中😀");
    tui.wait_for("Awaiting approval");
    click(&mut tui, "Approve and execute");
    tui.wait_for("Confirm Plan action");
    tui.wait_for("fixture-model");
    evidence(&tui, "plan-approval");
    assert!(received.try_recv().is_err());
    assert!(!workspace.join("reviewed.txt").exists());
    click(&mut tui, "Confirm Plan action");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(sent.to_string().contains("user-approved Plan"));
    assert!(sent.to_string().contains("Reviewed file change 中😀"));
    code_mode::tool_response(
        &mut socket,
        "exec",
        json!({"code":
            "const result = await tools.Write({path:'reviewed.txt',content:'Only the approved Plan writes this'}); if (result.kind !== 'completed') throw Error('write failed'); text(result); text(await tools.update_plan({steps:[{id:'write',status:'completed',note:'Verified the native write result'}]}));"
        }),
    );
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(sent.to_string().contains("recorded"), "{sent}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("reviewed.txt")).unwrap(),
        "Only the approved Plan writes this"
    );
    delta(&mut socket, "The reviewed Plan is complete.");
    complete(&mut socket);
    tui.wait_for("Plan completed");
    evidence(&tui, "plan-completed");
    click(&mut tui, "Plan history");
    tui.wait_for("Previous revisions");
    tui.wait_for("Awaiting approval");
    assert!(
        !tui.screen
            .preview(usize::MAX)
            .unwrap()
            .screen
            .contains("Approve and execute")
    );
    evidence(&tui, "plan-history");
    click(&mut tui, "Close");
    tui.wait_for("Keep this independent draft.");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    open(&mut tui);
    tui.wait_for("Plan completed");
    tui.wait_for("Reviewed file change 中😀");
    evidence(&tui, "plan-restarted");
    assert!(received.try_recv().is_err());
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
}
