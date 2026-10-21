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

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use nsh_platform_tui::{InputSnapshot, PressLease, WheelLease};
use std::{collections::VecDeque, sync::Arc};

pub(super) const MAX_EVENTS: usize = 128;
pub(super) const MAX_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(super) struct Input {
    pub event: Event,
    /// Keys follow the focused editor. Pointers use the exact frame, a sealed
    /// press action, or the native scroll owner in the last completed output.
    pub pointer: Option<Pointer>,
}

#[derive(Clone, Debug)]
pub(super) enum Pointer {
    Exact(u64),
    Press { epoch: u64, lease: Arc<PressLease> },
    Wheel { epoch: u64, lease: Arc<WheelLease> },
}

impl Pointer {
    fn epoch(&self) -> u64 {
        match self {
            Self::Exact(epoch) | Self::Press { epoch, .. } | Self::Wheel { epoch, .. } => *epoch,
        }
    }
}

#[derive(Clone)]
enum Capture {
    Active(Pointer),
    Cancelled,
}

#[derive(Default)]
pub(super) struct Inbox {
    events: VecDeque<Input>,
    bytes: usize,
    pub resize: Option<(u16, u16)>,
    pub graphics: super::graphics::Probe,
    pub cancel_pointer: bool,
    pub error: Option<String>,
    pub quit: bool,
    /// The sole output owner replaces this only after a successful flush.
    pub last_frame: Option<(u64, InputSnapshot)>,
    /// A physical input entered application code. Every earlier offered
    /// action is revoked, including offers older than the UI's current frame.
    pub reject_through: u64,
    capture: Option<Capture>,
    #[cfg(target_os = "linux")]
    pub pixel_probe: super::pixels::Probe,
    #[cfg(target_os = "linux")]
    pub pixels: Option<super::pixels::Inbox>,
}

impl Inbox {
    pub fn has_pending(&self) -> bool {
        !self.events.is_empty()
            || self.resize.is_some()
            || self.cancel_pointer
            || self.error.is_some()
            || self.quit
    }

    pub fn push(&mut self, event: Event, epoch: u64) {
        if let Event::Key(key) = event
            && key.kind != KeyEventKind::Release
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('q')
        {
            self.quit = true;
            return;
        }
        if let Event::Resize(columns, rows) = event {
            self.graphics.cancel();
            self.resize = Some((columns, rows));
            self.cancel_pointer = true;
            if let Some((epoch, _)) = self.last_frame.take() {
                self.reject_through = self.reject_through.max(epoch);
            }
            self.capture = None;
            return;
        }
        if self.error.is_some() {
            return;
        }
        let pointer = if let Event::Mouse(mouse) = &event {
            let captured = match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    self.capture = None;
                    if mouse.modifiers.is_empty() && self.resize.is_none() {
                        self.capture = self.last_frame.as_ref().and_then(|(offered, snapshot)| {
                            (*offered > self.reject_through)
                                .then(|| snapshot.press_at(mouse.column, mouse.row))
                                .flatten()
                                .map(|lease| {
                                    Capture::Active(Pointer::Press {
                                        epoch: *offered,
                                        lease: Arc::new(lease),
                                    })
                                })
                        });
                    }
                    match &self.capture {
                        Some(Capture::Active(pointer)) => Some(pointer.clone()),
                        _ => None,
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => match self.capture.take() {
                    Some(Capture::Active(pointer)) => Some(pointer),
                    Some(Capture::Cancelled) => {
                        self.cancel_pointer = true;
                        return;
                    }
                    None => None,
                },
                MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Down(_)
                    if self.capture.is_some() =>
                {
                    // Keep the cancellation in gesture order: a queued Down
                    // must not recreate its capture after a batch-level cancel.
                    match self.capture.replace(Capture::Cancelled) {
                        Some(Capture::Active(pointer)) => Some(pointer),
                        _ => return,
                    }
                }
                MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight
                    if !mouse.modifiers.contains(KeyModifiers::CONTROL)
                        && self.capture.is_none()
                        && self.resize.is_none() =>
                {
                    self.last_frame.as_ref().and_then(|(offered, snapshot)| {
                        (*offered > self.reject_through)
                            .then(|| snapshot.wheel_at(mouse.column, mouse.row))
                            .flatten()
                            .map(|lease| Pointer::Wheel {
                                epoch: *offered,
                                lease: Arc::new(lease),
                            })
                    })
                }
                _ => None,
            };
            if let Some(captured) = captured {
                if captured.epoch() <= self.reject_through || self.resize.is_some() {
                    self.cancel_pointer = true;
                    return;
                }
                Some(captured)
            } else if epoch == 0 || self.resize.is_some() {
                self.cancel_pointer = true;
                return;
            } else {
                Some(Pointer::Exact(epoch))
            }
        } else {
            None
        };
        let bytes = event_bytes(&event);
        if self.events.len() == MAX_EVENTS || bytes > MAX_BYTES.saturating_sub(self.bytes) {
            // Explicitly fail the input stream. Continuing with a silently
            // truncated paste or missing keystroke could submit another command.
            self.error =
                Some("terminal input exceeded its pending budget; input was rejected".into());
            return;
        }
        self.bytes += bytes;
        self.events.push_back(Input { event, pointer });
    }

