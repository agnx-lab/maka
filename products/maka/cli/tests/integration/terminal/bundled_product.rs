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

use super::application::HostRootGuard;
use super::*;
use maka_event_log::root::{RootNamespaces, RootOwner};
use serde_json::json;
use unicode_width::UnicodeWidthStr;

mod chat;
mod host_logs;

#[track_caller]
fn click(tui: &mut Pty, label: &str) {
    tui.wait_for(label);
    let screen = tui.screen.preview(usize::MAX).unwrap().screen;
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(label))
        .unwrap();
    let column = line[..line.find(label).unwrap()].width() + 2;
    tui.send(
        format!(
            "\x1b[<0;{};{}M\x1b[<0;{};{}m",
            column,
            row + 1,
            column,
            row + 1
        )
        .as_bytes(),
    );
}

fn select_text(tui: &mut Pty, text: &str) {
    let screen = tui.screen.preview(usize::MAX).unwrap().screen;
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(text))
        .unwrap();
    let start = line[..line.find(text).unwrap()].width() + 1;
    let end = start + text.width();
    tui.send(
        format!(
            "\x1b[<0;{start};{}M\x1b[<32;{end};{}M\x1b[<0;{end};{}m",
            row + 1,
            row + 1,
            row + 1
        )
        .as_bytes(),
    );
}

fn clipboard_output(tui: &mut Pty, start: usize, expected: &str) {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let expected = format!("\x1b]52;c;{}\x07", STANDARD.encode(expected));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tui.read();
        if tui.output[start..]
            .windows(expected.len())
            .any(|bytes| bytes == expected.as_bytes())
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "selected text did not reach the terminal clipboard"
        );
        assert!(
            tui.child.try_wait().unwrap().is_none(),
            "copy exited the application"
        );
    }
}

fn copy_selection(tui: &mut Pty, expected: &str) {
    let gestures: &[&[u8]] = if cfg!(target_os = "macos") {
        &[b"\x03", b"\x1b[99;9u"]
    } else {
        &[b"\x03"]
    };
    for gesture in gestures {
        let start = tui.output.len();
        tui.send(gesture);
        clipboard_output(tui, start, expected);
    }
}

fn edit_menu(tui: &mut Pty, label: &str) {
    tui.wait_for(label);
    let screen = tui.screen.preview(usize::MAX).unwrap().screen;
    let (row, line) = screen
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(label))
        .unwrap();
    let column = line[..line.find(label).unwrap()].width() + 2;
    tui.send(format!("\x1b[<2;{column};{}M\x1b[<2;{column};{}m", row + 1, row + 1).as_bytes());
    tui.wait_for("Select all");
    tui.wait_for("Paste");
}

