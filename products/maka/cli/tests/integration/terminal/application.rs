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

use super::{AsRawFd, Command, Pty};
use local_fixture::counter as fixture;
use maka_plugin_core::composition::PresentationTarget;
use std::time::{Duration, Instant};
fn packages(directory: &std::path::Path, poisoned: bool) -> Vec<String> {
    let (mut input, mut packages) =
        fixture::fixture("return frame({count:state.count+1,revision:state.revision+1});");
    input.environment.presentation = Some(PresentationTarget::Tui);
    if poisoned {
        use maka_packages::MANIFEST_FILE;
        use maka_packages::Package;
        use std::collections::BTreeMap;
        let original = packages.pop().unwrap();
        let source = std::str::from_utf8(original.file("index.mjs").unwrap())
            .unwrap()
            .replace(
                "return {App:{contract,kind:'component',component:['app']}};",
                r"throw new Error('\u001b]52;c;hostile\u0007');",
            );
        let package = Package::new(BTreeMap::from([
            (
                MANIFEST_FILE.into(),
                original.file(MANIFEST_FILE).unwrap().to_vec(),
            ),
            ("index.mjs".into(), source.into_bytes()),
        ]))
        .unwrap();
        input.instances.get_mut("app").unwrap().package = package.application_ref().into();
        packages.push(package);
    }

    let source = directory.join("input.json");
    std::fs::write(&source, serde_json::to_vec(&input).unwrap()).unwrap();
    let root = directory.join("root");
    let ns = maka_event_log::root::RootNamespaces::for_current_account().unwrap();
    drop(maka_event_log::root::RootOwner::create(&root, &ns).unwrap());
    let deployment = uuid::Uuid::new_v4().to_string();
    let mut commit = Command::new(env!("CARGO_BIN_EXE_maka"));
    commit
        .args(["application", "commit", "--root"])
        .arg(&root)
        .args([
            "--deployment",
            &deployment,
            "--expected-revision",
            "0",
            "--input",
        ])
        .arg(&source);
    for package in packages {
        let path = directory.join(format!("{}.maka-plugin", package.manifest().id));
        package.export_to(&path).unwrap();
        commit.arg("--package").arg(&path);
    }
    let result = commit.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    vec![
        "application".into(),
        "run".into(),
        "--root".into(),
        root.to_str().unwrap().into(),
        "--deployment".into(),
        deployment,
    ]
}
#[test]
fn standard_launch_requires_a_terminal_and_help_and_operator_commands_remain_available() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("unused-root");
    let help = Command::new(env!("CARGO_BIN_EXE_maka"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("application"));
    assert!(!help.stdout.contains(&0x1b));
    let piped = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args(["application", "launch", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!piped.status.success());
    assert!(String::from_utf8_lossy(&piped.stderr).contains("interactive terminal"));
    assert!(!root.exists());
    let rejected = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args(["tui", "--root", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(!root.exists());
}

#[test]
fn plugin_application_uses_actual_foreground_transactions_and_restores_the_real_terminal() {
    let directory = tempfile::tempdir().unwrap();
    let args = packages(directory.path(), false);
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut tui = Pty::spawn(&args);
    tui.wait_for("Increment");
    tui.send(b"\r");
    tui.wait_until(|screen| screen.lines().any(|line| line.trim() == "1"));
    tui.send(b"\x03");
    let expired = Instant::now() + Duration::from_millis(2100);
    while Instant::now() < expired {
        tui.read();
        assert!(
            tui.child.try_wait().unwrap().is_none(),
            "first interrupt must not quit on timeout"
        );
    }
    tui.send(b"\x03");
    let armed = Instant::now() + Duration::from_millis(100);
    while Instant::now() < armed {
        tui.read();
        assert!(
            tui.child.try_wait().unwrap().is_none(),
            "expired press must start a new window"
        );
    }
    tui.send(b"\x03");
    tui.finish();
    let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(
        unsafe { libc::tcgetattr(tui.master.as_ref().unwrap().as_raw_fd(), &mut termios) },
        0
    );
    assert_ne!(termios.c_lflag & libc::ICANON, 0);
    assert_ne!(termios.c_lflag & libc::ECHO, 0);
    let snapshot = tui.screen.preview(usize::MAX).unwrap();
    assert!(!snapshot.alternate_screen);
    assert!(snapshot.cursor.visible);
}
#[test]
fn enhanced_quit_keeps_the_terminal_until_its_fragmented_release_is_consumed() {
    let directory = tempfile::tempdir().unwrap();
    let args = packages(directory.path(), false);
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let mut tui = Pty::spawn_with_keyboard(&args, true);
    tui.wait_for("Increment");
    // A single key gesture can span transport reads. Do not hand its remaining
    // CSI-u bytes to the invoking shell when the press requests a clean exit.
    tui.send(b"\x1b[113;5u\x1b[113");
    let until = std::time::Instant::now() + std::time::Duration::from_millis(150);
    while std::time::Instant::now() < until {
        tui.read();
        assert!(tui.child.try_wait().unwrap().is_none());
        assert!(tui.screen.preview(usize::MAX).unwrap().alternate_screen);
    }
    tui.send(b";5:3u");
    tui.finish();
    let master = tui.master.as_ref().unwrap().as_raw_fd();
    let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
    assert_eq!(unsafe { libc::tcgetattr(master, &mut termios) }, 0);
    assert_ne!(termios.c_lflag & libc::ICANON, 0);
    assert_ne!(termios.c_lflag & libc::ECHO, 0);
    assert!(
        !tui.output.windows(5).any(|bytes| bytes == b";5:3u"),
        "quit release was echoed after terminal restoration"
    );
    let snapshot = tui.screen.preview(usize::MAX).unwrap();
    assert!(!snapshot.alternate_screen);
    assert!(snapshot.cursor.visible);
}
#[test]
fn plugin_application_setup_rejection_is_clean_and_diagnostics_cannot_emit_escape_sequences() {
    let directory = tempfile::tempdir().unwrap();
    let args = packages(directory.path(), false);
    let output = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args(&args)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("interactive terminal is required"),
        "{:?}",
        output
    );
    assert!(!output.stderr.contains(&0x1b));
    let mut malicious = args.clone();
    malicious[3] = "missing-\x1b]52;c;hostile\x07".into();
    let output = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args(&malicious)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!output.stderr.contains(&0x1b));
    assert!(!output.stderr.contains(&7));
    let failing = tempfile::tempdir().unwrap();
    let poisoned = packages(failing.path(), true);
    let output = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args(&poisoned)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("hostile"));
    assert!(!output.stderr.contains(&0x1b));
    assert!(!output.stderr.contains(&7));

    let mut input: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("input.json")).unwrap())
            .unwrap();
    // Application target validation fails before any terminal mutation.
    input["environment"] = serde_json::json!({"location":"host","presentation":null});
    input["instances"] = serde_json::json!({});
    input["launch"] = serde_json::Value::Null;
    let path = directory.path().join("input.json");
    std::fs::write(&path, serde_json::to_vec(&input).unwrap()).unwrap();
    let mut update = Command::new(env!("CARGO_BIN_EXE_maka"));
    update
        .args([
            "application",
            "commit",
            "--root",
            &args[3],
            "--deployment",
            &args[5],
            "--expected-revision",
            "1",
            "--input",
        ])
        .arg(&path);
    for entry in std::fs::read_dir(directory.path()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|s| s == "maka-plugin") {
            update.arg("--package").arg(path);
        }
    }
    let committed = update.output().unwrap();
    assert!(
        committed.status.success(),
        "{}",
        String::from_utf8_lossy(&committed.stderr)
    );
    let output = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args(&args)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!output.stderr.contains(&0x1b));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("stored presentation target mismatch")
    );
}

