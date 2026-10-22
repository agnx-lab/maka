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

//! The prompt rail's model: one tick per prompt the person sent, as Maka
//! Desktop's prompt navigation rail has it
//! (`packages/ui/src/prompt-anchor-rail.tsx`). The view draws it on the
//! transcript's right edge (`view/prompt_rail.rs`); everything here is
//! pure, so the choices Desktop makes are tested on their own.
//!
//! A tick stands for a Turn whose first user message has text: the Turns
//! the transcript holds, and before them the Turns of the Session's index
//! (`session.turn_landmarks.query`) it does not hold yet. Desktop leaves
//! out a Turn whose prompt is empty (files only) in both places, so the
//! rail is the same before and after older history is read. Past
//! [`MAX_TICKS`] Turns the rail samples them evenly, the first and the
//! last always among them.

use gpui_kit::SharedString;
use host_protocol::SessionTurnLandmark;
use transcript_model::{TextItem, Transcript, TurnItem, TurnView};

/// The most ticks the rail shows: Desktop's `MAX_PROMPT_RAIL_TICKS`.
pub(crate) const MAX_TICKS: usize = 64;

/// The fewest prompts that show a rail: below this there is nothing to
/// jump between.
pub(crate) const MIN_TICKS: usize = 3;

/// How many ticks either side of the hovered one grow with it:
/// Desktop's `HOVER_FALLOFF_TICKS`.
pub(crate) const HOVER_FALLOFF_TICKS: usize = 3;

/// How much of a reply the card shows, and of a prompt: the card clamps
/// each to two lines of its 280 px, which hold less than this.
pub(crate) const PREVIEW_CHARS: usize = 140;

/// A bar's width at rest, and the step each tick nearer the hovered one
/// adds (14, 17, 20, 23 px); the active tick's bar is [`BAR_WIDTH`].
const BAR_REST: f32 = 14.;
const BAR_FALLOFF_STEP: f32 = 3.;

/// The widest bar, the active tick's: 26 px, 3 px tall.
pub(crate) const BAR_WIDTH: f32 = 26.;
pub(crate) const BAR_HEIGHT: f32 = 3.;

/// The rail's padding, and each tick's above and below its bar. Ticks touch
/// (no gap), so the pointer moving down the rail goes from one straight to
/// the next; the padding makes the 11 px pitch.
pub(crate) const RAIL_PADDING: f32 = 4.;
pub(crate) const TICK_PADDING: f32 = 4.;

/// How far the rail sits from the transcript's right edge: clear of the
/// list's scrollbar, whose 16 px track takes the pointer while it shows
/// (Desktop rests at 12 px, clear of macOS's overlay scrollbar).
pub(crate) const RAIL_INSET: f32 = 16.;

/// How far the ticks settle inward while the rail is hovered.
pub(crate) const HOVER_SHIFT: f32 = 3.;

/// One prompt on the rail.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RailEntry {
    pub(crate) turn_id: SharedString,
    /// The prompt as the card shows it: whitespace collapsed, at most
    /// [`PREVIEW_CHARS`]; empty only for a landmark without a label.
    pub(crate) prompt: SharedString,
    /// The start of the Turn's final reply, the same way; empty without
    /// one (and for a Turn the transcript does not hold).
    pub(crate) reply: SharedString,
    /// For a Turn the transcript does not hold: where it starts in the
    /// Session.
    pub(crate) sequence: Option<u64>,
}

/// The rail's prompts, oldest first: the index's Turns the transcript does
/// not hold, then the held Turns whose prompt has text (Desktop's
/// `mergePromptAnchorRailTurns`). A Turn counts as held once the
/// transcript holds its start; the Turn the tail cut is still the index's.
/// Entries that read the same as in `previous` keep its strings.
pub(crate) fn rail_entries(
    transcript: &Transcript,
    index: &[SessionTurnLandmark],
    previous: &[RailEntry],
) -> Vec<RailEntry> {
    let held = |turn: &TurnView| transcript.has_turn_start(&turn.turn_id);
    let older = index
        .iter()
        .filter(|landmark| !transcript.turn(&landmark.turn_id).is_some_and(held))
        .map(|landmark| RailEntry {
            turn_id: landmark.turn_id.clone().into(),
            prompt: preview(&landmark.label).into(),
            reply: SharedString::default(),
            sequence: Some(landmark.sequence),
        });
    let loaded = transcript.turns().iter().filter(|turn| held(turn)).filter_map(|turn| {
        let Some(TurnItem::User(user)) = turn.items.first() else { return None };
        let prompt = preview(user.text());
        if prompt.is_empty() {
            return None;
        }
        let reply = final_reply(turn).map(|text| preview(&text.text)).unwrap_or_default();
        Some(RailEntry {
            turn_id: turn.turn_id.clone().into(),
            prompt: prompt.into(),
            reply: reply.into(),
            sequence: None,
        })
    });
    let mut entries: Vec<RailEntry> = older.chain(loaded).collect();
    for entry in &mut entries {
        if let Some(prior) = previous.iter().find(|prior| prior.turn_id == entry.turn_id) {
            if prior.prompt == entry.prompt {
                entry.prompt = prior.prompt.clone();
            }
            if prior.reply == entry.reply {
                entry.reply = prior.reply.clone();
            }
            entry.turn_id = prior.turn_id.clone();
        }
    }
    entries
}