#[test]
#[ignore = "starts the bundled product and its real Host in a native PTY"]
fn bundled_product_launch_initializes_and_reopens_original_roots_without_node() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let host = directory.path().join("host");
    let host_guard = HostRootGuard::new(&host);
    let ui = directory.path().join("ui");
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
            },
            "skills": {"bundled": true, "sources": []}
        }))
        .unwrap(),
    )
    .unwrap();
    let namespaces = RootNamespaces::for_current_account().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut anchor = None;
    let mut host_epoch = None;
    for first in [true, false] {
        let mut args = vec!["application", "launch", "--root", ui.to_str().unwrap()];
        if first {
            args.extend(["--configuration", configuration.to_str().unwrap()]);
        }
        let started = Instant::now();
        let mut tui = Pty::spawn_with(&args, false, |command| {
            command
                .current_dir(directory.path())
                .env("PATH", directory.path())
                .env_remove("NODE_PATH");
        });
        // First launch now includes authoring and three durable source commits
        // before the same bounded UI startup previously exercised on its own.
        tui.wait_until_with_timeout(
            |screen| screen.contains("Settings"),
            Duration::from_secs(if first { 60 } else { 30 }),
        );
        eprintln!(
            "bundled launch first={first}: first frame in {:?}",
            started.elapsed()
        );
        if first {
            // An independent client keeps the shared Host resident across UI
            // exits and the offline UI edits between these two launches.
            let discovery = maka_host_runtime::application::host::discovery::read(&host).unwrap();
            host_epoch = Some(discovery.host_epoch);
            anchor = Some(
                runtime
                    .block_on(maka_host_runtime::application::host::Client::connect(
                        &discovery.endpoint,
                        Some(discovery.host_epoch),
                    ))
                    .unwrap(),
            );
        }
        tui.send(b"\x0b");
        tui.wait_for("Close commands");
        // The first character after a newly visible dialog must survive its
        // outstanding output receipt, including on the faster reopen path.
        tui.send("测".as_bytes());
        tui.wait_for("测");
        tui.send(b"\x7fSkills");
        tui.wait_for("› Skills");
        tui.send(b"\r");
        tui.wait_until(|screen| {
            screen.contains("maka-plugin-authoring") && !screen.contains("Loading skills")
        });
        tui.send(b"\x1b");
        tui.wait_until(|screen| !screen.contains("╭Skills"));
        tui.send(b"\x11");
        tui.finish();
        assert!(!tui.screen.preview(usize::MAX).unwrap().alternate_screen);
        if first {
            // No authoring input remains for reopening; the record selects the
            // committed deployment, whose user-updated head must remain authoritative.
            std::fs::remove_file(&configuration).unwrap();
            runtime.block_on(async {
                let owner = std::sync::Arc::new(RootOwner::open(&ui, &namespaces).unwrap());
                let store = maka_event_log::EventLog::for_root(owner).await.unwrap();
                let launch = store.application_launch().await.unwrap().unwrap();
                assert!(launch.pending.is_none());
                let loaded = store
                    .load_application(launch.deployment)
                    .await
                    .unwrap()
                    .unwrap();
                let (mut loaded_source, loaded_packages, _, _) = loaded.into_parts();
                assert_eq!(loaded_source.reference.revision, 1);
                loaded_source
                    .input
                    .instances
                    .get_mut("appearance")
                    .unwrap()
                    .configuration
                    .values
                    .insert("scheme".into(), json!("light"));
                store
                    .commit_application(
                        launch.deployment,
                        1,
                        loaded_source.input,
                        loaded_source.remote_exports,
                        loaded_packages,
                    )
                    .await
                    .unwrap();
                store.close().await.unwrap();
            });
        } else {
            runtime.block_on(async {
                let owner = std::sync::Arc::new(RootOwner::open(&ui, &namespaces).unwrap());
                let store = maka_event_log::EventLog::for_root(owner).await.unwrap();
                let launch = store.application_launch().await.unwrap().unwrap();
                assert_eq!(
                    store
                        .read_application_metadata(launch.deployment)
                        .await
                        .unwrap()
                        .unwrap()
                        .source
                        .reference
                        .revision,
                    2
                );
                store.close().await.unwrap();
            });
        }
        drop(RootOwner::open(&ui, &namespaces).unwrap());
        let error = RootOwner::open(&host, &namespaces)
            .err()
            .expect("UI exit must leave the business Host resident");
        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        let discovery = maka_host_runtime::application::host::discovery::read(&host).unwrap();
        if let Some(epoch) = host_epoch {
            assert_eq!(discovery.host_epoch, epoch);
        }
        host_epoch = Some(discovery.host_epoch);
        runtime.block_on(async {
            let status = maka_host_runtime::application::host::Client::status(
                &discovery.endpoint,
                Some(discovery.host_epoch),
            )
            .await
            .unwrap();
            discovery.verify(&status).unwrap();
        });
    }
    runtime.block_on(async {
        anchor.take().unwrap().close().await.unwrap();
    });
    host_guard.stop();
    drop(RootOwner::open(&host, &namespaces).unwrap());
}