use crate::local_fixture;
/// Tests retain the business Host independently of every foreground UI. This
/// guard also retires product auto-started Hosts before temporary Roots vanish.
pub(super) struct HostRootGuard(std::path::PathBuf);
impl HostRootGuard {
    pub(super) fn new(root: &std::path::Path) -> Self {
        Self(root.to_path_buf())
    }
    pub(super) fn stop(&self) {
        self.retire().unwrap();
    }
    fn retire(&self) -> Result<(), String> {
        use maka_host_runtime::application::host::{Client, discovery};
        if !self.0.exists() {
            return Ok(());
        }
        let discovery = match discovery::read(&self.0) {
            Ok(discovery) => Some(discovery),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        if let Some(discovery) = discovery {
            let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
            runtime.block_on(async {
                // A startup failure may already be draining. Its disappearing
                // status channel is not cleanup proof; the Root lease below is.
                if let Ok(status) =
                    Client::status(&discovery.endpoint, Some(discovery.host_epoch)).await
                    && !status.closing
                    && !status.closed
                {
                    discovery
                        .verify(&status)
                        .map_err(|error| error.to_string())?;
                    if !Client::retire_with_options(
                        &discovery.endpoint,
                        discovery.host_epoch,
                        true,
                        false,
                    )
                    .await?
                    {
                        return Err("Host did not accept explicit retirement".into());
                    }
                }
                Ok::<_, String>(())
            })?;
        }
        let namespaces = maka_event_log::root::RootNamespaces::for_current_account()
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            match maka_event_log::root::RootOwner::open(&self.0, &namespaces) {
                Ok(owner) => {
                    drop(owner);
                    return Ok(());
                }
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::park_timeout(Duration::from_millis(5));
                }
                Err(error) => return Err(error.to_string()),
            }
        }
    }
}
impl Drop for HostRootGuard {
    fn drop(&mut self) {
        if let Err(error) = self.retire() {
            if !std::thread::panicking() {
                panic!("Host cleanup failed: {error}");
            }
            eprintln!("Host cleanup after test failure: {error}");
        }
    }
}

