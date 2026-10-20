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

/// A single startup negotiation. Neither reply carries a request identifier, so
/// a resize cancels this probe permanently instead of accepting a stale reply
/// as measurements for the new viewport.
pub(super) const QUERY: &[u8] = b"\x1b[c\x1b[16t";
const TIMEOUT: Duration = Duration::from_millis(250);

pub(super) struct Probe(Option<Replies>);

#[derive(Default)]
struct Replies {
    deadline: Option<Instant>,
    sixel: Option<(bool, Instant)>,
    cell: Option<(u16, u16, Instant)>,
}

impl Default for Probe {
    fn default() -> Self {
        Self(Some(Replies::default()))
    }
}

impl Probe {
    pub fn flushed(&mut self, at: Instant) {
        if let Some(replies) = &mut self.0 {
            let deadline = at + TIMEOUT;
            replies.deadline = Some(deadline);
            // Reader and writer run independently. A driver delayed in polling
            // this receipt must not turn already-buffered late replies into
            // capabilities merely because the deadline was not armed yet.
            replies.sixel = replies.sixel.filter(|(_, at)| *at <= deadline);
            replies.cell = replies.cell.filter(|(_, _, at)| *at <= deadline);
        }
    }

    pub fn cancel(&mut self) {
        self.0 = None;
    }

    pub fn observe(&mut self, event: &Event, now: Instant) {
        let Some(replies) = &mut self.0 else { return };
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
        let replies = self.0.as_ref()?;
        let deadline = replies.deadline?;
        if matches!(replies.sixel, Some((true, _)))
            && let Some((width, height, _)) = replies.cell
        {
            self.0 = None;
            return ImageProtocol::new(ProtocolType::Sixel, width, height).ok();
        }
        if matches!(replies.sixel, Some((false, _))) || now >= deadline {
            self.0 = None;
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
                probe.cancel();
            }
            let late = now + TIMEOUT + Duration::from_millis(1);
            observe(&mut probe, b"\x1b[?62;4c\x1b[6;16;8t", late);
            assert!(probe.take_ready(late).is_none());
        }
    }
}
