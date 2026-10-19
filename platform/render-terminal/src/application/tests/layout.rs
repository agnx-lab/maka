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
use maka_ui_runtime::{
    ComponentCall, ComponentOutput, Frame, NativeCapability, NativeComponent, NativeExport,
    NativeView,
};
use ratatui::style::{Color, Modifier};
use std::{any::Any, cell::Cell, collections::BTreeMap, rc::Rc};

pub(super) enum View {
    Native(Rc<dyn Widget>),
    Scene(Value),
    ChangingScene(Rc<std::cell::RefCell<Value>>),
    SourceScene(Rc<std::cell::RefCell<Value>>),
    InteractiveScene(Rc<std::cell::RefCell<Value>>),
    MetadataVisibility {
        scene: Rc<super::super::scene::Node>,
        visible: Rc<Cell<bool>>,
    },
    StandaloneSource(Rc<std::cell::RefCell<Value>>),
    StandaloneMarkdown(Rc<std::cell::RefCell<Value>>),
    Reading(Rc<std::cell::RefCell<Value>>),
    ReadingSlot(Rc<std::cell::RefCell<Value>>),
    OmittedReadingSlot(Rc<std::cell::RefCell<Value>>, Rc<Cell<bool>>),
    HiddenReadingSlot(Rc<std::cell::RefCell<Value>>, Rc<Cell<bool>>),
    WrappedReadingSlot(Rc<std::cell::RefCell<Value>>, Rc<Cell<bool>>),
    Rows(Rc<std::cell::RefCell<Value>>, Rc<Cell<bool>>),
    NestedRows(Rc<std::cell::RefCell<Value>>, Rc<Cell<u8>>),
    Flush,
    PanicLayout(Rc<Cell<usize>>),
}
struct Root {
    view: View,
    accept: Rc<Cell<bool>>,
}
impl NativeComponent<dyn Widget> for Root {
    fn call(
        &self,
        call: ComponentCall<'_>,
        _: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<dyn Widget>>> {
        let last_input = match &call {
            ComponentCall::Event { key, input, .. } => json!({"key":key,"value":input}),
            ComponentCall::Update { state, .. } => state["last_input"].clone(),
            _ => Value::Null,
        };
        let wheel = match &call {
            ComponentCall::Event { key, input, .. } if input["kind"] == "wheel" => json!(key),
            ComponentCall::Update { state, .. } | ComponentCall::Event { state, .. } => {
                state["wheel"].clone()
            }
            _ => Value::Null,
        };
        let observation = match &call {
            ComponentCall::Event { key, input, .. }
                if matches!(
                    *key,
                    "__reading_observation"
                        | "reading-observed"
                        | "row-activated"
                        | "terminal-observed"
                ) || matches!(
                    input.get("kind").and_then(Value::as_str),
                    Some("selected" | "activate" | "submit")
                ) =>
            {
                (*input).clone()
            }
            ComponentCall::Update { state, .. } | ComponentCall::Event { state, .. } => {
                state["observation"].clone()
            }
            _ => Value::Null,
        };
        let (revision, count) = match call {
            ComponentCall::Mount { .. } => (0, 0),
            ComponentCall::Update { state, .. } => (
                state["revision"].as_u64().unwrap() + 1,
                state["count"].as_u64().unwrap(),
            ),
            ComponentCall::Event {
                state, key, input, ..
            } => {
                if !self.accept.get() {
                    return Err(Error::Invalid("injected feedback rejection".into()));
                }
                let count = if matches!(self.view, View::Flush) {
                    assert_eq!(key, "flushed");
                    input.as_u64().unwrap()
                } else {
                    state["count"].as_u64().unwrap() + 1
                };
                (state["revision"].as_u64().unwrap() + 1, count)
            }
        };
        let mut view = match &self.view {
            View::Native(widget) => NativeView::<dyn Widget>::new(
                widget.clone(),
                BTreeMap::from([("back".into(), json!(true))]),
                vec![],
            )?,
            View::Scene(source) => super::super::scene::compile(source)?,
            View::ChangingScene(source)
            | View::SourceScene(source)
            | View::InteractiveScene(source) => super::super::scene::compile(&source.borrow())?,
            View::MetadataVisibility { scene, visible } => NativeView::<dyn Widget>::new(
                scene.clone(),
                BTreeMap::from([("back".into(), json!(true))]),
                vec![],
            )?
            .with_root_visibility(visible.get()),
            View::StandaloneSource(source) | View::StandaloneMarkdown(source) => {
                let view = if matches!(&self.view, View::StandaloneMarkdown(_)) {
                    super::super::source::compile_markdown(&source.borrow())?
                } else {
                    super::super::source::compile(&source.borrow())?
                };
                NativeView::<dyn Widget>::new(
                    view.view,
                    BTreeMap::from([("back".into(), json!(true))]),
                    vec![],
                )?
            }
            View::ReadingSlot(props)
            | View::HiddenReadingSlot(props, _)
            | View::OmittedReadingSlot(props, _)
            | View::WrappedReadingSlot(props, _) => {
                let mut reader = json!({"kind":"component","key":"reader","binding":"reading","member":0,"props":props.borrow().clone(),"on":{"observed":"reading-observed"}});
                if matches!(&self.view, View::WrappedReadingSlot(_, wrapped) if wrapped.get()) {
                    reader = json!({"kind":"dialog","key":"wrapper","label":"Commands","open":false,
                        "width":20,"height":8,"background_color":0,"border_color":0xffffff,"background":reader,
                        "content":{"kind":"text","key":"unused","text":"","color":0}});
                }
                super::super::scene::compile(
                    &json!({"kind":"stack","key":"container","direction":"column","gap":0,"padding":0,"background":0,
                    "children":[reader,{"kind":"button","key":"back","label":"Back"}]}),
                )?
            }
            View::Reading(props) => {
                super::super::reading::Component
                    .call(
                        ComponentCall::Mount {
                            props: &props.borrow(),
                        },
                        &Default::default(),
                    )?
                    .frame
                    .view
            }
            View::PanicLayout(_) => super::super::scene::compile(&json!({
            "kind":"dialog","key":"modal","label":"Commands","open":true,"width":24,"height":8,"background_color":0,"border_color":0xffffff,
            "background":{"kind":"text","key":"background","text":"Background","color":0xffffff},
            "content":{"kind":"stack","key":"foreground","direction":"column","gap":0,"padding":0,"background":0,"children":[
                {"kind":"component","key":"reader","binding":"reading","member":0,"on":{},"props":{"content":{"kind":"stack","key":"reading","direction":"column","gap":0,"padding":0,"background":0,"scroll":"manual","children":[]},"command":null}}
            ]}}))?,
            View::NestedRows(props, phase) => super::super::scene::compile(
                &json!({"kind":"dialog","key":"outer","label":"Outer","open":phase.get() > 0,"width":36,"height":10,"background_color":0,"border_color":0xffffff,
                    "background":{"kind":"button","key":"root-composer","label":"Root Composer"},
                    "content":{"kind":"dialog","key":"inner","label":"Inner","open":phase.get() > 1,"width":20,"height":4,"background_color":0,"border_color":0xffffff,
                        "background":{"kind":"stack","key":"content","direction":"column","gap":0,"padding":0,"background":0,"children":[
                            {"kind":"button","key":"back","label":"Composer"},
                            {"kind":"component","key":"rows","binding":"rows","member":0,"props":props.borrow().clone(),"on":{"activated":"row-activated"}}
                        ]},
                        "content":{"kind":"button","key":"close","label":"Close"}}}),
            )?,
            View::Rows(props, modal) => super::super::scene::compile(
                &json!({"kind":"dialog","key":"modal","label":"Commands","open":modal.get(),"width":20,"height":4,"background_color":0,"border_color":0xffffff,
                    "background":{"kind":"stack","key":"content","direction":"column","gap":0,"padding":0,"background":0,"children":[
                        {"kind":"button","key":"back","label":"Composer"},
                        {"kind":"component","key":"rows","binding":"rows","member":0,"props":props.borrow().clone(),"on":{"activated":"row-activated"}}
                    ]},
                    "content":{"kind":"button","key":"close","label":"Close"}}),
            )?,
            View::Flush => NativeView::<dyn Widget>::new(
                Rc::new(Flush),
                BTreeMap::from([("flushed".into(), json!({"type":"integer","minimum":0}))]),
                vec![],
            )?,
        };
        if let View::HiddenReadingSlot(_, visible) = &self.view {
            view = view.with_root_visibility(visible.get());
        }
        if let View::OmittedReadingSlot(_, shown) = &self.view {
            view.view = Rc::new(ConditionalSlots {
                scene: view.view.clone(),
                shown: shown.clone(),
            });
        }
        Ok(Frame {
            revision,
            state: json!({"revision":revision,"count":count,"observation":observation,"wheel":wheel,"last_input":last_input}),
            surface: Some(Surface::Terminal {
                binding: "surface".into(),
            }),
            view,
        }
        .output(vec![]))
    }
}
struct Flush;
impl Widget for Flush {
    fn create(&self, _: NativeContext) -> Box<dyn Any> {
        Box::new(None::<u16>)
    }
    fn measure(&self, _: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        Ok((width, 1))
    }
    fn paint(
        &self,
        slots: &mut Slots<'_>,
        area: Placement,
        _: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        slots.native_geometry(Box::new(area.width))
    }
    fn present(&self, state: &mut dyn Any, geometry: Box<dyn Any>) -> Option<NativeInput> {
        let seen = state.downcast_mut::<Option<u16>>().unwrap();
        let width = *geometry.downcast::<u16>().unwrap();
        if *seen == Some(width) {
            return None;
        }
        *seen = Some(width);
        Some(NativeInput {
            replaceable: true,
            key: "flushed".into(),
            value: json!(width),
        })
    }
}
pub(super) fn native(view: View, accept: Rc<Cell<bool>>) -> (tokio::runtime::Runtime, Presenter) {
    let (runtime, mut input, mut packages) = fixture("return frame(state);");
    let index = packages
        .iter()
        .position(|p| p.manifest().id == "dynamic.app")
        .unwrap();
    let original = packages.remove(index);
    let mut declaration = original
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    let entry = declaration.entries.get_mut(&EntryTarget::Tui).unwrap();
    entry.scene = None;
    entry.implementation = Implementation::Native {
        implementation: "test.layout".into(),
    };
    if matches!(
        view,
        View::Reading(_)
            | View::SourceScene(_)
            | View::ReadingSlot(_)
            | View::HiddenReadingSlot(_, _)
            | View::OmittedReadingSlot(_, _)
            | View::WrappedReadingSlot(_, _)
            | View::PanicLayout(_)
    ) {
        use maka_plugin_core::{
            composition::Target,
            contract::{Availability, Cardinality, Export, Import},
            package::{Declaration, EntryDeclaration},
        };
        for (binding, export, implementation, contract, identity) in [
            (
                "source",
                "SourceText",
                "maka.ui.source",
                maka_ui_runtime::source::source_contract(),
                150,
            ),
            (
                "markdown",
                "Markdown",
                "maka.ui.markdown",
                maka_ui_runtime::markdown::markdown_contract(),
                156,
            ),
            (
                "diff",
                "Diff",
                "maka.ui.diff",
                maka_ui_runtime::diff::diff_contract(),
                157,
            ),
        ] {
            entry.imports.insert(
                binding.into(),
                Import {
                    contract: contract.clone(),
                    cardinality: Cardinality::One,
                },
            );
            let package = support::package(
                &format!("test.{binding}"),
                Declaration {
                    entries: BTreeMap::from([(
                        EntryTarget::Tui,
                        EntryDeclaration {
                            scene: None,
                            implementation: Implementation::Native {
                                implementation: implementation.into(),
                            },
                            configuration: json!({"type":"object"}),
                            imports: BTreeMap::new(),
                            exports: BTreeMap::from([(
                                export.into(),
                                Export {
                                    contract,
                                    availability: Availability::Required,
                                },
                            )]),
                        },
                    )]),
                    composition: None,
                },
                None,
            );
            input.instances.get_mut("app").unwrap().bindings.insert(
                binding.into(),
                vec![Target::Instance {
                    path: vec![binding.into()],
                    export: export.into(),
                }],
            );
            input
                .instances
                .insert(binding.into(), support::node(&package, identity));
            packages.push(package);
        }
        if matches!(
            view,
            View::Reading(_)
                | View::ReadingSlot(_)
                | View::HiddenReadingSlot(_, _)
                | View::OmittedReadingSlot(_, _)
                | View::WrappedReadingSlot(_, _)
                | View::PanicLayout(_)
        ) {
            let contract = maka_ui_runtime::reading::reading_contract();
            entry.imports.insert(
                "reading".into(),
                Import {
                    contract: contract.clone(),
                    cardinality: Cardinality::One,
                },
            );
            let reading = support::package(
                "test.reading",
                Declaration {
                    entries: BTreeMap::from([(
                        EntryTarget::Tui,
                        EntryDeclaration {
                            scene: None,
                            implementation: Implementation::Native {
                                implementation: if matches!(view, View::PanicLayout(_)) {
                                    "test.layout-panic"
                                } else {
                                    "maka.ui.reading"
                                }
                                .into(),
                            },
                            configuration: json!({"type":"object"}),
                            imports: BTreeMap::from([
                                (
                                    "source".into(),
                                    Import {
                                        contract: maka_ui_runtime::source::source_contract(),
                                        cardinality: Cardinality::One,
                                    },
                                ),
                                (
                                    "markdown".into(),
                                    Import {
                                        contract: maka_ui_runtime::markdown::markdown_contract(),
                                        cardinality: Cardinality::One,
                                    },
                                ),
                                (
                                    "diff".into(),
                                    Import {
                                        contract: maka_ui_runtime::diff::diff_contract(),
                                        cardinality: Cardinality::One,
                                    },
                                ),
                            ]),
                            exports: BTreeMap::from([(
                                "ReadingArea".into(),
                                Export {
                                    contract,
                                    availability: Availability::Required,
                                },
                            )]),
                        },
                    )]),
                    composition: None,
                },
                None,
            );
            let mut node = support::node(&reading, 151);
            node.bindings.insert(
                "source".into(),
                vec![Target::Instance {
                    path: vec!["source".into()],
                    export: "SourceText".into(),
                }],
            );
            node.bindings.insert(
                "markdown".into(),
                vec![Target::Instance {
                    path: vec!["markdown".into()],
                    export: "Markdown".into(),
                }],
            );
            node.bindings.insert(
                "diff".into(),
                vec![Target::Instance {
                    path: vec!["diff".into()],
                    export: "Diff".into(),
                }],
            );
            input.instances.insert("reading".into(), node);
            input.instances.get_mut("app").unwrap().bindings.insert(
                "reading".into(),
                vec![Target::Instance {
                    path: vec!["reading".into()],
                    export: "ReadingArea".into(),
                }],
            );
            packages.push(reading);
        }
    }
    if matches!(
        view,
        View::Rows(..) | View::NestedRows(..) | View::InteractiveScene(..)
    ) {
        use maka_plugin_core::{
            composition::Target,
            contract::{Availability, Cardinality, Export, Import},
            package::{Declaration, EntryDeclaration},
        };
        let mut components = vec![(
            "rows",
            "Rows",
            "maka.ui.rows",
            maka_ui_runtime::rows::rows_contract(),
            152,
        )];
        if matches!(view, View::InteractiveScene(..)) {
            components.push((
                "terminal",
                "TerminalView",
                "maka.ui.terminal",
                maka_ui_runtime::terminal::terminal_view_contract(),
                155,
            ));
            components.push((
                "reading",
                "ReadingArea",
                "maka.ui.reading",
                maka_ui_runtime::reading::reading_contract(),
                154,
            ));
            components.push((
                "editor",
                "Textarea",
                "maka.ui.textarea",
                maka_ui_runtime::textarea::textarea_contract(),
                153,
            ));
        }
        for (binding, export, implementation, contract, identity) in components {
            entry.imports.insert(
                binding.into(),
                Import {
                    contract: contract.clone(),
                    cardinality: Cardinality::One,
                },
            );
            let rows = support::package(
                &format!("test.{binding}"),
                Declaration {
                    entries: BTreeMap::from([(
                        EntryTarget::Tui,
                        EntryDeclaration {
                            scene: None,
                            implementation: Implementation::Native {
                                implementation: implementation.into(),
                            },
                            configuration: json!({"type":"object"}),
                            imports: BTreeMap::new(),
                            exports: BTreeMap::from([(
                                export.into(),
                                Export {
                                    contract,
                                    availability: Availability::Required,
                                },
                            )]),
                        },
                    )]),
                    composition: None,
                },
                None,
            );
            input
                .instances
                .insert(binding.into(), support::node(&rows, identity));
            input.instances.get_mut("app").unwrap().bindings.insert(
                binding.into(),
                vec![Target::Instance {
                    path: vec![binding.into()],
                    export: export.into(),
                }],
            );
            packages.push(rows);
        }
    }
    let contract = entry.exports["App"].contract.clone();
    let package = support::package(
        "dynamic.app",
        declaration,
        Some(
            std::str::from_utf8(original.file("index.mjs").unwrap())
                .unwrap()
                .to_owned(),
        ),
    );
    input.instances.get_mut("app").unwrap().package = package.application_ref().into();
    packages.push(package);
    let mut registry = registry();
    if let View::PanicLayout(counter) = &view {
        let counter = counter.clone();
        registry
            .register("test.layout-panic", move |_, _| {
                Ok(BTreeMap::from([(
                    "ReadingArea".into(),
                    NativeExport {
                        contract: maka_ui_runtime::reading::reading_contract(),
                        value: NativeCapability::Component(Rc::new(PanicLayout(counter.clone()))),
                    },
                )]))
            })
            .unwrap();
    }
    let root = Rc::new(Root { view, accept });
    registry
        .register("test.layout", move |_, _| {
            Ok(BTreeMap::from([(
                "App".into(),
                NativeExport {
                    contract: contract.clone(),
                    value: NativeCapability::Component(root.clone()),
                },
            )]))
        })
        .unwrap();
    let presenter = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry,
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    (runtime, presenter)
}
fn scene(wrap: bool) -> Value {
    json!({"kind":"stack","key":"root","direction":"column","gap":1,"padding":0,"background":0,"children":[
        {"kind":"text","key":"heading","text":"Message source","color":0xffffff},
        {"kind":"stack","key":"body","direction":"column","gap":0,"padding":0,"background":0,"layout":{"grow":1},"children":[
            {"kind":"text","key":"source","text":"中文😀 source\n".repeat(2000),"color":0xffffff}]},
        {"kind":"stack","key":"controls","direction":"row","gap":1,"padding":0,"background":0,"layout":{"wrap":wrap},"children":[
            {"kind":"button","key":"back","label":"Back"},{"kind":"button","key":"next","label":"Next"},{"kind":"button","key":"last","label":"End"}]}
    ]})
}
#[test]
fn single_line_elision_preserves_graphemes_and_the_adjacent_control_row() {
    use super::super::paint::truncated_line;
    assert_eq!(truncated_line("界界abc", 0), "");
    assert_eq!(truncated_line("界界abc", 1), "…");
    assert_eq!(truncated_line("界界abc", 4), "界…");
    assert_eq!(truncated_line("界界abc", 5), "界界…");
    assert_eq!(truncated_line("e\u{301}xyz", 2), "e\u{301}…");
    assert_eq!(truncated_line("a\n\u{1b}b", 4), "a b");
    let source = json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
        {"kind":"text","key":"title","text":"界界abc\nlong title","truncate":true,"color":0xffffff},
        {"kind":"button","key":"back","label":"Send"}
    ]});
    let (_runtime, mut tui) = native(View::Scene(source), Rc::new(Cell::new(true)));
    for width in [1, 4, 5, 8, 30] {
        let buffer = draw(&mut tui, width, 8);
        assert_eq!(
            tui.scene
                .iter()
                .find(|target| target.key == "back")
                .unwrap()
                .area
                .y,
            1
        );
        assert_eq!(
            buffer[(0, 1)].symbol(),
            if width == 1 {
                "…"
            } else if width <= 5 {
                "S"
            } else {
                " "
            }
        );
    }
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
}

