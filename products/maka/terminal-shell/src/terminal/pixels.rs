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

mod geometry;
mod input;
mod mapping;
mod probe;
mod session;

use super::{Presentation, output::Output};
pub(super) use input::Inbox;
use nsh_engine::EngineConfig;
pub(super) use probe::Probe;
pub(super) use probe::{Capabilities, MouseCoordinates};
use std::{
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};

/// Uses the single input reader during negotiation. No engine has started, so
/// automatic selection cannot create competing applications or Host clients.
pub(super) fn select(
    output: &mut Output,
    presentation: &Presentation,
    size: termina::WindowSize,
) -> Result<Option<Capabilities>, String> {
    loop {
        if presentation.stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        let flushed = if output.poll()? {
            output.take_flushed()
        } else {
            None
        };
        let mut state = presentation.state.lock().unwrap();
        if state.quit {
            return Ok(None);
        }
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        if let Some(flushed) = flushed {
            state.graphics.flushed();
            state.pixel_probe.flushed(flushed);
        }
        if let Some(decision) = state.pixel_probe.decision(Instant::now(), size) {
            if let Some(capabilities) = decision {
                state.pixels.as_mut().unwrap().capabilities = capabilities;
                if let Some(trace) = &mut state.trace {
                    trace.protocol = capabilities.protocol.map(|protocol| match protocol {
                        probe::Protocol::Kitty => "kitty",
                        probe::Protocol::Sixel => "sixel",
                        probe::Protocol::Iterm2 => "iterm2",
                    });
                }
            } else if let Some(pixels) = state.pixels.take() {
                let (events, resize) = pixels.into_startup();
                for event in events {
                    if let Some(event) = super::adapter::event(event) {
                        state.push(event, 0);
                    }
                }
                if let Some(size) = resize {
                    state.push(crossterm::event::Event::Resize(size.cols, size.rows), 0);
                }
            }
            return Ok(decision);
        }
        let wait = state
            .pixel_probe
            .next_deadline()
            .map_or(super::TICK, |deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(super::TICK)
            });
        drop(state);
        thread::park_timeout(wait);
    }
}

enum Phase {
    Setup,
    Idle,
    Upload(PixelUpload),
    Ack {
        upload: PixelUpload,
        deadline: Instant,
    },
    Publish(input::Offer),
    Effect,
    Measure,
}

struct PixelUpload {
    offer: input::Offer,
    image: u32,
    retire: Vec<u8>,
}

