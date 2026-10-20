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
use maka_agent_api::session::SessionConfiguration;
use maka_event_log::{EventLog, usage::Query};
use maka_host_runtime::logs::Source;
use std::sync::Arc;

#[test]
#[ignore = "real default product Host log grants, no-session Diagnostics and live follow in a native PTY"]
fn default_product_host_logs_without_session_read_follow_and_release_roots() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let host = directory.path().join("host");
    let host_guard = HostRootGuard::new(&host);
    let ui = directory.path().join("ui");
    let namespaces = RootNamespaces::for_current_account().unwrap();
    let owner = RootOwner::create(&host, &namespaces).unwrap();
    let source = Source::from_owner(&owner).unwrap();
    let mut capture = source.open_capture().unwrap();
    drop(owner);
    let tag = uuid::Uuid::new_v4().simple().to_string();
    let first = format!("HOST-LOG-{}-ONE", &tag[..8]);
    let second = format!("HOST-LOG-{}-TWO 中文", &tag[..8]);
    writeln!(capture, "{first}").unwrap();
    capture.flush().unwrap();
    let configuration = directory.path().join("installation.json");
    std::fs::write(
        &configuration,
        serde_json::to_vec(&json!({
            "presentation": "tui", "locale": "en", "scheme": "dark",
            "hostRoot": host,
            "agent": {
                "workspaceRoots": [{"label": "Workspace", "path": workspace}],
                "tools": [],
                "permissions": {"access": "read_only", "requestPermissions": true}
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
    tui.wait_until_with_timeout(
        |screen| screen.contains("Settings") && screen.contains("Start a task."),
        Duration::from_secs(60),
    );
    assert!(
        !tui.screen
            .preview(usize::MAX)
            .unwrap()
            .screen
            .contains("Message…")
    );
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"Diagnostics");
    tui.wait_for("› Diagnostics");
    tui.send(b"\r");
    tui.wait_for("Host logs");
    tui.wait_for(&first);
    settings_evidence(&tui, "host-logs-read");
    click(&mut tui, "Follow");
    tui.wait_for("Stop");
    writeln!(capture, "{second}").unwrap();
    capture.flush().unwrap();
    tui.wait_for(&second);
    assert!(
        tui.screen
            .preview(usize::MAX)
            .unwrap()
            .screen
            .contains(&first)
    );
    settings_evidence(&tui, "host-logs-follow");
    click(&mut tui, "Stop");
    tui.wait_for("Follow");
    click(&mut tui, "Close");
    tui.wait_until(|screen| !screen.contains(&first) && !screen.contains("Follow"));
    tui.send(b"\x11");
    tui.finish();
    assert!(!String::from_utf8_lossy(&tui.output).contains("invalid UI activation"));
    assert!(!String::from_utf8_lossy(&tui.output).contains("invalid UI batch"));
    drop(capture);
    drop(RootOwner::open(&ui, &namespaces).unwrap());
    host_guard.stop();
    let owner = Arc::new(RootOwner::open(&host, &namespaces).unwrap());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let store = EventLog::for_root(owner).await.unwrap();
        assert!(
            store
                .list_sessions::<SessionConfiguration>(None, None, 1)
                .await
                .unwrap()
                .sessions
                .is_empty(),
            "Host diagnostics must not create a conversation"
        );
        assert!(
            store
                .model_attempts(
                    Query {
                        from: 0.0,
                        to: 253402300799999.0,
                        session_id: None,
                        through: None,
                    },
                    0,
                    1
                )
                .await
                .unwrap()
                .attempts
                .is_empty(),
            "Host diagnostics must not invoke a model"
        );
        store.close().await.unwrap();
    });
    drop(RootOwner::open(&host, &namespaces).unwrap());
}
