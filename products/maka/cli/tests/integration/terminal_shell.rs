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

use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};

#[test]
fn terminal_shell_preserves_arguments_stdio_cwd_pid_and_exit_status() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("maka");
    std::fs::copy(env!("CARGO_BIN_EXE_maka"), &executable).unwrap();
    let shell = directory.path().join("maka-terminal-shell");
    std::fs::write(&shell, "#!/bin/sh\nprintf '%s\\0' \"$$\" \"$PWD\" \"$@\"\nIFS= read -r input\nprintf '%s\\0' \"$input\"\nexit 73\n").unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
    let cwd = directory.path().join("工作 目录");
    std::fs::create_dir(&cwd).unwrap();
    let cwd = cwd.canonicalize().unwrap();
    for args in [
        vec![],
        vec!["--help"],
        vec!["--root", "相对 Root", "--session", "会话 1"],
    ] {
        let mut child = Command::new(&executable)
            .arg("terminal-shell")
            .args(&args)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id().to_string();
        child
            .stdin
            .take()
            .unwrap()
            .write_all("你好 stdin\n".as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(73),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut expected = vec![pid.as_str(), cwd.to_str().unwrap()];
        expected.extend(args.iter().copied());
        expected.push("你好 stdin");
        assert_eq!(output.stdout, (expected.join("\0") + "\0").as_bytes());
        assert!(!cwd.join("相对 Root").exists());
    }
    let decoy = directory.path().join("path");
    std::fs::create_dir(&decoy).unwrap();
    std::fs::rename(&shell, decoy.join("maka-terminal-shell")).unwrap();
    let missing = Command::new(&executable)
        .arg("terminal-shell")
        .env("PATH", decoy)
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty(), "a PATH decoy must not execute");
    assert!(String::from_utf8_lossy(&missing.stderr).contains("maka-terminal-shell"));
}
