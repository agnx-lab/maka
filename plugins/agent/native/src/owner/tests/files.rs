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
use maka_agent_facts::interaction::InteractionOutcome;
use maka_agent_facts::interaction::InteractionRecord;
use maka_agent_facts::interaction::InteractionRequest;

async fn fixture(method: &'static str) -> (TcpListener, Fixture, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture = Fixture::with_services(
        &format!("http://{}/v1", listener.local_addr().unwrap()),
        None,
        None,
        Some(service_tools::Setup::files(method)),
    )
    .await;
    if method == "applyPatch" {
        selection::edit(&fixture, |changes| {
            changes.model_overrides = Patch::Set(BTreeMap::from([(
                "fixture-model".into(),
                ModelOverride {
                    apply_patch: Some(true),
                    ..Default::default()
                },
            )]));
        })
        .await;
    }
    let session = fixture.create("Native workspace files").await;
    (listener, fixture, session)
}
pub(super) async fn change(
    f: &Fixture,
    session: &str,
    access: &str,
    ask: bool,
    revoke: bool,
) -> Value {
    let current = f
        .query("observe", json!({"sessionId":session}))
        .await
        .unwrap()["session"]
        .clone();
    f.mutation(
        2,
        "metadata",
        json!({"sessionId":session,"expectedRevision":current["revision"],
        "title":current["title"],"labels":current["labels"],"permissions":{
            "settings":{"access":access,"requestPermissions":ask},"revokeGrants":revoke
        }}),
        Uuid::new_v4(),
    )
    .await
    .unwrap()
}
pub(super) async fn call(
    listener: &TcpListener,
    f: &Fixture,
    session: &str,
    input: Value,
) -> Value {
    let started = f.send(session).await.unwrap();
    let (mut socket, _) = listener.accept().await.unwrap();
    let sent = request(&mut socket).await;
    assert_eq!(sent["tools"][0]["function"]["name"], "WorkspaceFile");
    tool_response(&mut socket, input).await;
    started
}
pub(super) async fn tool_response(socket: &mut TcpStream, input: Value) {
    let delta = json!({"id":"files-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"file-call","type":"function","function":{"name":"WorkspaceFile","arguments":input.to_string()}}]},"finish_reason":null}]});
    let body = format!(
        "data: {delta}\n\ndata: {{\"id\":\"files-model\",\"choices\":[{{\"index\":0,\"delta\":{{}},\"finish_reason\":\"tool_calls\"}}]}}\n\ndata: [DONE]\n\n"
    );
    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
}

pub(super) async fn finish(listener: &TcpListener, f: &Fixture) -> Value {
    let (mut socket, _) = listener.accept().await.unwrap();
    let sent = request(&mut socket).await;
    let tool = sent["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "tool")
        .unwrap();
    let result = serde_json::from_str(tool["content"].as_str().unwrap())
        .unwrap_or_else(|error| panic!("file tool result was not JSON: {error}: {tool}"));
    response(&mut socket).await;
    f.drained().await;
    result
}
pub(super) async fn pending(f: &Fixture, session: &str) -> InteractionRecord {
    let mut commits = f.store.subscribe_commits();
    loop {
        if let Some(record) = f
            .store
            .pending_interactions(session)
            .await
            .unwrap()
            .into_iter()
            .next()
        {
            assert!(matches!(
                record.request,
                InteractionRequest::Permissions { .. }
            ));
            return record;
        }
        if tokio::time::timeout(Duration::from_secs(10), commits.changed())
            .await
            .is_err()
        {
            let observed = f
                .query("observe", json!({"sessionId":session}))
                .await
                .unwrap();
            let history = f.query("history", json!({"sessionId":session,"through":observed["through"],"position":{"kind":"tail"}})).await;
            panic!("No file permission request; observed {observed}; history {history:?}");
        }
    }
}
pub(super) async fn answer(f: &Fixture, record: &InteractionRecord, scope: Option<&str>) -> Value {
    let InteractionRequest::Permissions { request, .. } = &record.request else {
        unreachable!()
    };
    let decision = scope.map_or(
        json!({"decision":"deny"}),
        |scope| json!({"decision":"allow","scope":scope,"permissions":request.permissions}),
    );
    f.mutation(
        2,
        "respond",
        json!({"sessionId":record.session_id,"requestId":record.request_id,
        "answer":{"kind":"permissions","decision":decision}}),
        Uuid::new_v4(),
    )
    .await
    .unwrap()
}
async fn close(f: &Fixture) {
    f.app.shutdown().await.unwrap();
    f.config.shutdown().await.unwrap();
}

#[tokio::test]
async fn native_read_text_pages_source_larger_than_one_mib() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let (listener, f, session) = fixture("readText").await;
        let source: String = (0..65_537)
            .map(|line| format!("row-{line:08}-data\n"))
            .collect();
        let source_bytes = source.len();
        std::fs::write(f.workspace.join("large-numbered.txt"), &source).unwrap();
        let mut ranges = Vec::new();
        for offset in [0, 32_768, 65_536, 65_538] {
            call(&listener, &f, &session,
                json!({"path":"large-numbered.txt","offset":offset,"limit":1})).await;
            ranges.push(finish(&listener, &f).await);
        }
        let line = "中文😀\\\"\t".repeat(2_000);
        let original = format!("prefix\n{line}\n{source}");
        std::fs::write(f.workspace.join("large-unicode.txt"), &original).unwrap();
        let mut input = json!({"path":"large-unicode.txt","offset":1,"limit":1});
        let mut pages = Vec::new();
        loop {
            call(&listener, &f, &session, input).await;
            let page = finish(&listener, &f).await;
            let next = page["value"]["next"].clone();
            pages.push(page);
            if next.is_null() { break; }
            input = next;
        }
        let first_next = pages[0]["value"]["next"].clone();
        let changed = if !first_next.is_null() {
            std::fs::write(f.workspace.join("large-unicode.txt"),
                original.replace("row-00065536-data", "row-99965536-data")).unwrap();
            call(&listener, &f, &session, first_next).await;
            Some(finish(&listener, &f).await)
        } else { None };
        call(&listener, &f, &session,
            json!({"path":"large-unicode.txt","offset":1,"limit":1})).await;
        let fresh = finish(&listener, &f).await;
        // All public result assertions follow real Host/Agent owner cleanup.
        close(&f).await;
        assert_eq!(source_bytes, 1_179_666);
        for (result, (offset, content, returned)) in ranges.iter().zip([
            (0, "row-00000000-data", 1),
            (32_768, "row-00032768-data", 1),
            (65_536, "row-00065536-data", 1),
            (65_538, "", 0),
        ]) {
            assert_eq!(result, &json!({"kind":"completed","value":{
                "content":content,"offset":offset,"returnedLines":returned,"totalLines":65_538,"next":null
            }}));
        }
        assert!(pages.len() > 1, "long Unicode line must exercise actual continuations: {pages:?}");
        let mut reconstructed = String::new();
        for (index, page) in pages.iter().enumerate() {
            assert_eq!(page["kind"], "completed", "{page}");
            assert!(serde_json::to_string(page).unwrap().encode_utf16().count()
                <= maka_agent_facts::read::MAX_PAGE_CHARS);
            let value = &page["value"];
            assert!(serde_json::to_string(value).unwrap().encode_utf16().count()
                <= maka_agent_facts::read::MAX_PAGE_CHARS);
            assert_eq!(value["offset"], 1);
            assert_eq!(value["totalLines"], 65_540);
            assert_eq!(value["partialLine"], true);
            assert_eq!(value["returnedLines"], usize::from(index + 1 == pages.len()));
            reconstructed.push_str(value["content"].as_str().unwrap());
        }
        assert!(reconstructed == line, "public continuations changed the original Unicode bytes");
        assert_eq!(changed, Some(json!({"kind":"failed","message":
            "The content changed since the previous page. Read the original path again"})));
        assert_eq!(fresh["kind"], "completed", "{fresh}");
        assert_eq!(fresh["value"]["content"], pages[0]["value"]["content"]);
        assert_ne!(fresh["value"]["next"], pages[0]["value"]["next"]);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_files_reuse_actual_read_search_write_edit_and_patch_executors() {
    tokio::time::timeout(Duration::from_secs(90), async {
        for (method, input, expected) in [
            ("readText", json!({"path":"original.txt"}), "alpha needle\n"),
            ("glob", json!({"pattern":"*.txt","cwd":"."}), "original.txt"),
            ("grep", json!({"pattern":"needle","path":"."}), "needle"),
            ("write", json!({"path":"original.txt","content":"changed\n"}), "changed\n"),
            ("edit", json!({"path":"original.txt","old_string":"alpha","new_string":"changed"}), "changed needle\n"),
            ("applyPatch", json!({"callId":"patch","operation":{"type":"create_file","path":"new.txt","diff":"+created\n"}}), "created"),
        ] {
            let (listener, f, session) = fixture(method).await;
            std::fs::write(f.workspace.join("original.txt"), "alpha needle\n").unwrap();
            call(&listener, &f, &session, input.clone()).await;
            let result = finish(&listener, &f).await;
            assert_eq!(result["kind"], "completed", "{method}: {result}");
            match method {
                "write" | "edit" => assert_eq!(std::fs::read_to_string(f.workspace.join("original.txt")).unwrap(), expected),
                "applyPatch" => assert_eq!(std::fs::read_to_string(f.workspace.join("new.txt")).unwrap(), expected),
                _ => assert!(result.to_string().contains(expected.trim()), "{method}: {result}"),
            }
            assert!(f.store.pending_interactions(&session).await.unwrap().is_empty());
            {
                let facts = service_tools::facts(&f, &session).await;
                assert!(facts.iter().any(|fact| matches!(fact, maka_agent_facts::event::Fact::ToolDispatched {presentation: Some(key), name, ..} if key == &format!("maka.workspace.Files@1/{method}") && name == "WorkspaceFile")));
                let through = *f.store.subscribe_commits().borrow();

                let page = f.query("history", json!({"sessionId":session,"through":through,"position":{"kind":"start"}})).await.unwrap();
                assert!(page["chunks"].as_array().unwrap().iter().any(|chunk| chunk["tool"]["call"]["presentation"] == format!("maka.workspace.Files@1/{method}")), "{page}");
                let call = page["chunks"].as_array().unwrap().iter().find(|chunk| chunk["role"] == "tool_call").unwrap();
                if matches!(method, "glob" | "grep") {
                    let pattern = f.query("source", json!({"sessionId":session,"through":through,
                        "source":{"kind":"row","position":call["position"]},"field":"arguments",
                        "path":["pattern"],"position":{"kind":"start"}})).await.unwrap();
                    assert_eq!(pattern["source"]["text"], if method == "glob" { "*.txt" } else { "needle" });
                    assert!(pattern["filePath"].is_null());
                    let arguments = f.query("source", json!({"sessionId":session,"through":through,
                        "source":{"kind":"row","position":call["position"]},"field":"arguments",
                        "path":[],"position":{"kind":"start"}})).await.unwrap();
                    assert_eq!(serde_json::from_str::<Value>(arguments["source"]["text"].as_str().unwrap()).unwrap(), input);
                    // Historical display must remain the recorded search, even after the workspace changes.
                    std::fs::write(f.workspace.join("original.txt"), "replacement without a match\n").unwrap();
                    std::fs::write(f.workspace.join("later.txt"), "later needle\n").unwrap();
                    for field in ["complete", if method == "glob" { "files" } else { "matches" }] {
                        let source = f.query("source", json!({"sessionId":session,"through":through,
                            "source":{"kind":"row","position":call["tool"]["result"]["position"]},
                            "field":"result","path":["value",field],"position":{"kind":"start"}})).await.unwrap();
                        assert_eq!(serde_json::from_str::<Value>(source["source"]["text"].as_str().unwrap()).unwrap(), result["value"][field]);
                        assert!(source["filePath"].is_null());
                    }
                } else {
                let path = if method == "applyPatch" { json!(["operation", "path"]) } else { json!(["path"]) };
                let source = f.query("source", json!({"sessionId":session,"through":through,
                    "source":{"kind":"row","position":call["position"]},"field":"arguments",
                    "path":path,"position":{"kind":"start"}})).await.unwrap();
                assert_eq!(source["filePath"], f.workspace.join(if method == "applyPatch" { "new.txt" } else { "original.txt" }).to_str().unwrap());
                if matches!(method, "write" | "edit") {
                    let source = f.query("source", json!({"sessionId":session,"through":through,
                        "source":{"kind":"row","position":call["tool"]["result"]["position"]},
                        "field":"result","path":["value", "path"],"position":{"kind":"start"}})).await.unwrap();
                    assert_eq!(source["filePath"], result["value"]["path"]);
                }
                }
            }
            close(&f).await;
        }
    }).await.unwrap();
}

#[tokio::test]
async fn native_file_permission_decisions_recover_first_winner_and_obey_turn_scope() {
    tokio::time::timeout(Duration::from_secs(90), async {
        for scope in [None, Some("once"), Some("turn"), Some("session")] {
            let (listener, f, session) = fixture("write").await;
            assert_eq!(
                change(&f, &session, "read_only", true, false).await["session"]["boundaryRevision"],
                1
            );
            call(
                &listener,
                &f,
                &session,
                json!({"path":"approved.txt","content":"first"}),
            )
            .await;
            let record = pending(&f, &session).await;
            assert!(!f.workspace.join("approved.txt").exists());
            let InteractionRequest::Permissions {
                base_revision,
                request,
                ..
            } = &record.request
            else {
                unreachable!()
            };
            assert_eq!(*base_revision, 1);
            assert_eq!(request.permissions.filesystem.len(), 1);
            assert_eq!(
                request.permissions.filesystem[0].path,
                f.workspace.join("approved.txt")
            );
            let first = answer(&f, &record, scope).await;
            assert_eq!(
                answer(
                    &f,
                    &record,
                    if scope.is_some() {
                        None
                    } else {
                        Some("session")
                    }
                )
                .await,
                first
            );
            let result = finish(&listener, &f).await;
            assert_eq!(
                result["kind"],
                if scope.is_some() {
                    "completed"
                } else {
                    "failed"
                },
                "{result}"
            );
            assert_eq!(f.workspace.join("approved.txt").exists(), scope.is_some());
            if scope.is_some() {
                call(
                    &listener,
                    &f,
                    &session,
                    json!({"path":"approved.txt","content":"next turn"}),
                )
                .await;
                if scope != Some("session") {
                    let next = pending(&f, &session).await;
                    assert_ne!(next.request_id, record.request_id);
                    assert_eq!(
                        std::fs::read_to_string(f.workspace.join("approved.txt")).unwrap(),
                        "first"
                    );
                    answer(&f, &next, Some("once")).await;
                }
                assert_eq!(finish(&listener, &f).await["kind"], "completed");
                assert_eq!(
                    std::fs::read_to_string(f.workspace.join("approved.txt")).unwrap(),
                    "next turn"
                );
                // Session grants authorize exactly the requested file, never siblings.
                call(
                    &listener,
                    &f,
                    &session,
                    json!({"path":"sibling.txt","content":"no"}),
                )
                .await;
                let sibling = pending(&f, &session).await;
                answer(&f, &sibling, None).await;
                assert_eq!(finish(&listener, &f).await["kind"], "failed");
                assert!(!f.workspace.join("sibling.txt").exists());
            }
            close(&f).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn revocation_and_stop_close_waiting_file_requests_without_effects() {
    tokio::time::timeout(Duration::from_secs(90), async {
        for stop in [false, true] {
            let (listener, f, session) = fixture("write").await;
            change(&f, &session, "read_only", true, false).await;
            let started = call(&listener, &f, &session, json!({"path":"blocked.txt","content":"never"})).await;
            let record = pending(&f, &session).await;
            if stop {
                f.mutation(2, "stop", json!({"sessionId":session,"invocationId":started["invocationId"]}), Uuid::new_v4()).await.unwrap();
                f.drained().await;
            } else {
                // Unchanged settings preserve pending requests; explicit revoke advances the boundary.
                let unchanged = change(&f, &session, "read_only", true, false).await;
                assert_eq!(unchanged["session"]["boundaryRevision"], 1);
                assert!(f.store.interaction(&record.request_id).await.unwrap().unwrap().outcome.is_none());
                assert_eq!(change(&f, &session, "read_only", true, true).await["session"]["boundaryRevision"], 2);
                assert_eq!(finish(&listener, &f).await["kind"], "failed");
            }
            let late = answer(&f, &record, Some("session")).await;
            assert_eq!(late["record"]["outcome"]["kind"], "closure");
            assert!(matches!(f.store.interaction(&record.request_id).await.unwrap().unwrap().outcome, Some(InteractionOutcome::Closure { .. })));
            assert!(!f.workspace.join("blocked.txt").exists());
            assert!(f.store.pending_interactions(&session).await.unwrap().is_empty());
            close(&f).await;
        }
    }).await.unwrap();
}

#[tokio::test]
async fn native_files_never_grant_metadata_private_root_symlink_or_parent_escape() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let (listener, f, session) = fixture("write").await;
        let outside = f._dir.path().join("outside.txt");
        std::fs::write(&outside, "private").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, f.workspace.join("escape")).unwrap();
        std::fs::create_dir(f.workspace.join(".git")).unwrap();
        for path in [
            ".git/config".into(),
            ".maka/secret".into(),
            "../outside.txt".into(),
            "escape".into(),
            f.root.join("secret").to_str().unwrap().to_owned(),
        ] {
            call(
                &listener,
                &f,
                &session,
                json!({"path":path,"content":"never"}),
            )
            .await;
            let result = finish(&listener, &f).await;
            assert_eq!(result["kind"], "failed", "{path}: {result}");
            assert!(
                f.store
                    .pending_interactions(&session)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "private");
        assert!(!f.workspace.join(".git/config").exists());
        change(&f, &session, "read_only", false, false).await;
        call(
            &listener,
            &f,
            &session,
            json!({"path":"no-prompt.txt","content":"never"}),
        )
        .await;
        assert_eq!(finish(&listener, &f).await["kind"], "failed");
        assert!(
            f.store
                .pending_interactions(&session)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(!f.workspace.join("no-prompt.txt").exists());
        close(&f).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn replacing_selected_path_while_approval_waits_never_redirects_the_held_directory() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let (listener, f, session) = fixture("write").await;
        change(&f, &session, "read_only", true, false).await;
        call(
            &listener,
            &f,
            &session,
            json!({"path":"target.txt","content":"original only"}),
        )
        .await;
        let record = pending(&f, &session).await;
        let original = f._dir.path().join("held-original");
        std::fs::rename(&f.workspace, &original).unwrap();
        std::fs::create_dir(&f.workspace).unwrap();
        std::fs::write(f.workspace.join("target.txt"), "replacement").unwrap();
        answer(&f, &record, Some("once")).await;
        let result = finish(&listener, &f).await;
        assert_eq!(
            std::fs::read_to_string(f.workspace.join("target.txt")).unwrap(),
            "replacement"
        );
        if result["kind"] == "completed" {
            assert_eq!(
                std::fs::read_to_string(original.join("target.txt")).unwrap(),
                "original only"
            );
        } else {
            assert!(!original.join("target.txt").exists());
        }
        close(&f).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn sandbox_off_allows_external_files_and_restoring_it_restricts_access() {
    tokio::time::timeout(Duration::from_secs(60), async {
        for method in ["write", "readText"] {
            let (listener, f, session) = fixture(method).await;
            let external = external_directory();
            let outside = external.path().canonicalize().unwrap().join("outside.txt");
            std::fs::write(&outside, "external").unwrap();
            let input = if method == "write" {
                json!({"path":outside,"content":"updated"})
            } else {
                json!({"path":outside})
            };
            for (mode, allowed) in [
                ("workspace_write", method == "readText"),
                ("danger_full_access", true),
                ("read_only", method == "readText"),
            ] {
                let changed = change(&f, &session, mode, false, true).await;
                assert_eq!(changed["kind"], "committed", "{changed}");
                call(&listener, &f, &session, input.clone()).await;
                let result = finish(&listener, &f).await;
                assert_eq!(
                    result["kind"],
                    if allowed { "completed" } else { "failed" },
                    "{mode}: {result}"
                );
                assert!(
                    f.store
                        .pending_interactions(&session)
                        .await
                        .unwrap()
                        .is_empty()
                );
            }
            assert_eq!(
                std::fs::read_to_string(&outside).unwrap(),
                if method == "write" {
                    "updated"
                } else {
                    "external"
                }
            );
            close(&f).await;
        }
    })
    .await
    .unwrap();
}

// External-write tests must live outside the newly writable system temp roots.
pub(super) fn external_directory() -> tempfile::TempDir {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap();
    tempfile::Builder::new()
        .prefix(".maka-permission-test-")
        .tempdir_in(home)
        .unwrap()
}

#[tokio::test]
async fn native_files_external_write_requires_approval_and_once_grants_expire() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let (listener, f, session) = fixture("write").await;
        let external = external_directory();
        let outside = external.path().canonicalize().unwrap().join("approved.txt");
        let input = json!({"path":outside,"content":"approved"});
        for scope in [None, Some("once"), None] {
            call(&listener, &f, &session, input.clone()).await;
            let record = pending(&f, &session).await;
            let InteractionRequest::Permissions { request, .. } = &record.request else {
                unreachable!()
            };
            assert_eq!(request.permissions.filesystem[0].path, outside);
            assert!(!outside.exists());
            answer(&f, &record, scope).await;
            let result = finish(&listener, &f).await;
            assert_eq!(
                result["kind"],
                if scope.is_some() {
                    "completed"
                } else {
                    "failed"
                },
                "{result}"
            );
            if scope.is_some() {
                assert_eq!(std::fs::read_to_string(&outside).unwrap(), "approved");
                std::fs::remove_file(&outside).unwrap();
            } else {
                assert!(!outside.exists());
            }
        }
        let temporary = f.workspace.parent().unwrap().join("temporary.txt");
        call(
            &listener,
            &f,
            &session,
            json!({"path":temporary,"content":"temporary"}),
        )
        .await;
        assert_eq!(finish(&listener, &f).await["kind"], "completed");
        assert_eq!(std::fs::read_to_string(&temporary).unwrap(), "temporary");
        assert!(
            f.store
                .pending_interactions(&session)
                .await
                .unwrap()
                .is_empty()
        );
        close(&f).await;
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn native_files_external_reads_allow_parent_paths_and_aliases_but_deny_private_state() {
    tokio::time::timeout(Duration::from_secs(90), async {
        let (listener, f, session) = fixture("readText").await;
        let outside = f.workspace.parent().unwrap().join("external-read.txt");
        std::fs::write(&outside, "public-external-file").unwrap();
        std::fs::write(f.root.join("private.txt"), "host-private-secret").unwrap();
        let mut paths = vec![
            outside.to_string_lossy().into_owned(),
            "../external-read.txt".into(),
        ];
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, f.workspace.join("external-alias")).unwrap();
            paths.push("external-alias".into());
        }
        for path in paths {
            call(&listener, &f, &session, json!({"path":path})).await;
            let result = finish(&listener, &f).await;
            assert_eq!(result["kind"], "completed", "{path}: {result}");
            assert!(result.to_string().contains("public-external-file"));
        }
        call(
            &listener,
            &f,
            &session,
            json!({"path":f.root.join("private.txt")}),
        )
        .await;
        let result = finish(&listener, &f).await;
        assert_eq!(result["kind"], "failed", "{result}");
        assert!(!result.to_string().contains("host-private-secret"));
        close(&f).await;
    })
    .await
    .unwrap();
}