    pub fn pop(&mut self, epoch: u64) -> Option<Input> {
        while let Some(input) = self.events.pop_front() {
            self.bytes -= event_bytes(&input.event);
            let stale = match &input.pointer {
                Some(Pointer::Exact(offered)) => *offered != epoch,
                Some(Pointer::Press { epoch, .. } | Pointer::Wheel { epoch, .. }) => {
                    *epoch <= self.reject_through
                }
                None => false,
            };
            if stale {
                self.cancel_pointer = true;
                continue;
            }
            return Some(input);
        }
        None
    }

    pub fn pop_batch(&mut self, epoch: u64) -> Vec<Input> {
        let Some(first) = self.pop(epoch) else {
            return Vec::new();
        };
        let mut inputs = vec![first];
        while let Some(next) = self.events.front() {
            if !continues_pointer(&inputs[0], inputs.last().unwrap(), next) {
                break;
            }
            let next = self.events.pop_front().unwrap();
            self.bytes -= event_bytes(&next.event);
            inputs.push(next);
        }
        inputs
    }
}

fn continues_pointer(first: &Input, last: &Input, next: &Input) -> bool {
    let Some(first_epoch) = first.pointer.as_ref().map(Pointer::epoch) else {
        return false;
    };
    if next.pointer.as_ref().map(Pointer::epoch) != Some(first_epoch) {
        return false;
    }
    let (Event::Mouse(first), Event::Mouse(last), Event::Mouse(next)) =
        (&first.event, &last.event, &next.event)
    else {
        return false;
    };
    match first.kind {
        MouseEventKind::Down(button) | MouseEventKind::Drag(button) => {
            !matches!(last.kind, MouseEventKind::Up(_))
                && matches!(next.kind, MouseEventKind::Drag(next) | MouseEventKind::Up(next) if next == button)
        }
        MouseEventKind::ScrollUp
        | MouseEventKind::ScrollDown
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => first == next,
        _ => false,
    }
}

