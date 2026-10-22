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

use super::super::activation::Admission;
use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

fn click(presenter: &Presenter, key: &str) -> Event {
    let area = presenter
        .scene
        .iter()
        .find(|target| target.key == key)
        .unwrap()
        .area
        .visible();
    Event::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    })
}
fn refresh(presenter: &mut Presenter) {
    let frame = presenter.application.frame(&presenter.root).unwrap();
    let revision = frame.revision;
    let key = if frame.view.declares_input("back") {
        "back"
    } else {
        "outside"
    };
    presenter
        .application
        .dispatch(&presenter.root, revision, key, json!({"kind":"click"}))
        .unwrap();
    presenter.settle().unwrap();
}
fn replay(presenter: &mut Presenter, input: super::super::activation::CapturedActivation) {
    assert_eq!(presenter.activation_admission(&input), Admission::Ready);
    if let Some(event) = presenter.release_activation(input).unwrap() {
        presenter.input_settled(event).unwrap();
    } else {
        presenter.settle().unwrap();
    }
}
fn scene() -> Value {
    json!({"kind":"stack","key":"root","direction":"column","gap":0,"padding":0,"background":0,"children":[
        {"kind":"button","key":"save","label":"Save"}, {"kind":"button","key":"back","label":"Back"}]})
}
#[test]
fn approval_actions_and_close_survive_commits_between_output_receipts() {
    let (runtime, mut input, originals) = fixture("");
    let packages = originals.into_iter().map(|old| {
        if old.manifest().id != "dynamic.app" { return old; }
        let mut source = std::str::from_utf8(old.file("index.mjs").unwrap()).unwrap().to_owned();
        source.push_str(r#"
        frame=(state)=>({actions:[],events:[],frame:{revision:state.revision,state,surface:{kind:'terminal',binding:'surface'},view:{kind:'stack',key:'root',direction:'column',gap:1,padding:1,background:0,children:[
        {kind:'button',key:'close',label:'Close'},
        {kind:'item',key:'allow',current:state.decision==='allow',content:{kind:'text',key:'allow-label',text:'Allow '+state.scope,color:16777215}},
        {kind:'item',key:'session',current:state.scope==='session',content:{kind:'text',key:'session-label',text:'Session',color:16777215}},
        {kind:'button',key:'submit',label:'Submit'}]}}});
        app.mount=()=>frame({revision:0,decision:'deny',scope:'once',submitted:null,closed:false});
        app.event=(state,event)=>{const next={...state,revision:state.revision+1};if(event.key==='allow')next.decision='allow';if(event.key==='session')next.scope='session';if(event.key==='submit')next.submitted={decision:state.decision,scope:state.scope};if(event.key==='close')next.closed=true;return frame(next);};
        "#);
        let package=Package::new(BTreeMap::from([
            (maka_packages::MANIFEST_FILE.into(),old.file(maka_packages::MANIFEST_FILE).unwrap().to_vec()),
            ("index.mjs".into(),source.into_bytes())])).unwrap();
        input.instances.get_mut("app").unwrap().package=package.application_ref().into();
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
    draw(&mut tui, 40, 20);
    let session = click(&tui, "session");
    assert!(
        tui.capture_activation(&session).is_none(),
        "current idle actions retain the direct path"
    );
    tui.input_settled(session).unwrap();
    let allow = tui.capture_activation(&click(&tui, "allow")).unwrap();
    assert_eq!(tui.activation_admission(&allow), Admission::Pending);
    draw(&mut tui, 40, 20);
    replay(&mut tui, allow);
    draw(&mut tui, 40, 20);
    let submit = click(&tui, "submit");
    tui.input_settled(submit).unwrap();
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["submitted"],
        json!({"decision":"allow","scope":"session"})
    );
    let close = tui.capture_activation(&click(&tui, "close")).unwrap();
    assert_eq!(tui.activation_admission(&close), Admission::Pending);
    draw(&mut tui, 40, 20);
    replay(&mut tui, close);
    assert_eq!(
        tui.application.frame(&tui.root).unwrap().state["closed"],
        true
    );
}
#[test]
fn held_activation_rejects_disabled_hidden_replaced_or_covered_controls() {
    for change in ["disabled", "hidden", "role", "cell", "owner", "retired"] {
        let source = Rc::new(RefCell::new(scene()));
        let (_runtime, mut presenter) = layout::native(
            layout::View::ChangingScene(source.clone()),
            Rc::new(Cell::new(true)),
        );
        draw(&mut presenter, 40, 12);
        refresh(&mut presenter);
        let held = presenter
            .capture_activation(&click(&presenter, "save"))
            .unwrap();
        match change {
            "disabled" => source.borrow_mut()["children"][0]["disabled"] = json!(true),
            "hidden" => {
                let button = source.borrow()["children"][0].clone();
                source.borrow_mut()["children"][0] = json!({"kind":"stack","key":"hidden","direction":"column","gap":0,"padding":0,"background":0,"visible":false,"children":[button]});
            }
            "role" => {
                source.borrow_mut()["children"][0] = json!({"kind":"item","key":"save","content":{"kind":"text","key":"label","text":"Save","color":0xffffff}})
            }
            "cell" => source.borrow_mut()["children"]
                .as_array_mut()
                .unwrap()
                .insert(0, json!({"kind":"button","key":"cover","label":"Cover"})),
            "retired" => {
                presenter.application.retire(&presenter.root).unwrap();
            }
            "owner" => {
                presenter
                    .resources
                    .owners
                    .get_mut(&presenter.root.id())
                    .unwrap()
                    .identity = Rc::new(());
            }
            _ => unreachable!(),
        }
        if !matches!(change, "owner" | "retired") {
            refresh(&mut presenter);
            draw(&mut presenter, 40, 12);
        }
        assert_eq!(
            presenter.activation_admission(&held),
            Admission::Discard,
            "{change}"
        );
    }
}
#[test]
fn only_a_current_written_scene_releases_the_original_activation() {
    let source = Rc::new(RefCell::new(scene()));
    let (_runtime, mut presenter) = layout::native(
        layout::View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 40, 12);
    refresh(&mut presenter);
    let held = presenter
        .capture_activation(&click(&presenter, "save"))
        .unwrap();
    let area = Rect::new(0, 0, 40, 12);
    let older = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    refresh(&mut presenter);
    presenter.present_written(older);
    assert_eq!(presenter.activation_admission(&held), Admission::Pending);
    let current = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    assert_eq!(
        presenter.activation_admission(&held),
        Admission::Pending,
        "paint alone is not output acknowledgement"
    );
    presenter.present_written(current);
    replay(&mut presenter, held);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["last_input"]["key"],
        "save"
    );
}
#[test]
fn modal_close_and_reopen_revokes_a_held_activation() {
    let source = Rc::new(RefCell::new(
        json!({"kind":"dialog","key":"modal","label":"Commands","open":true,"width":24,"height":8,"padding":0,"background_color":0,"border_color":0xffffff,
        "background":{"kind":"button","key":"outside","label":"Outside"},"content":scene()}),
    ));
    let (_runtime, mut presenter) = layout::native(
        layout::View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 40, 12);
    refresh(&mut presenter);
    let held = presenter
        .capture_activation(&click(&presenter, "save"))
        .unwrap();
    source.borrow_mut()["open"] = json!(false);
    refresh(&mut presenter);
    source.borrow_mut()["open"] = json!(true);
    refresh(&mut presenter);
    draw(&mut presenter, 40, 12);
    assert_eq!(presenter.activation_admission(&held), Admission::Discard);
}

// Hold the delivery of one real guest invocation without blocking its worker or
// sleeping. The retained output remains a genuine uncommitted UI transaction.
type Gate = Rc<RefCell<Option<tokio::sync::oneshot::Receiver<()>>>>;
struct GatedFactory(Rc<dyn maka_ui_runtime::backend::Factory>, Gate);
struct GatedRealm(Rc<dyn maka_ui_runtime::backend::Realm>, Gate);
struct GatedModule(Rc<dyn maka_ui_runtime::backend::Module>, Gate);
struct GatedComponent(Rc<dyn maka_ui_runtime::backend::Component>, Gate);
impl maka_ui_runtime::backend::Factory for GatedFactory {
    fn create(
        &self,
        scheduler: tokio::runtime::Handle,
    ) -> maka_ui_runtime::Result<Rc<dyn maka_ui_runtime::backend::Realm>> {
        Ok(Rc::new(GatedRealm(
            self.0.create(scheduler)?,
            self.1.clone(),
        )))
    }
}
impl maka_ui_runtime::backend::Realm for GatedRealm {
    fn load(
        &self,
        name: &str,
        source: &str,
    ) -> maka_ui_runtime::Result<Rc<dyn maka_ui_runtime::backend::Module>> {
        Ok(Rc::new(GatedModule(
            self.0.load(name, source)?,
            self.1.clone(),
        )))
    }
    fn failure(&self) -> Option<String> {
        self.0.failure()
    }
}
impl maka_ui_runtime::backend::Module for GatedModule {
    fn activate(
        &self,
        configuration: &Value,
        values: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<
        maka_ui_runtime::backend::Job<BTreeMap<String, maka_ui_runtime::abi::UiExport>>,
    > {
        self.0.activate(configuration, values)
    }
    fn component(
        &self,
        namespace: &[String],
        captures: bool,
    ) -> maka_ui_runtime::Result<Rc<dyn maka_ui_runtime::backend::Component>> {
        Ok(Rc::new(GatedComponent(
            self.0.component(namespace, captures)?,
            self.1.clone(),
        )))
    }
    fn close(&self) -> maka_ui_runtime::Result<maka_ui_runtime::backend::Job<()>> {
        self.0.close()
    }
}
impl maka_ui_runtime::backend::Component for GatedComponent {
    fn call(
        &self,
        call: maka_ui_runtime::backend::Call,
        values: maka_ui_runtime::abi::ValueBindings,
        prepare: std::sync::Arc<maka_ui_runtime::PrepareScene>,
    ) -> maka_ui_runtime::Result<
        maka_ui_runtime::backend::Job<
            maka_ui_runtime::ComponentOutput<maka_ui_runtime::PreparedScene>,
        >,
    > {
        let job = self.0.call(call, values, prepare)?;
        let gate = self.1.borrow_mut().take();
        Ok(maka_ui_runtime::backend::Job::new(async move {
            if let Some(gate) = gate {
                gate.await
                    .map_err(|_| Error::Invalid("test gate closed".into()))?;
            }
            job.await
        }))
    }
    fn capture(
        &self,
        call: maka_ui_runtime::backend::Capture,
    ) -> maka_ui_runtime::Result<maka_ui_runtime::backend::Job<maka_ui_runtime::actions::CapturePlan>>
    {
        self.0.capture(call)
    }
}
#[test]
fn activation_waits_for_the_pending_transaction_and_its_own_successful_flush() {
    let (runtime, input, packages) =
        fixture("return frame({count:state.count+1,revision:state.revision+1});");
    let gate = Rc::new(RefCell::new(None));
    let script = maka_ui_js::execution(Default::default()).script.unwrap();
    let execution = maka_ui_runtime::ExecutionOptions {
        script: Some(Rc::new(GatedFactory(script, gate.clone()))),
    };
    let mut presenter = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        registry(),
        json!({}),
        execution,
    )
    .unwrap();
    draw(&mut presenter, 40, 12);
    let (release, receiver) = tokio::sync::oneshot::channel();
    *gate.borrow_mut() = Some(receiver);
    presenter.input(click(&presenter, "inc")).unwrap();
    assert!(presenter.application.ui_pending());
    let held = presenter
        .capture_activation(&click(&presenter, "inc"))
        .unwrap();
    let area = Rect::new(0, 0, 40, 12);
    let scene = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    presenter.present_written(scene);
    assert_eq!(
        presenter.activation_admission(&held),
        Admission::Pending,
        "an old current frame cannot release input ahead of its pending transaction"
    );
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        0
    );
    release.send(()).unwrap();
    presenter.settle().unwrap();
    assert_eq!(
        presenter.activation_admission(&held),
        Admission::Pending,
        "logical completion alone is not a successful output"
    );
    draw(&mut presenter, 40, 12);
    replay(&mut presenter, held);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        2
    );
}
#[test]
fn held_input_gets_an_idle_gap_before_ready_service_feedback() {
    use maka_plugin_core::contract::{Contract, Interface, Method, MethodMode, Transport};
    use maka_ui_runtime::{
        NativeCapability, NativeExport,
        actions::{NativeService, ServiceFuture},
    };
    use std::sync::Arc;
    struct Immediate;
    impl NativeService for Immediate {
        fn call(&self, _: maka_ui_runtime::actions::ServiceRequest) -> ServiceFuture {
            Box::pin(async {
                ServiceCompletion {
                    result: Ok(Value::Null),
                    cleanup: Ok(()),
                }
            })
        }
    }
    let contract = Contract {
        name: "test.Service".into(),
        version: 1,
        transport: Transport::Local,
        interface: Interface::Service {
            methods: BTreeMap::from([(
                "run".into(),
                Method {
                    progress: None,
                    mode: MethodMode::Query,
                    input: json!({"type":"object"}).into(),
                    output: json!({}).into(),
                    resources: BTreeMap::new(),
                },
            )]),
        },
    };
    let (runtime, mut input, packages) = service_fixture(
        "const out=frame({count:state.count+1,revision:state.revision+1});if(event.kind==='raw'&&state.count===0)out.actions.push({key:'finished',binding:'service',member:0,method:'run',input:{},resources:{}});return out;",
        &contract,
    );
    let packages=packages.into_iter().map(|old| {
        if old.manifest().id!="dynamic.app" {return old;}
        let mut source=std::str::from_utf8(old.file("index.mjs").unwrap()).unwrap().to_owned();
        source.push_str("const baseFrame=frame;frame=(state)=>{const out=baseFrame(state);out.frame.view.shortcuts=[{key:'k',modifiers:['control'],input:'inc',enabled:true}];return out;};");
        let package=Package::new(BTreeMap::from([(maka_packages::MANIFEST_FILE.into(),old.file(maka_packages::MANIFEST_FILE).unwrap().to_vec()),("index.mjs".into(),source.into_bytes())])).unwrap();
        input.instances.get_mut("app").unwrap().package=package.application_ref().into();package
    }).collect();
    let mut native = registry();
    native
        .register("test.service", move |_, _| {
            Ok(BTreeMap::from([(
                "Service".into(),
                NativeExport {
                    contract: contract.clone(),
                    value: NativeCapability::Service(Arc::new(Immediate)),
                },
            )]))
        })
        .unwrap();
    let mut presenter = Presenter::new(
        &input,
        packages,
        runtime.handle().clone(),
        native,
        json!({}),
        maka_ui_js::execution(Default::default()),
    )
    .unwrap();
    draw(&mut presenter, 40, 12);
    // Commit the first physical event without the driver's service feedback pump.
    presenter.input(click(&presenter, "inc")).unwrap();
    runtime
        .block_on(async {
            std::future::poll_fn(|cx| {
                presenter.application.set_waker(cx.waker().clone());
                presenter.application.advance_ui()?;
                presenter.sync_native()?;
                if presenter.application.ui_pending() {
                    std::task::Poll::Pending
                } else {
                    std::task::Poll::Ready(Ok::<_, Error>(()))
                }
            })
            .await
        })
        .unwrap();
    let held = presenter
        .input_with_capture(Event::Key(KeyEvent::new(
            KeyCode::Char('k'),
            KeyModifiers::CONTROL,
        )))
        .unwrap()
        .unwrap();
    runtime
        .block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                std::future::poll_fn(|cx| {
                    presenter.application.set_waker(cx.waker().clone());
                    presenter.pump_with_input_priority(true)?;
                    if presenter.jobs.is_empty() {
                        std::task::Poll::Ready(Ok::<_, Error>(()))
                    } else {
                        std::task::Poll::Pending
                    }
                }),
            )
            .await
            .unwrap()
        })
        .unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        1,
        "ready feedback must leave physical input an idle gap"
    );
    assert!(!presenter.application.ui_pending());
    let area = Rect::new(0, 0, 40, 12);
    let scene = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    presenter.present_written(scene);
    assert_eq!(presenter.activation_admission(&held), Admission::Ready);
    if let Some(event) = presenter.release_activation(held).unwrap() {
        presenter.input(event).unwrap();
    }
    presenter.settle().unwrap();
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        3,
        "ordinary pumping resumes ready feedback after physical admission"
    );
}

