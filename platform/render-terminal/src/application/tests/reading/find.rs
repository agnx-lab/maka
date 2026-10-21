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
fn command(presenter: &mut Presenter, props: &Rc<RefCell<Value>>, sequence: u64, target: Value) {
    props.borrow_mut()["command"] = json!({"sequence":sequence,"target":target});
    refresh(presenter);
}
fn settle(presenter: &mut Presenter, width: u16, height: u16) -> Buffer {
    let buffer = draw_frame(presenter, width, height);
    presenter.sync_native().unwrap();
    buffer
}
#[test]
fn find_counts_folded_and_clipped_sources_without_taking_selection_or_scroll() {
    let (_runtime, mut presenter, props) = native(vec![
        source("first", "中😀中"),
        json!({"kind":"stack","key":"fold","direction":"column","gap":0,"padding":0,"background":0,"visible":false,"children":[source("folded","😀")] }),
        source("last", "last 😀"),
    ]);
    settle(&mut presenter, 12, 1);
    all(&mut presenter);
    let selected = copied(&mut presenter).unwrap();
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"😀","step":"first"}),
    );
    let area = Rect::new(0, 0, 12, 1);
    drop(presenter.paint(area, &mut Buffer::empty(area)).unwrap());
    assert_eq!(observation(&presenter)["find"], Value::Null);
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    settle(&mut presenter, 12, 1);
    assert_eq!(
        observation(&presenter)["find"],
        json!({"sequence":1,"count":3,"limited":false,"index":1,"active":{"path":["first"],"offset":3}})
    );
    assert_eq!(copied(&mut presenter).unwrap(), selected);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 0);
    command(
        &mut presenter,
        &props,
        2,
        json!({"kind":"find","query":"😀","step":"next"}),
    );
    settle(&mut presenter, 12, 1);
    assert_eq!(
        observation(&presenter)["find"]["active"]["path"],
        json!(["folded"])
    );
    command(
        &mut presenter,
        &props,
        3,
        json!({"kind":"find","query":"😀","step":"previous"}),
    );
    settle(&mut presenter, 12, 1);
    assert_eq!(observation(&presenter)["find"]["index"], 1);
    command(
        &mut presenter,
        &props,
        4,
        json!({"kind":"find","query":"😀","step":"previous"}),
    );
    settle(&mut presenter, 12, 1);
    assert_eq!(observation(&presenter)["find"]["index"], 3);
    props.borrow_mut()["content"]["children"][2]["props"]["source"] = json!("last 😀 plus 😀");
    refresh(&mut presenter);
    settle(&mut presenter, 12, 1);
    assert_eq!(observation(&presenter)["find"]["count"], 4);
    assert_eq!(observation(&presenter)["find"]["sequence"], 4);
    assert_eq!(observation(&presenter)["find"]["index"], 3);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 0);
    command(&mut presenter, &props, 5, json!({"kind":"clear_find"}));
    settle(&mut presenter, 12, 1);
    assert_eq!(observation(&presenter)["find"], Value::Null);
    close(&mut presenter);
}
#[test]
fn highlights_complete_graphemes_and_counts_every_match() {
    let (_runtime, mut presenter, props) = native(vec![source("first", "é😀 é😀")]);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"́","step":"first"}),
    );
    let buffer = settle(&mut presenter, 12, 2);
    use ratatui::style::Modifier;
    assert!(
        buffer[(0, 0)]
            .modifier
            .contains(Modifier::REVERSED | Modifier::BOLD)
    );
    assert!(buffer[(4, 0)].modifier.contains(Modifier::UNDERLINED));
    assert!(!buffer[(4, 0)].modifier.contains(Modifier::REVERSED));
    props.borrow_mut()["content"]["children"][0]["props"]["source"] = json!("x".repeat(4097));
    command(
        &mut presenter,
        &props,
        2,
        json!({"kind":"find","query":"x","step":"first"}),
    );
    settle(&mut presenter, 12, 2);
    assert_eq!(observation(&presenter)["find"]["count"], 4097);
    assert_eq!(observation(&presenter)["find"]["limited"], false);
    command(
        &mut presenter,
        &props,
        3,
        json!({"kind":"find","query":"x","step":"previous"}),
    );
    settle(&mut presenter, 12, 2);
    assert_eq!(observation(&presenter)["find"]["index"], 4097);
    assert_eq!(
        observation(&presenter)["find"]["active"],
        json!({"path":["first"],"offset":4096})
    );
    all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), "x".repeat(4097).as_bytes());
    close(&mut presenter);
}
#[test]
fn source_reveal_uses_offscreen_wrapped_bytes_and_waits_for_actual_second_flush() {
    let (_runtime, mut presenter, props) = native(vec![
        source("before", &"before\n".repeat(8)),
        source(
            "long",
            &format!("{}中😀target{}", "abcdefgh".repeat(10), "tail".repeat(20)),
        ),
    ]);
    settle(&mut presenter, 10, 6);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"reveal_source","path":["long"],"offset":87}),
    );
    let area = Rect::new(0, 0, 10, 6);
    drop(presenter.paint(area, &mut Buffer::empty(area)).unwrap());
    assert_eq!(state(&presenter).scrolls["reading"].offset, 0);
    settle(&mut presenter, 10, 6);
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 16);
    let buffer = settle(&mut presenter, 10, 6);
    assert_eq!(buffer[(4, 2)].symbol(), "t");
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"applied"})
    );
    close(&mut presenter);
}
#[test]
fn reveal_rejects_folded_missing_and_non_boundary_bytes_and_user_scroll_cancels() {
    let (_runtime, mut presenter, props) = native(vec![
        source("first", &"line\n".repeat(12)),
        json!({"kind":"stack","key":"fold","direction":"column","gap":0,"padding":0,"background":0,"visible":false,"children":[source("folded","中")] }),
    ]);
    settle(&mut presenter, 12, 3);
    for (sequence, path, offset) in [(1, "folded", 0), (2, "missing", 0), (3, "folded", 1)] {
        command(
            &mut presenter,
            &props,
            sequence,
            json!({"kind":"reveal_source","path":[path],"offset":offset}),
        );
        settle(&mut presenter, 12, 3);
        assert_eq!(
            observation(&presenter)["receipt"],
            json!({"sequence":sequence,"status":"rejected"})
        );
    }
    command(
        &mut presenter,
        &props,
        4,
        json!({"kind":"reveal_source","path":["first"],"offset":40}),
    );
    let area = Rect::new(0, 0, 12, 3);
    let delayed = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    mouse(&mut presenter, MouseEventKind::ScrollDown, 0, 0);
    presenter.present(delayed);
    presenter.sync_native().unwrap();
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":4,"status":"cancelled"})
    );
    assert_eq!(state(&presenter).scrolls["reading"].offset, 1);
    close(&mut presenter);
}
#[test]
fn find_preserves_complete_utf8_queries_and_original_selection() {
    let query = "中".repeat(171);
    let text = format!("before {query} after");
    let (_runtime, mut presenter, props) = native(vec![source("first", &text)]);
    settle(&mut presenter, 20, 3);
    all(&mut presenter);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":query,"step":"first"}),
    );
    settle(&mut presenter, 20, 3);
    assert_eq!(
        observation(&presenter)["find"],
        json!({"sequence":1,"count":1,"limited":false,"index":1,"active":{"path":["first"],"offset":7}})
    );
    assert_eq!(copied(&mut presenter).unwrap(), text.as_bytes());
    close(&mut presenter);
}

