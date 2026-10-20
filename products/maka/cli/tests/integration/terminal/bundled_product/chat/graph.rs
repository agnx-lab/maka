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
use maka_host_runtime::application::{host::Client, transport::wire::Reply};
use maka_plugin_core::operation::{OperationCleanup, OperationOutcome, Receipt};

async fn fixture_call(
    client: &Client,
    endpoint: &str,
    method: &str,
    input: serde_json::Value,
) -> serde_json::Value {
    let mut call = client
        .call(endpoint, method.into(), input, Default::default())
        .await
        .unwrap();
    call.start().await.unwrap();
    let value = match call.receive().await.unwrap() {
        Reply::Finished {
            result: Ok(value),
            cleanup: Ok(()),
        } => value,
        Reply::Prepared { .. } => {
            let Reply::Mutation {
                reply,
                cleanup: Ok(()),
            } = call.confirm().await.unwrap()
            else {
                panic!("{endpoint}.{method}: mutation did not finish with cleanup");
            };
            let Receipt::Recorded {
                outcome: OperationOutcome::Returned { value },
                cleanup: OperationCleanup::Confirmed,
            } = reply.receipt
            else {
                panic!("{endpoint}.{method}: original mutation was not recorded and returned");
            };
            value
        }
        reply => panic!("{endpoint}.{method}: unexpected reply {reply:?}"),
    };
    assert!(call.cleanup_confirmed());
    value
}

