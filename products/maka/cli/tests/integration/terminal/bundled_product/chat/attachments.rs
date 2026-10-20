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
use base64::{Engine as _, engine::general_purpose::STANDARD};

// The same valid 1x1 GIF used by Native image fixtures; changing one global
// palette channel gives distinct original encoded bytes without padding.
fn gif(index: u8) -> Vec<u8> {
    let mut bytes = vec![
        71, 73, 70, 56, 57, 97, 1, 0, 1, 0, 128, 0, 0, 0, 0, 0, 255, 255, 255, 33, 249, 4, 1, 0, 0,
        0, 0, 44, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 68, 1, 0,
    ];
    bytes[13] = index + 1;
    bytes.push(0x3b);
    bytes
}

#[test]
#[ignore = "real bundled product, nine pasted image uploads, vision HTTP and foreground PTY"]
fn nine_pasted_images_reach_the_actual_model_in_original_order_and_clear_the_draft() {
    const PROMPT: &str = "Inspect nine original images";
    const LIVE: &str = "Nine original images arrived.";
    const FINAL: &str = "All nine image uploads are complete.";
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 1, 0);
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let pictures: Vec<_> = (0..9).map(gif).collect();
    let paths: Vec<_> = pictures
        .iter()
        .enumerate()
        .map(|(index, bytes)| {
            let path = workspace.join(format!("original-{index}.gif"));
            std::fs::write(&path, bytes).unwrap();
            path
        })
        .collect();
    let host = directory.path().join("host");
    let host_guard = HostRootGuard::new(&host);
    let ui = directory.path().join("ui");
    let configuration = directory.path().join("installation.json");
    let namespaces = RootNamespaces::for_current_account().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(seed_connection(
        &host,
        &namespaces,
        &base,
        json!({"fixture-model":{"vision":true}}),
    ));
    std::fs::write(
        &configuration,
        serde_json::to_vec(&json!({
            "presentation":"tui","locale":"en","scheme":"dark","hostRoot":host,
            "agent":{
                "workspaceRoots":[{"label":"Workspace","path":workspace}],"tools":[],
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
    // One actual multi-file paste, consumed sequentially by the original native
    // selection/upload/Next loop. No direct service upload bypasses the composer.
    paste(
        &mut tui,
        &paths
            .iter()
            .map(|path| path.to_str().unwrap())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    tui.wait_until(|s| {
        s.lines().any(|line| {
            line.contains("original-0.gif") && line.contains("+8") && line.contains("Ready")
        }) && s.contains("Message…")
            && !s.contains("Attaching files…")
            && !s.contains("Skip file")
    });
    // Opening this control proves the last upload/Next completed and the
    // attachment action became usable; nine Ready rows need not fit the PTY.
    click(&mut tui, "original-0.gif");
    tui.wait_until(|s| s.contains("original-0.gif") && s.contains("Ready") && s.contains("Close"));
    evidence(&tui, "nine-images-attached");
    click(&mut tui, "Close");
    tui.wait_until(|s| {
        s.contains("Message…")
            && s.contains("original-0.gif")
            && s.contains("+8")
            && !s.contains("Close")
    });
    click(&mut tui, "Message…");
    paste(&mut tui, PROMPT);
    tui.wait_for(PROMPT);
    tui.send(b"\r");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert_eq!(sent["model"], "fixture-model");
    assert!(!is_title(&sent));
    let parts: Vec<_> = sent["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_array())
        .flatten()
        .collect();
    assert!(parts.iter().any(|part| {
        part["text"]
            .as_str()
            .is_some_and(|text| text.contains(PROMPT))
    }));
    let images: Vec<_> = parts
        .iter()
        .filter(|part| part["type"] == "image_url")
        .collect();
    assert_eq!(
        images.len(),
        9,
        "all nine original files must be actual image parts"
    );
    for (index, (part, bytes)) in images.iter().zip(&pictures).enumerate() {
        let expected = format!("data:image/gif;base64,{}", STANDARD.encode(bytes));
        assert!(
            part["image_url"]["url"] == expected,
            "original image bytes/order differ at {index}"
        );
    }
    delta(&mut socket, LIVE);
    tui.wait_for(LIVE);
    evidence(&tui, "nine-images-live");
    delta(&mut socket, &format!("\n\n{FINAL}"));
    complete(&mut socket);
    tui.wait_until(|s| {
        s.contains(FINAL)
            && s.contains("Send")
            && s.contains("Message…")
            && !s.contains("Queue for next turn")
            && !s
                .lines()
                .any(|line| line.contains(".gif") && line.contains("Ready"))
            && s.lines().filter(|line| line.contains(PROMPT)).count() == 1
    });
    evidence(&tui, "nine-images-completed-draft-cleared");
    assert!(
        received.try_recv().is_err(),
        "only one Main request is expected"
    );
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
    assert!(!tui.screen.preview(usize::MAX).unwrap().alternate_screen);
    drop(RootOwner::open(&host, &namespaces).unwrap());
}
