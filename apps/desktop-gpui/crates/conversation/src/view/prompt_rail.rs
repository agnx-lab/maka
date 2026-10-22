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

//! The prompt rail on the transcript's right edge: one tick per prompt the
//! person sent, the tick of the prompt being read drawn widest; hovering a
//! tick shows a card with the prompt and the start of its reply, and a
//! click scrolls that prompt to the top. Maka Desktop's prompt navigation
//! rail (`packages/ui/src/prompt-anchor-rail.tsx`,
//! `apps/desktop/src/renderer/styles/prompt-rail.css`); its model is
//! [`crate::prompt_rail`].
//!
//! The owner turns it on for a task's conversation
//! ([`ConversationView::set_prompt_rail`]); a side chat leaves it off, and
//! drafts and the landing view show no transcript. It shows from
//! [`MIN_TICKS`] prompts, in the room the conversation keeps right of its
//! reading column for it ([`ColumnGutters::Rail`]): from a transcript
//! 560 px wide, whatever the number of prompts, so nothing moves when the
//! third prompt brings the rail. The composer and the strip over it keep
//! the same column.
//!
//! The reading position: the kit's `MessageScroller` keeps its list state
//! private and reports no scroll position, so a probe painted after the
//! list reads the Turn on the list's top line from the rows' painted bounds
//! ([`crate::paging::Paging::reading_turn`]), with whether the list follows
//! its tail and the gutters the width it painted at keeps. Paint must
//! not change state, so a change is handed to the view after the frame,
//! which draws the new active tick in the next one, a frame late.
//!
//! A click on a Turn the transcript holds scrolls its first row to the top.
//! A Turn only the Session's index names is older than every Turn held:
//! older pages are read, as reaching the top reads them, until its start is
//! held, then it is scrolled to the top in the same way
//! ([`ConversationView::open_at_passage`] reads them the same way).
//!
//! Motion follows Desktop where the kit's motion helpers do it, and not at
//! all while the system reduces motion (they hold the end state then): the
//! bars ease to their width, the rail brightens and settles inward on
//! hover, and new ticks arrive from the edge, a whole rail cascading top
//! to bottom.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::base::motion::{
    Easing, Keyframe, Keyframes, SignedDuration, Timing, Transition, animate_keyframes, transition,
};
use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants as _};
use gpui_kit::component::hover_card::HoverCard;
use gpui_kit::component::{Selectable as _, h_flex, v_flex};
use gpui_kit::{
    Anchor, AnyElement, App, Context, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TestSupportExt as _, WeakEntity,
    Window, canvas, div, prelude::FluentBuilder as _,
};
use transcript_model::ItemKey;

use shared::copy::Locale;
use shared::copy::conversation as copy;
use shared::domain_element_id;
use shared::layout::ColumnGutters;
use shared::theme::ActiveMakaPalette as _;

use super::ConversationView;
use crate::prompt_rail::{
    BAR_HEIGHT, BAR_WIDTH, HOVER_SHIFT, MIN_TICKS, RAIL_INSET, RAIL_PADDING, RailEntry,
    TICK_PADDING, bar_width, rail_entries, sample, select_tick,
};
use crate::rows::RowKey;
use crate::state::{ConversationEvent, ConversationPhase, OlderHistory};
use crate::style::{SUPPORTING_SIZE, dp};

/// How long the pointer rests on a tick before its card opens: Desktop's
/// `PREVIEW_DELAY_MS`.
const PREVIEW_DELAY: Duration = Duration::from_millis(120);

/// The card's widest measure, and the gap between its two lines.
const CARD_MAX_WIDTH: f32 = 280.;
const CARD_GAP: f32 = 2.;
const CARD_LINE: f32 = 20.;

/// The rail's opacity at rest (Desktop's `--opacity-muted`); hovered it is
/// opaque.
const REST_OPACITY: f32 = 0.65;

/// The rail's distance from the transcript's top and bottom edges.
const RAIL_MARGIN: f32 = 8.;

