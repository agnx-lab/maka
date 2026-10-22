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

//! Native UI input and terminal output have independent owners. Host ownership
//! and recovery stay outside this shell; blocked stdout never stalls native
//! event dispatch or the input owner’s independent cancellation path.

mod adapter;
#[cfg(target_os = "linux")]
mod frame_trace;
mod graphics;
mod input;
mod output;
#[cfg(target_os = "linux")]
mod pixels;
mod ui;

use input::Inbox;
use nsh_engine::EngineConfig;
use nsh_platform_tui::CellMetrics;
use output::Output;
use std::{
    io::{self, IsTerminal},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use termina::{EventReader, PlatformTerminal, Terminal as _};

const SETUP: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?2004h";
const CLEANUP: &[u8] =
    b"\x1b[?2026l\x1b[?2004l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?25h\x1b[?1049l";
const TICK: Duration = Duration::from_millis(16);

pub(crate) type GpuWorker = thread::JoinHandle<Result<(), String>>;

#[derive(Clone, Default)]
struct Presentation {
    state: Arc<Mutex<Inbox>>,
    stop: Arc<AtomicBool>,
    driver: Option<thread::Thread>,
}

impl Presentation {
    fn wake_driver(&self) {
        if let Some(driver) = &self.driver {
            driver.unpark();
        }
    }

    fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// Runs the first-party terminal UI. Ctrl+Q cancels independently of the
/// UI and writer; Ctrl+C retains the surface's copy/exit behavior.
/// An error reports incomplete output restoration;
/// success means the cleanup bytes were flushed, rather than merely queued.
/// The embedding process may request graceful cancellation without acquiring
/// any UI or IO lock by setting `stop`.
pub(crate) fn run(
    mut config: EngineConfig,
    stop: Arc<AtomicBool>,
    _gpu_worker: &mut Option<GpuWorker>,
) -> Result<(), String> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("the terminal shell requires a terminal on stdin and stdout".into());
    }
    let mut terminal = PlatformTerminal::new().map_err(|error| error.to_string())?;
    #[cfg(windows)]
    terminal.set_discard_input_on_drop(false);
    let size = terminal
        .get_dimensions()
        .map_err(|error| error.to_string())?;
    let (columns, rows) = (size.cols, size.rows);
    config.viewport = CellMetrics::default().viewport(columns.max(1), rows.max(1));
    config.environment.window = config.viewport;
    terminal
        .enter_raw_mode()
        .map_err(|error| error.to_string())?;
    let reader = terminal.event_reader();
    let mut output = Output::new(io::stdout()).map_err(|error| error.to_string())?;
    let presentation = Presentation {
        stop: stop.clone(),
        driver: Some(thread::current()),
        ..Default::default()
    };
    let setup = [SETUP, graphics::QUERY].concat();
    #[cfg(target_os = "linux")]
    let setup = {
        let mut setup = setup;
        let mut state = presentation.state.lock().unwrap();
        state.trace = frame_trace::Trace::from_env();
        let tracing = state.trace.is_some();
        state.pixels = Some(pixels::Inbox::new(
            pixels::Capabilities {
                size,
                mouse: pixels::MouseCoordinates::Cells,
                protocol: None,
                synchronized: false,
                sixel_geometry: Default::default(),
            },
            tracing,
        ));
        setup.extend(state.pixel_probe.query()?);
        setup
    };
    output.submit(setup)?;
    let mut input = None;
    let result = (|| {
        input = Some(start_reader(presentation.clone(), reader)?);
        #[cfg(target_os = "linux")]
        {
            let capabilities = pixels::select(&mut output, &presentation, size)?;
            if presentation.stop.load(Ordering::Acquire) || presentation.state.lock().unwrap().quit
            {
                return Ok(());
            }
            if let Some(capabilities) = capabilities {
                return pixels::run(
                    &mut output,
                    &presentation,
                    config,
                    capabilities,
                    _gpu_worker,
                );
            }
        }
        let (commands, frames) = ui::spawn(config).map_err(|error| error.to_string())?;
        // Returning drops both channel endpoints without joining either
        // blocked owner, including startup errors after terminal mutation.
        drive(&mut output, &presentation, &commands, &frames)
    })();
    presentation.shutdown();
    // The platform guard must outlive the sole reader: restoring console modes
    // or code pages while it can still decode input races shutdown on Windows.
    drop(input);
    let mut cleanup = Vec::new();
    #[cfg(target_os = "linux")]
    cleanup.extend(pixels::cleanup(&presentation, result.is_err()));
    cleanup.extend_from_slice(CLEANUP);
    let cleanup = output.close(cleanup).and_then(|receipt| {
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
    let result = match (result, cleanup) {
        (Ok(()), result) => result,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
    };
    drop(terminal);
    #[cfg(target_os = "linux")]
    {
        let trace = presentation.state.lock().unwrap().trace.take();
        if let Some(trace) = trace {
            let saved = trace.write(&result);
            return match (result, saved) {
                (Ok(()), saved) => saved,
                (Err(error), Ok(())) => Err(error),
                (Err(error), Err(trace)) => Err(format!("{error}; {trace}")),
            };
        }
    }
    result
}

fn drive(
    output: &mut Output,
    presentation: &Presentation,
    commands: &mpsc::SyncSender<ui::Advance>,
    frames: &mpsc::Receiver<Result<ui::Frame, String>>,
) -> Result<(), String> {
    let Presentation { state, stop, .. } = presentation;
    let mut setup = true;
    let mut busy = true;
    // An encoded delta remains immutable until the sole writer accepts it.
    // Later native input can advance without producing another delta.
    let mut ready = None;
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
        let mut writable = output.poll()?;
        if writable && output.take_flushed().is_some() && setup {
            state.lock().unwrap().graphics.flushed();
            setup = false;
        }
        if busy {
            match frames.try_recv() {
                Ok(frame) => {
                    let frame = frame?;
                    if frame.quit {
                        return Ok(());
                    }
                    busy = false;
                    if let Some(bytes) = frame.bytes {
                        debug_assert!(ready.is_none());
                        ready = Some(bytes);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => return Err("NSH UI worker stopped".into()),
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if writable {
            if let Some(bytes) = ready.take() {
                output.submit(bytes)?;
                writable = false;
            } else if let Some(query) = state.lock().unwrap().graphics.query() {
                output.submit(query.to_vec())?;
                setup = true;
                writable = false;
            }
        }
        if !busy {
            let mut inbox = state.lock().unwrap();
            let command = ui::Advance {
                image_protocol: inbox.graphics.take_ready(),
                inputs: inbox.pop_batch(),
                resize: inbox.resize.take(),
                cancel_pointer: std::mem::take(&mut inbox.cancel_pointer),
                draw: writable && !setup && ready.is_none(),
            };
            if !command.is_probe() || Instant::now() >= next_tick {
                commands
                    .try_send(command)
                    .map_err(|error| error.to_string())?;
                busy = true;
                next_tick = Instant::now() + TICK;
            }
        }
        thread::park_timeout(Duration::from_millis(2));
    }
}

fn start_reader(presentation: Presentation, reader: EventReader) -> Result<Reader, String> {
    let stop = presentation.stop.clone();
    let waker = reader.waker();
    let handle = thread::Builder::new()
        .name("maka-nsh-input".into())
        .spawn(move || {
            let Presentation { state, stop, .. } = &presentation;
            while !stop.load(Ordering::Acquire) {
                {
                    let mut inbox = state.lock().unwrap();
                    // Keep polling and decoding in FIFO order, without
                    // waiting for stdin while holding the input lock.
                    for _ in 0..input::READ_BATCH {
                        let event = reader
                            .poll(Some(Duration::ZERO), |_| true)
                            .and_then(|ready| {
                                if ready {
                                    reader.read(|_| true).map(Some)
                                } else {
                                    Ok(None)
                                }
                            });
                        let event = match event {
                            Ok(Some(event)) => event,
                            Ok(None) => break,
                            Err(error) => {
                                inbox.error = Some(error.to_string());
                                presentation.wake_driver();
                                return;
                            }
                        };
                        presentation.wake_driver();
                        inbox.graphics.observe(&event);
                        #[cfg(target_os = "linux")]
                        {
                            let graphics_response =
                                inbox.pixel_probe.check_graphics_response(&event);
                            inbox.pixel_probe.observe(&event, Instant::now());
                            let sixel_geometry = inbox.pixel_probe.sixel_geometry;
                            let tracing = inbox.trace.is_some();
                            if let Some(pixels) = &mut inbox.pixels {
                                let geometry = pixels.geometry;
                                let resizing = matches!(&event, termina::Event::WindowResized(_))
                                    || pixels.capabilities.sixel_geometry != sixel_geometry;
                                let completed = (tracing
                                    && resizing
                                    && pixels.capabilities.protocol
                                        == Some(pixels::Protocol::Sixel))
                                .then(|| pixels.completed_frames().collect::<Vec<_>>());
                                let result = graphics_response
                                    .and_then(|()| pixels.set_sixel_geometry(sixel_geometry))
                                    .and_then(|()| pixels.push(event));
                                if let Some(wake) = &pixels.wake {
                                    wake.unpark();
                                }
                                let resized = pixels.geometry != geometry;
                                if resized
                                    && let (Some(completed), Some(trace)) =
                                        (completed, &mut inbox.trace)
                                {
                                    for offer in completed {
                                        trace.discard(&offer.token, offer.geometry, "resize");
                                    }
                                }
                                match result {
                                    Ok(true) => inbox.quit = true,
                                    Ok(false) => {}
                                    Err(error) => inbox.error = Some(error),
                                }
                                if inbox.quit || inbox.error.is_some() {
                                    break;
                                }
                                continue;
                            }
                        }
                        inbox.push_terminal(event);
                        if inbox.quit || inbox.error.is_some() || stop.load(Ordering::Acquire) {
                            break;
                        }
                    }
                }
                if !stop.load(Ordering::Acquire) {
                    match reader.wait_ready() {
                        Ok(()) => {}
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) => {
                            state.lock().unwrap().error = Some(error.to_string());
                            presentation.wake_driver();
                            return;
                        }
                    }
                }
            }
            presentation.wake_driver();
        })
        .map_err(|error| error.to_string())?;
    Ok(Reader {
        stop,
        waker,
        handle: Some(handle),
    })
}

struct Reader {
    stop: Arc<AtomicBool>,
    waker: termina::PlatformWaker,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for Reader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = self.waker.wake();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn blocked_stdout_does_not_prevent_independent_cancel() {
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
        let stop = Arc::new(AtomicBool::new(false));
        let (entered, observing) = mpsc::sync_channel(1);
        let (resume, gate) = mpsc::channel();
        let mut output = Output::new(Blocked {
            entered,
            resume: gate,
        })
        .unwrap();
        output.submit(b"SETUP".to_vec()).unwrap();
        let (commands, _worker) = mpsc::sync_channel(1);
        let (frames, receive) = mpsc::sync_channel(1);
        frames
            .send(Ok(ui::Frame {
                bytes: Some(b"FRAME".to_vec()),
                quit: false,
            }))
            .unwrap();
        let (finished, result) = mpsc::sync_channel(1);
        let presentation = Presentation {
            state: state.clone(),
            stop: stop.clone(),
            ..Default::default()
        };
        thread::spawn(move || {
            let result = drive(&mut output, &presentation, &commands, &receive);
            let _ = finished.send(result);
        });
        observing.recv_timeout(Duration::from_secs(1)).unwrap();
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