#[test]
fn actual_long_scene_keeps_footer_painted_and_native_click_bound_to_flush() {
    let (_runtime, mut presenter) = native(View::Scene(scene(false)), Rc::new(Cell::new(true)));
    let area = Rect::new(0, 0, 30, 10);
    let mut buffer = Buffer::empty(area);
    let pending = presenter.paint(area, &mut buffer).unwrap();
    assert!(presenter.scene.is_empty());
    assert_eq!(buffer[(2, 9)].symbol(), "B");
    presenter.present(pending);
    assert_eq!(
        presenter
            .scene
            .iter()
            .map(|target| target.key.as_str())
            .collect::<Vec<_>>(),
        ["back", "next", "last"]
    );
    let last = presenter.scene.last().unwrap().area.visible();
    assert_eq!(last.y, 9);
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: last.x,
            row: last.y,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        1
    );
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
#[test]
fn narrow_wrapped_footer_is_measured_before_the_body_viewport() {
    let (_runtime, mut presenter) = native(View::Scene(scene(true)), Rc::new(Cell::new(true)));
    draw(&mut presenter, 17, 10);
    assert_eq!(
        presenter
            .scene
            .iter()
            .map(|target| (target.key.as_str(), target.area.y))
            .collect::<Vec<_>>(),
        [("back", 7), ("next", 7), ("last", 9)]
    );
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
#[test]
fn post_flush_native_observation_coalesces_and_retries_without_guest_paint() {
    let accept = Rc::new(Cell::new(false));
    let (_runtime, mut presenter) = native(View::Flush, accept.clone());
    let original = presenter.resources.owners[&presenter.root.id()]
        .identity
        .clone();
    let area = Rect::new(0, 0, 10, 3);
    let mut buffer = Buffer::empty(area);
    let _discarded = presenter.paint(area, &mut buffer).unwrap();
    presenter.sync_native().unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        0
    );
    draw(&mut presenter, 10, 3);
    presenter.sync_native().unwrap(); // Rejection retains the original native pending observation.
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        0
    );
    accept.set(true);
    draw(&mut presenter, 20, 3); // New physical evidence wakes the retained publisher.
    presenter.settle().unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        20
    );
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["revision"],
        1
    );
    draw(&mut presenter, 20, 3);
    presenter.sync_native().unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["revision"],
        1
    );
    assert!(Rc::ptr_eq(
        &original,
        &presenter.resources.owners[&presenter.root.id()].identity
    ));
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn allocated_row_width_remeasures_wrapped_body_before_footer_placement() {
    for (wrap, width, footer_y) in [(false, 100, 6), (true, 15, 8)] {
        let source = json!({"kind":"stack","key":"root","direction":"column","gap":1,"padding":0,"background":0,"children":[
          {"kind":"stack","key":"row","direction":"row","gap":1,"padding":0,"background":0,"layout":{"wrap":wrap},"children":[
            {"kind":"stack","key":"narrow","direction":"column","gap":0,"padding":0,"background":0,"layout":{"basis":10},"children":[
              {"kind":"text","key":"body","text":"a".repeat(50),"color":0xffffff}]},
            {"kind":"button","key":"after","label":"After"}]},
          {"kind":"button","key":"footer","label":"Footer"}]});
        let (_runtime, mut presenter) = native(View::Scene(source), Rc::new(Cell::new(true)));
        let buffer = draw(&mut presenter, width, 12);
        for y in 0..5 {
            assert_eq!(buffer[(0, y)].symbol(), "a");
            assert_eq!(buffer[(9, y)].symbol(), "a");
        }
        assert_eq!(
            presenter
                .scene
                .iter()
                .find(|target| target.key == "footer")
                .unwrap()
                .area
                .y,
            footer_y
        );
        presenter
            .surface_closed(&presenter.surface(), Ok(()))
            .unwrap();
        presenter.shutdown_settled().unwrap();
    }
}

