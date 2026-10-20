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

use std::{
    collections::HashMap,
    rc::{Rc, Weak},
    sync::Arc,
};

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ClickEvent, ClickHandle, Context, Div, Element,
    ElementId, FocusHandle, GlobalElementId, InspectorElementId, InteractiveElement as _,
    IntoElement, LayoutId, Pixels, Role, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, WeakEntity, Window, div, size,
};

use super::{
    node::{BlockNode, LinkMark, LinkOccurrence, Paragraph},
    state::{ParsedContent, TextViewState},
};

pub(super) type EditorLinkHandler =
    dyn Fn(&SharedString, &ClickEvent, &mut Window, &mut App) + Send + Sync;

/// Rendering has exactly one activation authority. A retired TextView never
/// falls through to the external editor's default URL handling.
#[derive(Clone, Default)]
pub(crate) enum LinkSource {
    #[default]
    None,
    TextView(WeakEntity<TextViewState>),
    ExternalEditor(Option<Arc<EditorLinkHandler>>),
}

impl LinkSource {
    pub(super) fn text_view(&self) -> Option<&WeakEntity<TextViewState>> {
        match self {
            Self::TextView(view) => Some(view),
            Self::None | Self::ExternalEditor(_) => None,
        }
    }
}

pub(super) struct Fragment {
    mark: LinkMark,
    label: SharedString,
    bounds: Bounds<Pixels>,
}

impl Fragment {
    pub(super) fn new(mark: LinkMark, label: SharedString, bounds: Bounds<Pixels>) -> Self {
        Self {
            mark,
            label,
            bounds,
        }
    }
}

struct LinkOwner {
    mark: LinkMark,
    label: SharedString,
    focus: FocusHandle,
    lease: Option<Rc<()>>,
    click: Option<ClickHandle>,
}

impl LinkOwner {
    fn revoke(&mut self) {
        self.lease.take();
        self.focus = self.focus.clone().tab_stop(false);
        if let Some(click) = self.click.take() {
            click.cancel();
        }
    }
}

impl Drop for LinkOwner {
    fn drop(&mut self) {
        self.revoke();
    }
}

/// The document owner retains facts and native handles, never a second press state.
#[derive(Default)]
pub(super) struct Links(HashMap<LinkOccurrence, LinkOwner>);

impl Links {
    pub(super) fn reconcile(&mut self, content: &ParsedContent, cx: &mut Context<TextViewState>) {
        let mut facts = HashMap::new();
        fn paragraph(
            p: &Paragraph,
            content: &ParsedContent,
            facts: &mut HashMap<LinkOccurrence, (LinkMark, String)>,
        ) {
            let mut add = |mark: &LinkMark, label: &str| {
                let Some(occurrence) = mark.occurrence else {
                    return;
                };
                let mut mark = mark.clone();
                mark.resolve(&content.node_cx.link_refs);
                let fact = facts
                    .entry(occurrence)
                    .or_insert_with(|| (mark, String::new()));
                fact.1.push_str(label);
            };
            for node in &p.children {
                for (range, mark) in &node.marks {
                    if let Some(link) = &mark.link {
                        let label = node.text.get(range.clone()).unwrap_or_default();
                        add(link, label);
                    }
                }
                if let Some(image) = &node.image
                    && let Some(link) = &image.link
                {
                    add(link, &image.title());
                }
            }
        }
        fn block(
            b: &BlockNode,
            content: &ParsedContent,
            facts: &mut HashMap<LinkOccurrence, (LinkMark, String)>,
        ) {
            match b {
                BlockNode::Root { children, .. }
                | BlockNode::Blockquote { children, .. }
                | BlockNode::List { children, .. }
                | BlockNode::ListItem { children, .. } => {
                    for child in children {
                        block(child, content, facts);
                    }
                }
                BlockNode::Paragraph(p) | BlockNode::Heading { children: p, .. } => {
                    paragraph(p, content, facts)
                }
                BlockNode::Table(table) => {
                    for row in &table.children {
                        for cell in &row.children {
                            paragraph(&cell.children, content, facts);
                        }
                    }
                }
                _ => {}
            }
        }
        for b in content.document.blocks.iter() {
            block(b, content, &mut facts);
        }
        self.0.retain(|id, _| facts.contains_key(id));
        for (id, (mark, label)) in facts {
            if let Some(owner) = self.0.get_mut(&id) {
                if owner.mark.url != mark.url
                    || owner.mark.title != mark.title
                    || owner.mark.identifier != mark.identifier
                {
                    owner.revoke();
                }
                owner.mark = mark;
                owner.label = label.into();
            } else {
                self.0.insert(
                    id,
                    LinkOwner {
                        mark,
                        label: label.into(),
                        focus: cx.focus_handle(),
                        lease: None,
                        click: None,
                    },
                );
            }
        }
    }

    pub(super) fn revoke(&mut self) {
        for owner in self.0.values_mut() {
            owner.revoke();
        }
    }

    fn admits(&self, id: LinkOccurrence, lease: &Weak<()>) -> bool {
        self.0.get(&id).is_some_and(|owner| {
            owner
                .lease
                .as_ref()
                .is_some_and(|current| Weak::ptr_eq(&Rc::downgrade(current), lease))
                && lease.strong_count() > 0
        })
    }
}

/// Per successful paragraph prepaint. It does not escape into document/window state.
pub(super) type Fragments = Rc<std::cell::RefCell<Vec<Fragment>>>;

#[derive(Clone)]
enum Activation {
    TextView {
        view: WeakEntity<TextViewState>,
        id: LinkOccurrence,
        lease: Weak<()>,
    },
    ExternalEditor {
        url: SharedString,
        handler: Option<Arc<EditorLinkHandler>>,
    },
}

