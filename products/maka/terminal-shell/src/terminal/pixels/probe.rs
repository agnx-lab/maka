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

use super::geometry::PixelSizeQuery;
use std::time::{Duration, Instant};
use termina::{
    Event, WindowSize,
    escape::csi::{Csi, DecModeSetting, DecPrivateMode, DecPrivateModeCode, Mode},
};

const TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Copy)]
pub(super) struct Slots {
    pub query: u32,
    pub first: u32,
    pub second: u32,
}

impl Default for Slots {
    fn default() -> Self {
        let random = uuid::Uuid::new_v4();
        let query = u32::from_le_bytes(random.as_bytes()[..4].try_into().unwrap()).max(1);
        let first = query.wrapping_add(1).max(1);
        let second = first.wrapping_add(1).max(1);
        Self {
            query,
            first,
            second,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Capabilities {
    pub size: WindowSize,
    pub pixel_mouse: bool,
}

impl Capabilities {
    pub fn pixel_size(self) -> Option<gpui::Size<gpui::DevicePixels>> {
        let width = self.size.pixel_width?;
        let height = self.size.pixel_height?;
        (width != 0 && height != 0).then(|| {
            gpui::size(
                gpui::DevicePixels(i32::from(width)),
                gpui::DevicePixels(i32::from(height)),
            )
        })
    }

    pub fn viewport(self) -> Option<gpui::Size<gpui::DevicePixels>> {
        if self.size.cols == 0 || self.size.rows == 0 {
            return None;
        }
        self.pixel_size()
    }
}

pub(crate) struct Probe {
    pub slots: Slots,
    deadline: Option<Instant>,
    kitty: Option<(bool, Instant)>,
    mouse: Option<(bool, Instant)>,
    pub measurement: PixelSizeQuery,
    cancelled: bool,
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            slots: Slots::default(),
            deadline: None,
            kitty: None,
            mouse: None,
            measurement: PixelSizeQuery::default(),
            cancelled: false,
        }
    }
}

impl Probe {
    pub fn query(&mut self) -> Result<Vec<u8>, String> {
        if !self.measurement.begin(1)? {
            return Err("terminal geometry query is already in flight".into());
        }
        let mut bytes = nsh_gpui_terminal::kitty::capability_query(self.slots.query)
            .map_err(|error| error.to_string())?;
        bytes.extend_from_slice(b"\x1b[?1016$p\x1b[14t");
        Ok(bytes)
    }

    pub fn flushed(&mut self, at: Instant) {
        let deadline = at + TIMEOUT;
        self.deadline = Some(deadline);
        self.kitty = self.kitty.filter(|(_, at)| *at <= deadline);
        self.mouse = self.mouse.filter(|(_, at)| *at <= deadline);
        self.measurement.flushed(at);
    }

    pub fn observe(&mut self, event: &Event, now: Instant) {
        self.measurement.observe(event, now);
        if self.cancelled || self.deadline.is_some_and(|deadline| now > deadline) {
            return;
        }
        match event {
            Event::KittyGraphicsResponse(reply)
                if reply.image_id == self.slots.query && reply.placement_id.is_none() =>
            {
                self.kitty.get_or_insert((reply.result.is_ok(), now));
            }
            Event::Csi(Csi::Mode(Mode::ReportDecPrivateMode {
                mode: DecPrivateMode::Code(DecPrivateModeCode::SGRPixelsMouse),
                setting,
            })) => {
                self.mouse.get_or_insert((
                    matches!(
                        setting,
                        DecModeSetting::Set
                            | DecModeSetting::Reset
                            | DecModeSetting::PermanentlySet
                    ),
                    now,
                ));
            }
            Event::WindowResized(_) => self.cancelled = true,
            _ => {}
        }
    }