fn event_bytes(event: &Event) -> usize {
    match event {
        Event::Paste(text) => text.len(),
        _ => std::mem::size_of::<Event>(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};

    fn key(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }
    fn mouse() -> Event {
        pointer(MouseEventKind::Down(MouseButton::Left))
    }
    fn pointer(kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn keyboard_is_bounded_without_blocking_resize_or_cancel() {
        let mut inbox = Inbox::default();
        for _ in 0..MAX_EVENTS {
            inbox.push(key('a'), 0);
        }
        inbox.push(key('b'), 0);
        assert!(inbox.error.as_ref().unwrap().contains("rejected"));
        inbox.push(Event::Resize(20, 4), 0);
        inbox.push(Event::Resize(80, 20), 0);
        inbox.push(
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            0,
        );
        assert_eq!(inbox.resize, Some((80, 20)));
        assert!(inbox.quit);
        assert_eq!(inbox.events.len(), MAX_EVENTS);
    }

    #[test]
    fn paste_has_a_byte_limit_and_keeps_literal_unicode_in_one_event() {
        let mut inbox = Inbox::default();
        inbox.push(
            Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            0,
        );
        assert!(
            !inbox.quit,
            "Ctrl+C belongs to the surface's selection policy"
        );
        assert!(matches!(inbox.pop(0).unwrap().event, Event::Key(_)));
        inbox.push(Event::Paste("你好\nworld".into()), 0);
        assert!(matches!(inbox.pop(0).unwrap().event, Event::Paste(text) if text == "你好\nworld"));
        inbox.push(Event::Paste("x".repeat(MAX_BYTES + 1)), 0);
        assert!(inbox.error.is_some());
        assert!(inbox.events.is_empty());
    }

    #[test]
    fn pointer_requires_the_same_fully_presented_frame_and_resize_revokes_it() {
        let mut inbox = Inbox::default();
        inbox.push(mouse(), 7);
        assert!(inbox.pop(8).is_none());
        inbox.push(mouse(), 0);
        assert!(inbox.events.is_empty());
        inbox.push(mouse(), 8);
        inbox.push(key('x'), 8);
        inbox.push(Event::Resize(40, 10), 8);
        inbox.push(mouse(), 8);
        // The resize turn revokes pointers that were queued even before
        // Resize, while preserving keyboard order and literal text.
        assert!(
            matches!(inbox.pop(0).unwrap().event, Event::Key(key) if key.code == KeyCode::Char('x'))
        );
        assert!(inbox.events.is_empty());
        assert!(inbox.cancel_pointer);
    }

    #[test]
    fn groups_one_captured_gesture_without_consuming_the_next_key_or_click() {
        let mut inbox = Inbox::default();
        let down = mouse();
        let drag = pointer(MouseEventKind::Drag(MouseButton::Left));
        let up = pointer(MouseEventKind::Up(MouseButton::Left));
        for event in [down.clone(), drag.clone(), up.clone(), down, key('x')] {
            inbox.push(event, 7);
        }
        assert_eq!(
            inbox
                .pop_batch(7)
                .into_iter()
                .map(|input| input.event)
                .collect::<Vec<_>>(),
            vec![mouse(), drag, up]
        );
        // The callback may replace the row after the first gesture. The
        // second click keeps its old authority and cannot target the new row.
        assert_eq!(
            inbox
                .pop_batch(8)
                .into_iter()
                .map(|input| input.event)
                .collect::<Vec<_>>(),
            vec![key('x')]
        );
        assert!(inbox.cancel_pointer);
    }

    #[test]
    fn wheel_group_stops_before_old_coordinates_can_click_the_scrolled_content() {
        let mut inbox = Inbox::default();
        let wheel = pointer(MouseEventKind::ScrollDown);
        for event in [wheel.clone(), wheel.clone(), mouse(), key('a'), key('b')] {
            inbox.push(event, 7);
        }
        assert_eq!(
            inbox
                .pop_batch(7)
                .into_iter()
                .map(|input| input.event)
                .collect::<Vec<_>>(),
            vec![wheel.clone(), wheel]
        );
        for expected in [key('a'), key('b')] {
            assert_eq!(
                inbox
                    .pop_batch(8)
                    .into_iter()
                    .map(|input| input.event)
                    .collect::<Vec<_>>(),
                vec![expected]
            );
        }
        assert!(inbox.cancel_pointer);
    }

    #[test]
    fn wheel_group_reuses_the_existing_budget_and_releases_its_bytes() {
        let mut inbox = Inbox::default();
        for _ in 0..MAX_EVENTS {
            inbox.push(pointer(MouseEventKind::ScrollDown), 7);
        }
        assert_eq!(inbox.pop_batch(7).len(), 128);
        inbox.push(Event::Paste("z".repeat(MAX_BYTES)), 8);
        assert_eq!(
            inbox
                .pop_batch(8)
                .into_iter()
                .map(|input| input.event)
                .collect::<Vec<_>>(),
            vec![Event::Paste("z".repeat(MAX_BYTES))]
        );
    }

    #[test]
    fn native_wheels_keep_the_queue_bound_and_actions_and_resize_revoke_them() {
        use nsh_core::{
            Bounds, MountBatch, MountItem, Point, ROOT_ID, Size, ViewKind, geometry::px,
        };
        use nsh_platform_tui::{CellMetrics, Surface, SurfaceEvent};
        let mut surface = Surface::new(40, 12, CellMetrics::default());
        surface.apply(MountBatch {
            items: vec![
                MountItem::Create {
                    id: 2,
                    kind: ViewKind::Scroll,
                },
                MountItem::Frame {
                    id: 2,
                    frame: Bounds::new(
                        Point::new(px(0.0), px(0.0)),
                        Size::new(px(160.0), px(96.0)),
                    ),
                },
                MountItem::Insert {
                    parent: ROOT_ID,
                    child: 2,
                    before: None,
                },
                MountItem::ScrollContent {
                    id: 2,
                    size: Size::new(px(160.0), px(10000.0)),
                },
            ],
        });
        surface.drain_events().for_each(drop);
        let mut inbox = Inbox {
            last_frame: Some((7, surface.input_snapshot())),
            ..Default::default()
        };
        for _ in 0..MAX_EVENTS {
            inbox.push(pointer(MouseEventKind::ScrollDown), 0);
        }
        let inputs = inbox.pop_batch(8);
        assert_eq!(inputs.len(), 128);
        let Some(Pointer::Wheel { lease, .. }) = &inputs[0].pointer else {
            panic!("native scroll lease required")
        };
        let Event::Mouse(mouse) = inputs[0].event else {
            unreachable!()
        };
        assert_eq!(
            surface.handle_presented_wheel(mouse, inputs.len() as u16, lease),
            Some(128)
        );
        let offsets: Vec<_> = surface
            .drain_events()
            .filter_map(|event| match event {
                SurfaceEvent::Scroll(id, offset, _) => Some((id, offset.y)),
                _ => None,
            })
            .collect();
        assert_eq!(offsets, [(2, px(6144.0))]);
        inbox.push(Event::Paste("z".repeat(MAX_BYTES)), 8);
        assert!(
            matches!(inbox.pop(8).unwrap().event, Event::Paste(text) if text.len() == MAX_BYTES)
        );
        inbox.push(pointer(MouseEventKind::ScrollUp), 0);
        inbox.reject_through = 7;
        assert!(
            inbox.pop(8).is_none(),
            "an earlier application action revokes the old owner"
        );
        inbox.last_frame = Some((9, surface.input_snapshot()));
        inbox.push(pointer(MouseEventKind::ScrollUp), 0);
        inbox.push(Event::Resize(41, 12), 0);
        assert!(
            inbox.pop(0).is_none(),
            "resize revokes even an already queued wheel lease"
        );
        assert!(inbox.last_frame.is_none());
        assert_eq!(inbox.resize, Some((41, 12)));
    }
}