impl Activation {
    fn admitted(&self, cx: &App) -> bool {
        match self {
            Self::TextView { view, id, lease } => admitted(view, *id, lease, cx),
            Self::ExternalEditor { .. } => true,
        }
    }

    fn activate(&self, event: &ClickEvent, window: &mut Window, cx: &mut App) {
        match self {
            Self::TextView { view, id, lease } => activate(view, *id, lease, event, window, cx),
            Self::ExternalEditor { url, handler } => {
                if let Some(handler) = handler {
                    handler(url, event, window, cx);
                } else if activates(event) && !window.default_prevented() {
                    cx.open_url(url);
                }
            }
        }
    }
}

pub(super) fn targets(
    fragments: Vec<Fragment>,
    source: &LinkSource,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    if matches!(source, LinkSource::None) {
        return Vec::new();
    }
    let mut groups: Vec<(LinkMark, String, Vec<Bounds<Pixels>>)> = Vec::new();
    for Fragment {
        mark,
        label,
        bounds,
    } in fragments
    {
        if bounds.size.width <= Pixels::ZERO || bounds.size.height <= Pixels::ZERO {
            continue;
        }
        let Some(id) = mark.occurrence else { continue };
        if let Some((_, text, regions)) = groups
            .iter_mut()
            .find(|(mark, _, _)| mark.occurrence == Some(id))
        {
            text.push_str(&label);
            regions.push(bounds);
        } else {
            groups.push((mark, label.to_string(), vec![bounds]));
        }
    }
    let mut result = Vec::new();
    for (mark, label, regions) in groups {
        let id = mark.occurrence.unwrap();
        let (target_id, focus, activation, label, url, color) = match source {
            LinkSource::None => unreachable!(),
            LinkSource::TextView(weak) => {
                let Some(view) = weak.upgrade() else { continue };
                let prepared = view.update(cx, |state, _| {
                    if !state.interaction_enabled || state.parsed_error.is_some() {
                        return None;
                    }
                    let color = state.text_view_style.link();
                    let owner = state.links.0.get_mut(&id)?;
                    let lease = Rc::downgrade(owner.lease.get_or_insert_with(|| Rc::new(())));
                    owner.focus = owner.focus.clone().tab_stop(true);
                    Some((
                        owner.focus.clone(),
                        lease,
                        owner.label.clone(),
                        owner.mark.url.clone(),
                        color,
                    ))
                });
                let Some((focus, lease, label, url, color)) = prepared else {
                    continue;
                };
                (
                    SharedString::from(format!("link/{id:?}")),
                    focus,
                    Activation::TextView {
                        view: weak.clone(),
                        id,
                        lease,
                    },
                    label,
                    url,
                    color,
                )
            }
            LinkSource::ExternalEditor(handler) => {
                let target_id = SharedString::from(format!("editor-link/{id:?}/{}", mark.url));
                let focus = window
                    .use_keyed_state(target_id.clone(), cx, |_, cx| {
                        cx.focus_handle().tab_stop(true)
                    })
                    .read(cx)
                    .clone();
                (
                    target_id,
                    focus,
                    Activation::ExternalEditor {
                        url: mark.url.clone(),
                        handler: handler.clone(),
                    },
                    SharedString::from(label),
                    mark.url,
                    window.text_style().color,
                )
            }
        };
        let bounds = regions.iter().copied().reduce(|a, b| a.union(&b)).unwrap();
        let regions: Arc<[Bounds<Pixels>]> = regions
            .into_iter()
            .map(|r| Bounds::new(r.origin - bounds.origin, r.size))
            .collect::<Vec<_>>()
            .into();
        let mouse_activation = activation.clone();
        let key_activation = activation.clone();
        let click_activation = activation.clone();
        let aux_activation = activation.clone();
        let focus_activation = activation.clone();
        let ax_focus = focus.clone();
        let element = div()
            .id(target_id)
            .w(bounds.size.width)
            .h(bounds.size.height)
            .hitbox_regions(regions.clone())
            .track_focus(&focus)
            .tab_stop(true)
            .role(Role::Link)
            .aria_label(if label.is_empty() { url } else { label })
            .cursor_pointer()
            .on_a11y_action(gpui::accesskit::Action::Focus, move |_, window, cx| {
                if focus_activation.admitted(cx) {
                    window.focus(&ax_focus, cx);
                }
            })
            .capture_any_mouse_down(move |_, window, cx| {
                if !mouse_activation.admitted(cx) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            })
            .capture_key_down(move |_, window, cx| {
                if !key_activation.admitted(cx) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            })
            .on_click(move |event, window, cx| click_activation.activate(event, window, cx))
            .on_aux_click(move |event, window, cx| aux_activation.activate(event, window, cx));
        let mut target = LinkTarget {
            element,
            activation,
            focus,
            regions,
            color,
        }
        .into_any_element();
        target.prepaint_as_root(
            bounds.origin,
            size(
                AvailableSpace::Definite(bounds.size.width),
                AvailableSpace::Definite(bounds.size.height),
            ),
            window,
            cx,
        );
        result.push(target);
    }
    result
}

fn admitted(
    view: &WeakEntity<TextViewState>,
    id: LinkOccurrence,
    lease: &Weak<()>,
    cx: &App,
) -> bool {
    view.upgrade().is_some_and(|view| {
        let state = view.read(cx);
        state.interaction_enabled && state.parsed_error.is_none() && state.links.admits(id, lease)
    })
}

