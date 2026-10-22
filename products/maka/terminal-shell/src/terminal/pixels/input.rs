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

use super::probe::{Capabilities, Protocol, SixelGeometry};
use nsh_gpui_terminal::{FrameToken, SubmittedFrame};
use std::{collections::VecDeque, time::Instant};
use termina::{
    Event, WindowSize,
    event::{KeyCode, KeyEventKind, Modifiers, MouseEventKind},
};

pub(super) struct Input {
    pub event: Event,
    pub geometry: u64,
    pub queued_at: Option<Instant>,
}

#[derive(Clone)]
pub(super) struct Offer {
    pub token: FrameToken,
    pub geometry: u64,
}

pub(super) struct Candidate {
    pub frame: SubmittedFrame,
    pub geometry: u64,
    /// Present only for the existing opt-in frame trace.
    pub pump_started: Option<Instant>,
}

pub(super) enum WireFrame {
    Kitty(nsh_gpui_terminal::kitty::KittyFrame),
    Inline(nsh_gpui_terminal::inline::InlineFrame),
}

impl WireFrame {
    pub fn token(&self) -> FrameToken {
        match self {
            Self::Kitty(frame) => frame.token(),
            Self::Inline(frame) => frame.token(),
        }
    }
}

pub(super) struct Encoded {
    pub frame: WireFrame,
    pub geometry: u64,
}

/// One admitted draw keeps its capacity through readback, codec and FIFO wait.
/// Resize can discard completed work, but cannot cancel a running codec.
struct FrameJob {
    geometry: u64,
    work: FrameWork,
}

enum FrameWork {
    Reserved,
    Submitted(Candidate),
    Encoding(FrameToken),
    Ready(Encoded),
}

impl FrameWork {
    fn token(&self) -> Option<FrameToken> {
        match self {
            Self::Reserved => None,
            Self::Submitted(candidate) => Some(candidate.frame.token()),
            Self::Encoding(token) => Some(token.clone()),
            Self::Ready(encoded) => Some(encoded.frame.token()),
        }
    }
}

/// Send-only physical input and output state; native contacts stay on the UI owner.
pub(crate) struct Inbox {
    pub capabilities: Capabilities,
    events: VecDeque<Input>,
    tracing: bool,
    /// Last current-viewport writer receipt, used only by output diagnostics.
    pub published: Option<FrameToken>,
    frames: VecDeque<FrameJob>,
    frame_capacity: usize,
    /// Last image whose entire upload-and-retirement transaction was flushed.
    pub last_fully_written_image: Option<u32>,
    /// The terminal geometry of the last whole image transaction's flush.
    pub last_fully_written_geometry: Option<u64>,
    /// Sole writer transaction, retained until the native owner consumes its receipt.
    pub in_flight: Option<FrameToken>,
    pub(super) ready: Option<Offer>,
    pub geometry: u64,
    pub resize: Option<WindowSize>,
    pub cancel_pointer: bool,
    pub wake: Option<std::thread::Thread>,
    pub effects: Option<Vec<u8>>,
    pub restore_sixel_mode: bool,
}