/// Drives only terminal transport. GPU work runs on its native UI thread, so
/// blocked readback or stdout cannot prevent Ctrl+Q and Host-scope cleanup.
pub(super) fn run(
    output: &mut Output,
    presentation: &Presentation,
    config: EngineConfig,
    caps: Capabilities,
    worker: &mut Option<super::GpuWorker>,
) -> Result<(), String> {
    let mut setup = b"\x1b[?1003h\x1b[?1004h".to_vec();
    if matches!(caps.mouse, MouseCoordinates::Pixels(_)) {
        setup.extend_from_slice(b"\x1b[?1016h");
    }
    let tracing = {
        let mut state = presentation.state.lock().unwrap();
        if caps.protocol == Some(probe::Protocol::Sixel) && state.pixel_probe.needs_static_sixel() {
            setup.extend_from_slice(b"\x1b[?80h");
            // Arm restoration before submission: Close remains ordered after
            // this complete setup even if cancellation races its receipt.
            state.pixels.as_mut().unwrap().restore_sixel_mode = true;
        }
        state.trace.is_some()
    };
    output.submit(setup, None)?;
    let (startup, handle) = session::spawn(config, presentation.clone())?;
    *worker = Some(handle);
    let mut starting = true;
    let mut phase = Phase::Setup;
    loop {
        if presentation.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        {
            let state = presentation.state.lock().unwrap();
            if state.quit {
                return Ok(());
            }
            if let Some(error) = &state.error {
                return Err(error.clone());
            }
        }
        let flushed = if output.poll()? {
            output.take_flushed()
        } else {
            None
        };
        if let Some(at) = flushed {
            phase = match phase {
                Phase::Setup | Phase::Effect => Phase::Idle,
                Phase::Measure => {
                    presentation
                        .state
                        .lock()
                        .unwrap()
                        .pixel_probe
                        .measurement
                        .flushed(at);
                    Phase::Idle
                }
                Phase::Upload(upload) => {
                    if tracing {
                        let mut state = presentation.state.lock().unwrap();
                        if let Some(trace) = &mut state.trace {
                            let sample = trace.record(
                                &upload.offer.token,
                                upload.offer.geometry,
                                "upload_flushed",
                                at,
                                at,
                            );
                            sample.image = Some(upload.image);
                        }
                    }
                    Phase::Ack {
                        upload,
                        deadline: at + Duration::from_secs(5),
                    }
                }
                Phase::Publish(offer) => {
                    let mut state = presentation.state.lock().unwrap();
                    if let Some(trace) = &mut state.trace {
                        let stage = if caps.protocol == Some(probe::Protocol::Kitty) {
                            "retire_flushed"
                        } else {
                            "upload_flushed"
                        };
                        trace.record(&offer.token, offer.geometry, stage, at, at);
                    }
                    let inbox = state.pixels.as_mut().unwrap();
                    inbox.ready = Some(offer);
                    if let Some(wake) = &inbox.wake {
                        wake.unpark();
                    }
                    Phase::Idle
                }
                Phase::Idle | Phase::Ack { .. } => {
                    return Err("unexpected terminal output receipt".into());
                }
            };
        }
        {
            let mut state = presentation.state.lock().unwrap();
            let measured = state.pixel_probe.measurement.poll(Instant::now());
            let inbox = state.pixels.as_mut().unwrap();
            let needs_pixels = inbox.resize.is_some() && inbox.capabilities.pixel_size().is_none();
            match measured {
                Ok(Some((generation, width, height)))
                    if generation == inbox.geometry && needs_pixels =>
                {
                    inbox.capabilities.size.pixel_width = Some(width);
                    inbox.capabilities.size.pixel_height = Some(height);
                    inbox.resize = Some(inbox.capabilities.size);
                    if let Some(wake) = &inbox.wake {
                        wake.unpark();
                    }
                }
                Err(error) if needs_pixels => return Err(error),
                _ => {}
            }
        }
        if let Phase::Ack { upload, deadline } = &phase {
            let ack = {
                let mut state = presentation.state.lock().unwrap();
                let ack = state.pixels.as_mut().unwrap().ack.take();
                if let Some((at, result)) = &ack
                    && let Some(trace) = &mut state.trace
                {
                    let sample = trace.record(
                        &upload.offer.token,
                        upload.offer.geometry,
                        "matching_ack",
                        *at,
                        *at,
                    );
                    sample.image = Some(upload.image);
                    sample.outcome = if result.is_ok() { "ok" } else { "error" };
                }
                ack
            };
            if let Some((at, result)) = ack {
                if at > *deadline {
                    return Err(
                        "terminal did not acknowledge the pixel frame before its deadline".into(),
                    );
                }
                result.map_err(|error| format!("terminal rejected the pixel frame: {error}"))?;
                let mut state = presentation.state.lock().unwrap();
                state.pixels.as_mut().unwrap().displayed_image = Some(upload.image);
                if let Some(trace) = &mut state.trace {
                    let at = Instant::now();
                    let sample = trace.record(
                        &upload.offer.token,
                        upload.offer.geometry,
                        "retire_submit_begin",
                        at,
                        at,
                    );
                    sample.bytes = Some(upload.retire.len());
                    sample.image = Some(upload.image);
                }
                drop(state);
                let offer = upload.offer.clone();
                output.submit(upload.retire.clone(), None)?;
                phase = Phase::Publish(offer);
            } else if Instant::now() >= *deadline {
                return Err("terminal pixel-frame acknowledgement timed out".into());
            }
        }
        if matches!(phase, Phase::Idle) {
            if starting {
                match startup.try_recv() {
                    Ok(Ok(())) => starting = false,
                    Ok(Err(config)) => {
                        // No app was started. Return the exact original Host
                        // configuration to the character renderer.
                        let mut restore = cleanup(presentation);
                        // Kitty keeps one active mouse tracking/encoding
                        // value: resetting pixel modes also clears the
                        // original character modes. Re-enable those after
                        // cleanup without re-entering the alternate screen.
                        restore.extend_from_slice(b"\x1b[?1000h\x1b[?1002h\x1b[?1006h");
                        output.submit(restore, None)?;
                        while !output.poll()? {
                            if presentation.stop.load(Ordering::Acquire)
                                || presentation.state.lock().unwrap().quit
                            {
                                return Ok(());
                            }
                            thread::park_timeout(super::TICK);
                        }
                        output.take_flushed();
                        {
                            let mut state = presentation.state.lock().unwrap();
                            if let Some(pixels) = state.pixels.take() {
                                let (events, resize) = pixels.into_startup();
                                for event in events {
                                    if let Some(event) = super::adapter::event(event) {
                                        state.push(event, 0);
                                    }
                                }
                                if let Some(size) = resize {
                                    state.push(
                                        crossterm::event::Event::Resize(size.cols, size.rows),
                                        0,
                                    );
                                }
                            }
                        }
                        if presentation.stop.load(Ordering::Acquire)
                            || presentation.state.lock().unwrap().quit
                        {
                            return Ok(());
                        }
                        let (commands, frames) = super::ui::spawn(config, presentation.clone())
                            .map_err(|error| error.to_string())?;
                        return super::drive(output, presentation, &commands, &frames);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        return Err("terminal GPUI worker stopped during startup".into());
                    }
                }
            }
            let mut state = presentation.state.lock().unwrap();
            let missing = state
                .pixels
                .as_ref()
                .filter(|inbox| inbox.resize.is_some() && inbox.capabilities.pixel_size().is_none())
                .map(|inbox| inbox.geometry);
            if let Some(generation) = missing
                && state.pixel_probe.measurement.begin(generation)?
            {
                output.submit(geometry::QUERY.to_vec(), None)?;
                phase = Phase::Measure;
            }
            let inbox = state.pixels.as_mut().unwrap();
            if !matches!(phase, Phase::Idle) {
                continue;
            }
            if let Some(bytes) = inbox.effects.take() {
                output.submit(bytes, None)?;
                phase = Phase::Effect;
            } else if let Some(encoded) = inbox.encoded.take() {
                if let Some(wake) = &inbox.wake {
                    wake.unpark();
                }
                let encoded = encoded?;
                if encoded.geometry != inbox.geometry {
                    if inbox.in_flight.as_ref() == Some(&encoded.frame.token()) {
                        inbox.in_flight = None;
                    }
                    continue;
                }
                let offer = input::Offer {
                    token: encoded.frame.token(),
                    geometry: encoded.geometry,
                };
                let sample = tracing.then(|| {
                    let at = Instant::now();
                    let (bytes, image) = match &encoded.frame {
                        input::WireFrame::Kitty(frame) => {
                            (frame.upload().len(), Some(frame.image_id()))
                        }
                        input::WireFrame::Inline(frame) => (frame.bytes().len(), None),
                    };
                    (offer.clone(), at, bytes, image)
                });
                match encoded.frame {
                    input::WireFrame::Kitty(frame) => {
                        let image = frame.image_id();
                        // Arm before the first byte; ACK may precede the
                        // driver's observation of the writer flush receipt.
                        inbox.expected_ack = Some(image);
                        inbox.ack = None;
                        let upload = PixelUpload {
                            offer,
                            image,
                            retire: frame.retire_previous().to_vec(),
                        };
                        output.submit(frame.upload().to_vec(), None)?;
                        phase = Phase::Upload(upload);
                    }
                    input::WireFrame::Inline(frame) => {
                        // Sixel and OSC File have no per-image ACK. Only the
                        // original writer's full flush publishes this draw.
                        output.submit(frame.bytes().to_vec(), None)?;
                        phase = Phase::Publish(offer);
                    }
                }
                if let Some((offer, at, bytes, image)) = sample
                    && let Some(trace) = &mut state.trace
                {
                    let sample =
                        trace.record(&offer.token, offer.geometry, "upload_submit_begin", at, at);
                    sample.bytes = Some(bytes);
                    sample.image = image;
                }
            }
        }
        let measured = presentation
            .state
            .lock()
            .unwrap()
            .pixel_probe
            .measurement
            .next_deadline();
        let deadline = match &phase {
            Phase::Ack { deadline, .. } => Some(measured.map_or(*deadline, |at| at.min(*deadline))),
            _ => measured,
        };
        // IO, input and producer completions wake this driver directly. The
        // existing tick only bounds observation of external AtomicBool cancel.
        let wait = deadline.map_or(super::TICK, |deadline| {
            deadline
                .saturating_duration_since(Instant::now())
                .min(super::TICK)
        });
        thread::park_timeout(wait);
    }
}

pub(super) fn cleanup(presentation: &Presentation) -> Vec<u8> {
    let state = presentation.state.lock().unwrap();
    if state.pixels.is_none() {
        return Vec::new();
    }
    let inbox = state.pixels.as_ref().unwrap();
    let mut bytes = b"\x1b[?1016l\x1b[?1004l\x1b[?1003l".to_vec();
    match inbox.capabilities.protocol {
        Some(probe::Protocol::Kitty) => {
            let slots = state.pixel_probe.slots;
            bytes.extend(
                nsh_gpui_terminal::kitty::cleanup(slots.first, slots.second)
                    .expect("distinct nonzero owned image slots"),
            );
        }
        Some(probe::Protocol::Sixel | probe::Protocol::Iterm2) => {
            bytes.extend_from_slice(nsh_gpui_terminal::inline::cleanup());
        }
        None => {}
    }
    if inbox.restore_sixel_mode {
        bytes.extend_from_slice(b"\x1b[?80l");
    }
    bytes
}
