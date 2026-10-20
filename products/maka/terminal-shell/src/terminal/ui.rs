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

use super::{input::Input, output::FrameWriter};
use crossterm::event::Event;
use nsh_core::MountBatch;
use nsh_engine::{EngineConfig, LocalEngine, MountSink};
use nsh_platform_tui::{
    CellMetrics, ServiceOutput, ServiceRequest, Surface, SurfaceEvent, TerminalTextEngine,
};
use ratatui::{Terminal, TerminalOptions, Viewport, backend::CrosstermBackend, layout::Rect};
use std::{
    io::{self, Write},
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
    pub quit: bool,
}

pub(super) struct Advance {
    pub input: Option<Input>,
    pub resize: Option<(u16, u16)>,
    pub cancel_pointer: bool,
}

type Frames = mpsc::Receiver<Result<Frame, String>>;

pub(super) fn spawn(config: EngineConfig) -> io::Result<(mpsc::SyncSender<Advance>, Frames)> {
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
            if frames.send(session.initial_frame()).is_err() {
                return;
            }
            while let Ok(command) = commands.recv() {
                let result = session.advance(command);
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
    sink: Sink,
    terminal: Terminal<CrosstermBackend<FrameWriter>>,
    area: Rect,
    epoch: u64,
    started: Instant,
    effects: Vec<u8>,
}

impl Session {
    fn new(config: EngineConfig) -> Result<Self, String> {
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
        let sink = Sink::default();
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
            sink,
            terminal,
            area,
            epoch: 0,
            started: Instant::now(),
            effects: Vec::new(),
        })
    }

    fn initial_frame(&mut self) -> Result<Frame, String> {
        self.apply_commits()?;
        self.encode(true)
    }

    fn advance(&mut self, command: Advance) -> Result<Frame, String> {
        let mut changed = false;
        if command.cancel_pointer {
            // NSH's terminal focus-loss path cancels all pointer captures.
            // Reconcile immediately to restore the mounted input's focus.
            self.surface.handle_event(Event::FocusLost);
            self.surface.handle_event(Event::FocusGained);
            self.forward_events(false)?;
            changed = true;
        }
        if let Some((columns, rows)) = command.resize {
            self.area = viewport(columns, rows)?;
            self.surface.resize(self.area.width, self.area.height);
            self.engine.control().resize(self.surface.viewport());
            self.terminal
                .resize(self.area)
                .map_err(|error| error.to_string())?;
            changed = true;
        }
        if let Some(input) = command.input {
            if input.pointer_epoch.is_some_and(|epoch| epoch != self.epoch) {
                return Err("pointer input belongs to an obsolete presentation".into());
            }
            changed |= self.surface.handle_event(input.event);
            // Resolve the whole physical input against the presented Surface,
            // then invoke it before queued timers/Host completions can recycle
            // a cell or replace the node's handler.
            self.forward_events(true)?;
        }
        self.engine.pump();
        changed |= self.apply_commits()?;
        changed |= self.surface.tick(self.started.elapsed().as_secs_f64());
        self.forward_events(false)?;
        self.encode(changed)
    }

    fn forward_events(&mut self, presented_input: bool) -> Result<(), String> {
        let events: Vec<_> = self.surface.drain_events().collect();
        self.dispatch_events(events, presented_input)
    }

    fn dispatch_events(
        &mut self,
        events: Vec<SurfaceEvent>,
        presented_input: bool,
    ) -> Result<(), String> {
        if presented_input
            && events
                .iter()
                .filter(|event| matches!(event, SurfaceEvent::Event(..)))
                .count()
                > 1
        {
            // This first-party slice subscribes only to a row's onPress or
            // an input's onChangeText/onSubmitEditing. Multiple callbacks may
            // change a later target's meaning even inside a reactive batch;
            // reject the entire group rather than dispatching it incorrectly.
            return Err("this terminal slice does not support multiple application callbacks from one input".into());
        }
        let control = self.engine.control();
        for event in events {
            match event {
                SurfaceEvent::Event(id, event, payload) if presented_input => {
                    self.engine.dispatch_event_now(id, event, payload)
                }
                SurfaceEvent::Event(id, event, payload) => {
                    control.dispatch_event(id, event, payload)
                }
                SurfaceEvent::Scroll(id, x, y, applied) => control.scroll(id, x, y, applied),
                SurfaceEvent::States(id, states) => control.set_states(id, states),
                SurfaceEvent::AnimationsFinished(finished) => control.animations_finished(finished),
                SurfaceEvent::ComponentSize(id, size) => control.set_component_size(id, size),
                SurfaceEvent::Zoom(id, scale) => control.zoom(id, scale),
                SurfaceEvent::RootSize(id, size, insets) => control.set_root_size(id, size, insets),
                SurfaceEvent::ScreenHeader(id, height) => control.set_screen_header(id, height),
                SurfaceEvent::Service(ServiceRequest::ClipboardWrite(text)) => {
                    let sequence = ServiceOutput::Clipboard(text)
                        .terminal_sequence(std::env::var_os("TMUX").is_some());
                    if sequence.is_empty() {
                        return Err(
                            "clipboard copy exceeds the terminal's 64 KiB OSC 52 limit".into()
                        );
                    }
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
        Ok(())
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
                quit: self.surface.should_quit(),
            });
        }
        crossterm::queue!(
            self.terminal.backend_mut().writer_mut(),
            crossterm::terminal::BeginSynchronizedUpdate
        )
        .map_err(|error| error.to_string())?;
        self.terminal
            .draw(|frame| self.surface.render(self.area, frame.buffer_mut()))
            .map_err(|error| error.to_string())?;
        crossterm::queue!(
            self.terminal.backend_mut().writer_mut(),
            crossterm::terminal::EndSynchronizedUpdate
        )
        .map_err(|error| error.to_string())?;
        self.terminal
            .backend_mut()
            .writer_mut()
            .write_all(&std::mem::take(&mut self.effects))
            .map_err(|error| error.to_string())?;
        let bytes = std::mem::take(&mut self.terminal.backend_mut().writer_mut().0);
        Ok(Frame {
            epoch: self.epoch,
            bytes: Some(bytes),
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

    fn config(source: &str) -> EngineConfig {
        EngineConfig {
            app: App::Js(Bundle::source(source.to_owned())),
            viewport: CellMetrics::default().viewport(40, 8),
            trace: false,
            network: None,
            assets: None,
            environment: Default::default(),
            header_heights: (16.0, 32.0),
            limits: Limits::default(),
            ui_sets: Vec::new(),
            runtime: None,
            data_dir: None,
            storage: None,
        }
    }

    const APP: &str = r#"
        import { mount, signal } from "nsh";
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
                    const node = element("view");
                    prop(node, "style", { height: 16 });
                    bindProp(node, "accessibilityLabel", () => "row:" + row().id);
                    prop(node, "onPress", () => selected.set(row().id));
                    const label = element("text");
                    insert(label, () => row().id);
                    append(node, label);
                    return node;
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
        let row = session
            .surface
            .accessibility_snapshot()
            .into_iter()
            .find(|node| node.label == "row:A")
            .unwrap()
            .id;
        // A frame callback is queued, but VM and Surface have not advanced
        // beyond the fully presented A frame. Invoking pump first would
        // make this exact node's row accessor read B.
        session
            .dispatch_events(
                vec![SurfaceEvent::Event(
                    row,
                    EventKind::Press,
                    EventPayload::None,
                )],
                true,
            )
            .unwrap();
        session
            .advance(Advance {
                input: None,
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
    fn one_physical_event_with_multiple_business_callbacks_is_rejected_before_any_run() {
        let mut session = Session::new(config(APP)).unwrap();
        session.initial_frame().unwrap();
        let row = session
            .surface
            .accessibility_snapshot()
            .into_iter()
            .find(|node| node.label == "row:A")
            .unwrap()
            .id;
        let result = session.dispatch_events(
            vec![
                SurfaceEvent::Event(row, EventKind::Press, EventPayload::None),
                SurfaceEvent::Event(row, EventKind::Press, EventPayload::None),
            ],
            true,
        );
        assert!(
            result
                .unwrap_err()
                .contains("multiple application callbacks")
        );
        session
            .advance(Advance {
                input: None,
                resize: None,
                cancel_pointer: false,
            })
            .unwrap();
        assert!(session.surface.snapshot().contains("selected:none"));
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
                    frame: nsh_core::Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 160.0,
                        height: 16.0,
                    },
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
                column: (row.frame.x / 8.0) as u16,
                row: (row.frame.y / 16.0) as u16,
                modifiers: KeyModifiers::NONE,
            })
        };
        session
            .surface
            .handle_event(mouse(MouseEventKind::Down(MouseButton::Left)));
        session.forward_events(true).unwrap();
        session
            .advance(Advance {
                input: None,
                resize: None,
                cancel_pointer: true,
            })
            .unwrap();
        session
            .advance(Advance {
                input: Some(Input {
                    event: mouse(MouseEventKind::Up(MouseButton::Left)),
                    pointer_epoch: Some(session.epoch),
                }),
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
            input: Some(Input {
                event: Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::CONTROL)),
                pointer_epoch: None,
            }),
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
}
