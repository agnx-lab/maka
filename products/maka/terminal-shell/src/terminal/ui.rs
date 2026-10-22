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
    Presentation,
    input::{Input, Pointer},
    output::FrameWriter,
};
use crossterm::event::{Event, MouseButton, MouseEventKind};
use nsh_core::{MountBatch, platform::Environment};
use nsh_engine::{EngineConfig, LocalEngine, MountSink};
use nsh_platform_tui::{
    CellMetrics, ImageProtocol, InputSnapshot, PressLease, ProtocolType, ServiceOutput,
    ServiceRequest, Surface, SurfaceEvent, TerminalTextEngine,
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::{Backend, CrosstermBackend},
    layout::Rect,
};
use std::{
    io,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Instant,
};

#[derive(Default)]
struct Commits {
    batches: Vec<MountBatch>,
    error: Option<String>,
    frame: bool,
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Commits>>);

impl MountSink for Sink {
    fn submit(&self, batch: MountBatch) {
        let mut commits = self.0.lock().unwrap();
        // LocalEngine has at most four turns and one idle-GC commit in
        // pump(). This is an explicit fail-closed guard, never replacement
        // of a delta by a later delta.
        if commits.batches.len() >= 8 {
            commits.error = Some("NSH exceeded the per-turn mount batch limit".into());
        } else {
            commits.batches.push(batch);
        }
    }
    fn report_error(&self, message: &str) {
        self.0.lock().unwrap().error = Some(message.into());
    }
    fn request_frame(&self) {
        self.0.lock().unwrap().frame = true;
    }
}

pub(super) struct Frame {
    pub epoch: u64,
    pub bytes: Option<Vec<u8>>,
    /// None only when an idle probe kept the existing presentation.
    pub input: Option<InputSnapshot>,
    pub quit: bool,
}

pub(super) struct Advance {
    pub image_protocol: Option<ImageProtocol>,
    pub inputs: Vec<Input>,
    pub resize: Option<(u16, u16)>,
    pub cancel_pointer: bool,
}

impl Advance {
    pub fn is_probe(&self) -> bool {
        self.inputs.is_empty()
            && self.resize.is_none()
            && !self.cancel_pointer
            && self.image_protocol.is_none()
    }
}

type Frames = mpsc::Receiver<Result<Frame, String>>;

pub(super) fn spawn(
    config: EngineConfig,
    presentation: Presentation,
) -> io::Result<(mpsc::SyncSender<Advance>, Frames)> {
    let (send, commands) = mpsc::sync_channel::<Advance>(1);
    let (frames, receive) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("maka-nsh-ui".into())
        .spawn(move || {
            let mut session = match Session::new(config) {
                Ok(session) => session,
                Err(error) => {
                    let _ = frames.send(Err(error));
                    return;
                }
            };
            session.presentation = presentation;
            if frames.send(session.initial_frame()).is_err() {
                return;
            }
            while let Ok(command) = commands.recv() {
                let result = session.advance_if_needed(command);
                let failed = result.is_err();
                if frames.send(result).is_err() || failed {
                    return;
                }
            }
        })?;
    Ok((send, receive))
}

struct Session {
    engine: LocalEngine<TerminalTextEngine>,
    surface: Surface,
    environment: Environment,
    sink: Sink,
    terminal: Terminal<CrosstermBackend<FrameWriter>>,
    area: Rect,
    epoch: u64,
    started: Instant,
    effects: Vec<u8>,
    presentation: Presentation,
}

impl Session {
    fn new(mut config: EngineConfig) -> Result<Self, String> {
        let metrics = CellMetrics::default();
        let columns = (config.viewport.width / metrics.width()).round() as u16;
        let rows = (config.viewport.height / metrics.height()).round() as u16;
        let area = viewport(columns, rows)?;
        let mut surface = Surface::new(area.width, area.height, metrics);
        let access = match &config.app {
            nsh_engine::App::Js(bundle) => bundle.policy.access(),
            nsh_engine::App::Rust(_) => nsh_engine::Policy::default().access(),
        };
        surface.set_access(access);
        surface.set_host_services_enabled(true);
        let assets = config
            .assets
            .get_or_insert_with(|| Arc::new(crate::ApplicationAssets));
        surface.set_assets(Some(assets.clone()));
        let sink = Sink::default();
        let environment = config.environment;
        let engine = LocalEngine::start(config, TerminalTextEngine::new(metrics), sink.clone())
            .ok_or("NSH UI engine failed to start")?;
        let terminal = Terminal::with_options(
            CrosstermBackend::new(FrameWriter::default()),
            TerminalOptions {
                viewport: Viewport::Fixed(area),
            },
        )
        .map_err(|error| error.to_string())?;
        Ok(Self {
            engine,
            surface,
            environment,
            sink,
            terminal,
            area,
            epoch: 0,
            started: Instant::now(),
            effects: Vec::new(),
            presentation: Presentation::default(),
        })
    }

    fn initial_frame(&mut self) -> Result<Frame, String> {
        self.apply_commits()?;
        self.encode(true)
    }

    fn advance_if_needed(&mut self, command: Advance) -> Result<Frame, String> {
        if command.is_probe() {
            let presentation = &self.presentation;
            let inbox = presentation.state.lock().unwrap();
            if presentation.stop.load(std::sync::atomic::Ordering::Acquire)
                || inbox.has_pending()
                || (!self.engine.needs_pump() && !self.surface.animating())
            {
                return Ok(Frame {
                    epoch: self.epoch,
                    bytes: None,
                    input: None,
                    quit: self.surface.should_quit(),
                });
            }
            presentation
                .presented
                .store(0, std::sync::atomic::Ordering::Release);
        }
        let frame = self.advance(command)?;
        if frame.bytes.is_none()
            && let Some(snapshot) = &frame.input
        {
            self.presentation.publish(frame.epoch, snapshot.clone());
        }
        Ok(frame)
    }

