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

#[path = "tests/activation.rs"]
mod activation;
#[path = "tests/adaptive.rs"]
mod adaptive;
#[path = "tests/continuations.rs"]
mod continuations;
#[path = "tests/layout.rs"]
mod layout;
#[path = "tests/reading.rs"]
mod reading;
#[path = "tests/reconciliation.rs"]
mod reconciliation;
#[path = "tests/rows.rs"]
mod rows;
#[path = "tests/terminal.rs"]
mod terminal;

use super::*;
#[path = "tests/driver.rs"]
mod driver;
use crossterm::event::{KeyEvent, MouseEvent};
use maka_ui_runtime::Surface;
use maka_ui_runtime::actions::ServiceCompletion;
#[allow(dead_code)]
#[path = "../../../ui-js/examples/counter/support.rs"]
mod support;
pub(super) fn fixture(handler: &str) -> (tokio::runtime::Runtime, Input, Vec<Package>) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let (mut input, packages) = support::fixture(handler);
    input.environment.presentation = Some(PresentationTarget::Tui);
    (runtime, input, packages)
}
fn draw(presenter: &mut Presenter, width: u16, height: u16) -> Buffer {
    for _ in 0..16 {
        let buffer = draw_frame(presenter, width, height);
        if !presenter.resources.redraw {
            return buffer;
        }
    }
    panic!("fixture presentation did not converge");
}
fn draw_frame(presenter: &mut Presenter, width: u16, height: u16) -> Buffer {
    presenter.settle().unwrap();
    presenter.sync_native().unwrap();
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    let scene = presenter.paint(buffer.area, &mut buffer).unwrap();
    presenter.present(scene);
    presenter.settle().unwrap();
    buffer
}

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
#[test]
fn idle_presentation_waits_for_input_and_committed_changes() {
    let (runtime, input, packages) =
        fixture("return frame({count:state.count+1,revision:state.revision+1});");
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    draw(&mut tui, 30, 12);
    draw(&mut tui, 30, 12);
    assert!(!tui.resources.redraw);
    assert!(tui.next_frame.is_none());
    for _ in 0..10 {
        tui.pump_settled().unwrap();
    }
    assert!(
        !tui.resources.redraw,
        "unchanged native polling must not request another layout"
    );
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert!(tui.resources.redraw);
    let painted = draw(&mut tui, 30, 12);
    assert!(painted.content.iter().any(|cell| cell.symbol() == "1"));
    draw(&mut tui, 30, 12);
    assert!(!tui.resources.redraw);
    tui.input_settled(Event::Resize(50, 20)).unwrap();
    assert!(
        tui.resources.redraw,
        "resize must redraw even without guest changes"
    );
}

#[test]
fn independent_terminal_owners_keep_state_and_paint_never_calls_guest() {
    let (runtime, input, packages) =
        fixture("return frame({count:state.count+1,revision:state.revision+1});");
    let mut independent = Application::<()>::new(
        &input,
        packages.clone(),
        runtime.handle().clone(),
        maka_ui_js::execution(Default::default()),
        support::scene::registry(PresentationTarget::Tui),
        Default::default(),
    )
    .unwrap();
    runtime.block_on(independent.wait_ready()).unwrap();
    let mount = independent.mount_launch(json!({})).unwrap();
    runtime.block_on(independent.wait_frame(&mount)).unwrap();
    assert!(matches!(
        independent.frame(&mount).unwrap().surface,
        Some(Surface::Terminal { .. })
    ));
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    assert!(matches!(
        tui.application.frame(&tui.root).unwrap().surface,
        Some(Surface::Terminal { .. })
    ));
    draw(&mut tui, 30, 12); // Establish mechanical focus after the first publication.
    let first = draw(&mut tui, 30, 12);
    let second = draw(&mut tui, 30, 12);
    assert_eq!(first, second);
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state,
        json!({"count":0,"revision":0})
    );
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 1);
    // Coordinates bind the last flushed scene revision. No repaint/re-resolution.
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 1);
    draw(&mut tui, 30, 12);
    let hit = tui.scene[0].area.visible();
    tui.input_settled(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: KeyModifiers::NONE,
    }))
    .unwrap();
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 2);
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    assert!(tui.can_shutdown());
    tui.shutdown_settled().unwrap();
    assert_eq!(
        independent.frame(&mount).unwrap().state,
        json!({"count":0,"revision":0})
    );
    independent.close(&mount, Ok(())).unwrap();
    runtime
        .block_on(std::future::poll_fn(|cx| {
            independent.set_waker(cx.waker().clone());
            match independent.shutdown() {
                Err(Error::Core(maka_plugin_core::Error::CleanupPending)) => {
                    std::task::Poll::Pending
                }
                result => std::task::Poll::Ready(result),
            }
        }))
        .unwrap();
}
#[test]
fn unflushed_geometry_is_not_interactive_and_failure_receipt_preserves_owner() {
    let (runtime, input, packages) =
        fixture("return frame({count:state.count+1,revision:state.revision+1});");
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    let area = Rect::new(0, 0, 30, 12);
    let mut buffer = Buffer::empty(area);
    let _unflushed = tui.paint(area, &mut buffer).unwrap();
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 0);
    draw(&mut tui, 30, 12);
    assert!(
        tui.surface_closed(&tui.surface(), Err("native restore failed".into()))
            .is_err()
    );
    assert!(!tui.can_shutdown());
    assert!(tui.application.frame(&tui.root).is_ok());
    assert!(tui.scene.is_empty());
    tui.input(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 0);
    // Duplicate close cannot turn the original failure into a success.
    assert!(tui.surface_closed(&tui.surface(), Ok(())).is_err());
    // A later explicit real restoration retry recovers this exact physical owner.
    tui.surface_recovered(&tui.surface()).unwrap();
    assert!(tui.can_shutdown());
    tui.shutdown_settled().unwrap();
}
#[test]
fn unicode_geometry_clips_and_every_untrusted_text_sink_removes_controls() {
    let input = "中文 e\u{301} 👩‍💻\n\x1b]52;c;hostile\x07\u{009b}31m";
    let clean = sanitize(input);
    assert!(!clean.chars().any(|c| c.is_control() && c != '\n'));
    assert_eq!(sanitize("a\tb\r\n"), "a    b\n");
    let area = Rect::new(1, 1, 4, 3);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 6));
    paint::text(input, area.into(), &mut buffer, Default::default());
    for y in 0..6 {
        for x in 0..8 {
            let cell = &buffer[(x, y)];
            assert!(!cell.symbol().chars().any(char::is_control));
            if !area.contains((x, y).into()) {
                assert_eq!(cell.symbol(), " ");
            }
        }
    }
    assert_eq!(paint::text_size("中文", 3), (2, 2));
}

