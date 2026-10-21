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
use ratatui::style::Modifier;

fn markdown(key: &str, source: &str) -> Value {
    let body = json!({"font":{"family":"monospace","fallbacks":[],"size":16,"lineHeight":1.5,"weight":400},"foreground":0xddddddffu32});
    let mut heading = body.clone();
    heading["font"]["weight"] = json!(700);
    json!({"kind":"component","key":key,"binding":"markdown","member":0,"on":{},"props":{
        "source":source,"selectable":true,"disabled":false,
        "appearance":{"body":body,"code":body,"headings":[heading,heading,heading,heading,heading,heading],
        "muted":0x777777ffu32,"link":0x88aaffffu32,"selection":0x335577ffu32,"codeBackground":0x111111ffu32,
        "tableBackground":0x222222ffu32,"border":0x666666ffu32,"dark":true,"paragraphGap":12}
    }})
}
fn command(presenter: &mut Presenter, props: &Rc<RefCell<Value>>, sequence: u64, target: Value) {
    props.borrow_mut()["command"] = json!({"sequence":sequence,"target":target});
    refresh(presenter);
}
fn settle(presenter: &mut Presenter, width: u16, height: u16) -> Buffer {
    let buffer = draw(presenter, width, height);
    presenter.sync_native().unwrap();
    buffer
}
fn point(buffer: &Buffer, needle: &str) -> (u16, u16) {
    for row in buffer.area.y..buffer.area.bottom() {
        let text = (buffer.area.x..buffer.area.right())
            .map(|column| buffer[(column, row)].symbol())
            .collect::<String>();
        if let Some(offset) = text.find(needle) {
            let mut byte = 0;
            for column in buffer.area.x..buffer.area.right() {
                if byte == offset {
                    return (column, row);
                }
                byte += buffer[(column, row)].symbol().len();
            }
        }
    }
    panic!(
        "missing {needle}: {:?}",
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    );
}
#[test]
fn markdown_reuses_rendered_text_for_copy_find_and_exact_reveal() {
    let (_runtime, mut presenter, props) = native(vec![
        source("literal", "literal **bytes**"),
        markdown(
            "md",
            "# Title\n\n**bold** &amp; 中\n\n```rust\nlet x = 1;\n```",
        ),
    ]);
    let buffer = settle(&mut presenter, 40, 14);
    let (x, y) = point(&buffer, "Title");
    assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
    all(&mut presenter);
    assert_eq!(
        copied(&mut presenter).unwrap(),
        b"literal **bytes**\nTitle\n\nbold & \xe4\xb8\xad\n\nlet x = 1;"
    );
    presenter
        .input_settled(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
        .unwrap();
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"bold &","step":"first"}),
    );
    settle(&mut presenter, 40, 4);
    assert_eq!(observation(&presenter)["find"]["count"], 1);
    let active = observation(&presenter)["find"]["active"].clone();
    assert_eq!(active, json!({"path":["md"],"offset":7}));
    command(
        &mut presenter,
        &props,
        2,
        json!({"kind":"reveal_source","path":active["path"],"offset":active["offset"],"find_sequence":1}),
    );
    settle(&mut presenter, 40, 4);
    let buffer = settle(&mut presenter, 40, 4);
    assert_eq!(observation(&presenter)["receipt"]["status"], "applied");
    let (x, y) = point(&buffer, "bold &");
    assert!(
        buffer[(x, y)]
            .modifier
            .contains(Modifier::REVERSED | Modifier::BOLD)
    );
    close(&mut presenter);
}
#[test]
fn wrapped_table_pointer_selection_and_keyboard_follow_the_original_semantic_mapping() {
    let text = "| Name | Details |\n|---|---|\n| one two three four | five six seven eight |";
    let (_runtime, mut presenter, _) = native(vec![markdown("md", text)]);
    let buffer = settle(&mut presenter, 24, 14);
    let (x, y) = point(&buffer, "seven");
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        x,
        y,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Drag(MouseButton::Left),
        x + 5,
        y,
    );
    mouse(
        &mut presenter,
        MouseEventKind::Up(MouseButton::Left),
        x + 5,
        y,
    );
    assert_eq!(copied(&mut presenter).unwrap(), b"seven");
    presenter
        .input_settled(Event::Key(KeyEvent::new(
            KeyCode::Right,
            KeyModifiers::SHIFT,
        )))
        .unwrap();
    assert_eq!(copied(&mut presenter).unwrap(), b"seven ");
    let identity = presenter.resources.owners[&presenter
        .application
        .child_mount(&presenter.root, "md")
        .unwrap()
        .id()]
        .identity
        .clone();
    for width in [12, 44, 20, 60, 24] {
        settle(&mut presenter, width, 20);
        assert_eq!(copied(&mut presenter).unwrap(), b"seven ");
        assert!(Rc::ptr_eq(
            &identity,
            &presenter.resources.owners[&presenter
                .application
                .child_mount(&presenter.root, "md")
                .unwrap()
                .id()]
                .identity
        ));
    }
    close(&mut presenter);
}
#[test]
fn markdown_append_preserves_selection_but_reference_resolution_revokes_old_semantics() {
    let (_runtime, mut presenter, props) = native(vec![markdown("md", "**first**")]);
    settle(&mut presenter, 30, 8);
    all(&mut presenter);
    props.borrow_mut()["content"]["children"][0]["props"]["source"] = json!("**first** tail");
    refresh(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), b"first");
    settle(&mut presenter, 30, 8);
    assert_eq!(copied(&mut presenter).unwrap(), b"first");
    props.borrow_mut()["content"]["children"][0]["props"]["source"] = json!("[label][ref]");
    refresh(&mut presenter);
    settle(&mut presenter, 30, 8);
    all(&mut presenter);
    props.borrow_mut()["content"]["children"][0]["props"]["source"] =
        json!("[label][ref]\n\n[ref]: https://example.test");
    refresh(&mut presenter);
    assert_eq!(copied(&mut presenter), None);
    settle(&mut presenter, 30, 8);
    all(&mut presenter);
    assert_eq!(
        copied(&mut presenter).unwrap(),
        b"label (https://example.test)"
    );
    props.borrow_mut()["content"]["children"][0]["props"]["selectable"] = json!(false);
    refresh(&mut presenter);
    settle(&mut presenter, 30, 8);
    assert_eq!(copied(&mut presenter), None);
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"label","step":"first"}),
    );
    settle(&mut presenter, 30, 8);
    assert_eq!(observation(&presenter)["find"]["count"], 1);
    close(&mut presenter);
}
#[test]
fn markdown_preparation_publishes_complete_text_and_replaces_the_original_selection() {
    let (runtime, mut presenter, props) = native(vec![markdown("md", "original")]);
    settle(&mut presenter, 20, 4);
    all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), b"original");
    let complete = "x".repeat(65537) + "中😀tail";
    props.borrow_mut()["content"]["children"][0]["props"]["source"] = json!(complete);
    refresh(&mut presenter);
    settle(&mut presenter, 20, 4);
    all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), complete.as_bytes());
    props.borrow_mut()["content"]["children"][0]["props"]["source"] = json!("new");
    refresh(&mut presenter);
    settle(&mut presenter, 20, 4);
    all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), b"new");
    close(&mut presenter);
    drop(runtime);
}