#[test]
#[ignore = "faults a real bootstrap after its Host commit, then resumes in a native PTY"]
fn bundled_product_launch_recovers_partial_commits_without_reauthoring() {
    use maka_event_log::EventLog;
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    use std::sync::Arc;
    let directory = tempfile::tempdir().unwrap();
    let host = directory.path().join("host");
    let host_guard = HostRootGuard::new(&host);
    let ui = directory.path().join("ui");
    let configuration = directory.path().join("installation.json");
    std::fs::write(
        &configuration,
        serde_json::to_vec(&json!({
            "locale":"en", "scheme":"dark", "hostRoot":host,
            "agent":{"workspaceRoots":[{"label":"Workspace","path":directory.path()}],
                "tools":[],
                "permissions":{"access":"read_only","requestPermissions":true}}
        }))
        .unwrap(),
    )
    .unwrap();
    let namespaces = RootNamespaces::for_current_account().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let owner = Arc::new(RootOwner::create(&ui, &namespaces).unwrap());
        let store = EventLog::for_root(owner.clone()).await.unwrap();
        store.close().await.unwrap();
        let mut sql = SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(ui.join(maka_event_log::root::ROOT_DATABASE))).await.unwrap();
        // A deferred FK fails the actual management COMMIT after the independent
        // Host COMMIT succeeded. No production fault switch is involved.
        sqlx::raw_sql("CREATE TABLE launch_abort (deployment TEXT, revision INTEGER, FOREIGN KEY(deployment,revision) REFERENCES application_sources(deployment,revision) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER reject_launch_source AFTER INSERT ON application_sources BEGIN INSERT INTO launch_abort VALUES ('missing',1); END;")
            .execute(&mut sql).await.unwrap();
        sql.close().await.unwrap();
        drop(owner);
    });
    let failed = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args([
            "application",
            "launch",
            "--root",
            ui.to_str().unwrap(),
            "--configuration",
            configuration.to_str().unwrap(),
        ])
        .current_dir(directory.path())
        .env("PATH", directory.path())
        .env_remove("NODE_PATH")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("commit"),
        "{}",
        String::from_utf8_lossy(&failed.stderr)
    );
    host_guard.stop();
    let (ui_id, host_id) = runtime.block_on(async {
        let owner = Arc::new(RootOwner::open(&ui, &namespaces).unwrap());
        let store = EventLog::for_root(owner.clone()).await.unwrap();
        let record = store.application_launch().await.unwrap().unwrap();
        assert!(record.pending.is_some());
        assert!(
            store
                .read_application_metadata(record.deployment)
                .await
                .unwrap()
                .is_none()
        );
        let host_id = record.configuration["hosts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|host| host["location"]["kind"] == "public")
            .unwrap()["deployment"]
            .as_str()
            .unwrap()
            .parse::<uuid::Uuid>()
            .unwrap();
        store.close().await.unwrap();
        let mut sql = SqliteConnection::connect_with(
            &SqliteConnectOptions::new().filename(ui.join(maka_event_log::root::ROOT_DATABASE)),
        )
        .await
        .unwrap();
        sqlx::raw_sql("DROP TRIGGER reject_launch_source; DROP TABLE launch_abort;")
            .execute(&mut sql)
            .await
            .unwrap();
        sql.close().await.unwrap();
        drop(owner);
        let owner = Arc::new(RootOwner::open(&host, &namespaces).unwrap());
        let store = EventLog::for_root(owner).await.unwrap();
        let loaded = store.load_application(host_id).await.unwrap().unwrap();
        let (mut loaded_source, loaded_packages, _, _) = loaded.into_parts();
        assert_eq!(loaded_source.reference.revision, 1);
        loaded_source
            .input
            .instances
            .get_mut("agent")
            .unwrap()
            .configuration
            .values
            .insert("automaticTitles".into(), json!(true));
        store
            .commit_application(
                host_id,
                1,
                loaded_source.input,
                loaded_source.remote_exports,
                loaded_packages,
            )
            .await
            .unwrap();
        store.close().await.unwrap();
        (record.deployment, host_id)
    });
    std::fs::remove_file(configuration).unwrap();
    let mut tui = Pty::spawn_with(
        &["application", "launch", "--root", ui.to_str().unwrap()],
        false,
        |command| {
            command
                .current_dir(directory.path())
                .env("PATH", directory.path())
                .env_remove("NODE_PATH");
        },
    );
    // Recovery first commits the two missing deployments, just as installation
    // does. Bound that preparation separately from the unchanged UI deadline.
    let preparation = Instant::now();
    tui.wait_for_output("Opening product…", Duration::from_secs(30));
    eprintln!("partial launch preparation: {:?}", preparation.elapsed());
    tui.wait_until_with_timeout(
        |screen| screen.contains("Settings"),
        Duration::from_secs(30),
    );
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    runtime.block_on(async {
        let owner = Arc::new(RootOwner::open(&ui, &namespaces).unwrap());
        let store = EventLog::for_root(owner).await.unwrap();
        let record = store.application_launch().await.unwrap().unwrap();
        assert_eq!(record.deployment, ui_id);
        assert!(record.pending.is_none());
        assert_eq!(
            store
                .read_application_metadata(ui_id)
                .await
                .unwrap()
                .unwrap()
                .source
                .reference
                .revision,
            1
        );
        store.close().await.unwrap();
        let owner = Arc::new(RootOwner::open(&host, &namespaces).unwrap());
        let store = EventLog::for_root(owner).await.unwrap();
        let loaded = store
            .read_application_metadata(host_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.source.reference.revision, 2);
        assert_eq!(
            loaded.source.input.instances["agent"].configuration.values["automaticTitles"],
            json!(true)
        );
        store.close().await.unwrap();
    });
}