#[test]
fn accepted_worker_survives_surface_close_and_cleanup_failure_keeps_actual_capacity() {
    use maka_plugin_core::contract::{Contract, Interface, Method, MethodMode, Transport};
    use maka_ui_runtime::{
        NativeCapability, NativeExport,
        actions::{NativeService, ServiceFuture},
    };
    use std::{
        collections::BTreeMap,
        sync::Arc,
        time::{Duration, Instant},
    };
    struct Gate {
        permits: Arc<tokio::sync::Semaphore>,
        started: std::sync::mpsc::Sender<std::sync::Weak<()>>,
        cleanup_failed: bool,
    }
    impl NativeService for Gate {
        fn call(&self, _: maka_ui_runtime::actions::ServiceRequest) -> ServiceFuture {
            let permits = self.permits.clone();
            let started = self.started.clone();
            let failed = self.cleanup_failed;
            Box::pin(async move {
                let payload = Arc::new(());
                started.send(Arc::downgrade(&payload)).unwrap();
                let permit = permits.acquire().await.unwrap();
                permit.forget();
                drop(payload);
                ServiceCompletion {
                    result: Err("ordinary business error".into()),
                    cleanup: if failed {
                        Err("actual executor cleanup failure".into())
                    } else {
                        Ok(())
                    },
                }
            })
        }
    }
    for cleanup_failed in [false, true] {
        let contract = Contract {
            name: "test.Service".into(),
            version: 1,
            transport: Transport::Local,
            interface: Interface::Service {
                methods: BTreeMap::from([(
                    "run".into(),
                    Method {
                        progress: None,
                        // This fixture waits on a resource; it performs no business mutation.
                        mode: MethodMode::Query,
                        input: json!({"type":"object"}),
                        output: json!({}),
                        resources: BTreeMap::new(),
                    },
                )]),
            },
        };
        let (runtime, input, packages) = service_fixture(
            "const out=frame({count:state.count+1,revision:state.revision+1});if(event.kind==='raw')out.actions.push({key:'finished',binding:'service',member:0,method:'run',input:{},resources:{}});return out;",
            &contract,
        );
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let (tx, rx) = std::sync::mpsc::channel();
        let service = Arc::new(Gate {
            permits: gate.clone(),
            started: tx,
            cleanup_failed,
        });
        let mut native = registry();
        native
            .register("test.service", move |_, _values| {
                Ok(BTreeMap::from([(
                    "Service".into(),
                    NativeExport {
                        contract: contract.clone(),
                        value: NativeCapability::Service(service.clone()),
                    },
                )]))
            })
            .unwrap();
        let mut tui = Presenter::new(
            &input,
            packages,
            runtime.handle().clone(),
            native,
            json!({}),
            maka_ui_js::execution(Default::default()),
        )
        .unwrap();
        draw(&mut tui, 30, 12);
        tui.input_settled(key(KeyCode::Enter)).unwrap();
        tui.pump_settled().unwrap();
        let actual_payload = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(tui.application.accepted_action_count(), 1);
        tui.surface_closed(&tui.surface(), Ok(())).unwrap();
        assert!(tui.application.frame(&tui.root).is_err());
        assert!(!tui.can_shutdown());
        assert!(actual_payload.upgrade().is_some());
        gate.add_permits(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !tui.jobs.is_empty() && Instant::now() < deadline {
            let _ = tui.pump_settled();
            std::thread::yield_now();
        }
        assert!(tui.jobs.is_empty());
        assert!(actual_payload.upgrade().is_none());
        if cleanup_failed {
            assert!(!tui.can_shutdown());
            assert_eq!(tui.application.accepted_action_count(), 1);
            let failed = tui.application.failed_action_ids();
            assert_eq!(failed.len(), 1);
            // Explicit actual-executor recovery receipt for this exact action.
            tui.application
                .complete_action_cleanup(failed[0], Ok(()))
                .unwrap();
        } else {
            assert_eq!(tui.application.accepted_action_count(), 0);
        }
        assert!(tui.can_shutdown());
        tui.shutdown_settled().unwrap();
    }
}

#[test]
fn terminal_surface_rejects_window_variants_and_unknown_fields() {
    assert!(serde_json::from_value::<maka_ui_runtime::Frame>(json!({"revision":0,"state":{},"window":{"binding":"surface","width":640,"height":400,"title":"old"},"view":{"kind":"text","key":"a","text":"x","color":0}})).is_err());
    assert!(
        serde_json::from_value::<Surface>(
            json!({"kind":"terminal","binding":"surface","width":640})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<Surface>(
            json!({"kind":"window","binding":"surface","width":640,"height":400,"title":"removed"})
        )
        .is_err()
    );
}

fn terminal_packages(mut input: Input, packages: Vec<Package>) -> (Input, Vec<Package>) {
    use std::collections::BTreeMap;
    let mut adapted = Vec::new();
    for original in packages {
        let declaration = original
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone();
        let mut files = BTreeMap::from([(
            maka_packages::MANIFEST_FILE.into(),
            serde_json::to_vec(
                &json!({"schemaVersion":1,"id":original.manifest().id,"application":declaration}),
            )
            .unwrap(),
        )]);
        if let Some(source) = original.file("index.mjs") {
            let source = std::str::from_utf8(source)
                .unwrap()
                .replace("gap:8,padding:16", "gap:1,padding:1")
                .replace("gap:8,padding:8", "gap:1,padding:0")
                .replace("gap:16,padding:0", "gap:2,padding:0");
            files.insert("index.mjs".into(), source.into_bytes());
        }
        let package = Package::new(files).unwrap();
        for node in input.instances.values_mut() {
            if node.package == original.application_ref().into() {
                node.package = package.application_ref().into();
            }
        }
        adapted.push(package);
    }
    input.environment.presentation = Some(PresentationTarget::Tui);
    (input, adapted)
}
#[test]
fn imported_native_recipe_child_state_and_focus_use_real_mount_lifetimes() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let (input, packages) = support::native_fixture();
    let (input, packages) = terminal_packages(input, packages);
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    let buffer = draw(&mut tui, 80, 20);
    assert!(
        buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("Native 0")
    );
    let right = tui
        .application
        .child_mount(&tui.root, "right")
        .unwrap()
        .clone();
    let old = tui
        .scene
        .iter()
        .find(|target| target.mount == right)
        .unwrap()
        .clone();
    tui.focus = Some(old.clone());
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&right).unwrap().state["count"], 21);
    draw(&mut tui, 80, 20);
    let toggle = tui
        .scene
        .iter()
        .find(|t| t.key == "toggle" && t.mount == tui.root)
        .unwrap()
        .clone();
    tui.application
        .dispatch(&toggle.mount, toggle.revision, &toggle.key, json!({}))
        .unwrap();
    tui.pump_settled().unwrap();
    assert!(tui.focus.is_none());
    draw(&mut tui, 80, 20);
    let toggle = tui
        .scene
        .iter()
        .find(|t| t.key == "toggle" && t.mount == tui.root)
        .unwrap()
        .clone();
    tui.application
        .dispatch(&toggle.mount, toggle.revision, &toggle.key, json!({}))
        .unwrap();
    draw(&mut tui, 80, 20);
    let replacement = tui
        .application
        .child_mount(&tui.root, "right")
        .unwrap()
        .clone();
    assert_ne!(right, replacement);
    assert!(
        tui.application
            .dispatch(&old.mount, old.revision, &old.key, json!({}))
            .is_err()
    );
    assert_eq!(
        tui.application.frame(&replacement).unwrap().state["count"],
        20
    );
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    assert!(tui.can_shutdown());
    tui.shutdown_settled().unwrap();
}

