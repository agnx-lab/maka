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
fn source(key: &str, text: &str) -> Value {
    json!({"kind":"component","key":key,"binding":"source","member":0,"on":{},"props":{
        "source":text,"appearance":{"font":{"family":"monospace","fallbacks":[],"size":16,"lineHeight":1.5,"weight":400},"foreground":0x242724ffu32},"selection":0x4466ffffu32,"disabled":false}})
}
fn props(children: Vec<Value>) -> Value {
    json!({"content":{"kind":"stack","key":"reading","direction":"column","gap":0,"padding":0,"background":0,"scroll":"manual","children":children},"command":null})
}
fn native(children: Vec<Value>) -> (tokio::runtime::Runtime, Presenter, Rc<RefCell<Value>>) {
    let props = Rc::new(RefCell::new(props(children)));
    let (runtime, presenter) = layout::native(
        layout::View::Reading(props.clone()),
        Rc::new(Cell::new(true)),
    );
    (runtime, presenter, props)
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
fn all(presenter: &mut Presenter) {
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
    Some(STANDARD.decode(&bytes[7..bytes.len() - 1]).unwrap())
}
fn state(presenter: &Presenter) -> &super::super::scroll::SceneState {
    presenter.resources.owners[&presenter.root.id()]
        .state
        .downcast_ref()
        .unwrap()
}
fn observation(presenter: &Presenter) -> Value {
    presenter.application.frame(&presenter.root).unwrap().state["observation"].clone()
}
fn refresh(presenter: &mut Presenter) {
    presenter.settle().unwrap();
    let value = super::super::reading::current_observation(
        &*presenter.resources.owners[&presenter.root.id()].state,
    );
    let mount = presenter.root.clone();
    let revision = presenter.application.frame(&mount).unwrap().revision;
    presenter
        .application
        .dispatch(&mount, revision, "__reading_observation", value)
        .unwrap();
    presenter.settle().unwrap();
}
fn close(presenter: &mut Presenter) {
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
#[test]
fn wheel_batch_scrolls_immediately_and_publishes_only_its_final_observation() {
    let lines = (0..100).map(|n| format!("row {n}\n")).collect::<String>();
    let (_runtime, mut presenter, _) = native(vec![source("document", &lines)]);
    draw(&mut presenter, 30, 12);
    presenter.settle().unwrap();
    let before = observation(&presenter);
    for _ in 0..10 {
        presenter
            .wheel(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 10,
                row: 5,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
    }
    assert_eq!(state(&presenter).scrolls["reading"].offset, 10);
    assert_eq!(
        observation(&presenter),
        before,
        "local wheel movement does not run a reducer per tick"
    );
    draw(&mut presenter, 30, 12);
    presenter.sync_native().unwrap();
    presenter.settle().unwrap();
    let after = observation(&presenter);
    assert_eq!(after["scroll"]["direction"], "down");
    assert_eq!(after["viewport"]["atStart"], false);
    assert_eq!(
        after["viewport"]["readingRevision"],
        after["readingRevision"]
    );
    close(&mut presenter);
}

#[test]
fn wheel_above_a_short_page_reports_the_original_reading_boundary() {
    let (_runtime, mut presenter, _) = native(vec![source("one", "first")]);
    draw(&mut presenter, 30, 12);
    mouse(&mut presenter, MouseEventKind::ScrollUp, 10, 8);
    draw(&mut presenter, 30, 12);
    presenter.sync_native().unwrap();
    let observed = observation(&presenter);
    assert_eq!(observed["scroll"]["direction"], "up");
    assert_eq!(observed["viewport"]["atStart"], true);
    assert_eq!(observed["viewport"]["atEnd"], true);
    assert_eq!(
        observed["viewport"]["readingRevision"],
        observed["readingRevision"]
    );
    close(&mut presenter);
}
#[test]
fn selected_child_follows_original_native_focus_and_only_committed_group_geometry() {
    let group = |key: &str, child| json!({"kind":"stack","key":key,"direction":"column","gap":0,"padding":0,"background":0,"children":[child]});
    let (_runtime, mut presenter, props) = native(vec![
        group("message-a", source("one", "first")),
        group("message-b", source("two", "second")),
    ]);
    draw(&mut presenter, 20, 4);
    assert!(observation(&presenter)["selectedChildKey"].is_null());
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        1,
        0,
    );
    assert_eq!(observation(&presenter)["selectedChildKey"], "message-a");
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        3,
        1,
    );
    assert_eq!(observation(&presenter)["selectedChildKey"], "message-b");
    assert_eq!(copied(&mut presenter).unwrap(), b"irst\nsec");
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 3, 1);
    let scope = state(&presenter).reading.as_ref().unwrap().scope.clone();
    scope.suspend();
    assert_eq!(scope.selected_child().as_deref(), Some("message-b"));
    draw(&mut presenter, 20, 4);
    props.borrow_mut()["content"]["children"][1]["key"] = json!("message-renamed");
    refresh(&mut presenter);
    let area = Rect::new(0, 0, 20, 4);
    let mut buffer = Buffer::empty(area);
    drop(presenter.paint(area, &mut buffer).unwrap());
    assert_eq!(scope.selected_child().as_deref(), Some("message-b"));
    draw(&mut presenter, 20, 4);
    presenter.sync_native().unwrap();
    assert_eq!(
        observation(&presenter)["selectedChildKey"],
        "message-renamed"
    );
    props.borrow_mut()["content"]["children"]
        .as_array_mut()
        .unwrap()
        .pop();
    refresh(&mut presenter);
    draw(&mut presenter, 20, 4);
    presenter.sync_native().unwrap();
    assert!(observation(&presenter)["selectedChildKey"].is_null());
    close(&mut presenter);
}
#[test]
fn responsive_reading_uses_inner_width_and_preserves_source_owners_and_scroll() {
    let content = json!({"kind":"stack","key":"reading","direction":"column","gap":0,"padding":2,"background":0,
        "wide":{"min_width":100,"direction":"row","children":{"list":{"basis_fraction":{"numerator":34,"denominator":100}}}},
        "children":[
            {"kind":"stack","key":"hidden","direction":"column","gap":0,"padding":0,"background":0,"visible":false,"children":[],"layout":{"basis":40}},
            {"kind":"stack","key":"list","direction":"column","gap":0,"padding":0,"background":0,"scroll":"manual",
                "layout":{"basis_fraction":{"numerator":1,"denominator":3,"min":1,"max":6}},"children":[source("results", &(0..60).map(|i| format!("result {i}")).collect::<Vec<_>>().join("\n"))]},
            {"kind":"stack","key":"divider","direction":"column","gap":0,"padding":0,"background":0xffffff,"children":[],"layout":{"basis":1}},
            {"kind":"stack","key":"preview","direction":"column","gap":0,"padding":0,"background":0,"scroll":"manual",
                "layout":{"basis":0,"grow":1},"children":[source("document", "original preview source with wrapping bytes")]}]});
    let (_runtime, mut presenter) = layout::native(
        layout::View::SourceScene(Rc::new(RefCell::new(content))),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 103, 28);
    let document = presenter
        .application
        .child_mount(&presenter.root, "document")
        .unwrap()
        .clone();
    let owner = presenter.resources.owners[&document.id()].identity.clone();
    mouse(&mut presenter, MouseEventKind::ScrollDown, 3, 3);
    let offset = state(&presenter).scrolls["list"].offset;
    assert!(offset > 0);
    for (width, height) in [(104, 28), (103, 28), (104, 13), (103, 13), (140, 28)] {
        draw(&mut presenter, width, height);
        let list = state(&presenter).scrolls["list"]
            .presented
            .as_ref()
            .unwrap()
            .area;
        let preview = state(&presenter).scrolls["preview"]
            .presented
            .as_ref()
            .unwrap()
            .area;
        let inner_width = width - 4;
        let inner_height = u32::from(height - 4);
        if inner_width >= 100 {
            assert_eq!(list.width, inner_width * 34 / 100);
            assert_eq!(list.height, inner_height);
            assert_eq!(preview.x, 3 + i64::from(list.width));
            assert_eq!(preview.y, 2);
        } else {
            assert_eq!(list.width, inner_width);
            assert_eq!(list.height, (inner_height / 3).clamp(1, 6));
            assert_eq!(preview.x, 2);
            assert_eq!(preview.y, 3 + i64::from(list.height));
        }
        assert_eq!(state(&presenter).scrolls["list"].offset, offset);
        assert_eq!(
            presenter
                .application
                .child_mount(&presenter.root, "document")
                .unwrap(),
            &document
        );
        assert!(Rc::ptr_eq(
            &owner,
            &presenter.resources.owners[&document.id()].identity
        ));
    }
    close(&mut presenter);
}
#[test]
fn responsive_scroll_keeps_offset_across_rule_separated_wide_layout() {
    let content = json!({"kind":"stack","key":"reading","direction":"column","gap":0,"padding":0,"background":0,"scroll":"manual",
        "wide":{"min_width":40,"direction":"row","scroll":"none","children":{"divider":{"basis":1},"document":{"grow":1,"basis":0}}},
        "children":[
            {"kind":"stack","key":"divider","direction":"row","gap":0,"padding":0,"background":0,"layout":{"basis":0},"children":[
                {"kind":"rule","key":"rule","direction":"column","color":16777215,"ascii":false}]},
            source("document", &(0..30).map(|i| format!("row {i}")).collect::<Vec<_>>().join("\n"))]});
    let (_runtime, mut presenter) = layout::native(
        layout::View::SourceScene(Rc::new(RefCell::new(content))),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 30, 8);
    let document = presenter
        .application
        .child_mount(&presenter.root, "document")
        .unwrap()
        .clone();
    mouse(&mut presenter, MouseEventKind::ScrollDown, 3, 3);
    draw(&mut presenter, 30, 8);
    let offset = state(&presenter).scrolls["reading"].offset;
    assert!(offset > 0);
    let wide = draw(&mut presenter, 40, 8);
    for y in 0..8 {
        assert_eq!(wide[(0, y)].symbol(), "│");
    }
    mouse(&mut presenter, MouseEventKind::ScrollDown, 3, 3);
    draw(&mut presenter, 30, 8);
    assert_eq!(state(&presenter).scrolls["reading"].offset, offset);
    assert_eq!(
        presenter
            .application
            .child_mount(&presenter.root, "document")
            .unwrap(),
        &document
    );
    close(&mut presenter);
}
#[test]
fn wheel_commands_bubble_once_preserve_focus_and_require_presented_bindings() {
    let (_runtime, mut presenter, props) = native(vec![source("one", "one\ntwo\nthree\nfour")]);
    props.borrow_mut()["content"]["wheel"] = json!({"up":"previous","down":"next"});
    refresh(&mut presenter);
    draw(&mut presenter, 20, 2);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        1,
        0,
    );
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 1, 0);
    let focus = presenter.focus.clone().unwrap();
    for (kind, key) in [
        (MouseEventKind::ScrollDown, "next"),
        (MouseEventKind::ScrollUp, "previous"),
    ] {
        let count = presenter.application.frame(&presenter.root).unwrap().state["count"]
            .as_u64()
            .unwrap();
        mouse(&mut presenter, kind, 1, 0);
        assert_eq!(
            presenter.application.frame(&presenter.root).unwrap().state["wheel"],
            key
        );
        assert_eq!(
            presenter.application.frame(&presenter.root).unwrap().state["count"],
            count + 1
        );
        assert!(presenter.focus.as_ref().unwrap().same(&focus));
        assert_eq!(state(&presenter).scrolls["reading"].offset, 0);
    }
    props.borrow_mut()["content"]["wheel"]["down"] = json!("changed");
    refresh(&mut presenter);
    let count = presenter.application.frame(&presenter.root).unwrap().state["count"].clone();
    mouse(&mut presenter, MouseEventKind::ScrollDown, 1, 0);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        count
    );
    draw(&mut presenter, 20, 2);
    mouse(&mut presenter, MouseEventKind::ScrollDown, 1, 0);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["wheel"],
        "changed"
    );
    props.borrow_mut()["content"]["visible"] = json!(false);
    refresh(&mut presenter);
    let count = presenter.application.frame(&presenter.root).unwrap().state["count"].clone();
    mouse(&mut presenter, MouseEventKind::ScrollDown, 1, 0);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        count
    );
    close(&mut presenter);
}
#[test]
fn cross_source_drag_and_select_all_include_clipped_original_bytes() {
    let text = "third\r\n\t中\0";
    let (_runtime, mut presenter, _) = native(vec![
        source("one", "first"),
        source("two", "second"),
        source("three", text),
    ]);
    draw(&mut presenter, 20, 2);
    let third = presenter
        .application
        .child_mount(&presenter.root, "three")
        .unwrap();
    assert!(!presenter.scene.iter().any(|target| &target.mount == third));
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        1,
        0,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        3,
        1,
    );
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 3, 1);
    assert_eq!(copied(&mut presenter).unwrap(), b"irst\nsec");
    all(&mut presenter);
    assert_eq!(
        copied(&mut presenter).unwrap(),
        format!("first\nsecond\n{text}").as_bytes()
    );
    assert_eq!(
        observation(&presenter)["readingRevision"],
        state(&presenter).reading.as_ref().unwrap().scope.revision()
    );
    close(&mut presenter);
}
#[test]
fn commands_settle_after_flush_only_and_selection_invalidates_tail_immediately() {
    let (_runtime, mut presenter, props) = native(vec![
        source("one", "one"),
        source("two", "two"),
        source("three", "three"),
    ]);
    draw(&mut presenter, 20, 2);
    props.borrow_mut()["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    refresh(&mut presenter);
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    let area = Rect::new(0, 0, 20, 2);
    let mut buffer = Buffer::empty(area);
    let unflushed = presenter.paint(area, &mut buffer).unwrap();
    drop(unflushed);
    presenter.sync_native().unwrap();
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    draw(&mut presenter, 20, 2);
    presenter.sync_native().unwrap();
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"applied"})
    );
    assert_eq!(state(&presenter).scrolls["reading"].offset, 1);
    let reading = state(&presenter).reading.as_ref().unwrap();
    assert_eq!(
        reading.scope.revision(),
        observation(&presenter)["viewport"]["readingRevision"]
    );
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    all(&mut presenter);
    assert_ne!(
        observation(&presenter)["readingRevision"],
        observation(&presenter)["viewport"]["readingRevision"]
    );
    props.borrow_mut()["command"] = Value::Null;
    refresh(&mut presenter);
    draw(&mut presenter, 20, 2);
    assert_eq!(copied(&mut presenter).unwrap(), b"one\ntwo\nthree");
    close(&mut presenter);
}
#[test]
fn removed_middle_participant_revokes_old_range_only_on_successful_flush() {
    let (_runtime, mut presenter, props) = native(vec![
        source("one", "one"),
        json!({"kind":"stack","key":"middle","direction":"column","gap":0,"padding":0,"background":0,"children":[source("two","two")]}),
        source("three", "three"),
    ]);
    draw(&mut presenter, 20, 4);
    all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), b"one\ntwo\nthree");
    props.borrow_mut()["content"]["children"][1]["visible"] = json!(false);
    refresh(&mut presenter);
    let area = Rect::new(0, 0, 20, 4);
    let mut buffer = Buffer::empty(area);
    let failed = presenter.paint(area, &mut buffer).unwrap();
    drop(failed);
    assert_eq!(copied(&mut presenter).unwrap(), b"one\ntwo\nthree");
    draw(&mut presenter, 20, 4);
    assert_eq!(copied(&mut presenter), None);
    all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), b"one\nthree");
    close(&mut presenter);
}