    fn advance(&mut self, command: Advance) -> Result<Frame, String> {
        let mut changed = false;
        if command.cancel_pointer {
            self.cancel_pointer()?;
            changed = true;
        }
        if let Some((columns, rows)) = command.resize {
            self.surface.set_image_protocol(None);
            self.area = viewport(columns, rows)?;
            self.surface.resize(self.area.width, self.area.height);
            self.engine.set_focus_path_now(&self.surface.focus_path());
            self.environment.window = self.surface.viewport();
            let control = self.engine.control();
            control.resize(self.environment.window);
            control.set_environment(self.environment);
            // The supplied area is authoritative. Ratatui's resize path queries
            // the process TTY even for a Fixed viewport, but this backend only
            // encodes into memory for the separate physical output owner.
            let mut terminal = Terminal::with_options(
                CrosstermBackend::new(FrameWriter::default()),
                TerminalOptions {
                    viewport: Viewport::Fixed(self.area),
                },
            )
            .map_err(|error| error.to_string())?;
            crossterm::queue!(
                terminal.backend_mut().writer_mut(),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
            )
            .map_err(|error| error.to_string())?;
            self.terminal = terminal;
            changed = true;
        }
        if let Some(protocol) = command.image_protocol {
            if protocol.protocol_type() != ProtocolType::Sixel {
                return Err("this shell only negotiates stateless Sixel images".into());
            }
            self.surface.set_image_protocol(Some(protocol));
            changed = true;
        }
        for input in &command.inputs {
            if matches!(input.pointer, Some(Pointer::Exact(epoch)) if epoch != self.epoch) {
                return Err("pointer input belongs to an obsolete presentation".into());
            }
        }
        let mut inputs = command.inputs.into_iter().peekable();
        while let Some(input) = inputs.next() {
            if matches!(
                &input.pointer,
                Some(Pointer::Press { epoch, .. } | Pointer::Wheel { epoch, .. })
                    if *epoch <= self.presentation.state.lock().unwrap().resized_through
            ) {
                self.cancel_pointer()?;
                changed = true;
                continue;
            }
            let lease = match &input.pointer {
                Some(Pointer::Press { lease, .. }) => Some(lease.as_ref()),
                _ => None,
            };
            if let Event::Mouse(mouse) = &input.event
                && matches!(
                    mouse.kind,
                    MouseEventKind::ScrollUp
                        | MouseEventKind::ScrollDown
                        | MouseEventKind::ScrollLeft
                        | MouseEventKind::ScrollRight
                )
            {
                // Inbox supplies one bounded group of identical wheels.
                // The Surface resolves its original target once. A custom
                // component may consume only one; its old-frame remainder
                // is revoked, never replayed against another target.
                let steps = (1 + inputs.count()) as u16;
                let consumed = match &input.pointer {
                    Some(Pointer::Wheel { lease, .. }) => self
                        .surface
                        .handle_presented_wheel(*mouse, steps, lease)
                        .unwrap_or(0),
                    _ => self.surface.handle_wheel(*mouse, steps),
                };
                changed |= consumed != 0;
                self.forward_events(true)?;
                break;
            }
            if let (Event::Mouse(mouse), Some(lease)) = (&input.event, lease) {
                if !matches!(
                    mouse.kind,
                    MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
                ) && !(matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
                    && (lease.is_native_pan() || lease.is_native_input()))
                {
                    self.cancel_pointer()?;
                    changed = true;
                    continue;
                }
                match self.surface.handle_presented_mouse(*mouse, lease) {
                    Some(handled) => changed |= handled,
                    None => {
                        self.cancel_pointer()?;
                        changed = true;
                        continue;
                    }
                }
            } else {
                changed |= self.surface.handle_event(input.event);
            }
            let events: Vec<_> = self.surface.drain_events().collect();
            // Finish one captured gesture before applying its JS changes.
            // A later independent gesture validates its original native owner
            // and binding against that updated state.
            let invokes_application = self.dispatch_events(events, true, lease)?;
            if invokes_application {
                if inputs.peek().is_some() {
                    self.cancel_pointer()?;
                    changed = true;
                }
                break;
            }
        }
        self.engine.pump();
        changed |= self.apply_commits()?;
        changed |= self.surface.tick(self.started.elapsed().as_secs_f64());
        self.forward_events(false)?;
        self.encode(changed)
    }

    fn cancel_pointer(&mut self) -> Result<(), String> {
        // NSH's focus-loss path cancels captures. Reconcile immediately to
        // restore the mounted input's focus.
        self.surface.handle_event(Event::FocusLost);
        self.surface.handle_event(Event::FocusGained);
        // Cancellation restores native capture and focus without admitting
        // callbacks from the revoked gesture against a later presentation.
        let events = self
            .surface
            .drain_events()
            .filter(|event| !matches!(event, SurfaceEvent::Event(..)))
            .collect();
        self.dispatch_events(events, false, None).map(|_| ())
    }

    fn forward_events(&mut self, presented_input: bool) -> Result<(), String> {
        let events: Vec<_> = self.surface.drain_events().collect();
        self.dispatch_events(events, presented_input, None)
            .map(|_| ())
    }

    fn dispatch_events(
        &mut self,
        events: Vec<SurfaceEvent>,
        presented_input: bool,
        lease: Option<&PressLease>,
    ) -> Result<bool, String> {
        self.engine.set_focus_path_now(&self.surface.focus_path());
        let mut invokes_application = false;
        let control = self.engine.control();
        for event in events {
            match event {
                SurfaceEvent::Event(id, event, payload) if presented_input => {
                    if !invokes_application {
                        invokes_application = if let Some(lease) = lease {
                            let Some(binding) = lease.binding() else {
                                self.cancel_pointer()?;
                                break;
                            };
                            match self
                                .engine
                                .dispatch_bound_event_now(id, event, payload, binding)
                            {
                                Some(entered) => entered,
                                None => {
                                    self.cancel_pointer()?;
                                    break;
                                }
                            }
                        } else {
                            self.engine.dispatch_event_now(id, event, payload)
                        };
                    }
                }
                SurfaceEvent::Event(id, event, payload) => {
                    control.dispatch_event(id, event, payload)
                }
                SurfaceEvent::HintObservation(observation) => control.observe_hint(observation),
                SurfaceEvent::Scroll(id, offset, applied) => control.scroll(id, offset, applied),
                SurfaceEvent::States(id, states) => control.set_states(id, states),
                SurfaceEvent::AnimationsFinished(finished) => control.animations_finished(finished),
                SurfaceEvent::ComponentSize(id, size) => control.set_component_size(id, size),
                SurfaceEvent::Zoom(id, scale) => control.zoom(id, scale),
                SurfaceEvent::RootSize(id, size, insets) => control.set_root_size(id, size, insets),
                SurfaceEvent::Service(ServiceRequest::ClipboardWrite(text)) => {
                    let sequence = ServiceOutput::Clipboard(text)
                        .terminal_sequence(std::env::var_os("TMUX").is_some());
                    self.effects.extend_from_slice(sequence.as_bytes());
                }
                SurfaceEvent::Service(ServiceRequest::ClipboardRead { id, has_only }) => {
                    self.surface.complete_clipboard(
                        id,
                        has_only,
                        Err("this shell cannot read the system clipboard".into()),
                    );
                }
                SurfaceEvent::Service(ServiceRequest::ClipboardPaste(edit)) => {
                    self.surface.complete_paste(
                        edit,
                        Err("paste through the terminal's bracketed-paste command".into()),
                    );
                }
                SurfaceEvent::Service(_) => {
                    return Err("terminal host services are unavailable in this slice".into());
                }
            }
        }
        Ok(invokes_application)
    }

