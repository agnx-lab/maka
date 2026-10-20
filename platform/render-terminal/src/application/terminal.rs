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

//! Terminal view extracted from the original resource page; all process effects
//! leave through ordinary component observations, never through native paint.
mod input;
use super::{NativeInput, Placement, Slots, Widget, WidgetInput, paint};
use crossterm::event::{Event, MouseEventKind};
use maka_ui_runtime::{
    ComponentCall, ComponentOutput, NativeCapability, NativeComponent, NativeExport,
    NativeRegistry, NativeView, terminal::*,
};
use ratatui::{
    buffer::Buffer,
    style::{Color, Style},
};
use std::{any::Any, collections::BTreeMap, rc::Rc};
struct Geometry {
    area: Placement,
    focused: bool,
    source: String,
    controller: Option<uuid::Uuid>,
}
struct Owner {
    core: state::State,
    geometry: Option<Geometry>,
}
struct Recipe(TerminalViewProps);
fn color(c: maka_ui_runtime::appearance::Color) -> Color {
    Color::Rgb((c.0 >> 24) as u8, (c.0 >> 16) as u8, (c.0 >> 8) as u8)
}
impl Owner {
    fn observation(&mut self) -> Option<NativeInput> {
        let value = self.core.pending_observation()?;
        Some(NativeInput {
            replaceable: false,
            key: "observe".into(),
            value: serde_json::to_value(value).expect("terminal observation"),
        })
    }
}
impl Widget for Recipe {
    fn create(&self, context: super::NativeContext) -> Box<dyn Any> {
        Box::new(Owner {
            core: state::State::new(&context.budget),
            geometry: None,
        })
    }
    fn same_presentation(&self, other: &dyn Any) -> bool {
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.0 == other.0)
    }
    fn commit(&self, state: &mut dyn Any) -> maka_ui_runtime::Result<Option<NativeInput>> {
        let owner = state.downcast_mut::<Owner>().unwrap();
        owner.core.apply(&self.0);
        Ok(owner.observation())
    }
    fn published(&self, state: &mut dyn Any, key: &str, value: &serde_json::Value) {
        if key == "observe" {
            state.downcast_mut::<Owner>().unwrap().core.published(
                value["revision"]
                    .as_u64()
                    .expect("original terminal observation"),
            );
        }
    }
    fn observe(&self, state: &mut dyn Any) -> Option<NativeInput> {
        state.downcast_mut::<Owner>().unwrap().observation()
    }
    fn returns_focus(&self) -> bool {
        true
    }
    fn handles_tab(&self, state: &dyn Any, _: &str) -> bool {
        state.downcast_ref::<Owner>().unwrap().core.captured()
    }
    fn captures_keyboard(&self, state: &dyn Any) -> bool {
        state.downcast_ref::<Owner>().unwrap().core.captured()
    }
    fn raw_pointer(&self, state: &dyn Any) -> bool {
        state
            .downcast_ref::<Owner>()
            .unwrap()
            .core
            .display
            .is_some()
    }
    fn enter_focus(&self, state: &mut dyn Any, _: &str) -> bool {
        let owner = state.downcast_mut::<Owner>().unwrap();
        owner.core.capture(true);
        owner.core.display.is_some()
    }
    fn input(
        &self,
        state: &mut dyn Any,
        _: &str,
        event: &Event,
    ) -> maka_ui_runtime::Result<WidgetInput> {
        let owner = state.downcast_mut::<Owner>().unwrap();
        let Some(geometry) = &owner.geometry else {
            return Ok(WidgetInput::Consumed(None));
        };
        if geometry.source != self.0.source || geometry.controller != self.0.controller {
            return Ok(WidgetInput::Consumed(None));
        }
        let captured = owner.core.captured();
        if !captured
            && matches!(event, Event::Key(key) if matches!(key.code, crossterm::event::KeyCode::Tab | crossterm::event::KeyCode::BackTab))
        {
            return Ok(WidgetInput::ExitFocus);
        }
        let handled = if let Event::Mouse(mouse) = event {
            let area = geometry.area;
            if area.contains((mouse.column, mouse.row).into())
                && matches!(mouse.kind, MouseEventKind::Down(_))
            {
                owner.core.capture(true);
            }
            owner.mouse(*mouse, area)
        } else {
            owner.key(event)
        };
        if captured && !owner.core.captured() {
            return Ok(WidgetInput::ExitFocus);
        }
        Ok(if handled {
            WidgetInput::Consumed(owner.observation())
        } else {
            WidgetInput::Unhandled
        })
    }
    fn copy(
        &self,
        state: &dyn Any,
        selection_only: bool,
    ) -> maka_ui_runtime::Result<Option<String>> {
        if selection_only {
            return Ok(None);
        }
        Ok(state
            .downcast_ref::<Owner>()
            .unwrap()
            .core
            .display
            .as_ref()
            .map(|d| d.screen.clone()))
    }
    fn measure(&self, _: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        Ok((width, u32::from(self.0.rows)))
    }
    fn layout(&self, _: &Slots<'_>) -> maka_ui_runtime::Result<super::TerminalLayout> {
        // Resize the actual terminal viewport when controls leave less space;
        // clipping a larger screen would hide the shell's newest output.
        Ok(super::TerminalLayout {
            shrink: true,
            ..Default::default()
        })
    }
    fn paint(
        &self,
        slots: &mut Slots<'_>,
        area: Placement,
        buffer: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        if area.is_empty() {
            return Ok(());
        }
        let owner = slots.native_state::<Owner>()?;
        let focused = slots.focused();
        let style = Style::default()
            .fg(color(self.0.appearance.foreground))
            .bg(color(self.0.background));
        buffer.set_style(area.visible(), style);
        let text = owner.core.text(area.height as usize);
        // Canonical lines retain their terminal cell rows; no wrapping or replay parser.
        for (row, line) in text.lines().take(area.height as usize).enumerate() {
            let line_area = Placement::new(
                super::CellRect::new(area.x, area.y + row as i64, area.width, 1),
                area.visible(),
            );
            paint::text(line, line_area, buffer, style);
        }
        let cursor = if focused
            && owner.core.captured()
            && owner.core.scroll == 0
            && let Some(display) = &owner.core.display
            && display.cursor.visible
            && let Some(point) = area.point(display.cursor.x as usize, display.cursor.y as usize)
        {
            buffer[point].set_style(
                Style::default()
                    .bg(color(self.0.caret))
                    .fg(color(self.0.background)),
            );
            Some(point)
        } else {
            None
        };
        slots.native_cursor(cursor)?;
        slots.native_geometry(Box::new(Geometry {
            area,
            focused,
            source: self.0.source.clone(),
            controller: self.0.controller,
        }))?;
        slots.native_target("observe", area, self.0.tab_index, false)?;
        Ok(())
    }
    fn present(&self, state: &mut dyn Any, geometry: Box<dyn Any>) -> Option<NativeInput> {
        let owner = state.downcast_mut::<Owner>().unwrap();
        let geometry = *geometry.downcast::<Geometry>().unwrap();
        owner.core.presentation(true);
        owner
            .core
            .resize(geometry.area.width, geometry.area.height as u16);
        owner.core.capture(geometry.focused);
        owner.geometry = Some(geometry);
        owner.observation()
    }
    fn presentation(&self, state: &mut dyn Any, visible: bool) {
        if !visible {
            self.deactivate(state);
        }
    }
    fn deactivate(&self, state: &mut dyn Any) {
        let owner = state.downcast_mut::<Owner>().unwrap();
        owner.geometry = None;
        owner.core.presentation(false);
    }
}
pub(super) struct Component;
impl NativeComponent<dyn Widget> for Component {
    fn call(
        &self,
        call: ComponentCall<'_>,
        _: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<dyn Widget>>> {
        let output = reduce(call)?;
        let view = NativeView::<dyn Widget>::new(
            Rc::new(Recipe(output.frame.view.clone())),
            BTreeMap::from([("observe".into(), observation_schema())]),
            vec![],
        )?;
        Ok(output.map_view(|_| view))
    }
}
pub(super) fn register(registry: &mut NativeRegistry<dyn Widget>) {
    registry
        .register(maka_ui_runtime::terminal::IMPLEMENTATION, |_, _| {
            Ok(BTreeMap::from([(
                "TerminalView".into(),
                NativeExport {
                    contract: terminal_view_contract(),
                    value: NativeCapability::Component(Rc::new(Component)),
                },
            )]))
        })
        .expect("unique native terminal view");
}