#[test]
fn markdown_keyboard_skips_fence_decoration_and_unflushed_formatting_has_no_pointer_authority() {
    let (_runtime, mut presenter, props) =
        native(vec![markdown("md", "before\n\n```\ncode\n```\n\nafter")]);
    let buffer = settle(&mut presenter, 30, 12);
    let (x, y) = point(&buffer, "after");
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        x,
        y,
    );
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), x, y);
    presenter
        .input_settled(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)))
        .unwrap();
    assert_eq!(copied(&mut presenter).unwrap(), b"code\n\n");
    let selected = copied(&mut presenter);
    props.borrow_mut()["content"]["children"][0]["props"]["appearance"]["codeBackground"] =
        json!(0x444444ffu32);
    refresh(&mut presenter);
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        x,
        y,
    );
    assert!(presenter.drag.is_none());
    assert_eq!(copied(&mut presenter), selected);
    settle(&mut presenter, 30, 12);
    close(&mut presenter);
}

#[test]
fn standalone_markdown_uses_the_same_native_document_without_a_reading_wrapper() {
    let props = Rc::new(RefCell::new(
        markdown("md", "# **Standalone**")["props"].clone(),
    ));
    let (_runtime, mut presenter) = layout::native(
        layout::View::StandaloneMarkdown(props),
        Rc::new(Cell::new(true)),
    );
    let buffer = draw(&mut presenter, 30, 6);
    assert!(buffer[(0, 0)].modifier.contains(Modifier::BOLD));
    all(&mut presenter);
    assert_eq!(copied(&mut presenter).unwrap(), b"Standalone");
    close(&mut presenter);
}