#[test]
fn sibling_openings_do_not_discard_an_unchanged_foreground_action() {
    let modal = |key: &str, content: Value| {
        json!({"kind":"dialog","key":key,"label":"Commands","open":true,"width":24,"height":8,"padding":0,"background_color":0,"border_color":0xffffff,
        "background":{"kind":"text","key":format!("{key}-background"),"text":"Background","color":0xffffff},"content":content})
    };
    let source = Rc::new(RefCell::new(
        json!({"kind":"stack","key":"page","direction":"column","gap":0,"padding":0,"background":0,"children":[
        modal("first",json!({"kind":"button","key":"first-action","label":"First"})),modal("second",scene())]}),
    ));
    let (_runtime, mut presenter) = layout::native(
        layout::View::ChangingScene(source),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 40, 12);
    refresh(&mut presenter);
    assert!(presenter.opening_witnesses().unwrap().len() > presenter.physical_path.len());
    let held = presenter
        .capture_activation(&click(&presenter, "save"))
        .unwrap();
    assert_eq!(presenter.activation_admission(&held), Admission::Pending);
    draw(&mut presenter, 40, 12);
    replay(&mut presenter, held);
}

#[test]
fn changed_item_submission_or_clipboard_payload_revokes_activation() {
    for item in [false, true] {
        let mut original = scene();
        original["children"][0] = if item {
            json!({"kind":"item","key":"save","submit":false,"content":{"kind":"text","key":"label","text":"Save","color":0xffffff}})
        } else {
            json!({"kind":"button","key":"save","label":"Copy","copy_text":"Original"})
        };
        let source = Rc::new(RefCell::new(original));
        let (_runtime, mut presenter) = layout::native(
            layout::View::ChangingScene(source.clone()),
            Rc::new(Cell::new(true)),
        );
        draw(&mut presenter, 40, 12);
        refresh(&mut presenter);
        let held = presenter
            .capture_activation(&click(&presenter, "save"))
            .unwrap();
        if item {
            source.borrow_mut()["children"][0]["submit"] = json!(true);
        } else {
            source.borrow_mut()["children"][0]["copy_text"] = json!("Replacement");
        }
        refresh(&mut presenter);
        draw(&mut presenter, 40, 12);
        assert_eq!(presenter.activation_admission(&held), Admission::Discard);
        assert!(presenter.clipboard.is_none());
    }
}

