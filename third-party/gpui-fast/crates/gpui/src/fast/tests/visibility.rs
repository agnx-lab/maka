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

use crate::{
    AppContext as _, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, StatefulInteractiveElement as _, StyleRefinement, Styled as _, TestAppContext, Window,
    canvas, deferred, div, point, px,
};
use std::{cell::Cell, rc::Rc};

struct Content {
    renders: Rc<Cell<usize>>,
}
impl Render for Content {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div()
            .size_full()
            .child(deferred(div().size(px(20.)).bg(crate::red())))
    }
}
struct Branch {
    content: Entity<Content>,
    hidden: bool,
}
impl Render for Branch {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let branch = div().size(px(100.));
        let branch = if self.hidden {
            branch.invisible()
        } else {
            branch
        };
        div().size_full().child(
            branch.child(
                self.content
                    .clone()
                    .cached(StyleRefinement::default().size(px(100.))),
            ),
        )
    }
}

#[crate::test]
fn a_hidden_ancestor_cannot_replay_visible_deferred_content(cx: &mut TestAppContext) {
    cx.with_paused_automatic_drawing(|cx| {
        let renders = Rc::new(Cell::new(0));
        let window = cx.add_window({
            let renders = renders.clone();
            move |_, cx| Branch {
                content: cx.new(|_| Content { renders }),
                hidden: false,
            }
        });
        let draw = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear(cx);
                window
                    .rendered_frame
                    .scene
                    .quads
                    .iter()
                    .filter(|quad| quad.background == crate::red().into())
                    .count()
            })
            .unwrap()
        };
        assert_eq!(draw(cx), 1);
        window.update(cx, |_, _, cx| cx.notify()).unwrap();
        assert_eq!(draw(cx), 1);
        assert_eq!(renders.get(), 1, "the original cached content was reused");
        assert!(
            cx.update_window(window.into(), |_, window, _| {
                window.rendered_frame.retained.reused_any()
            })
            .unwrap()
        );
        window
            .update(cx, |view, _, cx| {
                view.hidden = true;
                cx.notify();
            })
            .unwrap();
        for _ in 0..3 {
            assert_eq!(
                draw(cx),
                0,
                "a hidden descendant never paints ghost content"
            );
            window.update(cx, |_, _, cx| cx.notify()).unwrap();
        }
        window
            .update(cx, |view, _, cx| {
                view.hidden = false;
                cx.notify();
            })
            .unwrap();
        assert_eq!(
            draw(cx),
            1,
            "showing the same cached owner restores actual paint"
        );
        assert_eq!(draw(cx), 1);
    });
}

#[crate::test]
fn hidden_scope_preserves_outside_autoscroll_and_restores_after_unwind(cx: &mut TestAppContext) {
    struct Probe(Rc<Cell<usize>>);
    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let runs = self.0.clone();
            canvas(
                move |bounds, window, _| {
                    let outside = bounds;
                    let inside = crate::Bounds {
                        origin: bounds.origin + point(px(10.), px(10.)),
                        ..bounds
                    };
                    window.request_autoscroll(outside);
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        window.with_prepaint_visibility(false, |window| {
                            window.request_autoscroll(inside);
                            window.with_prepaint_visibility(true, |window| {
                                window.request_autoscroll(inside)
                            });
                            assert_eq!(window.take_autoscroll(), None);
                            panic!("native prepaint scope unwind probe");
                        });
                    }));
                    assert!(result.is_err());
                    assert_eq!(window.take_autoscroll(), Some(outside));
                    window.request_autoscroll(inside);
                    assert_eq!(window.take_autoscroll(), Some(inside));
                    runs.set(runs.get() + 1);
                },
                |_, _, _, _| {},
            )
            .size(px(100.))
        }
    }
    cx.with_paused_automatic_drawing(|cx| {
        let runs = Rc::new(Cell::new(0));
        let window = cx.add_window({
            let runs = runs.clone();
            move |_, _| Probe(runs)
        });
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert!(runs.get() > 0);
    });
}