#[test]
fn selection_cancels_pending_scroll_without_replaying_the_same_command() {
    let (_runtime, mut presenter, props) = native(vec![
        source("one", "one"),
        source("two", "two"),
        source("three", "three"),
    ]);
    draw(&mut presenter, 20, 2);
    props.borrow_mut()["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    refresh(&mut presenter);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    all(&mut presenter);
    assert_eq!(observation(&presenter)["pendingCommand"], Value::Null);
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"cancelled"})
    );
    draw(&mut presenter, 20, 2);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 0);
    assert_eq!(copied(&mut presenter).unwrap(), b"one\ntwo\nthree");
    close(&mut presenter);
}
#[test]
fn resize_paint_accepts_actual_glyph_geometry_without_overwriting_later_selection() {
    let (_runtime, mut presenter, _) =
        native(vec![source("one", "abcdefgh"), source("two", "tail")]);
    draw(&mut presenter, 20, 5);
    let area = Rect::new(0, 0, 4, 5);
    let mut buffer = Buffer::empty(area);
    let candidate = presenter.paint(area, &mut buffer).unwrap();
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        1,
        0,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        3,
        0,
    );
    assert_eq!(copied(&mut presenter).unwrap(), b"bc");
    presenter.present(candidate);
    assert_eq!(copied(&mut presenter).unwrap(), b"bc");
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        1,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        2,
        1,
    );
    assert_eq!(copied(&mut presenter).unwrap(), b"ef");
    close(&mut presenter);
}