impl Inbox {
    pub fn new(capabilities: Capabilities, tracing: bool) -> Self {
        Self {
            capabilities,
            events: VecDeque::new(),
            tracing,
            published: None,
            frames: VecDeque::new(),
            // A five-process whole-frame codec scout exceeded 120 encodes/s.
            // Keep that finite candidate width; GUI/writer throughput is separate.
            // Smaller hosts use the same hardware width as the native executor.
            // Capacity gates GPU draws; it never rejects input or application data.
            frame_capacity: std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1)
                .saturating_sub(1)
                .clamp(1, 5),
            last_fully_written_image: None,
            last_fully_written_geometry: None,
            in_flight: None,
            ready: None,
            geometry: 1,
            resize: None,
            cancel_pointer: false,
            wake: None,
            effects: None,
            restore_sixel_mode: false,
        }
    }

    /// Admission is checked before drawing, so the native dirty bit coalesces
    /// changes while these finite readback/codec/result slots are occupied.
    pub(super) fn can_submit(&self) -> bool {
        let parallel = self.capabilities.protocol == Some(Protocol::Sixel);
        let capacity = if parallel { self.frame_capacity } else { 1 };
        if self.resize.is_some()
            || self.capabilities.viewport().is_none()
            || self.frames.len() >= capacity
            || (!parallel && self.in_flight.is_some())
        {
            return false;
        }
        // The first complete flush establishes opaque replacement eligibility
        // for this geometry. Do not encode a speculative initial parallel burst.
        self.last_fully_written_geometry == Some(self.geometry)
            || (self.in_flight.is_none()
                && !self.frames.iter().any(|job| job.geometry == self.geometry))
    }

    pub(super) fn reserve_frame(&mut self) -> bool {
        if !self.can_submit() {
            return false;
        }
        self.frames.push_back(FrameJob {
            geometry: self.geometry,
            work: FrameWork::Reserved,
        });
        true
    }

    pub(super) fn submitted(&mut self, candidate: Candidate) -> bool {
        let Some(job) = self.frames.iter_mut().find(|job| {
            job.geometry == candidate.geometry && matches!(job.work, FrameWork::Reserved)
        }) else {
            return false;
        };
        // A resize may arrive while draw submits. Keep that old readback's
        // reservation until its worker returns, just like an active codec.
        job.work = FrameWork::Submitted(candidate);
        true
    }

    pub(super) fn release_reservation(&mut self) {
        // A pump can produce no draw, or fail before producing a ticket.
        self.frames
            .retain(|job| !matches!(job.work, FrameWork::Reserved));
    }

    pub(super) fn oldest_frame(&self) -> Option<u64> {
        self.frames
            .front()?
            .work
            .token()
            .map(|token| token.sequence())
    }

    pub(super) fn take_candidate(&mut self) -> Option<Candidate> {
        let job = self
            .frames
            .iter_mut()
            .find(|job| matches!(job.work, FrameWork::Submitted(_)))?;
        let token = job.work.token().expect("submitted native frame");
        let FrameWork::Submitted(candidate) =
            std::mem::replace(&mut job.work, FrameWork::Encoding(token))
        else {
            unreachable!()
        };
        Some(candidate)
    }

    /// A worker can only finish its own immutable token. Stale and cancelled
    /// jobs release capacity here, after their readback/codec actually returned.
    pub(super) fn finished(&mut self, token: &FrameToken, encoded: Option<Encoded>) {
        let Some(index) = self
            .frames
            .iter()
            .position(|job| matches!(&job.work, FrameWork::Encoding(active) if active == token))
        else {
            return;
        };
        if let Some(encoded) = encoded.filter(|encoded| encoded.geometry == self.geometry) {
            self.frames[index].work = FrameWork::Ready(encoded);
        } else {
            self.frames.remove(index);
        }
    }

    /// Completion order cannot reorder terminal transactions. The native owner
    /// must consume the previous writer receipt before another handoff.
    pub(super) fn take_encoded(&mut self) -> Option<Encoded> {
        if self.in_flight.is_some()
            || !self
                .frames
                .front()
                .is_some_and(|job| matches!(job.work, FrameWork::Ready(_)))
        {
            return None;
        }
        let FrameWork::Ready(encoded) = self.frames.pop_front()?.work else {
            unreachable!()
        };
        self.in_flight = Some(encoded.frame.token());
        Some(encoded)
    }

    pub(super) fn presented(&mut self, token: &FrameToken) {
        if self.in_flight.as_ref() == Some(token) {
            self.in_flight = None;
        }
    }

    pub fn push(&mut self, event: Event) -> Result<bool, String> {
        if let Event::Key(key) = &event
            && key.kind != KeyEventKind::Release
            && key.modifiers.contains(Modifiers::CONTROL)
            && key.code == KeyCode::Char('q')
        {
            return Ok(true);
        }
        if let Event::WindowResized(size) = event {
            self.capabilities.size = size;
            self.invalidate_geometry()?;
            return Ok(false);
        }
        if matches!(
            event,
            Event::Csi(_)
                | Event::Osc(_)
                | Event::Dcs(_)
                | Event::Iterm2Capabilities(_)
                | Event::KittyGraphicsResponse(_)
        ) {
            return Ok(false);
        }
        if matches!(event, Event::MouseLeave | Event::FocusOut) {
            self.lose_pointer();
        }
        let mouse = match &event {
            Event::SgrMouse(mouse) => Some(mouse.kind),
            Event::Mouse(mouse) => Some(mouse.kind),
            _ => None,
        };
        if let Some(kind) = mouse
            && !super::mapping::mouse_position(&event, self.capabilities)
                .is_some_and(|position| self.capabilities.contains(position))
        {
            match kind {
                MouseEventKind::Down(_)
                | MouseEventKind::ScrollUp
                | MouseEventKind::ScrollDown
                | MouseEventKind::ScrollLeft
                | MouseEventKind::ScrollRight => return Ok(false),
                MouseEventKind::Moved => {
                    // Leaving the native canvas ends the current contact just
                    // like a physical leave, independently of output progress.
                    self.lose_pointer();
                    return Ok(false);
                }
                MouseEventKind::Drag(_) | MouseEventKind::Up(_) => {}
            }
        }
        if matches!(event, Event::MouseLeave) {
            return Ok(false);
        }
        self.events.push_back(Input {
            event,
            geometry: self.geometry,
            queued_at: self.tracing.then(Instant::now),
        });
        Ok(false)
    }

    pub fn set_sixel_geometry(&mut self, geometry: SixelGeometry) -> Result<(), String> {
        if self.capabilities.sixel_geometry == geometry {
            return Ok(());
        }
        let viewport = self.capabilities.viewport();
        self.capabilities.sixel_geometry = geometry;
        if viewport != self.capabilities.viewport() {
            self.invalidate_geometry()?;
        }
        Ok(())
    }

    /// Records a complete writer receipt, independently of whether resize has
    /// changed this frame's viewport. Only the native worker can publish.
    pub(super) fn written(&mut self, offer: Offer, image: Option<u32>) {
        self.last_fully_written_geometry = Some(offer.geometry);
        if let Some(image) = image {
            self.last_fully_written_image = Some(image);
        }
        self.ready = Some(offer);
        if let Some(wake) = &self.wake {
            wake.unpark();
        }
    }

    fn invalidate_geometry(&mut self) -> Result<(), String> {
        self.geometry = self
            .geometry
            .checked_add(1)
            .ok_or("terminal geometry generation exhausted")?;
        self.resize = Some(self.capabilities.size);
        self.published = None;
        // A submitted readback or codec still consumes its original slot.
        // Completed stale wire is safe to retire immediately.
        self.frames
            .retain(|job| !matches!(job.work, FrameWork::Ready(_)));
        self.lose_pointer();
        Ok(())
    }

    fn lose_pointer(&mut self) {
        self.cancel_pointer = true;
        self.events.retain(|input| !pointer(&input.event));
    }

    pub(super) fn has_pending_input(&self) -> bool {
        !self.events.is_empty() && self.resize.is_none()
    }

    pub(super) fn pop(&mut self) -> Option<Input> {
        if !self.has_pending_input() {
            return None;
        }
        self.events.pop_front()
    }

    pub(super) fn queue_len(&self) -> usize {
        self.events.len()
    }

    pub fn into_startup(self) -> (Vec<Event>, Option<WindowSize>) {
        (
            self.events.into_iter().map(|input| input.event).collect(),
            self.resize,
        )
    }
}