#[test]
#[ignore = "real foreground request processing and source reload in a native PTY"]
fn bundled_ui_authoring_installs_and_reloads_original_source_without_replaying() {
    use maka_capabilities::storage::{Data, Mutation, Namespace};
    use maka_event_log::EventLog;
    use maka_packages::Package;
    use maka_plugin_core::{composition::InstanceId, scope::Scope};
    use std::{collections::BTreeMap, sync::Arc};
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let host = directory.path().join("host");
    let host_guard = HostRootGuard::new(&host);
    let ui = directory.path().join("ui");
    let configuration = directory.path().join("configuration.json");
    std::fs::write(&configuration,serde_json::to_vec(&json!({
        "presentation":"tui","locale":"en","scheme":"dark","hostRoot":host,
        "agent":{"workspaceRoots":[{"label":"Workspace","path":workspace}],"tools":[],"permissions":{"access":"read_only","requestPermissions":true}}
    })).unwrap()).unwrap();
    let mut first = Pty::spawn(&[
        "application",
        "launch",
        "--root",
        ui.to_str().unwrap(),
        "--configuration",
        configuration.to_str().unwrap(),
    ]);
    first.wait_until_with_timeout(
        |screen| screen.contains("Settings"),
        Duration::from_secs(60),
    );
    first.send(b"\x11");
    first.finish();
    host_guard.stop();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let namespaces = RootNamespaces::for_current_account().unwrap();
    let namespace = Namespace::new("maka.application.authoring", Scope::Profile).unwrap();
    let ids = ["a".repeat(64), "b".repeat(64)];
    let (deployment,prefix)=runtime.block_on(async {
        let store=EventLog::for_root(Arc::new(RootOwner::open(&ui,&namespaces).unwrap())).await.unwrap();
        let launch=store.application_launch().await.unwrap().unwrap();
        let loaded=store.load_application(launch.deployment).await.unwrap().unwrap();
        let (loaded_source, loaded_packages, _, _) = loaded.into_parts();
        let host_id=launch.configuration["hosts"].as_array().unwrap().iter().find(|v|v["name"]=="local").unwrap()["deployment"].as_str().unwrap().parse().unwrap();
        let footer=loaded_packages.iter().find(|p|p.manifest().id=="maka.agent.composer-footer").unwrap();
        let mut files=footer.files().clone();
        files.get_mut("tui.js").unwrap().extend_from_slice(b"\n// runtime authored footer\n");
        let authored=Package::new(files.clone()).unwrap().application_ref();
        let files:BTreeMap<_,_>=files.into_iter().map(|(path,bytes)|(path,String::from_utf8(bytes).unwrap())).collect();
        let mut input=serde_json::to_value(&loaded_source.input).unwrap();
        input["instances"]["composer-footer"]["package"]=json!({"source":"pinned","package":authored});
        input["instances"]["agent-screen"]["configuration"]["values"]["locale"]=json!("zh-CN");
        let mut packages:Vec<_>=loaded_packages.iter().map(Package::application_ref).collect();packages.push(authored);
        let commands=[
            json!({"method":"installSource","input":{"expectedRevision":1,"files":files}}),
            json!({"method":"replaceSource","input":{"expectedRevision":2,"document":{"input":input,"remoteExports":loaded_source.remote_exports,"packages":packages}}})
        ];
        let deployment=launch.deployment;
        store.shutdown().await.unwrap();drop(store);
        let store=EventLog::for_root(Arc::new(RootOwner::open(&host,&namespaces).unwrap())).await.unwrap();
        let source=store.load_application(host_id).await.unwrap().unwrap().into_parts().0;
        let instance=InstanceId(vec![source.input.instances["ui-authoring"].key]);
        let prefix=maka_host_runtime::storage::instance_prefix(&source.reference.state_domain,host_id,&instance);
        // Seed accepted intents while the fixture Host is closed. The foreground,
        // management commits, journals and hot reload below are the shipped path.
        let mut mutations=vec![Mutation{key:format!("{prefix}queue"),expected_revision:None,data:Data::Present(json!(ids))}];
        for (id,command) in ids.iter().zip(commands) {mutations.push(Mutation{key:format!("{prefix}request/{id}"),expected_revision:None,data:Data::Present(json!({"session":"fixture","request":{"id":id,"command":command},"state":{"status":"queued"}}))});}
        store.plugin_data_batch(&namespace,mutations).await.unwrap();
        store.shutdown().await.unwrap();
        (deployment,prefix)
    });
    let mut resumed = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    resumed.wait_until_with_timeout(|screen| screen.contains("设置"), Duration::from_secs(45));
    // A visible reload precedes its original receipt publication. Keep reading
    // the terminal until both accepted requests settle, without replaying either.
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    let mut sql = runtime
        .block_on(SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(host.join(maka_event_log::root::ROOT_DATABASE))
                .read_only(true),
        ))
        .unwrap();
    resumed.wait_until_with_timeout(|_| {
        runtime.block_on(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM plugin_data WHERE package_id=? AND scope_id=? AND key IN (?, ?) AND json_extract(value_json,'$.state.status')='completed'"
        ).bind(namespace.package()).bind(String::from(namespace.scope().clone()))
        .bind(format!("{prefix}request/{}", ids[0])).bind(format!("{prefix}request/{}", ids[1]))
        .fetch_one(&mut sql)).unwrap() == 2
    }, Duration::from_secs(30));
    runtime.block_on(sql.close()).unwrap();
    resumed.send(b"\x11");
    resumed.finish();
    host_guard.stop();
    runtime.block_on(async {
        let store = EventLog::for_root(Arc::new(RootOwner::open(&host, &namespaces).unwrap()))
            .await
            .unwrap();
        for id in &ids {
            let record = store
                .plugin_data(&namespace, &format!("{prefix}request/{id}"))
                .await
                .unwrap()
                .unwrap();
            let state = &record.data.value().unwrap()["state"];
            assert_eq!(state["status"], "completed", "{state}");
            assert_eq!(
                state["outcome"]["reply"]["receipt"]["outcome"]["value"]["status"], "committed",
                "{state}"
            );
        }
        store.shutdown().await.unwrap();
        let store = EventLog::for_root(Arc::new(RootOwner::open(&ui, &namespaces).unwrap()))
            .await
            .unwrap();
        assert_eq!(
            store
                .read_application_metadata(deployment)
                .await
                .unwrap()
                .unwrap()
                .source
                .reference
                .revision,
            3,
            "exactly one commit per requested edit"
        );
        store.shutdown().await.unwrap();
    });
    let restored = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args([
            "application",
            "restore-defaults",
            "--root",
            ui.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&restored.stdout).unwrap();
    assert_eq!(result["revision"], 4);
    assert_eq!(result["deployment"], deployment.to_string());
    let mut restored_ui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    restored_ui.wait_until_with_timeout(
        |screen| screen.contains("Settings"),
        Duration::from_secs(45),
    );
    restored_ui.send(b"\x11");
    restored_ui.finish();
    host_guard.stop();
    runtime.block_on(async {
        let store = EventLog::for_root(Arc::new(RootOwner::open(&host, &namespaces).unwrap()))
            .await
            .unwrap();
        // Restoration changes desired composition; accepted request history survives.
        for id in &ids {
            let record = store
                .plugin_data(&namespace, &format!("{prefix}request/{id}"))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(record.data.value().unwrap()["state"]["status"], "completed");
        }
        store.shutdown().await.unwrap();
    });
}

