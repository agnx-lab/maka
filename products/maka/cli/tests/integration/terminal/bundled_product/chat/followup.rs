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

#[test]
#[ignore = "real bundled product, active streamed model and physical follow-up input"]
fn active_run_accepts_unicode_followups_by_enter_and_queue_button() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 3, 0);
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let host = directory.path().join("host");
    let host_guard = HostRootGuard::new(&host);
    let ui = directory.path().join("ui");
    let configuration = directory.path().join("installation.json");
    let namespaces = RootNamespaces::for_current_account().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(seed_connection(&host, &namespaces, &base, json!({})));
    std::fs::write(
        &configuration,
        serde_json::to_vec(&json!({
            "presentation":"tui", "locale":"en", "scheme":"dark", "hostRoot":host,
            "agent":{
                "workspaceRoots":[{"label":"Workspace","path":workspace}], "tools":[],
                "automaticTitles":false,
                "permissions":{"access":"read_only","requestPermissions":true}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let mut tui = Pty::spawn(&[
        "application",
        "launch",
        "--root",
        ui.to_str().unwrap(),
        "--configuration",
        configuration.to_str().unwrap(),
    ]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"New conversation");
    tui.wait_for("› New conversation");
    tui.send(b"\r");
    tui.wait_until(|s| s.contains("Message…") && !s.contains("Restoring draft…"));
    click(&mut tui, "Message…");
    paste(&mut tui, "Start the original turn");
    tui.wait_for("Start the original turn");
    tui.send(b"\r");
    let (_, mut socket) = next_request(&mut tui, &received);
    for (followup, button) in [("多刷一会", false), ("再核实一次😀", true)] {
        delta(&mut socket, "Still working on the accepted turn.\n");
        tui.wait_until(|s| s.contains("Queue for next turn") && s.contains("Message…"));
        click(&mut tui, "Message…");
        let (stop_stream, streaming) = mpsc::channel();
        let stream = std::thread::spawn(move || {
            // Keep actual model progress arriving throughout both input paths.
            // The controller stops the producer only after queue admission;
            // receive timeouts define source cadence, not assertion timing.
            let deadline = Instant::now() + Duration::from_secs(45);
            loop {
                match streaming.recv_timeout(Duration::from_millis(50)) {
                    Ok(()) => return socket,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        assert!(Instant::now() < deadline, "follow-up admission stalled");
                        delta(&mut socket, ".");
                    }
                    Err(error) => panic!("follow-up controller stopped: {error}"),
                }
            }
        });
        // An IME commits Unicode as terminal text. Keep this separate from the
        // bracketed-paste path used to seed the first turn.
        tui.send(followup.as_bytes());
        tui.wait_for(followup);
        let snapshot = tui.screen.preview(usize::MAX).unwrap();
        let (row, line) = snapshot
            .screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(followup))
            .unwrap();
        let end = line.find(followup).unwrap() + followup.len();
        assert!(snapshot.cursor.visible);
        assert_eq!(snapshot.cursor.y as usize, row);
        assert_eq!(snapshot.cursor.x as usize, line[..end].width());
        if button {
            evidence(&tui, "followup-before-click");
            click(&mut tui, "Queue for next turn");
        } else {
            tui.send(b"\r");
        }
        tui.wait_until(|s| {
            s.contains(followup) && s.contains("Next turn") && s.contains("Message…")
        });
        evidence(
            &tui,
            if button {
                "followup-click-queued"
            } else {
                "followup-enter-queued"
            },
        );
        assert!(
            received.try_recv().is_err(),
            "queued work must wait for its original turn"
        );
        stop_stream.send(()).unwrap();
        socket = stream.join().unwrap();
        complete(&mut socket);
        let (sent, next) = next_request(&mut tui, &received);
        assert_eq!(sent["model"], "fixture-model");
        assert!(
            sent["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| { message["role"] == "user" && message["content"] == followup }),
            "queued follow-up must reach the successor invocation: {sent}"
        );
        socket = next;
    }
    delta(&mut socket, "Both queued follow-ups were consumed.");
    complete(&mut socket);
    tui.wait_until(|s| {
        s.contains("Both queued follow-ups were consumed.")
            && s.contains("Send")
            && !s.contains("Queue for next turn")
    });
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
    assert!(!tui.screen.preview(usize::MAX).unwrap().alternate_screen);
    drop(RootOwner::open(&host, &namespaces).unwrap());
}
