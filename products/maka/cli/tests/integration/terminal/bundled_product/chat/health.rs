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

fn result(request: &serde_json::Value) -> serde_json::Value {
    let message = request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .unwrap();
    serde_json::from_str(message["content"].as_str().unwrap()).unwrap()
}

fn answer_health(tui: &mut Pty, endpoint: &TcpListener) -> std::net::TcpStream {
    let mut connection = None;
    tui.wait_until_with_timeout(
        |_| {
            connection = endpoint.accept().ok();
            connection.is_some()
        },
        Duration::from_secs(30),
    );
    let (mut endpoint_socket, _) = connection.unwrap();
    endpoint_socket.set_nonblocking(false).unwrap();
    endpoint_socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut head = vec![];
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        endpoint_socket.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
        assert!(head.len() < 8192);
    }
    assert!(head.starts_with(b"HEAD /health "));
    endpoint_socket
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 999999\r\nConnection: close\r\n\r\n")
        .unwrap();
    endpoint_socket
}

#[test]
#[ignore = "real default product process health, permission approval and restart"]
fn default_product_health_checks_process_and_independent_endpoint() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let endpoint = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/health", endpoint.local_addr().unwrap());
    endpoint.set_nonblocking(true).unwrap();
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 9, 1);
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let ui = directory.path().join("ui");
    let host = ui.join("host");
    let host_guard = HostRootGuard::new(&host);
    let namespaces = RootNamespaces::for_current_account().unwrap();
    drop(RootOwner::create(&ui, &namespaces).unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(seed_connection(
        &host,
        &namespaces,
        &base,
        json!({"fixture-model":{"codeMode":false}}),
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
    paste(&mut tui, "Check the tracked process and its endpoint.");
    tui.send(b"\r");
    let (_, mut socket) = next_request(&mut tui, &received);
    code_mode::tool_response(
        &mut socket,
        "tool_search",
        json!({"query":"Shell BackgroundTaskHealth"}),
    );
    let (catalog, mut socket) = next_request(&mut tui, &received);
    for name in ["Shell", "BackgroundTaskHealth"] {
        assert!(
            catalog["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["function"]["name"] == name),
            "{catalog}"
        );
    }
    code_mode::tool_response(
        &mut socket,
        "Shell",
        json!({"command":"printf product-health","login":false,"run_in_background":true,"timeout_ms":5000}),
    );
    let (shell, mut socket) = next_request(&mut tui, &received);
    let shell = result(&shell);
    assert_eq!(shell["kind"], "completed", "{shell}");
    let reference = shell["value"]["ref"].clone();
    assert!(reference.is_string());
    code_mode::tool_response(
        &mut socket,
        "BackgroundTaskHealth",
        json!({"ref":reference,"url":url}),
    );
    tui.wait_for("Approval needed");
    evidence(&tui, "composer-approval");
    click(&mut tui, "Approval needed");
    tui.wait_for("Allow health check access to");
    click(&mut tui, "Deny this call");
    let (denied, mut socket) = next_request(&mut tui, &received);
    let denied = result(&denied);
    assert_eq!(denied["value"]["endpoint"]["state"], "unknown", "{denied}");
    assert_eq!(
        endpoint.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    click(&mut tui, "Close");
    tui.wait_until(|s| {
        !s.contains("Allow health check access to")
            && s.contains("Message…")
            && !s.contains("Approval needed")
    });
    evidence(&tui, "composer-settled");
    code_mode::tool_response(
        &mut socket,
        "BackgroundTaskHealth",
        json!({"ref":reference,"url":url,"include_logs":true}),
    );
    tui.wait_for("Approval needed");
    click(&mut tui, "Approval needed");
    tui.wait_for("Allow health check access to");
    click(&mut tui, "This conversation");
    tui.wait_for("Allow · This conversation");
    click(&mut tui, "Allow · This conversation");
    // Close immediately while the approved endpoint is still unanswered. The
    // physical activation must survive the approval's logical commit, and
    // dismissal must not wait for the original service action to finish.
    let closing = Instant::now();
    click(&mut tui, "Close");
    tui.wait_until(|s| !s.contains("Allow health check access to") && s.contains("Message…"));
    eprintln!("approval Close to dismissed frame: {:?}", closing.elapsed());
    let mut endpoint_socket = answer_health(&mut tui, &endpoint);
    let (checked, mut socket) = next_request(&mut tui, &received);
    let checked = result(&checked);
    assert_eq!(
        checked["value"]["endpoint"]["health"], "healthy",
        "{checked}"
    );
    assert_eq!(
        checked["value"]["process"]["logs"]["stdout"],
        "product-health"
    );
    assert_eq!(endpoint_socket.read(&mut [0; 1]).unwrap(), 0);
    delta(
        &mut socket,
        "Process finished; endpoint independently responded.",
    );
    complete(&mut socket);
    tui.wait_for("Process finished; endpoint independently responded.");
    evidence(&tui, "health-completed");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    tui.wait_until_with_timeout(
        |s| s.contains("Process finished; endpoint independently responded."),
        Duration::from_secs(60),
    );
    tui.wait_until(|s| s.contains("Message…") && !s.contains("Restoring draft…"));
    click(&mut tui, "Message…");
    paste(&mut tui, "Read the original process health after restart.");
    tui.send(b"\r");
    let (_, mut socket) = next_request(&mut tui, &received);
    code_mode::tool_response(
        &mut socket,
        "tool_search",
        json!({"query":"BackgroundTaskHealth"}),
    );
    let (_, mut socket) = next_request(&mut tui, &received);
    code_mode::tool_response(
        &mut socket,
        "BackgroundTaskHealth",
        json!({"ref":reference,"include_logs":true}),
    );
    let (restored, mut socket) = next_request(&mut tui, &received);
    let restored = result(&restored);
    assert_eq!(restored["value"]["process"], checked["value"]["process"]);
    assert_eq!(restored["value"]["endpoint"]["state"], "not_checked");
    assert_eq!(
        endpoint.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    // Session approval must authorize a later invocation after process restart,
    // without another decision or silently widening the saved network boundary.
    code_mode::tool_response(
        &mut socket,
        "BackgroundTaskHealth",
        json!({"ref":reference,"url":url}),
    );
    let mut endpoint_socket = answer_health(&mut tui, &endpoint);
    let (rechecked, mut socket) = next_request(&mut tui, &received);
    let rechecked = result(&rechecked);
    assert_eq!(rechecked["value"]["endpoint"]["health"], "healthy");
    assert_eq!(endpoint_socket.read(&mut [0; 1]).unwrap(), 0);
    assert!(
        !tui.screen
            .preview(usize::MAX)
            .unwrap()
            .screen
            .contains("Approval needed")
    );
    delta(&mut socket, "Original process logs survived restart.");
    complete(&mut socket);
    tui.wait_for("Original process logs survived restart.");
    evidence(&tui, "health-restarted");
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
}