#[test]
fn parallel_sources_use_actual_two_dimensional_hits_in_both_drag_directions() {
    let (_runtime, mut presenter, _) = native(vec![
        json!({"kind":"stack","key":"parallel","direction":"row","gap":2,"padding":0,"background":0,"children":[source("left","left"),source("right","right")]}),
    ]);
    draw(&mut presenter, 12, 3);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        1,
        0,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        8,
        0,
    );
    assert_eq!(copied(&mut presenter).unwrap(), b"eft\nri");
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 8, 0);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        8,
        0,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        1,
        0,
    );
    assert_eq!(copied(&mut presenter).unwrap(), b"eft\nri");
    close(&mut presenter);
}

#[test]
fn late_flush_of_cancelled_command_reports_actual_viewport_without_replacing_desired_position() {
    let props = Rc::new(RefCell::new(props(vec![
        source("one", "one"),
        source("two", "two"),
        source("three", "three"),
    ])));
    let accept = Rc::new(Cell::new(true));
    let (_runtime, mut presenter) =
        layout::native(layout::View::Reading(props.clone()), accept.clone());
    draw(&mut presenter, 20, 2);
    props.borrow_mut()["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    refresh(&mut presenter);
    presenter.sync_native().unwrap();
    let area = Rect::new(0, 0, 20, 2);
    let mut buffer = Buffer::empty(area);
    let candidate = presenter.paint(area, &mut buffer).unwrap();
    // A refused callback retains the same committed generation while native intent changes.
    accept.set(false);
    all(&mut presenter);
    presenter.present(candidate);
    accept.set(true);
    refresh(&mut presenter);
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"cancelled"})
    );
    assert_eq!(observation(&presenter)["viewport"]["atEnd"], true);
    assert_ne!(
        observation(&presenter)["viewport"]["readingRevision"],
        observation(&presenter)["readingRevision"]
    );
    assert_eq!(state(&presenter).scrolls["reading"].offset, 0);
    let buffer = draw(&mut presenter, 20, 2);
    assert_eq!(buffer[(0, 0)].symbol(), "o");
    assert_eq!(copied(&mut presenter).unwrap(), b"one\ntwo\nthree");
    close(&mut presenter);
}

