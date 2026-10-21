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

use std::process::Command;

#[test]
fn terminal_shell_help_runs_from_the_cli_without_a_sibling_shell() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory
        .path()
        .join(if cfg!(windows) { "maka.exe" } else { "maka" });
    std::fs::copy(env!("CARGO_BIN_EXE_maka"), &executable).unwrap();
    let output = Command::new(&executable)
        .args(["terminal-shell", "--help"])
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--root"), "{help}");
    assert!(help.contains("--session"), "{help}");
    assert!(!help.contains("<ARGS>"), "{help}");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