/// The Turn's final reply: the last item of its last stretch of work that
/// is not reasoning, when that is text. Desktop's `finalAssistantReplyText`
/// (`foldTimeline` in packages/ui/src/timeline-fold.ts): a user message
/// (steering) starts a new stretch and forgets the reply so far; an
/// interrupted text ends its stretch.
pub(crate) fn final_reply(turn: &TurnView) -> Option<&TextItem> {
    let mut reply = None;
    // The last item of the open stretch that is not reasoning; `None` while
    // the stretch is empty.
    let mut last: Option<Option<&TurnItem>> = None;
    fn flush<'a>(last: &mut Option<Option<&'a TurnItem>>, reply: &mut Option<&'a TextItem>) {
        if let Some(item) = last.take() {
            *reply = match item {
                Some(TurnItem::Text(text)) => Some(text),
                _ => None,
            };
        }
    }
    for item in &turn.items {
        match item {
            TurnItem::User(_) => {
                flush(&mut last, &mut reply);
                reply = None;
            }
            TurnItem::Text(text) if text.interrupted => {
                last = Some(Some(item));
                flush(&mut last, &mut reply);
            }
            TurnItem::Thinking(_) => {
                last = Some(last.flatten());
            }
            _ => last = Some(Some(item)),
        }
    }
    flush(&mut last, &mut reply);
    reply
}

/// `text` with every run of whitespace made one space, trimmed, and cut to
/// [`PREVIEW_CHARS`] characters: Desktop's
/// `reply.replace(/\s+/g, ' ').trim().slice(0, 140)`.
pub(crate) fn preview(text: &str) -> String {
    let mut out = String::new();
    let mut chars = 0;
    for word in text.split_whitespace() {
        if chars > 0 {
            if chars == PREVIEW_CHARS {
                break;
            }
            out.push(' ');
            chars += 1;
        }
        for ch in word.chars() {
            if chars == PREVIEW_CHARS {
                return out;
            }
            out.push(ch);
            chars += 1;
        }
    }
    out
}

/// The entries the rail shows a tick for, by index: all of them up to
/// [`MAX_TICKS`], past it `MAX_TICKS` spread evenly from the first to the
/// last, `round(i × (n − 1) / 63)`.
pub(crate) fn sample(count: usize) -> Vec<usize> {
    if count <= MAX_TICKS {
        return (0..count).collect();
    }
    (0..MAX_TICKS).map(|tick| round_div(tick * (count - 1), MAX_TICKS - 1)).collect()
}

/// `a / b` rounded half up.
fn round_div(a: usize, b: usize) -> usize {
    (2 * a + b) / (2 * b)
}

/// The active tick for the Turn being read (Desktop's
/// `selectPromptRailTick`): its own tick; past the cap, the tick the
/// sampling projects its place onto; for a Turn the rail does not list, or
/// no reading position, the `previous` tick while it is still on the rail.
pub(crate) fn select_tick(
    reading: Option<&str>,
    entries: &[RailEntry],
    ticks: &[usize],
    previous: Option<&str>,
) -> Option<usize> {
    let tick_of = |turn_id: &str| ticks.iter().position(|&ix| entries[ix].turn_id == turn_id);
    if let Some(reading) = reading {
        if let Some(tick) = tick_of(reading) {
            return Some(tick);
        }
        let at = entries.iter().position(|entry| entry.turn_id == reading);
        if let Some(at) = at
            && entries.len() > 1
            && ticks.len() > 1
        {
            return Some(round_div(at * (ticks.len() - 1), entries.len() - 1));
        }
    }
    previous.and_then(tick_of)
}

/// Tick `tick`'s bar width in spec pixels: [`BAR_WIDTH`] when active,
/// otherwise 14 at rest growing by 3 for each tick nearer the hovered one
/// within [`HOVER_FALLOFF_TICKS`] (17, 20, and 23 for the hovered one).
pub(crate) fn bar_width(tick: usize, hovered: Option<usize>, active: bool) -> f32 {
    if active {
        return BAR_WIDTH;
    }
    let distance = hovered
        .map_or(HOVER_FALLOFF_TICKS, |hovered| tick.abs_diff(hovered).min(HOVER_FALLOFF_TICKS));
    BAR_REST + (HOVER_FALLOFF_TICKS - distance) as f32 * BAR_FALLOFF_STEP
}

