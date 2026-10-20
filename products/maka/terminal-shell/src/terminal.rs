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

//! One NSH UI turn per completed terminal presentation. Host ownership and
//! recovery stay outside this shell; neither blocked stdout nor JS prevents
//! the input owner from cancelling and returning to that Host scope.

mod input;
mod output;
mod ui;

use crossterm::{event, terminal};
use input::Inbox;
use nsh_engine::EngineConfig;
use nsh_platform_tui::CellMetrics;
use output::Output;
use std::{
    io::{self, IsTerminal},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const SETUP: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?2004h";
const CLEANUP: &[u8] =
    b"\x1b[?2026l\x1b[?2004l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?25h\x1b[?1049l";
const TICK: Duration = Duration::from_millis(16);

#[derive(Clone, Default)]
struct Presentation {
    state: Arc<Mutex<Inbox>>,
    presented: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Presentation {
    fn publish(&self, epoch: u64) {
        let inbox = self.state.lock().unwrap();
        if !self.stop.load(Ordering::Acquire)
            && !inbox.quit
            && inbox.error.is_none()
            && inbox.resize.is_none()
        {
            self.presented.store(epoch, Ordering::Release);
        }
    }

    fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        let _inbox = self.state.lock().unwrap();
        self.presented.store(0, Ordering::Release);
    }
}

/// Runs the first-party terminal UI. Ctrl+Q cancels independently of the
/// UI and writer; Ctrl+C retains the surface's copy/exit behavior.
/// An error reports incomplete output restoration;
/// success means the cleanup bytes were flushed, rather than merely queued.
/// The embedding process may request graceful cancellation without acquiring
/// any UI or IO lock by setting `stop`.
pub fn run(mut config: EngineConfig, stop: Arc<AtomicBool>) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("the terminal shell requires a terminal on stdin and stdout".into());
    }
    let (columns, rows) = terminal::size().map_err(|error| error.to_string())?;
    config.viewport = CellMetrics::default().viewport(columns.max(1), rows.max(1));
    config.environment.window = (config.viewport.width, config.viewport.height);
    terminal::enable_raw_mode().map_err(|error| error.to_string())?;
    let _raw = RawMode;
    let mut output = Output::new(io::stdout()).map_err(|error| error.to_string())?;
    output.submit(SETUP.to_vec(), None)?;
    let presentation = Presentation {
        stop: stop.clone(),
        ..Default::default()
    };
    let _reader = StopReader(stop.clone());
    let result = (|| {
        start_reader(presentation.clone())?;
        let (commands, frames) =
            ui::spawn(config, presentation.clone()).map_err(|error| error.to_string())?;
        // Returning drops both channel endpoints without joining either
        // blocked owner, including startup errors after terminal mutation.
        drive(&mut output, &presentation, &commands, &frames)
    })();
    presentation.shutdown();
    let cleanup = output.close(CLEANUP.to_vec()).and_then(|receipt| {
        match receipt.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => result.map_err(|error| error.to_string()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Err("terminal output restoration is still pending behind blocked output".into())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("terminal output worker stopped before restoration".into())
            }
        }
    });
    match (result, cleanup) {
        (Ok(()), result) => result,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
    }
}

