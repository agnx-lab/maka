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

#![cfg_attr(windows, windows_subsystem = "windows")]

//! The Maka desktop: runs the workbench (`maka-workbench`) in a window of
//! its own through NSH's GPUI backend. `maka desktop` starts it for the Host
//! it admitted, named by `MAKA_HOST_ROOT`, `MAKA_HOST_ENDPOINT` and
//! `MAKA_HOST_EPOCH`. Closing the window detaches this client; the Host keeps
//! the work it accepted.

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
mod window;

use std::{path::PathBuf, process::ExitCode};

const ROOT: &str = "MAKA_HOST_ROOT";
const ENDPOINT: &str = "MAKA_HOST_ENDPOINT";
const EPOCH: &str = "MAKA_HOST_EPOCH";

/// The Host `maka desktop` admitted: its Root, its endpoint and the epoch the
/// connection must find there.
#[derive(Debug, PartialEq)]
struct Target {
    root: PathBuf,
    endpoint: PathBuf,
    epoch: uuid::Uuid,
}

impl Target {
    fn from(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Result<Self, String> {
        let read = |name: &str| {
            var(name)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| format!("{name} is not set; start the desktop with `maka desktop`"))
        };
        let root = PathBuf::from(read(ROOT)?);
        if !root.is_absolute() {
            return Err(format!("{ROOT} is not an absolute path"));
        }
        let endpoint = PathBuf::from(read(ENDPOINT)?);
        let epoch = read(EPOCH)?;
        let epoch = epoch
            .to_str()
            .and_then(|epoch| epoch.parse().ok())
            .ok_or_else(|| format!("{EPOCH} is not a UUID: {}", epoch.to_string_lossy()))?;
        Ok(Self {
            root,
            endpoint,
            epoch,
        })
    }
}

/// What `--help` or `--version` prints, answered before anything can fail;
/// `None` starts the desktop.
fn answer(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Option<String> {
    let version = format!("maka-desktop-shell {}", env!("CARGO_PKG_VERSION"));
    let mut answer = None;
    for argument in arguments {
        if argument == "--help" || argument == "-h" {
            return Some(format!(
                "{version}\n\nStarted by `maka desktop --root PATH`, which names the Host in \
                 {ROOT}, {ENDPOINT} and {EPOCH}."
            ));
        }
        if argument == "--version" || argument == "-V" {
            answer = Some(version.clone());
        }
    }
    answer
}

fn main() -> ExitCode {
    if let Some(text) = answer(std::env::args_os().skip(1)) {
        println!("{text}");
        return ExitCode::SUCCESS;
    }
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("maka-desktop-shell: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
fn run() -> Result<(), String> {
    use maka_host_runtime::application::host::Client as HostClient;
    use maka_workbench::bindings::Bindings;

    let target = Target::from(|name| std::env::var_os(name))?;
    let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
    let (client, directory) = runtime.block_on(async {
        let host = HostClient::connect(&target.endpoint, Some(target.epoch)).await?;
        maka_workbench::host::admit(host, &target.root, "desktop").await
    })?;
    let bindings = Bindings::new(client, runtime.handle().clone());
    let mut config =
        maka_workbench::configuration(bindings.registry(None), runtime.handle().clone());
    config.data_dir = Some(directory);
    config.assets = Some(bindings.assets());
    window::run(config, bindings, runtime)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn run() -> Result<(), String> {
    Err("the desktop runs on macOS, Windows and Linux".into())
}

// Unix paths: on Windows an absolute Root also names a drive.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{collections::HashMap, ffi::OsString};

    #[test]
    fn help_and_version_answer_without_a_host() {
        let answer = |arguments: &[&str]| answer(arguments.iter().map(OsString::from));
        let version = format!("maka-desktop-shell {}", env!("CARGO_PKG_VERSION"));
        assert_eq!(answer(&["--version"]), Some(version.clone()));
        assert_eq!(answer(&["-V"]), Some(version.clone()));
        let help = answer(&["--version", "-h"]).unwrap();
        assert!(help.starts_with(&format!("{version}\n\n")), "{help}");
        assert!(help.contains("maka desktop --root PATH"), "{help}");
        assert_eq!(answer(&[]), None);
    }

    fn target(vars: &[(&str, &str)]) -> Result<Target, String> {
        let vars: HashMap<_, _> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), OsString::from(value)))
            .collect();
        Target::from(|name| vars.get(name).cloned())
    }

    #[test]
    fn the_target_is_the_root_endpoint_and_epoch_the_launcher_names() {
        let epoch = "8f1d3c0e-2a4b-4c6d-8e0f-1a2b3c4d5e6f";
        let complete = [
            (ROOT, "/hosts/main"),
            (ENDPOINT, "/run/maka/host.sock"),
            (EPOCH, epoch),
        ];
        assert_eq!(
            target(&complete),
            Ok(Target {
                root: "/hosts/main".into(),
                endpoint: "/run/maka/host.sock".into(),
                epoch: epoch.parse().unwrap(),
            })
        );
        for missing in [ROOT, ENDPOINT, EPOCH] {
            let vars: Vec<_> = complete
                .iter()
                .copied()
                .filter(|(name, _)| *name != missing)
                .collect();
            let error = target(&vars).unwrap_err();
            assert!(
                error.starts_with(&format!("{missing} is not set")),
                "{error}"
            );
        }
        let relative = [(ROOT, "hosts/main"), (ENDPOINT, "/sock"), (EPOCH, epoch)];
        assert_eq!(
            target(&relative),
            Err(format!("{ROOT} is not an absolute path"))
        );
        let latest = [
            (ROOT, "/hosts/main"),
            (ENDPOINT, "/sock"),
            (EPOCH, "latest"),
        ];
        assert_eq!(
            target(&latest),
            Err(format!("{EPOCH} is not a UUID: latest"))
        );
    }
}
