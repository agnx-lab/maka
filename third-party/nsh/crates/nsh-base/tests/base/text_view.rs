//! Exercise the real component's CPU worker while its engine keeps rendering.
use nsh_base::text_view::{Block, ParseContext, mdast};
use nsh_base::{MarkdownPlugin, MarkdownPlugins, text_view};
use nsh_core::{
    Size,
    ui::{EngineContext, Root, engine_context, prelude::*},
};
use nsh_engine::EngineConfig;
use nsh_testing::Headless;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[derive(Default)]
struct Gate {
    state: Mutex<(Vec<String>, usize)>,
    wake: Condvar,
}
impl Gate {
    fn enter(&self, source: String) {
        let mut state = self.state.lock().unwrap();
        state.0.push(source);
        let call = state.0.len();
        self.wake.notify_all();
        while state.1 < call {
            state = self.wake.wait(state).unwrap();
        }
    }
    fn entered(&self, count: usize) {
        let state = self.state.lock().unwrap();
        let (state, result) = self
            .wake
            .wait_timeout_while(state, Duration::from_secs(5), |s| s.0.len() < count)
            .unwrap();
        let seen = state.0.clone();
        drop(state);
        assert!(
            !result.timed_out(),
            "parse {count} never started; saw {seen:?}"
        );
    }
    fn release(&self, count: usize) {
        self.state.lock().unwrap().1 = count;
        self.wake.notify_all();
    }
    fn sources(&self) -> Vec<String> {
        self.state.lock().unwrap().0.clone()
    }
}
struct Release(Arc<Gate>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release(usize::MAX)
    }
}
struct Paused(Arc<Gate>);
impl MarkdownPlugin for Paused {
    fn name(&self) -> &str {
        "test CPU gate"
    }
    fn block(&self, node: &mdast::Node, cx: &mut ParseContext<'_>) -> Option<Block> {
        if matches!(node, mdast::Node::Paragraph(_)) {
            self.0.enter(cx.source().to_owned());
        }
        None
    }
}
#[component]
fn Stream() -> impl IntoElement {
    provide_context((*engine_context::<MarkdownPlugins>().expect("test plugins")).clone());
    let source = signal(SharedString::from("**FIRST**"));
    let source_id = signal(SharedString::from("message"));
    let visible = signal(true);
    let status = signal(SharedString::from("ready"));
    view()
        .flex_col()
        .child(view().test_id("append1").h(px(20.0)).on_press(move || {
            source.set("**FIRST** tail1".into());
            status.set("append1 received".into());
        }))
        .child(view().test_id("append2").h(px(20.0)).on_press(move || {
            source.set("**FIRST** tail2".into());
            status.set("append2 received".into());
        }))
        .child(
            view()
                .test_id("replace")
                .h(px(20.0))
                .on_press(move || source.set("**REPLACED**".into())),
        )
        .child(
            view()
                .test_id("identity")
                .h(px(20.0))
                .on_press(move || source_id.set("another message".into())),
        )
        .child(view().test_id("dispose").h(px(20.0)).on_press(move || {
            visible.set(false);
            status.set("disposed".into());
        }))
        .child(
            view()
                .test_id("probe")
                .h(px(20.0))
                .on_press(move || status.set("probe received".into())),
        )
        .child(text(move || status.get()))
        .child(dynamic(move || {
            visible.get().then(|| {
                text_view()
                    .markdown(move || Some(source.get()))
                    .source_id(move || Some(source_id.get()))
                    .selectable(true)
            })
        }))
}
#[test]
fn one_worker_coalesces_and_publishes_a_real_prefix_without_crossing_identity_or_disposal() {
    let gate = Arc::new(Gate::default());
    let plugins = MarkdownPlugins::new().with(Paused(gate.clone()));
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(500.0), px(800.0)),
        context: EngineContext::default().with(plugins),
        ..EngineConfig::new(Root::of::<Stream>())
    })
    .unwrap();
    let _release = Release(gate.clone());
    app.wait_for("text:**FIRST**").unwrap();
    gate.entered(1);
    app.press("id:append1").unwrap();
    app.press("id:append2").unwrap();
    app.wait_for("append2 received").unwrap();
    assert_eq!(gate.sources(), ["**FIRST**"]);
    gate.release(1);
    gate.entered(2);
    app.wait_for("text:FIRST").unwrap();
    assert_eq!(gate.sources(), ["**FIRST**", "**FIRST** tail2"]);
    gate.release(2);
    app.wait_for("text:FIRST tail2").unwrap();
    app.press("id:replace").unwrap();
    gate.entered(3);
    app.press("id:identity").unwrap();
    gate.release(3);
    gate.entered(4);
    app.press("id:probe").unwrap();
    app.wait_for("probe received").unwrap();
    // The prior identity's completed parse must not replace this literal.
    app.wait_for("text:**REPLACED**").unwrap();
    assert!(!app.shows("REPLACED"));
    app.press("id:dispose").unwrap();
    gate.release(4);
    app.wait_for("disposed").unwrap();
    app.wait_for_gone("text:**REPLACED**", Duration::from_secs(5))
        .unwrap();
}