#[test]
fn real_native_reading_slot_inherits_root_grow_and_receipts_route_through_original_component() {
    let mut props = props(vec![
        source("one", "one"),
        source("two", "two"),
        source("three", "three"),
        source("four", "four"),
        source("five", "five"),
    ]);
    props["content"]["layout"] = json!({"grow":1});
    props["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    let props = Rc::new(RefCell::new(props));
    let (_runtime, mut presenter) =
        layout::native(layout::View::ReadingSlot(props), Rc::new(Cell::new(true)));
    let buffer = draw(&mut presenter, 20, 4);
    assert_eq!(buffer[(0, 0)].symbol(), "t");
    assert_eq!(buffer[(0, 2)].symbol(), "f");
    assert_eq!(buffer[(8, 3)].symbol(), "B");
    presenter.sync_native().unwrap();
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"applied"})
    );
    let child = presenter
        .application
        .child_mount(&presenter.root, "reader")
        .unwrap();
    let state = presenter.resources.owners[&child.id()]
        .state
        .downcast_ref::<super::super::scroll::SceneState>()
        .unwrap();
    assert_eq!(
        state.scrolls["reading"]
            .presented
            .as_ref()
            .unwrap()
            .area
            .height,
        3
    );
    assert_eq!(state.scrolls["reading"].offset, 2);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    all(&mut presenter);
    assert_eq!(
        copied(&mut presenter).unwrap(),
        b"one\ntwo\nthree\nfour\nfive"
    );
    close(&mut presenter);
}