#[test]
fn wheel_drained_empty_or_native_click_cannot_retarget_a_new_allow_control() {
    for native in [false, true] {
        let mut original = scene();
        original["children"][0] = if native {
            json!({"kind":"component","key":"editor","binding":"editor","member":0,"on":{},"props":{
            "appearance":support::brand_appearance()["textarea"],"accessibilityLabel":"Composer","readOnly":false,"disabled":false,"softWrap":true,"tabIndex":0,"initialText":""}})
        } else {
            json!({"kind":"text","key":"empty","text":"Blank","color":0xffffff})
        };
        let source = Rc::new(RefCell::new(original));
        let (_runtime, mut presenter) = layout::native(
            layout::View::InteractiveScene(source.clone()),
            Rc::new(Cell::new(true)),
        );
        draw(&mut presenter, 40, 12);
        let event = Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        presenter
            .wheel(Event::Mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            }))
            .unwrap();
        presenter.sync_native().unwrap();
        // The runner's local next-input iteration handles this event before
        // another owner pump, including events with no declared scene action.
        assert!(presenter.capture_activation(&event).is_none());
        presenter.input(event).unwrap();
        source.borrow_mut()["children"][0] = json!({"kind":"button","key":"allow","label":"Allow"});
        refresh(&mut presenter);
        draw(&mut presenter, 40, 12);
        assert!(
            presenter
                .scene
                .iter()
                .any(|target| target.key == "allow" && target.area.contains((0, 0).into()))
        );
        assert_eq!(
            presenter.application.frame(&presenter.root).unwrap().state["last_input"]["key"],
            "back"
        );
    }
}

