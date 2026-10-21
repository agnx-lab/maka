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
use gpui::{PlatformInput, point, px};
use std::time::{Duration, Instant};
use termina::{
    Event,
    event::{KeyCode, KeyEventKind, Modifiers, MouseButton, MouseEventKind},
};

#[derive(Default)]
pub(super) struct Mapping {
    last_click: Option<(MouseButton, gpui::Point<gpui::Pixels>, Instant, usize)>,
    clicks: usize,
}

impl Mapping {
    pub fn input(&mut self, event: &Event, caps: Capabilities) -> Option<PlatformInput> {
        match event {
            Event::Key(event) => {
                let mut modifiers = modifiers(event.modifiers);
                let (key, key_char) = match event.code {
                    KeyCode::Char(character) => (
                        if character == ' ' {
                            "space".into()
                        } else {
                            character.to_lowercase().collect()
                        },
                        (!modifiers.control && !modifiers.alt && !modifiers.platform)
                            .then(|| character.to_string()),
                    ),
                    KeyCode::BackTab => {
                        modifiers.shift = true;
                        ("tab".into(), None)
                    }
                    KeyCode::Function(n) => (format!("f{n}"), None),
                    KeyCode::Enter => ("enter".into(), None),
                    KeyCode::Backspace => ("backspace".into(), None),
                    KeyCode::Tab => ("tab".into(), None),
                    KeyCode::Escape => ("escape".into(), None),
                    KeyCode::Left => ("left".into(), None),
                    KeyCode::Right => ("right".into(), None),
                    KeyCode::Up => ("up".into(), None),
                    KeyCode::Down => ("down".into(), None),
                    KeyCode::Home => ("home".into(), None),
                    KeyCode::End => ("end".into(), None),
                    KeyCode::PageUp => ("pageup".into(), None),
                    KeyCode::PageDown => ("pagedown".into(), None),
                    KeyCode::Insert => ("insert".into(), None),
                    KeyCode::Delete => ("delete".into(), None),
                    KeyCode::KeypadBegin => ("clear".into(), None),
                    KeyCode::CapsLock
                    | KeyCode::ScrollLock
                    | KeyCode::NumLock
                    | KeyCode::PrintScreen
                    | KeyCode::Pause
                    | KeyCode::Menu
                    | KeyCode::Null
                    | KeyCode::Modifier(_)
                    | KeyCode::Media(_) => return None,
                };
                let keystroke = gpui::Keystroke {
                    modifiers,
                    key,
                    key_char,
                };
                Some(match event.kind {
                    KeyEventKind::Release => PlatformInput::KeyUp(gpui::KeyUpEvent { keystroke }),
                    KeyEventKind::Press | KeyEventKind::Repeat => {
                        PlatformInput::KeyDown(gpui::KeyDownEvent {
                            keystroke,
                            is_held: event.kind == KeyEventKind::Repeat,
                            prefer_character_input: false,
                        })
                    }
                })
            }
            Event::SgrMouse(mouse) => {
                let position = if caps.pixel_mouse {
                    point(px(mouse.x as f32), px(mouse.y as f32))
                } else {
                    let x = u16::try_from(mouse.x).ok()?.checked_sub(1)?;
                    let y = u16::try_from(mouse.y).ok()?.checked_sub(1)?;
                    cell_centre(x, y, caps)?
                };
                Some(self.mouse(mouse.kind, mouse.modifiers, position))
            }
            Event::Mouse(mouse) => Some(self.mouse(
                mouse.kind,
                mouse.modifiers,
                cell_centre(mouse.column, mouse.row, caps)?,
            )),
            Event::MouseLeave => None,
            Event::WindowResized(_)
            | Event::FocusIn
            | Event::FocusOut
            | Event::Paste(_)
            | Event::Csi(_)
            | Event::Osc(_)
            | Event::Dcs(_)
            | Event::KittyGraphicsResponse(_) => None,
        }
    }

