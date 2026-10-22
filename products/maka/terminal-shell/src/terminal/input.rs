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

// Yield the input lock between read slices; this is not a queue limit.
pub(super) const READ_BATCH: usize = 128;

#[derive(Default)]
pub(super) struct Inbox {
    events: VecDeque<Event>,
    pub resize: Option<(u16, u16)>,
    pub graphics: super::graphics::Probe,
    pub cancel_pointer: bool,
    pub error: Option<String>,
    pub quit: bool,
    #[cfg(target_os = "linux")]
    pub pixel_probe: super::pixels::Probe,
    #[cfg(target_os = "linux")]
    pub pixels: Option<super::pixels::Inbox>,
    #[cfg(target_os = "linux")]
    pub trace: Option<super::frame_trace::Trace>,
}

impl Inbox {
    pub fn push(&mut self, event: Event) {
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
            // Coordinates read before the resize no longer describe the
            // pending viewport. Keys and paste keep their physical order.
            self.events
                .retain(|event| !matches!(event, Event::Mouse(_)));
            return;
        }
        if self.error.is_none() {
            self.events.push_back(event);
        }
    }

    pub fn pop_batch(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};

    fn key(character: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE))
    }

    fn mouse(kind: MouseEventKind) -> Event {
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
            inbox.push(key(character));
        }
        inbox.push(Event::Resize(20, 4));
        inbox.push(Event::Resize(80, 20));
        inbox.push(Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(inbox.resize, Some((80, 20)));
        assert!(inbox.cancel_pointer);
        assert!(inbox.quit);
        assert_eq!(inbox.pop_batch(), text.chars().map(key).collect::<Vec<_>>());
    }

    #[test]
    fn complete_large_paste_keeps_literal_unicode_in_one_ordered_event() {
        let mut inbox = Inbox::default();
        let text = "你好\nworld\t😀".repeat(32768);
        let expected = vec![key('a'), Event::Paste(text), key('b')];
        for event in expected.iter().cloned() {
            inbox.push(event);
        }
        assert_eq!(inbox.pop_batch(), expected);
    }

    #[test]
    fn pointer_and_key_bursts_keep_every_physical_event_in_order() {
        let mut inbox = Inbox::default();
        let expected = vec![
            mouse(MouseEventKind::Moved),
            mouse(MouseEventKind::Down(MouseButton::Left)),
            mouse(MouseEventKind::Drag(MouseButton::Left)),
            mouse(MouseEventKind::Up(MouseButton::Left)),
            mouse(MouseEventKind::ScrollDown),
            key('x'),
            mouse(MouseEventKind::Moved),
        ];
        for event in expected.iter().cloned() {
            inbox.push(event);
        }
        assert_eq!(inbox.pop_batch(), expected);
    }

    #[test]
    fn resize_retires_old_coordinates_and_keeps_new_viewport_input() {
        let mut inbox = Inbox::default();
        inbox.push(mouse(MouseEventKind::Down(MouseButton::Left)));
        inbox.push(key('a'));
        inbox.push(Event::Resize(80, 20));
        inbox.push(mouse(MouseEventKind::Down(MouseButton::Left)));
        inbox.push(mouse(MouseEventKind::Up(MouseButton::Left)));
        assert_eq!(inbox.resize, Some((80, 20)));
        assert!(inbox.cancel_pointer);
        assert_eq!(
            inbox.pop_batch(),
            vec![
                key('a'),
                mouse(MouseEventKind::Down(MouseButton::Left)),
                mouse(MouseEventKind::Up(MouseButton::Left)),
            ]
        );
    }
}
