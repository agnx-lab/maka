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

use nsh_platform_tui::{ImageProtocol, ProtocolType};
use std::time::{Duration, Instant};
use termina::{
    Event,
    escape::csi::{Csi, Window},
};

/// Neither reply carries a request identifier. A resized negotiation must
/// consume both original replies before another query can measure the new cells.
pub(super) const QUERY: &[u8] = b"\x1b[c\x1b[16t";
const TIMEOUT: Duration = Duration::from_millis(250);

pub(super) struct Probe(State);

enum State {
    Pending(Replies),
    Draining(Replies),
    NeedQuery,
    Complete,
    Unavailable,
}

#[derive(Default)]
struct Replies {
    deadline: Option<Instant>,
    sixel: Option<(bool, Instant)>,
    cell: Option<(u16, u16, Instant)>,
}

impl Default for Probe {
    fn default() -> Self {
        Self(State::Pending(Replies::default()))
    }
}

impl Probe {
    pub fn flushed(&mut self, at: Instant) {
        if let State::Pending(replies) | State::Draining(replies) = &mut self.0
            && replies.deadline.is_none()
        {
            let deadline = at + TIMEOUT;
            replies.deadline = Some(deadline);
            // Reader and writer run independently. A driver delayed in polling
            // this receipt must not turn already-buffered late replies into
            // capabilities merely because the deadline was not armed yet.
            replies.sixel = replies.sixel.filter(|(_, at)| *at <= deadline);
            replies.cell = replies.cell.filter(|(_, _, at)| *at <= deadline);
        }
    }

    pub fn resize(&mut self) {
        let state = std::mem::replace(&mut self.0, State::Unavailable);
        self.0 = match state {
            State::Pending(replies) | State::Draining(replies) => State::Draining(replies),
            State::NeedQuery | State::Complete => State::NeedQuery,
            State::Unavailable => State::Unavailable,
        };
    }