#[test]
fn narrow_row_uses_the_same_remaining_width_for_measure_and_unicode_paint() {
    use std::collections::BTreeMap;
    let (runtime, mut input, mut packages) = fixture("");
    let original = packages.pop().unwrap();
    let tree = json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
        {"kind":"stack","key":"row","direction":"row","gap":0,"padding":0,"background":0,"children":[
            {"kind":"text","key":"first","text":"12345","color":16777215},
            {"kind":"text","key":"wrapped","text":"中文abcd.","color":16777215}]},
        {"kind":"text","key":"after","text":"after","color":16777215}]});
    let source = format!(
        "export function activate(){{return {{App:{{contract:{},kind:'component',component:['app']}}}};}} export const app={{mount(){{return {{actions:[],events:[],frame:{{revision:0,state:{{}},surface:{{kind:'terminal',binding:'surface'}},view:{}}}}};}},update(){{throw Error('unexpected update');}},event(){{throw Error('unexpected event');}}}};",
        serde_json::to_string(
            &original.manifest().application.as_ref().unwrap().entries
                [&maka_plugin_core::package::EntryTarget::Tui]
                .exports["App"]
                .contract
        )
        .unwrap(),
        tree
    );
    let app = Package::new(BTreeMap::from([
        (
            maka_packages::MANIFEST_FILE.into(),
            original
                .file(maka_packages::MANIFEST_FILE)
                .unwrap()
                .to_vec(),
        ),
        ("index.mjs".into(), source.into_bytes()),
    ]))
    .unwrap();
    input.instances.get_mut("app").unwrap().package = app.application_ref().into();
    packages.push(app);
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    let buffer = draw(&mut tui, 10, 5);
    // The word and its punctuation stay together after the wide graphemes.
    assert_eq!(buffer[(5, 1)].symbol(), "a");
    assert_eq!(buffer[(6, 1)].symbol(), "b");
    assert_eq!(buffer[(7, 1)].symbol(), "c");
    assert_eq!(buffer[(8, 1)].symbol(), "d");
    assert_eq!(buffer[(9, 1)].symbol(), ".");
    assert_eq!(buffer[(0, 2)].symbol(), "a");
    assert_eq!(buffer[(4, 2)].symbol(), "r");
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
}