#[test]
fn retained_markdown_layout_leaves_native_capacity_available_and_matches_rebuilt_output() {
    use maka_ui_runtime::budget::{BudgetLimits, Kind};

    let props = Rc::new(RefCell::new(
        markdown("md", "# **Capacity**\n\n`code` 中")["props"].clone(),
    ));
    let (_runtime, mut presenter) = layout::native(
        layout::View::StandaloneMarkdown(props),
        Rc::new(Cell::new(true)),
    );
    let budget = presenter.resources.budget.clone();
    let native_usage = budget.usage();
    let cold = draw(&mut presenter, 24, 6);
    assert_eq!(cold[(0, 0)].symbol(), "C");
    assert!(cold[(0, 0)].modifier.contains(Modifier::BOLD));
    assert_eq!(budget.usage(), native_usage);
    let native_capacity = budget
        .reserve(
            Kind::Encoded,
            BudgetLimits::default().encoded_bytes - native_usage.0,
        )
        .expect("retained Markdown must leave native admission available");
    assert_eq!(draw(&mut presenter, 24, 6), cold);

    let revision = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .revision;
    presenter
        .application
        .dispatch(&presenter.root, revision, "back", json!({"kind":"click"}))
        .unwrap();
    assert_eq!(draw(&mut presenter, 24, 6), cold);
    all(&mut presenter);
    assert_eq!(
        copied(&mut presenter).unwrap(),
        "Capacity\n\ncode 中".as_bytes()
    );
    close(&mut presenter);
    drop(native_capacity);
    assert_eq!(budget.usage(), (0, 0, 0));
}

