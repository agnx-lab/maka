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
use maka_plugin_core::package::{EntryTarget, Implementation};
use std::collections::BTreeMap;

pub(super) fn editor_fixture() -> (tokio::runtime::Runtime, Input, Vec<Package>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let (mut input, packages) = support::resource_fixture();
    input.environment.presentation = Some(PresentationTarget::Tui);
    let packages = packages.into_iter().map(|original| {
        let declaration = original.manifest().application.as_ref().unwrap().declaration().clone();
        let entry = &declaration.entries[&EntryTarget::Tui];
        let mut files = BTreeMap::new();
        if let Implementation::JavaScript {module, ..} = &entry.implementation {
                let mut source = std::str::from_utf8(original.file(module).unwrap()).unwrap()
                    .replace("padding:16,gap:12", "padding:1,gap:1");
                if original.manifest().id == "dynamic.app" {
                    // The capture result remains complete in workflow state;
                    // its disposable status label must obey the scene budget.
                    source = source.replace("state.outcome?.value?.text??", "state.outcome?.value?.text?.slice(0,2048)??");
                    source = source.replace("if(event.kind==='action')", "if(event.key==='reject'){next.reject=event.event.reject;next.observation=null;}if(event.key==='configure'){next.options=event.event;}if(event.kind==='action')")
                        .replace("{kind:'button',key:'remove'", "{kind:'button',key:'reject',label:'Reject observation'},{kind:'button',key:'configure',label:'Configure editor'},{kind:'button',key:'remove'");
                    source.push_str("const normalFrame=frame;frame=(state)=>{const out=normalFrame(state);const editor=out.frame.view.children.find(node=>node.key==='editor');if(editor){editor.props={...editor.props,...state.options};delete editor.props.visible;delete editor.props.prefix;delete editor.props.readingViewport;}out.frame.view.children=out.frame.view.children.map(node=>node.key==='editor'?{kind:'stack',key:'presentation',direction:'column',gap:0,padding:0,background:out.frame.view.background,visible:state.options?.visible!==false,children:[node]}:node);if(state.options?.prefix)out.frame.view.children.unshift({kind:'text',key:'prefix',text:state.options.prefix,color:0xffffff});if(state.options?.readingViewport){const pane=out.frame.view.children.find(node=>node.key==='presentation');pane.scroll='manual';pane.layout={basis:6};pane.children.push({kind:'text',key:'long-tail',text:'tail\\n'.repeat(30),color:0xffffff});}if(state.reject&&state.observation)out.frame.view={kind:'invalid'};return out;};");
                }
                source = source.replace("on:{changed:'changed'}", "on:{changed:'changed',submitted:'submitted',navigated:'navigated'}")
                    .replace("name:'changed',value:event.event", "name:event.name,value:event.event")
                    .replace("else if(event.kind==='component')next.observation=event.event;", "else if(event.kind==='component'){if(event.name==='navigated'){next.navigation=event.event;next.navigations=(state.navigations??0)+1;}else next.observation=event.event;if(event.name==='submitted')next.submissions=(state.submissions??0)+1;}");
                files.insert(module.clone(), source.into_bytes());
        }
        files.insert("maka.extension.json".into(), serde_json::to_vec(&json!({"schemaVersion":1,"id":original.manifest().id,"application":declaration})).unwrap());
        let package = Package::new(files).unwrap();
        for node in input.instances.values_mut().filter(|node| matches!(&node.package,maka_plugin_core::package::PackageSelection::Pinned {package} if package.id == original.manifest().id)) {
            node.package = package.application_ref().into();
        }
        package
    }).collect();
    (runtime, input, packages)
}
fn presenter() -> (tokio::runtime::Runtime, Presenter) {
    let (runtime, input, packages) = editor_fixture();
    let presenter = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    (runtime, presenter)
}

