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

mod input;
mod mapping;
mod probe;
mod session;

use super::{Presentation, output::Output};
pub(super) use input::Inbox;
use nsh_engine::EngineConfig;
pub(super) use probe::Capabilities;
pub(super) use probe::Probe;
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
            state.graphics.flushed(flushed);
            state.pixel_probe.flushed(flushed);
        }
        if let Some(decision) = state.pixel_probe.decision(Instant::now(), size) {
            if let Some(capabilities) = decision {
                state.pixels.as_mut().unwrap().capabilities = capabilities;
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
        drop(state);
        thread::sleep(Duration::from_millis(2));
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
    Retire(input::Offer),
    Effect,
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
) -> Result<(), String> {
    let mut setup = b"\x1b[?1003h\x1b[?1004h".to_vec();
    if caps.pixel_mouse {
        setup.extend_from_slice(b"\x1b[?1016h");
    }
    output.submit(setup, None)?;
    let startup = session::spawn(config, presentation.clone())?;
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
                Phase::Upload(upload) => Phase::Ack {
                    upload,
                    deadline: at + Duration::from_millis(250),
                },
                Phase::Retire(offer) => {
                    let mut state = presentation.state.lock().unwrap();
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
        if let Phase::Ack { upload, deadline } = &phase {
            let ack = presentation
                .state
                .lock()
                .unwrap()
                .pixels
                .as_mut()
                .unwrap()
                .ack
                .take();
            if let Some((at, result)) = ack {
                if at > *deadline {
                    return Err(
                        "terminal did not acknowledge the pixel frame before its deadline".into(),
                    );
                }
                result.map_err(|error| format!("terminal rejected the pixel frame: {error}"))?;
                presentation
                    .state
                    .lock()
                    .unwrap()
                    .pixels
                    .as_mut()
                    .unwrap()
                    .displayed_image = Some(upload.image);
                let offer = upload.offer.clone();
                output.submit(upload.retire.clone(), None)?;
                phase = Phase::Retire(offer);
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
                            thread::sleep(Duration::from_millis(2));
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
            let inbox = state.pixels.as_mut().unwrap();
            if let Some(bytes) = inbox.effects.take() {
                output.submit(bytes, None)?;
                phase = Phase::Effect;
            } else if let Some(encoded) = inbox.encoded.take() {
                let encoded = encoded?;
                if encoded.geometry != inbox.geometry {
                    if inbox.in_flight.as_ref() == Some(&encoded.frame.token()) {
                        inbox.in_flight = None;
                    }
                    continue;
                }
                let image = encoded.frame.image_id();
                // Arm before the first byte; the reader can receive the ACK
                // before this driver observes the writer's flush receipt.
                inbox.expected_ack = Some(image);
                inbox.ack = None;
                let upload = PixelUpload {
                    offer: input::Offer {
                        token: encoded.frame.token(),
                        geometry: encoded.geometry,
                    },
                    image,
                    retire: encoded.frame.retire_previous().to_vec(),
                };
                output.submit(encoded.frame.upload().to_vec(), None)?;
                phase = Phase::Upload(upload);
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn cleanup(presentation: &Presentation) -> Vec<u8> {
    let state = presentation.state.lock().unwrap();
    if state.pixels.is_none() {
        return Vec::new();
    }
    let slots = state.pixel_probe.slots;
    let mut bytes = b"\x1b[?1016l\x1b[?1004l\x1b[?1003l".to_vec();
    bytes.extend(
        nsh_gpui_terminal::kitty::cleanup(slots.first, slots.second)
            .expect("distinct nonzero owned image slots"),
    );
    bytes
}
