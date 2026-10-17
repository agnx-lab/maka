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
use maka_ui_runtime::actions::{CaptureCall, CapturePlan, CaptureTarget, ContinuationWitness};
use maka_ui_runtime::{
    ComponentCall, ComponentOutput, NativeCapability, NativeComponent, NativeExport, NativeView,
};
use std::{any::Any, cell::Cell, collections::BTreeMap, rc::Rc, sync::Arc};
struct Probe;
impl Widget for Probe {
    fn create(&self, _: NativeContext) -> Box<dyn Any> {
        Box::new(Cell::new(7u64))
    }
    fn capture(
        &self,
        _: &dyn Any,
        _: &CaptureTarget,
    ) -> maka_ui_runtime::Result<Arc<dyn maka_capabilities::call::ReadResource>> {
        Ok(Arc::new(Text))
    }
    fn capture_continuation(
        &self,
        state: &dyn Any,
        _: &CaptureTarget,
    ) -> maka_ui_runtime::Result<ContinuationWitness> {
        Ok(ContinuationWitness::new(
            state.downcast_ref::<Cell<u64>>().unwrap().get(),
            8,
        ))
    }
    fn validate_continuation(
        &self,
        state: &dyn Any,
        _: &CaptureTarget,
        witness: &ContinuationWitness,
    ) -> maka_ui_runtime::Result<bool> {
        Ok(
            witness.downcast_ref::<u64>()
                == Some(&state.downcast_ref::<Cell<u64>>().unwrap().get()),
        )
    }
    fn measure(&self, _: &Slots<'_>, _: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        Ok((1, 1))
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
struct Text;
impl maka_capabilities::call::ReadResource for Text {
    fn resource_type(&self) -> maka_plugin_core::contract::ResourceType {
        maka_ui_runtime::text::text_resource_type()
    }
    fn bytes(&self) -> usize {
        5
    }
    fn metadata(&self) -> Value {
        json!({})
    }
    fn read(&self, range: std::ops::Range<usize>) -> Result<Vec<u8>, String> {
        b"Start"
            .get(range)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| "range".into())
    }
}
struct Component {
    visible: Rc<Cell<bool>>,
}
impl NativeComponent<dyn Widget> for Component {
    fn call(
        &self,
        call: ComponentCall<'_>,
        _: &maka_ui_runtime::abi::ValueBindings,
    ) -> maka_ui_runtime::Result<ComponentOutput<NativeView<dyn Widget>>> {
        let revision = match call {
            ComponentCall::Mount { .. } => 0,
            ComponentCall::Update { state, .. } | ComponentCall::Event { state, .. } => {
                state["revision"].as_u64().unwrap() + 1
            }
        };
        Ok(maka_ui_runtime::Frame {
            revision,
            state: json!({"revision":revision}),
            surface: None,
            view: NativeView::<dyn Widget>::new(Rc::new(Probe), BTreeMap::new(), vec![])?
                .with_root_visibility(self.visible.get()),
        }
        .output(vec![]))
    }
    fn capture(&self, _: CaptureCall<'_>) -> maka_ui_runtime::Result<CapturePlan> {
        Ok(CapturePlan::Native)
    }
}
fn terminal_fixture() -> (Input, Vec<Package>, maka_plugin_core::contract::Contract) {
    let (mut input, mut packages, contract) = support::continuation_fixture();
    input.environment.presentation = Some(PresentationTarget::Tui);
    for original in &mut packages {
        let declaration = original
            .manifest()
            .application
            .as_ref()
            .unwrap()
            .declaration()
            .clone();
        let source = original.file("index.mjs").map(|bytes| {
            let source = std::str::from_utf8(bytes).unwrap().to_owned();
            if original.manifest().id == "dynamic.app" {
                source.replace("initialText:'Start'", "initialText:state.label??'Start'")
                    .replace("else if(event.key==='remove')next.show=false;", "else if(event.key==='refresh')next.label=event.event.label;else if(event.key==='remove')next.show=false;")
                    .replace("{kind:'button',key:'read',label:'Capture draft'}", "{kind:'button',key:'refresh',label:'Refresh'},{kind:'button',key:'read',label:'Capture draft'}")
                    .replace("...(state.show?[{kind:'component',key:'editor'", "...[{kind:'stack',key:'editor-route',direction:'column',padding:0,gap:0,background:0,visible:state.show,children:[{kind:'component',key:'editor'")
                    .replace("command:state.command??null}}]:[])", "command:state.command??null}}]}]")
            } else { source }
        });
        let replacement = support::package(&original.manifest().id, declaration, source);
        for instance in input.instances.values_mut() {
            if instance.package == original.application_ref().into() {
                instance.package = replacement.application_ref().into();
            }
        }
        *original = replacement;
    }
    (input, packages, contract)
}
#[test]
fn terminal_pump_uses_original_native_continuation_before_feedback_publication() {
    let timer = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    for change in [
        "none",
        "reading",
        "owner",
        "retained_owner",
        "hidden",
        "hidden_restored",
        "own_hidden_restored",
    ] {
        let (input, packages, contract) = terminal_fixture();
        let mut registry = NativeRegistry::with_surface(PresentationTarget::Tui)
            .with_scene_compiler(scene::language(), scene::prepare, scene::finish)
            .unwrap();
        let visible = Rc::new(Cell::new(true));
        let component_visible = visible.clone();
        registry
            .register("maka.ui.textarea", move |_, _| {
                Ok(BTreeMap::from([(
                    "Textarea".into(),
                    NativeExport {
                        contract: contract.clone(),
                        value: NativeCapability::Component(Rc::new(Component {
                            visible: component_visible.clone(),
                        })),
                    },
                )]))
            })
            .unwrap();
        let app = Application::new(
            &input,
            packages,
            timer.handle().clone(),
            maka_ui_js::execution(Default::default()),
            registry,
            Default::default(),
        )
        .unwrap();
        let mut presenter = Presenter::mount(app, json!({})).unwrap();
        let outer = presenter
            .application
            .child_mount(&presenter.root, "editor")
            .unwrap()
            .clone();
        let leaf = presenter
            .application
            .child_mount(&outer, "draft")
            .unwrap()
            .clone();
        presenter
            .application
            .dispatch(&presenter.root, 0, "read", json!({"key":"A"}))
            .unwrap();
        timer
            .block_on(presenter.application.wait_prepared(&presenter.root.clone()))
            .unwrap();
        presenter.sync_native().unwrap();
        let task = presenter
            .application
            .start_task(&presenter.root)
            .unwrap()
            .unwrap();
        let completion = timer.block_on(task.future);
        let TaskCompletion::Query(ref result) = completion else {
            panic!("query")
        };
        assert_eq!(result.result.as_ref().unwrap()["text"], "Start");
        presenter
            .application
            .finish_task(task.id, completion)
            .unwrap();
        let retained = (change == "retained_owner")
            .then(|| presenter.resources.owners[&leaf.id()].identity.clone());
        match change {
            "reading" => presenter.resources.owners[&leaf.id()]
                .state
                .downcast_ref::<Cell<u64>>()
                .unwrap()
                .set(8),
            "owner" | "retained_owner" => {
                let owner = presenter.resources.owners.get_mut(&leaf.id()).unwrap();
                let original = Rc::downgrade(&owner.identity);
                owner.identity = Rc::new(());
                assert_eq!(original.upgrade().is_some(), retained.is_some());
            }
            "own_hidden_restored" => {
                let identity = presenter.resources.owners[&leaf.id()].identity.clone();
                for show in [false, true] {
                    visible.set(show);
                    let revision = presenter.application.frame(&leaf).unwrap().revision;
                    let revision_root = presenter
                        .application
                        .frame(&presenter.root)
                        .unwrap()
                        .revision;
                    presenter
                        .application
                        .dispatch(
                            &presenter.root,
                            revision_root,
                            "refresh",
                            json!({"label":if show { "restored" } else { "hidden" }}),
                        )
                        .unwrap();
                    timer
                        .block_on(presenter.application.wait_prepared(&presenter.root.clone()))
                        .unwrap();
                    presenter.sync_native().unwrap();
                    assert!(presenter.application.frame(&leaf).unwrap().revision > revision);
                }
                let area = Rect::new(0, 0, 80, 60);
                let scene = presenter.paint(area, &mut Buffer::empty(area)).unwrap();
                presenter.present(scene);
                assert!(Rc::ptr_eq(
                    &identity,
                    &presenter.resources.owners[&leaf.id()].identity
                ));
            }
            "hidden" | "hidden_restored" => {
                let identity = presenter.resources.owners[&leaf.id()].identity.clone();
                let revision = presenter
                    .application
                    .frame(&presenter.root)
                    .unwrap()
                    .revision;
                presenter
                    .application
                    .dispatch(&presenter.root, revision, "remove", json!({}))
                    .unwrap();
                if change == "hidden_restored" {
                    let revision = presenter
                        .application
                        .frame(&presenter.root)
                        .unwrap()
                        .revision;
                    presenter
                        .application
                        .dispatch(&presenter.root, revision, "restore", json!({}))
                        .unwrap();
                }
                assert!(Rc::ptr_eq(
                    &identity,
                    &presenter.resources.owners[&leaf.id()].identity
                ));
                assert_eq!(
                    presenter.application.child_mount(&outer, "draft").unwrap(),
                    &leaf
                );
            }
            _ => {}
        }
        assert!(
            presenter
                .application
                .frame(&presenter.root)
                .unwrap()
                .state
                .get("outcome")
                .is_none()
        );
        presenter.pump_settled().unwrap();
        assert_eq!(
            presenter.application.frame(&presenter.root).unwrap().state["outcome"]["status"],
            if change == "none" {
                "succeeded"
            } else {
                "stale"
            }
        );
        assert!(!presenter.application.is_retiring(&presenter.root));
        assert_eq!(presenter.application.accepted_action_count(), 0);
        drop(retained);
        presenter
            .surface_closed(&presenter.surface(), Ok(()))
            .unwrap();
        presenter.pump_settled().unwrap();
        presenter.shutdown_settled().unwrap();
    }
}
