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
use std::{any::Any, cell::Cell, rc::Rc};

fn paint(presenter: &Presenter, width: u16, height: u16) -> (Buffer, PaintedScene) {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    let scene = presenter.paint(buffer.area, &mut buffer).unwrap();
    (buffer, scene)
}

fn targets(scene: &PaintedScene) -> Vec<(String, Placement)> {
    scene
        .targets
        .iter()
        .map(|target| (target.key.clone(), target.area))
        .collect()
}

fn clear(presenter: &Presenter) {
    for owner in presenter.resources.owners.values() {
        *owner.measurements.borrow_mut() = Default::default();
    }
}

fn close(presenter: &mut Presenter) {
    presenter
        .surface_closed(&presenter.surface(), Ok(()))
        .unwrap();
    presenter.shutdown_settled().unwrap();
}

#[test]
fn pure_history_reuses_measurement_and_flow_without_freezing_scroll_or_offscreen_targets() {
    let source = json!({"kind":"stack","key":"history","direction":"column","gap":0,"padding":0,
        "scroll":"manual","children":(0..32).map(|index| json!({"kind":"item","key":format!("item-{index}"),
            "content":{"kind":"stack","key":format!("body-{index}"),"direction":"column","gap":0,"padding":0,"children":[
                {"kind":"text","key":format!("text-{index}"),"text":format!("row {index:02}\ndetail"),"color":0xffffff},
                {"kind":"button","key":format!("button-{index}"),"label":"Open"}
            ]}})).collect::<Vec<_>>()});
    let (_runtime, mut presenter) =
        layout::native(layout::View::Scene(source), Rc::new(Cell::new(true)));
    let node = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .view
        .view
        .scene()
        .unwrap();
    let cold = super::super::paint::Measurements::guarded(10000);
    assert_eq!(
        presenter
            .measure(&presenter.root, node, 30, cold.context(Default::default()))
            .unwrap(),
        (8, 96)
    );
    let warm = super::super::paint::Measurements::guarded(1);
    assert_eq!(
        presenter
            .measure(&presenter.root, node, 30, warm.context(Default::default()))
            .unwrap(),
        (8, 96)
    );
    assert!(cold.requests() > 32);
    assert_eq!(warm.requests(), 1);

    clear(&presenter);
    let (cold, cold_scene) = paint(&presenter, 30, 8);
    let (warm, warm_scene) = paint(&presenter, 30, 8);
    assert_eq!(cold, warm);
    assert_eq!(targets(&cold_scene), targets(&warm_scene));
    assert_eq!(warm[(0, 0)].symbol(), "r");
    assert_eq!(
        warm_scene
            .targets
            .iter()
            .filter(|target| matches!(target.kind, paint::TargetKind::Item { .. }))
            .count(),
        32
    );
    let stats = presenter.resources.owners[&presenter.root.id()]
        .measurements
        .borrow()
        .stats();
    assert!(stats.2 > 0 && stats.4 > 0);
    presenter.present(warm_scene);
    presenter
        .input_settled(Event::Mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 1,
            row: 1,
            modifiers: KeyModifiers::NONE,
        }))
        .unwrap();
    assert_eq!(
        presenter.resources.owners[&presenter.root.id()]
            .state
            .downcast_ref::<super::super::scroll::SceneState>()
            .unwrap()
            .scrolls["history"]
            .offset,
        1
    );
    let (warm_scroll, warm_scene) = paint(&presenter, 30, 8);
    clear(&presenter);
    let (cold_scroll, cold_scene) = paint(&presenter, 30, 8);
    assert_eq!(warm_scroll, cold_scroll);
    assert_eq!(targets(&warm_scene), targets(&cold_scene));
    assert_eq!(
        (0..6)
            .map(|x| warm_scroll[(x, 0)].symbol())
            .collect::<String>(),
        "detail"
    );
    close(&mut presenter);
}