fn local_arguments(
    directory: &std::path::Path,
    unrelated_failure: bool,
    automatic: bool,
) -> (Vec<String>, Option<std::process::Child>) {
    let timer = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap();
    let ns = maka_event_log::root::RootNamespaces::for_current_account().unwrap();
    let host = uuid::Uuid::new_v4();
    let caller = uuid::Uuid::new_v4();
    timer.block_on(async {
        for (name, id, target) in [
            ("host", host, None),
            ("caller", caller, Some(PresentationTarget::Tui)),
        ] {
            let owner = std::sync::Arc::new(
                maka_event_log::root::RootOwner::create(&directory.join(name), &ns).unwrap(),
            );
            let store = maka_event_log::EventLog::for_root(owner).await.unwrap();
            let (input, packages, observations) = if let Some(target) = target {
                local_fixture::ui(target)
            } else {
                let (input, packages) = if unrelated_failure {
                    local_fixture::host_with_startup_failure()
                } else {
                    local_fixture::host()
                };
                (input, packages, vec![])
            };
            store
                .commit_application(id, 0, input, observations, packages)
                .await
                .unwrap();
            store.close().await.unwrap();
        }
    });
    let path = directory.join("connections.json");
    let endpoints = serde_json::json!({"business":{"consumer":local_fixture::identity(13),"import":"business","grant":{"methods":["read","write"],"recovery":false}},"receipts":{"consumer":local_fixture::identity(13),"import":"receipts","grant":{"methods":["read"],"recovery":true}}});
    let endpoints_path = directory.join("endpoints.json");
    std::fs::write(&endpoints_path, serde_json::to_vec(&endpoints).unwrap()).unwrap();
    std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"hosts":[{
        "name":"local","location":{"kind":"public","root":directory.join("host")},"deployment":host,"endpoints":endpoints
    }]})).unwrap()).unwrap();
    let child = if automatic {
        None
    } else {
        let mut child = Command::new(env!("CARGO_BIN_EXE_maka"))
            .args(["application", "serve", "--root"])
            .arg(directory.join("host"))
            .args(["--deployment", &host.to_string(), "--endpoints"])
            .arg(endpoints_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::BufRead;
        let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut poll = libc::pollfd {
            fd: stdout.get_ref().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        assert!(
            unsafe { libc::poll(&mut poll, 1, 30_000) } > 0,
            "public Host did not become ready"
        );
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let ready: serde_json::Value = serde_json::from_str(&line).unwrap();
        let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            use maka_host_runtime::application::host::{Client, discovery};
            assert_eq!(ready["kind"], "ready");
            let epoch = ready["hostEpoch"]
                .as_str()
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
                .expect("actual application serve Ready must include its Host epoch");
            assert!(!epoch.is_nil());
            let found = discovery::read(&directory.join("host")).unwrap();
            assert_eq!(ready["rootId"], found.root_id);
            assert_eq!(ready["socketPath"], found.endpoint.to_str().unwrap());
            assert_eq!(epoch, found.host_epoch);
            let runtime = tokio::runtime::Runtime::new().unwrap();
            runtime.block_on(async {
                let path = std::path::Path::new(ready["socketPath"].as_str().unwrap());
                let mut client = Client::connect(path, Some(epoch)).await.unwrap();
                let status = Client::status(path, Some(epoch)).await.unwrap();
                found.verify(&status).unwrap();
                assert_eq!(status.epoch, epoch);
                client.close().await.unwrap();
            });
        }));
        if let Err(panic) = checked {
            HostRootGuard::new(&directory.join("host")).stop();
            child.wait().unwrap();
            std::panic::resume_unwind(panic);
        }
        Some(child)
    };
    (
        vec![
            "application".into(),
            "run".into(),
            "--root".into(),
            directory.join("caller").to_str().unwrap().into(),
            "--deployment".into(),
            caller.to_string(),
            "--connections".into(),
            path.to_str().unwrap().into(),
        ],
        child,
    )
}
#[test]
fn production_terminal_connections_execute_host_mutation_and_read_original_receipt_before_exit() {
    check_terminal_connections(false, false);
}
#[test]
fn production_terminal_connections_remain_available_after_independent_host_startup_failure() {
    check_terminal_connections(true, false);
}
#[test]
fn generic_terminal_automatically_starts_its_committed_host_and_reuses_it_on_reopen() {
    check_terminal_connections(false, true);
}
#[test]
fn default_terminal_exit_stops_its_last_client_host_process_and_releases_both_roots() {
    let directory = tempfile::tempdir().unwrap();
    let host_root = directory.path().join("host");
    let _guard = HostRootGuard::new(&host_root);
    // A service Host has no idle retirement, so exit proves the UI requested it.
    let (arguments, child) = local_arguments(directory.path(), false, false);
    let mut child = child.unwrap();
    let args = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    let mut tui = Pty::spawn(&args);
    tui.wait_for("Read Host");
    tui.send(b"\r");
    tui.wait_for("\"effects\":0");
    tui.send(b"\x11");
    tui.finish();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "last-client Host exit failed: {status}");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "default UI exit left its last-client Host running"
        );
        std::thread::park_timeout(Duration::from_millis(5));
    }
    let namespaces = maka_event_log::root::RootNamespaces::for_current_account().unwrap();
    drop(
        maka_event_log::root::RootOwner::open(&directory.path().join("caller"), &namespaces)
            .unwrap(),
    );
    drop(maka_event_log::root::RootOwner::open(&host_root, &namespaces).unwrap());
}

