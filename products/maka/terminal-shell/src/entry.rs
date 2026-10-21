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

use crate::{APPLICATION, bindings::Bindings, terminal};
use maka_app_client::{Client, Journal};
use maka_event_log::root;
use maka_host_runtime::application::host::startup;
use maka_plugin_core::operation::receipt_reader_contract;
use nsh_core::{
    Points, Size,
    platform::{Environment, Idiom, Renderer},
};
use nsh_engine::{Bundle, EngineConfig, Limits, Policy};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Options shared with the `maka terminal-shell` command.
#[derive(clap::Args)]
#[command(version, about = "Maka terminal application using NSH")]
pub struct Args {
    /// Business Host Root (default: ~/.maka/application/host).
    #[arg(long)]
    root: Option<PathBuf>,
    /// Existing Agent session to display initially; otherwise open the session picker.
    #[arg(long)]
    session: Option<String>,
}

impl Args {
    fn root_path(&self) -> Result<PathBuf, String> {
        if let Some(root) = &self.root {
            return std::path::absolute(root).map_err(|error| error.to_string());
        }
        #[cfg(unix)]
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
            .ok_or("home directory unavailable; select --root")?;
        #[cfg(windows)]
        let home = root::windows::account_home().map_err(|error| error.to_string())?;
        Ok(home.join(".maka/application/host"))
    }

    /// Runs against the selected public Host using the caller's Tokio runtime.
    /// Host cleanup completes before an error is returned to the caller.
    pub async fn run(self) -> Result<(), String> {
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
        let mut signals = Signals::watch(stop.clone()).map_err(|error| error.to_string())?;
        let path = self.root_path()?;
        let launcher = std::env::current_exe().map_err(|error| error.to_string())?;
        let mut native = tokio::select! {
            result = startup::connect_root(&launcher, &path) => result?,
            _ = &mut signals.0 => return Ok(()),
        };
        if stop.load(Ordering::Acquire) {
            return native.close().await;
        }
        let prepare = async {
            let location = root::resolve(&path).map_err(|error| error.to_string())?;
            if native.source().state_domain != location.root_id() {
                return Err("application Host changed during terminal startup admission".to_owned());
            }
            if native.catalogue().describe("receipts")?.contract != receipt_reader_contract() {
                return Err("Host receipts endpoint has a different contract".into());
            }
            let directory = location.canonical_path().join("clients");
            root::private_directory(&directory).map_err(|error| error.to_string())?;
            let journal = Journal::open(&directory.join("terminal.sqlite")).await?;
            Ok((journal, directory))
        }
        .await;
        let (journal, client_directory) = match prepare {
            Ok(prepared) => prepared,
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
        let mut config = configuration(bindings.registry(self.session), runtime);
        config.data_dir = Some(client_directory);
        config.assets = Some(bindings.assets());
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
    let viewport = Size::new(Points(960.0), Points(640.0));
    let mut capabilities = nsh_core::access::Capabilities::default();
    capabilities.clipboard = true;
    let app = Bundle::source(APPLICATION.into())
        .with_app_id(Some("org.apache.maka.terminal"))
        .with_policy(Policy {
            declared: Some(capabilities),
            ..Default::default()
        });
    EngineConfig {
        viewport,
        environment: Environment {
            renderer: Renderer::Terminal,
            idiom: Idiom::Desktop,
            window: viewport,
            ..Default::default()
        },
        header_heights: nsh_core::NO_NATIVE_HEADER,
        // APPLICATION is compiled into this trusted executable; only the
        // interpreter stack is bounded, as for other standalone NSH apps.
        limits: Limits::standalone(),
        ui_sets: vec![nsh_base::registry(), registry, crate::markdown::registry()],
        runtime: Some(runtime),
        ..EngineConfig::new(app)
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Command {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn optional_root_selects_the_default_host_and_preserves_explicit_paths() {
        const CHILD: &str = "MAKA_TERMINAL_DEFAULT_ROOT_TEST";
        if std::env::var_os(CHILD).is_some() {
            let select = |args: &[&str]| {
                Command::try_parse_from(args)
                    .unwrap()
                    .args
                    .root_path()
                    .unwrap()
            };
            assert_eq!(
                select(&["maka-terminal-shell"]),
                PathBuf::from("/isolated-maka-home/.maka/application/host")
            );
            assert_eq!(
                select(&["maka-terminal-shell", "--root", "custom-host"]),
                std::env::current_dir().unwrap().join("custom-host")
            );
            assert_eq!(
                select(&["maka-terminal-shell", "--root", "/explicit-host"]),
                PathBuf::from("/explicit-host")
            );
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "entry::tests::optional_root_selects_the_default_host_and_preserves_explicit_paths",
            ])
            .env(CHILD, "1")
            .env("HOME", "/isolated-maka-home")
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