/// Classification only: diagnostics retain no typed characters or pasted text.
pub(super) fn trace_kind(event: &Event) -> &'static str {
    let mouse = match event {
        Event::SgrMouse(mouse) => Some(mouse.kind),
        Event::Mouse(mouse) => Some(mouse.kind),
        _ => None,
    };
    if let Some(kind) = mouse {
        return match kind {
            MouseEventKind::Down(_) => "mouse_down",
            MouseEventKind::Up(_) => "mouse_up",
            MouseEventKind::Drag(_) => "mouse_drag",
            MouseEventKind::Moved => "mouse_move",
            MouseEventKind::ScrollUp => "wheel_up",
            MouseEventKind::ScrollDown => "wheel_down",
            MouseEventKind::ScrollLeft => "wheel_left",
            MouseEventKind::ScrollRight => "wheel_right",
        };
    }
    match event {
        Event::Key(key) => match key.kind {
            KeyEventKind::Press => "key_press",
            KeyEventKind::Repeat => "key_repeat",
            KeyEventKind::Release => "key_release",
        },
        Event::Paste(_) => "paste",
        Event::FocusIn => "focus_in",
        Event::FocusOut => "focus_out",
        _ => "other",
    }
}

pub(super) fn pointer(event: &Event) -> bool {
    matches!(
        event,
        Event::SgrMouse(_) | Event::Mouse(_) | Event::MouseLeave
    )
}

