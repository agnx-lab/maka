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

use super::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn props() -> Value {
    json!({"accessibilityLabel":"Terminal", "source":"task-A","controller":Uuid::new_v4(),"acknowledgement":null,
        "output":{"mode":"pty","screen":"SHELL","scrollback":"one\ntwo\nthree","cols":40,"rows":6,"cursor":{"x":5,"y":0,"visible":true},"alternateScreen":true,"truncated":false,"redacted":false},
        "appearance":{"font":{"family":"monospace","fallbacks":[],"size":14,"lineHeight":1,"weight":400},"foreground":4294967295u32},
        "background":255,"caret":4294967295u32,"rows":6,"tabIndex":0})
}
fn fixture() -> (tokio::runtime::Runtime, Presenter, Rc<RefCell<Value>>) {
    let scene = Rc::new(RefCell::new(
        json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
            {"kind":"button","key":"back","label":"Controls"},
            {"kind":"component","key":"terminal","binding":"terminal","member":0,"props":props(),"on":{"observed":"terminal-observed"}}
        ]}),
    ));
    let (runtime, presenter) = layout::native(
        layout::View::InteractiveScene(scene.clone()),
        Rc::new(Cell::new(true)),
    );
    (runtime, presenter, scene)
}
fn settle(p: &mut Presenter) {
    for _ in 0..3 {
        draw(p, 40, 12);
    }
}
fn observed(p: &Presenter) -> Value {
    p.application.frame(&p.root).unwrap().state["observation"].clone()
}
fn ack(p: &mut Presenter, scene: &Rc<RefCell<Value>>) {
    let o = observed(p);
    scene.borrow_mut()["children"][1]["props"]["acknowledgement"] = json!({"owner":o["owner"],"controller":o["batch"]["controller"],"sequence":o["batch"]["sequence"],"accepted":true});
    let rev = p.application.frame(&p.root).unwrap().revision;
    p.application
        .dispatch(&p.root, rev, "back", json!({"kind":"click"}))
        .unwrap();
    settle(p);
}
fn mouse(kind: MouseEventKind, x: u16, y: u16, modifiers: KeyModifiers) -> Event {
    Event::Mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers,
    })
}
#[test]
fn terminal_resize_uses_allocated_rows_below_controls_in_a_short_surface() {
    let (_runtime, mut p, _scene) = fixture();
    for _ in 0..3 {
        draw(&mut p, 40, 4);
    }
    let target = p.scene.iter().find(|t| t.key == "observe").unwrap();
    assert_eq!(target.area.height, 3);
    assert_eq!(target.area.visible().height, 3);
    assert_eq!(observed(&p)["batch"]["size"], json!({"cols":40,"rows":3}));
    p.surface_closed(&p.surface(), Ok(())).unwrap();
    p.shutdown_settled().unwrap();
}
#[test]
fn copy_button_requires_original_physical_activation_and_successful_dispatch() {
    let scene = Rc::new(RefCell::new(
        json!({"kind":"button","key":"copy","label":"Copy","copy_text":"原文\nexact"}),
    ));
    let accept = Rc::new(Cell::new(true));
    let (_runtime, mut p) =
        layout::native(layout::View::ChangingScene(scene.clone()), accept.clone());
    draw(&mut p, 40, 4);
    let revision = p.application.frame(&p.root).unwrap().revision;
    p.application
        .dispatch(&p.root, revision, "copy", json!({"kind":"click"}))
        .unwrap();
    let mut copied = Vec::new();
    assert!(!p.flush_clipboard(&mut copied).unwrap());
    draw(&mut p, 40, 4);
    p.input_settled(key(KeyCode::Enter)).unwrap();
    assert!(p.flush_clipboard(&mut copied).unwrap());
    let mut expected = Vec::new();
    crate::terminal::copy(&mut expected, "原文\nexact").unwrap();
    assert_eq!(copied, expected);
    assert!(!p.flush_clipboard(&mut copied).unwrap());
    draw(&mut p, 40, 4);
    accept.set(false);
    assert!(p.input_settled(key(KeyCode::Enter)).is_err());
    assert!(!p.flush_clipboard(&mut copied).unwrap());
    accept.set(true);
    scene.borrow_mut()["disabled"] = json!(true);
    let revision = p.application.frame(&p.root).unwrap().revision;
    p.application
        .dispatch(&p.root, revision, "copy", json!({"kind":"click"}))
        .unwrap();
    draw(&mut p, 40, 4);
    p.input_settled(key(KeyCode::Enter)).unwrap();
    assert!(!p.flush_clipboard(&mut copied).unwrap());
    assert_eq!(copied, expected);
    p.surface_closed(&p.surface(), Ok(())).unwrap();
    p.shutdown_settled().unwrap();
}
#[test]
fn complete_large_copy_uses_the_original_physical_action_and_exact_osc52_bytes() {
    use base64::Engine;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let scene = Rc::new(RefCell::new(
        json!({"kind":"button","key":"copy","label":"Copy","copy_text":"initial"}),
    ));
    let (_runtime, mut p) = layout::native(
        layout::View::ChangingScene(scene.clone()),
        Rc::new(Cell::new(true)),
    );
    let text = format!("COPY START\n{}COPY END", "保留😀z\n".repeat(8_000));
    assert!(text.len() > 65_536);
    let completed = catch_unwind(AssertUnwindSafe(|| -> Result<Vec<u8>, String> {
        draw(&mut p, 40, 4);
        scene.borrow_mut()["copy_text"] = json!(text);
        let revision = p
            .application
            .frame(&p.root)
            .map_err(|e| e.to_string())?
            .revision;
        p.application
            .dispatch(&p.root, revision, "copy", json!({"kind":"click"}))
            .map_err(|e| e.to_string())?;
        let mut copied = Vec::new();
        assert!(!p.flush_clipboard(&mut copied).map_err(|e| e.to_string())?);
        draw(&mut p, 40, 4);
        p.input_settled(key(KeyCode::Enter))
            .map_err(|e| e.to_string())?;
        assert!(p.flush_clipboard(&mut copied).map_err(|e| e.to_string())?);
        assert!(!p.flush_clipboard(&mut copied).map_err(|e| e.to_string())?);
        Ok(copied)
    }));
    p.surface_closed(&p.surface(), Ok(())).unwrap();
    p.shutdown_settled().unwrap();
    let captured = completed.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert!(
        captured.is_ok(),
        "complete native copy failed: {}",
        captured
            .as_ref()
            .err()
            .map(String::as_str)
            .unwrap_or_default()
    );
    let expected = format!(
        "\x1b]52;c;{}\x07",
        base64::engine::general_purpose::STANDARD.encode(text.as_bytes())
    );
    assert!(
        captured.unwrap() == expected.as_bytes(),
        "complete original clipboard bytes changed"
    );
}

