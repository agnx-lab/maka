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

use maka_host_runtime::HostError;
use std::{ffi::OsString, process::ExitCode};

#[derive(clap::Args)]
#[command(disable_help_flag = true, disable_version_flag = true)]
pub(super) struct Args {
    /// Options passed to maka-terminal-shell, including --root, --session and --help.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<OsString>,
}

impl Args {
    pub(super) fn run(self) -> Result<ExitCode, HostError> {
        let executable = std::env::current_exe()?.with_file_name(if cfg!(windows) {
            "maka-terminal-shell.exe"
        } else {
            "maka-terminal-shell"
        });
        let mut command = std::process::Command::new(&executable);
        command.args(self.args);
        // The shell owns Host discovery, signals and terminal restoration.
        // Keep its original working directory, environment and standard I/O.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            Err(format!("{}: {}", executable.display(), command.exec()).into())
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Console::{
                CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler,
            };

            unsafe extern "system" fn console_interrupt(event: u32) -> i32 {
                i32::from(matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT))
            }

            // Both processes receive console interrupts. Keep this waiter alive
            // until the shell has restored the terminal and closed its client.
            // A NULL handler would instead make the child inherit ignored Ctrl+C.
            if unsafe { SetConsoleCtrlHandler(Some(console_interrupt), 1) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let status = command
                .status()
                .map_err(|error| format!("{}: {error}", executable.display()))?;
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}