/// Desktop's durations: the rail's opacity (`--duration-base`), the bars'
/// width and the hover shift (`--duration-emphasized`), a tick's arrival
/// (`--duration-large`), and the step and cap of the arrival cascade.
const OPACITY_DURATION: Duration = Duration::from_millis(150);
const WIDTH_DURATION: Duration = Duration::from_millis(180);
const ARRIVAL_DURATION: Duration = Duration::from_millis(280);
const ARRIVAL_STEP: Duration = Duration::from_millis(18);
const ARRIVAL_STEPS_MAX: u32 = 20;

/// How far right a tick starts its arrival.
const ARRIVAL_OFFSET: f32 = 8.;

/// Desktop's `--ease-out-strong`.
fn ease_out_strong() -> Easing {
    Easing::CubicBezier { x1: 0.16, y1: 1., x2: 0.3, y2: 1. }
}

/// The rail's state in the view.
#[derive(Default)]
pub(crate) struct Rail {
    /// The owner turned the rail on.
    enabled: bool,
    /// The prompts, oldest first.
    entries: Vec<RailEntry>,
    /// The entries with a tick, by index.
    ticks: Vec<usize>,
    /// The active tick's Turn, and the last Turn that had it.
    active: Option<SharedString>,
    previous: Option<SharedString>,
    /// What the probe saw in the last frame that changed it.
    seen: Seen,
    /// Shared with the probe: what it reported last.
    reported: Rc<RefCell<Option<Seen>>>,
    /// The pointer is on the rail, and on this tick.
    hovered: bool,
    hovered_tick: Option<SharedString>,
    /// Where the rail's own list is scrolled: past its height it scrolls,
    /// and the active tick is kept in view without moving the transcript.
    scroll: ScrollHandle,
    /// A click on a Turn the transcript does not hold yet.
    jump: Option<RailJump>,
}

/// What a painted frame says about the rail.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    /// The Turn on the list's top line.
    reading: Option<String>,
    /// The list follows its tail: the reader is on the newest Turn.
    following: bool,
    /// The gutters the transcript's width keeps: the rail's room, or too
    /// narrow for it.
    gutters: ColumnGutters,
    /// The transcript's height in whole pixels: when it changes, the rail's
    /// own view may have lost the active tick.
    height: u32,
}

impl Default for Seen {
    /// Before a frame is painted the transcript is taken to hold the rail's
    /// room, as most do, so the column does not move when it is measured.
    fn default() -> Self {
        Self { reading: None, following: false, gutters: ColumnGutters::Rail, height: 0 }
    }
}

/// A Turn of the Session's index waiting for the transcript to hold it.
struct RailJump {
    session_id: SharedString,
    turn_id: SharedString,
    /// Follows the state while the jump waits; dropped with it.
    _changes: Subscription,
}

/// What a waiting jump needs next.
#[derive(Debug, PartialEq)]
enum JumpStep {
    Wait,
    ReadOlder,
    Land(usize),
    Abandon,
}

impl ConversationView {
    /// Shows the prompt rail on the transcript's right edge: a task's
    /// conversation does, a side chat's does not. While it shows, the state
    /// reads the Session's index of every Turn, so prompts the transcript
    /// does not hold yet have ticks too.
    pub fn set_prompt_rail(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.rail.enabled == on {
            return;
        }
        self.rail.enabled = on;
        self.state.update(cx, |state, cx| state.set_turn_index_wanted(on, cx));
        self.sync_rail(false, cx);
        cx.notify();
    }

    /// The Turns the rail shows a tick for, oldest first; empty while it
    /// does not show.
    #[cfg(test)]
    pub(crate) fn prompt_rail_ticks(&self) -> Vec<SharedString> {
        if !self.rail_shown() {
            return Vec::new();
        }
        self.rail.ticks.iter().map(|&ix| self.rail.entries[ix].turn_id.clone()).collect()
    }

    /// The active tick's Turn.
    #[cfg(test)]
    pub(crate) fn prompt_rail_active(&self) -> Option<SharedString> {
        self.rail.active.clone()
    }