struct Failing;
impl MarkdownPlugin for Failing {
    fn name(&self) -> &str {
        "test parser failure"
    }
    fn block(&self, node: &mdast::Node, cx: &mut ParseContext<'_>) -> Option<Block> {
        if matches!(node, mdast::Node::Paragraph(_)) && cx.source().contains("tail") {
            panic!("test parser failure");
        }
        None
    }
}
#[component]
fn Failure() -> impl IntoElement {
    provide_context((*engine_context::<MarkdownPlugins>().expect("test plugins")).clone());
    let source = signal(SharedString::from("**FIRST**"));
    let failure = signal(SharedString::from("no error"));
    view()
        .flex_col()
        .child(
            view()
                .test_id("fail")
                .h(px(20.0))
                .on_press(move || source.set("**FIRST** full tail".into())),
        )
        .child(
            view()
                .test_id("recover")
                .h(px(20.0))
                .on_press(move || source.set("**RECOVERED**".into())),
        )
        .child(text(move || failure.get()))
        .child(
            text_view()
                .markdown(move || Some(source.get()))
                .on_parse_error(move |reason: &Option<SharedString>| {
                    failure.set(if reason.is_some() {
                        "format failed".into()
                    } else {
                        "no error".into()
                    })
                }),
        )
}
#[test]
fn a_failed_append_shows_the_complete_latest_source_and_can_recover() {
    let plugins = MarkdownPlugins::new().with(Failing);
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(500.0), px(400.0)),
        context: EngineContext::default().with(plugins),
        ..EngineConfig::new(Root::of::<Failure>())
    })
    .unwrap();
    app.wait_for("text:FIRST").unwrap();
    app.press("id:fail").unwrap();
    app.wait_for("format failed").unwrap();
    app.wait_for("text:**FIRST** full tail").unwrap();
    app.press("id:recover").unwrap();
    app.wait_for("text:RECOVERED").unwrap();
    app.wait_for("no error").unwrap();
}

#[component]
fn NestedCode() -> impl IntoElement {
    let source = signal(SharedString::from("- item\n\n  ```txt\n  first\n"));
    view()
        .flex_col()
        .child(view().test_id("append").h(px(20.0)).on_press(move || {
            source.set("- item\n\n  ```txt\n  first\n  \tsecond  \n\n  ```".into())
        }))
        .child(
            text_view()
                .source_id("nested")
                .markdown(move || Some(source.get()))
                .selectable(true)
                .code_block_actions(vec![
                    nsh_base::TextViewCodeAction::new("wrap").with_label("Wrap code"),
                    nsh_base::TextViewCodeAction::new("copy").with_label("Copy code"),
                ]),
        )
}
#[test]
fn nested_code_keeps_the_wrap_control_and_copies_the_exact_grown_body() {
    let mut app = Headless::start(EngineConfig {
        viewport: Size::new(px(500.0), px(500.0)),
        ..EngineConfig::new(Root::of::<NestedCode>())
    })
    .unwrap();
    app.wait_for("Wrap code").unwrap();
    let original = app.screen().press_target("Wrap code").unwrap().unwrap();
    assert!(app.screen().selected("Wrap code").unwrap());
    app.press("Wrap code").unwrap();
    assert!(!app.screen().selected("Wrap code").unwrap());
    app.press("id:append").unwrap();
    app.wait_for("text:/second/").unwrap();
    assert_eq!(
        app.screen().press_target("Wrap code").unwrap(),
        Some(original)
    );
    assert!(!app.screen().selected("Wrap code").unwrap());
    app.press("Copy code").unwrap();
    assert_eq!(app.screen().clipboard(), Some("first\n\tsecond  \n"));
}