fn check_terminal_connections(unrelated_failure: bool, automatic: bool) {
    let directory = tempfile::tempdir().unwrap();
    let host_root = directory.path().join("host");
    let guard = HostRootGuard::new(&host_root);
    let (arguments, mut host_child) =
        local_arguments(directory.path(), unrelated_failure, automatic);
    let args = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    if automatic {
        assert!(maka_host_runtime::application::host::discovery::read(&host_root).is_err());
    }
    let mut tui = Pty::spawn(&args);
    if unrelated_failure {
        tui.wait_for("unrelated initializer rejected");
        tui.wait_for("open recovery interface");
        tui.send(b"\r");
    }
    tui.wait_for("Read Host");
    let discovery = maka_host_runtime::application::host::discovery::read(&host_root).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    // Default UI exit must leave the Host available to this independent client.
    let keepalive = runtime
        .block_on(maka_host_runtime::application::host::Client::connect(
            &discovery.endpoint,
            Some(discovery.host_epoch),
        ))
        .unwrap();
    tui.send(b"\r");
    tui.wait_for("\"effects\":0");
    tui.send(b"\t\r");
    tui.wait_for("\"effects\":1");
    tui.send(b"\t\r");
    tui.wait_for("Original receipt recorded");
    tui.send(b"\x11");
    tui.finish();
    let ns = maka_event_log::root::RootNamespaces::for_current_account().unwrap();
    drop(maka_event_log::root::RootOwner::open(&directory.path().join("caller"), &ns).unwrap());
    if let Some(child) = &mut host_child {
        assert!(
            child.try_wait().unwrap().is_none(),
            "UI exit stopped the independent Host"
        );
    }
    let error = maka_event_log::root::RootOwner::open(&host_root, &ns)
        .err()
        .expect("Host must retain its original Root");
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
    runtime.block_on(async {
        use maka_host_runtime::application::host::Client;
        use maka_host_runtime::application::transport::{Call, Reply, Source};
        let status = Client::status(&discovery.endpoint, Some(discovery.host_epoch))
            .await
            .unwrap();
        discovery.verify(&status).unwrap();
        assert!(!status.closing && !status.closed);
        assert_eq!(status.sessions, 1);
        let caller = std::sync::Arc::new(
            maka_event_log::root::RootOwner::open(&directory.path().join("caller"), &ns).unwrap(),
        );
        let store = maka_event_log::EventLog::for_root(caller).await.unwrap();
        let caller_id = arguments[5].parse().unwrap();
        let intents = store
            .application_intents(caller_id, local_fixture::identity(2), 0, 10)
            .await
            .unwrap();
        assert_eq!(intents.len(), 1, "one original UI write, never replayed");
        let intent = &intents[0];
        let configuration: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("connections.json")).unwrap(),
        )
        .unwrap();
        let endpoints =
            serde_json::from_value(configuration["hosts"][0]["endpoints"].clone()).unwrap();
        let mut peer = Client::connect_peer(
            &discovery.endpoint,
            Some(discovery.host_epoch),
            intent.caller.clone(),
            [intent.consumer.clone()].into(),
            endpoints,
        )
        .await
        .unwrap();
        let channel = peer.open_call_channel().await.unwrap();
        let (reader, writer) = tokio::io::split(channel);
        let mut read = Call::new(
            reader,
            writer,
            peer.catalogue().select("receipts").unwrap(),
            Source {
                deployment: intent.caller.clone(),
                consumer: intent.consumer.clone(),
                mount: uuid::Uuid::new_v4(),
                request: uuid::Uuid::new_v4(),
            },
            "read".into(),
            serde_json::to_value(&intent.reference).unwrap(),
            Default::default(),
        );
        read.start().await.unwrap();
        let Reply::Finished {
            result: Ok(value),
            cleanup: Ok(()),
        } = read.receive().await.unwrap()
        else {
            panic!("original receipt was not readable after UI exit");
        };
        assert_eq!(
            value["reference"],
            serde_json::to_value(&intent.reference).unwrap()
        );
        assert_eq!(value["receipt"]["kind"], "recorded");
        assert_eq!(value["receipt"]["outcome"]["value"]["effects"], 1);
        peer.close().await.unwrap();
        store.close().await.unwrap();
    });
    let mut reopened = Pty::spawn(&args);
    if unrelated_failure {
        reopened.wait_for("unrelated initializer rejected");
        reopened.wait_for("open recovery interface");
        reopened.send(b"\r");
    }
    reopened.wait_for("Read Host");
    reopened.send(b"\r");
    reopened.wait_for("\"effects\":1");
    reopened.send(b"\x11");
    reopened.finish();
    if let Some(child) = &mut host_child {
        assert!(child.try_wait().unwrap().is_none());
    }
    drop(keepalive);
    guard.stop();
    if let Some(mut child) = host_child {
        assert!(child.wait().unwrap().success());
    }
    drop(maka_event_log::root::RootOwner::open(&host_root, &ns).unwrap());
    let snapshot = tui.screen.preview(usize::MAX).unwrap();
    assert!(!snapshot.alternate_screen);
    assert!(snapshot.cursor.visible);
}

