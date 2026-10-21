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

use super::{super::Presentation, mapping::Mapping};
use gpui::{
    AppContext, Application, Bounds, PlatformInput, WindowBounds, WindowOptions, point, px, size,
};
use nsh_engine::EngineConfig;
use nsh_gpui_terminal::{FrameToken, TerminalPlatform};
use nsh_platform_gpui::{AppOptions, AppView, InputDisposition, InputSnapshot, InputSource};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, atomic::Ordering, mpsc},
    thread,
    time::{Duration, Instant},
};
use termina::Event;

pub(super) fn spawn(
    config: EngineConfig,
    presentation: Presentation,
) -> Result<mpsc::Receiver<Result<(), EngineConfig>>, String> {
    let (ready, startup) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("maka-nsh-gpui".into())
        .spawn(move || {
            let result = run(config, &presentation, ready);
            if let Err(error) = result {
                presentation.state.lock().unwrap().error = Some(error);
            }
        })
        .map(|_| startup)
        .map_err(|error| error.to_string())
}

fn run(
    mut config: EngineConfig,
    presentation: &Presentation,
    ready: mpsc::SyncSender<Result<(), EngineConfig>>,
) -> Result<(), String> {
    let renderer = match gpui_wgpu::WgpuHeadlessRenderer::new() {
        Ok(renderer) => renderer,
        Err(_) => {
            let _ = ready.send(Err(config));
            return Ok(());
        }
    };
    if ready.send(Ok(())).is_err() {
        return Ok(());
    }
    let snapshots = Rc::new(RefCell::new(HashMap::<FrameToken, InputSnapshot>::new()));
    let source = Rc::new(RefCell::new(InputSource::default()));
    let sampled = source.clone();
    let retained = snapshots.clone();
    let delivered = presentation.clone();
    let owner = thread::current();
    let waker = owner.clone();
    let platform = TerminalPlatform::new(
        Box::new(renderer),
        Box::new(move |frame| {
            if let Some(snapshot) = sampled.borrow().sample() {
                retained.borrow_mut().insert(frame.token(), snapshot);
                if let Some(inbox) = &mut delivered.state.lock().unwrap().pixels {
                    inbox.pending = Some(frame);
                }
            }
        }),
        Arc::new(move || waker.unpark()),
    )
    .map_err(|error| error.to_string())?;
    let caps = {
        let mut state = presentation.state.lock().unwrap();
        let inbox = state.pixels.as_mut().expect("selected pixel presentation");
        inbox.wake = Some(owner);
        inbox.capabilities
    };
    let viewport = caps
        .viewport()
        .ok_or("terminal pixel dimensions are unavailable")?;
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
                view.on_error(move |message| {
                    error.state.lock().unwrap().error = Some(message.into());
                });
                view.load_with_config(config);
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
    let mut mapping = Mapping::default();
    let result = (|| {
        while !presentation.stop.load(Ordering::Acquire) {
            let (resize, cancel, ready, input, caps, quit, error) = {
                let mut state = presentation.state.lock().unwrap();
                let quit = state.quit;
                let error = state.error.clone();
                let inbox = state.pixels.as_mut().expect("selected pixel presentation");
                (
                    inbox.resize.take(),
                    std::mem::take(&mut inbox.cancel_capture),
                    inbox.ready.take(),
                    inbox.pop(),
                    inbox.capabilities,
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
                source.borrow().cancel_capture();
            }
            if resize.is_some() {
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
            if let Some(token) = ready {
                // Resize invalidates the receipt immediately, even while GPU
                // readback or an old output transaction is still finishing.
                if resize.is_none() && platform.mark_presented(token.clone()).is_ok() {
                    let mut state = presentation.state.lock().unwrap();
                    let inbox = state.pixels.as_mut().unwrap();
                    if inbox.resize.is_none() {
                        inbox.published = Some(token);
                    }
                }
            }
            // Mount/focus work can precede its next draw. Complete the
            // foreground/draw barrier before admitting any opaque callback;
            // no pump occurs between this check and its actual dispatch.
            platform.pump();
            let exact_ready = !platform.has_pending();
            if let Some(input) = input {
                let token = input.token.or_else(|| platform.presented());
                match &input.event {
                    Event::FocusIn => {
                        platform
                            .set_active(true)
                            .map_err(|error| error.to_string())?;
                    }
                    Event::FocusOut => {
                        source.borrow().cancel_capture();
                        platform
                            .set_active(false)
                            .map_err(|error| error.to_string())?;
                    }
                    event => {
                        if let Some(token) = token
                            && let Some(snapshot) = snapshots.borrow().get(&token).cloned()
                        {
                            if let Event::Paste(text) = event {
                                if source.borrow().editor_matches(&snapshot) {
                                    platform.paste(text).map_err(|error| error.to_string())?;
                                }
                            } else if let Some(event) = mapping.input(event, caps) {
                                let stale = super::input::pointer(&input.event)
                                    && token.sequence()
                                        <= presentation
                                            .state
                                            .lock()
                                            .unwrap()
                                            .pixels
                                            .as_ref()
                                            .unwrap()
                                            .reject_through;
                                if !stale {
                                    let disposition = source.borrow().dispatch(&snapshot, &event);
                                    match disposition {
                                        InputDisposition::Handled => {}
                                        InputDisposition::Rejected => continue,
                                        InputDisposition::ExactFallback => {
                                            if !(exact_ready
                                                && platform.dispatch_input_exact(
                                                    token.clone(),
                                                    event.clone(),
                                                ))
                                                && let PlatformInput::KeyDown(key) = &event
                                                && !key.keystroke.modifiers.control
                                                && !key.keystroke.modifiers.alt
                                                && !key.keystroke.modifiers.platform
                                                && !key.keystroke.modifiers.function
                                                && let Some(text) = &key.keystroke.key_char
                                                && source.borrow().editor_matches(&snapshot)
                                            {
                                                platform
                                                    .paste(text)
                                                    .map_err(|error| error.to_string())?;
                                            }
                                        }
                                    }
                                    if matches!(
                                        event,
                                        PlatformInput::MouseUp(_) | PlatformInput::KeyDown(_)
                                    ) {
                                        let mut state = presentation.state.lock().unwrap();
                                        let inbox = state.pixels.as_mut().unwrap();
                                        inbox.reject_through =
                                            inbox.reject_through.max(token.sequence());
                                    }
                                }
                            }
                        }
                    }
                }
                application.update(|cx| {
                    if let Some(window) = window.borrow().as_ref() {
                        let _ = window.update(cx, |_, window, cx| {
                            window.refresh();
                            cx.notify();
                        });
                    }
                });
            }
            platform.pump();
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
                if bytes.is_empty() {
                    return Err("terminal copy exceeds its 64 KiB limit".into());
                }
                presentation
                    .state
                    .lock()
                    .unwrap()
                    .pixels
                    .as_mut()
                    .unwrap()
                    .effects = Some(bytes);
            }
            if !platform.has_pending() {
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
