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
}

/// Send-only terminal state. GPUI snapshots stay on their native owner thread.
pub(crate) struct Inbox {
    pub capabilities: Capabilities,
    events: VecDeque<Input>,
    bytes: usize,
    pub published: Option<FrameToken>,
    capture: Option<FrameToken>,
    pub pending: Option<CandidateFrame>,
    pub in_flight: Option<FrameToken>,
    pub ready: Option<FrameToken>,
    pub resize: Option<WindowSize>,
    pub cancel_capture: bool,
    pub expected_ack: Option<u32>,
    pub ack: Option<(std::time::Instant, Result<(), String>)>,
    pub reject_through: u64,
    pub wake: Option<std::thread::Thread>,
    pub effects: Option<Vec<u8>>,
}

impl Inbox {
    pub fn new(capabilities: Capabilities) -> Self {
        Self {
            capabilities,
            events: VecDeque::new(),
            bytes: 0,
            published: None,
            capture: None,
            pending: None,
            in_flight: None,
            ready: None,
            resize: None,
            cancel_capture: false,
            expected_ack: None,
            ack: None,
            reject_through: 0,
            wake: None,
            effects: None,
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
            self.resize = Some(size);
            self.published = None;
            self.pending = None;
            self.capture = None;
            self.cancel_capture = true;
            self.events.retain(|input| !pointer(&input.event));
            self.bytes = self.events.iter().map(|input| bytes(&input.event)).sum();
            return Ok(false);
        }
        if matches!(event, Event::Csi(_) | Event::Osc(_) | Event::Dcs(_)) {
            return Ok(false);
        }
        if matches!(event, Event::MouseLeave | Event::FocusOut) {
            self.capture = None;
            self.cancel_capture = true;
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
            _ => self.published.clone(),
        };
        if pointer(&event)
            && (token.is_none()
                || self.resize.is_some()
                || token
                    .as_ref()
                    .is_some_and(|token| token.sequence() <= self.reject_through))
        {
            return Ok(false);
        }
        let length = bytes(&event);
        if self.events.len() == super::super::input::MAX_EVENTS
            || length > super::super::input::MAX_BYTES.saturating_sub(self.bytes)
        {
            return Err("terminal input exceeded its pending budget; input was rejected".into());
        }
        self.bytes += length;
        self.events.push_back(Input { event, token });
        Ok(false)
    }

    pub fn pop(&mut self) -> Option<Input> {
        if self
            .events
            .front()
            .is_some_and(|input| input.token.is_none())
            && self.published.is_none()
        {
            return None;
        }
        let input = self.events.pop_front()?;
        self.bytes -= bytes(&input.event);
        Some(input)
    }

    pub fn retained(&self) -> HashSet<FrameToken> {
        self.events
            .iter()
            .filter_map(|input| input.token.clone())
            .chain(self.published.iter().cloned())
            .chain(self.capture.iter().cloned())
            .chain(self.pending.iter().map(CandidateFrame::token))
            .chain(self.in_flight.iter().cloned())
            .chain(self.ready.iter().cloned())
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

fn bytes(event: &Event) -> usize {
    match event {
        Event::Paste(text) => text.len(),
        _ => std::mem::size_of::<Event>(),
    }
}