    fn mouse(
        &mut self,
        kind: MouseEventKind,
        mods: Modifiers,
        position: gpui::Point<gpui::Pixels>,
    ) -> PlatformInput {
        let modifiers = modifiers(mods);
        match kind {
            MouseEventKind::Down(button) => {
                let now = Instant::now();
                self.clicks = self
                    .last_click
                    .filter(|(last, at, time, _)| {
                        *last == button
                            && now.duration_since(*time) < Duration::from_millis(400)
                            && f32::from((at.x - position.x).abs()) <= 4.0
                            && f32::from((at.y - position.y).abs()) <= 4.0
                    })
                    .map_or(1, |(_, _, _, clicks)| clicks % 3 + 1);
                self.last_click = Some((button, position, now, self.clicks));
                PlatformInput::MouseDown(gpui::MouseDownEvent {
                    button: mouse_button(button),
                    position,
                    modifiers,
                    click_count: self.clicks,
                    first_mouse: false,
                })
            }
            MouseEventKind::Up(button) => PlatformInput::MouseUp(gpui::MouseUpEvent {
                button: mouse_button(button),
                position,
                modifiers,
                click_count: self.clicks.max(1),
            }),
            MouseEventKind::Drag(button) => PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position,
                pressed_button: Some(mouse_button(button)),
                modifiers,
            }),
            MouseEventKind::Moved => PlatformInput::MouseMove(gpui::MouseMoveEvent {
                position,
                pressed_button: None,
                modifiers,
            }),
            MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => {
                let (x, y) = match kind {
                    MouseEventKind::ScrollUp => (0.0, 3.0),
                    MouseEventKind::ScrollDown => (0.0, -3.0),
                    MouseEventKind::ScrollLeft => (3.0, 0.0),
                    MouseEventKind::ScrollRight => (-3.0, 0.0),
                    _ => unreachable!(),
                };
                PlatformInput::ScrollWheel(gpui::ScrollWheelEvent {
                    position,
                    modifiers,
                    delta: gpui::ScrollDelta::Lines(point(x, y)),
                    touch_phase: gpui::TouchPhase::Moved,
                })
            }
        }
    }
}

fn cell_centre(x: u16, y: u16, caps: Capabilities) -> Option<gpui::Point<gpui::Pixels>> {
    if caps.size.cols == 0 || caps.size.rows == 0 {
        return None;
    }
    Some(point(
        px((f32::from(x) + 0.5) * f32::from(caps.size.pixel_width?) / f32::from(caps.size.cols)),
        px((f32::from(y) + 0.5) * f32::from(caps.size.pixel_height?) / f32::from(caps.size.rows)),
    ))
}

fn mouse_button(button: MouseButton) -> gpui::MouseButton {
    match button {
        MouseButton::Left => gpui::MouseButton::Left,
        MouseButton::Right => gpui::MouseButton::Right,
        MouseButton::Middle => gpui::MouseButton::Middle,
    }
}

fn modifiers(modifiers: Modifiers) -> gpui::Modifiers {
    gpui::Modifiers {
        control: modifiers.contains(Modifiers::CONTROL),
        alt: modifiers.contains(Modifiers::ALT),
        shift: modifiers.contains(Modifiers::SHIFT),
        platform: modifiers.intersects(Modifiers::SUPER | Modifiers::META),
        function: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_zero_and_negative_release_coordinates_do_not_take_the_cell_conversion() {
        let caps = Capabilities {
            size: termina::WindowSize {
                cols: 120,
                rows: 40,
                pixel_width: Some(960),
                pixel_height: Some(640),
            },
            pixel_mouse: true,
        };
        let mut mapping = Mapping::default();
        let raw = |x, y| {
            Event::SgrMouse(termina::event::SgrMouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                x,
                y,
                modifiers: Modifiers::NONE,
            })
        };
        for (x, y) in [(0, 0), (-5, 650)] {
            let Some(PlatformInput::MouseUp(event)) = mapping.input(&raw(x, y), caps) else {
                panic!("physical release is preserved");
            };
            assert_eq!(
                (f32::from(event.position.x), f32::from(event.position.y)),
                (x as f32, y as f32)
            );
        }
        let Some(PlatformInput::MouseUp(event)) = mapping.input(
            &raw(1, 1),
            Capabilities {
                pixel_mouse: false,
                ..caps
            },
        ) else {
            panic!("cell release maps to its centre");
        };
        assert_eq!(
            (f32::from(event.position.x), f32::from(event.position.y)),
            (4.0, 8.0)
        );
        assert!(
            mapping
                .input(
                    &raw(0, 1),
                    Capabilities {
                        pixel_mouse: false,
                        ..caps
                    }
                )
                .is_none()
        );
    }
}