/// The room the rail takes from the transcript's right edge with the
/// pointer on it, in spec pixels: the inset, the hover shift, the padding
/// either side and the widest bar (53). The task's conversation keeps it
/// right of the reading column (`shared::layout::ColumnGutters::Rail`), so
/// a hovered rail never covers the text.
pub(crate) const HIT_WIDTH: f32 = RAIL_INSET + HOVER_SHIFT + 2. * RAIL_PADDING + BAR_WIDTH;
const _: () = assert!(HIT_WIDTH == shared::layout::RAIL_GUTTER_REMS * 16.);

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(count: usize) -> Vec<RailEntry> {
        (0..count)
            .map(|ix| RailEntry {
                turn_id: format!("t{ix}").into(),
                prompt: format!("prompt {ix}").into(),
                reply: SharedString::default(),
                sequence: None,
            })
            .collect()
    }

    #[test]
    fn up_to_the_cap_every_prompt_has_a_tick() {
        assert_eq!(sample(0), Vec::<usize>::new());
        assert_eq!(sample(3), [0, 1, 2]);
        assert_eq!(sample(64), (0..64).collect::<Vec<_>>());
    }

    #[test]
    fn past_the_cap_the_ticks_spread_from_the_first_to_the_last() {
        let ticks = sample(100);
        assert_eq!(ticks.len(), MAX_TICKS);
        assert_eq!((ticks[0], ticks[63]), (0, 99));
        assert!(ticks.windows(2).all(|pair| pair[0] < pair[1]), "{ticks:?}");
        // Math.round(i * 99 / 63), as Desktop picks them.
        let desktop: Vec<usize> =
            (0..64).map(|i| (i as f64 * 99. / 63.).round() as usize).collect();
        assert_eq!(ticks, desktop);
    }

    #[test]
    fn the_active_tick_is_the_reading_turn_or_its_projection() {
        let list = entries(5);
        let ticks = sample(5);
        assert_eq!(select_tick(Some("t3"), &list, &ticks, None), Some(3));
        // Unknown positions keep the previous tick while it is on the rail.
        assert_eq!(select_tick(None, &list, &ticks, Some("t1")), Some(1));
        assert_eq!(select_tick(Some("gone"), &list, &ticks, Some("t2")), Some(2));
        assert_eq!(select_tick(None, &list, &ticks, Some("gone")), None);

        // Past the cap a Turn without a tick lands on the tick its place
        // projects onto: entry 51 of 100 → round(51 × 63 / 99) = 32.
        let list = entries(100);
        let ticks = sample(100);
        assert!(!ticks.contains(&51));
        assert_eq!(select_tick(Some("t51"), &list, &ticks, None), Some(32));
        assert_eq!(select_tick(Some("t99"), &list, &ticks, None), Some(63));
    }

    #[test]
    fn the_bars_fall_off_around_the_hovered_tick() {
        let widths: Vec<f32> = (0..9).map(|tick| bar_width(tick, Some(4), false)).collect();
        assert_eq!(widths, [14., 14., 17., 20., 23., 20., 17., 14., 14.]);
    }

    #[test]
    fn at_rest_every_bar_is_short_and_the_active_one_is_widest() {
        assert_eq!(bar_width(0, None, false), 14.);
        assert_eq!(bar_width(4, Some(4), true), BAR_WIDTH);
        assert_eq!(bar_width(2, Some(4), true), BAR_WIDTH);
    }

    #[test]
    fn a_preview_collapses_whitespace_and_stops_at_140_characters() {
        assert_eq!(preview("  Fix\n\n the   build\t "), "Fix the build");
        assert_eq!(preview(" \n "), "");
        let long = "word ".repeat(100);
        let cut = preview(&long);
        assert_eq!(cut.chars().count(), PREVIEW_CHARS);
        assert_eq!(cut, long.trim().chars().take(PREVIEW_CHARS).collect::<String>());
        // Characters, not bytes.
        assert_eq!(preview(&"字".repeat(200)).chars().count(), PREVIEW_CHARS);
    }

    #[test]
    fn the_rail_keeps_its_hit_box_beside_the_column_from_a_560_px_transcript() {
        use shared::layout::{ColumnGutters, RAIL_GUTTER_REMS};
        assert_eq!(HIT_WIDTH, 53.);
        assert_eq!(RAIL_GUTTER_REMS * 16., HIT_WIDTH);
        let gutters = |width: f32| ColumnGutters::for_transcript(true, width / 16.);
        assert_eq!(gutters(559.), ColumnGutters::Even);
        assert_eq!(gutters(560.), ColumnGutters::Rail);
        assert_eq!(gutters(1400.), ColumnGutters::Rail);
        // A side chat keeps even gutters at any width.
        assert_eq!(ColumnGutters::for_transcript(false, 1400. / 16.), ColumnGutters::Even);
    }
}