#[test]
fn terminal_native_focus_owns_control_c_tab_escape_and_returns_with_original_release_key() {
    let (_runtime, mut p, scene) = fixture();
    settle(&mut p);
    assert_eq!(p.focus.as_ref().unwrap().key, "back");
    assert!(!p.captures_keyboard().unwrap());
    p.input_settled(key(KeyCode::Tab)).unwrap();
    settle(&mut p);
    assert!(p.captures_keyboard().unwrap());
    p.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    assert_eq!(
        observed(&p)["batch"]["actions"][0],
        json!({"type":"key","key":"c","modifiers":["ctrl"]})
    );
    ack(&mut p, &scene);
    for (key_code, name) in [(KeyCode::Tab, "tab"), (KeyCode::Esc, "escape")] {
        p.input_settled(key(key_code)).unwrap();
        assert_eq!(observed(&p)["batch"]["actions"][0]["key"], name);
        ack(&mut p, &scene);
    }
    p.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('5'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    assert_eq!(p.focus.as_ref().unwrap().key, "back");
    assert!(!p.captures_keyboard().unwrap());
    p.surface_closed(&p.surface(), Ok(())).unwrap();
    p.shutdown_settled().unwrap();
}
#[test]
fn terminal_native_all_mouse_buttons_motion_and_shift_history_use_flushed_cell_coordinates() {
    let (_runtime, mut p, scene) = fixture();
    settle(&mut p);
    p.input_settled(key(KeyCode::Tab)).unwrap();
    settle(&mut p);
    let target = p
        .scene
        .iter()
        .find(|t| t.key == "observe")
        .unwrap()
        .area
        .visible();
    for (kind, event, button) in [
        (
            MouseEventKind::Down(MouseButton::Middle),
            "press",
            Some("middle"),
        ),
        (
            MouseEventKind::Up(MouseButton::Middle),
            "release",
            Some("middle"),
        ),
        (
            MouseEventKind::Drag(MouseButton::Right),
            "move",
            Some("right"),
        ),
        (
            MouseEventKind::Up(MouseButton::Right),
            "release",
            Some("right"),
        ),
        (MouseEventKind::Moved, "move", None),
    ] {
        p.input_settled(mouse(kind, target.x + 2, target.y + 1, KeyModifiers::NONE))
            .unwrap();
        let action = observed(&p)["batch"]["actions"][0].clone();
        assert_eq!(action["type"], "mouse");
        assert_eq!(action["event"], event);
        assert_eq!(action["x"], 2);
        assert_eq!(action["y"], 1);
        assert_eq!(action.get("button").and_then(Value::as_str), button);
        ack(&mut p, &scene);
    }
    p.input_settled(mouse(
        MouseEventKind::ScrollUp,
        target.x + 2,
        target.y + 1,
        KeyModifiers::SHIFT,
    ))
    .unwrap();
    let buffer = draw(&mut p, 40, 12);
    assert!(
        buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>()
            .contains("one")
    );
    assert!(observed(&p)["batch"].is_null());
    p.surface_closed(&p.surface(), Ok(())).unwrap();
    p.shutdown_settled().unwrap();
}

#[test]
fn terminal_readonly_canonical_screen_remains_keyboard_reachable_and_copyable() {
    let (_runtime, mut p, scene) = fixture();
    scene.borrow_mut()["children"][1]["props"]["controller"] = Value::Null;
    let revision = p.application.frame(&p.root).unwrap().revision;
    p.application
        .dispatch(&p.root, revision, "back", json!({"kind":"click"}))
        .unwrap();
    settle(&mut p);
    p.input_settled(key(KeyCode::Tab)).unwrap();
    settle(&mut p);
    assert_eq!(p.focus.as_ref().unwrap().key, "observe");
    assert!(!p.captures_keyboard().unwrap());
    let mut bytes = Vec::new();
    assert!(p.copy(&mut bytes).unwrap());
    assert!(String::from_utf8(bytes).unwrap().contains("U0hFTEw="));
    assert!(observed(&p)["batch"].is_null());
    p.surface_closed(&p.surface(), Ok(())).unwrap();
    p.shutdown_settled().unwrap();
}