#[test]
fn native_reading_observations_reuse_the_scene_but_command_updates_replace_it() {
    let settle = |presenter: &mut Presenter, width, height| {
        draw(presenter, width, height);
        presenter.settle().unwrap();
    };
    let mut declaration = props(vec![source("text", &"row\n".repeat(100))]);
    declaration["content"]["layout"] = json!({"grow":1});
    let props = Rc::new(RefCell::new(declaration));
    let (_runtime, mut presenter) = layout::native(
        layout::View::ReadingSlot(props.clone()),
        Rc::new(Cell::new(true)),
    );
    settle(&mut presenter, 30, 12);
    let reader = presenter
        .application
        .child_mount(&presenter.root, "reader")
        .unwrap()
        .clone();
    let recipe = presenter
        .application
        .frame(&reader)
        .unwrap()
        .view
        .view
        .clone();
    mouse(&mut presenter, MouseEventKind::ScrollDown, 5, 5);
    settle(&mut presenter, 30, 12);
    assert!(Rc::ptr_eq(
        &recipe,
        &presenter.application.frame(&reader).unwrap().view.view
    ));
    assert_eq!(observation(&presenter)["scroll"]["direction"], "down");
    props.borrow_mut()["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    let root = presenter.root.clone();
    let revision = presenter.application.frame(&root).unwrap().revision;
    presenter
        .application
        .dispatch(&root, revision, "back", json!({"kind":"click"}))
        .unwrap();
    settle(&mut presenter, 30, 12);
    assert!(!Rc::ptr_eq(
        &recipe,
        &presenter.application.frame(&reader).unwrap().view.view
    ));
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"applied"})
    );
    close(&mut presenter);
}

