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

#![cfg(unix)]

mod application;
mod bundled_product;
mod frames;

use maka_agent_facts::terminal::TerminalSize;
use maka_process::terminal::Screen;
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Child, Command},
    time::{Duration, Instant},
};

struct Pty {
    child: Child,
    master: Option<File>,
    // Keep one slave descriptor to read the final termios and avoid EIO while draining.
    slave: Option<File>,
    screen: Screen,
    pending: Vec<u8>,
    output: Vec<u8>,
    frames: frames::Frames,
    read_size: usize,
    enhanced_keyboard: bool,
    keyboard_queries: usize,
}

// A cloned descriptor controls only the PTY created by this fixture. Failure
// unwinding resumes the same output before Pty tears down its child.
struct PausedOutput(File);
impl PausedOutput {
    fn wait_until_blocked(&self) -> usize {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let mut queued: libc::c_int = 0;
            assert_eq!(
                unsafe { libc::ioctl(self.0.as_raw_fd(), libc::TIOCOUTQ, &mut queued) },
                0
            );
            let mut poll = libc::pollfd {
                fd: self.0.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            assert!(unsafe { libc::poll(&mut poll, 1, 0) } >= 0);
            if queued > 0 && poll.revents & libc::POLLOUT == 0 {
                return queued as usize;
            }
            assert!(
                Instant::now() < until,
                "stdout did not reach PTY backpressure; queued={queued}"
            );
            std::thread::park_timeout(Duration::from_millis(2));
        }
    }
}
impl Drop for PausedOutput {
    fn drop(&mut self) {
        // Also runs on a failed assertion or a model-request timeout.
        let restored = unsafe { libc::tcflow(self.0.as_raw_fd(), libc::TCOON) };
        if !std::thread::panicking() {
            assert_eq!(restored, 0);
        }
    }
}