#[test]
fn reveal_recomputes_after_resize_before_acknowledging_original_byte() {
    let (_runtime, mut presenter, props) = native(vec![source("long", &"0123456789".repeat(30))]);
    settle(&mut presenter, 10, 6);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"reveal_source","path":["long"],"offset":125}),
    );
    settle(&mut presenter, 10, 6);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 11);
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    settle(&mut presenter, 5, 6);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 29);
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    let buffer = settle(&mut presenter, 5, 6);
    assert_eq!(buffer[(1, 2)].symbol(), "5");
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"applied"})
    );
    close(&mut presenter);
}
#[test]
fn delayed_find_cannot_override_newer_physical_intent_even_when_guest_refuses_observation() {
    let props = Rc::new(RefCell::new(props(vec![source("first", "original find")])));
    let accept = Rc::new(Cell::new(true));
    let (_runtime, mut presenter) =
        layout::native(layout::View::Reading(props.clone()), accept.clone());
    settle(&mut presenter, 20, 3);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"find","step":"first"}),
    );
    let area = Rect::new(0, 0, 20, 3);
    let delayed = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    accept.set(false);
    all(&mut presenter);
    presenter.present(delayed);
    accept.set(true);
    refresh(&mut presenter);
    assert_eq!(observation(&presenter)["find"], Value::Null);
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":1,"status":"cancelled"})
    );
    assert_eq!(copied(&mut presenter).unwrap(), b"original find");
    close(&mut presenter);
}
#[test]
fn empty_find_blocks_tail_continuation_until_cleared_and_removed_match_is_not_retained() {
    let (_runtime, mut presenter, props) = native(vec![source("first", "first")]);
    settle(&mut presenter, 20, 3);
    assert!(state(&presenter).reading.as_ref().unwrap().eligible());
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"","step":"first"}),
    );
    settle(&mut presenter, 20, 3);
    assert_eq!(observation(&presenter)["find"]["count"], 0);
    assert!(!state(&presenter).reading.as_ref().unwrap().eligible());
    command(
        &mut presenter,
        &props,
        2,
        json!({"kind":"find","query":"first","step":"first"}),
    );
    settle(&mut presenter, 20, 3);
    assert_eq!(observation(&presenter)["find"]["count"], 1);
    props.borrow_mut()["content"]["children"][0]["props"]["source"] = json!("replaced");
    refresh(&mut presenter);
    settle(&mut presenter, 20, 3);
    assert_eq!(observation(&presenter)["find"]["count"], 0);
    assert_eq!(observation(&presenter)["find"]["active"], Value::Null);
    command(&mut presenter, &props, 3, json!({"kind":"clear_find"}));
    settle(&mut presenter, 20, 3);
    assert!(state(&presenter).reading.as_ref().unwrap().eligible());
    close(&mut presenter);
}

