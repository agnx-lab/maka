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

use super::{
    super::{
        Presentation,
        frame_trace::{InputPumpSample, InputSample},
    },
    mapping::Mapping,
};
use gpui::{
    AppContext, Application, Bounds, PlatformInput, WindowBounds, WindowOptions, point, px, size,
};
use nsh_engine::EngineConfig;
use nsh_gpui_terminal::{FrameToken, TerminalPlatform};
use nsh_platform_gpui::{AppOptions, AppView, InputDisposition, InputSnapshot, InputSource};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{Arc, atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};
use termina::{Event, event::KeyEventKind};

pub(super) fn spawn(
    config: EngineConfig,
    presentation: Presentation,
) -> Result<
    (
        mpsc::Receiver<Result<(), EngineConfig>>,
        super::super::GpuWorker,
    ),
    String,
> {
    let (ready, startup) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
        .name("maka-nsh-gpui".into())
        .spawn(move || {
            let result = run(config, &presentation, ready);
            if let Err(error) = &result {
                presentation.state.lock().unwrap().error = Some(error.clone());
            }
            presentation.wake_driver();
            result
        })
        .map_err(|error| error.to_string())?;
    Ok((startup, worker))
}

fn run(
    mut config: EngineConfig,
    presentation: &Presentation,
    ready: mpsc::SyncSender<Result<(), EngineConfig>>,
) -> Result<(), String> {
    if presentation.stop.load(Ordering::Acquire) || presentation.state.lock().unwrap().quit {
        return Ok(());
    }
    let renderer = match gpui_wgpu::WgpuHeadlessRenderer::new() {
        Ok(renderer) => renderer,
        Err(_) => {
            let _ = ready.send(Err(config));
            return Ok(());
        }
    };
    let tracing = {
        let mut state = presentation.state.lock().unwrap();
        if presentation.stop.load(Ordering::Acquire) || state.quit {
            return Ok(());
        }
        if let Some(trace) = &mut state.trace {
            trace.gpu = renderer.gpu_specs().map(|gpu| {
                serde_json::json!({
                    "is_software_emulated": gpu.is_software_emulated,
                    "device_name": gpu.device_name,
                    "driver_name": gpu.driver_name,
                    "driver_info": gpu.driver_info,
                })
            });
        }
        state.trace.is_some()
    };
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }
    presentation.wake_driver();
    let snapshots = Rc::new(RefCell::new(HashMap::<FrameToken, InputSnapshot>::new()));
    let source = Rc::new(RefCell::new(InputSource::default()));
    let sampled = source.clone();
    let retained = snapshots.clone();
    let delivered = presentation.clone();
    let geometry = Rc::new(Cell::new(0));
    let drawn_geometry = geometry.clone();
    let owner = thread::current();
    let waker = owner.clone();
    let pump_started = tracing.then(|| Rc::new(Cell::new(None::<Instant>)));
    let drawing_started = pump_started.clone();
    let platform = TerminalPlatform::new(
        Box::new(renderer),
        Box::new(move |frame| {
            let timing = drawing_started
                .as_ref()
                .and_then(|started| started.get())
                .map(|start| {
                    (
                        start,
                        Instant::now(),
                        frame.token(),
                        frame.pixels().len(),
                        frame.size(),
                    )
                });
            let snapshot = sampled.borrow().sample();
            let has_snapshot = snapshot.is_some();
            if let Some(snapshot) = snapshot {
                retained.borrow_mut().insert(frame.token(), snapshot);
            }
            if has_snapshot || timing.is_some() {
                let mut state = delivered.state.lock().unwrap();
                let pending_frame = timing.as_ref().and_then(|_| {
                    state
                        .pixels
                        .as_ref()?
                        .pending
                        .as_ref()
                        .map(|candidate| candidate.frame.token().sequence())
                });
                let mut admitted = false;
                if has_snapshot
                    && let Some(inbox) = &mut state.pixels
                    && drawn_geometry.get() == inbox.geometry
                    && inbox.resize.is_none()
                    && inbox.pending.is_none()
                {
                    inbox.pending = Some(super::input::Candidate {
                        frame,
                        geometry: drawn_geometry.get(),
                    });
                    admitted = true;
                }
                if let Some((start, end, token, bytes, size)) = timing
                    && let Some(trace) = &mut state.trace
                {
                    let sample = trace.record(
                        &token,
                        drawn_geometry.get(),
                        "pump_until_candidate",
                        start,
                        end,
                    );
                    sample.bytes = Some(bytes);
                    sample.pixel_size = Some((size.width.0, size.height.0));
                    sample.outcome = if admitted { "admitted" } else { "discarded" };
                    sample.pending_frame = pending_frame;
                }
            }
        }),
        Arc::new(move || waker.unpark()),
    )
    .map_err(|error| error.to_string())?;
    // Renderer initialization can overlap a terminal resize. Wait for the
    // same reader/writer measurement to complete, then keep its size and
    // geometry together; never pair startup dimensions with a newer ticket.
    let viewport = loop {
        let measured = {
            let mut state = presentation.state.lock().unwrap();
            if presentation.stop.load(Ordering::Acquire) || state.quit {
                return Ok(());
            }
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
            let inbox = state.pixels.as_mut().expect("selected pixel presentation");
            inbox.wake = Some(owner.clone());
            inbox
                .capabilities
                .viewport()
                .map(|viewport| (viewport, inbox.geometry))
        };
        if let Some((viewport, generation)) = measured {
            geometry.set(generation);
            break viewport;
        }
        thread::park_timeout(Duration::from_millis(10));
    };
    config
        .assets
        .get_or_insert_with(|| Arc::new(crate::ApplicationAssets));
    let surface = source.clone();
    let error = presentation.clone();
    let window = Rc::new(RefCell::new(None));
    let created = window.clone();
    let application = Application::with_platform(platform.clone()).run_embedded(move |cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(viewport.width.0 as f32), px(viewport.height.0 as f32)),
            ))),
            inactive_frame_interval: None,
            ..Default::default()
        };
        match cx.open_window(options, move |window, cx| {
            cx.new(|cx| {
                let view = AppView::new(
                    AppOptions {
                        title: "Maka".into(),
                        app_id: "org.apache.maka.terminal".into(),
                        ..Default::default()
                    },
                    window,
                    cx,
                );
                *surface.borrow_mut() = view.input_source();
                let cancelled =
                    error.stop.load(Ordering::Acquire) || error.state.lock().unwrap().quit;
                view.on_error(move |message| {
                    error.state.lock().unwrap().error = Some(message.into());
                });
                if !cancelled {
                    view.load_with_config(config);
                }
                view
            })
        }) {
            Ok(handle) => *created.borrow_mut() = Some(handle),
            Err(_) => cx.quit(),
        }
    });
    if window.borrow().is_none() {
        return Err("cannot open the terminal GPUI surface".into());
    }
    let background = application.background_executor();
    let mut mapping = Mapping::default();
    let mut inputs_before_draw = 0;
    let result = (|| {
        while !presentation.stop.load(Ordering::Acquire) {
            let (resize, cancel, ready, caps, generation, quit, error) = {
                let mut state = presentation.state.lock().unwrap();
                let quit = state.quit;
                let error = state.error.clone();
                let inbox = state.pixels.as_mut().expect("selected pixel presentation");
                (
                    if inbox.capabilities.viewport().is_some() {
                        inbox.resize.take()
                    } else {
                        None
                    },
                    std::mem::take(&mut inbox.cancel_pointer),
                    inbox.ready.take(),
                    inbox.capabilities,
                    inbox.geometry,
                    quit,
                    error,
                )
            };
            if quit {
                return Ok(());
            }
            if let Some(error) = error {
                return Err(error);
            }
            if cancel {
                source.borrow().cancel_pointer();
            }
            if resize.is_some() {
                geometry.set(generation);
                let viewport = caps
                    .viewport()
                    .ok_or("terminal resize did not report its real pixel dimensions")?;
                platform
                    .resize(viewport)
                    .map_err(|error| error.to_string())?;
                presentation
                    .state
                    .lock()
                    .unwrap()
                    .pixels
                    .as_mut()
                    .unwrap()
                    .pending = None;
            }
            if cancel || resize.is_some() {
                application.update(|cx| {
                    if let Some(window) = window.borrow().as_ref() {
                        let _ = window.update(cx, |_, window, cx| {
                            window.refresh();
                            cx.notify();
                        });
                    }
                });
            }
            if let Some(offer) = ready {
                // Resize invalidates the receipt immediately, even while GPU
                // readback or an old output transaction is still finishing.
                if resize.is_none() && offer.geometry == generation {
                    platform
                        .mark_presented(offer.token.clone())
                        .map_err(|error| error.to_string())?;
                    let mut state = presentation.state.lock().unwrap();
                    let inbox = state.pixels.as_mut().unwrap();
                    if inbox.resize.is_none() && inbox.geometry == offer.geometry {
                        inbox.keyboard = Some(offer.token.clone());
                        inbox.published = Some(offer.token.clone());
                        if let Some(trace) = &mut state.trace {
                            let at = Instant::now();
                            trace.record(&offer.token, offer.geometry, "published", at, at);
                        }
                    }
                }
                let mut state = presentation.state.lock().unwrap();
                let inbox = state.pixels.as_mut().unwrap();
                if inbox.in_flight.as_ref() == Some(&offer.token) {
                    inbox.in_flight = None;
                }
            }
            // Apply mount/focus work before admitting input. Deferred drawing
            // stays visible to has_pending(), so an opaque callback cannot
            // pass the exact-presentation barrier ahead of that work.
            let mut input_pump = tracing.then(|| {
                let state = presentation.state.lock().unwrap();
                let inbox = state.pixels.as_ref().unwrap();
                let start = Instant::now();
                InputPumpSample {
                    frame: inbox.published.as_ref().map(FrameToken::sequence),
                    geometry: inbox.geometry,
                    start,
                    end: start,
                    queue_before: inbox.queue_len(),
                    queue_after: 0,
                }
            });
            let mut worked = platform.pump_with_draw(false);
            if let Some(sample) = &mut input_pump {
                sample.end = Instant::now();
            }
            let (input, late_cancel, queue_len) = {
                let mut state = presentation.state.lock().unwrap();
                if presentation.stop.load(Ordering::Acquire) || state.quit {
                    return Ok(());
                }
                if let Some(error) = &state.error {
                    return Err(error.clone());
                }
                let inbox = state.pixels.as_mut().unwrap();
                let queue_len = inbox.queue_len();
                let input = inbox.pop();
                if input.is_some() {
                    if inputs_before_draw == 0 {
                        // Yield drawing for the current queue length only;
                        // arrivals during this turn cannot extend it.
                        inputs_before_draw = queue_len;
                    }
                    inputs_before_draw -= 1;
                } else {
                    inputs_before_draw = 0;
                }
                let late_cancel = std::mem::take(&mut inbox.cancel_pointer);
                if let Some(mut sample) = input_pump
                    && let Some(trace) = &mut state.trace
                {
                    sample.queue_after = queue_len;
                    trace.record_input_pump(sample);
                }
                (input, late_cancel, queue_len)
            };
            if late_cancel {
                source.borrow().cancel_pointer();
                application.update(|cx| {
                    if let Some(window) = window.borrow().as_ref() {
                        let _ = window.update(cx, |_, window, cx| {
                            window.refresh();
                            cx.notify();
                        });
                    }
                });
            }
            let exact_ready = !late_cancel
                && !platform.has_pending()
                && presentation
                    .state
                    .lock()
                    .unwrap()
                    .pixels
                    .as_ref()
                    .unwrap()
                    .resize
                    .is_none();
            if let Some(input) = input {
                let dispatch_start = input.queued_at.map(|_| Instant::now());
                let mut refresh = false;
                let mut release_pending = super::input::released(&input.event);
                let token = input
                    .token
                    .or_else(|| input.startup.then(|| platform.presented()).flatten());
                let measurement =
                    if let (Some(queued), Some(start)) = (input.queued_at, dispatch_start) {
                        let mut state = presentation.state.lock().unwrap();
                        state.trace.as_mut().map(|trace| {
                            trace.record_input(InputSample {
                                frame: token.as_ref().map(FrameToken::sequence),
                                viewport_generation: token
                                    .as_ref()
                                    .map(FrameToken::viewport_generation),
                                geometry: input.geometry,
                                kind: super::input::trace_kind(&input.event),
                                queued,
                                dispatch_start: start,
                                dispatch_end: None,
                                queue_len,
                                outcome: "incomplete",
                            })
                        })
                    } else {
                        None
                    };
                let mut outcome = if token.is_some() {
                    "missing_snapshot"
                } else {
                    "missing_token"
                };
                match &input.event {
                    Event::FocusIn => {
                        refresh = true;
                        platform
                            .set_active(true)
                            .map_err(|error| error.to_string())?;
                        outcome = "focus_in";
                    }
                    Event::FocusOut => {
                        refresh = true;
                        source.borrow().cancel_pointer();
                        platform
                            .set_active(false)
                            .map_err(|error| error.to_string())?;
                        outcome = "focus_out";
                    }
                    event => {
                        if let Some(token) = token
                            && let Some(snapshot) = snapshots.borrow().get(&token).cloned()
                        {
                            if let Event::Paste(text) = event {
                                outcome = "paste_rejected";
                                if source.borrow().editor_matches(&snapshot) {
                                    platform.paste(text).map_err(|error| error.to_string())?;
                                    outcome = "paste_handled";
                                    let mut state = presentation.state.lock().unwrap();
                                    let inbox = state.pixels.as_mut().unwrap();
                                    inbox.exact_reject_through =
                                        inbox.exact_reject_through.max(token.sequence());
                                }
                            } else if let Some(event) = mapping.input(event, caps) {
                                let (exact_floor, current_geometry) = {
                                    let state = presentation.state.lock().unwrap();
                                    let inbox = state.pixels.as_ref().unwrap();
                                    (inbox.exact_reject_through, inbox.geometry)
                                };
                                let is_pointer = super::input::pointer(&input.event);
                                let stale = is_pointer && input.geometry != current_geometry;
                                outcome = "stale_pointer";
                                if !stale {
                                    let disposition = source.borrow().dispatch(&snapshot, &event);
                                    let admitted = match disposition {
                                        InputDisposition::Handled => {
                                            outcome = "native_handled";
                                            true
                                        }
                                        InputDisposition::Rejected => {
                                            outcome = "native_rejected";
                                            false
                                        }
                                        InputDisposition::ExactFallback => {
                                            // Only unsealed pointer handlers need a new
                                            // offer after earlier input. Native owners
                                            // have already validated their own identity.
                                            let exact_unrevoked =
                                                !is_pointer || token.sequence() > exact_floor;
                                            if exact_unrevoked
                                                && exact_ready
                                                && input.geometry == current_geometry
                                                && platform.dispatch_input_exact(
                                                    token.clone(),
                                                    event.clone(),
                                                )
                                            {
                                                outcome = "exact_handled";
                                                true
                                            } else {
                                                let committed = commit_text(
                                                    &platform,
                                                    &source.borrow(),
                                                    &snapshot,
                                                    &event,
                                                )?;
                                                outcome = if committed {
                                                    "text_committed"
                                                } else if !exact_unrevoked {
                                                    "exact_revoked"
                                                } else if !exact_ready {
                                                    "exact_not_ready"
                                                } else if input.geometry != current_geometry {
                                                    "exact_geometry_changed"
                                                } else {
                                                    "exact_rejected"
                                                };
                                                committed
                                            }
                                        }
                                    };
                                    if admitted {
                                        release_pending = false;
                                    }
                                    if admitted
                                        && matches!(&input.event, Event::Key(key) if key.kind == KeyEventKind::Press)
                                        && let PlatformInput::KeyDown(key) = &event
                                    {
                                        // This terminal does not negotiate physical key releases.
                                        // Press is one complete stroke; preserve explicit Repeat
                                        // and clean up only the original native source/load.
                                        source.borrow().dispatch(
                                            &snapshot,
                                            &PlatformInput::KeyUp(gpui::KeyUpEvent {
                                                keystroke: key.keystroke.clone(),
                                            }),
                                        );
                                    }
                                    if admitted
                                        && matches!(
                                            event,
                                            PlatformInput::MouseUp(_) | PlatformInput::KeyDown(_)
                                        )
                                    {
                                        let mut state = presentation.state.lock().unwrap();
                                        let inbox = state.pixels.as_mut().unwrap();
                                        inbox.exact_reject_through =
                                            inbox.exact_reject_through.max(token.sequence());
                                    }
                                }
                            } else {
                                outcome = "unmapped";
                            }
                        }
                    }
                }
                if release_pending {
                    source.borrow().cancel_pointer();
                    refresh = true;
                }
                if refresh {
                    application.update(|cx| {
                        if let Some(window) = window.borrow().as_ref() {
                            let _ = window.update(cx, |_, window, cx| {
                                window.refresh();
                                cx.notify();
                            });
                        }
                    });
                }
                if let Some(index) = measurement {
                    let end = Instant::now();
                    let mut state = presentation.state.lock().unwrap();
                    if let Some(trace) = &mut state.trace {
                        trace.finish_input(index, end, outcome);
                    }
                }
            }
            let allow_draw = {
                let state = presentation.state.lock().unwrap();
                let inbox = state.pixels.as_ref().unwrap();
                inputs_before_draw == 0
                    && inbox.pending.is_none()
                    && inbox.encoded.is_none()
                    && inbox.resize.is_none()
                    && geometry.get() == inbox.geometry
                    && inbox.capabilities.viewport().is_some()
            };
            if let Some(started) = &pump_started {
                started.set(allow_draw.then(Instant::now));
            }
            worked |= platform.pump_with_draw(allow_draw);
            if let Some(started) = &pump_started {
                started.set(None);
            }
            if inputs_before_draw == 0 {
                worked |= encode_pending(presentation, &background);
            }
            let retained = presentation
                .state
                .lock()
                .unwrap()
                .pixels
                .as_ref()
                .unwrap()
                .retained();
            snapshots
                .borrow_mut()
                .retain(|token, _| retained.contains(token));
            if let Some(error) = platform.take_error() {
                return Err(error);
            }
            if platform.is_quit_requested() {
                presentation.state.lock().unwrap().quit = true;
                return Ok(());
            }
            if let Some(text) = platform.take_clipboard_write().and_then(|item| item.text()) {
                let bytes = nsh_platform_tui::ServiceOutput::Clipboard(text)
                    .terminal_sequence(std::env::var_os("TMUX").is_some())
                    .into_bytes();
                presentation
                    .state
                    .lock()
                    .unwrap()
                    .pixels
                    .as_mut()
                    .unwrap()
                    .effects = Some(bytes);
                presentation.wake_driver();
            }
            let input_pending = presentation
                .state
                .lock()
                .unwrap()
                .pixels
                .as_ref()
                .unwrap()
                .has_pending_input();
            if !worked && !input_pending {
                let wait = platform
                    .next_deadline()
                    .map_or(Duration::from_millis(16), |deadline| {
                        deadline
                            .saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(16))
                    });
                thread::park_timeout(wait);
            }
        }
        Ok(())
    })();
    application.update(|cx| {
        if let Some(window) = window.borrow().as_ref() {
            let _ = window.update(cx, |view, _, _| view.stop());
        }
    });
    drop(application);
    platform.shutdown();
    result
}