pub(super) fn released(event: &Event) -> bool {
    match event {
        Event::SgrMouse(mouse) => matches!(mouse.kind, MouseEventKind::Up(_)),
        Event::Mouse(mouse) => matches!(mouse.kind, MouseEventKind::Up(_)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;
    use termina::event::KeyEvent;

    fn maximum(width: u32, height: u32) -> SixelGeometry {
        SixelGeometry::Finite {
            width: NonZeroU32::new(width).unwrap(),
            height: NonZeroU32::new(height).unwrap(),
        }
    }

    fn capabilities() -> Capabilities {
        Capabilities {
            size: WindowSize {
                cols: 120,
                rows: 40,
                pixel_width: Some(1200),
                pixel_height: Some(800),
            },
            mouse: super::super::probe::MouseCoordinates::Cells,
            protocol: Some(super::super::probe::Protocol::Sixel),
            synchronized: false,
            sixel_geometry: SixelGeometry::Unknown,
        }
    }

    fn encoded(geometry: u64) -> Encoded {
        Encoded {
            frame: WireFrame::Inline(Default::default()),
            geometry,
        }
    }

    #[test]
    fn completed_tail_waits_for_its_predecessor_and_the_native_writer_receipt() {
        let mut inbox = Inbox::new(capabilities(), false);
        let token = FrameToken::default();
        inbox.frames.push_back(FrameJob {
            geometry: 1,
            work: FrameWork::Encoding(token.clone()),
        });
        inbox.frames.push_back(FrameJob {
            geometry: 1,
            work: FrameWork::Ready(encoded(1)),
        });
        assert!(
            inbox.take_encoded().is_none(),
            "the completed tail cannot pass the head"
        );
        assert_eq!(inbox.frames.len(), 2);
        inbox.finished(&token, Some(encoded(1)));
        let first = inbox.take_encoded().unwrap();
        assert_eq!(inbox.frames.len(), 1);
        assert!(inbox.take_encoded().is_none());
        inbox.written(
            Offer {
                token: first.frame.token(),
                geometry: 1,
            },
            None,
        );
        assert!(
            inbox.take_encoded().is_none(),
            "a flush cannot overwrite an unconsumed receipt"
        );
        let receipt = inbox.ready.take().unwrap();
        inbox.presented(&receipt.token);
        assert!(inbox.take_encoded().is_some());
        assert!(inbox.frames.is_empty());
    }

    #[test]
    fn first_flush_opens_parallel_admission_and_every_resize_closes_it() {
        let mut inbox = Inbox::new(capabilities(), false);
        inbox.frame_capacity = 5;
        assert!(inbox.reserve_frame());
        assert!(!inbox.can_submit(), "the first geometry admits one frame");
        // A pump which produces no submission must return its reservation.
        inbox.release_reservation();
        assert!(inbox.reserve_frame());
        let token = FrameToken::default();
        inbox.frames.back_mut().unwrap().work = FrameWork::Encoding(token.clone());
        inbox.finished(&token, Some(encoded(1)));
        assert!(!inbox.can_submit(), "codec completion is not a flush");
        let first = inbox.take_encoded().unwrap();
        assert!(!inbox.can_submit(), "writer handoff is not a flush");
        inbox.written(
            Offer {
                token: first.frame.token(),
                geometry: 1,
            },
            None,
        );
        for _ in 0..5 {
            assert!(inbox.reserve_frame());
        }
        assert!(
            !inbox.can_submit(),
            "all unhanded stages share the finite budget"
        );
        // Treat these reservations as executor-queued/active workers. Resize
        // cannot recover their capacity before each worker really returns.
        for job in &mut inbox.frames {
            job.work = FrameWork::Encoding(token.clone());
        }
        inbox
            .push(Event::WindowResized(capabilities().size))
            .unwrap();
        inbox.resize.take();
        let receipt = inbox.ready.take().unwrap();
        inbox.presented(&receipt.token);
        assert_eq!(inbox.frames.len(), 5);
        assert!(!inbox.can_submit());
        inbox.finished(&token, Some(encoded(1)));
        assert_eq!(
            inbox.frames.len(),
            4,
            "a completed stale result releases only its own slot"
        );
        assert!(inbox.reserve_frame());
        for _ in 0..4 {
            inbox.finished(&token, None);
        }
        assert_eq!(inbox.frames.len(), 1);
        assert!(
            !inbox.can_submit(),
            "new geometry needs its own whole flush even with spare capacity"
        );
        inbox.release_reservation();
        assert!(inbox.frames.is_empty());
        assert!(inbox.can_submit());
        assert_eq!(inbox.last_fully_written_geometry, Some(1));
    }

    #[test]
    fn stale_ready_wire_retires_but_failure_and_cancel_return_only_the_finished_slot() {
        let mut inbox = Inbox::new(capabilities(), false);
        let token = FrameToken::default();
        inbox.frames.push_back(FrameJob {
            geometry: 1,
            work: FrameWork::Ready(encoded(1)),
        });
        inbox.frames.push_back(FrameJob {
            geometry: 1,
            work: FrameWork::Encoding(token.clone()),
        });
        inbox
            .push(Event::WindowResized(capabilities().size))
            .unwrap();
        assert_eq!(
            inbox.frames.len(),
            1,
            "only already completed wire is disposable"
        );
        inbox.finished(&token, None);
        assert!(inbox.frames.is_empty());
        assert!(inbox.take_encoded().is_none());
        assert!(inbox.in_flight.is_none());
        assert!(
            inbox.ready.is_none(),
            "failure/cancel cannot manufacture a writer receipt"
        );
    }

    #[test]
    fn protocols_with_previous_image_state_remain_serial_after_flush() {
        for protocol in [Protocol::Kitty, Protocol::Iterm2] {
            let mut caps = capabilities();
            caps.protocol = Some(protocol);
            let mut inbox = Inbox::new(caps, false);
            inbox.frame_capacity = 5;
            inbox.last_fully_written_geometry = Some(1);
            assert!(inbox.reserve_frame());
            assert!(!inbox.can_submit());
            inbox.release_reservation();
            let token = FrameToken::default();
            inbox.in_flight = Some(token.clone());
            inbox.written(Offer { token, geometry: 1 }, Some(41));
            assert!(
                !inbox.can_submit(),
                "previous-image state waits for native receipt consumption"
            );
            let receipt = inbox.ready.take().unwrap();
            inbox.presented(&receipt.token);
            assert!(inbox.can_submit());
            assert_eq!(inbox.last_fully_written_image, Some(41));
        }
    }

    #[test]
    fn late_sixel_maxima_resize_the_same_canvas_without_changing_terminal_units() {
        let mut inbox = Inbox::new(capabilities(), false);
        assert_eq!(inbox.last_fully_written_geometry, None);
        let token = FrameToken::default();
        inbox.published = Some(token.clone());
        let draft = Event::Paste("unfinished 你好 draft".into());
        inbox.push(draft.clone()).unwrap();
        inbox.set_sixel_geometry(maximum(1000, 1000)).unwrap();
        assert_eq!(inbox.geometry, 2);
        assert_eq!(inbox.resize, Some(capabilities().size));
        assert_eq!(inbox.capabilities.size.pixel_width, Some(1200));
        assert_eq!(inbox.capabilities.size.pixel_height, Some(800));
        assert_eq!(
            inbox.capabilities.viewport(),
            Some(gpui::size(
                gpui::DevicePixels(1000),
                gpui::DevicePixels(800)
            ))
        );
        assert!(inbox.published.is_none());
        assert!(inbox.cancel_pointer);
        // Coordinates wait for the native owner to apply resize, while the
        // complete draft remains queued independently of output receipts.
        assert!(inbox.pop().is_none());
        inbox.resize = None;
        assert_eq!(inbox.pop().unwrap().event, draft);

        // The already-started old viewport transaction must finish physically;
        // its receipt records the old output geometry only.
        inbox.written(
            Offer {
                token: token.clone(),
                geometry: 1,
            },
            Some(41),
        );
        assert_eq!(inbox.last_fully_written_image, Some(41));
        assert_eq!(inbox.last_fully_written_geometry, Some(1));
        assert_eq!(inbox.ready.as_ref().unwrap().geometry, 1);
        assert!(inbox.published.is_none());

        // A new fact that does not change this canvas must not cancel a contact.
        inbox.resize = None;
        inbox.cancel_pointer = false;
        inbox.published = Some(token.clone());
        inbox.set_sixel_geometry(maximum(1000, 1200)).unwrap();
        assert_eq!(inbox.geometry, 2);
        assert_eq!(inbox.published, Some(token.clone()));
        assert!(!inbox.cancel_pointer);

        // Actual terminal resize always revokes old coordinates, even if the
        // effective canvas happens to stay unchanged.
        let mut size = capabilities().size;
        size.cols = 140;
        size.pixel_width = Some(1400);
        inbox.push(Event::WindowResized(size)).unwrap();
        assert_eq!(inbox.geometry, 3);
        assert!(inbox.published.is_none());
        assert!(inbox.cancel_pointer);
        assert_eq!(inbox.capabilities.size.pixel_width, Some(1400));
        assert_eq!(
            inbox.capabilities.viewport(),
            Some(gpui::size(
                gpui::DevicePixels(1000),
                gpui::DevicePixels(800)
            ))
        );
        size.cols = 80;
        size.pixel_width = Some(800);
        size.pixel_height = Some(600);
        inbox.push(Event::WindowResized(size)).unwrap();
        assert_eq!(inbox.geometry, 4);
        assert_eq!(inbox.last_fully_written_geometry, Some(1));
        assert_eq!(
            inbox.capabilities.viewport(),
            Some(gpui::size(gpui::DevicePixels(800), gpui::DevicePixels(600)))
        );
    }

    #[test]
    fn resizing_back_to_the_same_pixels_does_not_reuse_an_old_write() {
        let mut inbox = Inbox::new(capabilities(), false);
        let token = FrameToken::default();
        inbox.written(
            Offer {
                token: token.clone(),
                geometry: 1,
            },
            None,
        );
        assert_eq!(inbox.last_fully_written_geometry, Some(1));

        let mut size = capabilities().size;
        size.cols = 80;
        size.pixel_width = Some(800);
        inbox.push(Event::WindowResized(size)).unwrap();
        inbox
            .push(Event::WindowResized(capabilities().size))
            .unwrap();
        assert_eq!(inbox.geometry, 3);
        assert_eq!(inbox.last_fully_written_geometry, Some(1));
        // An old viewport can finish after resize without certifying the new one.
        inbox.written(
            Offer {
                token: token.clone(),
                geometry: 1,
            },
            None,
        );
        assert_eq!(inbox.last_fully_written_geometry, Some(1));
        inbox.written(Offer { token, geometry: 3 }, None);
        assert_eq!(inbox.last_fully_written_geometry, Some(3));
    }

    #[test]
    fn letterbox_presses_and_wheels_do_not_capture_and_hover_leaves_without_a_frame() {
        let mut caps = capabilities();
        caps.sixel_geometry = maximum(1000, 600);
        let mut inbox = Inbox::new(caps, false);
        let pointer = |kind, x, y| {
            Event::SgrMouse(termina::event::SgrMouseEvent {
                kind,
                x,
                y,
                modifiers: Modifiers::NONE,
            })
        };
        // Real cells remain 10x20 pixels. The right/bottom margins begin at
        // cell 101/31, independently of the smaller native canvas dimensions.
        for (x, y) in [(101, 1), (1, 31), (0, 1)] {
            inbox
                .push(pointer(
                    MouseEventKind::Down(termina::event::MouseButton::Left),
                    x,
                    y,
                ))
                .unwrap();
            inbox
                .push(pointer(MouseEventKind::ScrollDown, x, y))
                .unwrap();
            assert_eq!(inbox.queue_len(), 0);
            assert!(!inbox.cancel_pointer);
            inbox.push(pointer(MouseEventKind::Moved, x, y)).unwrap();
            assert!(inbox.cancel_pointer);
            inbox.cancel_pointer = false;
        }
        // The adjacent visible cell reaches the native FIFO even before any
        // output frame is published.
        inbox
            .push(pointer(
                MouseEventKind::Down(termina::event::MouseButton::Left),
                100,
                30,
            ))
            .unwrap();
        assert_eq!(inbox.queue_len(), 1);
        assert!(inbox.pop().is_some());
    }

    #[test]
    fn pointer_fifo_does_not_wait_for_output_and_resize_keeps_new_coordinates() {
        let mut inbox = Inbox::new(capabilities(), false);
        inbox.in_flight = Some(FrameToken::default());
        let pointer = |kind, x, y| {
            Event::SgrMouse(termina::event::SgrMouseEvent {
                kind,
                x,
                y,
                modifiers: Modifiers::NONE,
            })
        };
        let down = pointer(
            MouseEventKind::Down(termina::event::MouseButton::Left),
            4,
            5,
        );
        let wheel = pointer(MouseEventKind::ScrollDown, 4, 5);
        let up = pointer(MouseEventKind::Up(termina::event::MouseButton::Left), 4, 5);
        let events = [
            pointer(MouseEventKind::Moved, 3, 4),
            pointer(MouseEventKind::Moved, 4, 5),
            down,
            wheel,
            up,
        ];
        for event in &events {
            inbox.push(event.clone()).unwrap();
        }
        for event in events {
            assert_eq!(inbox.pop().unwrap().event, event);
        }
        assert!(inbox.in_flight.is_some());
        assert!(inbox.published.is_none());

        let old = pointer(
            MouseEventKind::Down(termina::event::MouseButton::Left),
            4,
            5,
        );
        inbox.push(old).unwrap();
        inbox
            .push(Event::WindowResized(capabilities().size))
            .unwrap();
        let current = pointer(
            MouseEventKind::Down(termina::event::MouseButton::Left),
            6,
            7,
        );
        inbox.push(current.clone()).unwrap();
        assert_eq!(inbox.queue_len(), 1, "resize retires only old coordinates");
        assert!(
            inbox.pop().is_none(),
            "apply native resize before mapping the new pointer"
        );
        inbox.resize.take();
        let input = inbox.pop().unwrap();
        assert_eq!(input.event, current);
        assert_eq!(input.geometry, 2);
    }

    #[test]
    fn keyboard_bursts_and_large_paste_survive_startup_and_resize_in_order() {
        let mut inbox = Inbox::new(
            Capabilities {
                size: WindowSize {
                    cols: 120,
                    rows: 40,
                    pixel_width: Some(1200),
                    pixel_height: Some(800),
                },
                mouse: super::super::probe::MouseCoordinates::Cells,
                protocol: None,
                synchronized: false,
                sixel_geometry: Default::default(),
            },
            false,
        );
        let mut expected = Vec::new();
        for character in "a你b界".repeat(1024).chars() {
            let event = Event::Key(KeyEvent::new(KeyCode::Char(character), Modifiers::NONE));
            assert!(!inbox.push(event.clone()).unwrap());
            expected.push(event);
        }
        let paste = Event::Paste("你好\nworld".repeat(16384));
        assert!(!inbox.push(paste.clone()).unwrap());
        expected.push(paste);
        assert!(
            inbox.has_pending_input(),
            "native input does not wait for pixels"
        );
        let size = WindowSize {
            cols: 80,
            rows: 24,
            pixel_width: Some(800),
            pixel_height: Some(480),
        };
        assert!(!inbox.push(Event::WindowResized(size)).unwrap());
        let (events, resize) = inbox.into_startup();
        assert_eq!(events, expected);
        assert_eq!(resize, Some(size));
    }
}
