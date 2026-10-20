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

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::collections::VecDeque;

const MAX_EVENTS: usize = 128;
const MAX_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(super) struct Input {
    pub event: Event,
    /// Pointer events are evidence of a particular completed frame. Keys
    /// retain their order and follow the focused editor after presentation.
    pub pointer_epoch: Option<u64>,
}

#[derive(Default)]
pub(super) struct Inbox {
    events: VecDeque<Input>,
    bytes: usize,
    pub resize: Option<(u16, u16)>,
    pub cancel_pointer: bool,
    pub error: Option<String>,
    pub quit: bool,
}

impl Inbox {
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
            self.resize = Some((columns, rows));
            self.cancel_pointer = true;
            return;
        }
        if self.error.is_some() {
            return;
        }
        let pointer_epoch = if matches!(event, Event::Mouse(_)) {
            if epoch == 0 || self.resize.is_some() {
                self.cancel_pointer = true;
                return;
            }
            Some(epoch)
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
        self.events.push_back(Input {
            event,
            pointer_epoch,
        });
    }

    pub fn pop(&mut self, epoch: u64) -> Option<Input> {
        while let Some(input) = self.events.pop_front() {
            self.bytes -= event_bytes(&input.event);
            if input.pointer_epoch.is_some_and(|offered| offered != epoch) {
                self.cancel_pointer = true;
                continue;
            }
            return Some(input);
        }
        None
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
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
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
}