    fn rail_shown(&self) -> bool {
        self.column_gutters() == ColumnGutters::Rail && self.rail.ticks.len() >= MIN_TICKS
    }

    /// The gutters of the reading column, which the rows and the status
    /// line take: the rail's room on the right while the rail is on and
    /// the transcript is wide enough for it (as the last frame measured),
    /// whether or not it shows; otherwise even.
    pub(crate) fn column_gutters(&self) -> ColumnGutters {
        if self.rail.enabled { self.rail.seen.gutters } else { ColumnGutters::Even }
    }

    /// Rebuilds the rail's prompts from the transcript and the index, after
    /// every commit.
    pub(super) fn sync_rail(&mut self, session_changed: bool, cx: &mut Context<Self>) {
        if session_changed {
            self.rail.active = None;
            self.rail.previous = None;
            self.rail.hovered_tick = None;
            self.rail.seen.reading = None;
            self.rail.reported.replace(None);
        }
        if !self.rail.enabled {
            self.rail.entries.clear();
            self.rail.ticks.clear();
            self.rail.jump = None;
            return;
        }
        let state = self.state.read(cx);
        let entries = match state.transcript() {
            Some(transcript) => rail_entries(transcript, state.turn_index(), &self.rail.entries),
            None => Vec::new(),
        };
        self.rail.ticks = sample(entries.len());
        self.rail.entries = entries;
        self.update_rail_active();
    }

    /// Picks the active tick for the reading position; keeps it in the
    /// rail's own view when it moves (scrolling the rail only, never the
    /// transcript, as Desktop's `keepActivePromptRailTickVisible`).
    fn update_rail_active(&mut self) {
        let rail = &mut self.rail;
        let reading = if rail.seen.following {
            rail.entries.last().map(|entry| entry.turn_id.as_ref())
        } else {
            rail.seen.reading.as_deref()
        };
        let tick = select_tick(reading, &rail.entries, &rail.ticks, rail.previous.as_deref());
        let active = tick.map(|tick| rail.entries[rail.ticks[tick]].turn_id.clone());
        if active == rail.active {
            return;
        }
        if let Some(tick) = tick {
            rail.scroll.scroll_to_item(tick);
            rail.previous = active.clone();
        }
        rail.active = active;
    }

    /// Takes what a painted frame saw.
    fn observe_rail(&mut self, seen: Seen, cx: &mut Context<Self>) {
        if self.rail.seen == seen {
            return;
        }
        let resized = self.rail.seen.height != seen.height;
        if seen.gutters != ColumnGutters::Rail {
            // Hidden under the pointer: no hover to keep.
            self.rail.hovered = false;
            self.rail.hovered_tick = None;
        }
        self.rail.seen = seen;
        self.update_rail_active();
        if resized
            && let Some(active) = &self.rail.active
            && let Some(tick) =
                self.rail.ticks.iter().position(|&ix| self.rail.entries[ix].turn_id == *active)
        {
            self.rail.scroll.scroll_to_item(tick);
        }
        cx.notify();
    }

    fn hover_rail(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.rail.hovered == hovered {
            return;
        }
        self.rail.hovered = hovered;
        if !hovered {
            self.rail.hovered_tick = None;
        }
        cx.notify();
    }

