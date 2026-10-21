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
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn props(source: &str) -> Value {
    json!({"source":source,"appearance":{"font":{"family":"monospace","fallbacks":[],"size":16,"lineHeight":1.5,"weight":400},"foreground":0x242724ffu32},"selection":0x4466ffffu32,"disabled":false})
}
fn native(source: &str) -> (tokio::runtime::Runtime, Presenter, Rc<RefCell<Value>>) {
    let props = Rc::new(RefCell::new(props(source)));
    let (runtime, presenter) = layout::native(
        layout::View::StandaloneSource(props.clone()),
        Rc::new(Cell::new(true)),
    );
    (runtime, presenter, props)
}
fn select_all(presenter: &mut Presenter) {
    presenter
        .input_settled(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        )))
        .unwrap();
}
fn copied(presenter: &mut Presenter) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    if !presenter.copy(&mut bytes).unwrap() {
        return None;
    }
    assert!(bytes.starts_with(b"\x1b]52;c;"));
    assert_eq!(bytes.last(), Some(&7));
    Some(STANDARD.decode(&bytes[7..bytes.len() - 1]).unwrap())
}
fn refresh(presenter: &mut Presenter) {
    let revision = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .revision;
    presenter
        .application
        .dispatch(&presenter.root, revision, "back", json!({"kind":"click"}))
        .unwrap();
    presenter.settle().unwrap();
}
fn close(presenter: &mut Presenter) {
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
fn mouse(presenter: &mut Presenter, kind: MouseEventKind, x: u16, y: u16) {
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
}
#[test]
fn literal_source_displays_safely_and_copies_every_original_control_and_utf8_byte() {
    let source = "  **literal** <tag>\r\n\t中😀\r\0\x1b[31m e\u{301}\n\n  ";
    let (_runtime, mut presenter, _) = native(source);
    assert_eq!(copied(&mut presenter), None);
    let buffer = draw(&mut presenter, 40, 8);
    assert!(
        !buffer
            .content
            .iter()
            .any(|cell| cell.symbol().chars().any(char::is_control))
    );
    assert_eq!(buffer[(4, 1)].symbol(), "中");
    let geometry = |presenter: &Presenter| {
        let owner = presenter.resources.owners[&presenter.root.id()]
            .state
            .downcast_ref::<super::super::source::Owner>()
            .unwrap();
        owner
            .document
            .borrow()
            .glyphs
            .as_ref()
            .unwrap()
            .layout
            .clone()
    };
    let original = geometry(&presenter);
    draw(&mut presenter, 40, 8);
    let repeated = geometry(&presenter);
    let super::super::selection::Layout::Literal(first) = &original else {
        panic!("literal geometry")
    };
    let super::super::selection::Layout::Literal(second) = &repeated else {
        panic!("literal geometry")
    };
    assert!(std::sync::Arc::ptr_eq(first, second));
    draw(&mut presenter, 20, 8);
    let resized = geometry(&presenter);
    let super::super::selection::Layout::Literal(next) = &resized else {
        panic!("literal geometry")
    };
    assert!(!std::sync::Arc::ptr_eq(first, next));
    select_all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), source.as_bytes());
    close(&mut presenter);
}