#[test]
fn intrinsic_dialog_editor_grows_shrinks_and_keeps_the_real_caret_and_controls_visible() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let field = json!({"kind":"component","key":"editor","binding":"editor","member":0,"on":{},"props":{
        "appearance":support::brand_appearance()["textarea"],"accessibilityLabel":"Message",
        "readOnly":false,"disabled":false,"softWrap":true,"terminalRows":{"min":3,"max":10},
        "tabIndex":0,"autoFocus":true,"initialText":""}});
    let source = Rc::new(RefCell::new(json!({
        "kind":"dialog","key":"dialog","label":"Edit queued message","open":true,
        "width":72,"height":20,"padding":1,"background_color":0,"border_color":0xffffff,
        "background":{"kind":"button","key":"outside","label":"Outside"},
        "content":{"kind":"stack","key":"content","direction":"column","gap":0,"padding":0,"background":0,
          "children":[{"kind":"stack","key":"form","direction":"column","gap":1,"padding":0,"background":0,"layout":{"grow":1},
            "children":[
              {"kind":"stack","key":"field","direction":"column","gap":0,"padding":0,"background":0,"layout":{"shrink":true},
                "children":[field]},
              {"kind":"text","key":"hint","text":"Ctrl+S to save","color":0xffffff},
              {"kind":"button","key":"back","label":"Save"}
            ]}]
        }
    })));
    let (_runtime, mut tui) = layout::native(
        layout::View::InteractiveScene(source),
        Rc::new(Cell::new(true)),
    );
    draw(&mut tui, 80, 30);
    let mount = tui
        .application
        .child_mount(&tui.root, "editor")
        .unwrap()
        .clone();
    let geometry = |tui: &Presenter| {
        tui.scene
            .iter()
            .find(|target| target.mount == mount)
            .unwrap()
            .area
    };
    let editor = |tui: &Presenter| {
        let owner = editor_owner(&*tui.resources.owners[&mount.id()].state);
        (
            owner.editor.text().to_owned(),
            owner.editor.cursor_position(),
        )
    };
    assert_eq!(geometry(&tui).height, 5); // Original minimum three rows plus inset.
    let focus = tui.focus.clone().unwrap();
    assert_eq!(focus.mount, mount);
    tui.input_settled(Event::Paste(
        (0..20)
            .map(|n| format!("line-{n:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
    .unwrap();
    draw(&mut tui, 80, 30);
    assert_eq!(geometry(&tui).height, 12); // Original maximum ten rows plus inset.
    for (width, height) in [(80, 18), (30, 18), (80, 30)] {
        let buffer = draw(&mut tui, width, height);
        let area = geometry(&tui);
        assert_eq!(area.height, u32::from(area.visible().height));
        let (_, caret) = editor(&tui);
        let caret = caret.expect("last line remains visible inside the resized editor");
        assert!(area.visible().contains(caret));
        assert!(
            buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("line-19")
        );
        assert!(
            tui.scene
                .iter()
                .any(|target| target.key == "back" && !target.area.is_empty())
        );
        assert!(tui.focus.as_ref().unwrap().same(&focus));
        tui.input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: caret.x,
            row: caret.y,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
        tui.input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: caret.x,
            row: caret.y,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
        tui.input_settled(Event::Paste("!".into())).unwrap();
        assert!(editor(&tui).0.ends_with('!'));
    }
    // Width changes use the original soft-wrap algorithm, including wide glyphs.
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    tui.input_settled(Event::Paste("中".repeat(100))).unwrap();
    draw(&mut tui, 80, 30);
    assert_eq!(geometry(&tui).height, 6);
    draw(&mut tui, 30, 30);
    assert_eq!(geometry(&tui).height, 12);
    assert!(editor(&tui).1.is_some());
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    tui.input_settled(Event::Paste("short".into())).unwrap();
    draw(&mut tui, 80, 30);
    assert_eq!(geometry(&tui).height, 5);
    assert_eq!(editor(&tui).0, "short");
    tui.input_settled(key(KeyCode::Tab)).unwrap();
    assert_eq!(tui.focus.as_ref().unwrap().key, "back");
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 1);
    close(&mut tui);
}

#[test]
fn viewport_row_cap_uses_each_paint_without_replacing_the_native_draft_or_focus() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let source = Rc::new(RefCell::new(
        json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
            {"kind":"component","key":"editor","binding":"editor","member":0,"on":{},"props":{
                "appearance":support::brand_appearance()["textarea"],"accessibilityLabel":"Composer","readOnly":false,
                "disabled":false,"softWrap":true,"tabIndex":0,"autoFocus":true,"initialText":"",
                "terminalRows":{"min":1,"max":8,"maxViewportFraction":1.0/3.0}}},
            {"kind":"button","key":"back","label":"Send"}
        ]}),
    ));
    let (_runtime, mut tui) = layout::native(
        layout::View::InteractiveScene(source),
        Rc::new(Cell::new(true)),
    );
    draw(&mut tui, 80, 30);
    let focus = tui.focus.clone().unwrap();
    let mount = focus.mount.clone();
    let height = |tui: &Presenter| {
        tui.scene
            .iter()
            .find(|target| target.mount == mount)
            .unwrap()
            .area
            .height
    };
    assert_eq!(height(&tui), 3);
    tui.input_settled(Event::Paste(
        (0..20)
            .map(|n| format!("line-{n:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
    .unwrap();
    for (width, viewport, expected) in [(80, 30, 10), (25, 18, 8), (40, 12, 6), (80, 30, 10)] {
        let revision = tui.application.frame(&mount).unwrap().revision;
        let buffer = draw(&mut tui, width, viewport);
        assert_eq!(height(&tui), expected);
        assert_eq!(tui.application.frame(&mount).unwrap().revision, revision);
        assert!(tui.focus.as_ref().unwrap().same(&focus));
        assert!(
            buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("line-19")
        );
        let caret = editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .cursor_position()
            .unwrap();
        tui.input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: caret.x,
            row: caret.y,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
        tui.input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: caret.x,
            row: caret.y,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
        tui.input_settled(Event::Paste("!".into())).unwrap();
        assert!(
            editor_owner(&*tui.resources.owners[&mount.id()].state)
                .editor
                .text()
                .ends_with('!')
        );
    }
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    tui.input_settled(Event::Paste("short".into())).unwrap();
    draw(&mut tui, 40, 12);
    assert_eq!(height(&tui), 3);
    close(&mut tui);
}

#[test]
fn composer_boundary_shares_focus_and_keeps_the_borderless_editor_at_its_actual_cells() {
    use ratatui::style::Color;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let mut appearance = support::brand_appearance()["textarea"].clone();
    appearance["borderWidth"] = json!(0);
    let field = json!({"kind":"component","key":"editor","binding":"editor","member":0,"on":{},"props":{
        "appearance":appearance,"accessibilityLabel":"Composer","readOnly":false,"disabled":false,"softWrap":true,
        "tabIndex":0,"autoFocus":true,"initialText":"","terminalRows":{"min":1,"max":8,"maxViewportFraction":1.0/3.0}}});
    let composer = json!({"kind":"stack","key":"composer","direction":"row","gap":0,"padding":0,"background":0,
        "border":{"color":0x112233,"focused_color":0xabcdef,"style":"rounded"},"children":[
          {"kind":"stack","key":"editor-area","direction":"column","gap":0,"padding":0,"background":0,"layout":{"grow":1,"shrink":true},"children":[field]},
          {"kind":"button","key":"send","label":"Send"}]});
    let source = json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[composer,
        {"kind":"button","key":"back","label":"Outside"}]});
    let (_runtime, mut tui) = layout::native(
        layout::View::InteractiveScene(Rc::new(RefCell::new(source))),
        Rc::new(Cell::new(true)),
    );
    draw(&mut tui, 40, 16);
    let buffer = draw(&mut tui, 40, 16);
    let focus = tui.focus.clone().unwrap();
    let area = |tui: &Presenter| {
        tui.scene
            .iter()
            .find(|target| target.mount == focus.mount)
            .unwrap()
            .area
    };
    assert_eq!(
        (
            area(&tui).x,
            area(&tui).y,
            area(&tui).width,
            area(&tui).height
        ),
        (1, 1, 30, 1)
    );
    assert_eq!(buffer[(0, 0)].symbol(), "╭");
    assert_eq!(buffer[(39, 2)].symbol(), "╯");
    assert_eq!(buffer[(0, 0)].fg, Color::Rgb(0xab, 0xcd, 0xef));
    assert_eq!(buffer[(33, 1)].symbol(), "S");
    for (key, expected) in [
        ("send", Color::Rgb(0xab, 0xcd, 0xef)),
        ("back", Color::Rgb(0x11, 0x22, 0x33)),
    ] {
        let target = tui
            .scene
            .iter()
            .find(|target| target.key == key)
            .unwrap()
            .area
            .visible();
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            tui.input_settled(Event::Mouse(MouseEvent {
                kind,
                column: target.x,
                row: target.y,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
        }
        assert_eq!(draw(&mut tui, 40, 16)[(0, 0)].fg, expected);
    }
    let target = area(&tui).visible();
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        tui.input_settled(Event::Mouse(MouseEvent {
            kind,
            column: target.x,
            row: target.y,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
    }
    tui.input_settled(Event::Paste(
        (0..20)
            .map(|n| format!("line-{n:02}"))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
    .unwrap();
    for (width, height, expected_rows) in [(40, 16, 5), (20, 12, 4), (80, 30, 8)] {
        let buffer = draw(&mut tui, width, height);
        assert_eq!(area(&tui).height, expected_rows);
        assert!(tui.focus.as_ref().unwrap().same(&focus));
        assert!(
            buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("line-19")
        );
        let caret = editor_owner(&*tui.resources.owners[&focus.mount.id()].state)
            .editor
            .cursor_position()
            .unwrap();
        assert!(area(&tui).contains(caret));
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            tui.input_settled(Event::Mouse(MouseEvent {
                kind,
                column: caret.x,
                row: caret.y,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
        }
        tui.input_settled(Event::Paste("!".into())).unwrap();
        assert!(
            editor_owner(&*tui.resources.owners[&focus.mount.id()].state)
                .editor
                .text()
                .ends_with('!')
        );
    }
    close(&mut tui);
}

#[test]
fn session_viewport_uses_remaining_body_without_leaking_to_siblings_or_cached_measurements() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let long = (0..20)
        .map(|n| format!("line-{n:02}"))
        .collect::<Vec<_>>()
        .join("\n");
    let field = |key: &str, autofocus: bool| {
        json!({"kind":"component","key":key,"binding":"editor","member":0,"on":{},"props":{
        "appearance":support::brand_appearance()["textarea"],"accessibilityLabel":key,"readOnly":false,
        "disabled":false,"softWrap":true,"tabIndex":if autofocus {1} else {-1},"autoFocus":autofocus,"initialText":long,
        "terminalRows":{"min":1,"max":8,"maxViewportFraction":1.0/3.0}}})
    };
    let rows = json!({"kind":"component","key":"rows","binding":"rows","member":0,"on":{},"props":{
        "revision":1,"entry":"row-5","rows":(0..6).map(|n|json!({"key":format!("row-{n}"),"text":format!("pending {n}"),"actions":[]})).collect::<Vec<_>>(),
        "shortcuts":[],"maxRows":3,"compactBelow":{"height":12,"maxRows":1},"tabIndex":2,
        "foreground":0xffffffffu32,"muted":0x888888ffu32,"selected":0x222222ffu32,"hovered":0x333333ffu32,
        "scrollbar":{"thumb":"┃","track":"│","thumbColor":0xffffffffu32,"trackColor":0x888888ffu32}}});
    let body = json!({"kind":"stack","key":"body","direction":"column","gap":0,"padding":1,"background":0,"viewport":true,"layout":{"grow":1},"children":[
        {"kind":"stack","key":"reading","direction":"column","gap":0,"padding":0,"background":0,"layout":{"grow":1,"basis":1},"children":[]},
        rows,
        {"kind":"stack","key":"wrapper","direction":"column","gap":0,"padding":0,"background":0,"children":[field("editor",true)]},
        {"kind":"button","key":"back","label":"Send"}
    ]});
    let source = Rc::new(RefCell::new(
        json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
            {"kind":"text","key":"header","text":"Header\nHeader\nHeader","color":0xffffff}, body, field("outside",false)
        ]}),
    ));
    let (_runtime, mut tui) = layout::native(
        layout::View::InteractiveScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    draw(&mut tui, 40, 30);
    let focus = tui.focus.clone().unwrap();
    let height = |tui: &Presenter, child: &str| {
        let mount = tui.application.child_mount(&tui.root, child).unwrap();
        tui.scene
            .iter()
            .find(|target| target.mount == *mount)
            .unwrap()
            .area
            .height
    };
    for (header, editor_rows, queue_rows) in [(3, 7, 3), (6, 6, 3), (7, 5, 1), (3, 7, 3)] {
        source.borrow_mut()["children"][0]["text"] = json!(vec!["Header"; header].join("\n"));
        let root = tui.root.clone();
        let revision = tui.application.frame(&root).unwrap().revision;
        tui.application
            .dispatch(&root, revision, "back", json!({"kind":"click"}))
            .unwrap();
        tui.sync_native().unwrap();
        let buffer = draw(&mut tui, 40, 30);
        assert_eq!(height(&tui, "editor"), editor_rows);
        assert_eq!(height(&tui, "rows"), queue_rows);
        assert_eq!(
            height(&tui, "outside"),
            10,
            "sibling keeps the surface viewport"
        );
        assert!(tui.focus.as_ref().unwrap().same(&focus));
        assert!(
            buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("line-19")
        );
    }
    let cache = super::super::paint::Measurements::guarded(32);
    let mount = tui.application.child_mount(&tui.root, "editor").unwrap();
    for (height, expected) in [(12, 6), (30, 10), (12, 6)] {
        assert_eq!(
            tui.measure_mount(
                mount,
                38,
                cache.context(ratatui::layout::Size { width: 38, height })
            )
            .unwrap()
            .1,
            expected
        );
    }
    let node = tui
        .application
        .frame(&tui.root)
        .unwrap()
        .view
        .view
        .scene()
        .unwrap();
    let super::super::scene::Node::Stack { children, .. } = node else {
        panic!("root stack")
    };
    assert_eq!(
        tui.measure(
            &tui.root,
            &children[1],
            40,
            cache.context(ratatui::layout::Size {
                width: 40,
                height: 30
            })
        )
        .unwrap(),
        (2, 2),
        "contained descendants cannot inflate the body's intrinsic size"
    );
    tui.input_settled(Event::Paste("!".into())).unwrap();
    assert!(
        editor_owner(&*tui.resources.owners[&focus.mount.id()].state)
            .editor
            .text()
            .ends_with("line-19!")
    );
    close(&mut tui);
}

#[test]
fn terminal_row_limits_resize_the_original_editor_and_reject_inverted_ranges() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 40);
    tui.input_settled(Event::Paste("\nline".repeat(20)))
        .unwrap();
    draw(&mut tui, 50, 40);
    let mount = draft(&tui);
    let height = |tui: &Presenter| {
        tui.scene
            .iter()
            .find(|target| target.mount == mount)
            .unwrap()
            .area
            .height
    };
    assert_eq!(height(&tui), 10); // Other products retain the default eight text rows.
    let original = observation(&tui);
    dispatch(
        &mut tui,
        "configure",
        json!({"terminalRows":{"min":1,"max":2}}),
    );
    draw(&mut tui, 50, 40);
    assert_eq!(height(&tui), 4);
    assert_eq!(observation(&tui), original);
    for fraction in [0.0, -0.5, 1.1] {
        let revision = tui.application.frame(&tui.root).unwrap().revision;
        assert!(
            tui.application
                .dispatch(
                    &tui.root.clone(),
                    revision,
                    "configure",
                    json!({"terminalRows":{"min":1,"max":8,"maxViewportFraction":fraction}})
                )
                .is_ok()
        );
        assert!(tui.settle().is_err());
        assert_eq!(tui.application.frame(&tui.root).unwrap().revision, revision);
    }
    let revision = tui.application.frame(&tui.root).unwrap().revision;
    assert!(
        tui.application
            .dispatch(
                &tui.root.clone(),
                revision,
                "configure",
                json!({"terminalRows":{"min":4,"max":3}})
            )
            .is_ok()
    );
    assert!(tui.settle().is_err());
    assert_eq!(tui.application.frame(&tui.root).unwrap().revision, revision);
    draw(&mut tui, 50, 40);
    assert_eq!(height(&tui), 4);
    assert_eq!(observation(&tui), original);
    close(&mut tui);
}

#[test]
fn full_capacity_reset_captures_every_byte_and_oversize_reset_preserves_original_draft() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    let original = observation(&tui);
    let text = "中".repeat(21845) + "x";
    assert_eq!(text.len(), 65536);
    dispatch(
        &mut tui,
        "configure",
        json!({"command":{
            "owner":original["snapshot"]["text"]["owner"],"sequence":1,
            "basedOn":original["revision"],"edit":{"kind":"reset","text":text}
        }}),
    );
    let full = observation(&tui);
    assert_eq!(full["receipt"]["outcome"]["status"], "applied");
    assert_eq!(full["snapshot"]["text"]["bytes"], 65536);
    dispatch(
        &mut tui,
        "read",
        json!({"end":65536,"capture":{
            "kind":"at_revision","owner":full["snapshot"]["text"]["owner"],"revision":full["revision"]
        }}),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while tui.application.frame(&tui.root).unwrap().state["outcome"].is_null() {
        assert!(std::time::Instant::now() < deadline);
        tui.pump_settled().unwrap();
        std::thread::yield_now();
    }
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["outcome"]["value"]["text"],
        text
    );
    // Within the schema's character budget but above the native UTF-8 byte budget.
    dispatch(
        &mut tui,
        "configure",
        json!({"command":{
            "owner":full["snapshot"]["text"]["owner"],"sequence":2,
            "basedOn":full["revision"],"edit":{"kind":"reset","text":"中".repeat(21846)}
        }}),
    );
    let kept = observation(&tui);
    assert_eq!(
        kept["receipt"]["outcome"],
        json!({"status":"conflict","reason":"representation"})
    );
    assert_eq!(kept["snapshot"], full["snapshot"]);
    close(&mut tui);
}
fn draft(tui: &Presenter) -> Mount {
    let composer = tui.application.child_mount(&tui.root, "editor").unwrap();
    tui.application
        .child_mount(composer, "draft")
        .unwrap()
        .clone()
}
fn observation(tui: &Presenter) -> Value {
    tui.application.frame(&tui.root).unwrap().state["observation"].clone()
}
fn click(tui: &mut Presenter, kind: MouseEventKind, x: u16, y: u16) {
    tui.input_settled(Event::Mouse(MouseEvent {
        kind,
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    }))
    .unwrap();
}
fn dispatch(tui: &mut Presenter, key: &str, value: Value) {
    let revision = tui.application.frame(&tui.root).unwrap().revision;
    tui.application
        .dispatch(&tui.root, revision, key, value)
        .unwrap();
    tui.pump_settled().unwrap();
}
fn close(tui: &mut Presenter) {
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    let scheduler = tui.application.scheduler().clone();
    scheduler.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            std::future::poll_fn(|cx| {
                tui.application.set_waker(cx.waker().clone());
                tui.pump().unwrap();
                // File-paste workers can still own cleanup after the surface closes.
                if tui.can_shutdown() {
                    std::task::Poll::Ready(())
                } else {
                    std::task::Poll::Pending
                }
            }),
        )
        .await
        .expect("original textarea workers did not close");
    });
    tui.shutdown_settled().unwrap();
}