fn graph_tool_response(socket: &mut TcpStream, name: &str, input: serde_json::Value) {
    let input = if matches!(
        name,
        "agent_list" | "view_agent_graph" | "update_agent_graph" | "workhub_tasks"
    ) {
        json!({"request": input})
    } else {
        input
    };
    code_mode::tool_response(socket, name, input);
}
fn command(tui: &mut Pty, name: &str) {
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"\x1b[F");
    tui.send(&[0x7f; 40]);
    tui.send(name.as_bytes());
    tui.wait_for(&format!("› {name}"));
    tui.send(b"\r");
}
fn value(input: &serde_json::Value) -> serde_json::Value {
    let message = input["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|message| message["role"] == "tool")
        .unwrap();
    serde_json::from_str(message["content"].as_str().unwrap()).unwrap()
}

#[test]
#[ignore = "real default WorkHub graph delegation, multiple coordinator conversations and epoch recovery"]
fn default_product_graph_delegates_and_keeps_separate_conversation_epochs() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 8, 0);
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let ui = directory.path().join("ui");
    let host_guard = HostRootGuard::new(&ui.join("host"));
    let namespaces = RootNamespaces::for_current_account().unwrap();
    drop(RootOwner::create(&ui, &namespaces).unwrap());
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let connection_id = runtime.block_on(seed_connection(
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
    let second_title = "Graph fixture second conversation";
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            let discovery =
                maka_host_runtime::application::host::discovery::read(&ui.join("host")).unwrap();
            let mut client = Client::connect(&discovery.endpoint, Some(discovery.host_epoch))
                .await
                .unwrap();
            client.catalogue().select("coordinator").unwrap();
            client.catalogue().select("workhub").unwrap();
            let directories = fixture_call(
                &client,
                "coordinator",
                "directories",
                json!({"kind":"roots"}),
            )
            .await;
            assert_eq!(directories["kind"], "roots");
            let workspace = workspace.canonicalize().unwrap();
            let directory = directories["roots"]
                .as_array()
                .unwrap()
                .iter()
                .find(|directory| Path::new(directory["path"].as_str().unwrap()) == workspace)
                .expect("fixture workspace is published by the real coordinator");
            let resolved = fixture_call(
                &client,
                "workhub",
                "resolve",
                json!({"workspace":directory["selection"]}),
            )
            .await;
            assert_eq!(resolved["kind"], "ready", "{resolved}");
            let first_id = resolved["session"]["id"].as_str().unwrap();
            let created = fixture_call(
                &client,
                "coordinator",
                "create",
                json!({
                    "title":second_title,"automaticTitle":false,"labels":[],
                    "workspace":{"kind":"session","sessionId":first_id}
                }),
            )
            .await;
            assert_eq!(created["kind"], "created", "{created}");
            let second_id = created["session"]["id"].as_str().unwrap();
            assert_ne!(first_id, second_id);
            let backend = json!({"kind":"model","connectionId":connection_id,
            "modelId":"fixture-model","thinkingLevel":null});
            // Persist the model before either screen can optimistically select it.
            for session_id in [first_id, second_id] {
                let selected = fixture_call(
                    &client,
                    "coordinator",
                    "selectBackend",
                    json!({
                        "sessionId":session_id,"backend":backend
                    }),
                )
                .await;
                assert_eq!(selected["kind"], "committed", "{selected}");
                assert_eq!(selected["session"]["id"], session_id);
                assert_eq!(selected["session"]["backend"], backend);
            }
            client.close().await.unwrap();
        })
        .await
        .expect("public Host graph fixture setup timed out");
    });
    command(&mut tui, "WorkHub");
    let setup_prompt = "Choose a working directory for the coordinator.";
    tui.wait_until(|s| {
        !s.contains("Close commands") && (s.contains("Message…") || s.contains(setup_prompt))
    });
    if tui
        .screen
        .preview(usize::MAX)
        .unwrap()
        .screen
        .contains(setup_prompt)
    {
        click(&mut tui, "Check result");
    }
    tui.wait_until(|s| s.contains("Message…") && !s.contains("Restoring draft…"));
    click(&mut tui, "Graph / Swarm");
    click(&mut tui, "Start Swarm");
    tui.wait_for("swarm #1");
    click(&mut tui, "Close");
    click(&mut tui, "Message…");
    paste(
        &mut tui,
        "Delegate and verify the original graph result 中😀",
    );
    tui.send(b"\r");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(sent.to_string().contains("Agent Swarm supervisor"));
    assert!(sent["tools"].to_string().contains("agent_list"));
    graph_tool_response(&mut socket, "agent_list", json!({"operation":"workers"}));
    let (sent, mut socket) = next_request(&mut tui, &received);
    let catalog = value(&sent);
    assert_eq!(catalog["workers"][0]["name"], "worker");
    let model = catalog["supervisorModel"]["model"].clone();
    assert_eq!(model["modelId"], "fixture-model");
    graph_tool_response(
        &mut socket,
        "agent_list",
        json!({"operation":"directories","worker":"worker","location":{"kind":"roots"}}),
    );
    let (sent, mut socket) = next_request(&mut tui, &received);
    let directories = value(&sent);
    let workspace = directories["roots"][0]["selection"].clone();
    assert!(workspace.is_object(), "{directories}");
    graph_tool_response(
        &mut socket,
        "update_agent_graph",
        json!({"operation":"add_work","work":[{"target":{"kind":"worker","worker":"worker","title":"Original graph child","workspace":workspace,"tools":[],"model":model},"instruction":"Produce the original delegated graph result."}]}),
    );
    let mut workers = 0;
    let mut read_result = false;
    let mut selected = false;
    loop {
        let (sent, mut socket) = next_request(&mut tui, &received);
        if !sent.to_string().contains("Agent Swarm supervisor") {
            workers += 1;
            assert_eq!(workers, 1);
            assert!(
                sent.to_string()
                    .contains("Produce the original delegated graph result.")
            );
            delta(&mut socket, "Original delegated result 中😀");
            complete(&mut socket);
        } else if selected {
            assert!(
                value(&sent)["update"]["finish"]["resultIds"].is_array(),
                "{sent}"
            );
            delta(&mut socket, "Graph workflow complete.");
            complete(&mut socket);
            break;
        } else if read_result {
            let graph = value(&sent);
            assert_eq!(graph["work"][0]["status"], "completed", "{graph}");
            let result = graph["work"][0]["resultId"].clone();
            graph_tool_response(
                &mut socket,
                "update_agent_graph",
                json!({"operation":"finish","resultIds":[result],"reason":"Verified original worker result"}),
            );
            selected = true;
        } else if sent["messages"]
            .to_string()
            .contains("has new durable worker observations")
        {
            graph_tool_response(
                &mut socket,
                "view_agent_graph",
                json!({"operation":"status"}),
            );
            read_result = true;
        } else {
            assert_eq!(value(&sent)["revision"], 1, "{sent}");
            graph_tool_response(&mut socket, "yield_agent_graph", json!({}));
        }
    }
    assert_eq!(workers, 1);
    tui.wait_for("Graph workflow complete.");
    click(&mut tui, "Graph / Swarm");
    click(&mut tui, "Read result");
    tui.wait_for("Original delegated result 中😀");
    evidence(&tui, "graph-original-result");
    click(&mut tui, "Epoch history");
    click(&mut tui, "View epoch swarm #1");
    tui.wait_for("Historical epoch · read only");
    assert!(
        !tui.screen
            .preview(usize::MAX)
            .unwrap()
            .screen
            .contains("Restart Swarm")
    );
    evidence(&tui, "graph-pinned-history");
    click(&mut tui, "Close");
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"\x1b[F");
    tui.send(&[0x7f; 40]);
    tui.send(second_title.as_bytes());
    tui.wait_for(&format!("› Conversation · {second_title}"));
    tui.send(b"\r");
    tui.wait_until(|s| {
        s.contains("What would you like to accomplish?")
            && s.contains("Message…")
            && !s.contains("Restoring draft…")
            && !s.contains("Close commands")
    });
    click(&mut tui, "Graph / Swarm");
    tui.wait_for("Mode: Ordinary conversation");
    click(&mut tui, "Start Graph");
    tui.wait_for("graph #1");
    click(&mut tui, "Stop epoch");
    tui.wait_for("Stopped");
    click(&mut tui, "Restart Swarm");
    tui.wait_for("swarm #2");
    evidence(&tui, "graph-second-conversation");
    assert!(received.try_recv().is_err());
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
    command(&mut tui, "WorkHub");
    tui.wait_until(|s| {
        s.contains("Message…") && !s.contains("Restoring draft…") && !s.contains("Close commands")
    });
    click(&mut tui, "Graph / Swarm");
    tui.wait_for("swarm #1");
    tui.wait_for("Selected final result");
    click(&mut tui, "Read result");
    tui.wait_for("Original delegated result 中😀");
    evidence(&tui, "graph-restarted-original");
    assert!(received.try_recv().is_err());
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
}