#[test]
fn first_written_stale_frame_retains_its_global_shortcut_until_current_output() {
    let mut original = scene();
    original["shortcuts"] =
        json!([{"key":"k","modifiers":["control"],"input":"back","enabled":true}]);
    let source = Rc::new(RefCell::new(original));
    let (_runtime, mut presenter) = layout::native(
        layout::View::ChangingScene(source),
        Rc::new(Cell::new(true)),
    );
    let area = Rect::new(0, 0, 40, 12);
    let first = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    refresh(&mut presenter);
    presenter.present_written(first);
    assert!(
        presenter.focus.is_none(),
        "a stale first output cannot establish logical focus"
    );
    let command = Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    let held = presenter
        .input_with_capture(command)
        .unwrap()
        .expect("the first visible global command must retain its original binding without focus");
    assert_eq!(presenter.activation_admission(&held), Admission::Pending);
    let current = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
    presenter.present_written(current);
    let count = presenter.application.frame(&presenter.root).unwrap().state["count"]
        .as_u64()
        .unwrap();
    replay(&mut presenter, held);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        count + 1
    );
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["last_input"]["key"],
        "back"
    );
}
#[test]
fn stale_shortcut_rejects_binding_replacement_instead_of_replaying_the_chord() {
    let mut original = scene();
    original["shortcuts"] =
        json!([{"key":"k","modifiers":["control"],"input":"back","enabled":true}]);
    let source = Rc::new(RefCell::new(original));
    let (_runtime, mut presenter) = layout::native(
        layout::View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 40, 12);
    source.borrow_mut()["shortcuts"][0]["input"] = json!("save");
    refresh(&mut presenter);
    let command = Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
    let held = presenter.input_with_capture(command).unwrap().unwrap();
    assert_eq!(presenter.activation_admission(&held), Admission::Pending);
    draw(&mut presenter, 40, 12);
    assert_eq!(presenter.activation_admission(&held), Admission::Discard);
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["last_input"]["key"],
        "back"
    );
}
#[test]
fn native_editor_undo_precedes_a_stale_global_shortcut_with_the_same_chord() {
    let mut original = scene();
    original["shortcuts"] =
        json!([{"key":"z","modifiers":["control"],"input":"back","enabled":true}]);
    original["children"][0] = json!({"kind":"component","key":"editor","binding":"editor","member":0,"on":{},"props":{
        "appearance":support::brand_appearance()["textarea"],"accessibilityLabel":"Composer","readOnly":false,"disabled":false,"softWrap":true,"tabIndex":0,"initialText":""}});
    let source = Rc::new(RefCell::new(original));
    let (_runtime, mut presenter) = layout::native(
        layout::View::InteractiveScene(source),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 40, 12);
    let editor = click(&presenter, "observe");
    presenter.input_settled(editor).unwrap();
    presenter
        .input_settled(Event::Paste("original-draft".into()))
        .unwrap();
    let before = draw(&mut presenter, 40, 12);
    assert!(
        before
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("original-draft")
    );
    refresh(&mut presenter);
    let count = presenter.application.frame(&presenter.root).unwrap().state["count"].clone();
    let undo = Event::Key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL));
    assert!(
        presenter.input_with_capture(undo).unwrap().is_none(),
        "native undo is consumed before shortcut capture"
    );
    let after = draw(&mut presenter, 40, 12);
    assert!(
        !after
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
            .contains("original-draft")
    );
    assert_eq!(
        presenter.application.frame(&presenter.root).unwrap().state["count"],
        count
    );
}