#[test]
fn native_unicode_selection_and_undo_survive_component_updates_and_stale_owner_cannot_edit() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    let original = observation(&tui)["snapshot"]["text"]["owner"].clone();
    tui.input_settled(Event::Paste("中文👩🏽‍💻e\u{301}".into()))
        .unwrap();
    let observed = observation(&tui);
    assert_eq!(observed["snapshot"]["text"]["owner"], original);
    assert_eq!(
        observed["snapshot"]["selection"]["end"],
        "Start中文👩🏽‍💻e\u{301}".encode_utf16().count()
    );
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    assert_eq!(observation(&tui)["snapshot"]["text"]["bytes"], 5);
    let mount = draft(&tui);
    // A captured target cannot gain authority over a replacement physical owner.
    tui.resources.close(&mount).unwrap();
    tui.settle().unwrap();
    assert_ne!(observation(&tui)["snapshot"]["text"]["owner"], original);
    tui.input_settled(Event::Paste("foreign".into())).unwrap();
    assert_eq!(observation(&tui)["snapshot"]["text"]["bytes"], 5);
    close(&mut tui);
}

#[test]
fn failed_flush_keeps_previous_geometry_and_new_document_rejects_old_pointer_mapping() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    let target = tui.scene[0].clone();
    let x = target.area.visible().x + 1;
    let y = target.area.visible().y + 1;
    // Paint a completely different viewport without publishing a flush.
    let area = Rect::new(10, 5, 30, 14);
    let mut buffer = Buffer::empty(area);
    let _unflushed = tui.paint(area, &mut buffer).unwrap();
    click(&mut tui, MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(observation(&tui)["snapshot"]["selection"]["end"], 0);
    click(&mut tui, MouseEventKind::Drag(MouseButton::Left), x + 3, y);
    assert_eq!(
        observation(&tui)["snapshot"]["selection"],
        json!({"start":0,"end":3})
    );
    click(&mut tui, MouseEventKind::Up(MouseButton::Left), x + 3, y);
    tui.input_settled(Event::Paste("中".into())).unwrap();
    let after_edit = observation(&tui);
    click(&mut tui, MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(observation(&tui), after_edit);
    draw(&mut tui, 50, 20);
    click(&mut tui, MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(observation(&tui)["snapshot"]["selection"]["end"], 0);
    close(&mut tui);
}

#[test]
fn forwarding_capture_precedes_same_frame_reset_and_stale_reset_preserves_later_edits() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    let revision = observation(&tui)["revision"].as_u64().unwrap();
    let original_owner = observation(&tui)["snapshot"]["text"]["owner"].clone();
    dispatch(
        &mut tui,
        "read",
        json!({"command":{"owner":original_owner,"sequence":1,"basedOn":revision,"edit":{"kind":"reset","text":""}}}),
    );
    assert_eq!(observation(&tui)["snapshot"]["text"]["bytes"], 0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while tui.application.frame(&tui.root).unwrap().state["outcome"].is_null() {
        assert!(std::time::Instant::now() < deadline);
        tui.pump_settled().unwrap();
        std::thread::yield_now();
    }
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["outcome"]["value"]["text"],
        "Start"
    );
    tui.input_settled(Event::Paste("新草稿".into())).unwrap();
    let current = observation(&tui)["revision"].as_u64().unwrap();
    // Capture against the accepted current revision, but clear against an old
    // revision: immutable read succeeds and the later native draft remains.
    let original_owner = observation(&tui)["snapshot"]["text"]["owner"].clone();
    dispatch(
        &mut tui,
        "read",
        json!({"end":9,"capture":{"kind":"at_revision","owner":original_owner,"revision":current},"command":{"owner":original_owner,"sequence":2,"basedOn":revision,"edit":{"kind":"reset","text":""}}}),
    );
    let observed = observation(&tui);
    assert_eq!(observed["snapshot"]["text"]["bytes"], 9);
    assert_eq!(
        observed["receipt"]["outcome"],
        json!({"status":"conflict","reason":"revision"})
    );
    dispatch(&mut tui, "remove", json!({}));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !tui.jobs.is_empty() {
        assert!(std::time::Instant::now() < deadline);
        tui.pump_settled().unwrap();
        std::thread::yield_now();
    }
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["outcome"]["value"]["text"],
        "新草稿"
    );
    close(&mut tui);
}