#[test]
fn retained_constraints_follow_actual_passes_without_consuming_native_capacity() {
    let source = json!({"kind":"stack","key":"body","direction":"column","gap":0,"padding":0,
        "children":[{"kind":"text","key":"words","text":"word ".repeat(40),"color":0xffffff},
            {"kind":"button","key":"back","label":"Back"}]});
    let (_runtime, mut presenter) =
        layout::native(layout::View::Scene(source), Rc::new(Cell::new(true)));
    let native_usage = presenter.resources.budget.usage();
    for width in 12..76 {
        paint(&presenter, width, 8);
    }
    let stats = presenter.resources.owners[&presenter.root.id()]
        .measurements
        .borrow()
        .stats();
    assert!(stats.0 > 0 && stats.1 > 0 && stats.2 > 0);
    assert_eq!(presenter.resources.budget.usage(), native_usage);
    let remaining_native_capacity = presenter
        .resources
        .budget
        .reserve(
            maka_ui_runtime::budget::Kind::Encoded,
            maka_ui_runtime::budget::BudgetLimits::default().encoded_bytes - native_usage.0,
        )
        .expect("derived cache must leave the original native admission capacity available");
    drop(remaining_native_capacity);

    // All earlier resize constraints have retired: the retained query set is
    // exactly the one reached by the last two actual paint passes.
    clear(&presenter);
    paint(&presenter, 74, 8);
    paint(&presenter, 75, 8);
    let fresh = presenter.resources.owners[&presenter.root.id()]
        .measurements
        .borrow()
        .stats();
    assert_eq!((stats.0, stats.1, stats.2), (fresh.0, fresh.1, fresh.2));

    let (retained, retained_scene) = paint(&presenter, 35, 8);
    assert_eq!(retained[(0, 0)].symbol(), "w");
    clear(&presenter);
    let (uncached, uncached_scene) = paint(&presenter, 35, 8);
    assert_eq!(retained, uncached);
    assert_eq!(targets(&retained_scene), targets(&uncached_scene));
    close(&mut presenter);
}

#[test]
fn every_width_queried_in_one_pass_remains_available_for_the_next_pass() {
    let source = json!({"kind":"stack","key":"body","direction":"column","gap":0,"padding":0,
        "children":[{"kind":"text","key":"words","text":"abcdefghij","color":0xffffff},
            {"kind":"button","key":"back","label":"Back"}]});
    let (_runtime, mut presenter) =
        layout::native(layout::View::Scene(source), Rc::new(Cell::new(true)));
    let node = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .view
        .view
        .scene()
        .unwrap();
    let widths = [12, 15, 18, 21, 24, 27];
    let cold = super::super::paint::Measurements::default();
    for width in widths {
        assert_eq!(
            presenter
                .measure(
                    &presenter.root,
                    node,
                    width,
                    cold.context(Default::default())
                )
                .unwrap(),
            (10, 2)
        );
    }
    assert!(cold.requests() > widths.len());
    // Each query returns the retained root size. A fixed per-node width count
    // would make earlier widths expand the subtree again in this order.
    for _ in 0..2 {
        let warm = super::super::paint::Measurements::guarded(widths.len());
        for width in widths {
            assert_eq!(
                presenter
                    .measure(
                        &presenter.root,
                        node,
                        width,
                        warm.context(Default::default())
                    )
                    .unwrap(),
                (10, 2)
            );
        }
        assert_eq!(warm.requests(), widths.len());
    }
    close(&mut presenter);
}