fn activate(
    view: &WeakEntity<TextViewState>,
    id: LinkOccurrence,
    lease: &Weak<()>,
    event: &ClickEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let data = view.upgrade().and_then(|view| {
        let state = view.read(cx);
        if !state.interaction_enabled
            || state.parsed_error.is_some()
            || !state.links.admits(id, lease)
            || (matches!(event, ClickEvent::Mouse(_) | ClickEvent::Touch(_))
                && state.has_selection(cx))
        {
            return None;
        }
        Some((
            state.links.0.get(&id)?.mark.url.clone(),
            state.link_activation_handler.clone(),
        ))
    });
    let Some((url, handler)) = data else { return };
    let decision = handler
        .as_ref()
        .map(|handler| handler(&url, event, window, cx))
        .unwrap_or_default();
    if decision.prevent_default {
        window.prevent_default();
    }
    if decision.stop_propagation {
        cx.stop_propagation();
    }
    if activates(event) && !window.default_prevented() && admitted(view, id, lease, cx) {
        crate::TextSelection::end(window, cx);
        cx.open_url(&url);
    }
}

fn activates(event: &ClickEvent) -> bool {
    match event {
        ClickEvent::Mouse(click) => matches!(
            click.up.button,
            gpui::MouseButton::Left | gpui::MouseButton::Middle
        ),
        ClickEvent::Touch(click) => !click.long_press,
        ClickEvent::Keyboard(_) | ClickEvent::Accessibility(_) => true,
    }
}

