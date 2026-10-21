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

use super::Placement;
use super::editor::Owner as EditorOwner;
mod clipboard;
pub(super) mod paste;
struct Owner {
    edit: EditorOwner,
    files: paste::Paste,
    visible: bool,
    rows: Option<u16>,
    resize_drag: Option<(u16, u16, u16)>,
    resize: Option<Resize>,
    checkpoint: Option<maka_ui_runtime::checkpoint::EditorCheckpoint>,
}
#[derive(Clone, Copy)]
struct Resize {
    grip: ratatui::layout::Rect,
    rows: u16,
    maximum: u16,
}
struct Presented {
    editor: Geometry,
    resize: Option<Resize>,
}
impl Owner {
    fn checkpoint(&mut self, props: &TextareaProps) {
        let Some(policy) = &props.persistence else {
            if let Some(checkpoint) = &mut self.checkpoint {
                checkpoint
                    .reject_restore("editor persistence cannot be removed from its original mount");
            }
            return;
        };
        self.edit.refresh();
        let Some(checkpoint) = &mut self.checkpoint else {
            return;
        };
        checkpoint.poll(policy);
        let snapshot = self.edit.snapshot();
        if let Some(restore) = checkpoint.restoration(&snapshot.text.owner, policy) {
            let restored =
                maka_ui_runtime::checkpoint::selection_bytes(restore.text(), restore.snapshot())
                    .is_some_and(|(anchor, focus)| {
                        self.edit.editor.restore(restore.text(), anchor, focus)
                    });
            if restored {
                checkpoint.restored();
                self.edit.refresh();
            } else {
                checkpoint
                    .reject_restore("saved text or selection cannot be represented by this editor");
            }
        }
        let snapshot = self.edit.snapshot();
        checkpoint.save(&snapshot, self.edit.editor.text(), &policy.metadata);
    }
    fn checkpoint_observation(&mut self) -> Option<NativeInput> {
        let owner = self.edit.snapshot().text.owner;
        self.checkpoint
            .as_mut()?
            .observe(&owner)
            .map(|value| NativeInput {
                replaceable: false,
                key: "checkpoint".into(),
                value: serde_json::to_value(value).expect("native checkpoint observation"),
            })
    }
    fn persistence_ready(&self) -> bool {
        self.checkpoint
            .as_ref()
            .is_none_or(|checkpoint| checkpoint.accepts_input())
    }
}
#[cfg(test)]
pub(super) fn editor_owner_mut(state: &mut dyn std::any::Any) -> &mut EditorOwner {
    &mut state.downcast_mut::<Owner>().unwrap().edit
}
#[cfg(test)]
pub(super) fn editor_owner(state: &dyn std::any::Any) -> &EditorOwner {
    &state.downcast_ref::<Owner>().unwrap().edit
}
#[cfg(test)]
pub(super) fn clipboard_text(state: &mut dyn std::any::Any, text: String) {
    state
        .downcast_mut::<Owner>()
        .unwrap()
        .files
        .set_clipboard_text(text);
}
use super::{NativeInput, Slots, Widget, WidgetInput};
use crate::editor::{Editor, Geometry};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use maka_ui_runtime::{
    ComponentCall, ComponentOutput, NativeCapability, NativeComponent, NativeExport,
    NativeRegistry, NativeView, editor::*, textarea::*,
};
use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier, Style},
};
use std::{any::Any, collections::BTreeMap, rc::Rc, sync::Arc};

