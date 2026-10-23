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

//! Native Windows acceptance fixture for the production Host/process boundary.
//! Run the built executable from a Windows local path with an unelevated token.

#[cfg(not(windows))]
fn main() {
    eprintln!("windows_mxc must run on native Windows with MXC support");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() -> native::Result<()> {
    native::main()
}

#[cfg(windows)]
mod native {
    use maka_agent_facts::terminal::TerminalSize;
    use maka_host_runtime::sandbox::windows::{self, Backend, Installation, Status};
    use maka_process::{
        Command, ObservedProcess, PipeEvent, ProcessOutcome, Termination, bootstrap, pipe, pty,
    };
    use maka_sandbox::{
        Destination, Network, Sandbox,
        filesystem::{Access, Policy, Rule},
    };
    use serde_json::json;
    use std::{
        io::{self, BufRead, Read, Write},
        net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream},
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        path::{Path, PathBuf},
        process::Stdio,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    use tokio_util::sync::CancellationToken;
    use windows_sys::Win32::{
        Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::{
            Console::{
                CONSOLE_SCREEN_BUFFER_INFO, GetConsoleScreenBufferInfo, GetStdHandle,
                STD_OUTPUT_HANDLE,
            },
            Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        },
    };

    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

    pub fn main() -> Result<()> {
        let mut args = std::env::args().skip(1);
        match args.next().as_deref() {
            Some(bootstrap::MXC_HELPER) => {
                let endpoint = args.next().ok_or("missing helper endpoint")?.parse()?;
                let host = args.next().ok_or("missing helper host PID")?.parse()?;
                if args.next().is_some() {
                    return Err("unexpected helper argument".into());
                }
                if std::env::current_exe()?
                    .file_stem()
                    .and_then(|name| name.to_str())
                    == Some("windows_mxc_hung")
                {
                    return hung_helper();
                }
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?
                    .block_on(bootstrap::serve_mxc(endpoint, host))?;
                Ok(())
            }
            Some("__fixture") => worker(args.collect()),
            Some(_) => Err("windows_mxc accepts no user arguments".into()),
            None => tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?
                .block_on(run()),
        }
    }

    struct Fixture {
        directory: tempfile::TempDir,
        state: PathBuf,
        work: PathBuf,
        executable: PathBuf,
        backend: Arc<Backend>,
    }

    // Observe the real prepared proxy without retaining its owner or replacing
    // backend selection, policy compilation, admission or cleanup.
    struct ObservedBackend {
        backend: Backend,
        prepared: Mutex<Option<tokio::sync::oneshot::Sender<SocketAddr>>>,
    }

    impl bootstrap::Backend for ObservedBackend {
        fn prepare(
            &self,
            filesystem: Policy,
            network: Network,
            route: maka_network::Policy,
            executable: PathBuf,
            cwd: PathBuf,
        ) -> bootstrap::Preparation {
            let preparation = bootstrap::Backend::prepare(
                &self.backend,
                filesystem,
                network,
                route,
                executable,
                cwd,
            );
            let sender = self
                .prepared
                .lock()
                .expect("fixture observer lock")
                .take()
                .expect("one fixture launch");
            Box::pin(async move {
                let launch = preparation.await?;
                let address = match &launch {
                    bootstrap::Launch::Mxc {
                        proxy_address: Some(address),
                        ..
                    } => *address,
                    _ => {
                        return Err(io::Error::other(
                            "fixture requires a real MXC execution proxy",
                        ));
                    }
                };
                sender
                    .send(address)
                    .map_err(|_| io::Error::other("fixture proxy observer closed"))?;
                Ok(launch)
            })
        }
    }

    impl Fixture {
        fn new() -> Result<Self> {
            let local =
                PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("missing LOCALAPPDATA")?);
            let directory = tempfile::Builder::new()
                .prefix("maka-mxc-")
                .tempdir_in(local)?;
            let state = directory.path().join("state");
            let work = directory.path().join("workspace 中文");
            std::fs::create_dir(&work)?;
            let executable = std::env::current_exe()?;
            let backend = Arc::new(Backend::new(&state, &executable));
            Ok(Self {
                directory,
                state,
                work,
                executable,
                backend,
            })
        }

        fn filesystem_policy(&self) -> Policy {
            Policy {
                default: Access::Read,
                rules: vec![
                    Rule::subtree(&self.work, Access::Write),
                    Rule::subtree(self.work.join(".git"), Access::Read),
                    Rule::subtree(self.work.join(".agents"), Access::Read),
                    Rule::subtree(self.work.join(".maka"), Access::Read),
                    Rule::exact(self.work.join(".maka-workspace.json"), Access::Read),
                    Rule::subtree(self.work.join("private"), Access::Deny),
                ],
                deny_globs: vec![],
            }
        }

        fn command(&self, role: &str, network: Network) -> Result<Command> {
            let mut command = Command::new(&self.executable, &self.work)
                .with_backend(self.backend.clone())
                .sandbox(&Sandbox::Managed {
                    filesystem: self.filesystem_policy(),
                    network,
                })?;
            command.args(["__fixture", role]);
            Ok(command)
        }

        async fn capture(&self, command: Command) -> Result<String> {
            let mut spawned = pipe::spawn(command).await?;
            drop(spawned.stdin);
            let mut output = String::new();
            let mut errors = String::new();
            let (status, out, err) = tokio::join!(
                spawned.child.wait(),
                spawned.stdout.read_to_string(&mut output),
                spawned.stderr.read_to_string(&mut errors),
            );
            out?;
            err?;
            assert_eq!(
                status?.code(),
                Some(0),
                "stdout: {output}\nstderr: {errors}"
            );
            assert_eq!(errors, "");
            Ok(output)
        }
    }

    async fn run() -> Result<()> {
        assert!(
            maka_sandbox::windows::mxc::is_available(),
            "native MXC capability probe failed"
        );
        let fixture = Fixture::new()?;
        assert_eq!(
            Installation::new(&fixture.state).status()?,
            Status::NotConfigured
        );
        assert_eq!(windows::status(&fixture.state)?, Status::NotRequired);

        filesystem(&fixture).await?;
        nested_filesystem(&fixture).await?;
        missing_metadata(&fixture).await?;
        filesystem_aliases(&fixture).await?;
        protected_ancestor(&fixture).await?;
        unsupported_reopening(&fixture).await?;
        arguments_and_environment(&fixture).await?;
        pipe_input(&fixture).await?;
        cancel_hung_admission(&fixture).await?;
        cancel_tree(&fixture).await?;
        terminal(&fixture).await?;
        background(&fixture).await?;
        network(&fixture).await?;

        assert_eq!(
            Installation::new(&fixture.state).status()?,
            Status::NotConfigured
        );
        assert!(
            !fixture.state.exists(),
            "MXC must not provision an account installation"
        );
        println!(
            "PASS native Windows MXC: Host selection, filesystem, argv/environment, pipes, cancellation, ConPTY, background lifetime, network"
        );
        Ok(())
    }

    async fn filesystem(fixture: &Fixture) -> Result<()> {
        std::fs::create_dir(fixture.work.join(".git"))?;
        std::fs::write(fixture.work.join(".git/config"), "metadata-original")?;
        std::fs::create_dir(fixture.work.join("private"))?;
        std::fs::write(fixture.work.join("private/secret"), "private-original")?;
        let outside = fixture.directory.path().join("outside");
        std::fs::write(&outside, "outside-original")?;
        let mut command = fixture.command("filesystem", Network::Denied)?;
        command.arg(&outside);
        assert_eq!(fixture.capture(command).await?, "filesystem-ok\n");
        assert_eq!(
            std::fs::read_to_string(fixture.work.join("文件 with spaces.txt"))?,
            "写入 😀"
        );
        assert_eq!(std::fs::read_to_string(&outside)?, "outside-original");
        assert_eq!(
            std::fs::read_to_string(fixture.work.join(".git/config"))?,
            "metadata-original"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.work.join("private/secret"))?,
            "private-original"
        );
        assert!(!fixture.work.join(".maka").exists());

        let readonly =
            fixture
                .command("read-only", Network::Denied)?
                .sandbox(&Sandbox::Managed {
                    filesystem: Policy::uniform(Access::Read),
                    network: Network::Denied,
                })?;
        assert_eq!(fixture.capture(readonly).await?, "read-only-ok\n");
        let mut unrestricted = fixture
            .command("unrestricted", Network::Denied)?
            .sandbox(&Sandbox::Disabled)?;
        unrestricted.arg(&outside);
        assert_eq!(fixture.capture(unrestricted).await?, "unrestricted-ok\n");
        assert_eq!(std::fs::read_to_string(&outside)?, "unrestricted-write");
        println!(
            "PASS filesystem: read-only/workspace-write/disabled, outside and private denial, missing metadata protection"
        );
        Ok(())
    }

    async fn nested_filesystem(fixture: &Fixture) -> Result<()> {
        let read_parent = fixture.directory.path().join("read-parent");
        let write_child = read_parent.join("write-child");
        let write_parent = fixture.work.join("write-parent");
        let read_child = write_parent.join("read-child");
        let write_grandchild = read_child.join("write-grandchild");
        std::fs::create_dir_all(&write_child)?;
        std::fs::create_dir_all(&write_grandchild)?;
        std::fs::write(read_parent.join("original"), "read-parent-original")?;
        std::fs::write(read_child.join("original"), "read-child-original")?;
        // Host writes prove the negative cases are not filesystem ACL failures.
        std::fs::write(read_parent.join("original"), "host-positive-control")?;
        std::fs::write(read_parent.join("original"), "read-parent-original")?;
        std::fs::write(read_child.join("original"), "host-positive-control")?;
        std::fs::write(read_child.join("original"), "read-child-original")?;
        let mut filesystem = fixture.filesystem_policy();
        filesystem.rules.extend([
            Rule::subtree(&read_parent, Access::Read),
            Rule::subtree(&write_child, Access::Write),
            Rule::subtree(&read_child, Access::Read),
            Rule::subtree(&write_grandchild, Access::Write),
        ]);
        let mut command = fixture
            .command("nested-filesystem", Network::Denied)?
            .sandbox(&Sandbox::Managed {
                filesystem,
                network: Network::Denied,
            })?;
        command.arg(&read_parent).arg(&write_parent);
        assert_eq!(fixture.capture(command).await?, "nested-filesystem-ok\n");
        for (path, expected) in [
            (read_parent.join("original"), "read-parent-original"),
            (write_child.join("written"), "read-to-write"),
            (write_parent.join("written"), "parent-write"),
            (read_child.join("original"), "read-child-original"),
            (write_grandchild.join("written"), "write-read-write"),
        ] {
            assert_eq!(std::fs::read_to_string(path)?, expected);
        }
        println!("PASS nested Read→Write and Write→Read→Write permissions");
        Ok(())
    }

    async fn missing_metadata(fixture: &Fixture) -> Result<()> {
        let root = fixture.work.join("new-workspace");
        std::fs::create_dir(&root)?;
        let mut filesystem = fixture.filesystem_policy();
        for name in [".git", ".agents", ".maka"] {
            let path = root.join(name);
            std::fs::create_dir(&path)?;
            std::fs::remove_dir(&path)?;
            filesystem.rules.push(Rule::subtree(path, Access::Read));
        }
        let marker = root.join(".maka-workspace.json");
        std::fs::write(&marker, "host-positive-control")?;
        std::fs::remove_file(&marker)?;
        filesystem.rules.push(Rule::exact(&marker, Access::Read));
        let mut command = fixture
            .command("missing-metadata", Network::Denied)?
            .sandbox(&Sandbox::Managed {
                filesystem,
                network: Network::Denied,
            })?;
        command.arg(&root);
        assert_eq!(fixture.capture(command).await?, "missing-metadata-ok\n");
        assert_eq!(
            std::fs::read_to_string(root.join("ordinary"))?,
            "ordinary-write"
        );
        for name in [".git", ".agents", ".maka", ".maka-workspace.json"] {
            assert!(
                !root.join(name).exists(),
                "created protected metadata: {name}"
            );
        }
        println!("PASS missing .git/.agents/.maka and exact workspace marker cannot be created");
        Ok(())
    }

    async fn filesystem_aliases(fixture: &Fixture) -> Result<()> {
        let outside = fixture.directory.path().join("hardlink-original");
        let hardlink = fixture.work.join("hardlink-alias");
        std::fs::write(&outside, "hardlink-original")?;
        std::fs::hard_link(&outside, &hardlink)?;
        std::fs::write(&hardlink, "host-hardlink-write")?;
        assert_eq!(std::fs::read_to_string(&outside)?, "host-hardlink-write");
        std::fs::write(&outside, "hardlink-original")?;

        let outside_directory = fixture.directory.path().join("junction-original");
        std::fs::create_dir(&outside_directory)?;
        std::fs::write(outside_directory.join("original"), "junction-original")?;
        let junction = fixture.work.join("junction-alias");
        junction_directory(&junction, &outside_directory)?;
        std::fs::write(junction.join("original"), "host-junction-write")?;
        assert_eq!(
            std::fs::read_to_string(outside_directory.join("original"))?,
            "host-junction-write"
        );
        std::fs::write(outside_directory.join("original"), "junction-original")?;

        let private_alias = fixture.work.join("private-alias");
        junction_directory(&private_alias, &fixture.work.join("private"))?;
        std::fs::write(private_alias.join("secret"), "host-private-alias-write")?;
        assert_eq!(
            std::fs::read_to_string(fixture.work.join("private/secret"))?,
            "host-private-alias-write"
        );
        std::fs::write(fixture.work.join("private/secret"), "private-original")?;

        assert_eq!(
            fixture
                .capture(fixture.command("filesystem-aliases", Network::Denied)?)
                .await?,
            "filesystem-aliases-ok\n"
        );
        assert_eq!(std::fs::read_to_string(&outside)?, "hardlink-original");
        assert_eq!(
            std::fs::read_to_string(outside_directory.join("original"))?,
            "junction-original"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.work.join("private/secret"))?,
            "private-original"
        );
        std::fs::remove_dir(private_alias)?;
        std::fs::remove_dir(junction)?;
        println!("PASS hardlink and junction aliases cannot reopen readonly or denied objects");
        Ok(())
    }

    async fn protected_ancestor(fixture: &Fixture) -> Result<()> {
        let ancestor = fixture.work.join("protected-ancestor");
        let moved = fixture.work.join("moved-ancestor");
        let metadata = ancestor.join("metadata");
        std::fs::create_dir_all(&metadata)?;
        std::fs::write(metadata.join("original"), "protected-original")?;
        std::fs::rename(&ancestor, &moved)?;
        std::fs::rename(&moved, &ancestor)?;
        let mut filesystem = fixture.filesystem_policy();
        filesystem
            .rules
            .push(Rule::subtree(&metadata, Access::Read));
        let command = fixture
            .command("protected-ancestor", Network::Denied)?
            .sandbox(&Sandbox::Managed {
                filesystem,
                network: Network::Denied,
            })?;
        assert_eq!(fixture.capture(command).await?, "protected-ancestor-ok\n");
        assert_eq!(
            std::fs::read_to_string(metadata.join("original"))?,
            "protected-original"
        );
        assert!(!moved.exists());
        println!("PASS writable ancestors cannot be renamed to bypass protected descendants");
        Ok(())
    }

    async fn unsupported_reopening(fixture: &Fixture) -> Result<()> {
        let reopened = fixture.work.join("private/reopened");
        std::fs::create_dir(&reopened)?;
        let mut filesystem = fixture.filesystem_policy();
        filesystem
            .rules
            .push(Rule::subtree(&reopened, Access::Write));
        assert!(matches!(
            maka_sandbox::windows::mxc::PreparedPolicy::new(&filesystem, &Network::Denied, &fixture.work),
            Err(maka_sandbox::Error::Unsupported(reason)) if reason == "native MXC cannot reopen a descendant of a denied path"
        ));
        let command =
            fixture
                .command("must-not-run", Network::Denied)?
                .sandbox(&Sandbox::Managed {
                    filesystem,
                    network: Network::Denied,
                })?;
        let failure = match pipe::spawn(command).await {
            Err(failure) => failure,
            Ok(_) => return Err("unsupported denial reopening executed a command".into()),
        };
        assert!(
            failure
                .to_string()
                .contains("native MXC cannot reopen a descendant of a denied path"),
            "{failure}"
        );
        assert!(!fixture.work.join("unexpected-execution").exists());
        println!("PASS unrepresentable Deny→Write policy rejects before execution");
        Ok(())
    }

    fn junction_directory(link: &Path, target: &Path) -> Result<()> {
        let cmd = PathBuf::from(std::env::var_os("SystemRoot").ok_or("missing SystemRoot")?)
            .join("System32/cmd.exe");
        let result = std::process::Command::new(cmd)
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()?;
        assert!(
            result.status.success(),
            "junction positive control failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(())
    }

    async fn arguments_and_environment(fixture: &Fixture) -> Result<()> {
        let mut command = fixture.command("arguments", Network::Denied)?;
        command.arg("quoted \"value\"").arg("trailing\\");
        command.raw_arg(r#""raw 空格" "" "two words""#);
        let output = fixture.capture(command).await?;
        assert_eq!(
            serde_json::from_str::<Vec<String>>(output.trim())?,
            [
                "quoted \"value\"",
                "trailing\\",
                "raw 空格",
                "",
                "two words"
            ]
        );

        let mut command = fixture.command("environment", Network::Denied)?;
        command
            .env_clear()
            .env("MAKA_FIXTURE", "仅此 😀")
            .env("sYsTeMrOoT", r"Q:\poison")
            .env("SYSTEMDRIVE", "Q:")
            .env("localappdata", r"Q:\poison");
        let output = fixture.capture(command).await?;
        let value: serde_json::Value = serde_json::from_str(output.trim())?;
        assert_eq!(
            value,
            json!({
                "marker": "仅此 😀",
                "path": null,
                "userprofile": null,
                "SystemRoot": std::env::var("SystemRoot")?,
                "SystemDrive": std::env::var("SystemDrive")?,
                "LOCALAPPDATA": std::env::var("LOCALAPPDATA")?,
            })
        );
        println!("PASS raw/quoted arguments and env_clear with poisoned startup locations");
        Ok(())
    }

    async fn pipe_input(fixture: &Fixture) -> Result<()> {
        let mut spawned = pipe::spawn(fixture.command("input", Network::Denied)?).await?;
        spawned.stdin.write_all("输入 😀\n".as_bytes()).await?;
        drop(spawned.stdin);
        let mut output = String::new();
        let mut errors = String::new();
        let (status, out, err) = tokio::join!(
            spawned.child.wait(),
            spawned.stdout.read_to_string(&mut output),
            spawned.stderr.read_to_string(&mut errors)
        );
        out?;
        err?;
        assert_eq!(status?.code(), Some(0));
        assert_eq!(output, "input:输入 😀\n");
        assert_eq!(errors, "");
        println!("PASS bidirectional pipe input/output");
        Ok(())
    }

    async fn cancel_hung_admission(fixture: &Fixture) -> Result<()> {
        let helper = fixture.directory.path().join("windows_mxc_hung.exe");
        std::fs::copy(&fixture.executable, &helper)?;
        let marker = helper.with_extension("ready");
        let (sender, prepared) = tokio::sync::oneshot::channel();
        let backend = Arc::new(ObservedBackend {
            backend: Backend::new(&fixture.state, &helper),
            prepared: Mutex::new(Some(sender)),
        });
        let cancellation = CancellationToken::new();
        let command = fixture
            .command(
                "must-not-run",
                Network::destination(Destination::new("example.invalid", 443)?),
            )?
            .with_backend(backend)
            .with_startup_cancellation(cancellation.clone());
        let mut startup = tokio::spawn(async move { pipe::spawn(command).await });
        let proxy = prepared.await?;
        // The listener belongs to the real production launch, not the observer.
        drop(tokio::net::TcpStream::connect(proxy).await?);
        let pids: [u32; 2] = loop {
            tokio::select! {
                outcome = &mut startup => {
                    match outcome? {
                        Ok(mut spawned) => {
                            spawned.child.terminate()?;
                            spawned.child.wait().await?;
                            return Err("hung-helper admission unexpectedly started a command".into());
                        }
                        Err(failure) => return Err(format!("admission ended before the hung-helper ready signal: {failure}").into()),
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(10)) => {
                    match std::fs::read(&marker) {
                        Ok(bytes) => break serde_json::from_slice(&bytes)?,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        };
        let helper_process = process_handle(pids[0])?;
        let descendant = process_handle(pids[1])?;
        assert_running(&helper_process);
        assert_running(&descendant);
        cancellation.cancel();
        let failure = match startup.await? {
            Err(failure) => failure,
            Ok(mut spawned) => {
                spawned.child.terminate()?;
                spawned.child.wait().await?;
                return Err("cancelled hung-helper admission started a command".into());
            }
        };
        assert!(
            matches!(failure, maka_agent_facts::tools::ToolError::Failed(ref message) if message == "sandbox launch cancelled"),
            "{failure}"
        );
        assert_exited(&helper_process);
        assert_exited(&descendant);
        assert!(
            tokio::net::TcpStream::connect(proxy).await.is_err(),
            "cancelled startup retained its execution proxy"
        );
        assert!(!fixture.work.join("unexpected-execution").exists());
        drop((helper_process, descendant));
        std::fs::remove_file(marker)?;
        std::fs::remove_file(helper)?;
        println!(
            "PASS startup cancellation settles a hung trusted helper, its descendants and its real execution proxy"
        );
        Ok(())
    }

    fn hung_helper() -> Result<()> {
        let executable = std::env::current_exe()?;
        let mut descendant = std::process::Command::new(&executable)
            .args(["__fixture", "sleeper"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()?;
        let mut ready = String::new();
        io::BufReader::new(descendant.stdout.take().unwrap()).read_line(&mut ready)?;
        assert_eq!(ready.trim_end(), "descendant-ready");
        let pending = executable.with_extension("ready.tmp");
        std::fs::write(
            &pending,
            serde_json::to_vec(&[std::process::id(), descendant.id()])?,
        )?;
        std::fs::rename(pending, executable.with_extension("ready"))?;
        // Deliberately never connect to the authenticated startup endpoint.
        // Only the owner's cancellation may complete this admission attempt.
        loop {
            std::thread::park();
        }
    }

    async fn cancel_tree(fixture: &Fixture) -> Result<()> {
        let cancellation = CancellationToken::new();
        let ObservedProcess {
            mut events,
            completion,
        } = fixture
            .command("cancel-tree", Network::Denied)?
            .observe(None, cancellation.clone())?;
        let completion = tokio::spawn(completion);
        let mut output = Vec::new();
        while !output.contains(&b'\n') {
            match events
                .recv()
                .await
                .ok_or("process ended before descendant startup")?
            {
                PipeEvent::Output { bytes, .. } => output.extend(bytes),
                PipeEvent::Started => {}
            }
        }
        let line = String::from_utf8(output)?;
        let descendant = process_handle(descendant_pid(&line)?)?;
        assert_running(&descendant);
        cancellation.cancel();
        drop(events);
        let captured = completion.await??;
        assert!(matches!(
            captured.outcome,
            ProcessOutcome::Interrupted {
                reason: Termination::Cancelled,
                signal_applied: true
            }
        ));
        assert_eq!(captured.stdout, line);
        assert_eq!(captured.stderr, "");
        assert_exited(&descendant);
        println!("PASS foreground cancellation settles the complete descendant tree");
        Ok(())
    }

    async fn terminal(fixture: &Fixture) -> Result<()> {
        let (mut child, io) = pty::spawn(
            fixture.command("terminal", Network::Denied)?,
            TerminalSize::new(80, 24)?,
        )
        .await?;
        read_until(&io, "input-ready").await?;
        child.resize(TerminalSize::new(120, 41)?).await?;
        write_all(&io, b"terminal-input\r").await?;
        let output = read_until(&io, "terminal-ok").await?;
        assert!(output.contains("input:terminal-input"), "{output}");
        assert!(output.contains("size:120x41"), "{output}");
        assert_eq!(child.wait().await?.code(), Some(0));
        let (closed, drained) = tokio::join!(child.close(), drain(&io));
        closed?;
        drained?;
        println!("PASS ConPTY input, resize, output and close/drain");
        Ok(())
    }

    async fn background(fixture: &Fixture) -> Result<()> {
        let (mut child, io) = pty::spawn(
            fixture.command("background", Network::Denied)?,
            TerminalSize::new(80, 24)?,
        )
        .await?;
        let output = read_until(&io, "descendant:").await?;
        let mut complete = output;
        while !complete[complete.rfind("descendant:").unwrap()..].contains('\n') {
            let mut buffer = [0; 4096];
            let size = io.read(&mut buffer).await?;
            assert_ne!(size, 0, "missing descendant PID: {complete}");
            complete.push_str(&String::from_utf8_lossy(&buffer[..size]));
        }
        let line = complete[complete.rfind("descendant:").unwrap()..]
            .lines()
            .next()
            .unwrap();
        let descendant = process_handle(descendant_pid(line)?)?;
        assert_eq!(child.wait().await?.code(), Some(0));
        assert_running(&descendant);
        child.resize(TerminalSize::new(100, 30)?).await?;
        let (closed, drained) = tokio::join!(child.close(), drain(&io));
        closed?;
        drained?;
        assert_exited(&descendant);
        println!(
            "PASS root exit keeps background processes isolated until explicit owner settlement"
        );
        Ok(())
    }

    async fn network(fixture: &Fixture) -> Result<()> {
        let v4 = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let v6 = tokio::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).await?;
        let addresses = [v4.local_addr()?, v6.local_addr()?];
        for address in addresses {
            tokio::net::TcpStream::connect(address).await?;
        }
        let mut denied = fixture.command("network-denied", Network::Denied)?;
        denied.args(addresses.map(|address| address.to_string()));
        assert_eq!(fixture.capture(denied).await?, "blocked-v4\nblocked-v6\n");

        let allowed = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let destination = allowed.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = allowed.accept().await?;
            let mut request = String::new();
            let mut reader = tokio::io::BufReader::new(&mut socket);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).await? == 0 || line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            assert!(
                request.starts_with("GET /allowed HTTP/1.1\r\n"),
                "{request}"
            );
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nproxy-ok",
                )
                .await?;
            Ok::<_, io::Error>(())
        });
        let mut restricted = fixture.command(
            "network-restricted",
            Network::destination(Destination::new("127.0.0.1", destination.port())?),
        )?;
        restricted.args(addresses.map(|address| address.to_string()));
        restricted.arg(destination.to_string());
        assert_eq!(
            fixture.capture(restricted).await?,
            "loopback-v4\nloopback-v6\nproxy-allowed\nproxy-denied\n"
        );
        server.await??;
        println!(
            "PASS denied IPv4/IPv6, Codex-compatible local loopback, destination-enforcing execution proxy"
        );
        Ok(())
    }

    fn worker(args: Vec<String>) -> Result<()> {
        match args.first().map(String::as_str) {
            Some("filesystem") => {
                std::fs::write("文件 with spaces.txt", "写入 😀")?;
                denied_write(Path::new(&args[1]));
                denied_write(Path::new(".git/config"));
                denied_write(Path::new("private/secret"));
                assert_eq!(
                    std::fs::read("private/secret").unwrap_err().raw_os_error(),
                    Some(5)
                );
                assert_eq!(
                    std::fs::create_dir(".maka").unwrap_err().raw_os_error(),
                    Some(5)
                );
                println!("filesystem-ok");
            }
            Some("read-only") => {
                assert_eq!(std::fs::read_to_string("文件 with spaces.txt")?, "写入 😀");
                denied_write(Path::new("文件 with spaces.txt"));
                println!("read-only-ok");
            }
            Some("unrestricted") => {
                assert_eq!(
                    std::fs::read_to_string("private/secret")?,
                    "private-original"
                );
                std::fs::write(&args[1], "unrestricted-write")?;
                println!("unrestricted-ok");
            }
            Some("nested-filesystem") => {
                let read_parent = Path::new(&args[1]);
                let write_parent = Path::new(&args[2]);
                assert_eq!(
                    std::fs::read_to_string(read_parent.join("original"))?,
                    "read-parent-original"
                );
                denied_write(&read_parent.join("original"));
                std::fs::write(read_parent.join("write-child/written"), "read-to-write")?;
                std::fs::write(write_parent.join("written"), "parent-write")?;
                denied_write(&write_parent.join("read-child/original"));
                std::fs::write(
                    write_parent.join("read-child/write-grandchild/written"),
                    "write-read-write",
                )?;
                println!("nested-filesystem-ok");
            }
            Some("missing-metadata") => {
                let root = Path::new(&args[1]);
                std::fs::write(root.join("ordinary"), "ordinary-write")?;
                for name in [".git", ".agents", ".maka"] {
                    assert_eq!(
                        std::fs::create_dir(root.join(name))
                            .unwrap_err()
                            .raw_os_error(),
                        Some(5),
                        "{name}"
                    );
                }
                denied_write(&root.join(".maka-workspace.json"));
                println!("missing-metadata-ok");
            }
            Some("filesystem-aliases") => {
                denied_write(Path::new("hardlink-alias"));
                denied_write(Path::new("junction-alias/original"));
                denied_write(Path::new("private-alias/secret"));
                assert_eq!(
                    std::fs::read("private-alias/secret")
                        .unwrap_err()
                        .raw_os_error(),
                    Some(5)
                );
                println!("filesystem-aliases-ok");
            }
            Some("protected-ancestor") => {
                std::fs::write("protected-ancestor/ordinary", "ordinary-write")?;
                assert!(
                    std::fs::rename("protected-ancestor", "moved-ancestor").is_err(),
                    "renaming a writable ancestor bypassed its readonly descendant"
                );
                denied_write(Path::new("protected-ancestor/metadata/original"));
                println!("protected-ancestor-ok");
            }
            Some("must-not-run") => {
                std::fs::write("unexpected-execution", "policy-was-bypassed")?;
            }
            Some("arguments") => println!("{}", serde_json::to_string(&args[1..])?),
            Some("environment") => println!(
                "{}",
                json!({
                    "marker": std::env::var("MAKA_FIXTURE").ok(),
                    "path": std::env::var("PATH").ok(),
                    "userprofile": std::env::var("USERPROFILE").ok(),
                    "SystemRoot": std::env::var("SystemRoot").ok(),
                    "SystemDrive": std::env::var("SystemDrive").ok(),
                    "LOCALAPPDATA": std::env::var("LOCALAPPDATA").ok(),
                })
            ),
            Some("input") => {
                let mut text = String::new();
                io::stdin().read_to_string(&mut text)?;
                print!("input:{text}");
            }
            Some("terminal") => {
                println!("input-ready");
                io::stdout().flush()?;
                let mut text = String::new();
                io::stdin().read_line(&mut text)?;
                println!("input:{}", text.trim_end());
                let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
                // SAFETY: the fixture inherits a real ConPTY console output handle.
                if unsafe { GetConsoleScreenBufferInfo(GetStdHandle(STD_OUTPUT_HANDLE), &mut info) }
                    == 0
                {
                    return Err(io::Error::last_os_error().into());
                }
                println!(
                    "size:{}x{}",
                    info.dwSize.X,
                    info.srWindow.Bottom - info.srWindow.Top + 1
                );
                println!("terminal-ok");
            }
            Some("sleeper") => {
                println!("descendant-ready");
                io::stdout().flush()?;
                loop {
                    std::thread::park();
                }
            }
            Some(role @ ("background" | "cancel-tree")) => {
                let mut child = std::process::Command::new(std::env::current_exe()?)
                    .args(["__fixture", "sleeper"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .spawn()?;
                let mut ready = String::new();
                io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut ready)?;
                assert_eq!(ready.trim_end(), "descendant-ready");
                println!("descendant:{}", child.id());
                io::stdout().flush()?;
                if role == "cancel-tree" {
                    loop {
                        std::thread::park();
                    }
                }
            }
            Some("network-denied") => {
                for (address, marker) in args[1..].iter().zip(["blocked-v4", "blocked-v6"]) {
                    assert!(
                        TcpStream::connect_timeout(&address.parse()?, Duration::from_secs(10))
                            .is_err(),
                        "network escaped: {address}"
                    );
                    println!("{marker}");
                }
            }
            Some("network-restricted") => {
                for (address, marker) in args[1..3].iter().zip(["loopback-v4", "loopback-v6"]) {
                    TcpStream::connect_timeout(&address.parse()?, Duration::from_secs(10))?;
                    println!("{marker}");
                }
                let proxy = std::env::var("HTTP_PROXY")?;
                let proxy = proxy
                    .strip_prefix("http://")
                    .ok_or("invalid execution proxy URL")?
                    .trim_end_matches('/')
                    .parse()?;
                let response = proxy_request(proxy, &format!("http://{}/allowed", args[3]))?;
                assert!(response.starts_with("HTTP/1.1 200 "), "{response}");
                assert!(response.ends_with("proxy-ok"), "{response}");
                println!("proxy-allowed");
                let response = proxy_request(proxy, &format!("http://{}/denied", args[1]))?;
                assert!(response.starts_with("HTTP/1.1 403 "), "{response}");
                println!("proxy-denied");
            }
            _ => return Err("unknown fixture worker role".into()),
        }
        io::stdout().flush()?;
        Ok(())
    }

    fn denied_write(path: &Path) {
        assert_eq!(
            std::fs::write(path, "escaped").unwrap_err().raw_os_error(),
            Some(5),
            "{}",
            path.display()
        );
    }

    fn proxy_request(proxy: SocketAddr, destination: &str) -> Result<String> {
        let mut socket = TcpStream::connect_timeout(&proxy, Duration::from_secs(10))?;
        socket.set_read_timeout(Some(Duration::from_secs(10)))?;
        write!(
            socket,
            "GET {destination} HTTP/1.1\r\nHost: fixture\r\nConnection: close\r\n\r\n"
        )?;
        let mut output = String::new();
        socket.read_to_string(&mut output)?;
        Ok(output)
    }

    fn descendant_pid(line: &str) -> Result<u32> {
        let value = line
            .trim()
            .strip_prefix("descendant:")
            .ok_or("missing descendant marker")?;
        // ConPTY may append cursor-control bytes after the PID on the same line.
        let end = value
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(value.len());
        Ok(value[..end].parse()?)
    }

    fn process_handle(pid: u32) -> Result<OwnedHandle> {
        // SAFETY: opens one owned synchronization handle, retained to prevent PID reuse.
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
        if handle.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: successful OpenProcess transfers one fresh owned handle.
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }

    fn assert_running(process: &OwnedHandle) {
        // SAFETY: the owned handle remains valid throughout the nonblocking observation.
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle(), 0) },
            WAIT_TIMEOUT
        );
    }

    fn assert_exited(process: &OwnedHandle) {
        // SAFETY: settlement has completed; this checks the exact retained process object.
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
    }

    async fn read_until(io: &pty::PtyIo, marker: &str) -> Result<String> {
        let mut output = Vec::new();
        while !String::from_utf8_lossy(&output).contains(marker) {
            let mut buffer = [0; 4096];
            let size = io.read(&mut buffer).await?;
            assert_ne!(
                size,
                0,
                "PTY ended before {marker}: {}",
                String::from_utf8_lossy(&output)
            );
            output.extend_from_slice(&buffer[..size]);
        }
        Ok(String::from_utf8(output)?)
    }

    async fn write_all(io: &pty::PtyIo, bytes: &[u8]) -> Result<()> {
        let mut written = 0;
        while written < bytes.len() {
            let size = io.write(&bytes[written..]).await?;
            assert_ne!(size, 0);
            written += size;
        }
        Ok(())
    }

    async fn drain(io: &pty::PtyIo) -> Result<()> {
        let mut buffer = [0; 4096];
        while io.read(&mut buffer).await? != 0 {}
        Ok(())
    }
}