    /// A positive graphics reply is required. A terminal without 1016 keeps
    /// its real cell precision; its pointer is mapped to that cell's centre.
    pub fn decision(&self, now: Instant, mut size: WindowSize) -> Option<Option<Capabilities>> {
        if self.cancelled {
            return Some(None);
        }
        let deadline = self.deadline?;
        if self.kitty.is_some_and(|(supported, _)| !supported) {
            return Some(None);
        }
        if self.kitty.is_none()
            || self.mouse.is_none()
            || self.measurement.pixels_before(deadline).is_none()
        {
            if now < deadline {
                return None;
            }
        }
        if !self.kitty.is_some_and(|(supported, _)| supported) {
            return Some(None);
        }
        if let Some((width, height)) = self.measurement.pixels_before(deadline) {
            size.pixel_width = Some(width);
            size.pixel_height = Some(height);
        }
        if size.cols == 0
            || size.rows == 0
            || size.pixel_width.is_none_or(|n| n == 0)
            || size.pixel_height.is_none_or(|n| n == 0)
        {
            return Some(None);
        }
        Some(Some(Capabilities {
            size,
            pixel_mouse: self.mouse.is_some_and(|(supported, _)| supported),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size() -> WindowSize {
        WindowSize {
            cols: 120,
            rows: 40,
            pixel_width: None,
            pixel_height: None,
        }
    }

    fn probe() -> Probe {
        let mut probe = Probe {
            slots: Slots {
                query: 99,
                first: 41,
                second: 73,
            },
            ..Default::default()
        };
        probe.query().unwrap();
        probe
    }

    fn reply(probe: &mut Probe, bytes: &[u8], at: Instant) {
        let mut parser = termina::Parser::default();
        parser.parse(bytes, false).unwrap();
        while let Some(event) = parser.pop() {
            probe.observe(&event, at);
        }
    }

    #[test]
    fn graphics_pixel_geometry_and_mouse_precision_are_independent_replies() {
        let now = Instant::now();
        for (mode, precision) in [(2, true), (0, false)] {
            let mut probe = probe();
            probe.flushed(now);
            reply(
                &mut probe,
                format!("\x1b_Gi=99;OK\x1b\\\x1b[?1016;{mode}$y\x1b[4;640;960t").as_bytes(),
                now,
            );
            let result = probe.decision(now, size()).unwrap().unwrap();
            assert_eq!(result.pixel_mouse, precision);
            assert_eq!(
                result.viewport(),
                Some(gpui::size(gpui::DevicePixels(960), gpui::DevicePixels(640)))
            );
        }
    }

    #[test]
    fn startup_geometry_reply_debt_survives_the_capability_observation_window() {
        let flushed = Instant::now();
        let mut probe = probe();
        probe.flushed(flushed);
        reply(&mut probe, b"\x1b_Gi=99;OK\x1b\\\x1b[?1016;2$y", flushed);
        let mut ioctl = size();
        ioctl.pixel_width = Some(960);
        ioctl.pixel_height = Some(640);
        assert!(probe.decision(flushed + TIMEOUT, ioctl).unwrap().is_some());
        // A resize cannot reassign the still-unfinished startup query. Its
        // late reply belongs to geometry 1, even after discovery has ended.
        assert!(!probe.measurement.begin(2).unwrap());
        let after_discovery = flushed + TIMEOUT + Duration::from_millis(10);
        reply(&mut probe, b"\x1b[4;640;960t", after_discovery);
        assert_eq!(
            probe.measurement.poll(after_discovery).unwrap(),
            Some((1, 960, 640))
        );
        assert!(probe.measurement.begin(2).unwrap());
        ioctl.pixel_width = Some(0);
        assert!(
            Capabilities {
                size: ioctl,
                pixel_mouse: false
            }
            .viewport()
            .is_none()
        );
    }

    #[test]
    fn foreign_late_or_resized_replies_cannot_choose_the_pixel_renderer() {
        let flushed = Instant::now();
        let late = flushed + TIMEOUT + Duration::from_millis(1);
        for (nonce, arrival, resized) in
            [(98, flushed, false), (99, late, false), (99, flushed, true)]
        {
            let mut probe = probe();
            probe.flushed(flushed);
            if resized {
                probe.observe(&Event::WindowResized(size()), flushed);
            }
            reply(
                &mut probe,
                format!("\x1b_Gi={nonce};OK\x1b\\\x1b[?1016;2$y\x1b[4;640;960t").as_bytes(),
                arrival,
            );
            assert!(matches!(probe.decision(late, size()), Some(None)));
        }
    }
}