fn diff(source: &str, rows: Value) -> Value {
    json!({"kind":"component","key":"diff","binding":"diff","member":0,"on":{},"props":{
        "content":{"kind":"annotated","source":source,"rows":rows},"ascii":false,"disabled":false,"focusOnPointer":true,
        "appearance":{"text":{"font":{"family":"monospace","fallbacks":[],"size":16,"lineHeight":1.5,"weight":400},"foreground":0xddddddffu32},
        "background":0x111111ffu32,"added":0x33cc55ffu32,"removed":0xdd3344ffu32,"selection":0x335577ffu32,"dark":true}
    }})
}
#[test]
fn native_diff_reuses_original_gutters_copy_find_resize_and_owner() {
    let text = "let old = 1;\nlet new = 2;\n中😀 final\n";
    let split = text.find("let new").unwrap();
    let context = text.find("中").unwrap();
    let rows = json!([
        {"start":0,"end":split,"kind":"removed","language":"rust"},
        {"start":split,"end":context,"kind":"added","language":"rust"},
        {"start":context,"end":text.len(),"kind":"context","language":"rust"}
    ]);
    let (_runtime, mut presenter, props) =
        native(vec![source("literal", "Original"), diff(text, rows)]);
    let buffer = settle(&mut presenter, 36, 10);
    point(&buffer, "− let old = 1;");
    point(&buffer, "+ let new = 2;");
    all(&mut presenter);
    assert_eq!(
        copied(&mut presenter).unwrap(),
        format!("Original\n{}", text.trim_end()).as_bytes()
    );
    presenter
        .input_settled(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
        .unwrap();
    command(
        &mut presenter,
        &props,
        1,
        json!({"kind":"find","query":"let new","step":"first"}),
    );
    settle(&mut presenter, 10, 4);
    assert_eq!(observation(&presenter)["find"]["count"], 1);
    let active = observation(&presenter)["find"]["active"].clone();
    assert_eq!(active, json!({"path":["diff"],"offset":split}));
    command(
        &mut presenter,
        &props,
        2,
        json!({"kind":"reveal_source","path":active["path"],"offset":split,"find_sequence":1}),
    );
    settle(&mut presenter, 10, 4);
    settle(&mut presenter, 10, 4);
    assert_eq!(observation(&presenter)["receipt"]["status"], "applied");
    settle(&mut presenter, 36, 10);
    all(&mut presenter);
    assert_eq!(
        copied(&mut presenter).unwrap(),
        format!("Original\n{}", text.trim_end()).as_bytes()
    );
    close(&mut presenter);
}
#[test]
fn native_diff_rejects_invalid_untrusted_byte_ranges_before_layout() {
    let text = "中😀\n";
    for rows in [
        json!([{"start":1,"end":3,"kind":"added"}]),
        json!([{"start":0,"end":4,"kind":"added"}]),
        json!([{"start":0,"end":9,"kind":"added"}]),
        json!([{"start":3,"end":0,"kind":"added"}]),
        json!([{"start":0,"end":3,"kind":"removed"},{"start":0,"end":7,"kind":"added"}]),
    ] {
        let declaration = diff(text, rows);
        assert!(crate::application::source::compile_diff(&declaration["props"]).is_err());
    }
}

#[test]
fn native_diff_preserves_complete_sources_and_language_hints() {
    for (text, language) in [
        ("中😀\n".to_owned(), "x".repeat(129)),
        ("中😀\n".to_owned(), "unrecognized-language".to_owned()),
        ("x".repeat(65537) + "tail", "rust".to_owned()),
        ("界".repeat(21846) + "😀tail", "rust".to_owned()),
    ] {
        let rows = json!([{"start":0,"end":text.len(),"kind":"added","language":language}]);
        let (_runtime, mut presenter, props) = native(vec![diff(&text, rows)]);
        settle(&mut presenter, 20, 4);
        all(&mut presenter);
        assert_eq!(copied(&mut presenter).unwrap(), text.trim_end().as_bytes());
        let query = if text.ends_with("tail") {
            "tail"
        } else {
            "😀"
        };
        command(
            &mut presenter,
            &props,
            1,
            json!({"kind":"find","query":query,"step":"first"}),
        );
        settle(&mut presenter, 20, 4);
        assert_eq!(
            observation(&presenter)["find"],
            json!({"sequence":1,"count":1,"limited":false,"index":1,"active":{"path":["diff"],"offset":text.find(query).unwrap()}})
        );
        assert_eq!(copied(&mut presenter).unwrap(), text.trim_end().as_bytes());
        close(&mut presenter);
    }
}

#[test]
fn native_comparison_and_patch_share_reading_copy_find_and_resize() {
    for (content, expected, needle) in [
        (
            json!({"kind":"comparison","before":"same\nold 中😀\nend","after":"same\nnew é\nend","language":"rs"}),
            "same\nold 中😀\nnew é\nend",
            "new é",
        ),
        (
            json!({"kind":"patch","source":"@@\n same\n-old 中😀\n+new é","operation":"update","language":"rs"}),
            "@@\nsame\nold 中😀\nnew é",
            "new é",
        ),
        (
            json!({"kind":"patch","source":"@@\n-old\n+new\n*** Delete File: injected","operation":"update","language":null}),
            "@@\n-old\n+new\n*** Delete File: injected",
            "injected",
        ),
    ] {
        let mut declaration = diff("", json!([]));
        declaration["props"]["content"] = content;
        let (_runtime, mut presenter, props) = native(vec![declaration]);
        settle(&mut presenter, 40, 10);
        all(&mut presenter);
        assert_eq!(copied(&mut presenter).unwrap(), expected.as_bytes());
        presenter
            .input_settled(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
            .unwrap();
        command(
            &mut presenter,
            &props,
            1,
            json!({"kind":"find","query":needle,"step":"first"}),
        );
        settle(&mut presenter, 8, 4);
        assert_eq!(observation(&presenter)["find"]["count"], 1);
        let active = observation(&presenter)["find"]["active"].clone();
        assert_eq!(active["offset"], expected.find(needle).unwrap());
        settle(&mut presenter, 40, 10);
        all(&mut presenter);
        assert_eq!(copied(&mut presenter).unwrap(), expected.as_bytes());
        close(&mut presenter);
    }
}

#[test]
fn keyboard_range_crosses_markdown_decorations_into_clipped_literal_and_back() {
    let (_runtime, mut presenter, _) = native(vec![
        markdown("md", "before\n\n```\ncode\n```"),
        source("literal", "tail"),
    ]);
    let buffer = settle(&mut presenter, 30, 8);
    let (x, y) = point(&buffer, "code");
    mouse(
        &mut presenter,
        MouseEventKind::Down(MouseButton::Left),
        x,
        y,
    );
    mouse(&mut presenter, MouseEventKind::Up(MouseButton::Left), x, y);
    presenter
        .input_settled(Event::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::SHIFT,
        )))
        .unwrap();
    settle(&mut presenter, 30, 3);
    settle(&mut presenter, 30, 3);
    // The original code row has a one-cell gutter; vertical movement keeps
    // that visual column when entering the literal document.
    assert_eq!(copied(&mut presenter).unwrap(), b"code\nt");
    presenter
        .input_settled(Event::Key(KeyEvent::new(
            KeyCode::Right,
            KeyModifiers::SHIFT,
        )))
        .unwrap();
    assert_eq!(copied(&mut presenter).unwrap(), b"code\nta");
    presenter
        .input_settled(Event::Key(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT)))
        .unwrap();
    settle(&mut presenter, 30, 3);
    settle(&mut presenter, 30, 3);
    assert_eq!(copied(&mut presenter).unwrap(), b"c");
    close(&mut presenter);
}