#[test]
fn find_stops_at_nested_reading_boundary_and_reveal_does_not_claim_inner_clipped_success() {
    let nested = json!({"kind":"component","key":"nested","binding":"reading","member":0,"on":{},"props":props(vec![source("inner", "needle")])});
    let (_runtime, mut presenter, props) = native(vec![
        source("first", "needle"),
        nested,
        json!({"kind":"stack","key":"clip","direction":"column","gap":0,"padding":0,"background":0,"scroll":"manual","layout":{"basis":2},"children":[source("clipped", "zero\none\ntwo\nthree\nfour\nneedle")]}),
    ]);
    settle(&mut presenter, 20, 8);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"needle","step":"first"}),
    );
    settle(&mut presenter, 20, 8);
    assert_eq!(observation(&presenter)["find"]["count"], 2);
    command(
        &mut presenter,
        &props,
        2,
        json!({"kind":"reveal_source","path":["nested","inner"],"offset":0}),
    );
    settle(&mut presenter, 20, 8);
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":2,"status":"rejected"})
    );
    command(
        &mut presenter,
        &props,
        3,
        json!({"kind":"reveal_source","path":["clipped"],"offset":24}),
    );
    settle(&mut presenter, 20, 8);
    assert_eq!(
        observation(&presenter)["receipt"],
        json!({"sequence":3,"status":"rejected"})
    );
    close(&mut presenter);
}