    /// The pointer entered (`hovered`) or left tick `turn_id`. Ticks touch,
    /// so leaving one for the next may arrive after entering it.
    fn hover_rail_tick(&mut self, turn_id: &SharedString, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.rail.hovered_tick = Some(turn_id.clone());
        } else if self.rail.hovered_tick.as_ref() == Some(turn_id) {
            self.rail.hovered_tick = None;
        }
        cx.notify();
    }

    /// Scrolls Turn `turn_id`'s prompt to the top, reading older pages
    /// first when the transcript does not hold it yet.
    fn jump_to_prompt(&mut self, turn_id: SharedString, cx: &mut Context<Self>) {
        self.rail.jump = None;
        if let Some(ix) = self.prompt_row(&turn_id, cx) {
            self.land_on_prompt(ix, cx);
            return;
        }
        let Some(session_id) = self.state.read(cx).session_id().cloned() else { return };
        let changes = cx.subscribe(&self.state, |this, _, event: &ConversationEvent, cx| {
            let ConversationEvent::Changed { .. } = event;
            this.advance_rail_jump(cx);
        });
        self.rail.jump = Some(RailJump { session_id, turn_id, _changes: changes });
        self.advance_rail_jump(cx);
    }

    fn advance_rail_jump(&mut self, cx: &mut Context<Self>) {
        let Some(jump) = &self.rail.jump else { return };
        match self.rail_jump_step(jump, cx) {
            JumpStep::Wait => {}
            JumpStep::ReadOlder => self.state.update(cx, |state, cx| state.load_older_history(cx)),
            JumpStep::Abandon => self.rail.jump = None,
            JumpStep::Land(ix) => {
                self.rail.jump = None;
                self.land_on_prompt(ix, cx);
            }
        }
    }

    fn rail_jump_step(&self, jump: &RailJump, cx: &App) -> JumpStep {
        let state = self.state.read(cx);
        if state.session_id() != Some(&jump.session_id) {
            return JumpStep::Abandon;
        }
        match state.phase() {
            ConversationPhase::Live => {}
            ConversationPhase::WaitingForHost | ConversationPhase::Opening => {
                return JumpStep::Wait;
            }
            _ => return JumpStep::Abandon,
        }
        if let Some(ix) = self.prompt_row(&jump.turn_id, cx) {
            return JumpStep::Land(ix);
        }
        match state.older_history() {
            OlderHistory::Available => JumpStep::ReadOlder,
            OlderHistory::Loading => JumpStep::Wait,
            // Held to the first row, or nothing more can be read.
            OlderHistory::None | OlderHistory::Reached | OlderHistory::Failed(_) => {
                JumpStep::Abandon
            }
        }
    }

    /// The row of Turn `turn_id`'s prompt, once the transcript holds the
    /// Turn's start (before that, its first user message may be steering).
    fn prompt_row(&self, turn_id: &str, cx: &App) -> Option<usize> {
        let transcript = self.state.read(cx).transcript()?;
        if transcript.turn(turn_id).is_none() || !transcript.has_turn_start(turn_id) {
            return None;
        }
        self.rows.iter().position(|row| {
            matches!(&row.key, RowKey::Item { turn_id: id, key: ItemKey::User(_) } if id == turn_id)
        })
    }

    fn land_on_prompt(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.paging.cancel();
        self.scroller.update(cx, |scroller, cx| scroller.scroll_to_item(ix, cx));
        cx.notify();
    }

    /// An empty element over the list that, once the list has painted,
    /// reports what the rail needs from the frame. Place it after the list.
    pub(super) fn rail_probe(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.rail.enabled {
            return None;
        }
        let paging = self.paging.clone();
        let scroller = self.scroller.clone();
        let reported = self.rail.reported.clone();
        let view = cx.weak_entity();
        let probe = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                // The transcript's width in rems.
                let width = f32::from(bounds.size.width) / f32::from(window.rem_size());
                let seen = Seen {
                    reading: paging.reading_turn(),
                    following: scroller.read(cx).is_following_tail(),
                    gutters: ColumnGutters::for_transcript(true, width),
                    height: f32::from(bounds.size.height).round().max(0.) as u32,
                };
                if reported.borrow().as_ref() == Some(&seen) {
                    return;
                }
                reported.replace(Some(seen.clone()));
                let view = view.clone();
                // Paint must not change state; the view takes it after
                // this frame.
                cx.defer(move |cx| {
                    view.update(cx, |view, cx| view.observe_rail(seen, cx)).ok();
                });
            },
        )
        .absolute()
        .inset_0();
        Some(probe.into_any_element())
    }

    /// The rail, when it shows.
    pub(super) fn render_prompt_rail(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.rail_shown() {
            return None;
        }
        let rail = &self.rail;
        let hovered = rail.hovered_tick.as_ref().and_then(|turn_id| {
            rail.ticks.iter().position(|&ix| rail.entries[ix].turn_id == *turn_id)
        });
        let ticks = rail
            .ticks
            .iter()
            .enumerate()
            .map(|(tick, &ix)| {
                let entry = &rail.entries[ix];
                let active = rail.active.as_ref() == Some(&entry.turn_id);
                TickView {
                    turn_id: entry.turn_id.clone(),
                    prompt: entry.prompt.clone(),
                    reply: entry.reply.clone(),
                    active,
                    hovered: hovered == Some(tick),
                    width: bar_width(tick, hovered, active),
                }
            })
            .collect();
        Some(
            PromptRail {
                ticks,
                hovered: rail.hovered,
                scroll: rail.scroll.clone(),
                view: cx.weak_entity(),
            }
            .into_any_element(),
        )
    }
}

