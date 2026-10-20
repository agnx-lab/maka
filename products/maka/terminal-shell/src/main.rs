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

//! Runs the NSH application against one launcher-selected public Host.

use clap::Parser;
use maka_app_client::{Client, Journal};
use maka_event_log::root;
use maka_host_runtime::application::host::{Client as HostClient, discovery};
use maka_plugin_core::operation::receipt_reader_contract;
use maka_terminal_shell::{APPLICATION, bindings::Bindings, terminal};
use nsh_core::{
    Size,
    platform::{Environment, Idiom, Renderer},
};
use nsh_engine::{Bundle, EngineConfig, Limits, Policy};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Parser)]
#[command(
    name = "maka-terminal-shell",
    version,
    about = "Maka terminal application using NSH"
)]
struct Args {
    /// Existing business Host Root selected by the native launcher.
    #[arg(long)]
    root: PathBuf,
    /// Existing Agent session to display initially; otherwise open the session picker.
    #[arg(long)]
    session: Option<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build();
    let result = match runtime {
        Ok(runtime) => runtime.block_on(args.run()),
        Err(error) => Err(format!("runtime startup: {error}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(error);
            ExitCode::FAILURE
        }
    }
}

fn report(error: String) {
    let (sent, received) = std::sync::mpsc::sync_channel(1);
    if std::thread::Builder::new()
        .name("maka-terminal-diagnostic".into())
        .spawn(move || {
            let result = writeln!(io::stderr().lock(), "maka-terminal-shell: {error}");
            let _ = sent.send(result);
        })
        .is_ok()
    {
        // stderr can share the same blocked terminal as stdout. Diagnostic
        // delivery must not hold the already closed client scope alive.
        let _ = received.recv_timeout(std::time::Duration::from_millis(100));
    }
}

impl Args {
    async fn run(self) -> Result<(), String> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err("the terminal shell requires a terminal on stdin and stdout".into());
        }
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.is_empty() || session.len() > 128)
        {
            return Err("session must contain between 1 and 128 bytes".into());
        }
        let stop = Arc::new(AtomicBool::new(false));
        // Install before opening any Host session or entering terminal modes.
        let _signals = Signals::watch(stop.clone()).map_err(|error| error.to_string())?;
        let location = root::resolve(&self.root).map_err(|error| error.to_string())?;
        let registered =
            discovery::read(location.canonical_path()).map_err(|error| error.to_string())?;
        let status = HostClient::status(&registered.endpoint, Some(registered.host_epoch)).await?;
        registered
            .verify(&status)
            .map_err(|error| error.to_string())?;
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut native =
            HostClient::connect(&registered.endpoint, Some(registered.host_epoch)).await?;
        let prepare = async {
            if native.source() != &registered.source
                || native.source().state_domain != location.root_id()
            {
                return Err("application Host changed during terminal startup admission".to_owned());
            }
            if native.catalogue().describe("receipts")?.contract != receipt_reader_contract() {
                return Err("Host receipts endpoint has a different contract".into());
            }
            let directory = location.canonical_path().join("clients");
            root::private_directory(&directory).map_err(|error| error.to_string())?;
            Journal::open(&directory.join("terminal.sqlite")).await
        }
        .await;
        let journal = match prepare {
            Ok(journal) => journal,
            Err(error) => {
                return match native.close().await {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(format!("{error}; terminal Host cleanup: {cleanup}")),
                };
            }
        };
        let client = Client::new(native, journal, "receipts".into())?;
        let runtime = tokio::runtime::Handle::current();
        let bindings = Bindings::new(client, runtime.clone());
        let config = configuration(bindings.registry(self.session), runtime);
        let result = if stop.load(Ordering::Acquire) {
            Ok(())
        } else {
            tokio::task::spawn_blocking(move || terminal::run(config, stop))
                .await
                .map_err(|error| format!("terminal worker: {error}"))
                .and_then(|result| result)
        };
        let cleanup = bindings.close().await;
        match (result, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(format!("terminal Host cleanup: {error}")),
            (Err(error), Err(cleanup)) => Err(format!("{error}; terminal Host cleanup: {cleanup}")),
        }
    }
}

fn configuration(
    registry: nsh_core::ui::Registry,
    runtime: tokio::runtime::Handle,
) -> EngineConfig {
    let viewport = Size::new(960.0, 640.0);
    EngineConfig {
        app: Bundle::source(APPLICATION.into())
            .with_app_id(Some("org.apache.maka.terminal"))
            .with_policy(Policy {
                declared: Some(nsh_core::access::Capabilities {
                    clipboard: true,
                    ..Default::default()
                }),
                ..Default::default()
            })
            .into(),
        viewport,
        trace: false,
        network: None,
        assets: None,
        environment: Environment {
            renderer: Renderer::Terminal,
            idiom: Idiom::Desktop,
            window: (viewport.width, viewport.height),
            ..Default::default()
        },
        header_heights: (16.0, 32.0),
        // APPLICATION is compiled into this trusted executable; only the
        // interpreter stack is bounded, as for other standalone NSH apps.
        limits: Limits::standalone(),
        ui_sets: vec![nsh_ui_mobile::registry(), registry],
        runtime: Some(runtime),
        data_dir: None,
        storage: None,
    }
}

struct Signals(tokio::task::JoinHandle<()>);
impl Signals {
    fn watch(stop: Arc<AtomicBool>) -> io::Result<Self> {
        #[cfg(unix)]
        let (mut interrupt, mut terminate) = (
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?,
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
        );
        #[cfg(windows)]
        let (mut interrupt, mut terminate) = (
            tokio::signal::windows::ctrl_c()?,
            tokio::signal::windows::ctrl_break()?,
        );
        Ok(Self(tokio::spawn(async move {
            tokio::select! { _ = interrupt.recv() => {}, _ = terminate.recv() => {} }
            stop.store(true, Ordering::Release);
        })))
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        self.0.abort();
    }
}