#[test]
fn relative_reading_rows_settle_only_on_flush_and_physical_scroll_cancels_late_geometry() {
    let (_runtime, mut presenter, props) = native(vec![source(
        "preview",
        &(0..40)
            .map(|row| format!("line {row}\n"))
            .collect::<String>(),
    )]);
    settle(&mut presenter, 20, 5);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"scroll_rows","rows":8}),
    );
    let area = Rect::new(0, 0, 20, 5);
    drop(presenter.paint(area, &mut Buffer::empty(area)).unwrap());
    assert_eq!(state(&presenter).scrolls["reading"].offset, 0);
    assert_eq!(observation(&presenter)["pendingCommand"], 1);
    settle(&mut presenter, 20, 5);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 8);
    assert_eq!(observation(&presenter)["receipt"]["status"], "applied");
    settle(&mut presenter, 20, 5);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 8);
    command(
        &mut presenter,
        &props,
        2,
        json!({"kind":"scroll_rows","rows":-8}),
    );
    let stale = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    mouse(&mut presenter, MouseEventKind::ScrollDown, 1, 1);
    let offset = state(&presenter).scrolls["reading"].offset;
    presenter.present(stale);
    settle(&mut presenter, 20, 5);
    assert_eq!(state(&presenter).scrolls["reading"].offset, offset);
    assert_eq!(observation(&presenter)["receipt"]["status"], "cancelled");
    command(
        &mut presenter,
        &props,
        3,
        json!({"kind":"scroll_rows","rows":-65536}),
    );
    settle(&mut presenter, 20, 5);
    assert_eq!(state(&presenter).scrolls["reading"].offset, 0);
    close(&mut presenter);
}

#[test]
fn source_pointer_selection_can_keep_the_existing_keyboard_focus() {
    let mut preview = source("preview", "selectable preview");
    preview["props"]["focusOnPointer"] = json!(false);
    let (_runtime, mut presenter, _) = native(vec![preview, source("other", "original focus")]);
    settle(&mut presenter, 30, 3);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        2,
        1,
    );
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 2, 1);
    let focus = presenter.focus.clone().unwrap();
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        0,
        0,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        10,
        0,
    );
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), 10, 0);
    assert!(presenter.focus.as_ref().unwrap().same(&focus));
    assert_eq!(copied(&mut presenter).unwrap(), b"selectable");
    close(&mut presenter);
}

#[test]
fn accepted_find_cannot_reveal_after_later_physical_reading_intent() {
    let (_runtime, mut presenter, props) = native(vec![source(
        "long",
        &format!("{}needle", "row\n".repeat(30)),
    )]);
    settle(&mut presenter, 20, 5);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"needle","step":"first"}),
    );
    settle(&mut presenter, 20, 5);
    assert_eq!(observation(&presenter)["receipt"]["status"], "applied");
    mouse(&mut presenter, MouseEventKind::ScrollDown, 1, 1);
    settle(&mut presenter, 20, 5);
    let offset = state(&presenter).scrolls["reading"].offset;
    command(
        &mut presenter,
        &props,
        2,
        json!({"kind":"reveal_source","path":["long"],"offset":120,"find_sequence":1}),
    );
    settle(&mut presenter, 20, 5);
    assert_eq!(state(&presenter).scrolls["reading"].offset, offset);
    assert_eq!(observation(&presenter)["receipt"]["status"], "cancelled");
    command(
        &mut presenter,
        &props,
        3,
        json!({"kind":"find","query":"needle","step":"next"}),
    );
    settle(&mut presenter, 20, 5);
    command(
        &mut presenter,
        &props,
        4,
        json!({"kind":"reveal_source","path":["long"],"offset":120,"find_sequence":3}),
    );
    settle(&mut presenter, 20, 5);
    settle(&mut presenter, 20, 5);
    assert_eq!(observation(&presenter)["receipt"]["status"], "applied");
    assert!(state(&presenter).scrolls["reading"].offset > offset);
    close(&mut presenter);
}