#[test]
fn deep_legal_rows_share_one_measurement_pass_instead_of_expanding_recursively() {
    let mut source = json!({"kind":"text","key":"leaf","text":"word ".repeat(40),"color":0xffffff});
    for depth in 1..=31 {
        source = json!({"kind":"stack","key":format!("row-{depth}"),"direction":"row","gap":0,"padding":0,"background":0,"layout":{"basis":depth},"children":[source]});
    }
    let (_runtime, mut presenter) = native(View::Scene(source), Rc::new(Cell::new(true)));
    let frame = presenter.application.frame(&presenter.root).unwrap();
    let node = (frame.view.view.as_ref() as &dyn Any)
        .downcast_ref::<super::super::scene::Node>()
        .unwrap();
    let measurements = super::super::paint::Measurements::guarded(4096);
    presenter
        .measure(
            &presenter.root,
            node,
            80,
            measurements.context(Default::default()),
        )
        .unwrap();
    assert!(measurements.requests() <= 32 * 32 * 3);
    // Real painting also uses one ephemeral pass; no cache enters accepted resources.
    let before = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .state
        .clone();
    draw(&mut presenter, 80, 24);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state,
        before
    );
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

fn scroll_scene(policy: &str, text: &str) -> Value {
    json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
        {"kind":"stack","key":"body","direction":"column","gap":0,"padding":0,"background":0,"layout":{"grow":1},"scroll":policy,"children":[
            {"kind":"text","key":"source","text":text,"color":0xffffff}]},
        {"kind":"button","key":"back","label":"Back"}]})
}
fn flushed(presenter: &mut Presenter, width: u16, height: u16) -> Buffer {
    presenter.settle().unwrap();
    presenter.sync_native().unwrap();
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    let scene = presenter.paint(area, &mut buffer).unwrap();
    presenter.present(scene);
    buffer
}
fn wheel(presenter: &mut Presenter, down: bool, column: u16, row: u16) {
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind: if down {
                MouseEventKind::ScrollDown
            } else {
                MouseEventKind::ScrollUp
            },
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
}
fn scroll_state(presenter: &Presenter, key: &str) -> (u32, u32) {
    let state = presenter.resources.owners[&presenter.root.id()]
        .state
        .downcast_ref::<super::super::scroll::SceneState>()
        .unwrap();
    let scroll = &state.scrolls[key];
    (scroll.offset, scroll.presented.as_ref().unwrap().offset)
}
#[test]
fn native_scroll_has_real_clipped_cells_and_does_not_ack_unflushed_or_superseded_intent() {
    let text = (0..30).map(|n| format!("row {n:02}\n")).collect::<String>();
    let (_runtime, mut presenter) = native(
        View::Scene(scroll_scene("manual", &text)),
        Rc::new(Cell::new(true)),
    );
    let first = flushed(&mut presenter, 12, 6);
    assert_eq!(first[(4, 0)].symbol(), "0");
    wheel(&mut presenter, true, 1, 1);
    assert_eq!(scroll_state(&presenter, "body"), (1, 0));
    let area = Rect::new(0, 0, 12, 6);
    let mut discarded = Buffer::empty(area);
    let stale = presenter.paint(area, &mut discarded).unwrap();
    assert_eq!(discarded[(5, 0)].symbol(), "1");
    assert_eq!(discarded[(4, 5)].symbol(), "B");
    wheel(&mut presenter, true, 1, 1);
    presenter.present(stale);
    assert_eq!(scroll_state(&presenter, "body"), (2, 0));
    let next = flushed(&mut presenter, 12, 6);
    assert_eq!(next[(5, 0)].symbol(), "2");
    assert_eq!(scroll_state(&presenter, "body"), (2, 2));
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
#[test]
fn wheel_and_page_keys_bubble_from_the_original_button_target() {
    let mut source = scroll_scene("manual", "");
    source["children"][0]["children"] = json!(
        (0..20)
            .map(|n| json!({"kind":"button","key":format!("b{n}"),"label":format!("Item {n}")}))
            .collect::<Vec<_>>()
    );
    let (_runtime, mut presenter) = native(View::Scene(source), Rc::new(Cell::new(true)));
    flushed(&mut presenter, 20, 7);
    wheel(&mut presenter, true, 2, 0);
    assert_eq!(scroll_state(&presenter, "body").0, 1);
    flushed(&mut presenter, 20, 7);
    presenter
        .input_settled(Event::Key(KeyEvent::new(
            KeyCode::PageDown,
            KeyModifiers::NONE,
        )))
        .unwrap();
    assert_eq!(scroll_state(&presenter, "body").0, 7);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        0
    );
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
#[test]
fn follow_end_uses_actual_flush_and_manual_interaction_survives_recipe_updates() {
    let source = Rc::new(std::cell::RefCell::new(scroll_scene(
        "follow_end",
        "0\n1\n2\n3\n4\n5\n6\n7",
    )));
    let (_runtime, mut presenter) = native(
        View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    flushed(&mut presenter, 10, 5);
    assert_eq!(scroll_state(&presenter, "body"), (4, 4));
    let owner = presenter.resources.owners[&presenter.root.id()]
        .identity
        .clone();
    source.borrow_mut()["children"][0]["children"][0]["text"] =
        json!("0\n1\n2\n3\n4\n5\n6\n7\n8\n9");
    refresh(&mut presenter);
    flushed(&mut presenter, 10, 5);
    assert_eq!(scroll_state(&presenter, "body"), (6, 6));
    wheel(&mut presenter, false, 1, 1);
    source.borrow_mut()["children"][0]["children"][0]["text"] =
        json!("0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11");
    refresh(&mut presenter);
    flushed(&mut presenter, 10, 5);
    assert_eq!(scroll_state(&presenter, "body"), (5, 5));
    // One queued gesture is applied against the same presented frame, then
    // streaming extends the content before its next paint.
    wheel(&mut presenter, false, 1, 1);
    wheel(&mut presenter, false, 1, 1);
    source.borrow_mut()["children"][0]["children"][0]["text"] =
        json!("0\n1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13");
    refresh(&mut presenter);
    flushed(&mut presenter, 10, 5);
    assert_eq!(scroll_state(&presenter, "body"), (3, 3));
    assert!(Rc::ptr_eq(
        &owner,
        &presenter.resources.owners[&presenter.root.id()].identity
    ));
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
#[test]
fn anchor_keeps_gap_offset_and_original_child_after_hide_and_prepend() {
    let mut initial = scroll_scene("anchor", "A");
    initial["children"][0]["gap"] = json!(10);
    initial["children"][0]["children"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"text","key":"second","text":"B\nC\nD\nE\nF\nG", "color":0xffffff}));
    let source = Rc::new(std::cell::RefCell::new(initial));
    let (_runtime, mut presenter) = native(
        View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    flushed(&mut presenter, 10, 5);
    wheel(&mut presenter, true, 1, 1);
    flushed(&mut presenter, 10, 5);
    flushed(&mut presenter, 10, 5);
    assert_eq!(scroll_state(&presenter, "body"), (1, 1));
    source.borrow_mut()["children"][0]["visible"] = json!(false);
    refresh(&mut presenter);
    flushed(&mut presenter, 10, 5);
    assert_eq!(scroll_state(&presenter, "body"), (1, 1));
    source.borrow_mut()["children"][0]["children"]
        .as_array_mut()
        .unwrap()
        .insert(
            0,
            json!({"kind":"text","key":"prefix","text":"prefix\n".repeat(100),"color":0xffffff}),
        );
    source.borrow_mut()["children"][0]["visible"] = json!(true);
    refresh(&mut presenter);
    flushed(&mut presenter, 10, 5);
    assert_eq!(scroll_state(&presenter, "body"), (112, 112));
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn history_prepend_preserves_wheel_movement_since_the_last_presented_anchor() {
    for policy in ["anchor", "follow_end"] {
        let text = (0..30).map(|n| format!("row {n:02}\n")).collect::<String>();
        let source = Rc::new(std::cell::RefCell::new(scroll_scene(policy, &text)));
        let (_runtime, mut presenter) = native(
            View::ChangingScene(source.clone()),
            Rc::new(Cell::new(true)),
        );
        flushed(&mut presenter, 12, 6);
        for _ in 0..5 {
            wheel(&mut presenter, policy == "anchor", 1, 1);
        }
        flushed(&mut presenter, 12, 6);
        let before = scroll_state(&presenter, "body").0;
        wheel(&mut presenter, false, 1, 1);
        wheel(&mut presenter, false, 1, 1);
        source.borrow_mut()["children"][0]["children"]
            .as_array_mut()
            .unwrap()
            .insert(
                0,
                json!({"kind":"text","key":"older","text":"older\nolder\nolder","color":0xffffff}),
            );
        refresh(&mut presenter);
        let buffer = flushed(&mut presenter, 12, 6);
        assert_eq!(scroll_state(&presenter, "body"), (before + 1, before + 1));
        assert_eq!(buffer[(5, 0)].symbol(), ((before - 2) % 10).to_string());
        presenter
            .surface_closed(&presenter.surface(), Ok(()))
            .unwrap();
        presenter.shutdown_settled().unwrap();
    }
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

#[test]
fn changed_scene_content_rejects_pending_geometry_with_the_same_inputs_and_slots() {
    let source = Rc::new(std::cell::RefCell::new(json!({
        "kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,
        "children":[
            {"kind":"text","key":"text","text":"one","color":0xffffff},
            {"kind":"button","key":"back","label":"Back"}
        ]
    })));
    let (_runtime, mut presenter) = native(
        View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    let button_row = |presenter: &Presenter| {
        presenter
            .scene
            .iter()
            .find(|target| target.key == "back")
            .unwrap()
            .area
            .y
    };
    flushed(&mut presenter, 20, 6);
    assert_eq!(button_row(&presenter), 1);
    let area = Rect::new(0, 0, 20, 6);
    let pending = presenter.paint(area, &mut Buffer::empty(area)).unwrap();

    source.borrow_mut()["children"][0]["text"] = json!("one\ntwo\nthree");
    refresh(&mut presenter);
    let current = flushed(&mut presenter, 20, 6);
    assert_eq!(current[(0, 2)].symbol(), "t");
    assert_eq!(button_row(&presenter), 3);
    presenter.present(pending);
    assert_eq!(button_row(&presenter), 3);

    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn disabled_buttons_retain_layout_but_revoke_click_dispatch_and_tab_targets() {
    let source = Rc::new(std::cell::RefCell::new(json!({
        "kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,
        "children":[
            {"kind":"button","key":"save","label":"Save","color":0x416653},
            {"kind":"button","key":"back","label":"Back"}
        ]
    })));
    let (_runtime, mut presenter) = native(
        View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    let first = flushed(&mut presenter, 20, 3);
    assert_eq!(first[(0, 0)].fg, Color::Rgb(0x41, 0x66, 0x53));
    assert_eq!(first[(0, 1)].fg, Color::Reset);
    let owner = presenter.resources.owners[&presenter.root.id()]
        .identity
        .clone();
    assert_eq!(presenter.focus.as_ref().unwrap().key, "save");
    source.borrow_mut()["children"][0]["disabled"] = json!(true);
    refresh(&mut presenter);
    let count = presenter.application.frame(&presenter.root).unwrap().state["count"].clone();
    let revision = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .revision;
    assert!(
        presenter
            .application
            .dispatch(&presenter.root, revision, "save", json!({"kind":"click"}))
            .is_err()
    );
    assert!(
        presenter.input_settled(key(KeyCode::Enter)).is_ok(),
        "stale physical input is ignored without terminating the host"
    );
    let disabled = flushed(&mut presenter, 20, 3);
    for y in 0..3 {
        for x in 0..20 {
            assert_eq!(
                first[(x, y)].symbol(),
                disabled[(x, y)].symbol(),
                "disabled retains button geometry"
            );
        }
    }
    assert!(
        disabled[(0, 0)].modifier.contains(Modifier::DIM),
        "original Surface dims disabled controls even with an explicit role color"
    );
    assert_eq!(presenter.tab_stops().len(), 1);
    assert_eq!(presenter.focus.as_ref().unwrap().key, "save");
    presenter.input_settled(key(KeyCode::Char(' '))).unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        count
    );
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
    presenter.input_settled(key(KeyCode::Tab)).unwrap();
    assert_eq!(presenter.focus.as_ref().unwrap().key, "back");
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        count
    );
    source.borrow_mut()["children"][0]["disabled"] = json!(false);
    refresh(&mut presenter);
    flushed(&mut presenter, 20, 3);
    assert!(Rc::ptr_eq(
        &owner,
        &presenter.resources.owners[&presenter.root.id()].identity
    ));
    presenter.input_settled(key(KeyCode::Tab)).unwrap();
    assert_eq!(presenter.focus.as_ref().unwrap().key, "save");
    let before = presenter.application.frame(&presenter.root).unwrap().state["count"]
        .as_u64()
        .unwrap();
    presenter.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        before + 1
    );
    // A save that temporarily disables its own control must not send the next
    // Space to an unrelated button when the original control becomes ready.
    source.borrow_mut()["children"][0]["disabled"] = json!(true);
    refresh(&mut presenter);
    flushed(&mut presenter, 20, 3);
    assert_eq!(presenter.focus.as_ref().unwrap().key, "save");
    source.borrow_mut()["children"][0]["disabled"] = json!(false);
    refresh(&mut presenter);
    flushed(&mut presenter, 20, 3);
    assert_eq!(presenter.focus.as_ref().unwrap().key, "save");
    presenter.input_settled(key(KeyCode::Char(' '))).unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["last_input"]["key"],
        "save"
    );
    source.borrow_mut()["children"][1]["disabled"] = json!(true);
    refresh(&mut presenter);
    let default_disabled = flushed(&mut presenter, 20, 3);
    assert_eq!(default_disabled[(0, 1)].fg, Color::Reset);
    assert!(default_disabled[(0, 1)].modifier.contains(Modifier::DIM));
    assert_eq!(presenter.tab_stops().len(), 1);
    assert_eq!(presenter.tab_stops()[0].key, "save");
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn native_scroll_reaches_content_beyond_u16_terminal_height() {
    let mut source = scroll_scene("manual", "");
    source["children"][0]["children"] = json!((0..4).map(|n| json!({"kind":"text", "key":format!("part{n}"), "text":"p\n".repeat(20000), "color":0xffffff})).collect::<Vec<_>>());
    let (_runtime, mut presenter) = native(View::Scene(source), Rc::new(Cell::new(true)));
    flushed(&mut presenter, 10, 6);
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
    presenter
        .input_settled(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)))
        .unwrap();
    let end = flushed(&mut presenter, 10, 6);
    assert_eq!(scroll_state(&presenter, "body"), (79999, 79999));
    assert_eq!(end[(0, 0)].symbol(), "p");
    assert_eq!(end[(3, 5)].symbol(), "B");
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
#[test]
fn nested_scroll_consumes_innermost_wheel_then_bubbles_at_its_boundary() {
    let mut source = scroll_scene("manual", "");
    source["children"][0]["children"] = json!([
        {"kind":"stack","key":"inner","direction":"column","gap":0,"padding":0,"background":0,"scroll":"manual","layout":{"basis":3},"children":[
            {"kind":"text","key":"inner-text","text":"0\n1\n2\n3\n4\n5\n6\n7\n8\n9","color":0xffffff}]},
        {"kind":"text","key":"outer-text","text":"tail\n".repeat(30),"color":0xffffff}]);
    let (_runtime, mut presenter) = native(View::Scene(source), Rc::new(Cell::new(true)));
    flushed(&mut presenter, 10, 6);
    for _ in 0..7 {
        wheel(&mut presenter, true, 1, 1);
        flushed(&mut presenter, 10, 6);
    }
    assert_eq!(scroll_state(&presenter, "inner"), (7, 7));
    assert_eq!(scroll_state(&presenter, "body"), (0, 0));
    wheel(&mut presenter, true, 1, 1);
    flushed(&mut presenter, 10, 6);
    assert_eq!(scroll_state(&presenter, "body"), (1, 1));
    assert_eq!(scroll_state(&presenter, "inner"), (7, 7));
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn delayed_hidden_metadata_cannot_replace_same_native_recipe_foreground() {
    let visible = Rc::new(Cell::new(true));
    let scene = Rc::new(super::super::scene::Node::Button {
        align: None,
        key: "back".into(),
        label: "Continue".into(),
        hint: String::new(),
        focus_child: None,
        copy_text: None,
        disabled: false,
        color: None,
        background: None,
        hover_background: None,
        bold: false,
    });
    let (_runtime, mut presenter) = native(
        View::MetadataVisibility {
            scene,
            visible: visible.clone(),
        },
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 20, 2);
    let root = presenter.root.clone();
    visible.set(false);
    let revision = presenter.application.frame(&root).unwrap().revision;
    presenter
        .application
        .dispatch(&root, revision, "back", json!(true))
        .unwrap();
    presenter.sync_native().unwrap();
    let area = Rect::new(0, 0, 20, 2);
    let hidden = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    visible.set(true);
    let revision = presenter.application.frame(&root).unwrap().revision;
    presenter
        .application
        .dispatch(&root, revision, "back", json!(true))
        .unwrap();
    draw(&mut presenter, 20, 2);
    let count = presenter.application.frame(&root).unwrap().state["count"]
        .as_u64()
        .unwrap();
    presenter.present(hidden);
    presenter.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        presenter.application.frame(&root).unwrap().state["count"],
        count + 1
    );
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

/// A genuine native parent omits an ordinary live declared child from placement.
struct ConditionalSlots {
    scene: Rc<dyn Widget>,
    shown: Rc<Cell<bool>>,
}
impl Widget for ConditionalSlots {
    fn create(&self, cx: NativeContext) -> Box<dyn Any> {
        self.scene.create(cx)
    }
    fn commit(&self, state: &mut dyn Any) -> maka_ui_runtime::Result<Option<NativeInput>> {
        self.scene.commit(state)
    }
    fn prepare_focus(&self, state: &mut dyn Any, visible: bool, revision: u64) {
        self.scene.prepare_focus(state, visible, revision);
    }
    fn measure(&self, slots: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        self.scene.measure(slots, width)
    }
    fn paint(
        &self,
        slots: &mut Slots<'_>,
        area: Placement,
        buffer: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        if self.shown.get() {
            self.scene.paint(slots, area, buffer)
        } else {
            let button = super::super::scene::Node::Button {
                align: None,
                key: "back".into(),
                label: "Back".into(),
                hint: String::new(),
                focus_child: None,
                copy_text: None,
                disabled: false,
                color: None,
                background: None,
                hover_background: None,
                bold: false,
            };
            button.paint(slots, area, buffer)
        }
    }
}

#[test]
fn dialog_wrapping_original_reading_slot_keeps_grow_basis_and_footer_geometry() {
    let props = Rc::new(std::cell::RefCell::new(
        json!({"content":{"kind":"stack","key":"reading","direction":"column","gap":0,"padding":0,"background":0,
        "scroll":"manual","layout":{"grow":1,"basis":0},"children":[{"kind":"text","key":"line","text":"one\ntwo\nthree","color":0xffffff}]},"command":null}),
    ));
    let wrapped = Rc::new(Cell::new(false));
    let (_runtime, mut presenter) = native(
        View::WrappedReadingSlot(props, wrapped.clone()),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 30, 8);
    let original = draw(&mut presenter, 30, 8);
    let footer = presenter
        .scene
        .iter()
        .find(|target| target.key == "back")
        .unwrap()
        .area;
    wrapped.set(true);
    let revision = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .revision;
    presenter
        .application
        .dispatch(&presenter.root, revision, "back", json!({}))
        .unwrap();
    let wrapped_buffer = draw(&mut presenter, 30, 8);
    assert_eq!(wrapped_buffer, original);
    assert_eq!(
        presenter
            .scene
            .iter()
            .find(|target| target.key == "back")
            .unwrap()
            .area,
        footer
    );
    assert_eq!(footer.visible().y, 7);
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

struct PanicLayout(Rc<Cell<usize>>);
struct OriginalDrop(Rc<Cell<usize>>);
impl Drop for OriginalDrop {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
impl Widget for PanicLayout {
    fn create(&self, _: NativeContext) -> Box<dyn Any> {
        Box::new(OriginalDrop(self.0.clone()))
    }
    fn layout(&self, _: &Slots<'_>) -> maka_ui_runtime::Result<TerminalLayout> {
        panic!("original native layout failure")
    }
    fn measure(&self, _: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        Ok((width, 1))
    }
    fn paint(
        &self,
        _: &mut Slots<'_>,
        _: Placement,
        _: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        Ok(())
    }
}
impl NativeComponent<dyn Widget> for PanicLayout {
    fn call(
        &self,
        _: ComponentCall<'_>,
        _: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<dyn Widget>>> {
        Ok(Frame {
            revision: 0,
            state: json!({}),
            surface: None,
            view: NativeView::<dyn Widget>::new(
                Rc::new(Self(self.0.clone())),
                BTreeMap::new(),
                vec![],
            )?,
        }
        .output(vec![]))
    }
}
#[test]
fn dialog_deferred_native_layout_panic_returns_error_and_releases_original_owner() {
    let drops = Rc::new(Cell::new(0));
    let (_runtime, mut presenter) =
        native(View::PanicLayout(drops.clone()), Rc::new(Cell::new(true)));
    let area = Rect::new(0, 0, 40, 12);
    let result = presenter.paint(area, &mut Buffer::empty(area));
    assert!(
        matches!(result,Err(Error::Invalid(message)) if message=="terminal native painting panicked")
    );
    assert_eq!(drops.get(), 0);
    assert!(!presenter.physical_admitted());
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
    assert_eq!(drops.get(), 1);
}

#[test]
fn revealed_container_focus_waits_for_flush_and_preserves_user_navigation() {
    let mut initial = scroll_scene("manual", &"reading\n".repeat(30));
    initial["children"][0]["auto_focus"] = json!(true);
    initial["children"][0]["visible"] = json!(false);
    initial["children"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"kind":"button","key":"header","label":"Header"}));
    let source = Rc::new(std::cell::RefCell::new(initial));
    let (_runtime, mut presenter) = native(
        View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    flushed(&mut presenter, 24, 8);
    assert_eq!(presenter.focus.as_ref().unwrap().key, "header");
    source.borrow_mut()["children"][1]["visible"] = json!(true);
    refresh(&mut presenter);
    presenter.sync_native().unwrap();
    let area = Rect::new(0, 0, 24, 8);
    let mut buffer = Buffer::empty(area);
    drop(presenter.paint(area, &mut buffer).unwrap());
    assert_eq!(presenter.focus.as_ref().unwrap().key, "header");
    let stale = presenter.paint(area, &mut buffer).unwrap();
    source.borrow_mut()["children"][1]["visible"] = json!(false);
    refresh(&mut presenter);
    source.borrow_mut()["children"][1]["visible"] = json!(true);
    refresh(&mut presenter);
    presenter.sync_native().unwrap();
    presenter.present(stale);
    assert_eq!(presenter.focus.as_ref().unwrap().key, "header");
    let first = presenter.paint(area, &mut buffer).unwrap();
    let twin = presenter.paint(area, &mut buffer).unwrap();
    presenter.present(first);
    assert_eq!(presenter.focus.as_ref().unwrap().key, "body");
    presenter.input_settled(key(KeyCode::PageDown)).unwrap();
    assert!(scroll_state(&presenter, "body").0 > 0);
    presenter.input_settled(key(KeyCode::Tab)).unwrap();
    let chosen = presenter.focus.as_ref().unwrap().key.clone();
    assert_ne!(chosen, "body");
    presenter.present(twin);
    assert_eq!(presenter.focus.as_ref().unwrap().key, chosen);
    flushed(&mut presenter, 24, 2);
    flushed(&mut presenter, 24, 8);
    assert_eq!(presenter.focus.as_ref().unwrap().key, chosen);

    // A newly revealed form chooses its first enabled, actually placed control.
    source.borrow_mut()["children"][1]["visible"] = json!(false);
    refresh(&mut presenter);
    flushed(&mut presenter, 24, 8);
    source.borrow_mut()["children"][1]["visible"] = json!(true);
    source.borrow_mut()["children"][1]["children"] = json!([
        {"kind":"button","key":"disabled","label":"Disabled","disabled":true},
        {"kind":"button","key":"edit","label":"Edit"}
    ]);
    refresh(&mut presenter);
    flushed(&mut presenter, 24, 8);
    assert_eq!(presenter.focus.as_ref().unwrap().key, "edit");
    let before = presenter.application.frame(&presenter.root).unwrap().state["count"]
        .as_u64()
        .unwrap();
    presenter.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        before + 1
    );
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn anchored_model_dialog_follows_its_trigger_across_viewport_sizes() {
    let scene = json!({
        "kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,
        "children":[
            {"kind":"stack","key":"work","direction":"column","gap":0,"padding":0,"background":0,"layout":{"grow":1},"children":[]},
            {"kind":"dialog","key":"models","label":"Models","open":true,"anchor_to_background":true,
             "width":20,"height":8,"padding":0,"background_color":0,"border_color":0xffffff,
             "background":{"kind":"button","key":"trigger","label":"Choose model"},
             "content":{"kind":"button","key":"model","label":"Local model"}}
        ]
    });
    let (_runtime, mut presenter) = native(View::Scene(scene), Rc::new(Cell::new(true)));
    for (cols, rows) in [(80, 24), (65, 30), (14, 6)] {
        flushed(&mut presenter, cols, rows);
        let selected = presenter
            .scene
            .iter()
            .find(|target| target.key == "model")
            .unwrap()
            .area
            .visible();
        assert_eq!(
            selected.x, 1,
            "model list stays next to the trigger's left edge"
        );
        assert_eq!(
            selected.y,
            rows - 3,
            "model list opens directly above the footer"
        );
        assert!(selected.right() <= cols && selected.bottom() <= rows);
    }
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn unchanged_button_hover_does_not_redraw_or_clear_an_existing_redraw() {
    let source = json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
        {"kind":"button","key":"tool","label":"Read file","hint":"Show details"}
    ]});
    let (_runtime, mut presenter) = native(View::Scene(source), Rc::new(Cell::new(true)));
    flushed(&mut presenter, 32, 6);
    let mouse = |column, row| {
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    presenter.resources.redraw = false;
    presenter.input(mouse(30, 5)).unwrap();
    assert!(
        !presenter.resources.redraw,
        "empty space has no hover feedback"
    );
    presenter.input(mouse(2, 0)).unwrap();
    assert!(presenter.resources.redraw, "entering shows the hint");
    let buffer = flushed(&mut presenter, 32, 6);
    assert!(
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("Show details")
    );
    let revision = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .revision;
    presenter.resources.redraw = false;
    for column in [3, 4, 5, 6, 7, 6, 5, 4] {
        presenter.input_with_capture(mouse(column, 0)).unwrap();
        assert!(!presenter.resources.redraw, "same button at {column}");
    }
    assert_eq!(
        presenter
            .application
            .frame(&presenter.root)
            .unwrap()
            .revision,
        revision
    );
    presenter.resources.redraw = true;
    presenter.input(mouse(3, 0)).unwrap();
    assert!(
        presenter.resources.redraw,
        "pending business redraw survives"
    );
    presenter.resources.redraw = false;
    presenter.input(mouse(30, 5)).unwrap();
    assert!(presenter.resources.redraw, "leaving removes the hint");
    let buffer = flushed(&mut presenter, 32, 6);
    assert!(
        !buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("Show details")
    );
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn passive_text_hint_hover_never_steals_focus_or_activation() {
    let source = json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
        {"kind":"text","key":"status","text":"•","color":0x00ff00,"hint":"Succeeded"},
        {"kind":"button","key":"back","label":"Back"}
    ]});
    let (_runtime, mut presenter) = native(View::Scene(source), Rc::new(Cell::new(true)));
    flushed(&mut presenter, 24, 4);
    assert!(
        presenter
            .tab_stops()
            .iter()
            .all(|target| target.key != "status")
    );
    let focus = presenter.focus.as_ref().map(|target| target.key.clone());
    let revision = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .revision;
    let mouse = |kind, column, row| {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    };
    presenter
        .input_settled(mouse(MouseEventKind::Moved, 0, 0))
        .unwrap();
    let buffer = flushed(&mut presenter, 24, 4);
    let row: String = (0..24).map(|x| buffer[(x, 1)].symbol()).collect();
    assert!(row.contains("Succeeded"), "{row}");
    presenter
        .input_settled(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0))
        .unwrap();
    assert_eq!(
        presenter
            .application
            .frame(&presenter.root)
            .unwrap()
            .revision,
        revision
    );
    assert_eq!(
        presenter.focus.as_ref().map(|target| target.key.clone()),
        focus
    );
    presenter
        .input_settled(mouse(MouseEventKind::Moved, 20, 3))
        .unwrap();
    let buffer = flushed(&mut presenter, 24, 4);
    let row: String = (0..24).map(|x| buffer[(x, 1)].symbol()).collect();
    assert!(!row.contains("Succeeded"));
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn long_tooltip_wraps_above_a_low_anchor_without_hiding_its_explanation() {
    let source = json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
        {"kind":"text","key":"space","text":"\n\n","color":0xffffff},
        {"kind":"text","key":"status","text":"Privacy","color":0xffffff,
         "hint":"Chats are still saved. Model requests still use your provider."}
    ]});
    let (_runtime, mut presenter) = native(View::Scene(source), Rc::new(Cell::new(true)));
    flushed(&mut presenter, 26, 6);
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 0,
            row: 3,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
    let buffer = flushed(&mut presenter, 26, 6);
    let rows: Vec<String> = (0..3)
        .map(|y| {
            (0..26)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect();
    assert_eq!(
        rows.join(" "),
        "Chats are still saved. Model requests still use your provider."
    );
    assert_eq!(
        buffer[(0, 3)].symbol(),
        "P",
        "tooltip stays above its anchor"
    );
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}