    fn apply_commits(&mut self) -> Result<bool, String> {
        let mut commits = std::mem::take(&mut *self.sink.0.lock().unwrap());
        if let Some(error) = commits.error {
            return Err(error);
        }
        let changed = !commits.batches.is_empty();
        // Every delta is necessary. In particular Create/Insert and later
        // prop/text deltas cannot be coalesced to the last MountBatch.
        for batch in commits.batches.drain(..) {
            self.surface.apply(batch);
        }
        self.forward_events(false)?;
        if commits.frame {
            self.engine.control().frame();
        }
        Ok(changed)
    }

    fn encode(&mut self, changed: bool) -> Result<Frame, String> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or("terminal presentation epoch exhausted")?;
        if !changed && self.effects.is_empty() {
            return Ok(Frame {
                epoch: self.epoch,
                bytes: None,
                input: Some(self.surface.input_snapshot()),
                quit: self.surface.should_quit(),
            });
        }
        // This UI draws its own caret, so the physical cursor is always hidden.
        let writer = self.terminal.backend_mut().writer_mut();
        crossterm::queue!(writer, crossterm::terminal::BeginSynchronizedUpdate)
            .map_err(|error| error.to_string())?;
        self.surface
            .render(self.area, self.terminal.current_buffer_mut());
        self.terminal.flush().map_err(|error| error.to_string())?;
        self.terminal
            .hide_cursor()
            .map_err(|error| error.to_string())?;
        crossterm::queue!(
            self.terminal.backend_mut().writer_mut(),
            crossterm::terminal::EndSynchronizedUpdate
        )
        .map_err(|error| error.to_string())?;
        self.terminal.swap_buffers();
        Backend::flush(self.terminal.backend_mut()).map_err(|error| error.to_string())?;
        let mut bytes = std::mem::take(&mut self.terminal.backend_mut().writer_mut().0);
        // Complete clipboard effects share this immutable physical write
        // and its original flush receipt.
        bytes.append(&mut self.effects);
        Ok(Frame {
            epoch: self.epoch,
            bytes: Some(bytes),
            input: Some(self.surface.input_snapshot()),
            quit: self.surface.should_quit(),
        })
    }
}