#[test]
fn hidden_same_root_retains_pending_command_until_visible_flush() {
    let (_runtime, mut presenter, props) = native(vec![
        source("one", "one"),
        source("two", "two"),
        source("three", "three"),
    ]);
    draw(&mut presenter, 20, 2);
    props.borrow_mut()["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    refresh(&mut presenter);
    props.borrow_mut()["content"]["visible"] = json!(false);
    refresh(&mut presenter);
    // Old physical targets cannot cancel the retained command of a declared-hidden root.
    mouse(&mut presenter, MouseEventKind::ScrollDown, 0, 0);
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    draw(&mut presenter, 20, 2);
    presenter.sync_native().unwrap();
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    assert_eq!(observation(&presenter)["viewport"], Value::Null);
    props.borrow_mut()["content"]["visible"] = json!(true);
    props.borrow_mut()["command"] = Value::Null;
    refresh(&mut presenter);
    draw(&mut presenter, 20, 2);
    presenter.sync_native().unwrap();
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"applied"})
    );
    assert_eq!(state(&presenter).scrolls["reading"].offset, 1);
    close(&mut presenter);
}

#[test]
fn horizontal_extension_moves_original_graphemes_across_clipped_participants() {
    let (_runtime, mut presenter, _) = native(vec![source("one", "one"), source("two", "中😀")]);
    draw(&mut presenter, 20, 1);
    all(&mut presenter);
    for expected in ["one\n中", "one\n", "one", "on"] {
        presenter
            .input_settled(Event::Key(KeyEvent::new(
                KeyCode::Left,
                KeyModifiers::SHIFT,
            )))
            .unwrap();
        assert_eq!(copied(&mut presenter).unwrap(), expected.as_bytes());
    }
    close(&mut presenter);
}