/// One tick as drawn.
struct TickView {
    turn_id: SharedString,
    prompt: SharedString,
    reply: SharedString,
    active: bool,
    hovered: bool,
    /// The bar's width in spec pixels.
    width: f32,
}

/// The rail over the transcript, centred on its right edge. An element of
/// its own, so its motion keeps keyed state as the kit's helpers want.
#[derive(IntoElement)]
struct PromptRail {
    ticks: Vec<TickView>,
    hovered: bool,
    scroll: ScrollHandle,
    view: WeakEntity<ConversationView>,
}

impl RenderOnce for PromptRail {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let opacity = transition(
            "prompt-rail-opacity",
            if self.hovered { 1. } else { REST_OPACITY },
            Transition::new(OPACITY_DURATION).easing(ease_out_strong()),
            window,
            cx,
        );
        let shift = transition(
            "prompt-rail-shift",
            if self.hovered { HOVER_SHIFT } else { 0. },
            Transition::new(WIDTH_DURATION).easing(ease_out_strong()),
            window,
            cx,
        );
        let ticks: Vec<AnyElement> = self
            .ticks
            .into_iter()
            .enumerate()
            .map(|(ix, tick)| render_tick(ix, tick, &self.view, window, cx))
            .collect();
        let view = self.view;
        // A cover without a hitbox, so the transcript under it keeps the
        // pointer everywhere but on the rail.
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_end()
            .justify_center()
            .pr(dp(RAIL_INSET))
            .py(dp(RAIL_MARGIN))
            .child(
                v_flex()
                    .id("prompt-rail")
                    .test_support()
                    .role(Role::Navigation)
                    .aria_label(copy::PROMPT_RAIL.get(cx))
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .max_h_full()
                    .min_h_0()
                    .items_end()
                    // The ticks settle inward on hover inside a box that
                    // grows on its left edge, so the pointer never leaves
                    // the rail because the rail moved.
                    .pl(dp(RAIL_PADDING))
                    .pr(dp(RAIL_PADDING + shift))
                    .py(dp(RAIL_PADDING))
                    .opacity(opacity)
                    .on_hover(move |hovered, _, cx| {
                        view.update(cx, |view, cx| view.hover_rail(*hovered, cx)).ok();
                    })
                    .children(ticks),
            )
    }
}