#[test]
fn retained_source_layout_leaves_native_capacity_available_and_matches_rebuilt_output() {
    use maka_ui_runtime::budget::{BudgetLimits, Kind};

    let source = "Literal **text** 中😀\nsecond line";
    let (_runtime, mut presenter, _) = native(source);
    let budget = presenter.resources.budget.clone();
    let native_usage = budget.usage();
    let cold = draw(&mut presenter, 24, 5);
    assert_eq!(cold[(0, 0)].symbol(), "L");
    assert_eq!(budget.usage(), native_usage);
    let native_capacity = budget
        .reserve(
            Kind::Encoded,
            BudgetLimits::default().encoded_bytes - native_usage.0,
        )
        .expect("retained text must leave native admission available");
    assert_eq!(draw(&mut presenter, 24, 5), cold);

    // A new immutable recipe starts cold while keeping the original document owner.
    refresh(&mut presenter);
    assert_eq!(draw(&mut presenter, 24, 5), cold);
    select_all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), source.as_bytes());
    close(&mut presenter);
    drop(native_capacity);
    assert_eq!(budget.usage(), (0, 0, 0));
}
#[test]
fn append_preserves_literal_byte_endpoints_even_when_graphemes_or_crlf_merge() {
    for (before, after) in [("e", "e\u{301}"), ("👩", "👩\u{200d}💻"), ("x\r", "x\r\n")] {
        let (_runtime, mut presenter, props) = native(before);
        draw(&mut presenter, 20, 4);
        select_all(&mut presenter);
        let identity = presenter.resources.owners[&presenter.root.id()]
            .identity
            .clone();
        props.borrow_mut()["source"] = json!(after);
        refresh(&mut presenter);
        assert_eq!(copied(&mut presenter).unwrap(), before.as_bytes());
        draw(&mut presenter, 20, 4);
        assert_eq!(copied(&mut presenter).unwrap(), before.as_bytes());
        assert!(Rc::ptr_eq(
            &identity,
            &presenter.resources.owners[&presenter.root.id()].identity
        ));
        select_all(&mut presenter);
        assert_eq!(copied(&mut presenter).unwrap(), after.as_bytes());
        close(&mut presenter);
    }
}
#[test]
fn replacement_disabled_and_owner_replacement_revoke_selection_and_drag() {
    let (_runtime, mut presenter, props) = native("original");
    draw(&mut presenter, 20, 4);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    assert!(presenter.drag.is_some());
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        4,
        0,
    );
    assert_eq!(copied(&mut presenter).unwrap(), b"orig");
    props.borrow_mut()["source"] = json!("replacement");
    refresh(&mut presenter);
    assert!(presenter.drag.is_none());
    assert_eq!(copied(&mut presenter), None);
    draw(&mut presenter, 20, 4);
    select_all(&mut presenter);
    props.borrow_mut()["disabled"] = json!(true);
    refresh(&mut presenter);
    assert_eq!(copied(&mut presenter), None);
    draw(&mut presenter, 20, 4);
    assert!(presenter.focus.is_none());
    props.borrow_mut()["disabled"] = json!(false);
    refresh(&mut presenter);
    draw(&mut presenter, 20, 4);
    select_all(&mut presenter);
    let mount = presenter.root.clone();
    presenter.resources.close(&mount).unwrap();
    presenter.sync_native().unwrap();
    assert_eq!(copied(&mut presenter), None);
    close(&mut presenter);
}
#[test]
fn stale_unflushed_or_preappend_pointer_geometry_cannot_select_new_source() {
    let (_runtime, mut presenter, props) = native("abc");
    let area = Rect::new(0, 0, 20, 4);
    let mut buffer = Buffer::empty(area);
    let _discarded = presenter.paint(area, &mut buffer).unwrap();
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    assert!(presenter.drag.is_none());
    draw(&mut presenter, 20, 4);
    props.borrow_mut()["source"] = json!("abcdef");
    refresh(&mut presenter);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    assert!(presenter.drag.is_none());
    draw(&mut presenter, 20, 4);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        6,
        0,
    );
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 6, 0);
    assert_eq!(copied(&mut presenter).unwrap(), b"abcdef");
    assert!(presenter.drag.is_none());
    close(&mut presenter);
}
#[test]
fn copy_writer_failure_preserves_selection_and_application_owner() {
    struct Refused;
    impl std::io::Write for Refused {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected copy refusal"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let (_runtime, mut presenter, _) = native("selected");
    draw(&mut presenter, 20, 4);
    select_all(&mut presenter);
    assert!(presenter.copy(&mut Refused).is_err());
    assert!(presenter.surface.is_live());
    assert_eq!(copied(&mut presenter).unwrap(), b"selected");
    close(&mut presenter);
}
#[test]
fn complete_multibyte_sources_copy_without_truncation() {
    for source in ["中".repeat(21845) + "x", "中".repeat(21846) + "😀tail"] {
        let (_runtime, mut presenter, _) = native(&source);
        draw(&mut presenter, 20, 4);
        select_all(&mut presenter);
        assert_eq!(copied(&mut presenter).unwrap(), source.as_bytes());
        close(&mut presenter);
    }
}

#[test]
fn retiring_a_captured_source_releases_its_original_mount_reference() {
    let (_runtime, mut presenter, _) = native("original");
    draw(&mut presenter, 20, 4);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    assert!(presenter.drag.is_some());
    presenter.retire().unwrap();
    assert!(presenter.drag.is_none());
    assert_eq!(copied(&mut presenter), None);
    close(&mut presenter);
}
#[test]
fn first_shift_click_extends_the_original_caret_without_editing_literal_source() {
    let (_runtime, mut presenter, _) = native("中文😀 raw");
    draw(&mut presenter, 20, 4);
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: 0,
            modifiers: KeyModifiers::SHIFT,
        }))
        .unwrap();
    assert_eq!(copied(&mut presenter).unwrap(), "中文".as_bytes());
    close(&mut presenter);
}