#[test]
fn aggregate_copy_preserves_complete_separators_and_selection() {
    let (_runtime, mut presenter, _) = native(vec![
        source("one", &"a".repeat(32768)),
        source("two", &"b".repeat(32768)),
    ]);
    draw(&mut presenter, 20, 2);
    all(&mut presenter);
    let expected = format!("{}\n{}", "a".repeat(32768), "b".repeat(32768));
    assert_eq!(copied(&mut presenter).unwrap(), expected.as_bytes());
    assert!(
        state(&presenter)
            .reading
            .as_ref()
            .unwrap()
            .scope
            .has_selection()
    );
    presenter
        .input_settled(Event::Key(KeyEvent::new(
            KeyCode::Left,
            KeyModifiers::SHIFT,
        )))
        .unwrap();
    assert_eq!(
        copied(&mut presenter).unwrap(),
        &expected.as_bytes()[..expected.len() - 1]
    );
    close(&mut presenter);
}
#[test]
fn collapsed_pointer_gesture_blocks_tail_until_release_and_matching_paint() {
    let (_runtime, mut presenter, _) = native(vec![source("one", "one")]);
    draw(&mut presenter, 20, 2);
    assert!(state(&presenter).reading.as_ref().unwrap().eligible());
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    draw(&mut presenter, 20, 2);
    let reading = state(&presenter).reading.as_ref().unwrap();
    assert!(!reading.scope.has_selection());
    assert!(!reading.eligible());
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 0, 0);
    draw(&mut presenter, 20, 2);
    assert!(state(&presenter).reading.as_ref().unwrap().eligible());
    close(&mut presenter);
}

#[test]
fn same_commit_source_replacement_and_new_command_do_not_invent_user_cancellation() {
    let (_runtime, mut presenter, props) = native(vec![
        source("one", "one"),
        source("two", "two"),
        source("three", "three"),
    ]);
    draw(&mut presenter, 20, 2);
    all(&mut presenter);
    props.borrow_mut()["content"]["children"][0]["props"]["source"] = json!("new original");
    props.borrow_mut()["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    refresh(&mut presenter);
    draw(&mut presenter, 20, 2);
    presenter.sync_native().unwrap();
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"applied"})
    );
    assert_eq!(state(&presenter).scrolls["reading"].offset, 1);
    assert_eq!(copied(&mut presenter), None);
    close(&mut presenter);
}