#[test]
#[ignore = "starts the standard product and its real Host in a native PTY"]
fn first_install_trust_uses_one_terminal_and_cancel_has_no_product_effects() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("ui");
    let _host_guard = HostRootGuard::new(&root.join("host"));
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let args = [
        "application",
        "launch",
        "--root",
        root.to_str().unwrap(),
        "--workspace",
        workspace.to_str().unwrap(),
        "--locale",
        "en",
    ];
    let mut cancelled = Pty::spawn(&args);
    cancelled.wait_for("Trust this workspace?");
    assert!(
        cancelled
            .screen
            .preview(usize::MAX)
            .unwrap()
            .alternate_screen
    );
    assert!(!root.join("host").exists());
    cancelled.send(b"\r");
    cancelled.finish();
    assert!(
        !cancelled
            .screen
            .preview(usize::MAX)
            .unwrap()
            .alternate_screen
    );
    assert!(!root.join("host").exists());
    let mut tui = Pty::spawn(&args);
    tui.wait_for("Trust this workspace?");
    tui.send(b"\t\r");
    tui.wait_until_with_timeout(
        |screen| screen.contains("Settings"),
        Duration::from_secs(60),
    );
    assert!(tui.screen.preview(usize::MAX).unwrap().alternate_screen);
    // No alternate-screen teardown between confirmation and the actual product.
    assert!(!tui.output.windows(8).any(|bytes| bytes == b"\x1b[?1049l"));
    tui.send(b"\x11");
    tui.finish();
    assert!(!tui.screen.preview(usize::MAX).unwrap().alternate_screen);
    let mut reopened = Pty::spawn(&["application", "launch", "--root", root.to_str().unwrap()]);
    reopened.wait_until_with_timeout(
        |screen| screen.contains("Settings"),
        Duration::from_secs(30),
    );
    assert!(!String::from_utf8_lossy(&reopened.output).contains("Trust this workspace?"));
    reopened.send(b"\x11");
    reopened.finish();
}

#[test]
#[ignore = "checks a failed standard product startup in a native PTY"]
fn first_install_failure_keeps_original_host_error_after_terminal_restoration() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("ui");
    let _host_guard = HostRootGuard::new(&root.join("host"));
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let mut tui = Pty::spawn(&[
        "application",
        "launch",
        "--root",
        root.to_str().unwrap(),
        "--workspace",
        workspace.to_str().unwrap(),
        "--locale",
        "en",
    ]);
    tui.wait_for("Trust this workspace?");
    // The selected path disappears while the user reads the prompt. Its Host
    // initializer must fail, and that concrete failure must survive the UI error.
    std::fs::remove_dir(&workspace).unwrap();
    tui.send(b"\t\r");
    tui.wait_until_with_timeout(
        |screen| screen.contains("Host local: plugin"),
        Duration::from_secs(60),
    );
    let startup = tui.screen.preview(usize::MAX).unwrap();
    assert!(startup.alternate_screen);
    assert!(
        startup.screen.contains("skills_configuration_invalid")
            || startup
                .screen
                .contains("agent_workspace_configuration_invalid")
    );
    tui.send(b"\x11");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        tui.read();
        if let Some(status) = tui.child.try_wait().unwrap() {
            tui.read();
            assert!(!status.success());
            break;
        }
        assert!(Instant::now() < deadline, "failed startup did not settle");
    }
    let snapshot = tui.screen.preview(usize::MAX).unwrap();
    assert!(!snapshot.alternate_screen);
    assert!(
        snapshot.screen.contains("Host local: plugin"),
        "{}",
        snapshot.screen
    );
    assert!(
        snapshot.screen.contains("skills_configuration_invalid")
            || snapshot
                .screen
                .contains("agent_workspace_configuration_invalid"),
        "{}",
        snapshot.screen
    );
    let output = String::from_utf8_lossy(&tui.output);
    assert!(output.rfind("Host local: plugin").unwrap() > output.rfind("\x1b[?1049l").unwrap());
}

fn editor_thumb(screen: &str, label: &str) -> Vec<usize> {
    let lines: Vec<_> = screen.lines().collect();
    let label = lines
        .iter()
        .position(|line| line.contains(label))
        .expect("editor label");
    let (border, line) = lines
        .iter()
        .enumerate()
        .skip(label + 1)
        .find(|(_, line)| line.contains('╭'))
        .expect("editor border");
    let right = line[..line.rfind('╮').expect("right border")].width();
    lines
        .iter()
        .enumerate()
        .skip(border + 1)
        .take_while(|(_, line)| !line.contains('╰'))
        .filter_map(|(row, line)| (line.chars().nth(right - 1) == Some('┃')).then_some(row))
        .collect()
}
fn settings_evidence(tui: &Pty, name: &str) {
    if let Some(path) = std::env::var_os("MAKA_SETTINGS_EVIDENCE") {
        std::fs::create_dir_all(&path).unwrap();
        let path = std::path::PathBuf::from(path);
        std::fs::write(path.join(format!("{name}.ansi")), &tui.output).unwrap();
        std::fs::write(
            path.join(format!("{name}.txt")),
            tui.screen.preview(usize::MAX).unwrap().screen,
        )
        .unwrap();
    }
}