fn viewport(columns: u16, rows: u16) -> Result<Rect, String> {
    let area = Rect::new(0, 0, columns.max(1), rows.max(1));
    if usize::from(area.width) * usize::from(area.height) > 1_048_576 {
        return Err("terminal viewport exceeds cell limit".into());
    }
    Ok(area)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use nsh_core::{EventKind, EventPayload, MountItem, ROOT_ID, ViewKind};
    use nsh_engine::{App, Bundle, Limits};
    use std::io::Write;

    fn config(source: &str) -> EngineConfig {
        let mut config = EngineConfig::new(App::Js(Bundle::source(source.to_owned())));
        config.viewport = CellMetrics::default().viewport(40, 8);
        config.header_heights = nsh_core::NO_NATIVE_HEADER;
        config.limits = Limits::default();
        config.ui_sets = vec![nsh_base::registry()];
        config
    }

    fn publish(
        output: &mut super::super::output::Output,
        presentation: &Presentation,
        frame: Frame,
    ) {
        output
            .submit(
                frame.bytes.unwrap_or_default(),
                Some((
                    presentation.clone(),
                    frame.epoch,
                    frame.input.expect("completed frame input"),
                )),
            )
            .unwrap();
        let deadline = Instant::now() + std::time::Duration::from_secs(2);
        while !output.poll().unwrap() {
            assert!(Instant::now() < deadline, "output did not flush");
            thread::yield_now();
        }
    }

    #[test]
    fn production_resize_updates_the_js_window_breakpoint() {
        let mut options = config(
            r#"
            import { mount } from "nsh";
            import { windowSize } from "nsh/device";
            import { element, insert } from "nsh:rt";
            mount(() => {
                const label = element("text");
                insert(label, () => windowSize().width >= 800 ? "WIDE_WINDOW" : "NARROW_WINDOW");
                return label;
            });
            "#,
        );
        options.environment.window = options.viewport;
        let mut session = Session::new(options).unwrap();
        session.initial_frame().unwrap();
        assert!(session.surface.snapshot().contains("NARROW_WINDOW"));
        for (columns, rows, expected, absent) in [
            (120, 40, "WIDE_WINDOW", "NARROW_WINDOW"),
            (80, 24, "NARROW_WINDOW", "WIDE_WINDOW"),
        ] {
            session
                .advance(Advance {
                    image_protocol: None,
                    inputs: Vec::new(),
                    resize: Some((columns, rows)),
                    cancel_pointer: false,
                })
                .unwrap();
            let screen = session.surface.snapshot();
            assert!(screen.contains(expected), "{screen}");
            assert!(!screen.contains(absent), "{screen}");
        }
    }

    #[test]
    fn packaged_images_load_and_paint_through_the_production_session() {
        use nsh_engine::Assets as _;

        assert_eq!(
            crate::ApplicationAssets.read("asset:/missing.png"),
            Err("Unknown bundled asset: asset:/missing.png".into())
        );
        let mut session = Session::new(config(
            r#"
            import { mount, signal } from "nsh";
            import { element, prop, insert, append } from "nsh:rt";
            const status = signal("loading");
            mount(() => {
                const root = element("view");
                const image = element("image");
                prop(image, "src", "asset:/assets/maka.png");
                prop(image, "style", { width: 64, height: 64, flexShrink: 0 });
                prop(image, "onLoad", e => status.set("loaded:" + e.width + "x" + e.height));
                prop(image, "onError", e => status.set("failed:" + e.message));
                const label = element("text");
                insert(label, () => status());
                append(root, image);
                append(root, label);
                return root;
            });
        "#,
        ))
        .unwrap();
        let mut bytes = session.initial_frame().unwrap().bytes.unwrap_or_default();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let frame = session
                .advance(Advance {
                    image_protocol: None,
                    inputs: Vec::new(),
                    resize: None,
                    cancel_pointer: false,
                })
                .unwrap();
            bytes.extend(frame.bytes.unwrap_or_default());
            let screen = session.surface.snapshot();
            assert!(!screen.contains("failed:"), "{screen}");
            if screen.contains("loaded:1024x1024") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "packaged image did not load:\n{screen}"
            );
            thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            bytes
                .windows("▀".len())
                .any(|bytes| bytes == "▀".as_bytes()),
            "the real output frame must paint the image's Unicode fallback"
        );

        // Exercise the same image protocol with a large clipboard effect.
        // The test captures FrameWriter bytes without writing real stdout.
        let prepare = |session: &mut Session| {
            // A fresh fixed terminal resets both diff buffers without calling
            // Terminal::clear, whose cursor query would read the real stdin.
            session.terminal = Terminal::with_options(
                CrosstermBackend::new(FrameWriter::default()),
                TerminalOptions {
                    viewport: Viewport::Fixed(session.area),
                },
            )
            .unwrap();
            session
                .terminal
                .backend_mut()
                .writer_mut()
                .write_all(b"\x1b[2JPREFIX")
                .unwrap();
            session.terminal.backend().writer().0.clone()
        };
        let protocol = ImageProtocol::new(ProtocolType::Sixel, 24, 48).unwrap();
        let mut native = session
            .advance(Advance {
                image_protocol: Some(protocol),
                inputs: Vec::new(),
                resize: None,
                cancel_pointer: false,
            })
            .unwrap()
            .bytes
            .unwrap_or_default();
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        while !native.windows(2).any(|bytes| bytes == b"\x1bP") {
            assert!(
                Instant::now() < deadline,
                "the real Sixel encoder did not complete"
            );
            native = session
                .advance(Advance {
                    image_protocol: None,
                    inputs: Vec::new(),
                    resize: None,
                    cancel_pointer: false,
                })
                .unwrap()
                .bytes
                .unwrap_or_default();
            thread::sleep(std::time::Duration::from_millis(1));
        }
        let resized = session
            .advance(Advance {
                image_protocol: None,
                inputs: Vec::new(),
                resize: Some((40, 8)),
                cancel_pointer: false,
            })
            .unwrap()
            .bytes
            .unwrap();
        assert!(!resized.windows(2).any(|bytes| bytes == b"\x1bP"));
        assert!(
            resized
                .windows("▀".len())
                .any(|bytes| bytes == "▀".as_bytes())
        );
        let mut restored = session
            .advance(Advance {
                image_protocol: Some(ImageProtocol::new(ProtocolType::Sixel, 24, 48).unwrap()),
                inputs: Vec::new(),
                resize: Some((40, 8)),
                cancel_pointer: false,
            })
            .unwrap()
            .bytes
            .unwrap_or_default();
        let deadline = Instant::now() + std::time::Duration::from_secs(3);
        while !restored.windows(2).any(|bytes| bytes == b"\x1bP") {
            assert!(
                Instant::now() < deadline,
                "Sixel did not resume after resize"
            );
            restored = session
                .advance(Advance {
                    image_protocol: None,
                    inputs: Vec::new(),
                    resize: None,
                    cancel_pointer: false,
                })
                .unwrap()
                .bytes
                .unwrap_or_default();
            thread::sleep(std::time::Duration::from_millis(1));
        }
        let prefix = prepare(&mut session);
        let original = "中😀".repeat(1_200_000);
        let effect = ServiceOutput::Clipboard(original).terminal_sequence(false);
        session.effects.extend_from_slice(effect.as_bytes());
        let frame = session.encode(true).unwrap().bytes.unwrap();
        assert!(frame.len() > 8 * 1024 * 1024);
        assert!(
            frame.starts_with(&prefix),
            "clear and preexisting output precede the complete image and effect"
        );
        assert!(frame.windows(2).any(|bytes| bytes == b"\x1bP"));
        assert!(frame.ends_with(effect.as_bytes()));
        assert_eq!(
            frame
                .windows(7)
                .filter(|bytes| *bytes == b"\x1b]52;c;")
                .count(),
            1
        );
        assert!(session.effects.is_empty());
        assert!(session.encode(false).unwrap().bytes.is_none());
    }

    const APP: &str = r#"
        import { mount, signal } from "nsh";
        import { Button } from "nsh/base";
        import { element, prop, bindProp, insert, append, list } from "nsh:rt";
        const rows = signal([{ id: "A" }]);
        const selected = signal("none");
        mount(() => {
            const root = element("view");
            prop(root, "style", { flex: 1 });
            const result = element("text");
            insert(result, () => "selected:" + selected());
            append(root, result);
            append(root, list({
                get each() { return rows(); },
                estimatedItemSize: 16,
                style: { flex: 1 },
                children: (row) => {
                    const label = element("text");
                    insert(label, () => row().id);
                    return Button({
                        style: { height: 16 },
                        get accessibilityLabel() { return "row:" + row().id; },
                        onPress: () => selected.set(row().id),
                        children: label,
                    });
                }
            }));
            return root;
        });
        requestAnimationFrame(() => rows.set([{ id: "B" }]));
    "#;

    #[test]
    fn presented_input_precedes_a_queued_rebind_of_the_same_list_node() {
        let mut session = Session::new(config(APP)).unwrap();
        session.initial_frame().unwrap();
        let epoch = session.epoch;
        let presentation = Presentation::default();
        presentation.publish(epoch, session.surface.input_snapshot());
        session.presentation = presentation.clone();
        let row = session
            .surface
            .accessibility_snapshot()
            .into_iter()
            .find(|node| node.label == "row:A")
            .unwrap()
            .id;
        for event in click(&session.surface, "row:A") {
            presentation.state.lock().unwrap().push(event, epoch);
        }
        // The driver has already posted an idle probe when the physical
        // click arrives. Its queued frame callback must not rebind A to B
        // before the original input owner can admit that click.
        let probe = session
            .advance_if_needed(Advance {
                image_protocol: None,
                inputs: Vec::new(),
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert_eq!(probe.epoch, epoch);
        assert!(probe.bytes.is_none());
        assert!(
            session
                .surface
                .accessibility_snapshot()
                .iter()
                .any(|node| node.label == "row:A")
        );
        let inputs = presentation.state.lock().unwrap().pop_batch(session.epoch);
        session
            .advance(Advance {
                image_protocol: None,
                inputs,
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(session.surface.snapshot().contains("selected:A"));
        let rebound = session
            .surface
            .accessibility_snapshot()
            .into_iter()
            .find(|node| node.label == "row:B")
            .unwrap()
            .id;
        assert_eq!(row, rebound, "the actual virtual-list cell was retained");
    }

    #[test]
    fn driver_accepts_a_click_queued_in_one_presented_frame() {
        let source = APP.replace(
            "requestAnimationFrame(() => rows.set([{ id: \"B\" }]));",
            "",
        );
        let mut session = Session::new(config(&source)).unwrap();
        drive_burst(&mut session, |surface| click(surface, "row:A"));
        assert!(session.surface.snapshot().contains("selected:A"));
    }

    #[test]
    fn a_flushed_close_survives_background_work_and_an_earlier_action() {
        let mut session = Session::new(config(
            r#"
            import { inputAction, mount, signal } from "nsh";
            import { Button } from "nsh/base";
            import { element, prop, insert, append } from "nsh:rt";
            const status = signal("before");
            const closed = signal(0);
            const close = () => closed.set(closed() + 1);
            mount(() => {
                const root = element("view");
                const result = element("text");
                prop(result, "style", { height: 16 });
                insert(result, () => status() + " closed:" + closed());
                append(root, result);
                append(root, Button({
                    accessibilityLabel: "Close",
                    style: { height: 16 },
                    onPress: inputAction(close, null),
                }));
                return root;
            });
            requestAnimationFrame(() => status.set("after"));
        "#,
        ))
        .unwrap();
        let mut output = super::super::output::Output::new(io::sink()).unwrap();
        let presentation = session.presentation.clone();
        // The next UI revision is ready before stdout has displayed it. The
        // ordinary Close action still belongs to the previously flushed UI.
        let initial = session.initial_frame().unwrap();
        let initial_epoch = initial.epoch;
        publish(&mut output, &presentation, initial);
        let events = click(&session.surface, "Close");
        let background = session
            .advance_if_needed(Advance {
                image_protocol: None,
                inputs: Vec::new(),
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(background.epoch > initial_epoch);
        assert!(session.surface.snapshot().contains("after closed:0"));
        assert_eq!(
            session
                .presentation
                .presented
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
        {
            let mut inbox = session.presentation.state.lock().unwrap();
            for event in events.iter().chain(events.iter()) {
                inbox.push(event.clone(), 0);
            }
        }
        let inputs = session
            .presentation
            .state
            .lock()
            .unwrap()
            .pop_batch(session.epoch);
        assert_eq!(inputs.len(), 2);
        session
            .advance(Advance {
                image_protocol: None,
                inputs,
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(session.surface.snapshot().contains("after closed:1"));
        let inputs = {
            let mut inbox = session.presentation.state.lock().unwrap();
            let inputs = inbox.pop_batch(session.epoch);
            assert_eq!(
                inputs.len(),
                2,
                "the same displayed owner and binding retain the second independent click"
            );
            assert!(!inbox.cancel_pointer);
            inputs
        };
        let acted = session
            .advance(Advance {
                image_protocol: None,
                inputs,
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(session.surface.snapshot().contains("after closed:2"));
        publish(&mut output, &presentation, acted);
        {
            let mut inbox = session.presentation.state.lock().unwrap();
            for event in events {
                inbox.push(event, session.epoch);
            }
        }
        let inputs = session
            .presentation
            .state
            .lock()
            .unwrap()
            .pop_batch(session.epoch);
        let fresh = session
            .advance(Advance {
                image_protocol: None,
                inputs,
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(
            session.surface.snapshot().contains("after closed:3"),
            "a freshly displayed Close remains usable"
        );
        publish(&mut output, &presentation, fresh);
        let events = click(&session.surface, "Close");
        {
            let mut inbox = session.presentation.state.lock().unwrap();
            inbox.push(events[0].clone(), session.epoch);
            let Event::Mouse(mut drag) = events[0].clone() else {
                unreachable!()
            };
            drag.kind = MouseEventKind::Drag(MouseButton::Left);
            inbox.push(Event::Mouse(drag), session.epoch);
            inbox.push(events[1].clone(), session.epoch);
        }
        let (inputs, cancel_pointer) = {
            let mut inbox = session.presentation.state.lock().unwrap();
            (
                inbox.pop_batch(session.epoch),
                std::mem::take(&mut inbox.cancel_pointer),
            )
        };
        session
            .advance(Advance {
                image_protocol: None,
                inputs,
                resize: None,
                cancel_pointer,
            })
            .unwrap();
        assert!(
            session.surface.snapshot().contains("after closed:3"),
            "a cancelled sealed gesture cannot release through the opaque path"
        );
        // A later opaque release must not complete a capture recreated by the
        // cancelled group's Down. This also checks that its pressed state ended.
        session
            .advance(Advance {
                image_protocol: None,
                inputs: vec![Input {
                    event: events[1].clone(),
                    pointer: Some(Pointer::Exact(session.epoch)),
                }],
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(session.surface.snapshot().contains("after closed:3"));
        let inputs = click(&session.surface, "Close")
            .into_iter()
            .map(|event| Input {
                event,
                pointer: Some(Pointer::Exact(session.epoch)),
            })
            .collect();
        session
            .advance(Advance {
                image_protocol: None,
                inputs,
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(
            session.surface.snapshot().contains("after closed:4"),
            "the next native click remains usable after cancellation"
        );
    }

    #[test]
    fn a_flushed_editor_keeps_click_and_drag_selection_while_background_output_is_pending() {
        const APP: &str = r#"
            import { mount, signal } from "nsh";
            import { element, prop, bindProp, insert, append } from "nsh:rt";
            const status = signal("before");
            const draft = signal("hello");
            mount(() => {
                const root = element("view");
                prop(root, "style", { flex: 1 });
                const result = element("text");
                prop(result, "style", { height: 16 });
                insert(result, () => status() + " draft:" + draft());
                append(root, result);
                const previous = element("input");
                prop(previous, "style", { height: 16 });
                prop(previous, "autoFocus", true);
                prop(previous, "value", "previous");
                append(root, previous);
                const input = element("input");
                prop(input, "style", { height: 16 });
                prop(input, "accessibilityLabel", "Composer");
                bindProp(input, "value", draft);
                prop(input, "onChange", text => draft.set(text));
                append(root, input);
                return root;
            });
            requestAnimationFrame(() => status.set("after"));
        "#;
        for (drag, selected, expected, caret) in
            [(false, 2..2, "heXllo", 3..3), (true, 1..4, "hXo", 2..2)]
        {
            let mut session = Session::new(config(APP)).unwrap();
            // Reproduce the completed/pending frame pair from the physical
            // workspace-picker failure without advancing through idle turns.
            session.epoch = 64;
            let mut output = super::super::output::Output::new(io::sink()).unwrap();
            let presentation = session.presentation.clone();
            let initial = session.initial_frame().unwrap();
            assert_eq!(initial.epoch, 65);
            publish(&mut output, &presentation, initial);
            assert_eq!(
                session.surface.input_context().unwrap().text.as_deref(),
                Some("previous")
            );
            let composer = session
                .surface
                .accessibility_snapshot()
                .into_iter()
                .find(|node| node.label == "Composer")
                .unwrap();
            let pointer = |kind, offset| {
                Event::Mouse(MouseEvent {
                    kind,
                    column: (composer.frame.origin.x.0 / 8.0) as u16 + offset,
                    row: (composer.frame.origin.y.0 / 16.0) as u16,
                    modifiers: KeyModifiers::NONE,
                })
            };
            let background = session
                .advance_if_needed(Advance {
                    image_protocol: None,
                    inputs: Vec::new(),
                    resize: None,
                    cancel_pointer: false,
                })
                .unwrap();
            assert_eq!(background.epoch, 66);
            assert!(session.surface.snapshot().contains("after draft:hello"));
            assert_eq!(
                presentation
                    .presented
                    .load(std::sync::atomic::Ordering::Acquire),
                0
            );
            {
                let mut inbox = presentation.state.lock().unwrap();
                assert_eq!(inbox.last_frame.as_ref().unwrap().0, 65);
                inbox.resized_through = 56;
                inbox.push(
                    pointer(
                        MouseEventKind::Down(MouseButton::Left),
                        if drag { 1 } else { 2 },
                    ),
                    0,
                );
                if drag {
                    inbox.push(pointer(MouseEventKind::Drag(MouseButton::Left), 4), 0);
                }
                inbox.push(
                    pointer(
                        MouseEventKind::Up(MouseButton::Left),
                        if drag { 4 } else { 2 },
                    ),
                    0,
                );
                inbox.push(Event::Paste("X".into()), 0);
            }
            publish(&mut output, &presentation, background);
            let (inputs, cancel_pointer) = {
                let mut inbox = presentation.state.lock().unwrap();
                (
                    inbox.pop_batch(session.epoch),
                    std::mem::take(&mut inbox.cancel_pointer),
                )
            };
            assert_eq!(inputs.len(), if drag { 3 } else { 2 });
            let acted = session
                .advance(Advance {
                    image_protocol: None,
                    inputs,
                    resize: None,
                    cancel_pointer,
                })
                .unwrap();
            let editor = session.surface.input_context().unwrap();
            assert_eq!(editor.node, composer.id);
            assert_eq!(editor.text.as_deref(), Some("hello"));
            assert_eq!(editor.selection, selected);
            publish(&mut output, &presentation, acted);
            let inputs = presentation.state.lock().unwrap().pop_batch(session.epoch);
            session
                .advance(Advance {
                    image_protocol: None,
                    inputs,
                    resize: None,
                    cancel_pointer: false,
                })
                .unwrap();
            let editor = session.surface.input_context().unwrap();
            assert_eq!(editor.text.as_deref(), Some(expected));
            assert_eq!(editor.selection, caret);
            let screen = session.surface.snapshot();
            assert!(
                screen.contains(&format!("after draft:{expected}")),
                "{screen}"
            );
            assert!(screen.contains("previous"), "{screen}");
        }
    }

    fn click(surface: &Surface, label: &str) -> Vec<Event> {
        let row = surface
            .accessibility_snapshot()
            .into_iter()
            .find(|node| node.label == label)
            .unwrap();
        [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ]
        .into_iter()
        .map(|kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: (row.frame.origin.x.0 / 8.0) as u16,
                row: (row.frame.origin.y.0 / 16.0) as u16,
                modifiers: KeyModifiers::NONE,
            })
        })
        .collect()
    }

    fn drive_burst(session: &mut Session, events: impl FnOnce(&Surface) -> Vec<Event>) {
        let initial = session.initial_frame().unwrap();
        let events = events(&session.surface);
        let state = Arc::new(Mutex::new(super::super::input::Inbox::default()));
        let presented = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let presentation = Presentation {
            state: state.clone(),
            presented: presented.clone(),
            stop: stop.clone(),
            ..Default::default()
        };
        session.presentation = presentation.clone();
        let (commands, receive) = mpsc::sync_channel(1);
        let (frames, ready) = mpsc::sync_channel(1);
        frames.send(Ok(initial)).unwrap();
        let driver = {
            let presentation = presentation.clone();
            thread::spawn(move || {
                let mut output = super::super::output::Output::new(io::sink()).unwrap();
                output.submit(b"SETUP".to_vec(), None).unwrap();
                super::super::drive(&mut output, &presentation, &commands, &ready)
            })
        };
        let deadline = Instant::now() + std::time::Duration::from_secs(1);
        loop {
            let command = receive
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            if command.is_probe() && !session.engine.needs_pump() && !session.surface.animating() {
                // The real driver has sent an idle probe, but its UI owner
                // has not replied. Input still belongs to the visible frame.
                let mut inbox = state.lock().unwrap();
                let epoch = presented.load(std::sync::atomic::Ordering::Acquire);
                assert_eq!(epoch, session.epoch);
                for event in events {
                    inbox.push(event, epoch);
                }
                drop(inbox);
                let frame = session.advance_if_needed(command).unwrap();
                assert_eq!(frame.epoch, epoch);
                assert!(frame.bytes.is_none());
                frames.send(Ok(frame)).unwrap();
                break;
            }
            frames.send(session.advance_if_needed(command)).unwrap();
            assert!(Instant::now() < deadline, "the UI did not reach idle");
        }
        for _ in 0..4 {
            let command = receive
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            frames.send(session.advance_if_needed(command)).unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Release);
        driver.join().unwrap().unwrap();
    }

    #[test]
    fn driver_revokes_a_second_old_click_after_the_first_callback_rebinds_its_row() {
        let source = APP
            .replace(
                "requestAnimationFrame(() => rows.set([{ id: \"B\" }]));",
                "",
            )
            .replace(
                "onPress: () => selected.set(row().id),",
                "onPress: () => { selected.set(row().id); rows.set([{ id: \"B\" }]); },",
            );
        let mut session = Session::new(config(&source)).unwrap();
        drive_burst(&mut session, |surface| {
            let mut events = click(surface, "row:A");
            events.extend(click(surface, "row:A"));
            events
        });
        assert!(session.surface.snapshot().contains("selected:A"));
        assert!(
            session
                .surface
                .accessibility_snapshot()
                .iter()
                .any(|node| node.label == "row:B")
        );
        drive_burst(&mut session, |surface| click(surface, "row:B"));
        assert!(session.surface.snapshot().contains("selected:B"));
    }

    #[test]
    fn mouse_down_callback_revokes_the_queued_release_before_it_reads_a_new_row() {
        let source = APP
            .replace("requestAnimationFrame(() => rows.set([{ id: \"B\" }]));", "")
            .replace(
                "onPress: () => selected.set(row().id),",
                "onPress: () => selected.set(row().id), onPressDown: () => { selected.set('pressed:' + row().id); rows.set([{ id: \"B\" }]); },",
            );
        let mut session = Session::new(config(&source)).unwrap();
        session.initial_frame().unwrap();
        let inputs = click(&session.surface, "row:A")
            .into_iter()
            .map(|event| Input {
                event,
                pointer: Some(Pointer::Exact(session.epoch)),
            })
            .collect();
        session
            .advance(Advance {
                image_protocol: None,
                inputs,
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(session.surface.snapshot().contains("selected:pressed:A"));
        assert!(
            session
                .surface
                .accessibility_snapshot()
                .iter()
                .any(|node| node.label == "row:B")
        );
    }

    #[test]
    fn driver_accumulates_wheels_without_clicking_new_content_at_old_coordinates() {
        let mut session = Session::new(config(
            r#"
            import { mount, signal } from "nsh";
            import { element, prop, insert, append } from "nsh:rt";
            const offset = signal(0);
            const selected = signal("none");
            mount(() => {
                const root = element("view");
                prop(root, "style", { flex: 1 });
                const result = element("text");
                insert(result, () => "offset:" + offset() + " selected:" + selected());
                append(root, result);
                const scroll = element("scroll");
                prop(scroll, "style", { flex: 1 });
                prop(scroll, "onScroll", (event) => offset.set(event.offset.y));
                for (let row = 0; row < 30; row++) {
                    const node = element("view");
                    prop(node, "style", { height: 16 });
                    prop(node, "accessibilityLabel", "row:" + row);
                    prop(node, "onPress", () => selected.set(String(row)));
                    const label = element("text");
                    insert(label, "Row " + row);
                    append(node, label);
                    append(scroll, node);
                }
                append(root, scroll);
                return root;
            });
        "#,
        ))
        .unwrap();
        drive_burst(&mut session, |surface| {
            let click = click(surface, "row:0");
            let Event::Mouse(mut mouse) = click[0] else {
                unreachable!()
            };
            mouse.kind = MouseEventKind::ScrollDown;
            let mut events = vec![Event::Mouse(mouse); 3];
            events.extend(click);
            events
        });
        assert!(
            session
                .surface
                .snapshot()
                .contains("offset:144 selected:none")
        );
        drive_burst(&mut session, |surface| click(surface, "row:9"));
        assert!(session.surface.snapshot().contains("offset:144 selected:9"));
    }

    #[test]
    fn a_flushed_wheel_survives_background_work_but_not_a_keyed_reader_replacement() {
        const APP: &str = r#"
            import { mount, signal } from "nsh";
            import { element, prop, bindProp, insert, append } from "nsh:rt";
            const owners = signal([{ id: "A" }]);
            const status = signal("before");
            const observed = signal("none");
            const draft = signal("wheel draft");
            mount(() => {
                const root = element("view");
                prop(root, "style", { flex: 1 });
                const result = element("text");
                prop(result, "style", { height: 16 });
                insert(result, () => status() + " " + observed());
                append(root, result);
                insert(root, Each({
                    get of() { return owners(); },
                    key: owner => owner.id,
                    children: owner => {
                        const id = owner().id;
                        const scroll = element("scroll");
                        prop(scroll, "style", { flex: 1 });
                        prop(scroll, "accessibilityLabel", "owner:" + id);
                        prop(scroll, "contentOffset", { y: 144 });
                        prop(scroll, "onScroll", event => observed.set(id + ":" + event.offset.y));
                        for (let row = 0; row < 30; row++) {
                            const node = element("view");
                            prop(node, "style", { height: 16 });
                            prop(node, "accessibilityLabel", "row:" + id + ":" + row);
                            const label = element("text");
                            insert(label, "Row " + id + ":" + row);
                            append(node, label);
                            append(scroll, node);
                        }
                        return scroll;
                    },
                }));
                const input = element("input");
                prop(input, "style", { height: 16, flexShrink: 0 });
                prop(input, "autoFocus", true);
                bindProp(input, "value", draft);
                prop(input, "onChange", text => draft.set(text));
                append(root, input);
                return root;
            });
            requestAnimationFrame(() => {
                status.set("after");
                if (REPLACE_OWNER) owners.set([{ id: "B" }]);
            });
        "#;
        for replace_owner in [false, true] {
            let mut session = Session::new(config(
                &APP.replace("REPLACE_OWNER", &replace_owner.to_string()),
            ))
            .unwrap();
            let mut output = super::super::output::Output::new(io::sink()).unwrap();
            let presentation = session.presentation.clone();
            let initial = session.initial_frame().unwrap();
            let initial_epoch = initial.epoch;
            publish(&mut output, &presentation, initial);
            assert!(session.surface.snapshot().contains("Row A:9"));
            let editor = session.surface.input_context().unwrap();
            assert_eq!(editor.text.as_deref(), Some("wheel draft"));
            let owner_id = |surface: &Surface, owner: &str| {
                surface
                    .accessibility_snapshot()
                    .into_iter()
                    .find(|node| node.label == format!("owner:{owner}"))
                    .unwrap()
                    .id
            };
            let original_owner = owner_id(&session.surface, "A");
            let Event::Mouse(mut wheel) = click(&session.surface, "row:A:9")[0] else {
                unreachable!()
            };
            wheel.kind = MouseEventKind::ScrollUp;
            let background = session
                .advance_if_needed(Advance {
                    image_protocol: None,
                    inputs: Vec::new(),
                    resize: None,
                    cancel_pointer: false,
                })
                .unwrap();
            assert!(background.epoch > initial_epoch);
            assert!(session.surface.snapshot().contains("after "));
            assert_eq!(
                presentation
                    .presented
                    .load(std::sync::atomic::Ordering::Acquire),
                0
            );
            let owner = if replace_owner { "B" } else { "A" };
            let current_owner = owner_id(&session.surface, owner);
            if replace_owner {
                assert_ne!(current_owner, original_owner);
            } else {
                assert_eq!(current_owner, original_owner);
            }
            {
                let mut inbox = presentation.state.lock().unwrap();
                assert_eq!(inbox.last_frame.as_ref().unwrap().0, initial_epoch);
                inbox.push(Event::Mouse(wheel), 0);
            }
            // The queued wheel still names the flushed reader when newer
            // background output completes before the driver can deliver it.
            publish(&mut output, &presentation, background);
            let inputs = presentation.state.lock().unwrap().pop_batch(session.epoch);
            assert_eq!(inputs.len(), 1);
            let acted = session
                .advance(Advance {
                    image_protocol: None,
                    inputs,
                    resize: None,
                    cancel_pointer: false,
                })
                .unwrap();
            let screen = session.surface.snapshot();
            if replace_owner {
                assert!(screen.contains("after B:144"), "{screen}");
                assert!(screen.contains("Row B:9"), "{screen}");
                assert!(!screen.contains("Row B:6"), "{screen}");
            } else {
                assert!(screen.contains("after A:96"), "{screen}");
                assert!(screen.contains("Row A:6"), "{screen}");
            }
            let current_editor = session.surface.input_context().unwrap();
            assert_eq!(current_editor.node, editor.node);
            assert_eq!(current_editor.text, editor.text);
            assert_eq!(current_editor.selection, editor.selection);
            if replace_owner {
                // Rejection is specific to the old owner: a newly flushed
                // wheel reaches B and produces its real onScroll payload.
                publish(&mut output, &presentation, acted);
                let Event::Mouse(mut fresh) = click(&session.surface, "row:B:9")[0] else {
                    unreachable!()
                };
                fresh.kind = MouseEventKind::ScrollUp;
                let inputs = {
                    let mut inbox = presentation.state.lock().unwrap();
                    inbox.push(Event::Mouse(fresh), session.epoch);
                    inbox.pop_batch(session.epoch)
                };
                assert_eq!(inputs.len(), 1);
                session
                    .advance(Advance {
                        image_protocol: None,
                        inputs,
                        resize: None,
                        cancel_pointer: false,
                    })
                    .unwrap();
                let screen = session.surface.snapshot();
                assert!(screen.contains("after B:96"), "{screen}");
                assert!(screen.contains("Row B:6"), "{screen}");
            }
        }
    }

    #[test]
    fn a_callback_revokes_later_callbacks_from_the_same_physical_input() {
        let source = APP.replace(
            "onPress: () => selected.set(row().id),",
            "onPress: () => selected.set(selected() === 'none' ? 'once' : 'twice'),",
        );
        let mut session = Session::new(config(&source)).unwrap();
        session.initial_frame().unwrap();
        let row = session
            .surface
            .accessibility_snapshot()
            .into_iter()
            .find(|node| node.label == "row:A")
            .unwrap()
            .id;
        let invoked = session
            .dispatch_events(
                vec![
                    SurfaceEvent::Event(row, EventKind::Press, EventPayload::None),
                    SurfaceEvent::Event(row, EventKind::Press, EventPayload::None),
                ],
                true,
                None,
            )
            .unwrap();
        assert!(invoked);
        session
            .advance(Advance {
                image_protocol: None,
                inputs: Vec::new(),
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(session.surface.snapshot().contains("selected:once"));
    }

    #[test]
    fn mount_deltas_are_applied_in_order_including_create_and_later_value() {
        let mut session = Session::new(config("import { mount } from 'nsh'; import { element } from 'nsh:rt'; mount(() => element('view')); ")).unwrap();
        session.initial_frame().unwrap();
        session.sink.submit(MountBatch {
            items: vec![
                MountItem::Create {
                    id: 900,
                    kind: ViewKind::Input,
                },
                MountItem::Insert {
                    parent: ROOT_ID,
                    child: 900,
                    before: None,
                },
                MountItem::Frame {
                    id: 900,
                    frame: nsh_core::Bounds::new(
                        nsh_core::geometry::point(
                            nsh_core::geometry::px(0.0),
                            nsh_core::geometry::px(0.0),
                        ),
                        nsh_core::geometry::size(
                            nsh_core::geometry::px(160.0),
                            nsh_core::geometry::px(16.0),
                        ),
                    ),
                },
            ],
        });
        session.sink.submit(MountBatch {
            items: vec![MountItem::InputValue {
                id: 900,
                text: "ordered".into(),
                count: 0,
            }],
        });
        assert!(session.apply_commits().unwrap());
        assert!(session.surface.snapshot().contains("ordered"));
    }

    #[test]
    fn rejected_mouse_up_cancels_the_old_gesture_before_native_ticks_resume() {
        let source = APP.replace(
            "requestAnimationFrame(() => rows.set([{ id: \"B\" }]));",
            "",
        );
        let mut session = Session::new(config(&source)).unwrap();
        session.initial_frame().unwrap();
        let row = session
            .surface
            .accessibility_snapshot()
            .into_iter()
            .find(|node| node.label == "row:A")
            .unwrap();
        let mouse = |kind| {
            Event::Mouse(MouseEvent {
                kind,
                column: (row.frame.origin.x.0 / 8.0) as u16,
                row: (row.frame.origin.y.0 / 16.0) as u16,
                modifiers: KeyModifiers::NONE,
            })
        };
        session
            .surface
            .handle_event(mouse(MouseEventKind::Down(MouseButton::Left)));
        session.forward_events(true).unwrap();
        session
            .advance(Advance {
                image_protocol: None,
                inputs: Vec::new(),
                resize: None,
                cancel_pointer: true,
            })
            .unwrap();
        session
            .advance(Advance {
                image_protocol: None,
                inputs: vec![Input {
                    event: mouse(MouseEventKind::Up(MouseButton::Left)),
                    pointer: Some(Pointer::Exact(session.epoch)),
                }],
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(session.surface.snapshot().contains("selected:none"));
    }

    #[test]
    fn ctrl_c_copies_selected_unicode_through_the_same_output_frame() {
        let mut session = Session::new(config(
            r#"
            import { mount } from "nsh";
            import { element, prop, insert } from "nsh:rt";
            mount(() => {
                const node = element("text");
                prop(node, "selectable", true);
                insert(node, "你好A");
                return node;
            });
        "#,
        ))
        .unwrap();
        session.initial_frame().unwrap();
        let command = |key| Advance {
            image_protocol: None,
            inputs: vec![Input {
                event: Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::CONTROL)),
                pointer: None,
            }],
            resize: None,
            cancel_pointer: false,
        };
        session.advance(command('a')).unwrap();
        let copied = session.advance(command('c')).unwrap();
        assert!(!copied.quit);
        let sequence = ServiceOutput::Clipboard("你好A".into())
            .terminal_sequence(std::env::var_os("TMUX").is_some());
        assert!(
            copied
                .bytes
                .unwrap()
                .windows(sequence.len())
                .any(|bytes| bytes == sequence.as_bytes())
        );
    }

    #[test]
    fn large_copy_preserves_the_draft_and_cut_delivers_the_complete_text_once() {
        let mut session = Session::new(config(
            r#"
            import { mount, signal } from "nsh";
            import { element, prop, bindProp } from "nsh:rt";
            const value = signal("x".repeat(1048577) + "😀");
            mount(() => {
                const input = element("input");
                prop(input, "autoFocus", true);
                prop(input, "style", { height: 16 });
                bindProp(input, "value", value);
                prop(input, "onChange", (text) => value.set(text));
                return input;
            });
        "#,
        ))
        .unwrap();
        session.initial_frame().unwrap();
        let command = |key, modifiers| Advance {
            image_protocol: None,
            inputs: vec![Input {
                event: Event::Key(KeyEvent::new(KeyCode::Char(key), modifiers)),
                pointer: None,
            }],
            resize: None,
            cancel_pointer: false,
        };
        session
            .advance(command('a', KeyModifiers::CONTROL))
            .unwrap();
        let original = "x".repeat(1_048_577) + "😀";
        for key in ['c', 'x'] {
            let frame = session
                .advance(command(key, KeyModifiers::CONTROL))
                .unwrap();
            assert!(!frame.quit);
            assert_eq!(
                session.surface.input_context().unwrap().text,
                Some(if key == 'c' {
                    original.clone()
                } else {
                    String::new()
                })
            );
            let bytes = frame.bytes.unwrap();
            let sequence = ServiceOutput::Clipboard(original.clone())
                .terminal_sequence(std::env::var_os("TMUX").is_some());
            assert!(
                bytes
                    .windows(sequence.len())
                    .any(|bytes| bytes == sequence.as_bytes())
            );
        }
        session.advance(command('z', KeyModifiers::NONE)).unwrap();
        assert_eq!(
            session.surface.input_context().unwrap().text.as_deref(),
            Some("z")
        );
        session
            .advance(command('a', KeyModifiers::CONTROL))
            .unwrap();
        let cut = session
            .advance(command('x', KeyModifiers::CONTROL))
            .unwrap();
        assert_eq!(
            session.surface.input_context().unwrap().text.as_deref(),
            Some("")
        );
        let sequence = ServiceOutput::Clipboard("z".into())
            .terminal_sequence(std::env::var_os("TMUX").is_some());
        assert!(
            cut.bytes
                .unwrap()
                .windows(sequence.len())
                .any(|bytes| bytes == sequence.as_bytes())
        );
    }
}