#[test]
fn non_tab_stop_keeps_mouse_editing_and_readonly_disabled_gate_before_edit() {
    let (_runtime, mut tui) = presenter();
    dispatch(&mut tui, "configure", json!({"tabIndex":-1}));
    draw(&mut tui, 50, 24);
    assert_eq!(tui.focus.as_ref().unwrap().key, "read");
    let target = tui
        .scene
        .iter()
        .find(|target| target.key == "observe")
        .unwrap()
        .clone();
    click(
        &mut tui,
        MouseEventKind::Down(MouseButton::Left),
        target.area.visible().x + 1,
        target.area.visible().y + 1,
    );
    tui.input_settled(Event::Paste("中".into())).unwrap();
    assert_eq!(observation(&tui)["snapshot"]["text"]["bytes"], 8);
    tui.input_settled(key(KeyCode::Tab)).unwrap();
    assert_ne!(tui.focus.as_ref().unwrap().key, "observe");
    dispatch(&mut tui, "configure", json!({"tabIndex":2,"readOnly":true}));
    draw(&mut tui, 50, 24);
    tui.input_settled(key(KeyCode::BackTab)).unwrap();
    assert_eq!(tui.focus.as_ref().unwrap().key, "observe");
    let before = observation(&tui);
    tui.input_settled(Event::Paste("blocked".into())).unwrap();
    assert_eq!(observation(&tui), before);
    dispatch(&mut tui, "configure", json!({"tabIndex":0,"disabled":true}));
    // Old successfully presented geometry is still visible, but current native
    // props must reject editing before touching the original buffer.
    tui.input_settled(Event::Paste("blocked".into())).unwrap();
    assert_eq!(observation(&tui), before);
    close(&mut tui);
}

#[test]
fn failed_observation_preserves_edits_and_original_command_receipt_until_publication() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 24);
    let revision = observation(&tui)["revision"].as_u64().unwrap();
    let original_owner = observation(&tui)["snapshot"]["text"]["owner"].clone();
    dispatch(&mut tui, "reject", json!({"reject":true}));
    dispatch(
        &mut tui,
        "read",
        json!({"command":{"owner":original_owner,"sequence":1,"basedOn":revision,"edit":{"kind":"reset","text":""}}}),
    );
    assert!(tui.surface.is_live());
    assert!(observation(&tui).is_null());
    tui.input_settled(Event::Paste("Later".into())).unwrap();
    assert!(tui.surface.is_live());
    dispatch(&mut tui, "reject", json!({"reject":false}));
    let observed = observation(&tui);
    assert_eq!(observed["receipt"]["sequence"], 1);
    assert_eq!(observed["receipt"]["outcome"]["status"], "applied");
    assert_eq!(observed["snapshot"]["text"]["bytes"], 5);
    tui.pump_settled().unwrap();
    assert_eq!(observation(&tui), observed);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !tui.jobs.is_empty() {
        assert!(std::time::Instant::now() < deadline);
        tui.pump_settled().unwrap();
        std::thread::yield_now();
    }
    close(&mut tui);
}

#[test]
fn wheel_hits_visible_editor_without_stealing_button_focus_or_snapping_back() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 30);
    let text = (0..20).map(|i| format!("\nrow {i:02}")).collect::<String>();
    tui.input_settled(Event::Paste(text)).unwrap();
    draw(&mut tui, 50, 30);
    tui.input_settled(key(KeyCode::Tab)).unwrap();
    assert_eq!(tui.focus.as_ref().unwrap().key, "read");
    let before = draw(&mut tui, 50, 30);
    let editor = tui
        .scene
        .iter()
        .find(|target| target.key == "observe")
        .unwrap()
        .clone();
    click(
        &mut tui,
        MouseEventKind::ScrollUp,
        editor.area.visible().x + 1,
        editor.area.visible().y + 1,
    );
    assert_eq!(tui.focus.as_ref().unwrap().key, "read");
    let after = draw(&mut tui, 50, 30);
    assert_ne!(before, after);
    assert_eq!(after, draw(&mut tui, 50, 30));
    close(&mut tui);
}

#[test]
fn a_different_editor_with_equal_revision_cannot_capture_or_clear_the_original_draft() {
    let (_runtime_a, mut a) = presenter();
    let (_runtime_b, mut b) = presenter();
    draw(&mut a, 50, 20);
    draw(&mut b, 50, 20);
    let original = observation(&a);
    let replacement = observation(&b);
    assert_eq!(original["revision"], replacement["revision"]);
    assert_eq!(
        original["snapshot"]["text"]["bytes"],
        replacement["snapshot"]["text"]["bytes"]
    );
    assert_ne!(
        original["snapshot"]["text"]["owner"],
        replacement["snapshot"]["text"]["owner"]
    );
    let owner = &original["snapshot"]["text"]["owner"];
    let revision = &original["revision"];
    dispatch(
        &mut b,
        "configure",
        json!({"command":{"owner":owner,"sequence":900,"basedOn":revision,"edit":{"kind":"reset","text":""}}}),
    );
    assert_eq!(
        observation(&b)["receipt"]["outcome"],
        json!({"status":"conflict","reason":"owner"})
    );
    assert_eq!(observation(&b)["snapshot"]["text"]["bytes"], 5);
    dispatch(
        &mut b,
        "read",
        json!({"capture":{"kind":"at_revision","owner":owner,"revision":revision}}),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while b.application.frame(&b.root).unwrap().state["outcome"].is_null() {
        assert!(std::time::Instant::now() < deadline);
        b.pump_settled().unwrap();
        std::thread::yield_now();
    }
    assert_eq!(
        b.application.frame(&b.root).unwrap().state["outcome"]["status"],
        "not_accepted"
    );
    let current_owner = observation(&b)["snapshot"]["text"]["owner"].clone();
    dispatch(
        &mut b,
        "configure",
        json!({"command":{"owner":current_owner,"sequence":1,"basedOn":revision,"edit":{"kind":"reset","text":""}}}),
    );
    assert_eq!(
        observation(&b)["receipt"]["outcome"],
        json!({"status":"applied"})
    );
    assert_eq!(observation(&b)["snapshot"]["text"]["bytes"], 0);
    close(&mut a);
    close(&mut b);
}

#[test]
fn completely_clipped_editor_cannot_receive_focus_or_physical_input() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    let mount = draft(&tui);
    dispatch(
        &mut tui,
        "configure",
        json!({"prefix":"heading\n".repeat(40)}),
    );
    draw(&mut tui, 50, 20);
    assert!(!tui.scene.iter().any(|target| target.mount == mount));
    assert!(
        tui.focus
            .as_ref()
            .is_none_or(|target| target.mount != mount)
    );
    let before = observation(&tui);
    tui.input_settled(Event::Paste("invisible edit".into()))
        .unwrap();
    assert_eq!(observation(&tui), before);
    close(&mut tui);
}
#[test]
fn overflowing_editor_keeps_boundary_wheels_inside_its_scrollable_parent() {
    let (_runtime, mut tui) = presenter();
    dispatch(&mut tui, "configure", json!({"readingViewport":true}));
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Paste(
        (0..20).map(|i| format!("\nrow {i:02}")).collect(),
    ))
    .unwrap();
    draw(&mut tui, 50, 20);
    let mount = draft(&tui);
    let target = tui
        .scene
        .iter()
        .find(|target| target.mount == mount)
        .unwrap();
    let point = target.area.visible();
    let before = observation(&tui);
    for direction in [MouseEventKind::ScrollUp, MouseEventKind::ScrollDown] {
        for _ in 0..25 {
            click(&mut tui, direction, point.x + 1, point.y + 1);
            draw(&mut tui, 50, 20);
            let state = tui.resources.owners[&tui.root.id()]
                .state
                .downcast_ref::<super::super::scroll::SceneState>()
                .unwrap();
            assert_eq!(state.scrolls["presentation"].offset, 0);
        }
    }
    assert_eq!(observation(&tui), before);
    close(&mut tui);
}

#[test]
fn fitting_editor_leaves_wheel_scrolling_to_its_parent() {
    let (_runtime, mut tui) = presenter();
    dispatch(&mut tui, "configure", json!({"readingViewport":true}));
    draw(&mut tui, 50, 20);
    let mount = draft(&tui);
    let point = tui
        .scene
        .iter()
        .find(|target| target.mount == mount)
        .unwrap()
        .area
        .visible();
    click(
        &mut tui,
        MouseEventKind::ScrollDown,
        point.x + 1,
        point.y + 1,
    );
    let state = tui.resources.owners[&tui.root.id()]
        .state
        .downcast_ref::<super::super::scroll::SceneState>()
        .unwrap();
    assert!(state.scrolls["presentation"].offset > 0);
    close(&mut tui);
}