#[test]
fn viewport_intrinsic_cache_does_not_retain_dynamic_component_flow() {
    let mut appearance = support::brand_appearance()["textarea"].clone();
    appearance["borderWidth"] = json!(0);
    let source = Rc::new(std::cell::RefCell::new(json!({"kind":"stack","key":"body",
        "direction":"column","gap":0,"padding":0,"viewport":true,"children":[
            {"kind":"component","key":"editor","binding":"editor","member":0,"on":{},"props":{
                "initialText":"","placeholder":"Draft","autoFocus":false,"terminalRows":{"min":1,"max":5},
                "appearance":appearance,"accessibilityLabel":"Draft","readOnly":false,"disabled":false,
                "softWrap":true,"tabIndex":0}},
            {"kind":"button","key":"back","label":"Back"}]})));
    let (_runtime, mut presenter) = layout::native(
        layout::View::InteractiveScene(source),
        Rc::new(Cell::new(true)),
    );
    let recipe = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .view
        .view
        .clone();
    let (_, first) = paint(&presenter, 30, 10);
    assert_eq!(
        first
            .targets
            .iter()
            .find(|target| target.key == "back")
            .unwrap()
            .area
            .y,
        1
    );
    let editor = presenter
        .application
        .child_mount(&presenter.root, "editor")
        .unwrap()
        .clone();
    super::super::textarea::editor_owner_mut(
        presenter
            .resources
            .owners
            .get_mut(&editor.id())
            .unwrap()
            .state
            .as_mut(),
    )
    .editor
    .insert("one\ntwo\nthree");
    let (warm, warm_scene) = paint(&presenter, 30, 10);
    assert!(Rc::ptr_eq(
        &recipe,
        &presenter
            .application
            .frame(&presenter.root)
            .unwrap()
            .view
            .view
    ));
    assert_eq!(
        warm_scene
            .targets
            .iter()
            .find(|target| target.key == "back")
            .unwrap()
            .area
            .y,
        3
    );
    clear(&presenter);
    let (cold, cold_scene) = paint(&presenter, 30, 10);
    assert_eq!(warm, cold);
    assert_eq!(targets(&warm_scene), targets(&cold_scene));
    close(&mut presenter);
}

struct TemporaryScene(Rc<Cell<usize>>);

impl TemporaryScene {
    fn declaration(&self) -> maka_ui_runtime::NativeView<dyn Widget> {
        super::super::scene::compile(&json!({"kind":"stack","key":"body","direction":"column",
            "gap":0,"padding":0,"children":[
                {"kind":"text","key":"text","text":vec!["line";self.0.get()].join("\n"),"color":0xffffff},
                {"kind":"button","key":"back","label":"Back"}]})).unwrap()
    }
}

impl Widget for TemporaryScene {
    fn create(&self, _: NativeContext) -> Box<dyn Any> {
        Box::<super::super::scroll::SceneState>::default()
    }
    fn measure(&self, slots: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        self.declaration().view.measure(slots, width)
    }
    fn paint(
        &self,
        slots: &mut Slots<'_>,
        area: Placement,
        buffer: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        self.declaration().view.paint(slots, area, buffer)
    }
}

#[test]
fn native_temporary_nodes_are_not_indexed_under_the_owners_recipe_identity() {
    let lines = Rc::new(Cell::new(1));
    let (_runtime, mut presenter) = layout::native(
        layout::View::Native(Rc::new(TemporaryScene(lines.clone()))),
        Rc::new(Cell::new(true)),
    );
    for count in [1, 3, 2, 4, 1] {
        lines.set(count);
        let (buffer, scene) = paint(&presenter, 30, 8);
        assert_eq!(buffer[(0, 0)].symbol(), "l");
        assert_eq!(
            scene
                .targets
                .iter()
                .find(|target| target.key == "back")
                .unwrap()
                .area
                .y,
            count as i64
        );
        assert_eq!(
            presenter.resources.owners[&presenter.root.id()]
                .measurements
                .borrow()
                .stats(),
            (0, 0, 0, 0, 0)
        );
    }
    close(&mut presenter);
}

