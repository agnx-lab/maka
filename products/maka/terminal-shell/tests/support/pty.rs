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

use maka_agent_facts::terminal::TerminalSize;
use maka_process::terminal::Screen;
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};

/// The production binary owns this actual terminal; Screen only interprets its ANSI.
pub struct Pty {
    child: Child,
    master: File,
    slave: File,
    original: libc::termios,
    screen: Screen,
    pending: Vec<u8>,
    output: Vec<u8>,
}

impl Pty {
    pub fn spawn(root: &Path, session: Option<&str>) -> Self {
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 40,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &raw mut size,
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) },
                -1
            );
        }
        let mut original = unsafe { std::mem::zeroed() };
        assert_eq!(
            unsafe { libc::tcgetattr(slave.as_raw_fd(), &mut original) },
            0
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_maka-terminal-shell"));
        command.args(["--root", root.to_str().unwrap()]);
        if let Some(session) = session {
            command.args(["--session", session]);
        }
        command
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env_remove("NO_COLOR")
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave.try_clone().unwrap());
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().unwrap();
        Self {
            child,
            master,
            slave,
            original,
            screen: Screen::new(TerminalSize::new(120, 40).unwrap()),
            pending: vec![],
            output: vec![],
        }
    }
    pub fn send(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).unwrap();
    }
    pub fn paste(&mut self, text: &str) {
        self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
    }
    pub fn click_text(&mut self, label: &str) {
        let screen = self.snapshot();
        let (row, line) = screen
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(label))
            .unwrap_or_else(|| panic!("no visible PTY button {label:?}:\n{screen}"));
        let offset = line.find(label).unwrap();
        let column = ratatui::text::Line::from(&line[..offset]).width() + 1;
        let row = row + 1;
        // Both physical events arrive in one write, as a real terminal's input
        // buffer can deliver a quick click. Production admission must keep it.
        self.send(format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m").as_bytes());
    }
    pub fn snapshot(&self) -> String {
        self.screen.text_preview(128 * 1024).screen
    }
    pub fn read(&mut self) {
        let mut poll = libc::pollfd {
            fd: self.master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let available = unsafe { libc::poll(&mut poll, 1, 10) };
        if available < 0
            && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
        {
            return;
        }
        assert!(available >= 0, "PTY read poll failed");
        if available == 0 {
            return;
        }
        let mut bytes = [0; 8192];
        let length = self.master.read(&mut bytes).unwrap();
        self.output.extend_from_slice(&bytes[..length]);
        self.pending.extend_from_slice(&bytes[..length]);
        let end = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(error) => panic!("invalid terminal UTF-8: {error}"),
        };
        let reply = self
            .screen
            .write(std::str::from_utf8(&self.pending[..end]).unwrap())
            .unwrap();
        self.pending.drain(..end);
        if !reply.is_empty() {
            self.send(reply.as_bytes());
        }
    }
    pub async fn wait_for(&mut self, expected: &str) -> String {
        let started = Instant::now();
        loop {
            self.read();
            let screen = self.screen.text_preview(128 * 1024).screen;
            if screen.contains(expected) {
                return screen;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "production terminal exited:\n{}",
                String::from_utf8_lossy(&self.output)
            );
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "production terminal never showed {expected:?}:\n{screen}"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
    pub async fn finish(&mut self) {
        let started = Instant::now();
        loop {
            self.read();
            if let Some(status) = self.child.try_wait().unwrap() {
                self.read();
                assert!(
                    status.success(),
                    "production terminal exited {status}:\n{}",
                    String::from_utf8_lossy(&self.output)
                );
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "production terminal did not quit"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(
            !self.screen.display().metadata.alternate_screen,
            "production binary left the alternate screen active"
        );
        let mut current = unsafe { std::mem::zeroed::<libc::termios>() };
        assert_eq!(
            unsafe { libc::tcgetattr(self.slave.as_raw_fd(), &mut current) },
            0
        );
        assert_eq!(
            (
                current.c_iflag,
                current.c_oflag,
                current.c_cflag,
                current.c_lflag
            ),
            (
                self.original.c_iflag,
                self.original.c_oflag,
                self.original.c_cflag,
                self.original.c_lflag
            ),
            "production binary did not restore terminal flags"
        );
        assert_eq!(
            current.c_cc, self.original.c_cc,
            "production binary did not restore terminal control characters"
        );
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