#[test]
fn unhandled_editor_page_key_reaches_its_original_scene_scroll_ancestor() {
    let (_runtime, mut tui) = presenter();
    dispatch(&mut tui, "configure", json!({"readingViewport":true}));
    draw(&mut tui, 50, 20);
    let mount = draft(&tui);
    let target = tui
        .scene
        .iter()
        .find(|target| target.mount == mount)
        .unwrap()
        .clone();
    tui.focus = Some(target);
    let before = observation(&tui);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::PageDown,
        KeyModifiers::NONE,
    )))
    .unwrap();
    let state = tui.resources.owners[&tui.root.id()]
        .state
        .downcast_ref::<super::super::scroll::SceneState>()
        .unwrap();
    assert_eq!(state.scrolls["presentation"].offset, 6);
    assert_eq!(observation(&tui), before);
    close(&mut tui);
}

#[test]
fn pointer_capture_survives_tab_focus_change_and_release_keeps_the_new_focus() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    let mount = draft(&tui);
    let target = tui
        .scene
        .iter()
        .find(|target| target.mount == mount)
        .unwrap()
        .clone();
    let point = target.area.visible();
    click(
        &mut tui,
        MouseEventKind::Down(MouseButton::Left),
        point.x + 1,
        point.y + 1,
    );
    assert!(
        tui.drag
            .as_ref()
            .is_some_and(|captured| captured.same(&target))
    );
    tui.input_settled(Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)))
        .unwrap();
    let next = tui.focus.clone().unwrap();
    assert!(!next.same(&target));
    click(
        &mut tui,
        MouseEventKind::Drag(MouseButton::Left),
        point.x + 4,
        point.y + 1,
    );
    assert_eq!(
        observation(&tui)["snapshot"]["selection"],
        json!({"start":0,"end":3})
    );
    assert!(tui.focus.as_ref().unwrap().same(&next));
    click(
        &mut tui,
        MouseEventKind::Up(MouseButton::Left),
        point.x + 4,
        point.y + 1,
    );
    assert!(tui.drag.is_none());
    assert!(tui.focus.as_ref().unwrap().same(&next));
    close(&mut tui);
}

#[test]
fn terminal_navigation_is_revision_bound_and_preserves_native_editing() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Paste("/co".into())).unwrap();
    let original = observation(&tui);
    let navigation =
        json!({"owner":original["snapshot"]["text"]["owner"],"basedOn":original["revision"]});
    dispatch(&mut tui, "configure", json!({"navigation":navigation}));
    draw(&mut tui, 50, 20);
    let focus = tui.focus.clone().unwrap();
    for (index, code) in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Tab,
        KeyCode::Enter,
        KeyCode::Esc,
    ]
    .into_iter()
    .enumerate()
    {
        tui.input_settled(key(code)).unwrap();
        assert_eq!(observation(&tui), original);
        assert!(tui.focus.as_ref().unwrap().same(&focus));
        assert_eq!(
            tui.application.frame(&tui.root).unwrap().state["navigations"],
            index + 1
        );
        assert_eq!(
            tui.application.frame(&tui.root).unwrap().state["navigation"]["observation"],
            original
        );
    }
    tui.input_settled(Event::Paste("x".into())).unwrap();
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["navigations"],
        5
    );
    assert_eq!(
        observation(&tui)["snapshot"]["text"]["bytes"],
        original["snapshot"]["text"]["bytes"].as_u64().unwrap() + 2
    );
    let current = observation(&tui);
    dispatch(
        &mut tui,
        "configure",
        json!({"navigation":{"owner":"retired-owner","basedOn":current["revision"]}}),
    );
    draw(&mut tui, 50, 20);
    tui.input_settled(key(KeyCode::Up)).unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["navigations"],
        5
    );
    assert_ne!(
        observation(&tui)["snapshot"]["selection"],
        current["snapshot"]["selection"]
    );
    close(&mut tui);
}

#[test]
fn terminal_textarea_submit_keeps_native_draft_and_obeys_current_editor_policy() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    dispatch(&mut tui, "configure", json!({"submit":"control_enter"}));
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Paste("中文🙂".into())).unwrap();
    let original = observation(&tui);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submissions"],
        1
    );
    assert_eq!(observation(&tui), original);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submissions"],
        1
    );
    assert_eq!(
        observation(&tui)["snapshot"]["text"]["bytes"],
        original["snapshot"]["text"]["bytes"].as_u64().unwrap() + 1
    );
    for options in [
        json!({"submit":null}),
        json!({"submit":"control_enter","readOnly":true}),
        json!({"submit":"control_enter","disabled":true}),
    ] {
        dispatch(&mut tui, "configure", options);
        draw(&mut tui, 50, 20);
        tui.input_settled(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::CONTROL,
        )))
        .unwrap();
        assert_eq!(
            tui.application.frame(&tui.root).unwrap().state["submissions"],
            1
        );
    }
    close(&mut tui);
}

#[test]
fn terminal_enter_submit_keeps_shift_enter_control_j_and_paste_in_the_original_draft() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    dispatch(&mut tui, "configure", json!({"submit":"enter"}));
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Paste("first\n第二行".into()))
        .unwrap();
    for event in [
        KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT),
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
    ] {
        let before = observation(&tui)["snapshot"]["text"]["bytes"]
            .as_u64()
            .unwrap();
        tui.input_settled(Event::Key(event)).unwrap();
        assert_eq!(observation(&tui)["snapshot"]["text"]["bytes"], before + 1);
        assert_eq!(
            tui.application.frame(&tui.root).unwrap().state["submissions"]
                .as_u64()
                .unwrap_or(0),
            0
        );
    }
    let draft = observation(&tui);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submissions"],
        1
    );
    assert_eq!(observation(&tui), draft);
    tui.input_settled(Event::Key(KeyEvent::new_with_kind(
        KeyCode::Enter,
        KeyModifiers::NONE,
        crossterm::event::KeyEventKind::Repeat,
    )))
    .unwrap();
    assert_eq!(observation(&tui), draft);
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submissions"],
        1
    );
    dispatch(
        &mut tui,
        "configure",
        json!({"submit":"enter","filePaste":{"context":"original-session","allowFiles":true}}),
    );
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Paste("/missing-maka-paste-test.txt".into()))
        .unwrap();
    // A physical file paste blocks even an immediate submit, before another
    // paint or a parent files observation can disable the product's Send.
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(observation(&tui), draft);
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submissions"],
        1
    );
    close(&mut tui);
}

#[test]
fn failed_submit_is_not_replayed_and_preserves_unpublished_command_receipt() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 24);
    let original = observation(&tui);
    dispatch(&mut tui, "reject", json!({"reject":true}));
    dispatch(
        &mut tui,
        "configure",
        json!({"submit":"control_enter","command":{
            "owner":original["snapshot"]["text"]["owner"],"sequence":1,
            "basedOn":original["revision"],"edit":{"kind":"reset","text":"Ready"}
        }}),
    );
    let editor = draft(&tui);
    assert_eq!(
        tui.application.frame(&editor).unwrap().state["pending"],
        true
    );
    assert!(
        tui.input_settled(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::CONTROL,
        )))
        .is_err()
    );
    assert!(tui.surface.is_live());
    assert_eq!(
        tui.application.frame(&editor).unwrap().state["pending"],
        true
    );
    tui.pump_settled().unwrap();
    dispatch(&mut tui, "reject", json!({"reject":false}));
    let published = observation(&tui);
    assert_eq!(published["receipt"]["sequence"], 1);
    assert_eq!(published["receipt"]["outcome"]["status"], "applied");
    assert_eq!(published["snapshot"]["text"]["bytes"], 5);
    assert_eq!(
        tui.application.frame(&editor).unwrap().state["pending"],
        false
    );
    for _ in 0..3 {
        tui.pump_settled().unwrap();
        assert!(tui.application.frame(&tui.root).unwrap().state["submissions"].is_null());
    }
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submissions"],
        1
    );
    close(&mut tui);
}