fn drive(
    output: &mut Output,
    presentation: &Presentation,
    commands: &mpsc::SyncSender<ui::Advance>,
    frames: &mpsc::Receiver<Result<ui::Frame, String>>,
) -> Result<(), String> {
    let Presentation {
        state,
        presented,
        stop,
    } = presentation;
    let mut setup = true;
    let mut busy = true;
    let mut pending = None;
    let mut epoch = 0;
    let mut next_tick = Instant::now();
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        {
            let inbox = state.lock().unwrap();
            if inbox.quit {
                return Ok(());
            }
            if let Some(error) = &inbox.error {
                return Err(error.clone());
            }
        }
        if output.poll()? {
            setup = false;
            if let Some(completed) = pending.take() {
                epoch = completed;
            }
        }
        if !setup && pending.is_none() && busy {
            match frames.try_recv() {
                Ok(frame) => {
                    let frame = frame?;
                    if frame.quit {
                        return Ok(());
                    }
                    busy = false;
                    if let Some(bytes) = frame.bytes {
                        output.submit(bytes, Some((presentation.clone(), frame.epoch)))?;
                        pending = Some(frame.epoch);
                    } else {
                        epoch = frame.epoch;
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => return Err("NSH UI worker stopped".into()),
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !setup && !busy && pending.is_none() {
            let mut inbox = state.lock().unwrap();
            let resize = inbox.resize.take();
            let inputs = inbox.pop_batch(if resize.is_some() { 0 } else { epoch });
            let cancel_pointer = std::mem::take(&mut inbox.cancel_pointer);
            let command = ui::Advance {
                inputs,
                resize,
                cancel_pointer,
            };
            if !command.is_probe() || Instant::now() >= next_tick {
                if !command.is_probe() {
                    // Explicit input revokes before the UI worker can run.
                    // Idle probes first check whether any work is due.
                    presented.store(0, Ordering::Release);
                }
                commands
                    .try_send(command)
                    .map_err(|error| error.to_string())?;
                busy = true;
                next_tick = Instant::now() + TICK;
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn start_reader(presentation: Presentation) -> Result<(), String> {
    thread::Builder::new()
        .name("maka-nsh-input".into())
        .spawn(move || {
            let Presentation {
                state,
                presented,
                stop,
            } = presentation;
            while !stop.load(Ordering::Acquire) {
                {
                    let mut inbox = state.lock().unwrap();
                    // poll itself can consume and cache terminal bytes. Keep
                    // polling, decoding and stamping under the presentation
                    // fence, without waiting for stdin while holding it.
                    let mut read = 0;
                    for _ in 0..input::MAX_EVENTS {
                        let event = event::poll(Duration::ZERO).and_then(|ready| {
                            if ready {
                                event::read().map(Some)
                            } else {
                                Ok(None)
                            }
                        });
                        let event = match event {
                            Ok(Some(event)) => event,
                            Ok(None) => break,
                            Err(error) => {
                                inbox.error = Some(error.to_string());
                                return;
                            }
                        };
                        read += 1;
                        if matches!(event, event::Event::Resize(..)) {
                            presented.store(0, Ordering::Release);
                        }
                        inbox.push(event, presented.load(Ordering::Acquire));
                        if inbox.quit || inbox.error.is_some() || stop.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    if read == input::MAX_EVENTS
                        && !inbox.quit
                        && inbox.error.is_none()
                        && !stop.load(Ordering::Acquire)
                    {
                        match event::poll(Duration::ZERO) {
                            Ok(true) => inbox.error = Some(
                                "terminal input exceeded its pending budget; input was rejected"
                                    .into(),
                            ),
                            Ok(false) => {}
                            Err(error) => inbox.error = Some(error.to_string()),
                        }
                    }
                }
                thread::sleep(Duration::from_millis(2));
            }
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

struct RawMode;
impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}
struct StopReader(Arc<AtomicBool>);
impl Drop for StopReader {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn blocked_stdout_does_not_publish_the_frame_or_prevent_independent_cancel() {
        for external in [false, true] {
            blocked_cancel(external);
        }
    }

    fn blocked_cancel(external: bool) {
        struct Blocked {
            entered: mpsc::SyncSender<()>,
            resume: mpsc::Receiver<()>,
        }
        impl Write for Blocked {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if bytes == b"FRAME" {
                    let _ = self.entered.send(());
                    self.resume
                        .recv()
                        .map_err(|_| io::Error::other("gate closed"))?;
                }
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let state = Arc::new(Mutex::new(Inbox::default()));
        let presented = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (entered, observing) = mpsc::sync_channel(1);
        let (resume, gate) = mpsc::channel();
        let mut output = Output::new(Blocked {
            entered,
            resume: gate,
        })
        .unwrap();
        output.submit(b"SETUP".to_vec(), None).unwrap();
        let (commands, _worker) = mpsc::sync_channel(1);
        let (frames, receive) = mpsc::sync_channel(1);
        frames
            .send(Ok(ui::Frame {
                epoch: 7,
                bytes: Some(b"FRAME".to_vec()),
                quit: false,
            }))
            .unwrap();
        let (finished, result) = mpsc::sync_channel(1);
        let presentation = Presentation {
            state: state.clone(),
            presented: presented.clone(),
            stop: stop.clone(),
        };
        thread::spawn(move || {
            let result = drive(&mut output, &presentation, &commands, &receive);
            let _ = finished.send(result);
        });
        observing.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(presented.load(Ordering::Acquire), 0);
        if external {
            stop.store(true, Ordering::Release);
        } else {
            state.lock().unwrap().quit = true;
        }
        // The caller can now run Host close/recovery while stdout is still
        // held by the test gate. No UI engine or output join is required.
        result
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        resume.send(()).unwrap();
    }
}