fn resize_editor(tui: &mut Pty, label: &str, delta: i32) {
    let grip = |screen: &str| {
        let start = screen.lines().position(|line| line.contains(label))?;
        screen
            .lines()
            .enumerate()
            .skip(start + 1)
            .find_map(|(row, line)| {
                let column = line.find('↕')?;
                Some((row, line[..column].width()))
            })
    };
    tui.wait_until(|screen| grip(screen).is_some());
    let (row, column) =
        grip(&tui.screen.preview(usize::MAX).unwrap().screen).expect("visible resize grip");
    let end = (row as i32 + delta) as usize;
    tui.send(
        format!(
            "\x1b[<0;{};{}M\x1b[<32;{};{}M\x1b[<0;{};{}m",
            column + 1,
            row + 1,
            column + 1,
            end + 1,
            column + 1,
            end + 1
        )
        .as_bytes(),
    );
    tui.wait_until(|screen| grip(screen).is_some_and(|(row, _)| row == end));
}

#[test]
#[ignore = "opens assistant preferences in the real product PTY and after restart"]
fn bundled_assistant_preferences_opens_with_compaction_editor() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let ui = directory.path().join("ui");
    let _host_guard = HostRootGuard::new(&ui.join("host"));
    let home = directory.path().join("home");
    std::fs::create_dir_all(home.join(".agents")).unwrap();
    std::fs::write(home.join(".agents/AGENTS.md"), "PTY global instruction.").unwrap();
    let configuration = directory.path().join("installation.json");
    std::fs::write(&configuration, serde_json::to_vec(&json!({
        "presentation":"tui", "locale":"en", "scheme":"dark", "hostRoot":ui.join("host"),
        "agent":{"assistant":{"home":home},"workspaceRoots":[{"label":"Workspace","path":workspace}], "tools":[],"permissions":{"access":"read_only","requestPermissions":true}}
    })).unwrap()).unwrap();
    for first in [true, false] {
        let mut args = vec!["application", "launch", "--root", ui.to_str().unwrap()];
        if first {
            args.extend(["--configuration", configuration.to_str().unwrap()]);
        }
        let mut tui = Pty::spawn(&args);
        tui.wait_until_with_timeout(|s| s.contains("Settings"), Duration::from_secs(60));
        tui.send(b"\x0b");
        tui.wait_for("Close commands");
        tui.send(b"Settings");
        tui.wait_for("› Settings");
        tui.send(b"\r");
        click(&mut tui, "Assistant preferences");
        tui.wait_for("Your name");
        tui.wait_for("Global instructions");
        tui.wait_for(if first {
            "PTY global instruction."
        } else {
            "PTY updated global instruction."
        });
        assert!(
            tui.screen
                .preview(usize::MAX)
                .unwrap()
                .screen
                .contains(".agents/AGENTS.md")
        );
        resize_editor(&mut tui, "Global instructions", 4);
        resize_editor(&mut tui, "Global instructions", -4);
        if first {
            click(&mut tui, "PTY global instruction.");
            tui.send(b"\x01");
            tui.send(b"\x1b[200~PTY updated global instruction.\x1b[201~");
            tui.wait_for("PTY updated global instruction.");
            click(&mut tui, "Save global instructions");
            tui.wait_for("Global instructions saved.");
            assert_eq!(
                std::fs::read_to_string(home.join(".agents/AGENTS.md")).unwrap(),
                "PTY updated global instruction."
            );
            assert!(!home.join(".maka/AGENTS.md").exists());
        }
        tui.wait_for("Compression prompt");
        assert!(
            !editor_thumb(
                &tui.screen.preview(usize::MAX).unwrap().screen,
                "Compression prompt"
            )
            .is_empty(),
            "partly clipped prompt must still show its scroll thumb"
        );
        settings_evidence(&tui, "settings-overview");
        // Scroll the settings pane until the entire prompt editor fits, then
        // keep wheeling inside it past both boundaries.
        for _ in 0..80 {
            let screen = tui.screen.preview(usize::MAX).unwrap().screen;
            let (row, line) = screen
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("Compression prompt"))
                .unwrap();
            if row <= 4 {
                break;
            }
            let column = line[..line.find("Compression prompt").unwrap()].width() + 2;
            tui.send(format!("\x1b[<65;{column};{}M", row + 1).as_bytes());
            tui.wait_until(|s| {
                s.lines()
                    .position(|line| line.contains("Compression prompt"))
                    != Some(row)
            });
        }
        let screen = tui.screen.preview(usize::MAX).unwrap().screen;
        let (row, line) = screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Compression prompt"))
            .unwrap();
        assert!(row <= 4, "prompt must reach top of settings pane: {screen}");
        let column = line[..line.find("Compression prompt").unwrap()].width() + 4;
        tui.send(
            format!("\x1b[<64;{column};{}M", row + 3)
                .repeat(80)
                .as_bytes(),
        );
        let default_prompt = maka_agent_facts::context::default_compaction_prompt(
            maka_agent_facts::language::Language::En,
        );
        tui.wait_for(&default_prompt.chars().take(32).collect::<String>());
        assert_eq!(
            tui.screen
                .preview(usize::MAX)
                .unwrap()
                .screen
                .lines()
                .position(|line| line.contains("Compression prompt")),
            Some(row)
        );
        let top_thumb = editor_thumb(
            &tui.screen.preview(usize::MAX).unwrap().screen,
            "Compression prompt",
        );
        assert!(!top_thumb.is_empty());
        settings_evidence(&tui, "settings-prompt-top");
        tui.send(
            format!("\x1b[<65;{column};{}M", row + 3)
                .repeat(80)
                .as_bytes(),
        );
        let ending: String = default_prompt.chars().rev().take(12).collect();
        let prompt_tail: String = ending.chars().rev().collect();
        tui.wait_for(&prompt_tail);
        let bottom_thumb = editor_thumb(
            &tui.screen.preview(usize::MAX).unwrap().screen,
            "Compression prompt",
        );
        assert!(!bottom_thumb.is_empty());
        assert_ne!(
            top_thumb, bottom_thumb,
            "wheel scrolling must move the visible thumb"
        );
        settings_evidence(&tui, "settings-prompt-bottom");
        assert_eq!(
            tui.screen
                .preview(usize::MAX)
                .unwrap()
                .screen
                .lines()
                .position(|line| line.contains("Compression prompt")),
            Some(row)
        );
        let screen = tui.screen.preview(usize::MAX).unwrap().screen;
        let tail = screen
            .lines()
            .position(|line| line.contains(&prompt_tail))
            .unwrap();
        assert!(
            tail >= row + 20,
            "the prompt should use the full settings pane: {screen}"
        );
        resize_editor(&mut tui, "Compression prompt", 5);
        resize_editor(&mut tui, "Compression prompt", -5);
        assert!(
            !screen.contains("New subagent"),
            "subagents have their own settings category"
        );
        // Mouse-select a visible line, then copy the entire native prompt.
        let line = default_prompt
            .lines()
            .rfind(|line| line.starts_with("## "))
            .unwrap();
        tui.wait_for(line);
        select_text(&mut tui, line);
        copy_selection(&mut tui, line);
        tui.send(b"\x01");
        copy_selection(
            &mut tui,
            maka_agent_facts::context::default_compaction_prompt(
                maka_agent_facts::language::Language::En,
            ),
        );
        edit_menu(&mut tui, &prompt_tail);
        click(&mut tui, "Select all");
        edit_menu(&mut tui, &prompt_tail);
        let start = tui.output.len();
        click(&mut tui, "Copy");
        clipboard_output(
            &mut tui,
            start,
            maka_agent_facts::context::default_compaction_prompt(
                maka_agent_facts::language::Language::En,
            ),
        );
        tui.wait_until(|screen| !screen.contains("Select all"));
        settings_evidence(&tui, "settings");
        let screen = tui.screen.preview(usize::MAX).unwrap().screen;
        let (row, line) = screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Save compression prompt"))
            .unwrap();
        let column = line[..line.find("Save compression prompt").unwrap()].width() + 2;
        tui.send(
            format!("\x1b[<65;{column};{}M", row + 1)
                .repeat(40)
                .as_bytes(),
        );
        tui.wait_for("Privacy mode");
        let is_private = |screen: &str, enabled: bool| {
            screen.lines().any(|line| {
                line.contains("Privacy mode")
                    && line.contains(if enabled { "━●" } else { "○─" })
            })
        };
        tui.wait_until(|screen| is_private(screen, false));
        let screen = tui.screen.preview(usize::MAX).unwrap().screen;
        let (row, line) = screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Privacy mode"))
            .unwrap();
        let column = line[..line.find("Privacy mode").unwrap()].width() + 2;
        tui.send(format!("\x1b[<35;{column};{}M", row + 1).as_bytes());
        tui.wait_for("still saved;");
        tui.wait_for("configured provider.");
        settings_evidence(&tui, "settings-switch-tooltip");
        let column = line[..line.find("○─").unwrap()].width() + 1;
        tui.send(format!("\x1b[<0;{column};{}M\x1b[<0;{column};{}m", row + 1, row + 1).as_bytes());
        tui.wait_until(|screen| is_private(screen, true));
        tui.send(b" ");
        tui.wait_until(|screen| is_private(screen, false));
        tui.send(b"\x11");
        tui.finish();
        assert!(!String::from_utf8_lossy(&tui.output).contains("invalid UI activation"));
    }
}