    /// Called only by the sole output owner while its previous frame is complete.
    pub fn query(&mut self) -> Option<&'static [u8]> {
        if !matches!(self.0, State::NeedQuery) {
            return None;
        }
        self.0 = State::Pending(Replies::default());
        Some(QUERY)
    }

    pub fn observe(&mut self, event: &Event, now: Instant) {
        let (State::Pending(replies) | State::Draining(replies)) = &mut self.0 else {
            return;
        };
        if replies.deadline.is_some_and(|deadline| now > deadline) {
            return;
        }
        match event {
            Event::Csi(Csi::PrimaryDeviceAttributes(attributes)) => {
                replies
                    .sixel
                    .get_or_insert((attributes.attributes.contains(&4), now));
            }
            Event::Csi(Csi::Window(window)) => {
                if let Window::ReportCellSizePixelsResponse {
                    width: Some(width),
                    height: Some(height),
                } = window.as_ref()
                    && let (Ok(width), Ok(height)) = (u16::try_from(*width), u16::try_from(*height))
                    && width != 0
                    && height != 0
                {
                    replies.cell.get_or_insert((width, height, now));
                }
            }
            _ => {}
        }
    }

    pub fn take_ready(&mut self, now: Instant) -> Option<ImageProtocol> {
        let (replies, stale) = match &self.0 {
            State::Pending(replies) => (replies, false),
            State::Draining(replies) => (replies, true),
            _ => return None,
        };
        let deadline = replies.deadline?;
        if let Some((supported, _)) = replies.sixel
            && let Some((width, height, _)) = replies.cell
        {
            let protocol = (!stale && supported)
                .then(|| ImageProtocol::new(ProtocolType::Sixel, width, height).ok())
                .flatten();
            self.0 = if stale {
                State::NeedQuery
            } else {
                State::Complete
            };
            return protocol;
        }
        if now >= deadline {
            self.0 = State::Unavailable;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termina::Parser;

    fn observe(probe: &mut Probe, input: &[u8], now: Instant) {
        let mut parser = Parser::default();
        parser.parse(input, false).unwrap();
        while let Some(event) = parser.pop() {
            probe.observe(&event, now);
        }
    }

    #[test]
    fn negotiation_requires_both_sixel_attribute_and_real_cell_pixels() {
        let now = Instant::now();
        let mut probe = Probe::default();
        // A fast reply may precede the driver's observation of its flush receipt.
        observe(&mut probe, b"\x1b[?62;1;4;22c", now);
        assert!(probe.take_ready(now).is_none());
        probe.flushed(now);
        assert!(probe.take_ready(now).is_none());
        observe(&mut probe, b"\x1b[6;0;8t\x1b[6;17;9t", now);
        let protocol = probe.take_ready(now).unwrap();
        assert_eq!(protocol.protocol_type(), ProtocolType::Sixel);
        assert_eq!(protocol.cell_size(), (9, 17));
        assert!(probe.take_ready(now).is_none());
    }

    #[test]
    fn delayed_receipt_observation_keeps_only_replies_arriving_before_actual_deadline() {
        let flushed = Instant::now();
        let late = flushed + TIMEOUT + Duration::from_millis(1);
        for (arrived, accepted) in [(flushed + Duration::from_millis(1), true), (late, false)] {
            let mut probe = Probe::default();
            observe(&mut probe, b"\x1b[?62;4c\x1b[6;16;8t", arrived);
            // The writer completed at `flushed`; only its observer is delayed.
            probe.flushed(flushed);
            assert_eq!(probe.take_ready(late).is_some(), accepted);
        }
    }

    #[test]
    fn resized_cells_are_measured_only_after_the_original_replies_are_consumed() {
        let now = Instant::now();
        let mut probe = Probe::default();
        probe.flushed(now);
        observe(&mut probe, b"\x1b[?61;4c\x1b[6;20;10t", now);
        assert_eq!(probe.take_ready(now).unwrap().cell_size(), (10, 20));

        probe.resize();
        assert_eq!(probe.query(), Some(b"\x1b[c\x1b[16t".as_slice()));
        let flushed = now + Duration::from_millis(1);
        observe(&mut probe, b"\x1b[6;22;11t", flushed);
        probe.resize();
        probe.resize();
        assert!(probe.take_ready(flushed).is_none());
        assert!(probe.query().is_none());
        probe.flushed(flushed);
        observe(&mut probe, b"\x1b[?61;4c", flushed);
        assert!(probe.take_ready(flushed).is_none());

        assert_eq!(probe.query(), Some(b"\x1b[c\x1b[16t".as_slice()));
        probe.flushed(flushed);
        observe(&mut probe, b"\x1b[?61;4c\x1b[6;24;12t", flushed);
        assert_eq!(probe.take_ready(flushed).unwrap().cell_size(), (12, 24));

        probe.resize();
        assert!(probe.query().is_some());
        probe.flushed(flushed);
        observe(&mut probe, b"\x1b[?61;14c", flushed);
        assert!(probe.take_ready(flushed).is_none());
        probe.resize();
        assert!(probe.query().is_none(), "the old 16t reply is still owed");
        let late = flushed + TIMEOUT + Duration::from_millis(1);
        assert!(probe.take_ready(late).is_none());
        observe(&mut probe, b"\x1b[6;32;16t", late);
        probe.resize();
        assert!(probe.query().is_none());
        assert!(probe.take_ready(late).is_none());
    }

    #[test]
    fn unsupported_late_or_resized_negotiations_never_enable_graphics() {
        let now = Instant::now();
        for reply in [b"\x1b[?4;14c".as_slice(), b"\x1b[?62;14c"] {
            let mut probe = Probe::default();
            probe.flushed(now);
            observe(&mut probe, reply, now);
            observe(&mut probe, b"\x1b[6;16;8t", now);
            assert!(
                probe.take_ready(now).is_none(),
                "terminal type and substring are not attributes"
            );
        }
        for resize in [false, true] {
            let mut probe = Probe::default();
            probe.flushed(now);
            if resize {
                probe.resize();
            }
            let late = now + TIMEOUT + Duration::from_millis(1);
            observe(&mut probe, b"\x1b[?62;4c\x1b[6;16;8t", late);
            assert!(probe.take_ready(late).is_none());
        }
    }
}
