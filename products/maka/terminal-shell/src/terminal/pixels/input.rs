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

use super::probe::Capabilities;
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
            self.geometry = self
                .geometry
                .checked_add(1)
                .ok_or("terminal geometry generation exhausted")?;
            self.capabilities.size = size;
            self.resize = Some(size);
            self.published = None;
            self.pending = None;
            self.lose_pointer();
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

    fn lose_pointer(&mut self) {
        self.capture = None;
        self.cancel_pointer = true;
        self.events.retain(|input| !pointer(&input.event));
    }

    pub(super) fn pop(&mut self) -> Option<Input> {
        if self
            .events
            .front()
            .is_some_and(|input| input.token.is_none())
            && self.published.is_none()
        {
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
    use termina::event::KeyEvent;

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