fn representable(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
}
fn color(color: maka_ui_runtime::appearance::Color) -> Color {
    let n = color.0;
    Color::Rgb((n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8)
}
struct Recipe {
    props: TextareaProps,
    file_request: Option<String>,
}
impl Widget for Recipe {
    fn publication_capacity(&self, state: &dyn Any) -> Vec<(&'static str, usize)> {
        let owner = state.downcast_ref::<Owner>().unwrap();
        let mut capacity = Vec::new();
        if !owner.edit.announced
            || self
                .props
                .command
                .as_ref()
                .is_some_and(|command| owner.edit.last_request.as_ref() != Some(command))
        {
            capacity.push(("observe", 4096));
        }
        if owner.files.will_accept(self.file_request.as_deref()) {
            capacity.extend([("file_observe", 65536); 2]);
        }
        capacity
    }

    fn cancel_pointer(&self, state: &mut dyn Any) {
        let owner = state.downcast_mut::<Owner>().unwrap();
        owner.resize_drag = None;
        owner.edit.editor.cancel_pointer();
    }
    fn create(&self, context: super::NativeContext) -> Box<dyn Any> {
        let checkpoint = self.props.persistence.as_ref().map(|policy| {
            maka_ui_runtime::checkpoint::EditorCheckpoint::new(
                context.checkpoint.clone(),
                policy,
                &context.resources,
                context.budget.clone(),
            )
        });
        let mut edit = EditorOwner::new(Editor::default(), &self.props.initial_text);
        edit.editor.set_inline_tokens(&self.props.inline_tokens);
        edit.clipboard(context.clone());
        Box::new(Owner {
            checkpoint,
            edit,
            files: paste::Paste::new(context),
            visible: true,
            rows: None,
            resize_drag: None,
            resize: None,
        })
    }
    fn commit(&self, state: &mut dyn Any) -> maka_ui_runtime::Result<Option<NativeInput>> {
        let owner = state
            .downcast_mut::<Owner>()
            .expect("terminal editor owner");
        owner
            .edit
            .editor
            .set_inline_tokens(&self.props.inline_tokens);
        owner.files.commit(
            self.file_request.as_deref(),
            self.props.file_command.as_ref(),
            owner.visible
                && !self.props.disabled
                && !self.props.read_only
                && self.props.file_paste.is_some(),
        );
        let changed = self
            .props
            .command
            .as_ref()
            .is_some_and(|command| owner.edit.last_request.as_ref() != Some(command));
        if changed {
            if owner.persistence_ready() {
                owner.edit.apply(self.props.command.as_ref().unwrap());
            } else {
                owner.edit.reject(
                    self.props.command.as_ref().unwrap(),
                    EditorConflict::Persistence,
                );
            }
        }
        owner
            .edit
            .poll_paste(owner.visible && !self.props.disabled && !self.props.read_only);
        owner.files.poll(
            &mut owner.edit.editor,
            self.props
                .file_paste
                .as_ref()
                .map(|policy| policy.context.as_str()),
        );
        let previous = owner.edit.snapshot();
        owner.checkpoint(&self.props);
        let edited = owner.edit.refresh() || previous != owner.edit.snapshot();
        if changed || edited || !owner.edit.announced {
            owner.edit.announced = true;
            Ok(Some(owner.edit.observation()))
        } else {
            Ok(owner
                .checkpoint_observation()
                .or_else(|| owner.files.observe()))
        }
    }
    fn observe(&self, state: &mut dyn Any) -> Option<NativeInput> {
        let owner = state.downcast_mut::<Owner>().unwrap();
        owner
            .edit
            .poll_paste(owner.visible && !self.props.disabled && !self.props.read_only);
        owner.files.poll(
            &mut owner.edit.editor,
            self.props
                .file_paste
                .as_ref()
                .map(|policy| policy.context.as_str()),
        );
        let previous = owner.edit.snapshot();
        let edited = owner.edit.refresh();
        owner.checkpoint(&self.props);
        if edited || previous != owner.edit.snapshot() {
            Some(owner.edit.observation())
        } else {
            owner
                .checkpoint_observation()
                .or_else(|| owner.files.observe())
        }
    }
    fn present_focus(&self, state: &mut dyn Any) -> bool {
        state
            .downcast_mut::<Owner>()
            .expect("terminal editor owner")
            .edit
            .focus
            .presented()
    }
    fn prepare_focus(&self, state: &mut dyn Any, visible: bool, _: u64) {
        state
            .downcast_mut::<Owner>()
            .expect("terminal editor owner")
            .edit
            .focus
            .reconcile(visible && !self.props.disabled && self.props.auto_focus);
    }
    fn input(
        &self,
        state: &mut dyn Any,
        _: &str,
        event: &Event,
    ) -> maka_ui_runtime::Result<WidgetInput> {
        let owner = state
            .downcast_mut::<Owner>()
            .expect("terminal editor owner");
        let read_only = self.props.read_only || !owner.persistence_ready();
        if self.props.disabled {
            return Ok(if matches!(event, Event::Key(_)) {
                WidgetInput::Unhandled
            } else {
                WidgetInput::Consumed(None)
            });
        }
        if self.props.terminal_rows.resizable && owner.visible {
            if let Event::Mouse(mouse) = event {
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(resize) = owner
                            .resize
                            .filter(|resize| resize.grip.contains((mouse.column, mouse.row).into()))
                        {
                            owner.edit.editor.cancel_pointer();
                            owner.edit.cancel_paste();
                            owner.resize_drag = Some((mouse.row, resize.rows, resize.maximum));
                            return Ok(WidgetInput::Consumed(None));
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left)
                    | MouseEventKind::Up(MouseButton::Left) => {
                        if let Some((start, rows, maximum)) = owner.resize_drag {
                            let rows = (i32::from(rows) + i32::from(mouse.row) - i32::from(start))
                                .clamp(i32::from(self.props.terminal_rows.min), i32::from(maximum))
                                as u16;
                            owner.edit.redraw |= owner.rows != Some(rows);
                            owner.rows = Some(rows);
                            if matches!(mouse.kind, MouseEventKind::Up(_)) {
                                owner.resize_drag = None;
                            }
                            return Ok(WidgetInput::Consumed(None));
                        }
                    }
                    _ => {}
                }
            }
        } else {
            owner.resize_drag = None;
        }
        if !read_only
            && owner.visible
            && let Event::Key(key) = event
            && key.modifiers.is_empty()
            && let Some(navigation) = &self.props.navigation
        {
            owner.edit.refresh();
            let observation = owner.edit.current_observation();
            let navigation_key = match key.code {
                KeyCode::Up => Some(EditorNavigationKey::Up),
                KeyCode::Down => Some(EditorNavigationKey::Down),
                KeyCode::Enter => Some(EditorNavigationKey::Enter),
                KeyCode::Tab => Some(EditorNavigationKey::Tab),
                KeyCode::Esc => Some(EditorNavigationKey::Escape),
                _ => None,
            };
            if navigation.matches(&observation)
                && let Some(key_value) = navigation_key
            {
                return Ok(if key.kind == KeyEventKind::Release {
                    WidgetInput::Consumed(None)
                } else {
                    WidgetInput::Emitted(NativeInput {
                        key: "navigate".into(),
                        value: serde_json::to_value(EditorNavigated {
                            key: key_value,
                            observation,
                        })
                        .unwrap(),
                        replaceable: false,
                    })
                });
            }
        }
        if !read_only
            && owner.visible
            && let Event::Key(key) = event
            && key.code == KeyCode::Char('v')
            && matches!(key.modifiers, KeyModifiers::CONTROL | KeyModifiers::SUPER)
        {
            if self.props.file_paste.is_none() {
                if key.kind == KeyEventKind::Press {
                    owner.edit.paste(false);
                }
                return Ok(WidgetInput::Consumed(None));
            }
            return Ok(if key.kind == KeyEventKind::Press {
                owner
                    .files
                    .stage_clipboard(&owner.edit.editor, self.props.file_paste.as_ref().unwrap())
                    .map_or(WidgetInput::Consumed(None), WidgetInput::Emitted)
            } else {
                WidgetInput::Consumed(None)
            });
        }
        // An empty editor yields upward navigation to its declared scope.
        // Nonempty text, including a cursor on the first line, stays in Editor.
        if owner.edit.editor.text().is_empty()
            && matches!(event, Event::Key(key) if key.code == KeyCode::Up && key.modifiers.is_empty())
        {
            return Ok(WidgetInput::Navigate);
        }
        if !read_only
            && let Event::Key(key) = event
            && key.code == KeyCode::Enter
            && match self.props.submit {
                Some(SubmitKey::Enter) => key.modifiers.is_empty(),
                Some(SubmitKey::ControlEnter | SubmitKey::PrimaryEnter) => {
                    key.modifiers == KeyModifiers::CONTROL
                }
                None => false,
            }
        {
            if key.kind != KeyEventKind::Press || owner.files.pending() {
                return Ok(WidgetInput::Consumed(None));
            }
            owner.edit.refresh();
            let mut observation = owner.edit.observation();
            observation.key = "submit".into();
            return Ok(WidgetInput::Emitted(observation));
        }
        let handled = match event {
            Event::Key(key) => {
                let navigation = matches!(
                    key.code,
                    KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::Up
                        | KeyCode::Down
                        | KeyCode::Home
                        | KeyCode::End
                ) || key.code == KeyCode::Char('a')
                    && key.modifiers.contains(KeyModifiers::CONTROL);
                if read_only && !navigation {
                    false
                } else {
                    owner.edit.editor.key(*key)
                }
            }
            Event::Paste(text) if !read_only => {
                if owner.visible
                    && let Some(policy) = &self.props.file_paste
                    && let Some(input) = owner.files.stage(text, policy)
                {
                    return Ok(WidgetInput::Emitted(input));
                }
                owner.edit.editor.insert(text)
            }
            Event::Mouse(mouse) => owner
                .edit
                .presented
                .as_ref()
                .is_some_and(|geometry| owner.edit.editor.mouse_presented(*mouse, geometry)),
            _ => false,
        };
        let observed = handled && owner.edit.refresh();
        owner.checkpoint(&self.props);
        Ok(if handled {
            WidgetInput::Consumed(observed.then(|| owner.edit.observation()))
        } else {
            WidgetInput::Unhandled
        })
    }
    fn pointer_capture(&self, state: &dyn Any) -> bool {
        let owner = state.downcast_ref::<Owner>().unwrap();
        owner.resize_drag.is_some() || owner.edit.editor.dragging()
    }
    fn handles_tab(&self, state: &dyn Any, _: &str) -> bool {
        let owner = state.downcast_ref::<Owner>().unwrap();
        !self.props.disabled
            && !self.props.read_only
            && owner.visible
            && owner.persistence_ready()
            && self
                .props
                .navigation
                .as_ref()
                .is_some_and(|navigation| navigation.matches(&owner.edit.current_observation()))
    }
    fn capture(
        &self,
        state: &dyn Any,
        target: &maka_ui_runtime::actions::CaptureTarget,
    ) -> maka_ui_runtime::Result<Arc<dyn maka_capabilities::call::ReadResource>> {
        let owner = state
            .downcast_ref::<Owner>()
            .expect("terminal editor owner");
        if target.method() == "file" {
            owner.files.capture(target.input())
        } else {
            owner.edit.capture(target)
        }
    }

    fn layout(&self, _: &Slots<'_>) -> maka_ui_runtime::Result<super::TerminalLayout> {
        // The original editor scrolls within its allocated viewport. Shrinking
        // must resize that viewport, not merely clip its last visible rows.
        Ok(super::TerminalLayout {
            shrink: true,
            ..Default::default()
        })
    }
    fn measure(&self, slots: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        let owner = slots.native_state::<Owner>()?;
        let inset = if self.props.appearance.border_width > 0.0 {
            2
        } else {
            0
        };
        let rows = self.props.terminal_rows;
        let vertical_inset = if inset == 0 && rows.resizable {
            1
        } else {
            inset
        };
        let maximum = rows.max_viewport_fraction.map_or(rows.max, |fraction| {
            ((f64::from(slots.viewport().height) * fraction) as u16).clamp(rows.min, rows.max)
        });
        Ok((
            width,
            owner
                .rows
                .filter(|_| rows.resizable)
                .map_or_else(
                    || {
                        owner
                            .edit
                            .editor
                            .rows(width.saturating_sub(inset))
                            .clamp(rows.min, maximum)
                    },
                    |rows_override| {
                        rows_override.clamp(
                            rows.min,
                            slots
                                .viewport()
                                .height
                                .saturating_sub(vertical_inset)
                                .max(rows.min),
                        )
                    },
                )
                .saturating_add(vertical_inset)
                .into(),
        ))
    }
    fn paint(
        &self,
        slots: &mut Slots<'_>,
        area: Placement,
        buffer: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        if area.is_empty() {
            if !self.props.disabled {
                slots.native_target("observe", area, self.props.tab_index, false)?;
            }
            return Ok(());
        }
        let owner = slots.native_state::<Owner>()?;
        let focused = slots.focused();
        let appearance = &self.props.appearance;
        let terminal = appearance.terminal.as_ref();
        let foreground = terminal.map_or_else(
            || color(appearance.text.foreground),
            |p| super::paint::color(p.foreground),
        );
        let background = terminal.map_or_else(
            || color(appearance.background),
            |p| super::paint::color(p.background),
        );
        let selection = terminal.map_or_else(
            || color(appearance.selection),
            |p| super::paint::color(p.selection),
        );
        let selection_text = terminal.map_or(foreground, |p| super::paint::color(p.selection_text));
        let muted = terminal.map_or_else(
            || color(appearance.muted_foreground),
            |p| super::paint::color(p.subtle),
        );
        let border = terminal.map_or_else(
            || {
                color(if focused {
                    appearance.focused_border
                } else {
                    appearance.border
                })
            },
            |p| super::paint::color(if focused { p.accent } else { p.border }),
        );
        buffer.set_style(
            area.visible(),
            Style::default().bg(background).fg(foreground),
        );
        let inner = if appearance.border_width > 0.0 {
            super::border::paint(
                buffer,
                area,
                border,
                if appearance.ascii {
                    maka_ui_runtime::scene::terminal::BorderStyle::Ascii
                } else if appearance.radius > 0.0 {
                    maka_ui_runtime::scene::terminal::BorderStyle::Rounded
                } else {
                    maka_ui_runtime::scene::terminal::BorderStyle::Square
                },
            );
            super::border::inner(area)
        } else {
            let mut inner = area;
            if self.props.terminal_rows.resizable {
                inner.bounds.height = inner.height.saturating_sub(1);
            }
            inner
        };
        let geometry = owner.edit.editor.paint_prepared(
            inner,
            buffer,
            focused,
            Style::default().fg(foreground).bg(background),
            Style::default().bg(selection).fg(selection_text),
            Style::default().add_modifier(Modifier::REVERSED),
        );
        geometry.paint_scrollbar(
            buffer,
            Style::default()
                .fg(terminal.map_or_else(
                    || color(appearance.border),
                    |p| super::paint::color(p.border),
                ))
                .bg(background),
            Style::default()
                .fg(terminal.map_or_else(
                    || color(appearance.scrollbar.thumb.color),
                    |p| super::paint::color(p.scrollbar),
                ))
                .bg(background),
            appearance.ascii,
        );
        if owner.edit.editor.text().is_empty() {
            super::paint::text(
                &self.props.placeholder,
                inner,
                buffer,
                Style::default().fg(muted),
            );
        }
        if let Some(error) = owner.edit.editor.error {
            let error_area = ratatui::layout::Rect::new(
                inner.visible().x,
                inner.visible().bottom().saturating_sub(1),
                inner.visible().width,
                1,
            );
            buffer.set_stringn(
                error_area.x,
                error_area.y,
                super::editor::error_message(error),
                usize::from(error_area.width),
                Style::default().fg(Color::Red).bg(background),
            );
        }
        let autofocus = owner.edit.focus.pending();
        let resize =
            if self.props.terminal_rows.resizable && area.width >= 3 && !self.props.disabled {
                area.point(
                    usize::from(area.width - 3),
                    area.height.saturating_sub(1) as usize,
                )
                .and_then(|point| {
                    let grip = ratatui::layout::Rect::new(point.x, point.y, 3, 1);
                    (grip.intersection(area.visible()) == grip).then(|| {
                        buffer.set_string(
                            point.x,
                            point.y,
                            if appearance.ascii { "[=]" } else { " ↕ " },
                            Style::default().fg(border).bg(background),
                        );
                        Resize {
                            grip,
                            rows: inner.height.min(u32::from(u16::MAX)) as u16,
                            maximum: slots
                                .viewport()
                                .height
                                .saturating_sub(if appearance.border_width > 0.0 { 2 } else { 1 })
                                .max(self.props.terminal_rows.min),
                        }
                    })
                })
            } else {
                None
            };
        if !self.props.disabled {
            slots.native_cursor(geometry.cursor_position())?;
        }
        slots.native_geometry(Box::new(Presented {
            editor: geometry,
            resize,
        }))?;
        if !self.props.disabled {
            slots.native_target("observe", area, self.props.tab_index, autofocus)?;
        }
        Ok(())
    }
    fn present(&self, state: &mut dyn Any, geometry: Box<dyn Any>) -> Option<NativeInput> {
        let owner = state
            .downcast_mut::<Owner>()
            .expect("terminal editor owner");
        let geometry = *geometry.downcast::<Presented>().expect("editor geometry");
        owner.resize = geometry.resize;
        owner.edit.editor.present_geometry(geometry.editor.clone());
        owner.edit.presented = Some(geometry.editor);
        None
    }
    fn take_redraw(&self, state: &mut dyn Any) -> bool {
        let owner = state.downcast_mut::<Owner>().unwrap();
        std::mem::take(&mut owner.edit.redraw)
    }
    fn cancel_paste(&self, state: &mut dyn Any) {
        let owner = state.downcast_mut::<Owner>().unwrap();
        owner.edit.cancel_paste();
    }
    fn editing_menu(&self, state: &dyn Any) -> Option<(TextareaMenu, EditorSnapshot, [bool; 4])> {
        let owner = state.downcast_ref::<Owner>().unwrap();
        if self.props.disabled || !owner.visible {
            return None;
        }
        let menu = self.props.menu.clone()?;
        let selected = !owner.edit.editor.selected_text().is_empty();
        let editable = !self.props.read_only && owner.persistence_ready();
        Some((
            menu,
            owner.edit.snapshot(),
            [
                selected,
                selected && editable,
                editable,
                !owner.edit.editor.text().is_empty(),
            ],
        ))
    }
    fn copy(&self, state: &dyn Any, _: bool) -> maka_ui_runtime::Result<Option<String>> {
        let owner = state.downcast_ref::<Owner>().unwrap();
        if !owner.visible || self.props.disabled {
            return Ok(None);
        }
        let text = owner.edit.editor.selected_text();
        Ok((!text.is_empty()).then(|| text.to_owned()))
    }
    fn deactivate(&self, state: &mut dyn Any) {
        let owner = state.downcast_mut::<Owner>().unwrap();
        owner.edit.refresh();
        if let Some(checkpoint) = &mut owner.checkpoint {
            if let Some(policy) = &self.props.persistence {
                checkpoint.save(
                    &owner.edit.snapshot(),
                    owner.edit.editor.text(),
                    &policy.metadata,
                );
            }
            checkpoint.close();
        }
        self.presentation(state, false);
    }
    fn presentation(&self, state: &mut dyn Any, visible: bool) {
        let owner = state.downcast_mut::<Owner>().unwrap();
        owner.visible = visible;
        if !visible {
            owner.resize = None;
            owner.resize_drag = None;
            owner.edit.invalidate_geometry();
            owner.files.suspend();
        }
    }
}
struct Component;
impl NativeComponent<dyn Widget> for Component {
    fn capture(
        &self,
        _: maka_ui_runtime::actions::CaptureCall<'_>,
    ) -> maka_ui_runtime::Result<maka_ui_runtime::actions::CapturePlan> {
        Ok(maka_ui_runtime::actions::CapturePlan::Native)
    }
    fn call(
        &self,
        call: ComponentCall<'_>,
        _: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<dyn Widget>>> {
        let output = maka_ui_runtime::textarea::reduce(call)?;
        let props = output.frame.view.props.clone();
        let file_request = output.frame.view.file_request.clone();
        if !props.soft_wrap {
            return Err(maka_ui_runtime::Error::Invalid(
                "terminal editor requires soft wrapping".into(),
            ));
        }
        if !representable(&props.initial_text) {
            return Err(maka_ui_runtime::Error::Invalid(
                "initial text cannot be represented in terminal editor".into(),
            ));
        }
        let view = NativeView::<dyn Widget>::new(
            Rc::new(Recipe {
                props,
                file_request,
            }),
            BTreeMap::from([
                ("observe".into(), observation_schema()),
                (
                    "checkpoint".into(),
                    maka_ui_runtime::checkpoint::observation_schema(),
                ),
                ("submit".into(), observation_schema()),
                ("navigate".into(), navigation_schema()),
                ("file_choose".into(), maka_ui_runtime::file::choose_schema()),
                ("file_observe".into(), file_paste_schema()),
            ]),
            vec![],
        )?;
        Ok(output.map_view(|_| view))
    }
}
pub(super) fn register(registry: &mut NativeRegistry<dyn Widget>) {
    registry
        .register(maka_ui_runtime::textarea::IMPLEMENTATION, |_, _| {
            Ok(BTreeMap::from([(
                "Textarea".into(),
                NativeExport {
                    contract: textarea_contract(),
                    value: NativeCapability::Component(Rc::new(Component)),
                },
            )]))
        })
        .expect("unique terminal textarea");
}