#[test]
fn yielded_navigation_shortcut_rejects_a_new_nearer_focus_binding() {
    let editor = json!({"kind":"component","key":"editor","binding":"editor","member":0,"on":{},"props":{
        "appearance":support::brand_appearance()["textarea"],"accessibilityLabel":"Composer","readOnly":false,"disabled":false,"softWrap":true,"tabIndex":0,"initialText":""}});
    let mut original = scene();
    original["shortcuts"] = json!([{"key":"up","modifiers":[],"input":"back","enabled":true}]);
    original["children"][0] = json!({"kind":"stack","key":"local","direction":"column","gap":0,"padding":0,"background":0,"children":[editor]});
    let source = Rc::new(RefCell::new(original));
    let (_runtime, mut presenter) = layout::native(
        layout::View::InteractiveScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    draw(&mut presenter, 40, 12);
    let editor = click(&presenter, "observe");
    presenter.input_settled(editor).unwrap();
    refresh(&mut presenter);
    let held = presenter
        .input_with_capture(key(KeyCode::Up))
        .unwrap()
        .expect("empty editor yields Up to the original ordinary shortcut");
    source.borrow_mut()["children"][0]["focus_shortcuts"] =
        json!([{"key":"up","modifiers":[],"child":"editor","enabled":false}]);
    refresh(&mut presenter);
    draw(&mut presenter, 40, 12);
    assert_eq!(
        presenter.activation_admission(&held),
        Admission::Discard,
        "a new nearest focus declaration owns navigation even when disabled"
    );
}