fn service_fixture(
    handler: &str,
    contract: &maka_plugin_core::contract::Contract,
) -> (tokio::runtime::Runtime, Input, Vec<Package>) {
    use maka_plugin_core::{
        composition::Target,
        contract::{Availability, Cardinality, Export, Import},
        package::{Declaration, EntryDeclaration, EntryTarget, Implementation},
    };
    use std::collections::BTreeMap;
    let (runtime, mut input, mut packages) = fixture(handler);
    let service = support::package(
        "native.service",
        Declaration {
            entries: BTreeMap::from([(
                EntryTarget::Tui,
                EntryDeclaration {
                    scene: None,
                    implementation: Implementation::Native {
                        implementation: "test.service".into(),
                    },
                    configuration: json!({"type":"object"}),
                    imports: BTreeMap::new(),
                    exports: BTreeMap::from([(
                        "Service".into(),
                        Export {
                            contract: contract.clone(),
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
        .insert("service".into(), support::node(&service, 99));
    packages.push(service);
    let original = packages.remove(1);
    let mut declaration = original
        .manifest()
        .application
        .as_ref()
        .unwrap()
        .declaration()
        .clone();
    declaration
        .entries
        .get_mut(&EntryTarget::Tui)
        .unwrap()
        .imports
        .insert(
            "service".into(),
            Import {
                contract: contract.clone(),
                cardinality: Cardinality::One,
            },
        );
    let app = Package::new(BTreeMap::from([
        (
            maka_packages::MANIFEST_FILE.into(),
            serde_json::to_vec(
                &json!({"schemaVersion":1,"id":"dynamic.app","application":declaration}),
            )
            .unwrap(),
        ),
        (
            "index.mjs".into(),
            original.file("index.mjs").unwrap().to_vec(),
        ),
    ]))
    .unwrap();
    input.instances.get_mut("app").unwrap().package = app.application_ref().into();
    input.instances.get_mut("app").unwrap().bindings.insert(
        "service".into(),
        vec![Target::Instance {
            path: vec!["service".into()],
            export: "Service".into(),
        }],
    );
    packages.push(app);
    (runtime, input, packages)
}

#[path = "mutations.rs"]
mod mutations;

struct SlotRow {
    duplicate: bool,
}
impl Widget for SlotRow {
    fn measure(&self, slots: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        assert!(slots.measure("undeclared", width).is_err());
        let first = slots.measure("right", width / 2)?;
        let second = slots.measure("left", width / 2)?;
        // Repeated measurement is permitted and has no guest effects.
        assert_eq!(first, slots.measure("right", width / 2)?);
        Ok((
            first.0.saturating_add(second.0).min(width),
            first.1.max(second.1),
        ))
    }
    fn paint(
        &self,
        slots: &mut Slots<'_>,
        area: Placement,
        buffer: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        assert_eq!(slots.keys().collect::<Vec<_>>(), ["right", "left"]);
        let half = area.width / 2;
        slots.paint(
            "right",
            Placement::new(CellRect::new(area.x, area.y, half, area.height), area.clip),
            buffer,
        )?;
        slots.paint(
            if self.duplicate { "right" } else { "left" },
            Placement::new(
                CellRect::new(
                    area.x + i64::from(half),
                    area.y,
                    area.width - half,
                    area.height,
                ),
                area.clip,
            ),
            buffer,
        )
    }
}
struct SlotRoot {
    duplicate: bool,
}
impl maka_ui_runtime::NativeComponent<dyn Widget> for SlotRoot {
    fn call(
        &self,
        call: maka_ui_runtime::ComponentCall<'_>,
        _values: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<
        maka_ui_runtime::ComponentOutput<maka_ui_runtime::NativeView<dyn Widget>>,
    > {
        let revision = match call {
            maka_ui_runtime::ComponentCall::Mount { .. } => 0,
            maka_ui_runtime::ComponentCall::Update { state, .. }
            | maka_ui_runtime::ComponentCall::Event { state, .. } => {
                state["revision"].as_u64().unwrap() + 1
            }
        };
        let slots = [("right", 20), ("left", 10)]
            .into_iter()
            .map(|(key, initial)| maka_ui_runtime::ComponentSlot {
                key: key.into(),
                binding: "counter".into(),
                member: 0,
                on: Default::default(),
                props: json!({"label":format!("{key} {revision}"),"initial":initial}),
            })
            .collect();
        Ok(maka_ui_runtime::Frame {
            revision,
            state: json!({"revision":revision}),
            surface: Some(Surface::Terminal {
                binding: "surface".into(),
            }),
            view: maka_ui_runtime::NativeView::new(
                std::rc::Rc::new(SlotRow {
                    duplicate: self.duplicate,
                }) as std::rc::Rc<dyn Widget>,
                [("parent".into(), json!(true))].into(),
                slots,
            )?
            .with_hidden_slots(if revision % 2 == 1 {
                ["left".into()].into()
            } else {
                Default::default()
            })?,
        }
        .output(vec![]))
    }
}
fn native_slots_fixture(runtime: &tokio::runtime::Runtime, duplicate: bool) -> Presenter {
    use maka_plugin_core::package::{EntryTarget, Implementation};
    let (mut input, packages) =
        support::composed_fixture(support::CHILD_UPDATE, support::PARENT_EVENT);
    input.environment.presentation = Some(PresentationTarget::Tui);
    let mut converted = Vec::new();
    let mut root_contract = None;
    for old in packages {
        let mut declaration = old
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone();
        let mut entry = declaration.entries.remove(&EntryTarget::Tui).unwrap();
        let is_root = old.manifest().id == "dynamic.app";
        if is_root {
            entry.scene = None;
            entry.implementation = Implementation::Native {
                implementation: "test.terminal-slots".into(),
            };
            root_contract = Some(entry.exports["App"].contract.clone());
            entry.imports.get_mut("surface").unwrap().contract =
                maka_ui_runtime::surface_contract(PresentationTarget::Tui);
        }
        if old.manifest().id == "native.surface" {
            entry.implementation = Implementation::Native {
                implementation: "maka.ui.surface.tui".into(),
            };
            entry.exports.get_mut("Surface").unwrap().contract =
                maka_ui_runtime::surface_contract(PresentationTarget::Tui);
        }
        if entry.scene.is_some() {
            entry.scene = Some(super::scene::language());
        }
        declaration.entries.insert(EntryTarget::Tui, entry);
        let source = if is_root {
            None
        } else {
            old.file("index.mjs")
                .map(|s| std::str::from_utf8(s).unwrap().to_string())
        };
        let package = support::package(&old.manifest().id, declaration, source);
        for node in input.instances.values_mut() {
            if node.package == old.application_ref().into() {
                node.package = package.application_ref().into();
            }
        }
        converted.push(package);
    }
    let contract = root_contract.unwrap();
    let mut native = registry();
    native
        .register("test.terminal-slots", move |_, _values| {
            Ok(std::collections::BTreeMap::from([(
                "App".into(),
                maka_ui_runtime::NativeExport {
                    contract: contract.clone(),
                    value: maka_ui_runtime::NativeCapability::Component(std::rc::Rc::new(
                        SlotRoot { duplicate },
                    )),
                },
            )]))
        })
        .unwrap();
    Presenter::new(
        &input,
        converted,
        runtime.handle().clone(),
        native,
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap()
}
#[test]
fn native_terminal_slots_measure_and_paint_js_children_with_original_input_mounts() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let mut tui = native_slots_fixture(&runtime, false);
    let root = tui.root.clone();
    let left = tui.application.child_mount(&root, "left").unwrap().clone();
    let right = tui.application.child_mount(&root, "right").unwrap().clone();
    draw(&mut tui, 80, 40); // Publish initial keyboard focus.
    let first = draw(&mut tui, 80, 40);
    assert_eq!(first, draw(&mut tui, 80, 40));
    let target = tui
        .scene
        .iter()
        .find(|t| t.mount == left && t.key == "inc")
        .unwrap()
        .clone();
    tui.input_settled(Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: target.area.visible().x,
        row: target.area.visible().y,
        modifiers: KeyModifiers::NONE,
    }))
    .unwrap();
    assert_eq!(tui.application.frame(&left).unwrap().state["count"], 11);
    assert_eq!(tui.application.frame(&right).unwrap().state["count"], 20);
    tui.application
        .dispatch(&root, 0, "parent", Value::Null)
        .unwrap();
    draw(&mut tui, 80, 40);
    assert_eq!(tui.application.child_mount(&root, "left").unwrap(), &left);
    assert_eq!(tui.application.frame(&left).unwrap().state["count"], 11);
    assert_eq!(
        tui.application.frame(&left).unwrap().state["label"],
        "left 1"
    );
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
}
#[test]
fn native_terminal_hidden_slots_override_custom_parent_measurement_and_paint() {
    let hidden = json!({"kind":"stack","key":"root","direction":"column","padding":0,"gap":0,"background":0,"visible":false,"children":(0..257).map(|n| json!({"kind":"button","key":format!("button-{n}"),"label":"Hidden"})).collect::<Vec<_>>()});
    let compiled = super::scene::compile(&hidden).unwrap();
    assert!(!compiled.root_visible());
    assert!(!compiled.declares_input("button-256"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let mut tui = native_slots_fixture(&runtime, false);
    let root = tui.root.clone();
    let left = tui.application.child_mount(&root, "left").unwrap().clone();
    draw(&mut tui, 80, 40);
    let original = tui
        .scene
        .iter()
        .find(|target| target.mount == left && target.key == "inc")
        .unwrap()
        .clone();
    let owner = tui.resources.owners[&left.id()].identity.clone();
    tui.application
        .dispatch(&root, 0, "parent", Value::Null)
        .unwrap();
    tui.settle().unwrap();
    let frame = tui.application.frame(&root).unwrap();
    assert_eq!(frame.view.slots().len(), 2);
    assert_eq!(
        frame
            .view
            .visible_slots()
            .map(|slot| slot.key.as_str())
            .collect::<Vec<_>>(),
        ["right"]
    );
    let right = tui.application.child_mount(&root, "right").unwrap();
    assert_eq!(
        tui.measure_mount(
            &root,
            80,
            paint::Measurements::default().context(Default::default())
        )
        .unwrap(),
        tui.measure_mount(
            right,
            40,
            paint::Measurements::default().context(Default::default())
        )
        .unwrap(),
        "custom parent measurement excludes hidden left slot"
    );
    let mut pending_buffer = Buffer::empty(Rect::new(0, 0, 80, 40));
    let pending = tui.paint(pending_buffer.area, &mut pending_buffer).unwrap();
    assert!(!pending.targets.iter().any(|target| target.mount == left));
    let event = Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        tui.resources.input(&original, &event).unwrap().is_some(),
        "unflushed projection does not change last presented facts"
    );
    tui.present(pending);
    assert!(tui.resources.input(&original, &event).unwrap().is_none());
    tui.application
        .dispatch(&root, 1, "parent", Value::Null)
        .unwrap();
    draw(&mut tui, 80, 40);
    assert!(
        tui.scene
            .iter()
            .any(|target| target.mount == left && target.key == "inc")
    );
    assert!(std::rc::Rc::ptr_eq(
        &owner,
        &tui.resources.owners[&left.id()].identity
    ));
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
}
#[test]
fn duplicate_native_terminal_placement_rejects_pending_scene_before_input_publication() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let mut tui = native_slots_fixture(&runtime, true);
    let area = Rect::new(0, 0, 80, 40);
    let mut buffer = Buffer::empty(area);
    assert!(tui.paint(area, &mut buffer).is_err());
    assert!(tui.scene.is_empty());
    assert_eq!(tui.application.frame(&tui.root).unwrap().revision, 0);
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
}

#[test]
fn raw_click_focus_is_immediate_even_when_business_candidate_is_rejected() {
    for (handler, accepts, focuses) in [
        (
            "return frame({count:state.count+1,revision:state.revision+1});",
            true,
            true,
        ),
        ("throw new Error('reject input');", false, true),
    ] {
        let (runtime, input, packages) = fixture(handler);
        let mut tui = Presenter::new(
            &input,
            packages,
            runtime.handle().clone(),
            registry(),
            json!({}),
            maka_ui_js::execution(Default::default()),
        )
        .unwrap();
        draw(&mut tui, 30, 12);
        let hit = tui.scene[0].area.visible();
        tui.focus = None;
        let result = tui.input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: hit.x,
            row: hit.y,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(
            result.is_ok(),
            accepts,
            "click result for handler {handler}: {result:?}"
        );
        assert_eq!(tui.focus.is_some(), focuses);
        assert_eq!(
            tui.application.frame(&tui.root).unwrap().revision,
            u64::from(accepts)
        );
        tui.surface_closed(&tui.surface(), Ok(())).unwrap();
        tui.shutdown_settled().unwrap();
    }
}

#[path = "subscriptions.rs"]
mod subscriptions;

#[path = "tests/input.rs"]
mod input;
#[path = "tests/textarea.rs"]
mod textarea;

#[test]
fn pure_native_paint_panics_drain_original_state_and_failed_drop_keeps_root() {
    use std::{
        any::Any,
        collections::BTreeMap,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    struct State {
        drops: Arc<AtomicUsize>,
        failed: bool,
    }
    impl Drop for State {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            assert!(!self.failed, "original native destruction failed");
        }
    }
    struct Panic {
        drops: Arc<AtomicUsize>,
        failed: bool,
    }
    impl Widget for Panic {
        fn create(&self, _: super::NativeContext) -> Box<dyn Any> {
            Box::new(State {
                drops: self.drops.clone(),
                failed: self.failed,
            })
        }
        fn measure(&self, _: &Slots<'_>, _: u16) -> maka_ui_runtime::Result<(u16, u32)> {
            panic!("pure measurement failure")
        }
        fn paint(
            &self,
            _: &mut Slots<'_>,
            _: Placement,
            _: &mut Buffer,
        ) -> maka_ui_runtime::Result<()> {
            panic!("pure painting failure")
        }
    }
    for failed in [false, true] {
        let (runtime, input, packages) = fixture("");
        let drops = Arc::new(AtomicUsize::new(0));
        let watched = drops.clone();
        let native = NativeRegistry::with_surface(PresentationTarget::Tui)
            .with_scene_compiler(
                scene::language(),
                move |_| {
                    maka_ui_runtime::PreparedScene::new(
                        Panic {
                            drops: watched.clone(),
                            failed,
                        },
                        BTreeMap::new(),
                        vec![],
                    )
                },
                |prepared| prepared.finish::<Panic, dyn Widget>(|recipe| Rc::new(recipe)),
            )
            .unwrap();
        let mut tui = Presenter::new(
            &input,
            packages,
            runtime.handle().clone(),
            native,
            json!({}),
            maka_ui_js::execution(Default::default()),
        )
        .unwrap();
        assert!(
            tui.measure_mount(
                &tui.root,
                30,
                paint::Measurements::default().context(Default::default())
            )
            .is_err()
        );
        let area = Rect::new(0, 0, 30, 12);
        assert!(tui.paint(area, &mut Buffer::empty(area)).is_err());
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(tui.surface_closed(&tui.surface(), Ok(())).is_ok(), !failed);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(tui.can_shutdown(), !failed);
        if failed {
            assert!(tui.application.frame(&tui.root).is_ok());
            assert!(tui.shutdown_settled().is_err());
        } else {
            tui.shutdown_settled().unwrap();
        }
    }
}

#[path = "tests/source.rs"]
mod source;

#[test]
fn retired_target_cleanup_preserves_the_original_nested_scroll_ancestor_identity() {
    use maka_plugin_core::package::EntryTarget;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_time()
        .build()
        .unwrap();
    let (mut input, mut packages) = support::native_fixture();
    let index = packages
        .iter()
        .position(|package| package.manifest().id == "dynamic.app")
        .unwrap();
    let original = packages.remove(index);
    let source = std::str::from_utf8(original.file("index.mjs").unwrap())
        .unwrap()
        .to_owned()
        + "const originalFrame=frame;frame=state=>{const out=originalFrame(state);const root=out.frame.view;const counters=root.children.find(node=>node.key==='counters').children;root.children=[{kind:'stack',key:'outer',direction:'column',gap:1,padding:0,background:root.background,scroll:'manual',layout:{grow:1},children:[...(state.right?[counters.find(node=>node.key==='right')]:[]),{kind:'stack',key:'inner',direction:'column',gap:0,padding:0,background:root.background,scroll:'manual',layout:{basis:3},children:[{kind:'button',key:'inner-action',label:'Inner'},{kind:'text',key:'inner-tail',text:'inner\\n'.repeat(20),color:16777215}]},{kind:'text',key:'outer-tail',text:'outer\\n'.repeat(20),color:16777215}]},...root.children.filter(node=>node.kind==='button')];return out;};";
    let replacement = support::package(
        "dynamic.app",
        original
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone(),
        Some(source),
    );
    input.instances.get_mut("app").unwrap().package = replacement.application_ref().into();
    packages.push(replacement);
    let (input, packages) = terminal_packages(input, packages);
    assert!(packages.iter().any(|package| {
        package
            .manifest()
            .application
            .as_ref()
            .is_some_and(|app| app.entries.contains_key(&EntryTarget::Tui))
    }));
    let mut presenter = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    draw(&mut presenter, 40, 20);
    let target = presenter
        .scene
        .iter()
        .find(|target| target.key == "inner-action")
        .unwrap()
        .clone();
    let point = target.area.visible();
    let retired = presenter
        .application
        .child_mount(&presenter.root, "right")
        .unwrap()
        .clone();
    let toggle = presenter
        .scene
        .iter()
        .find(|target| target.key == "toggle")
        .unwrap()
        .clone();
    presenter
        .application
        .dispatch(&toggle.mount, toggle.revision, &toggle.key, json!({}))
        .unwrap();
    presenter.pump_settled().unwrap();
    assert!(!presenter.scene.iter().any(|target| target.mount == retired));
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: point.x,
            row: point.y,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
    let state = presenter.resources.owners[&presenter.root.id()]
        .state
        .downcast_ref::<super::scroll::SceneState>()
        .unwrap();
    assert_eq!(state.scrolls["inner"].offset, 1);
    assert_eq!(state.scrolls["outer"].offset, 0);
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn held_native_work_waits_through_kind_handoff_and_original_surface_close() {
    use std::{
        any::Any,
        collections::BTreeMap,
        rc::Rc,
        sync::{Arc, Mutex},
    };
    struct Old(Arc<Mutex<Option<maka_capabilities::call::Ticket>>>);
    struct New;
    impl Widget for Old {
        fn create(&self, context: NativeContext) -> Box<dyn Any> {
            let mut ticket = context.resources.reserve().unwrap();
            ticket.start();
            *self.0.lock().unwrap() = Some(ticket);
            Box::new(())
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
    impl Widget for New {
        fn create(&self, _: NativeContext) -> Box<dyn Any> {
            Box::new(42_u32)
        }
        fn measure(&self, slots: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
            assert_eq!(*slots.native_state::<u32>()?, 42);
            Ok((width, 1))
        }
        fn paint(
            &self,
            slots: &mut Slots<'_>,
            _: Placement,
            _: &mut Buffer,
        ) -> maka_ui_runtime::Result<()> {
            assert_eq!(*slots.native_state::<u32>()?, 42);
            Ok(())
        }
    }
    for (change_kind, aba) in [(false, false), (true, false), (true, true)] {
        let (runtime, input, packages) =
            fixture("return frame({count:state.count+1,revision:state.revision+1});");
        let ticket = Arc::new(Mutex::new(None));
        let held = ticket.clone();
        let native = NativeRegistry::with_surface(PresentationTarget::Tui)
            .with_scene_compiler(
                scene::language(),
                |view| {
                    maka_ui_runtime::PreparedScene::new(
                        view["children"][0]["text"] == "1",
                        BTreeMap::from([("inc".into(), json!({"type":"object"}))]),
                        vec![],
                    )
                },
                move |prepared| {
                    prepared.finish::<bool, dyn Widget>(|new| {
                        if new {
                            Rc::new(New) as Rc<dyn Widget>
                        } else {
                            Rc::new(Old(held.clone())) as Rc<dyn Widget>
                        }
                    })
                },
            )
            .unwrap();
        let mut tui = Presenter::new(
            &input,
            packages,
            runtime.handle().clone(),
            native,
            json!({}),
            maka_ui_js::execution(Default::default()),
        )
        .unwrap();
        let original = tui.root.clone();
        if change_kind {
            let identity = tui.resources.owners[&original.id()].identity.clone();
            for _ in 0..if aba { 2 } else { 1 } {
                tui.application
                    .dispatch(&original, 0, "inc", json!({}))
                    .unwrap();
                runtime
                    .block_on(tui.application.wait_prepared(&original))
                    .unwrap();
                let error = tui.pump_settled().unwrap_err().to_string();
                assert!(error.contains("native recipe type changed"), "{error}");
                assert_eq!(tui.application.frame(&original).unwrap().revision, 0);
                let owner = &tui.resources.owners[&original.id()];
                assert!(Rc::ptr_eq(&identity, &owner.identity));
                assert_eq!(
                    owner.recipe.as_ref().type_id(),
                    std::any::TypeId::of::<Old>()
                );
                assert!(tui.resources.ready(&tui.application, &original).unwrap());
                assert!(
                    ticket.lock().unwrap().is_some(),
                    "failed candidate must retain the original worker"
                );
                draw(&mut tui, 20, 4);
            }
            ticket.lock().unwrap().take().unwrap().complete(Ok(()));
        } else {
            tui.surface_closed(&original, Ok(())).unwrap();
            tui.pump_settled().unwrap();
            assert!(!tui.can_shutdown());
            assert!(tui.resources.owners.contains_key(&original.id()));
            ticket.lock().unwrap().take().unwrap().complete(Ok(()));
            tui.pump_settled().unwrap();
            assert!(!tui.resources.owners.contains_key(&original.id()));
        }
        if change_kind {
            tui.surface_closed(&original, Ok(())).unwrap();
        }
        assert!(tui.can_shutdown());
        tui.shutdown_settled().unwrap();
    }
}

#[test]
fn recovery_uses_independent_component_after_exact_original_surface_restoration() {
    let (runtime, mut input, mut packages) = fixture("throw new Error('primary fault');");
    support::add_recovery(&mut input, &mut packages);
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    draw(&mut tui, 30, 12);
    let old = tui.surface();
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert!(tui.requires_surface_replacement());
    assert!(tui.try_replace_surface(json!({})).is_err());
    // The receipt can precede the original realm's asynchronous close. Await
    // that owner's error rather than requiring its first pump to finish close.
    let failure = match tui.surface_closed(&old, Err("original Guard restoration failed".into())) {
        Err(error) => error,
        Ok(()) => runtime.block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                std::future::poll_fn(|cx| {
                    tui.application.set_waker(cx.waker().clone());
                    match tui.pump() {
                        Err(error) => std::task::Poll::Ready(error),
                        Ok(()) => std::task::Poll::Pending,
                    }
                }),
            )
            .await
            .expect("original Guard failure was not observed after realm close")
        }),
    };
    assert!(
        failure
            .to_string()
            .contains("original Guard restoration failed"),
        "{failure}"
    );
    assert!(tui.try_replace_surface(json!({})).is_err());
    assert_eq!(tui.surface(), old);
    tui.surface_recovered(&old).unwrap();
    assert!(tui.replace_surface_settled(json!({})).unwrap());
    assert_ne!(tui.surface(), old);
    assert_eq!(
        tui.application.root_kind(&tui.surface()).unwrap(),
        RootKind::Recovery
    );
    assert!(tui.surface_closed(&old, Ok(())).is_err());
    draw(&mut tui, 30, 12);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    assert_eq!(tui.application.frame(&tui.root).unwrap().state["count"], 1);
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    assert!(tui.can_shutdown());
    tui.shutdown_settled().unwrap();
}

#[test]
fn recovery_failure_does_not_recursively_replace_the_failed_recovery_root() {
    let (runtime, mut input, mut packages) = fixture("throw new Error('root fault');");
    support::add_recovery(&mut input, &mut packages);
    input.instances.get_mut("recovery").unwrap().package = input.instances["app"].package.clone();
    let mut tui = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    draw(&mut tui, 30, 12);
    tui.input_settled(key(KeyCode::Enter)).unwrap();
    tui.surface_closed(&tui.surface(), Ok(())).unwrap();
    assert!(tui.replace_surface_settled(json!({})).unwrap());
    let recovery = tui.surface();
    assert_eq!(
        tui.application.root_kind(&recovery).unwrap(),
        RootKind::Recovery
    );
    draw(&mut tui, 30, 12);
    assert!(tui.input_settled(key(KeyCode::Enter)).is_err());
    assert!(tui.application.is_retiring(&recovery));
    assert!(!tui.requires_surface_replacement());
    assert!(tui.try_replace_surface(json!({})).is_err());
    tui.surface_closed(&recovery, Ok(())).unwrap();
    tui.shutdown_settled().unwrap();
}

fn editor_owner(state: &dyn std::any::Any) -> &super::editor::Owner {
    state
        .downcast_ref::<super::editor::Owner>()
        .unwrap_or_else(|| super::textarea::editor_owner(state))
}