/// Delegates all native stages; only records the weak locator of its actual Div state.
struct LinkTarget {
    element: Stateful<Div>,
    activation: Activation,
    focus: FocusHandle,
    regions: Arc<[Bounds<Pixels>]>,
    color: gpui::Hsla,
}
impl IntoElement for LinkTarget {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for LinkTarget {
    type RequestLayoutState = <Stateful<Div> as Element>::RequestLayoutState;
    type PrepaintState = <Stateful<Div> as Element>::PrepaintState;
    fn id(&self) -> Option<ElementId> {
        Element::id(&self.element)
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        self.element.source_location()
    }
    fn a11y_role(&self) -> Option<gpui::Role> {
        self.element.a11y_role()
    }
    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        self.element.write_a11y_info(node);
    }
    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut Self::PrepaintState,
        builder: &mut gpui::A11ySubtreeBuilder,
    ) {
        Element::a11y_synthetic_children(&mut self.element, prepaint, builder);
    }
    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout = self.element.request_layout(id, inspector, window, cx);
        let click = self.element.interactivity().click_handle(id, window);
        if let Activation::TextView { view, id, lease } = &self.activation
            && let Some(view) = view.upgrade()
        {
            view.update(cx, |state, _| {
                if state.links.admits(*id, lease) {
                    state.links.0.get_mut(id).unwrap().click = click;
                }
            });
        }
        layout
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.element
            .prepaint(id, inspector, bounds, layout, window, cx)
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.element
            .paint(id, inspector, bounds, layout, prepaint, window, cx);
        if self.focus.is_focused(window)
            && window.last_input_was_keyboard()
            && self.activation.admitted(cx)
        {
            for region in self.regions.iter() {
                window.paint_quad(gpui::quad(
                    Bounds::new(bounds.origin + region.origin, region.size),
                    gpui::px(2.),
                    gpui::transparent_black(),
                    gpui::px(1.),
                    self.color,
                    gpui::BorderStyle::Solid,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::prelude::FluentBuilder as _;
    use gpui::{
        AppContext as _, Context, Entity, Modifiers, MouseButton, ParentElement as _, Render,
        TestAppContext, VisualContext as _, VisualTestContext, point, px,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Root {
        state: Entity<TextViewState>,
        calls: Arc<AtomicUsize>,
        selectable: bool,
        scrollable: bool,
        replace_on_click: Option<SharedString>,
        mdx: bool,
    }
    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let calls = self.calls.clone();
            let state = self.state.downgrade();
            let replacement = self.replace_on_click.clone();
            div()
                .w(px(180.))
                .h(px(100.))
                .bg(gpui::white())
                .child(crate::TextSelectionLayer)
                .child(
                    crate::TextView::new(&self.state)
                        .selectable(self.selectable)
                        .scrollable(self.scrollable)
                        .when(self.mdx, |this| this.markdown_mdx())
                        .on_link_activation(move |_, _, _, cx| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            if let Some(replacement) = &replacement {
                                state
                                    .update(cx, |state, cx| state.set_text(replacement, cx))
                                    .unwrap();
                            }
                            crate::text::LinkDecision::default()
                        }),
                )
        }
    }

    fn setup<'a>(
        source: &str,
        selectable: bool,
        scrollable: bool,
        replacement: Option<&str>,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<TextViewState>,
        Arc<AtomicUsize>,
        &'a mut VisualTestContext,
    ) {
        cx.update(crate::init);
        let calls = Arc::new(AtomicUsize::new(0));
        let (root, cx) = cx.add_window_view(|_, cx| Root {
            state: cx.new(|cx| TextViewState::markdown(source, cx)),
            calls: calls.clone(),
            selectable,
            scrollable,
            replace_on_click: replacement.map(Into::into),
            mdx: true,
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        draw(cx);
        let state = root.read_with(cx, |root, _| root.state.clone());
        (state, calls, cx)
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    fn tree_links(
        cx: &mut VisualTestContext,
    ) -> Vec<(gpui::accesskit::NodeId, gpui::accesskit::Node)> {
        let handle = cx.window_handle();
        cx.cx
            .accessibility_update(handle)
            .unwrap()
            .nodes
            .into_iter()
            .filter(|(_, node)| node.role() == Role::Link)
            .collect()
    }
    fn enable_ax(cx: &mut VisualTestContext) {
        let handle = cx.window_handle();
        cx.cx.activate_accessibility(handle);
        draw(cx);
    }
    fn ax(
        cx: &mut VisualTestContext,
        id: gpui::accesskit::NodeId,
        action: gpui::accesskit::Action,
    ) {
        let handle = cx.window_handle();
        cx.cx.simulate_accessibility_action(
            handle,
            gpui::accesskit::ActionRequest {
                action,
                target_node: id,
                target_tree: gpui::accesskit::TreeId::ROOT,
                data: None,
            },
        );
    }
    fn position(node: &gpui::accesskit::Node, cx: &mut VisualTestContext) -> gpui::Point<Pixels> {
        let rect = node.bounds().unwrap();
        let scale = cx.update(|window, _| window.scale_factor());
        point(
            px(rect.x0 as f32 / scale + 3.),
            px(rect.y0 as f32 / scale + 5.),
        )
    }

    fn key(cx: &mut VisualTestContext, key: &str) {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
    }

    #[gpui::test]
    fn external_editor_links_use_native_fragments_and_their_explicit_source(
        cx: &mut TestAppContext,
    ) {
        use crate::text::editor_host::{FlowPosition, RenderContext};
        use crate::text::node::{ImageNode, InlineNode, TextMark};

        struct Host {
            blocks: Vec<BlockNode>,
            context: RenderContext,
        }
        impl Render for Host {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let blocks: Vec<_> = self.blocks.iter().collect();
                div().w(px(180.)).children(
                    blocks
                        .iter()
                        .enumerate()
                        .map(|(ix, block)| {
                            block.render_with_id(
                                ix,
                                FlowPosition::at(&blocks, ix, ix + 1 == blocks.len()),
                                &self.context,
                                window,
                                cx,
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            }
        }

        cx.update(crate::init);
        let calls = Arc::new(std::sync::Mutex::new(Vec::<SharedString>::new()));
        let mouse_positions = Arc::new(std::sync::Mutex::new(None));
        let (host, cx) = cx.add_window_view({
            let calls = calls.clone();
            let mouse_positions = mouse_positions.clone();
            move |_, cx| {
                let text = "a hand-built code link wrapping across several native fragments";
                let mut paragraph = Paragraph::default();
                paragraph.push(InlineNode::new(text).marks(vec![(
                    0..text.len(),
                    TextMark::default().code().link(LinkMark {
                        url: "https://external.test".into(),
                        ..Default::default()
                    }),
                )]));
                let mut image_and_text = Paragraph::default();
                image_and_text.push(InlineNode::image(ImageNode {
                    url: "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='20' height='20'%3E%3C/svg%3E".into(),
                    link: Some(LinkMark { url: "https://image.test".into(), ..Default::default() }),
                    alt: Some("image".into()),
                    width: Some(px(20.).into()),
                    height: Some(px(20.).into()),
                    ..Default::default()
                }));
                image_and_text.push(InlineNode::new("text").marks(vec![(
                    0..4, TextMark::default().link(LinkMark { url: "https://external.test".into(), ..Default::default() }),
                )]));
                let blocks = vec![BlockNode::Paragraph(paragraph), BlockNode::Paragraph(image_and_text)];
                Host {
                    blocks,
                    context: RenderContext::from_app(cx).on_link_click(move |url, event, _, _| {
                        calls.lock().unwrap().push(url.clone());
                        if let ClickEvent::Mouse(click) = event {
                            *mouse_positions.lock().unwrap() =
                                Some((click.down.position, click.up.position));
                        }
                    }),
                }
            }
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        enable_ax(cx);
        let links = tree_links(cx);
        assert_eq!(
            links.len(),
            3,
            "hand-built image/text and same-URL occurrences remain distinct"
        );
        assert_eq!(
            links[0].1.label(),
            Some("a hand-built code link wrapping across several native fragments")
        );
        let rect = links[0].1.bounds().unwrap();
        let scale = cx.update(|window, _| window.scale_factor());
        assert!(
            rect.height() / scale as f64 > 40.,
            "the first link must actually wrap"
        );
        let first = position(&links[0].1, cx);
        let last = point(first.x, px(rect.y1 as f32 / scale - 5.));
        cx.simulate_mouse_down(first, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(last, MouseButton::Left, Modifiers::default());
        assert_eq!(*mouse_positions.lock().unwrap(), Some((first, last)));
        assert_eq!(
            calls.lock().unwrap().len(),
            1,
            "one cross-fragment native click"
        );
        ax(cx, links[0].0, gpui::accesskit::Action::Focus);
        key(cx, "enter");
        ax(cx, links[0].0, gpui::accesskit::Action::Click);
        assert_eq!(calls.lock().unwrap().len(), 3);
        assert_eq!(
            cx.opened_url(),
            None,
            "a host handler replaces default opening"
        );

        ax(cx, links[1].0, gpui::accesskit::Action::Click);
        ax(cx, links[2].0, gpui::accesskit::Action::Click);
        assert_eq!(
            calls
                .lock()
                .unwrap()
                .iter()
                .map(AsRef::<str>::as_ref)
                .collect::<Vec<_>>(),
            [
                "https://external.test",
                "https://external.test",
                "https://external.test",
                "https://image.test",
                "https://external.test",
            ]
        );

        host.update(cx, |host, cx| {
            host.context = RenderContext::new(host.context.style().clone());
            host.blocks = crate::text::editor_host::parse_markdown(
                "[default](https://default.test)",
                Default::default(),
            )
            .unwrap();
            cx.notify();
        });
        draw(cx);
        let default_link = tree_links(cx)[0].0;
        ax(cx, default_link, gpui::accesskit::Action::Click);
        assert_eq!(cx.opened_url().as_deref(), Some("https://default.test"));
        assert_eq!(calls.lock().unwrap().len(), 5);

        cx.update(|window, cx| {
            let state = cx.new(|cx| TextViewState::markdown("retired", cx));
            let retired = state.downgrade();
            drop(state);
            for source in [LinkSource::None, LinkSource::TextView(retired)] {
                let mark = LinkMark {
                    occurrence: Some(LinkOccurrence::ExternalEditor {
                        start: 0,
                        ordinal: 0,
                    }),
                    url: "https://must-not-open.test".into(),
                    ..Default::default()
                };
                let fragment = Fragment::new(
                    mark,
                    "retired".into(),
                    Bounds::new(point(px(0.), px(0.)), size(px(50.), px(20.))),
                );
                assert!(targets(vec![fragment], &source, window, cx).is_empty());
            }
        });
    }

    #[gpui::test]
    fn occurrences_are_native_links_with_one_focus_across_code_and_wrap(cx: &mut TestAppContext) {
        let (state, calls, cx) = setup(
            "[first **bold** `code` wrapping words wrapping words](https://same.test) [second](https://same.test)",
            false,
            false,
            None,
            cx,
        );
        enable_ax(cx);
        let links = tree_links(cx);
        assert_eq!(links.len(), 2);
        assert_ne!(links[0].0, links[1].0);
        let first = &links[0];
        assert!(first.1.label().unwrap().contains("bold code"));
        ax(cx, first.0, gpui::accesskit::Action::Focus);
        let focused = cx.update(|window, cx| window.focused(cx).unwrap());
        assert!(state.read_with(cx, |state, _| {
            state.links.0.values().any(|link| link.focus == focused)
        }));
        draw(cx);
        key(cx, "enter");
        key(cx, "space");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(cx.opened_url().as_deref(), Some("https://same.test"));
        draw(cx);
        let color = state.read_with(cx, |state, _| state.text_view_style.link());
        let rings = cx.update(|window, _| {
            window
                .painted_quads()
                .into_iter()
                .filter(|quad| {
                    quad.border_color == color
                        && quad.border_widths.top == px(1.).scale(window.scale_factor())
                })
                .count()
        });
        assert!(
            rings > 1,
            "one focused wrapped anchor must mark its actual fragments"
        );
        ax(cx, links[1].0, gpui::accesskit::Action::Click);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[gpui::test]
    fn revoked_native_mouse_keyboard_and_ax_do_not_revive_before_fresh_paint(
        cx: &mut TestAppContext,
    ) {
        cx.with_paused_automatic_drawing(|cx| {
            let (state, calls, cx) = setup("[first](https://old.test)", false, false, None, cx);
            enable_ax(cx);
            let (id, node) = tree_links(cx).remove(0);
            let position = position(&node, cx);
            ax(cx, id, gpui::accesskit::Action::Focus);
            let focus = cx.update(|window, cx| window.focused(cx).unwrap());
            cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
            state.update(cx, |state, cx| {
                state.set_interaction_enabled(false, cx);
                state.set_interaction_enabled(true, cx);
            });
            cx.update(|window, cx| window.blur(cx));
            ax(cx, id, gpui::accesskit::Action::Focus);
            assert!(cx.update(|window, cx| window.focused(cx).is_none()));
            ax(cx, id, gpui::accesskit::Action::Click);
            cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(cx.opened_url(), None);
            draw(cx);
            assert_eq!(tree_links(cx)[0].0, id);
            cx.update(|window, cx| window.focus_next(cx));
            assert!(
                cx.update(|window, _| focus.is_focused(window)),
                "fresh paint restores the original focus's Tab admission"
            );
            draw(cx);
            key(cx, "enter");
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        });
    }

    #[gpui::test]
    fn changed_target_and_parse_error_reject_old_ax_focus_and_click(cx: &mut TestAppContext) {
        for replacement in ["[first](https://new.test)", "{unclosed"] {
            cx.with_paused_automatic_drawing(|cx| {
                let (state, calls, cx) = setup("[first](https://old.test)", true, false, None, cx);
                let before = cx.opened_url();
                enable_ax(cx);
                let (id, node) = tree_links(cx).remove(0);
                let position = position(&node, cx);
                cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
                state.update(cx, |state, cx| state.set_text(replacement, cx));
                cx.update(|window, cx| window.blur(cx));
                ax(cx, id, gpui::accesskit::Action::Focus);
                ax(cx, id, gpui::accesskit::Action::Click);
                cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                assert!(cx.update(|window, cx| window.focused(cx).is_none()));
                assert_eq!(cx.opened_url(), before);
                state.update(cx, |state, cx| {
                    state.set_text("[first](https://recovered.test)", cx)
                });
                ax(cx, id, gpui::accesskit::Action::Click);
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                draw(cx);
                let (id, _) = tree_links(cx).remove(0);
                ax(cx, id, gpui::accesskit::Action::Click);
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                assert_eq!(cx.opened_url().as_deref(), Some("https://recovered.test"));
            });
        }
    }

    #[gpui::test]
    fn link_handler_can_replace_or_fail_source_without_old_default_or_owner_borrow(
        cx: &mut TestAppContext,
    ) {
        for replacement in ["[first](https://new.test)", "{unclosed"] {
            let (state, calls, cx) = setup(
                "[first](https://old.test)",
                false,
                false,
                Some(replacement),
                cx,
            );
            enable_ax(cx);
            let (id, _) = tree_links(cx).remove(0);
            ax(cx, id, gpui::accesskit::Action::Click);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(cx.opened_url(), None);
            state.read_with(cx, |state, _| {
                assert_eq!(state.parsed_error.is_some(), replacement == "{unclosed");
            });
        }
    }

    #[gpui::test]
    fn unchanged_streamed_occurrence_keeps_native_press_and_focus(cx: &mut TestAppContext) {
        cx.with_paused_automatic_drawing(|cx| {
            let (state, calls, cx) = setup("[first](https://same.test)", false, false, None, cx);
            enable_ax(cx);
            let (id, node) = tree_links(cx).remove(0);
            ax(cx, id, gpui::accesskit::Action::Focus);
            let focus = cx.update(|window, cx| window.focused(cx).unwrap());
            let position = position(&node, cx);
            cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
            state.update(cx, |state, cx| state.push_str(" continued", cx));
            cx.run_until_parked();
            assert_eq!(
                state.read_with(cx, |state, _| state.source().to_string()),
                "[first](https://same.test) continued"
            );
            cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            draw(cx);
            assert_eq!(tree_links(cx)[0].0, id);
            assert!(state.read_with(cx, |state, _| {
                state.links.0.values().any(|link| link.focus == focus)
            }));
        });
    }

    #[gpui::test]
    fn virtualized_focused_row_keeps_keyboard_and_text_view_action_ancestry(
        cx: &mut TestAppContext,
    ) {
        let source = format!(
            "[first](https://same.test)\n\n{}",
            "other paragraph words\n\n".repeat(80)
        );
        let (state, calls, cx) = setup(&source, true, true, None, cx);
        // Keep AX inactive so this uses the actual Fast List path.
        cx.update(|window, cx| window.focus_next(cx));
        let focus = cx.update(|window, cx| window.focused(cx).unwrap());
        assert!(state.read_with(cx, |state, _| {
            state.links.0.values().any(|link| link.focus == focus)
        }));
        state.update(cx, |state, _| state.list_state.scroll_by(px(600.)));
        draw(cx);
        draw(cx);
        assert!(
            state.read_with(cx, |state, _| state.list_state.logical_scroll_top().item_ix
                > 0)
        );
        key(cx, "enter");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        cx.dispatch_action(crate::input::SelectAll);
        draw(cx);
        assert!(!state.read_with(cx, |state, _| state.selected_text().is_empty()));
        cx.dispatch_action(crate::input::Copy);
        assert!(cx.update(|_, cx| {
            cx.read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .starts_with("first")
        }));
    }
    struct ProbeRoot {
        state: Entity<TextViewState>,
        list: gpui::ListState,
        probe_bounds: Arc<std::sync::Mutex<Option<Bounds<Pixels>>>>,
        row: Entity<ProbeRow>,
    }
    struct ProbeRow {
        state: Entity<TextViewState>,
        extensions: crate::text::MarkdownExtensions,
        calls: Arc<AtomicUsize>,
    }
    impl Render for ProbeRow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let calls = self.calls.clone();
            crate::TextView::new(&self.state)
                .selectable(false)
                .motion(
                    crate::text::TextViewMotion::default()
                        .with_stream_fade(std::time::Duration::ZERO),
                )
                .markdown_extensions(self.extensions.clone())
                .on_link_activation(move |_, _, _, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    crate::text::LinkDecision::default()
                })
        }
    }
    impl ProbeRoot {
        fn new(
            state: Entity<TextViewState>,
            prepaints: Arc<AtomicUsize>,
            calls: Arc<AtomicUsize>,
            cx: &mut Context<Self>,
        ) -> Self {
            let probe_bounds: Arc<std::sync::Mutex<Option<Bounds<Pixels>>>> = Arc::default();
            let bounds = probe_bounds.clone();
            let extensions = crate::text::MarkdownExtensions::default().plugin(
                crate::text::markdown_ext::TestInlinePlugin::new("math")
                    .parse_with(|node, _| {
                        matches!(node, markdown::mdast::Node::InlineMath(_))
                            .then(|| crate::text::MarkdownNode::new("math", ()).text("probe"))
                    })
                    .render_with(move |_, _, _, _| {
                        let prepaints = prepaints.clone();
                        let bounds = bounds.clone();
                        Some(crate::text::InlineElement::new(
                            div().w(px(20.)).h(px(20.)).child(
                                gpui::canvas(
                                    move |actual_bounds, _, _| {
                                        prepaints.fetch_add(1, Ordering::SeqCst);
                                        *bounds.lock().unwrap() = Some(actual_bounds);
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            ),
                        ))
                    }),
            );
            let row = cx.new(|_| ProbeRow {
                state: state.clone(),
                extensions,
                calls,
            });
            Self {
                state,
                list: gpui::ListState::new(1, gpui::ListAlignment::Top, px(1000.)).measure_all(),
                probe_bounds,
                row,
            }
        }
    }
    impl Render for ProbeRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let row = self.row.clone();
            div().size_full().bg(gpui::white()).child(
                div().w(px(180.)).h(px(100.)).child(
                    gpui::list(self.list.clone(), move |_, _, _| {
                        row.clone().into_any_element()
                    })
                    .size_full(),
                ),
            )
        }
    }

    #[gpui::test]
    fn ax_list_reveal_retries_original_paragraph_without_discarded_link_nodes(
        cx: &mut TestAppContext,
    ) {
        cx.with_paused_automatic_drawing(|cx| {
            cx.update(crate::init);
            let source = format!(
                "$probe$ {}[last](https://last.test)",
                "wrapping words ".repeat(100)
            );
            let prepaints = Arc::new(AtomicUsize::new(0));
            let calls = Arc::new(AtomicUsize::new(0));
            let (root, cx) = cx.add_window_view(|_, cx| {
                ProbeRoot::new(
                    cx.new(|cx| TextViewState::markdown(&source, cx)),
                    prepaints.clone(),
                    calls.clone(),
                    cx,
                )
            });
            let cx: &mut VisualTestContext = cx;
            cx.run_until_parked();
            draw(cx);
            enable_ax(cx);
            let state = root.read_with(cx, |root, _| root.state.clone());
            state.update(cx, |state, cx| {
                let text = state.rendered_text();
                let start = text.as_str().find("last").unwrap();
                state.reveal_range(start..start + 4, cx).unwrap();
            });
            let before = prepaints.load(Ordering::SeqCst);
            draw(cx);
            assert_eq!(
                prepaints.load(Ordering::SeqCst) - before,
                2,
                "the original List must discard and retry the first prepaint"
            );
            let links = tree_links(cx);
            assert_eq!(links.len(), 1);
            let rect = links[0].1.bounds().unwrap();
            let scale = cx.update(|window, _| window.scale_factor()) as f64;
            assert!(
                rect.y0 >= 0. && rect.y1 <= 100. * scale,
                "final AX bounds must follow the corrected original offset: {rect:?}"
            );
            ax(cx, links[0].0, gpui::accesskit::Action::Click);
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(cx.opened_url().as_deref(), Some("https://last.test"));
        });
    }

    #[gpui::test]
    fn scroll_retains_complete_link_fragments_in_the_original_list_row(cx: &mut TestAppContext) {
        cx.with_paused_automatic_drawing(|cx| {
            cx.update(crate::init);
            let source = format!(
                "{}[$probe$](https://last.test) {}",
                "wrapping words ".repeat(12),
                "trailing words ".repeat(100)
            );
            let prepaints = Arc::new(AtomicUsize::new(0));
            let calls = Arc::new(AtomicUsize::new(0));
            let (root, cx) = cx.add_window_view(|_, cx| ProbeRoot::new(
                cx.new(|cx| TextViewState::markdown(&source, cx)),
                prepaints.clone(), calls.clone(), cx,
            ));
            let cx: &mut VisualTestContext = cx;
            cx.run_until_parked();
            draw(cx);
            let list = root.read_with(cx, |root, _| root.list.clone());
            let initial = root.read_with(cx, |root, _| root.probe_bounds.lock().unwrap().unwrap());
            assert!(initial.top() >= px(100.), "probe starts wholly outside the actual viewport: {initial:?}");
            // Two native wheel frames promote the opaque List layer, then
            // a third same-row scroll must replay it without prepainting.
            for _ in 0..2 {
                let frame = cx.update(|window, _| window.layout_stats().frames);
                cx.simulate_event(gpui::ScrollWheelEvent {
                    position: point(px(40.), px(40.)),
                    delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-5.))),
                    modifiers: Modifiers::default(),
                    touch_phase: gpui::TouchPhase::Moved,
                });
                if cx.update(|window, _| window.layout_stats().frames) == frame { draw(cx); }
            }
            let before = prepaints.load(Ordering::SeqCst);
            let current_offset = list.logical_scroll_top().offset_in_item;
            let delta = initial.center().y - px(50.) - current_offset;
            let frame = cx.update(|window, _| window.layout_stats().frames);
            cx.simulate_event(gpui::ScrollWheelEvent {
                position: point(px(40.), px(40.)),
                delta: gpui::ScrollDelta::Pixels(point(px(0.), -delta)),
                modifiers: Modifiers::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            if cx.update(|window, _| window.layout_stats().frames) == frame { draw(cx); }
            assert_eq!(
                prepaints.load(Ordering::SeqCst),
                before,
                "prove actual keeps-row replay, with AX inactive: {:?}", cx.update(|window, _| window.layout_stats())
            );
            let offset = list.logical_scroll_top().offset_in_item;
            assert!(offset > px(0.));
            let position = initial.center() - point(px(0.), offset);
            assert!(position.y > px(10.) && position.y < px(90.));
            cx.simulate_click(position, Modifiers::default());
            assert_eq!(calls.load(Ordering::SeqCst), 1, "the original masked-out object fragment becomes clickable through retained native geometry");

        });
    }

    #[gpui::test]
    fn synchronous_shrink_and_regrow_keep_list_focus_metadata_aligned(cx: &mut TestAppContext) {
        cx.with_paused_automatic_drawing(|cx| {
            let source = format!(
                "{}[last](https://last.test)",
                "other paragraph words\n\n".repeat(60)
            );
            let (state, calls, cx) = setup(&source, false, true, None, cx);
            state.update(cx, |state, cx| {
                state.set_text("one", cx);
                state.set_text(&source, cx);
            });
            draw(cx);
            state.update(cx, |state, _| {
                let last = state.list_state.item_count() - 1;
                state.list_state.scroll_to_reveal_item(last);
            });
            draw(cx);
            assert!(
                state.read_with(cx, |state, _| state
                    .links
                    .0
                    .values()
                    .next()
                    .unwrap()
                    .lease
                    .is_some()),
                "the tail target must actually be prepainted before focusing"
            );
            let focus = state.read_with(cx, |state, _| {
                state.links.0.values().next().unwrap().focus.clone()
            });
            cx.update(|window, cx| window.focus(&focus, cx));
            draw(cx);
            state.update(cx, |state, _| {
                state.list_state.scroll_to(gpui::ListOffset::default())
            });
            draw(cx);
            draw(cx);
            assert_eq!(
                state.read_with(cx, |state, _| state.list_state.logical_scroll_top().item_ix),
                0
            );
            key(cx, "enter");
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        });
    }
    #[gpui::test]
    fn linked_images_share_one_native_anchor_and_cross_fragment_press(cx: &mut TestAppContext) {
        cx.update(crate::init);
        let source = r#"[<img src="https://example.com/one.svg" width="24" height="20" alt="one"><img src="https://example.com/two.svg" width="24" height="20" alt="two">](https://images.test)"#;
        let calls = Arc::new(AtomicUsize::new(0));
        let (_, cx) = cx.add_window_view(|_, cx| Root {
            state: cx.new(|cx| TextViewState::markdown(source, cx)),
            calls: calls.clone(),
            selectable: false,
            scrollable: false,
            replace_on_click: None,
            mdx: false,
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();
        draw(cx);
        enable_ax(cx);
        let links = tree_links(cx);
        assert_eq!(
            links.len(),
            1,
            "two images of one source anchor share the same native AX/focus target"
        );
        let (id, node) = &links[0];
        let rect = node.bounds().unwrap();
        let scale = cx.update(|window, _| window.scale_factor()) as f64;
        assert_eq!((rect.x1 - rect.x0) / scale, 48.);
        let first = point(
            px((rect.x0 / scale + 4.) as f32),
            px((rect.y0 / scale + 4.) as f32),
        );
        let second = point(px((rect.x1 / scale - 4.) as f32), first.y);
        cx.simulate_mouse_down(first, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(second, MouseButton::Left, Modifiers::default());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        ax(cx, *id, gpui::accesskit::Action::Focus);
        draw(cx);
        key(cx, "enter");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(cx.opened_url().as_deref(), Some("https://images.test"));
    }

    #[gpui::test]
    fn fenced_code_keeps_explicit_document_selection_owner(cx: &mut TestAppContext) {
        let (state, _, cx) = setup("```rust\nfenced_word\n```", true, false, None, cx);
        let bounds = state.read_with(cx, |state, _| state.selection_adapter.text_bounds()[0]);
        cx.simulate_event(gpui::MouseDownEvent {
            button: MouseButton::Left,
            position: bounds.origin + point(px(16.), px(8.)),
            click_count: 2,
            modifiers: Modifiers::default(),
            first_mouse: false,
        });
        draw(cx);
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_text()),
            "fenced_word"
        );
        cx.dispatch_action(crate::input::Copy);
        assert_eq!(
            cx.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            "fenced_word"
        );
    }

    struct RetirableRoot {
        state: Option<Entity<TextViewState>>,
        extensions: crate::text::MarkdownExtensions,
        calls: Arc<AtomicUsize>,
    }
    impl Render for RetirableRoot {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let root = div()
                .w(px(180.))
                .h(px(100.))
                .child(crate::TextSelectionLayer);
            if let Some(state) = &self.state {
                let calls = self.calls.clone();
                root.child(
                    crate::TextView::new(state)
                        .markdown_extensions(self.extensions.clone())
                        .on_link_activation(move |_, _, _, _| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            crate::text::LinkDecision::default()
                        }),
                )
            } else {
                root
            }
        }
    }

    #[gpui::test]
    fn retired_selectable_document_withdraws_links_and_releases_native_frame_custody(
        cx: &mut TestAppContext,
    ) {
        for source in [
            "[plain **styled** `code`](https://retired.test)",
            "[$probe$](https://retired.test)",
        ] {
            cx.with_paused_automatic_drawing(|cx| {
                cx.update(crate::init);
                let calls = Arc::new(AtomicUsize::new(0));
                let (root, cx) = cx.add_window_view(|_, cx| RetirableRoot {
                    state: Some(cx.new(|cx| TextViewState::markdown(source, cx))),
                    calls: calls.clone(),
                    extensions: crate::text::MarkdownExtensions::default().plugin(
                        crate::text::markdown_ext::TestInlinePlugin::new("math")
                            .parse_with(|node, _| {
                                matches!(node, markdown::mdast::Node::InlineMath(_)).then(|| {
                                    crate::text::MarkdownNode::new("math", ()).text("probe")
                                })
                            })
                            .render_with(|_, _, _, _| {
                                Some(crate::text::InlineElement::new(div().w(px(20.)).h(px(20.))))
                            }),
                    ),
                });
                let cx: &mut VisualTestContext = cx;
                cx.run_until_parked();
                draw(cx);
                enable_ax(cx);
                let (id, node) = tree_links(cx).remove(0);
                let owner = root.read_with(cx, |root, _| root.state.as_ref().unwrap().downgrade());
                root.update(cx, |root, cx| {
                    root.state
                        .as_ref()
                        .unwrap()
                        .update(cx, |state, cx| state.set_interaction_enabled(false, cx));
                    root.state.take();
                    cx.notify();
                });
                cx.run_until_parked();
                ax(cx, id, gpui::accesskit::Action::Click);
                let position = position(&node, cx);
                cx.simulate_click(position, Modifiers::default());
                assert_eq!(calls.load(Ordering::SeqCst), 0);
                // Original native Rebuild keeps an AnyView until its retained
                // frames are cleared; interaction retirement precedes that.
                draw(cx);
                draw(cx);
                cx.run_until_parked();
                assert!(
                    owner.upgrade().is_none(),
                    "native retired frame custody must drain"
                );
            });
        }
    }
    #[gpui::test]
    fn materialized_offscreen_anchor_accepts_ax_and_reveals_its_native_focus(
        cx: &mut TestAppContext,
    ) {
        let source = format!(
            "{}[last](https://offscreen.test)",
            "wrapping words ".repeat(100)
        );
        let (state, calls, cx) = setup(&source, false, true, None, cx);
        enable_ax(cx);
        let links = tree_links(cx);
        assert_eq!(
            links.len(),
            1,
            "the materialized paragraph keeps its complete AX anchor, including offscreen fragments"
        );
        let (id, node) = &links[0];
        let scale = cx.update(|window, _| window.scale_factor()) as f64;
        assert!(node.bounds().unwrap().y0 >= 100. * scale);
        ax(cx, *id, gpui::accesskit::Action::Click);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        ax(cx, *id, gpui::accesskit::Action::Focus);
        draw(cx);
        draw(cx);
        assert!(state.read_with(
            cx,
            |state, _| state.list_state.logical_scroll_top().offset_in_item > px(0.)
        ));
        let rect = tree_links(cx)[0].1.bounds().unwrap();
        assert!(
            rect.y0 >= 0. && rect.y1 <= 100. * scale,
            "AX focus reveals the actual native anchor: {rect:?}"
        );
    }
}