impl Pty {
    fn spawn(args: &[&str]) -> Self {
        Self::spawn_with_keyboard(args, false)
    }
    fn spawn_with_keyboard(args: &[&str], enhanced_keyboard: bool) -> Self {
        Self::spawn_with(args, enhanced_keyboard, |_| {})
    }
    fn spawn_with(
        args: &[&str],
        enhanced_keyboard: bool,
        configure: impl FnOnce(&mut Command),
    ) -> Self {
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
        let mut command = Command::new(env!("CARGO_BIN_EXE_maka"));
        command
            .args(args)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env_remove("NO_COLOR")
            .env("LC_ALL", "en_US.UTF-8")
            .stdin(slave.try_clone().unwrap())
            .stdout(slave.try_clone().unwrap())
            .stderr(slave.try_clone().unwrap());
        configure(&mut command);
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
            master: Some(master),
            slave: Some(slave),
            screen: Screen::new(TerminalSize::new(120, 40).unwrap()),
            pending: Vec::new(),
            output: Vec::new(),
            frames: frames::Frames::default(),
            read_size: 8192,
            enhanced_keyboard,
            keyboard_queries: 0,
        }
    }
    fn pause_output(&self) -> PausedOutput {
        let slave = self.slave.as_ref().unwrap().try_clone().unwrap();
        assert_eq!(unsafe { libc::tcflow(slave.as_raw_fd(), libc::TCOOFF) }, 0);
        PausedOutput(slave)
    }
    fn resize(&mut self, cols: u16, rows: u16) {
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe {
                libc::ioctl(
                    self.master.as_ref().unwrap().as_raw_fd(),
                    libc::TIOCSWINSZ,
                    &size,
                )
            },
            0
        );
        self.screen
            .resize(TerminalSize::new(cols, rows).unwrap())
            .unwrap();
        self.frames.resize();
    }
    fn send(&mut self, bytes: &[u8]) {
        self.master.as_mut().unwrap().write_all(bytes).unwrap();
    }
    fn read(&mut self) {
        let mut poll = libc::pollfd {
            fd: self.master.as_ref().unwrap().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let available = unsafe { libc::poll(&mut poll, 1, 50) };
        if available < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                return;
            }
            panic!("PTY poll failed: {error}");
        }
        if available == 0 {
            return;
        }
        let mut buffer = [0; 8192];
        let length = self
            .master
            .as_mut()
            .unwrap()
            .read(&mut buffer[..self.read_size])
            .unwrap();
        self.frames.feed(&buffer[..length]);
        self.output.extend_from_slice(&buffer[..length]);
        if self.enhanced_keyboard {
            // The Host screen intentionally does not advertise CSI-u input.
            // This outer terminal fixture also exercises the enhanced protocol.
            let queries = self
                .output
                .windows(4)
                .filter(|bytes| *bytes == b"\x1b[?u")
                .count();
            for _ in self.keyboard_queries..queries {
                self.send(b"\x1b[?3u");
            }
            self.keyboard_queries = queries;
        }
        if self.output.len() > 1024 * 1024 {
            self.output.drain(..self.output.len() - 1024 * 1024);
        }
        self.pending.extend_from_slice(&buffer[..length]);
        let end = match std::str::from_utf8(&self.pending) {
            Ok(_) => self.pending.len(),
            Err(error) if error.error_len().is_none() => error.valid_up_to(),
            Err(error) => panic!("Invalid terminal UTF-8: {error}"),
        };
        let text = std::str::from_utf8(&self.pending[..end]).unwrap();
        let reply = self.screen.write(text).unwrap();
        self.pending.drain(..end);
        if !reply.is_empty() {
            self.send(reply.as_bytes());
        }
    }
    #[track_caller]
    fn wait_for(&mut self, text: &str) {
        self.wait_until(|screen| screen.contains(text));
    }
    #[track_caller]
    fn wait_for_output(&mut self, text: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            self.read();
            if String::from_utf8_lossy(&self.output).contains(text) {
                return;
            }
            assert!(Instant::now() < deadline, "startup output missing: {text}");
            assert!(self.child.try_wait().unwrap().is_none(), "startup exited");
        }
    }
    #[track_caller]
    fn wait_until(&mut self, predicate: impl FnMut(&str) -> bool) {
        self.wait_until_with_timeout(predicate, Duration::from_secs(10));
    }
    #[track_caller]
    fn wait_until_with_timeout(
        &mut self,
        mut predicate: impl FnMut(&str) -> bool,
        timeout: Duration,
    ) {
        let deadline = Instant::now() + timeout;
        loop {
            self.read();
            let snapshot = self.screen.snapshot().unwrap();
            if self.frames.ready() && predicate(&snapshot.screen) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "screen condition timed out (pending tty bytes: {}):\n{}\n{}",
                {
                    let mut pending = 0;
                    assert_eq!(
                        unsafe {
                            libc::ioctl(
                                self.slave.as_ref().unwrap().as_raw_fd(),
                                libc::FIONREAD,
                                &mut pending,
                            )
                        },
                        0
                    );
                    pending
                },
                snapshot.screen,
                String::from_utf8_lossy(&self.output)
                    .lines()
                    .find(|line| line.contains("invalid UI activation or batch:"))
                    .unwrap_or("")
            );
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "TUI exited:\n{}",
                snapshot.screen
            );
        }
    }
    fn finish(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            self.read();
            if let Some(status) = self.child.try_wait().unwrap() {
                self.read();
                assert!(
                    status.success(),
                    "TUI exited {status}:\n{}",
                    String::from_utf8_lossy(&self.output[self.output.len().saturating_sub(8192)..])
                );
                return;
            }
            assert!(Instant::now() < deadline, "TUI did not exit");
        }
    }
    fn terminate(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.kill()?;
        // Session teardown may need the last parent-held PTY handles released.
        self.master.take();
        self.slave.take();
        self.child.wait()
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        if std::thread::panicking()
            && let Ok(snapshot) = self.screen.snapshot()
        {
            eprintln!("Final terminal frame:\n{}", snapshot.screen);
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.terminate();
        }
    }
}