/// A lower-priority nested draw still follows its actual ancestor; cached and
/// rebuilt paint must replay that admission through the original frame stream.
#[crate::test]
fn nested_deferred_priority_preserves_causal_paint_through_reuse_and_rebuild(
    cx: &mut TestAppContext,
) {
    struct Gap(bool);
    impl Render for Gap {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let gap = div().size(px(20.)).bg(if self.0 {
                crate::white()
            } else {
                crate::black()
            });
            if self.0 {
                gap.child(deferred(div().size(px(10.)).bg(crate::white())))
            } else {
                gap
            }
        }
    }
    struct Nested {
        gap: Entity<Gap>,
        renders: Rc<Cell<usize>>,
        priorities: (usize, usize),
    }
    impl Render for Nested {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.set(self.renders.get() + 1);
            div()
                .size_full()
                .child(self.gap.clone())
                .child(
                    deferred(div().size(px(20.)).bg(crate::red()).child(
                        deferred(div().size(px(20.)).bg(crate::blue())).priority(self.priorities.1),
                    ))
                    .priority(self.priorities.0),
                )
                .child(deferred(div().size(px(20.)).bg(crate::green())).priority(3))
        }
    }
    struct Shell {
        content: Entity<Nested>,
        cached: bool,
    }
    impl Render for Shell {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let content = if self.cached {
                self.content
                    .clone()
                    .cached(StyleRefinement::default().size(px(100.)))
                    .into_any_element()
            } else {
                self.content.clone().into_any_element()
            };
            div().size_full().child(content)
        }
    }
    for (cached, priorities) in [false, true]
        .into_iter()
        .flat_map(|cached| [(1, 2), (2, 1), (1, 1)].map(|priorities| (cached, priorities)))
    {
        cx.with_paused_automatic_drawing(|cx| {
            let renders = Rc::new(Cell::new(0));
            let gap = cx.new(|_| Gap(false));
            let window = cx.add_window({
                let renders = renders.clone();
                let gap = gap.clone();
                move |_, cx| Shell {
                    content: cx.new(|_| Nested {
                        gap,
                        renders,
                        priorities,
                    }),
                    cached,
                }
            });
            let expected =
                [crate::red(), crate::blue(), crate::green()].map(crate::Background::from);
            let draw = |cx: &mut TestAppContext| {
                cx.update_window(window.into(), |_, window, cx| {
                    window.draw(cx).clear(cx);
                    let colors = window
                        .rendered_frame
                        .scene
                        .paint_operations
                        .iter()
                        .filter_map(|operation| match operation {
                            crate::scene::PaintOperation::Primitive(
                                crate::scene::Primitive::Quad(quad),
                            ) => Some(quad.background),
                            _ => None,
                        })
                        .filter(|background| expected.contains(background))
                        .collect::<Vec<_>>();
                    assert_eq!(colors, expected, "priority {priorities:?}");
                    window.describe_rendered_frame()
                })
                .unwrap()
            };
            let first = draw(cx);
            for _ in 0..3 {
                window.update(cx, |_, _, cx| cx.notify()).unwrap();
                assert_eq!(first, draw(cx));
                assert_eq!(renders.get(), 1, "ordinary cached replay");
            }
            gap.update(cx, |gap, cx| {
                gap.0 = true;
                cx.notify();
            });
            let rebuilt = draw(cx);
            assert_eq!(
                renders.get(),
                2,
                "deferred draws outside the gap require rebuilding their ancestor"
            );
            for _ in 0..3 {
                window.update(cx, |_, _, cx| cx.notify()).unwrap();
                assert_eq!(rebuilt, draw(cx), "replay the rebuilt deferred paint range");
            }
            let last = draw(cx);
            cx.update_window(window.into(), |_, window, _| window.forget_retained_state())
                .unwrap();
            assert_eq!(last, draw(cx), "fresh and retained composition agree");
        });
    }
}

#[crate::test]
fn visible_on_hover_admits_deferred_paint_and_refreshes_hidden_prepaint_context(
    cx: &mut TestAppContext,
) {
    struct Reader(crate::ScrollHandle);
    impl Render for Reader {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(deferred(
                div()
                    .id("deferred-scroll-reader")
                    .size(px(100.))
                    .bg(crate::red())
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.0)
                    .children((0..3).map(|i| div().id(i).h(px(100.)).flex_shrink_0())),
            ))
        }
    }
    struct Appearing {
        reader: Entity<Reader>,
        hidden: bool,
    }
    impl Render for Appearing {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let branch = div().id("hover-visible").size(px(100.));
            let branch = if self.hidden {
                branch.invisible()
            } else {
                branch
            };
            div().size_full().child(
                branch
                    .hover(|mut style| {
                        style.visibility = Some(crate::Visibility::Visible);
                        style
                    })
                    .child(
                        self.reader
                            .clone()
                            .cached(StyleRefinement::default().size(px(100.))),
                    ),
            )
        }
    }
    cx.with_paused_automatic_drawing(|cx| {
        let handle = crate::ScrollHandle::new();
        let window = cx.add_window({
            let handle = handle.clone();
            move |_, cx| Appearing {
                reader: cx.new(|_| Reader(handle)),
                hidden: true,
            }
        });
        let draw = |cx: &mut TestAppContext| {
            cx.update_window(window.into(), |_, window, cx| {
                window.draw(cx).clear(cx);
                window
                    .rendered_frame
                    .scene
                    .quads
                    .iter()
                    .filter(|quad| quad.background == crate::red().into())
                    .count()
            })
            .unwrap()
        };
        cx.update_window(window.into(), |_, window, cx| {
            window.simulate_mouse_move(point(px(400.), px(400.)), cx)
        })
        .unwrap();
        assert_eq!(draw(cx), 0);
        handle.scroll_to_top_of_item(2);
        cx.update_window(window.into(), |_, window, cx| {
            window.simulate_mouse_move(point(px(10.), px(10.)), cx)
        })
        .unwrap();
        assert_eq!(
            draw(cx),
            1,
            "actual visible hover admits paint despite hidden prepaint"
        );
        assert_eq!(
            handle.offset().y,
            px(0.),
            "hidden prepaint did not resolve the item"
        );
        window
            .update(cx, |view, _, cx| {
                view.hidden = false;
                cx.notify();
            })
            .unwrap();
        assert_eq!(draw(cx), 1);
        assert_eq!(
            handle.offset().y,
            px(-200.),
            "visible context re-prepaints and resolves the still-pending item"
        );
    });
}
