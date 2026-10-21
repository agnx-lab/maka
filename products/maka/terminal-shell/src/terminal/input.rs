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

// Yield the presentation lock between read slices; this is not a queue limit.
pub(super) const READ_BATCH: usize = 128;

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
    #[cfg(target_os = "linux")]
    pub trace: Option<super::frame_trace::Trace>,
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
            self.graphics.resize();
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
                    if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
                        && let Some(Capture::Active(pointer @ Pointer::Press { lease, .. })) =
                            &self.capture
                        && (lease.is_native_pan() || lease.is_native_input())
                    {
                        Some(pointer.clone())
                    } else {
                        // Keep the cancellation in gesture order: a queued Down
                        // must not recreate its capture after a batch-level cancel.
                        match self.capture.replace(Capture::Cancelled) {
                            Some(Capture::Active(pointer)) => Some(pointer),
                            _ => return,
                        }
                    }
                }
                MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight
                    if self.capture.is_none() && self.resize.is_none() =>
                {
                    self.last_frame.as_ref().and_then(|(offered, snapshot)| {
                        (*offered > self.reject_through)
                            .then(|| snapshot.wheel_at(*mouse))
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
        if let Event::Mouse(next) = &event
            && next.kind == MouseEventKind::Moved
            && let Some(Input {
                event: Event::Mouse(previous),
                pointer: Some(Pointer::Exact(previous_epoch)),
            }) = self.events.back_mut()
            && previous.kind == MouseEventKind::Moved
            && previous.modifiers == next.modifiers
            && matches!(&pointer, Some(Pointer::Exact(epoch)) if *epoch == *previous_epoch)
        {
            previous.column = next.column;
            previous.row = next.row;
            return;
        }
        self.events.push_back(Input { event, pointer });
    }

    pub fn pop(&mut self, epoch: u64) -> Option<Input> {
        while let Some(input) = self.events.pop_front() {
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
        // The native wheel API accepts u16 steps. Keep any remainder queued.
        while inputs.len() < usize::from(u16::MAX)
            && let Some(next) = self.events.front()
        {
            if !continues_pointer(&inputs[0], inputs.last().unwrap(), next) {
                break;
            }
            let next = self.events.pop_front().unwrap();
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
    fn keyboard_bursts_keep_their_order_without_blocking_resize_or_cancel() {
        let mut inbox = Inbox::default();
        let text = "a你b界".repeat(1024);
        for character in text.chars() {
            inbox.push(key(character), 0);
        }
        inbox.push(Event::Resize(20, 4), 0);
        inbox.push(Event::Resize(80, 20), 0);
        inbox.push(
            Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            0,
        );
        assert_eq!(inbox.resize, Some((80, 20)));
        assert!(inbox.quit);
        assert!(inbox.error.is_none());
        for character in text.chars() {
            assert_eq!(inbox.pop(0).unwrap().event, key(character));
        }
        assert!(inbox.pop(0).is_none());
    }

    #[test]
    fn complete_large_paste_keeps_literal_unicode_in_one_ordered_event() {
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
        let text = format!("PASTE START\n{}PASTE END", "保留😀z\n".repeat(100_000));
        assert!(text.len() > 1024 * 1024);
        inbox.push(key('a'), 0);
        inbox.push(Event::Paste(text.clone()), 0);
        inbox.push(key('b'), 0);
        assert!(
            inbox.error.is_none(),
            "complete paste must not fail input admission"
        );
        assert!(
            matches!(inbox.pop(0).unwrap().event,Event::Key(value) if value.code == KeyCode::Char('a'))
        );
        assert!(
            matches!(inbox.pop(0).unwrap().event,Event::Paste(value) if value == text),
            "one original paste must retain all bytes"
        );
        assert!(
            matches!(inbox.pop(0).unwrap().event,Event::Key(value) if value.code == KeyCode::Char('b'))
        );
        assert!(inbox.pop(0).is_none());
    }

    #[test]
    fn hover_bursts_coalesce_without_crossing_input_or_frame_boundaries() {
        let mut inbox = Inbox::default();
        let moved = |column, modifiers| {
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Moved,
                column,
                row: 1,
                modifiers,
            })
        };
        for column in 1..=4096 {
            inbox.push(moved(column, KeyModifiers::NONE), 7);
        }
        let barriers = [
            mouse(),
            pointer(MouseEventKind::Drag(MouseButton::Left)),
            pointer(MouseEventKind::Up(MouseButton::Left)),
            pointer(MouseEventKind::ScrollDown),
            key('你'),
            Event::Paste("中文".into()),
        ];
        let mut expected = vec![moved(4096, KeyModifiers::NONE)];
        for barrier in barriers {
            inbox.push(barrier.clone(), 7);
            inbox.push(moved(5, KeyModifiers::NONE), 7);
            inbox.push(moved(6, KeyModifiers::NONE), 7);
            expected.extend([barrier, moved(6, KeyModifiers::NONE)]);
        }
        inbox.push(moved(7, KeyModifiers::SHIFT), 7);
        expected.push(moved(7, KeyModifiers::SHIFT));
        for event in expected {
            assert_eq!(inbox.pop(7).unwrap().event, event);
        }
        assert!(inbox.pop(7).is_none());
        inbox.push(moved(1, KeyModifiers::NONE), 7);
        inbox.push(moved(2, KeyModifiers::NONE), 8);
        assert_eq!(inbox.pop(7).unwrap().event, moved(1, KeyModifiers::NONE));
        assert_eq!(inbox.pop(8).unwrap().event, moved(2, KeyModifiers::NONE));
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
    fn wheel_bursts_split_at_the_native_steps_width_without_losing_input() {
        let mut inbox = Inbox::default();
        for _ in 0..65536 {
            inbox.push(pointer(MouseEventKind::ScrollDown), 7);
        }
        inbox.push(mouse(), 7);
        assert_eq!(inbox.pop_batch(7).len(), 65535);
        assert_eq!(inbox.pop_batch(7).len(), 1);
        assert_eq!(inbox.pop(7).unwrap().event, mouse());
        let text = "z".repeat(256 * 1024);
        inbox.push(Event::Paste(text.clone()), 8);
        assert_eq!(
            inbox
                .pop_batch(8)
                .into_iter()
                .map(|input| input.event)
                .collect::<Vec<_>>(),
            vec![Event::Paste(text)]
        );
    }

    #[test]
    fn native_wheels_keep_their_owner_and_actions_and_resize_revoke_them() {
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
        for _ in 0..128 {
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
        inbox.push(Event::Paste("z".repeat(256 * 1024)), 8);
        assert!(
            matches!(inbox.pop(8).unwrap().event, Event::Paste(text) if text.len() == 256 * 1024)
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

    #[test]
    fn native_value_gestures_keep_the_original_capture_across_new_presentations() {
        use nsh_core::{
            Bounds, GestureValues, MountBatch, MountItem, Point, ROOT_ID, Size, ViewKind,
            animation::AnimationOp, geometry::px,
        };
        use nsh_platform_tui::{CellMetrics, Surface, SurfaceEvent};

        let mut surface = Surface::new(40, 12, CellMetrics::default());
        surface.apply(MountBatch {
            items: vec![
                MountItem::Create {
                    id: 2,
                    kind: ViewKind::View,
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
                MountItem::Animation(AnimationOp::CreateValue {
                    node: 100,
                    value: 0.0,
                }),
                MountItem::Animation(AnimationOp::CreateValue {
                    node: 102,
                    value: 1.0,
                }),
                MountItem::Gestures {
                    id: 2,
                    values: GestureValues {
                        pan_x: Some(100),
                        pinch: Some(102),
                        ..Default::default()
                    },
                },
            ],
        });
        surface.drain_events().for_each(drop);
        let mut inbox = Inbox {
            last_frame: Some((7, surface.input_snapshot())),
            ..Default::default()
        };
        inbox.push(mouse(), 0);
        let drag = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 3,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        inbox.push(drag, 8);
        inbox.push(pointer(MouseEventKind::Up(MouseButton::Left)), 8);
        let batch = inbox.pop_batch(8);
        assert_eq!(batch.len(), 3);
        let mut capture = None;
        for input in batch {
            let Some(Pointer::Press { epoch: 7, lease }) = input.pointer else {
                panic!("every phase must keep the original native capture")
            };
            assert!(lease.is_native_pan());
            assert_eq!(lease.binding(), None);
            if let Some(original) = &capture {
                assert!(Arc::ptr_eq(original, &lease));
            } else {
                capture = Some(lease.clone());
            }
            let Event::Mouse(mouse) = input.event else {
                unreachable!()
            };
            assert!(surface.handle_presented_mouse(mouse, &lease).is_some());
        }
        assert!(!inbox.cancel_pointer);
        let zoom = Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::CONTROL,
        });
        inbox.push(zoom, 8);
        let input = inbox.pop(8).unwrap();
        let Some(Pointer::Wheel { epoch: 7, lease }) = input.pointer else {
            panic!("Ctrl+wheel must retain its original native pinch owner")
        };
        let Event::Mouse(mouse) = input.event else {
            unreachable!()
        };
        assert_eq!(surface.handle_presented_wheel(mouse, 1, &lease), Some(1));
        assert!(
            !surface
                .drain_events()
                .any(|event| matches!(event, SurfaceEvent::Event(..)))
        );
    }
}