/// A tick: a Button the size of its slot holding its bar, inside the
/// HoverCard that shows its prompt.
fn render_tick(
    ix: usize,
    tick: TickView,
    view: &WeakEntity<ConversationView>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let maka = cx.maka();
    let width = transition(
        ("prompt-rail-bar", tick.turn_id.clone()),
        tick.width,
        Transition::new(WIDTH_DURATION).easing(ease_out_strong()),
        window,
        cx,
    );
    let arrived = arrival(ix, &tick.turn_id, window, cx);
    let color: Hsla = if tick.active || tick.hovered { maka.ink } else { maka.ink_muted };
    let prompt: SharedString = if tick.prompt.is_empty() {
        copy::EMPTY_PROMPT.get(cx).into()
    } else {
        tick.prompt.clone()
    };
    let label = copy::jump_to_prompt(Locale::current(cx), &prompt);
    let transparent = Hsla::transparent_black();
    let (hover_view, click_view) = (view.clone(), view.clone());
    let (hover_id, click_id) = (tick.turn_id.clone(), tick.turn_id.clone());
    let bar = div()
        .relative()
        .left(dp(ARRIVAL_OFFSET * (1. - arrived)))
        .opacity(arrived)
        .w(dp(width))
        .h(dp(BAR_HEIGHT))
        .rounded_full()
        .bg(color);
    let button = Button::new(domain_element_id("prompt-rail-tick", &tick.turn_id))
        .custom(
            ButtonCustomVariant::new(cx)
                .color(transparent)
                .foreground(color)
                .hover(transparent)
                .active(transparent)
                .shadow(false),
        )
        .w(dp(BAR_WIDTH))
        .h(dp(BAR_HEIGHT + 2. * TICK_PADDING))
        .p_0()
        .selected(tick.active)
        .accessibility_label(label)
        .on_hover(move |hovered, _, cx| {
            hover_view.update(cx, |view, cx| view.hover_rail_tick(&hover_id, *hovered, cx)).ok();
        })
        .on_click(move |_, _, cx| {
            let turn_id = click_id.clone();
            click_view.update(cx, |view, cx| view.jump_to_prompt(turn_id, cx)).ok();
        })
        .child(h_flex().w_full().justify_end().child(bar));
    let reply = tick.reply;
    HoverCard::new(domain_element_id("prompt-rail-card", &tick.turn_id))
        .anchor(Anchor::RightCenter)
        .open_delay(PREVIEW_DELAY)
        .trigger(button)
        .content(move |_, _, cx| render_card(prompt.clone(), reply.clone(), cx))
        .into_any_element()
}

/// How far tick `ix` has arrived, 0 to 1: it slides in from the edge as it
/// fades in, after the ticks above it (Desktop's `maka-prompt-rail-tick-in`,
/// 18 ms apart up to the 20th). Keyed by its Turn, so a rail that stays
/// does not replay it; one shown anew cascades again.
fn arrival(ix: usize, turn_id: &SharedString, window: &mut Window, cx: &mut App) -> f32 {
    let Ok(keyframes) = Keyframes::try_new([Keyframe::new(0., 0.), Keyframe::new(1., 1.)]) else {
        return 1.;
    };
    let steps = u32::try_from(ix).unwrap_or(u32::MAX).min(ARRIVAL_STEPS_MAX);
    let timing = Timing::new(ARRIVAL_DURATION)
        .delay(SignedDuration::positive(ARRIVAL_STEP * steps))
        .ease(ease_out_strong());
    animate_keyframes(("prompt-rail-tick-in", turn_id.clone()), &keyframes, timing, window, cx)
        .value
}

/// A tick's card: the prompt in the heading-5 tier, the start of the reply
/// muted, each at most two lines (Desktop's `maka-prompt-rail-preview`).
/// The card itself is the kit's popover surface.
fn render_card(prompt: SharedString, reply: SharedString, cx: &App) -> AnyElement {
    let maka = cx.maka();
    let line = |id: &'static str, text: SharedString| {
        div().id(id).test_support().aria_label(text.clone()).min_w_0().line_clamp(2).child(text)
    };
    v_flex()
        .id("prompt-rail-card")
        .test_support()
        .max_w(dp(CARD_MAX_WIDTH))
        .gap(dp(CARD_GAP))
        .text_size(dp(SUPPORTING_SIZE))
        .line_height(dp(CARD_LINE))
        .child(
            line("prompt-rail-card-prompt", prompt)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(maka.ink),
        )
        .when(!reply.is_empty(), |card| {
            card.child(line("prompt-rail-card-reply", reply).text_color(maka.ink_muted))
        })
        .into_any_element()
}