#[test]
fn replacing_a_recipe_under_the_same_mount_replaces_retained_sizes_and_flow() {
    let source = Rc::new(std::cell::RefCell::new(json!({"kind":"stack","key":"body",
        "direction":"column","gap":0,"padding":0,"children":[
            {"kind":"text","key":"text","text":"one","color":0xffffff},
            {"kind":"button","key":"back","label":"Back"}]})));
    let (_runtime, mut presenter) = layout::native(
        layout::View::ChangingScene(source.clone()),
        Rc::new(Cell::new(true)),
    );
    let (_, before) = paint(&presenter, 30, 8);
    assert_eq!(
        before
            .targets
            .iter()
            .find(|target| target.key == "back")
            .unwrap()
            .area
            .y,
        1
    );
    source.borrow_mut()["children"][0]["text"] = json!("one\ntwo\nthree");
    let revision = presenter
        .application
        .frame(&presenter.root)
        .unwrap()
        .revision;
    presenter
        .application
        .dispatch(&presenter.root, revision, "back", json!(true))
        .unwrap();
    presenter.settle().unwrap();
    assert_eq!(
        presenter.resources.owners[&presenter.root.id()]
            .measurements
            .borrow()
            .stats(),
        (0, 0, 0, 0, 0)
    );
    let (warm, warm_scene) = paint(&presenter, 30, 8);
    assert_eq!(warm[(0, 2)].symbol(), "t");
    assert_eq!(
        warm_scene
            .targets
            .iter()
            .find(|target| target.key == "back")
            .unwrap()
            .area
            .y,
        3
    );
    clear(&presenter);
    let (cold, cold_scene) = paint(&presenter, 30, 8);
    assert_eq!(warm, cold);
    assert_eq!(targets(&warm_scene), targets(&cold_scene));
    close(&mut presenter);
}

struct ResponsiveWidth {
    scene: Rc<dyn Widget>,
    width: Rc<Cell<u16>>,
}

impl Widget for ResponsiveWidth {
    fn scene(&self) -> Option<&TerminalScene> {
        self.scene.scene()
    }
    fn create(&self, context: NativeContext) -> Box<dyn Any> {
        self.scene.create(context)
    }
    fn commit(&self, state: &mut dyn Any) -> maka_ui_runtime::Result<Option<NativeInput>> {
        self.scene.commit(state)
    }
    fn measure(&self, slots: &Slots<'_>, width: u16) -> maka_ui_runtime::Result<(u16, u32)> {
        self.scene.measure(slots, width)
    }
    fn paint(
        &self,
        slots: &mut Slots<'_>,
        mut area: Placement,
        buffer: &mut Buffer,
    ) -> maka_ui_runtime::Result<()> {
        area.bounds.width = self.width.get();
        self.scene.paint(slots, area, buffer)
    }
}

#[test]
fn scrollbar_narrowing_keeps_the_original_responsive_flow_mode() {
    let source = json!({"kind":"stack","key":"body","direction":"column","gap":0,"padding":0,"scroll":"manual",
        "wide":{"min_width":12,"direction":"column","children":{"left":{"basis":3},"right":{"basis":3}}},
        "children":[
            {"kind":"stack","key":"left","direction":"column","gap":0,"padding":0,"children":[
                {"kind":"text","key":"left-text","text":"L","color":0xffffff}]},
            {"kind":"stack","key":"right","direction":"column","gap":0,"padding":0,"children":[
                {"kind":"text","key":"right-text","text":"R","color":0xffffff}]}]});
    let width = Rc::new(Cell::new(12));
    let widget = ResponsiveWidth {
        scene: super::super::scene::compile(&source).unwrap().view,
        width: width.clone(),
    };
    let (_runtime, mut presenter) = layout::native(
        layout::View::Native(Rc::new(widget)),
        Rc::new(Cell::new(true)),
    );
    let (wide, _) = paint(&presenter, 30, 2);
    assert_eq!(wide[(0, 0)].symbol(), "L");
    assert_eq!(wide[(11, 0)].symbol(), "┃");
    width.set(11);
    let (narrow, narrow_scene) = paint(&presenter, 30, 2);
    assert_eq!(narrow[(0, 0)].symbol(), "L");
    assert_eq!(narrow[(0, 1)].symbol(), "R");
    assert_eq!(narrow[(10, 0)].symbol(), " ");
    clear(&presenter);
    let (cold, cold_scene) = paint(&presenter, 30, 2);
    assert_eq!(narrow, cold);
    assert_eq!(targets(&narrow_scene), targets(&cold_scene));
    close(&mut presenter);
}