#[test]
fn reading_component_rejects_changed_sequence_and_omission_keeps_original_command() {
    use maka_ui_runtime::{ComponentCall, NativeComponent};
    let mut props = props(vec![]);
    props["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    let component = super::super::reading::Component;
    let first = component
        .call(ComponentCall::Mount { props: &props }, &Default::default())
        .unwrap();
    props["command"] = json!({"sequence":1,"target":{"kind":"start"}});
    assert!(
        component
            .call(
                ComponentCall::Update {
                    props: &props,
                    state: &first.frame.state
                },
                &Default::default()
            )
            .is_err()
    );
    props["command"] = Value::Null;
    let next = component
        .call(
            ComponentCall::Update {
                props: &props,
                state: &first.frame.state,
            },
            &Default::default(),
        )
        .unwrap();
    assert_eq!(next.frame.state["command"], first.frame.state["command"]);
}

#[test]
fn reading_declared_root_visibility_projects_before_physical_flush() {
    let mut props = props(vec![source("one", "one")]);
    props["content"]["visible"] = json!(false);
    use maka_ui_runtime::{ComponentCall, NativeComponent};
    let output = super::super::reading::Component
        .call(ComponentCall::Mount { props: &props }, &Default::default())
        .unwrap();
    assert!(!output.frame.view.root_visible());
    assert_eq!(output.frame.view.visible_slots().count(), 0);
    assert_eq!(output.frame.view.slots().len(), 1);
}

#[test]
fn hidden_native_component_root_does_not_take_parent_grow_space() {
    let mut props = props(vec![source("one", "one")]);
    props["content"]["layout"] = json!({"grow":1});
    props["content"]["visible"] = json!(false);
    let props = Rc::new(RefCell::new(props));
    let (_runtime, mut presenter) =
        layout::native(layout::View::ReadingSlot(props), Rc::new(Cell::new(true)));
    let buffer = draw(&mut presenter, 20, 4);
    assert_eq!(buffer[(8, 0)].symbol(), "B");
    assert_eq!(buffer[(0, 3)].symbol(), " ");
    close(&mut presenter);
}

#[test]
fn delayed_hidden_flush_cannot_suspend_new_original_reading_and_source_geometry() {
    let (_runtime, mut presenter, props) =
        native(vec![source("one", "original"), source("two", "中😀")]);
    draw(&mut presenter, 20, 2);
    props.borrow_mut()["content"]["visible"] = json!(false);
    refresh(&mut presenter);
    let area = Rect::new(0, 0, 20, 2);
    let hidden = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    props.borrow_mut()["content"]["visible"] = json!(true);
    refresh(&mut presenter);
    draw(&mut presenter, 20, 2);
    all(&mut presenter);
    let before = copied(&mut presenter).unwrap();
    let observed_before = observation(&presenter);
    let targets: Vec<_> = presenter
        .scene
        .iter()
        .map(|target| (target.mount.clone(), target.key.clone(), target.area))
        .collect();
    presenter.present(hidden);
    assert_eq!(copied(&mut presenter).unwrap(), before);
    assert_eq!(
        presenter
            .scene
            .iter()
            .map(|target| (target.mount.clone(), target.key.clone(), target.area))
            .collect::<Vec<_>>(),
        targets
    );
    presenter.sync_native().unwrap();
    assert_eq!(observation(&presenter), observed_before);
    // Actual pointer geometry still addresses the new original participants.
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        1,
        0,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        4,
        0,
    );
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 4, 0);
    assert_eq!(copied(&mut presenter).unwrap(), b"rig");
    close(&mut presenter);
}

#[path = "reading/dialog.rs"]
mod dialog;

#[path = "reading/choice.rs"]
mod choice;

#[path = "reading/items.rs"]
mod items;

#[path = "reading/find.rs"]
mod find;

#[path = "reading/markdown.rs"]
mod markdown;

#[path = "reading/keyboard.rs"]
mod keyboard;

#[path = "reading/context_menu.rs"]
mod context_menu;

#[test]
fn long_reading_window_keeps_hundreds_of_original_text_owners_without_count_caps() {
    let children = (0..600)
        .map(|i| source(&format!("row-{i}"), &format!("message {i}")))
        .collect();
    let (_runtime, mut presenter, props) = native(children);
    draw(&mut presenter, 40, 10);
    let reading = presenter.root.clone();
    let first = presenter
        .application
        .child_mount(&reading, "row-0")
        .unwrap()
        .clone();
    let last = presenter
        .application
        .child_mount(&reading, "row-599")
        .unwrap()
        .clone();
    props.borrow_mut()["command"] = json!({"sequence":1,"target":{"kind":"end"}});
    refresh(&mut presenter);
    let buffer = draw(&mut presenter, 40, 10);
    assert!(
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("message 599")
    );
    assert_eq!(
        presenter
            .application
            .child_mount(&reading, "row-0")
            .unwrap(),
        &first
    );
    assert_eq!(
        presenter
            .application
            .child_mount(&reading, "row-599")
            .unwrap(),
        &last
    );
    close(&mut presenter);
}