#[test]
fn wheel_scrolling_reuses_text_layouts_with_a_scrollbar_and_after_resize() {
    use super::super::super::selection::Layout;
    use std::sync::Arc;

    let text = "Short **bold** `code` 中.\n\n".repeat(100);
    for child in [
        source("text", &text),
        markdown("text", &text),
        diff(&text, json!([])),
    ] {
        let key = child["key"].as_str().unwrap().to_owned();
        // The row probes the available width and then paints the child's
        // intrinsic width, in addition to the reading area's scrollbar probe.
        let row = json!({"kind":"stack","key":"message","direction":"row","gap":0,"padding":0,"background":0,"children":[child]});
        let (_runtime, mut presenter, _) = native(vec![row]);
        let geometry = |presenter: &Presenter| {
            let mount = presenter
                .application
                .child_mount(&presenter.root, &key)
                .unwrap();
            let owner = presenter.resources.owners[&mount.id()]
                .state
                .downcast_ref::<super::super::super::source::Owner>()
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
        let shared = |before: &Layout, after: &Layout| match (before, after) {
            (Layout::Literal(a), Layout::Literal(b)) => Arc::ptr_eq(a, b),
            (Layout::Markdown { lines: a, .. }, Layout::Markdown { lines: b, .. }) => {
                Arc::ptr_eq(a, b)
            }
            _ => false,
        };
        let budget = presenter.resources.budget.clone();
        let mut previous = None;
        for width in [40, 15, 50, 12] {
            settle(&mut presenter, width, 10);
            settle(&mut presenter, width, 10);
            let original = geometry(&presenter);
            if let Some(before) = previous.take() {
                assert!(
                    !shared(&before, &original),
                    "resize must recompute wrapping"
                );
            }
            let retained = budget.usage();
            for kind in [
                MouseEventKind::ScrollDown,
                MouseEventKind::ScrollDown,
                MouseEventKind::ScrollUp,
            ] {
                mouse(&mut presenter, kind, 5, 5);
                settle(&mut presenter, width, 10);
                assert!(
                    shared(&original, &geometry(&presenter)),
                    "wheel scrolling must reuse wrapped rows even with a scrollbar"
                );
                assert_eq!(
                    budget.usage(),
                    retained,
                    "warm scrolling must not retain more layout storage"
                );
            }
            previous = Some(original);
        }
        close(&mut presenter);
        drop(previous);
        assert_eq!(budget.usage(), (0, 0, 0));
    }
}