/// Admit one CPU encoding job on the existing background executor. The native
/// input/GPU owner keeps pumping while the same in-flight token retains its
/// snapshot. Neither encoding nor executor shutdown can hold terminal cleanup.
fn encode_pending(presentation: &Presentation, background: &gpui::BackgroundExecutor) -> bool {
    if presentation.stop.load(Ordering::Acquire) {
        return false;
    }
    let work = {
        let mut state = presentation.state.lock().unwrap();
        if state.quit || state.error.is_some() {
            return false;
        }
        let slots = state.pixel_probe.slots;
        let tracing = state.trace.is_some();
        let inbox = state.pixels.as_mut().unwrap();
        if inbox.in_flight.is_some() || inbox.encoded.is_some() || inbox.resize.is_some() {
            return false;
        }
        let Some(candidate) = inbox.pending.take() else {
            return false;
        };
        if candidate.geometry != inbox.geometry {
            return false;
        }
        let previous = inbox.last_fully_written_image;
        let image = if previous == Some(slots.first) {
            slots.second
        } else {
            slots.first
        };
        let capabilities = inbox.capabilities;
        inbox.in_flight = Some(candidate.frame.token());
        if let Some(trace) = &mut state.trace {
            let at = Instant::now();
            trace.record(
                &candidate.frame.token(),
                candidate.geometry,
                "codec_queued",
                at,
                at,
            );
        }
        (candidate, image, previous, capabilities, tracing)
    };
    let presentation = presentation.clone();
    background
        .spawn(async move {
            let (candidate, image, previous, caps, tracing) = work;
            let size = caps.size;
            use super::{input::WireFrame, probe::Protocol};
            use nsh_gpui_terminal::inline::{InlineFrame, InlineProtocol};
            let started = tracing.then(Instant::now);
            let result = match caps.protocol.expect("negotiated pixel transport") {
                Protocol::Kitty => nsh_gpui_terminal::kitty::KittyFrame::encode(
                    &candidate.frame,
                    image,
                    previous,
                    size.cols,
                    size.rows,
                )
                .map(WireFrame::Kitty),
                protocol @ (Protocol::Sixel | Protocol::Iterm2) => InlineFrame::encode(
                    &candidate.frame,
                    match protocol {
                        Protocol::Sixel => InlineProtocol::Sixel,
                        _ => InlineProtocol::Iterm2,
                    },
                    size.cols,
                    size.rows,
                    caps.synchronized,
                )
                .map(WireFrame::Inline),
            }
            .map(|frame| super::input::Encoded {
                frame,
                geometry: candidate.geometry,
            })
            .map_err(|error| error.to_string());
            let ended = started.map(|_| Instant::now());
            let bytes = if tracing {
                result.as_ref().ok().map(|encoded| match &encoded.frame {
                    WireFrame::Kitty(frame) => frame.wire().len(),
                    WireFrame::Inline(frame) => frame.bytes().len(),
                })
            } else {
                None
            };
            let failed = result.is_err();
            let mut state = presentation.state.lock().unwrap();
            let cancelled =
                state.quit || state.error.is_some() || presentation.stop.load(Ordering::Acquire);
            let inbox = state.pixels.as_mut().unwrap();
            let discarded = cancelled || inbox.geometry != candidate.geometry;
            if discarded {
                if inbox.in_flight.as_ref() == Some(&candidate.frame.token()) {
                    inbox.in_flight = None;
                }
            } else {
                inbox.encoded = Some(result);
            }
            let wake = inbox.wake.clone();
            if let (Some(start), Some(end), Some(trace)) = (started, ended, &mut state.trace) {
                let sample = trace.record(
                    &candidate.frame.token(),
                    candidate.geometry,
                    "codec",
                    start,
                    end,
                );
                sample.bytes = bytes;
                sample.image = (caps.protocol == Some(Protocol::Kitty)).then_some(image);
                sample.outcome = if failed {
                    "error"
                } else if discarded {
                    "discarded"
                } else {
                    "ok"
                };
            }
            drop(state);
            if let Some(wake) = wake {
                wake.unpark();
            }
            presentation.wake_driver();
        })
        .detach();
    true
}

fn commit_text(
    platform: &TerminalPlatform,
    source: &InputSource,
    snapshot: &InputSnapshot,
    event: &PlatformInput,
) -> Result<bool, String> {
    let PlatformInput::KeyDown(key) = event else {
        return Ok(false);
    };
    let modifiers = key.keystroke.modifiers;
    if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
        return Ok(false);
    }
    let Some(text) = &key.keystroke.key_char else {
        return Ok(false);
    };
    if !source.editor_matches(snapshot) {
        return Ok(false);
    }
    platform.paste(text).map_err(|error| error.to_string())?;
    Ok(true)
}