#[test]
fn committed_addition_preserves_native_draft_selection_focus_and_undo() {
    use maka_event_log::{
        EventLog,
        root::{RootNamespaces, RootOwner},
    };
    use std::{rc::Rc, sync::Arc};
    let (runtime, mut input, packages) = editor_fixture();
    let dir = tempfile::tempdir().unwrap();
    let namespaces = RootNamespaces {
        ownership: dir.path().join("owners"),
        control: dir.path().join("control"),
    };
    let store = Arc::new(
        runtime
            .block_on(EventLog::for_root(Arc::new(
                RootOwner::create(&dir.path().join("root"), &namespaces).unwrap(),
            )))
            .unwrap(),
    );
    let deployment = Uuid::new_v4();
    let original = runtime
        .block_on(store.commit_application(deployment, 0, input.clone(), vec![], packages.clone()))
        .unwrap();
    let mut tui = runtime
        .block_on(Presenter::start_from_store(
            store.clone(),
            original,
            runtime.handle().clone(),
            registry(),
            json!({}),
            vec![],
            maka_ui_js::execution(Default::default()),
        ))
        .unwrap();
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Paste("中文👩🏽‍💻e\u{301}".into()))
        .unwrap();
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Left,
        KeyModifiers::SHIFT,
    )))
    .unwrap();
    let before = observation(&tui);
    let mount = draft(&tui);
    let identity = tui.resources.owners[&mount.id()].identity.clone();
    let focused = tui.focus.as_ref().unwrap().mount.clone();
    let mut extra = input.instances["editor"].clone();
    extra.key = Uuid::new_v4();
    input.instances.insert("unused-editor".into(), extra);
    let next = runtime
        .block_on(store.commit_application(deployment, 1, input.clone(), vec![], packages.clone()))
        .unwrap();
    let load = tui.load_committed_update(next.clone()).unwrap();
    // Rendering and input remain available while the immutable snapshot loads.
    draw(&mut tui, 50, 20);
    let update = runtime.block_on(load).unwrap();
    tui.adopt_update(update, vec![]).unwrap();
    tui.pump_settled().unwrap();
    draw(&mut tui, 50, 20);
    assert_eq!(tui.deployment_status().effective, Some(next));
    assert_eq!(draft(&tui), mount);
    assert!(Rc::ptr_eq(
        &identity,
        &tui.resources.owners[&mount.id()].identity
    ));
    assert_eq!(tui.focus.as_ref().unwrap().mount, focused);
    assert_eq!(observation(&tui), before);

    let original = packages
        .iter()
        .find(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let mut declaration = original
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    if let Implementation::JavaScript { realm, .. } = &mut declaration
        .entries
        .get_mut(&EntryTarget::Tui)
        .unwrap()
        .implementation
    {
        *realm = maka_plugin_core::package::Realm::Dedicated;
    }
    let failing = support::package(
        "test.failed-addition",
        declaration,
        Some("export function activate(){throw Error('new isolated plugin failed');}".into()),
    );
    let mut extra = input.instances["app"].clone();
    extra.key = Uuid::new_v4();
    extra.package = failing.application_ref().into();
    input.instances.insert("failed-addition".into(), extra);
    let mut bytes = packages;
    bytes.push(failing);
    let failed = runtime
        .block_on(store.commit_application(deployment, 2, input, vec![], bytes))
        .unwrap();
    let update = runtime
        .block_on(tui.load_committed_update(failed.clone()).unwrap())
        .unwrap();
    tui.adopt_update(update, vec![]).unwrap();
    // A failed, independent candidate must not turn into terminal shutdown.
    tui.pump_settled().unwrap();
    tui.pump_settled().unwrap();
    draw(&mut tui, 50, 20);
    let status = tui.deployment_status();
    assert_eq!(status.desired, Some(failed));
    assert!(status.error.unwrap().contains("new isolated plugin failed"));
    assert!(status.converging);
    assert!(!tui.application.is_retiring(&tui.root));
    assert_eq!(observation(&tui), before);
    assert!(Rc::ptr_eq(
        &identity,
        &tui.resources.owners[&mount.id()].identity
    ));
    assert_eq!(tui.focus.as_ref().unwrap().mount, focused);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    assert_eq!(observation(&tui)["snapshot"]["text"]["bytes"], 5);
    assert!(!tui.requires_surface_replacement());
    drop(identity);
    close(&mut tui);
    drop(tui);
    runtime
        .block_on(Arc::try_unwrap(store).ok().unwrap().close())
        .unwrap();
}

#[test]
fn native_clipboard_gesture_uses_the_original_textarea_commit_and_observation() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    dispatch(
        &mut tui,
        "configure",
        json!({"submit":"enter", "filePaste":{"context":"original", "allowFiles":true}}),
    );
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    tui.input_settled(Event::Paste("before".into())).unwrap();
    let mount = draft(&tui);
    super::super::textarea::clipboard_text(
        &mut *tui.resources.owners.get_mut(&mount.id()).unwrap().state,
        "中文\nnext".into(),
    );
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        tui.pump_settled().unwrap();
        draw(&mut tui, 50, 20);
        if editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text()
            == "before中文\nnext"
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native clipboard did not publish"
        );
        std::thread::yield_now();
    }
    assert_eq!(
        observation(&tui)["snapshot"]["text"]["bytes"],
        "before中文\nnext".len()
    );
    assert!(tui.application.frame(&tui.root).unwrap().state["submissions"].is_null());
    let observed = super::super::textarea::editor_owner_mut(
        tui.resources
            .owners
            .get_mut(&mount.id())
            .unwrap()
            .state
            .as_mut(),
    )
    .observation();
    let files = NativeInput {
        replaceable: false,
        key: "file_observe".into(),
        value: json!({"context":"original","selection":{"owner":"native-file","revision":2,"phase":"empty","file":null,"error":null},"remaining":0,"retryable":false}),
    };
    tui.resources.observe(&mount, files.clone()).unwrap();
    tui.resources.observe(&mount, observed.clone()).unwrap();
    tui.resources.commit(&tui.application, &tui.root).unwrap();
    assert!(tui.resources.owners[&mount.id()].pending.pending());
    tui.settle().unwrap();
    assert!(!tui.resources.owners[&mount.id()].pending.pending());
    close(&mut tui);
}

#[test]
fn native_observation_initializes_new_scroll_scene_before_first_paint() {
    let (runtime, mut input, packages) = editor_fixture();
    let packages = packages.into_iter().map(|original| {
        if original.manifest().id != "dynamic.app" { return original; }
        let Implementation::JavaScript { module, .. } = &original.manifest().application.as_ref().unwrap().entries[&EntryTarget::Tui].implementation else { unreachable!() };
        let mut files = original.files().clone();
        let source = files.get_mut(module).unwrap();
        source.extend_from_slice(br#"
const beforeRestoration=frame;
frame=(state)=>{
  const out=beforeRestoration(state);
  if(state.observation) out.frame.view.children.push({kind:'stack',key:'restored-scroll',direction:'column',gap:0,padding:0,background:0,scroll:'manual',layout:{basis:3},children:[{kind:'text',key:'restored-content',text:'restored\n'.repeat(12),color:16777215}]});
  return out;
};
"#);
        let package = Package::new(files).unwrap();
        for node in input.instances.values_mut().filter(|node| matches!(&node.package,maka_plugin_core::package::PackageSelection::Pinned {package} if package.id == "dynamic.app")) {
            node.package = package.application_ref().into();
        }
        package
    }).collect();
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    tui.settle().unwrap();
    assert!(tui.application.frame(&tui.root).unwrap().state["observation"].is_object());
    let owner = &tui.resources.owners[&tui.root.id()];
    assert!(
        owner
            .state
            .downcast_ref::<super::super::scroll::SceneState>()
            .unwrap()
            .scrolls
            .contains_key("restored-scroll"),
        "the scene created by a native observation must be committed before paint"
    );
    // Deliberately do not call draw(): its extra sync would hide the missing
    // native commit after an observation creates a new scrolling region.
    let area = Rect::new(0, 0, 60, 30);
    let mut buffer = Buffer::empty(area);
    let scene = tui.paint(area, &mut buffer).unwrap();
    tui.present(scene);
    assert!(buffer.content.iter().any(|cell| cell.symbol() == "r"));
    tui.input_settled(key(KeyCode::Down)).unwrap();
}

#[test]
fn paste_then_enter_is_immediate_and_ordered_while_the_original_js_job_is_slow() {
    let (runtime, mut input, packages) = editor_fixture();
    let packages = packages.into_iter().map(|original| {
        if !matches!(original.manifest().id.as_str(), "dynamic.app" | "dynamic.composer") { return original; }
        let id = original.manifest().id.clone();
        let declaration = original.manifest().application.as_ref().unwrap().declaration().clone();
        let Implementation::JavaScript { module, .. } = &declaration.entries[&EntryTarget::Tui].implementation else { unreachable!() };
        let mut source = std::str::from_utf8(original.file(module).unwrap()).unwrap()
            .replace("on:{changed:'changed',submitted:'submitted',navigated:'navigated'}", "on:{changed:'changed',submitted:'submitted',checkpoint:'checkpoint'}");
        if original.manifest().id == "dynamic.app" {
            source = source.replace(
                "if(event.key==='reject'){", "if(event.key==='reject'){const until=Date.now()+100;while(Date.now()<until){};",
            ).replace(
                "next.observation=event.event;if(event.name==='submitted')next.submissions=(state.submissions??0)+1;",
                "if(event.name==='checkpoint'){next.nativeReady=event.event.state.initialized;}else{if(event.name==='navigated'){next.navigation=event.event;next.navigations=(state.navigations??0)+1;}else next.observation=event.event;if(event.name==='submitted'){if(!state.nativeReady)throw new Error('submission preceded its original checkpoint');next.submissions=(state.submissions??0)+1;}}",
            );
        }
        let package = support::package(&id, declaration, Some(source));
        for node in input.instances.values_mut().filter(|node| matches!(&node.package, maka_plugin_core::package::PackageSelection::Pinned { package } if package.id == id)) {
            node.package = package.application_ref().into();
        }
        package
    }).collect();
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    dispatch(
        &mut tui,
        "configure",
        json!({"submit":"enter","autoFocus":true}),
    );
    draw(&mut tui, 80, 24);
    let mount = draft(&tui);
    let revision = tui.application.frame(&tui.root).unwrap().revision;
    let at = std::time::Instant::now();
    tui.application
        .dispatch(
            &tui.root.clone(),
            revision,
            "reject",
            json!({"reject":false}),
        )
        .unwrap();
    assert!(at.elapsed() < std::time::Duration::from_millis(30));
    assert!(tui.application.ui_pending());
    // Initialization is captured by the original native producer while JS still
    // owns an earlier transaction. Its initial and completed facts cross another
    // observation key; Enter must wait for their actual logical publication.
    let owner = observation(&tui)["snapshot"]["text"]["owner"].clone();
    let queue = tui.resources.owners[&mount.id()].pending.clone();
    drop(
        queue
            .capture(
                "checkpoint",
                json!({"owner":owner,"state":{"kind":"ready","saved":false,"initialized":false}}),
                false,
            )
            .unwrap(),
    );
    drop(queue.capture("observe", observation(&tui), true).unwrap());
    drop(
        queue
            .capture(
                "checkpoint",
                json!({"owner":owner,"state":{"kind":"ready","saved":true,"initialized":true}}),
                false,
            )
            .unwrap(),
    );
    let at = std::time::Instant::now();
    // These are the production admission methods, with no fixture await between
    // paste and Enter. Rust keeps the buffer current before any guest result.
    tui.input(Event::Paste("fast🙂".into())).unwrap();
    tui.input(key(KeyCode::Enter)).unwrap();
    assert!(
        at.elapsed() < std::time::Duration::from_millis(30),
        "native input waited for JS: {:?}",
        at.elapsed()
    );
    assert_eq!(
        editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text(),
        "Startfast🙂"
    );
    assert_eq!(tui.application.frame(&tui.root).unwrap().revision, revision);
    assert!(tui.application.frame(&tui.root).unwrap().state["submissions"].is_null());
    tui.settle().unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submissions"],
        1
    );
    assert_eq!(
        observation(&tui)["snapshot"]["text"]["bytes"],
        "Startfast🙂".len()
    );
    close(&mut tui);
}

