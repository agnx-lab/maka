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

use super::probe::{Capabilities, SixelGeometry};
use nsh_gpui_terminal::{CandidateFrame, FrameToken};
use std::collections::{HashSet, VecDeque};
use termina::{
    Event, WindowSize,
    event::{KeyCode, KeyEventKind, Modifiers, MouseEventKind},
};

pub(super) struct Input {
    pub event: Event,
    pub token: Option<FrameToken>,
    pub geometry: u64,
    pub startup: bool,
}

#[derive(Clone)]
pub(super) struct Offer {
    pub token: FrameToken,
    pub geometry: u64,
}

pub(super) struct Candidate {
    pub frame: CandidateFrame,
    pub geometry: u64,
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

/// Send-only terminal state. GPUI snapshots stay on their native owner thread.
pub(crate) struct Inbox {
    pub capabilities: Capabilities,
    events: VecDeque<Input>,
    pub published: Option<FrameToken>,
    pub keyboard: Option<FrameToken>,
    capture: Option<FrameToken>,
    pub(super) pending: Option<Candidate>,
    pub(super) encoded: Option<Result<Encoded, String>>,
    pub displayed_image: Option<u32>,
    pub in_flight: Option<FrameToken>,
    pub(super) ready: Option<Offer>,
    pub geometry: u64,
    pub resize: Option<WindowSize>,
    pub cancel_pointer: bool,
    pub expected_ack: Option<u32>,
    pub ack: Option<(std::time::Instant, Result<(), String>)>,
    pub reject_through: u64,
    pub wake: Option<std::thread::Thread>,
    pub effects: Option<Vec<u8>>,
    pub restore_sixel_mode: bool,
}

impl Inbox {
    pub fn new(capabilities: Capabilities) -> Self {
        Self {
            capabilities,
            events: VecDeque::new(),
            published: None,
            keyboard: None,
            capture: None,
            pending: None,
            encoded: None,
            displayed_image: None,
            in_flight: None,
            ready: None,
            geometry: 1,
            resize: None,
            cancel_pointer: false,
            expected_ack: None,
            ack: None,
            reject_through: 0,
            wake: None,
            effects: None,
            restore_sixel_mode: false,
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
        if let Event::KittyGraphicsResponse(reply) = &event {
            if self.expected_ack == Some(reply.image_id) && reply.placement_id == Some(1) {
                self.ack
                    .get_or_insert_with(|| (std::time::Instant::now(), reply.result.clone()));
            }
            return Ok(false);
        }
        if let Event::WindowResized(size) = event {
            self.capabilities.size = size;
            self.invalidate_geometry()?;
            return Ok(false);
        }
        if matches!(
            event,
            Event::Csi(_) | Event::Osc(_) | Event::Dcs(_) | Event::Iterm2Capabilities(_)
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
                    // Leaving the native canvas has the same cleanup authority
                    // as a physical leave, even without an admissible frame.
                    self.lose_pointer();
                    return Ok(false);
                }
                MouseEventKind::Drag(_) | MouseEventKind::Up(_) => {}
            }
        }
        let token = match &event {
            Event::SgrMouse(mouse) => match mouse.kind {
                MouseEventKind::Down(termina::event::MouseButton::Left) => {
                    self.capture = self.published.clone();
                    self.capture.clone()
                }
                MouseEventKind::Up(termina::event::MouseButton::Left) => {
                    self.capture.take().or_else(|| self.published.clone())
                }
                MouseEventKind::Drag(termina::event::MouseButton::Left) => self.capture.clone(),
                _ => self.published.clone(),
            },
            Event::Key(_) | Event::Paste(_) => self.keyboard.clone(),
            _ => self.published.clone(),
        };
        if matches!(event, Event::MouseLeave) {
            return Ok(false);
        }
        if pointer(&event)
            && (token.is_none()
                || self.resize.is_some()
                || token
                    .as_ref()
                    .is_some_and(|token| token.sequence() <= self.reject_through))
        {
            if released(&event) {
                self.lose_pointer();
            }
            return Ok(false);
        }
        if let Some(previous) = self.events.back_mut()
            && previous.token == token
            && previous.geometry == self.geometry
        {
            match (&mut previous.event, &event) {
                (Event::SgrMouse(previous), Event::SgrMouse(next))
                    if previous.kind == MouseEventKind::Moved
                        && next.kind == MouseEventKind::Moved
                        && previous.modifiers == next.modifiers =>
                {
                    previous.x = next.x;
                    previous.y = next.y;
                    return Ok(false);
                }
                (Event::Mouse(previous), Event::Mouse(next))
                    if previous.kind == MouseEventKind::Moved
                        && next.kind == MouseEventKind::Moved
                        && previous.modifiers == next.modifiers =>
                {
                    previous.column = next.column;
                    previous.row = next.row;
                    return Ok(false);
                }
                _ => {}
            }
        }
        self.events.push_back(Input {
            event,
            token,
            geometry: self.geometry,
            startup: self.keyboard.is_none(),
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

    fn invalidate_geometry(&mut self) -> Result<(), String> {
        self.geometry = self
            .geometry
            .checked_add(1)
            .ok_or("terminal geometry generation exhausted")?;
        self.resize = Some(self.capabilities.size);
        self.published = None;
        self.pending = None;
        self.lose_pointer();
        Ok(())
    }

    fn lose_pointer(&mut self) {
        self.capture = None;
        self.cancel_pointer = true;
        self.events.retain(|input| !pointer(&input.event));
    }

    pub(super) fn has_pending_input(&self) -> bool {
        self.events
            .front()
            .is_some_and(|input| input.token.is_some() || self.published.is_some())
    }

    pub(super) fn pop(&mut self) -> Option<Input> {
        if !self.has_pending_input() {
            return None;
        }
        self.events.pop_front()
    }

    pub fn retained(&self) -> HashSet<FrameToken> {
        self.events
            .iter()
            .filter_map(|input| input.token.clone())
            .chain(self.published.iter().cloned())
            .chain(self.keyboard.iter().cloned())
            .chain(self.capture.iter().cloned())
            .chain(self.pending.iter().map(|candidate| candidate.frame.token()))
            .chain(self.in_flight.iter().cloned())
            .chain(self.ready.iter().map(|offer| offer.token.clone()))
            .collect()
    }

    pub fn into_startup(self) -> (Vec<Event>, Option<WindowSize>) {
        (
            self.events.into_iter().map(|input| input.event).collect(),
            self.resize,
        )
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

    #[test]
    fn late_sixel_maxima_resize_the_same_canvas_without_changing_terminal_units() {
        let mut inbox = Inbox::new(capabilities());
        let token = FrameToken::default();
        inbox.published = Some(token.clone());
        inbox.keyboard = Some(token.clone());
        inbox.capture = Some(token.clone());
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
        assert!(inbox.capture.is_none());
        assert!(inbox.cancel_pointer);
        assert_eq!(inbox.keyboard, Some(token.clone()));
        assert_eq!(inbox.pop().unwrap().event, draft);

        // A new fact that does not change this canvas must not revoke a click.
        inbox.resize = None;
        inbox.cancel_pointer = false;
        inbox.published = Some(token.clone());
        inbox.capture = Some(token.clone());
        inbox.set_sixel_geometry(maximum(1000, 1200)).unwrap();
        assert_eq!(inbox.geometry, 2);
        assert_eq!(inbox.published, Some(token.clone()));
        assert_eq!(inbox.capture, Some(token));
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
        assert_eq!(
            inbox.capabilities.viewport(),
            Some(gpui::size(gpui::DevicePixels(800), gpui::DevicePixels(600)))
        );
    }

    #[test]
    fn letterbox_presses_and_wheels_do_not_capture_and_hover_leaves_without_a_frame() {
        let mut caps = capabilities();
        caps.sixel_geometry = maximum(1000, 600);
        let mut inbox = Inbox::new(caps);
        let token = FrameToken::default();
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
            inbox.capture = Some(token.clone());
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
            assert_eq!(inbox.capture, Some(token.clone()));
            assert!(!inbox.cancel_pointer);
            inbox.push(pointer(MouseEventKind::Moved, x, y)).unwrap();
            assert!(inbox.capture.is_none());
            assert!(inbox.cancel_pointer);
            inbox.cancel_pointer = false;
        }
        // The adjacent visible cell reaches admission instead of being
        // confused with the margin. No published frame means no new capture.
        inbox.capture = Some(token);
        inbox
            .push(pointer(
                MouseEventKind::Down(termina::event::MouseButton::Left),
                100,
                30,
            ))
            .unwrap();
        assert!(inbox.capture.is_none());
    }

    #[test]
    fn keyboard_bursts_and_large_paste_survive_startup_and_resize_in_order() {
        let mut inbox = Inbox::new(Capabilities {
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
        });
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
            !inbox.has_pending_input(),
            "startup input waits for its first presentation"
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