#[test]
#[ignore = "starts the bundled product and checks connection settings in a native PTY"]
fn bundled_connections_search_scroll_click_and_chatgpt_credentials() {
    let directory = tempfile::tempdir().unwrap();
    let _host_guard = HostRootGuard::new(&directory.path().join("host"));
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let configuration = directory.path().join("installation.json");
    let ui = directory.path().join("ui");
    std::fs::write(&configuration, serde_json::to_vec(&json!({
        "presentation":"tui", "locale":"en", "scheme":"dark", "hostRoot":directory.path().join("host"),
        "agent":{"workspaceRoots":[{"label":"Workspace","path":workspace}], "tools":[],"permissions":{"access":"read_only","requestPermissions":true}}
    })).unwrap()).unwrap();
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
    tui.send(b"Settings");
    tui.wait_for("› Settings");
    tui.send(b"\r");
    click(&mut tui, "Models");
    tui.wait_for("No connections yet.");
    click(&mut tui, "Add connection");
    tui.wait_until(|s| {
        s.contains("ChatGPT Subscription") && s.contains('█') && !s.contains("Connecting")
    });
    let size = libc::winsize {
        ws_row: 20,
        ws_col: 64,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    assert_eq!(
        unsafe {
            libc::ioctl(
                tui.master.as_ref().unwrap().as_raw_fd(),
                libc::TIOCSWINSZ,
                &size,
            )
        },
        0
    );
    tui.screen
        .resize(TerminalSize::new(64, 20).unwrap())
        .unwrap();
    tui.frames.resize();
    tui.wait_until(|s| s.contains("ChatGPT Subscription") && s.contains('█'));
    let before = tui.screen.preview(usize::MAX).unwrap().screen;
    let row = before
        .lines()
        .position(|line| line.contains("ChatGPT Subscription"))
        .unwrap();
    let line = before.lines().nth(row).unwrap();
    let column = line[..line.find("ChatGPT Subscription").unwrap()]
        .chars()
        .count()
        + 2;
    let scroll_started = Instant::now();
    tui.send(format!("\x1b[<65;{};{}M", column, row + 2).as_bytes());
    tui.wait_until(|s| !s.contains("ChatGPT Subscription"));
    eprintln!(
        "provider wheel to visible change: {:?}",
        scroll_started.elapsed()
    );
    click(&mut tui, "Search providers");
    let search_started = Instant::now();
    tui.send(b"chatgpt");
    tui.wait_until(|s| s.contains("ChatGPT Subscription") && !s.contains("Anthropic"));
    eprintln!(
        "provider input to filtered frame: {:?}",
        search_started.elapsed()
    );
    let selection_started = Instant::now();
    tui.send(b"\x1b[B");
    tui.send(b"\r");
    tui.wait_for("Continue to sign in");
    eprintln!(
        "provider selection to form: {:?}",
        selection_started.elapsed()
    );
    let screen = tui.screen.preview(usize::MAX).unwrap().screen;
    assert!(screen.contains("Name"), "{screen}");
    assert!(!screen.contains("Next field"), "{screen}");
    assert!(!screen.contains("Connection ID"), "{screen}");
    click(&mut tui, "Continue to sign in");
    tui.wait_for("Sign in with ChatGPT");
    assert!(
        !tui.screen
            .preview(usize::MAX)
            .unwrap()
            .screen
            .contains("Enter an API key")
    );
    tui.send(b"\x11");
    tui.finish();
}

#[test]
#[ignore = "checks live language changes and persistence in the real product PTY"]
fn bundled_language_switch_updates_and_remains_responsive() {
    let directory = tempfile::tempdir().unwrap();
    let _host_guard = HostRootGuard::new(&directory.path().join("host"));
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let configuration = directory.path().join("installation.json");
    let ui = directory.path().join("ui");
    std::fs::write(&configuration, serde_json::to_vec(&json!({
        "presentation":"tui", "locale":"en", "scheme":"dark", "hostRoot":directory.path().join("host"),
        "agent":{"workspaceRoots":[{"label":"Workspace","path":workspace}], "tools":[],"permissions":{"access":"read_only","requestPermissions":true}}
    })).unwrap()).unwrap();
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
    tui.send(b"Settings");
    tui.wait_for("› Settings");
    tui.send(b"\r");
    click(&mut tui, "Models");
    tui.wait_for("No connections yet.");
    click(&mut tui, "Add connection");
    tui.wait_for("ChatGPT Subscription");
    tui.send(b"\x1b");
    tui.wait_for("Add connection");
    click(&mut tui, "Appearance");
    click(&mut tui, "Language");
    tui.wait_for("繁體中文");
    settings_evidence(&tui, "language-options");
    click(&mut tui, "简体中文");
    tui.wait_until_with_timeout(|s| s.contains("语言"), Duration::from_secs(3));
    settings_evidence(&tui, "language-zh-CN");
    click(&mut tui, "语言");
    click(&mut tui, "繁體中文");
    tui.wait_until_with_timeout(|s| s.contains("語言"), Duration::from_secs(3));
    settings_evidence(&tui, "language-zh-TW");
    click(&mut tui, "語言");
    click(&mut tui, "English");
    tui.wait_for("Language");
    click(&mut tui, "Language");
    click(&mut tui, "简体中文");
    tui.wait_until_with_timeout(|s| s.contains("语言"), Duration::from_secs(3));
    tui.send(b"\x11");
    tui.finish();
    assert!(!String::from_utf8_lossy(&tui.output).contains("invalid UI action"));
    let mut reopened = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    reopened.wait_until_with_timeout(|s| s.contains("设置"), Duration::from_secs(30));
    settings_evidence(&reopened, "language-reopened");
    reopened.send(b"\x11");
    reopened.finish();
    assert!(!String::from_utf8_lossy(&reopened.output).contains("invalid UI action"));
}