#[test]
fn consecutive_native_command_receipts_keep_original_values_through_candidate_rejection() {
    let (runtime, mut input, packages) = editor_fixture();
    let packages = packages.into_iter().map(|original| {
        if original.manifest().id != "dynamic.app" { return original; }
        let declaration = original.manifest().application.as_ref().unwrap().declaration().clone();
        let Implementation::JavaScript { module, .. } = &declaration.entries[&EntryTarget::Tui].implementation else { unreachable!() };
        let source = std::str::from_utf8(original.file(module).unwrap()).unwrap().replace(
            "next.observation=event.event;", "next.observation=event.event;if(event.event.receipt)next.receipts=[...(state.receipts??[]),event.event.receipt];",
        );
        let package = support::package("dynamic.app", declaration, Some(source));
        for node in input.instances.values_mut().filter(|node| matches!(&node.package, maka_plugin_core::package::PackageSelection::Pinned { package } if package.id == "dynamic.app")) {
            node.package = package.application_ref().into();
        }
        package
    }).collect();
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    draw(&mut tui, 80, 24);
    let mount = draft(&tui);
    let owner = observation(&tui)["snapshot"]["text"]["owner"]
        .as_str()
        .unwrap()
        .to_owned();
    let based_on = observation(&tui)["revision"].as_u64().unwrap();
    dispatch(&mut tui, "reject", json!({"reject":true}));
    for (sequence, text) in [(1, "A"), (2, "B")] {
        dispatch(
            &mut tui,
            "configure",
            json!({"command":{
                "owner":owner,"sequence":sequence,"basedOn":based_on,"edit":{"kind":"append","text":text}
            }}),
        );
    }
    assert_eq!(
        editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text(),
        "StartA"
    );
    assert!(
        observation(&tui).is_null(),
        "failed candidate cannot publish either receipt"
    );
    assert!(tui.resources.owners[&mount.id()].pending.pending());
    assert!(!tui.application.is_retiring(&mount));
    // Exhaust the same original byte budget. The next candidate must fail
    // before native command 3 can append or replace the retained receipt.
    let budget = tui.resources.budget.clone();
    let occupied = budget
        .reserve(
            maka_ui_runtime::budget::Kind::Encoded,
            64 * 1024 * 1024 - budget.usage().0,
        )
        .unwrap();
    let revision = tui.application.frame(&tui.root).unwrap().revision;
    tui.application
        .dispatch(
            &tui.root.clone(),
            revision,
            "configure",
            json!({"command":{
                "owner":owner,"sequence":3,"basedOn":based_on+1,"edit":{"kind":"append","text":"C"}
            }}),
        )
        .unwrap();
    assert!(tui.settle().is_err());
    assert_eq!(tui.application.frame(&tui.root).unwrap().revision, revision);
    assert_eq!(
        editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text(),
        "StartA"
    );
    drop(occupied);
    dispatch(&mut tui, "reject", json!({"reject":false}));
    tui.settle().unwrap();
    let receipts = tui.application.frame(&tui.root).unwrap().state["receipts"].clone();
    assert_eq!(
        receipts,
        json!([
            {"owner":owner,"sequence":1,"revision":based_on+1,"outcome":{"status":"applied"}},
            {"owner":owner,"sequence":2,"revision":based_on+1,"outcome":{"status":"conflict","reason":"revision"}}
        ])
    );
    assert!(!tui.resources.owners[&mount.id()].pending.pending());
    close(&mut tui);
}

#[test]
fn blocked_partial_output_keeps_written_geometry_and_native_input_until_the_next_frame() {
    let (runtime, mut tui) = presenter();
    dispatch(
        &mut tui,
        "configure",
        json!({"submit":"enter","autoFocus":true}),
    );
    draw(&mut tui, 80, 24);
    let mount = draft(&tui);
    let writer = crate::terminal::tests::SlowWriter::default();
    let output = crate::terminal::Output::with_writer(writer.clone()).unwrap();
    let mut screen = crate::terminal::tests::screen(Rect::new(0, 0, 80, 24));
    let mut encode = |tui: &mut Presenter| {
        let mut scene = None;
        let bytes = crate::terminal::render(&mut screen, |frame| {
            scene = Some(tui.paint(frame.area(), frame.buffer_mut()).unwrap());
        })
        .unwrap();
        (bytes, scene.unwrap())
    };
    let (a, scene) = encode(&mut tui);
    runtime
        .block_on(output.submit(a.clone()).unwrap())
        .unwrap()
        .unwrap();
    tui.present_written(scene);
    tui.settle().unwrap();
    let old = tui
        .scene
        .iter()
        .find(|target| target.key == "read")
        .unwrap()
        .clone();
    dispatch(
        &mut tui,
        "configure",
        json!({"submit":"enter","autoFocus":true,"prefix":"B\n"}),
    );
    let (b, scene_b) = encode(&mut tui);
    writer.block();
    let mut receipt_b = output.submit(b.clone()).unwrap();
    writer.wait_for_prefix();
    assert_eq!(writer.bytes(), [&a[..], &b[..2]].concat());
    assert!(matches!(
        receipt_b.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    // C commits while B is partially written. It cannot reinterpret B's hit
    // geometry, clear C's dirty state, or restore old callback authority.
    dispatch(
        &mut tui,
        "configure",
        json!({"submit":"enter","autoFocus":true,"prefix":"C\nC\n"}),
    );
    let c_revision = tui.application.frame(&tui.root).unwrap().revision;
    let at = std::time::Instant::now();
    tui.input(Event::Paste("while blocked".into())).unwrap();
    assert!(at.elapsed() < std::time::Duration::from_millis(30));
    assert!(
        editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text()
            .ends_with("while blocked")
    );
    assert!(tui.focus.as_ref().unwrap().key == "observe");
    let current_focus = tui.focus.clone().unwrap();
    writer.release();
    runtime.block_on(receipt_b).unwrap().unwrap();
    assert_eq!(
        writer.bytes(),
        [&a[..], &b[..]].concat(),
        "partial ANSI was preserved and fully drained"
    );
    tui.present_written(scene_b);
    assert!(tui.resources.redraw);
    assert!(tui.focus.as_ref().unwrap().same(&current_focus));
    assert!(
        tui.scene
            .iter()
            .find(|target| target.key == "read")
            .unwrap()
            .area
            .y
            > old.area.y
    );
    let stale = tui
        .scene
        .iter()
        .find(|target| target.key == "read")
        .unwrap()
        .clone();
    tui.input(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: stale.area.visible().x,
        row: stale.area.visible().y,
        modifiers: KeyModifiers::NONE,
    }))
    .unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().revision,
        c_revision
    );
    assert!(tui.surface.is_live());
    tui.settle().unwrap();
    let (c, scene_c) = encode(&mut tui);
    runtime
        .block_on(output.submit(c.clone()).unwrap())
        .unwrap()
        .unwrap();
    tui.present_written(scene_c);
    tui.settle().unwrap();
    assert_eq!(writer.bytes(), [&a[..], &b[..], &c[..]].concat());
    // The freshly written editor is usable, and Enter is delivered once.
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submissions"],
        1
    );
    close(&mut tui);
}

#[test]
fn interrupt_clears_native_draft_before_yielding_to_exit_and_keeps_undo() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 80, 24);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    let mount = draft(&tui);
    tui.input_settled(Event::Paste("draft to clear".into()))
        .unwrap();
    draw(&mut tui, 80, 24);
    let interrupt = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(tui.dispatch_focused_shortcut(&interrupt).unwrap());
    tui.settle().unwrap();
    assert_eq!(
        editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text(),
        ""
    );
    assert_eq!(observation(&tui)["snapshot"]["text"]["bytes"], 0);
    draw(&mut tui, 80, 24);
    assert!(!tui.dispatch_focused_shortcut(&interrupt).unwrap());
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    assert_eq!(
        editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text(),
        "draft to clear"
    );
    close(&mut tui);
}