#[test]
fn bundled_authoring_starter_generates_checked_presenters_and_runs_native_counter() {
    let directory = tempfile::tempdir().unwrap();
    let starter = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../skills/maka-plugin-authoring/assets/starter/create.mjs");
    for target in ["host", "tui"] {
        let output = Command::new("node")
            .arg(&starter)
            .arg("--output")
            .arg(directory.path().join(target))
            .args(["--target", target, "--maka", env!("CARGO_BIN_EXE_maka")])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = Command::new("python3")
        .arg(starter.with_file_name("verify-host.py"))
        .args(["--maka", env!("CARGO_BIN_EXE_maka")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root = directory.path().join("root");
    let ns = maka_event_log::root::RootNamespaces::for_current_account().unwrap();
    drop(maka_event_log::root::RootOwner::create(&root, &ns).unwrap());
    let deployment = uuid::Uuid::new_v4().to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_maka"))
        .args(["application", "commit", "--root"])
        .arg(&root)
        .args([
            "--deployment",
            &deployment,
            "--expected-revision",
            "0",
            "--input",
        ])
        .arg(directory.path().join("tui/composition.json"))
        .arg("--package")
        .arg(directory.path().join("tui/plugin"))
        .arg("--package")
        .arg(directory.path().join("tui/surface"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut tui = Pty::spawn(&[
        "application",
        "run",
        "--root",
        root.to_str().unwrap(),
        "--deployment",
        &deployment,
    ]);
    tui.wait_for("Count 0");
    tui.send(b"\r");
    tui.wait_for("Count 1");
    tui.send(b"\r");
    tui.wait_for("Count 2");
    tui.send(b"\x11");
    tui.finish();
}