#[test]
fn primary_copy_requires_selection_and_preserves_the_draft() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 80, 24);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    tui.input_settled(Event::Paste("copy this".into())).unwrap();
    draw(&mut tui, 80, 24);
    let mut bytes = Vec::new();
    assert!(!tui.copy_scope(&mut bytes, true).unwrap());
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    assert!(tui.copy_scope(&mut bytes, true).unwrap());
    let mut expected = Vec::new();
    crate::terminal::copy(&mut expected, "copy this").unwrap();
    assert_eq!(bytes, expected);
    let mount = draft(&tui);
    assert_eq!(
        editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text(),
        "copy this"
    );
    close(&mut tui);
}

#[test]
fn ordinary_textarea_ctrl_v_replaces_selection_without_file_policy() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    let mount = draft(&tui);
    super::super::textarea::editor_owner_mut(
        tui.resources
            .owners
            .get_mut(&mount.id())
            .unwrap()
            .state
            .as_mut(),
    )
    .clipboard
    .as_mut()
    .unwrap()
    .set_clipboard_text("Plain 中文\nnext".into());
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while editor_owner(tui.resources.owners[&mount.id()].state.as_ref())
        .editor
        .text()
        != "Plain 中文\nnext"
    {
        tui.pump_settled().unwrap();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        observation(&tui)["snapshot"]["text"]["bytes"],
        "Plain 中文\nnext".len()
    );
    close(&mut tui);
}

#[test]
fn editor_right_click_menu_copies_cuts_and_selects_original_editor() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    let mount = draft(&tui);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    draw(&mut tui, 50, 20);
    let area = tui.focus.as_ref().unwrap().area.visible();
    click(
        &mut tui,
        MouseEventKind::Down(MouseButton::Right),
        area.x,
        area.y,
    );
    assert!(tui.editor_menu.is_some());
    let mut buffer = Buffer::empty(Rect::new(0, 0, 50, 20));
    let painted = tui.paint(buffer.area, &mut buffer).unwrap();
    let screen: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
    for label in ["Copy", "Cut", "Paste", "Select all"] {
        assert!(screen.contains(label), "missing {label}: {screen}");
    }
    // Opening itself has no clipboard side effect, and activation waits for flush.
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))
    .unwrap();
    assert!(!tui.flush_clipboard(&mut Vec::new()).unwrap());
    tui.present(painted);
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))
    .unwrap();
    let mut copied = Vec::new();
    assert!(tui.flush_clipboard(&mut copied).unwrap());
    assert_eq!(copied, b"\x1b]52;c;U3RhcnQ=\x07");
    assert_eq!(
        editor_owner(tui.resources.owners[&mount.id()].state.as_ref())
            .editor
            .text(),
        "Start"
    );
    draw(&mut tui, 50, 20);
    click(
        &mut tui,
        MouseEventKind::Down(MouseButton::Right),
        area.x,
        area.y,
    );
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)))
        .unwrap();
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))
    .unwrap();
    assert_eq!(
        editor_owner(tui.resources.owners[&mount.id()].state.as_ref())
            .editor
            .text(),
        ""
    );
    assert!(tui.flush_clipboard(&mut Vec::new()).unwrap());
    super::super::textarea::editor_owner_mut(
        tui.resources
            .owners
            .get_mut(&mount.id())
            .unwrap()
            .state
            .as_mut(),
    )
    .clipboard
    .as_mut()
    .unwrap()
    .set_clipboard_text("menu paste".into());
    draw(&mut tui, 50, 20);
    click(
        &mut tui,
        MouseEventKind::Down(MouseButton::Right),
        area.x,
        area.y,
    );
    draw(&mut tui, 50, 20);
    click(
        &mut tui,
        MouseEventKind::Down(MouseButton::Left),
        area.x + 2,
        area.y.min(14) + 3,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while editor_owner(tui.resources.owners[&mount.id()].state.as_ref())
        .editor
        .text()
        != "menu paste"
    {
        tui.pump_settled().unwrap();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    draw(&mut tui, 50, 20);
    click(
        &mut tui,
        MouseEventKind::Down(MouseButton::Right),
        area.x,
        area.y,
    );
    draw(&mut tui, 50, 20);
    tui.input_settled(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))
        .unwrap();
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))
    .unwrap();
    assert_eq!(
        editor_owner(tui.resources.owners[&mount.id()].state.as_ref())
            .editor
            .selected_text(),
        "menu paste"
    );
    draw(&mut tui, 50, 20);
    click(
        &mut tui,
        MouseEventKind::Down(MouseButton::Right),
        area.x,
        area.y,
    );
    draw(&mut tui, 50, 20);
    dispatch(&mut tui, "configure", json!({"readOnly":true}));
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )))
    .unwrap();
    assert!(tui.editor_menu.is_none());
    assert!(!tui.flush_clipboard(&mut Vec::new()).unwrap());
    assert_eq!(
        editor_owner(tui.resources.owners[&mount.id()].state.as_ref())
            .editor
            .text(),
        "menu paste"
    );
    close(&mut tui);
}

#[test]
fn pending_editor_paste_is_cancelled_on_window_focus_loss_and_errors_are_visible() {
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 20);
    let mount = draft(&tui);
    let owner = super::super::textarea::editor_owner_mut(
        tui.resources
            .owners
            .get_mut(&mount.id())
            .unwrap()
            .state
            .as_mut(),
    );
    owner
        .clipboard
        .as_mut()
        .unwrap()
        .set_clipboard_text("must not insert".into());
    owner.paste(false);
    tui.input_settled(Event::FocusLost).unwrap();
    for _ in 0..20 {
        tui.pump_settled().unwrap();
    }
    assert_eq!(
        editor_owner(tui.resources.owners[&mount.id()].state.as_ref())
            .editor
            .text(),
        "Start"
    );
    tui.input_settled(Event::FocusGained).unwrap();
    let owner = super::super::textarea::editor_owner_mut(
        tui.resources
            .owners
            .get_mut(&mount.id())
            .unwrap()
            .state
            .as_mut(),
    );
    owner.editor.error = Some("composer-too-large");
    let buffer = draw(&mut tui, 50, 20);
    let screen: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
    assert!(screen.contains("Text exceeds the input limit"));
    assert!(!screen.contains("composer-too-large"));
    close(&mut tui);
}

#[test]
fn resize_grip_changes_height_without_editing_text_or_selection() {
    let (_runtime, mut tui) = presenter();
    dispatch(
        &mut tui,
        "configure",
        json!({"terminalRows":{"min":3,"max":8,"resizable":true}}),
    );
    draw(&mut tui, 50, 40);
    tui.input_settled(Event::Paste("original draft".into()))
        .unwrap();
    tui.input_settled(Event::Key(KeyEvent::new(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    )))
    .unwrap();
    draw(&mut tui, 50, 40);
    let mount = draft(&tui);
    let area = |tui: &Presenter| {
        tui.scene
            .iter()
            .find(|target| target.mount == mount)
            .unwrap()
            .area
            .visible()
    };
    let initial = area(&tui);
    let before = observation(&tui);
    let column = initial.right() - 2;
    let row = initial.bottom() - 1;
    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), row),
        (MouseEventKind::Drag(MouseButton::Left), row + 10),
        (MouseEventKind::Up(MouseButton::Left), row + 10),
    ] {
        tui.input_settled(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
        draw(&mut tui, 50, 40);
    }
    assert_eq!(
        area(&tui).height,
        initial.height + 10,
        "manual height exceeds the automatic preferred maximum"
    );
    assert_eq!(observation(&tui), before);
    let row = area(&tui).bottom() - 1;
    for (kind, row) in [
        (MouseEventKind::Down(MouseButton::Left), row),
        (MouseEventKind::Drag(MouseButton::Left), 0),
        (MouseEventKind::Up(MouseButton::Left), 0),
    ] {
        tui.input_settled(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
        draw(&mut tui, 50, 40);
    }
    assert_eq!(area(&tui).height, 5, "minimum text rows plus border");
    assert_eq!(observation(&tui), before);
    tui.input_settled(Event::Paste("replacement".into()))
        .unwrap();
    assert_eq!(
        super::super::textarea::editor_owner(&*tui.resources.owners[&mount.id()].state)
            .editor
            .text(),
        "replacement"
    );
    close(&mut tui);
}

#[test]
fn physical_cursor_follows_the_rendered_multiline_caret_after_resize() {
    use ratatui::layout::Position;
    let (_runtime, mut tui) = presenter();
    draw(&mut tui, 50, 24);
    tui.input_settled(Event::Paste("中🙂\n末".into())).unwrap();
    for width in [50, 11, 32] {
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, 24));
        let pending = tui.paint(buffer.area, &mut buffer).unwrap();
        let (x, y) = (0..24)
            .flat_map(|y| (0..width).map(move |x| (x, y)))
            .find(|&(x, y)| buffer[(x, y)].symbol() == "末")
            .expect("last edited line is painted");
        assert_eq!(pending.cursor_position(), Some(Position::new(x + 2, y)));
        tui.present(pending);
        tui.settle().unwrap();
    }
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
}
