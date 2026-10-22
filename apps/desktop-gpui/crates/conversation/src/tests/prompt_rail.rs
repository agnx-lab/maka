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

//! The prompt rail: a tick per prompt from three prompts on, sampled past
//! 64; the active tick following the reading position; a click scrolling
//! a prompt to the top, reading older pages first for a prompt only the
//! Session's index names; the index read only while older history lies
//! outside the transcript; the hover card; the room the reading column
//! keeps for it, and hidden below 560 px.

use super::*;
use host_protocol::SESSION_TURN_LANDMARK_MAX_ITEMS;

use crate::rows::RowKey;

/// The rows of finished Turn `turn` from sequence `first` (three rows): the
/// prompt `prompt`, a reply of `paragraphs` paragraphs, and the terminal
/// state.
fn turn_rows(turn: &str, first: u64, prompt: &str, paragraphs: usize) -> Vec<(u64, Value)> {
    let reply: String =
        (1..=paragraphs).map(|n| format!("Reply {turn} paragraph {n}.\n\n")).collect();
    vec![
        (first, json!({"type": "user", "id": turn, "turnId": turn, "ts": first, "text": prompt})),
        (
            first + 1,
            json!({"type": "assistant", "id": format!("{turn}-a"), "turnId": turn,
                   "ts": first + 1, "text": reply, "contentOrder": ["text"], "modelId": "m"}),
        ),
        (
            first + 2,
            json!({"type": "turn_state", "id": format!("{turn}-end"), "turnId": turn,
                   "ts": first + 2, "status": "completed"}),
        ),
    ]
}

/// Turns `t{from}` to `t{to}` (prompt "Prompt n"), Turn n from sequence
/// 3n + 1, each reply `paragraphs` paragraphs.
fn turns(from: usize, to: usize, paragraphs: usize) -> Vec<(u64, Value)> {
    (from..=to)
        .flat_map(|n| {
            turn_rows(&format!("t{n}"), 3 * n as u64 + 1, &format!("Prompt {n}"), paragraphs)
        })
        .collect()
}

/// The watermark every page of these sessions is read through.
const THROUGH: u64 = 10_000;

/// An `older` page of `session`: `rows` newest first on the wire, more
/// behind cursor `next`.
fn page_of(session: &str, rows: &[(u64, Value)], next: Option<&str>) -> Value {
    let mut raw = 0;
    let fragments: Vec<Value> = rows
        .iter()
        .rev()
        .map(|(sequence, row)| {
            let bytes = serde_json::to_vec(row).expect("row");
            raw += bytes.len();
            json!({
                "sequence": sequence, "byteOffset": 0, "totalBytes": bytes.len(),
                "payloadDigest": null,
                "data": base64::engine::general_purpose::STANDARD.encode(&bytes)
            })
        })
        .collect();
    json!({
        "kind": "page", "sessionId": session, "direction": "older", "throughSequence": THROUGH,
        "rawBytes": raw, "fragments": fragments, "nextCursor": next, "endsAtTurnBoundary": true
    })
}

/// `subscription.open` of `session` whose tail holds `rows`, older history
/// behind cursor `older` when given.
fn open_of(session: &str, rows: &[(u64, Value)], older: Option<&str>) -> Value {
    let mut open = open_result_for(session, &format!("sub-{session}"));
    open["transcript"]["durable"] = page_of(session, rows, older);
    open
}

/// A landmark of Turn `t{n}` (as [`turns`] numbers them), labelled `label`.
fn landmark(n: u64, label: &str) -> Value {
    json!({"turnId": format!("t{n}"), "sequence": 3 * n + 1, "lastSequence": 3 * n + 3,
           "label": label})
}

fn index_of(session: &str, landmarks: Vec<Value>) -> Value {
    json!({"sessionId": session, "throughSequence": THROUGH, "landmarks": landmarks})
}

/// A harness whose view shows the prompt rail, on session `s1` opened
/// from `open`.
fn open_with_rail(transport: Arc<ScriptedHost>, open: Value, cx: &mut TestAppContext) -> Harness {
    transport.reply("subscription.open", Ok(open));
    let harness = Harness::open(transport, EPOCH, cx);
    harness.view.update(cx, |view, cx| view.set_prompt_rail(true, cx));
    harness.select(SESSION, cx);
    harness
}

/// Draws twice: the rail takes what a frame showed in the next.
fn draw(harness: &Harness, cx: &mut TestAppContext) {
    harness.with_window(cx, |_, _| {});
    harness.with_window(cx, |_, _| {});
}

fn ticks(harness: &Harness, cx: &mut TestAppContext) -> Vec<String> {
    draw(harness, cx);
    harness
        .view
        .read_with(cx, |view, _| view.prompt_rail_ticks().iter().map(ToString::to_string).collect())
}

fn active(harness: &Harness, cx: &mut TestAppContext) -> Option<String> {
    draw(harness, cx);
    harness.view.read_with(cx, |view, _| view.prompt_rail_active().map(|id| id.to_string()))
}

fn tick_id(turn: &str) -> ElementId {
    shared::domain_element_id("prompt-rail-tick", turn)
}

fn user_row(turn: &str) -> ElementId {
    item_element_id(turn, &ItemKey::User(turn.into()))
}

fn rail_shows(harness: &Harness, cx: &mut TestAppContext) -> bool {
    draw(harness, cx);
    harness.with_window(cx, |window, _| window.try_find("prompt-rail").is_some())
}

/// Scrolls the list so that row `key` is at the top.
fn scroll_to(harness: &Harness, key: RowKey, cx: &mut TestAppContext) {
    harness.view.update(cx, |view, cx| {
        let ix = view.rows().iter().position(|row| row.key == key).expect("a row");
        view.scroller().update(cx, |scroller, cx| scroller.scroll_to_item(ix, cx));
    });
}

#[gpui_kit::test]
fn two_prompts_show_no_rail_and_three_show_a_tick_each(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    transport.reply("subscription.open", Ok(open_of("s0", &turns(1, 2, 1), None)));
    transport.reply("subscription.open", Ok(open_of(SESSION, &turns(1, 3, 1), None)));
    let harness = Harness::open(transport, EPOCH, cx);
    harness.view.update(cx, |view, cx| view.set_prompt_rail(true, cx));
    harness.select("s0", cx);
    assert_eq!(ticks(&harness, cx), Vec::<String>::new());
    assert!(!rail_shows(&harness, cx), "two prompts are nothing to jump between");

    harness.select(SESSION, cx);
    assert_eq!(ticks(&harness, cx), ["t1", "t2", "t3"]);
    harness.with_window(cx, |window, _| {
        let rail = window.find("prompt-rail");
        assert_eq!(rail.label(), Some(copy::PROMPT_RAIL.en()));
        assert_eq!(rail.role(), Some(gpui_kit::Role::Navigation));
        for n in 1..=3 {
            let tick = window.find(tick_id(&format!("t{n}")));
            assert_eq!(tick.role(), Some(gpui_kit::Role::Button));
            assert_eq!(tick.label(), Some(format!("Jump to prompt: Prompt {n}").as_str()));
        }
        // On the right edge, clear of the scrollbar's 16 px track.
        let region = window.find("conversation-transcript").bounds();
        let tick = window.find(tick_id("t1")).bounds();
        assert!(region.right() - tick.right() >= px(16.), "{region:?} {tick:?}");
        assert!(tick.left() > region.center().x);
    });
    assert!(
        harness.transport.requests("session.turn_landmarks.query").is_empty(),
        "the tail holds every Turn"
    );
}

#[gpui_kit::test]
fn a_hundred_prompts_show_sixty_four_ticks_from_the_first_to_the_last(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    let harness = open_with_rail(transport, open_of(SESSION, &turns(0, 99, 1), None), cx);
    let shown = ticks(&harness, cx);
    assert_eq!(shown.len(), 64);
    assert_eq!((shown[0].as_str(), shown[63].as_str()), ("t0", "t99"));
    harness.with_window(cx, |window, _| {
        assert!(window.find(tick_id("t0")).bounds().size.height > px(0.));
        assert!(window.try_find(tick_id("t1")).is_none(), "t1 is not sampled");
    });
    // Following the tail, the reader is on the newest prompt.
    assert_eq!(active(&harness, cx).as_deref(), Some("t99"));

    // Too short for every tick, the rail scrolls on its own to keep the
    // active one in view; the transcript stays where it is.
    cx.simulate_window_resize(harness.window.into(), size(px(900.), px(400.)));
    draw(&harness, cx);
    harness.with_window(cx, |window, _| {
        let rail = window.find("prompt-rail").bounds();
        let last = window.find(tick_id("t99")).bounds();
        let first = window.find(tick_id("t0")).bounds();
        assert!(last.bottom() <= rail.bottom() && last.top() >= rail.top(), "{rail:?} {last:?}");
        assert!(first.top() < rail.top(), "the first ticks scrolled out: {first:?}");
    });
    assert!(following_tail(&harness, cx));
}

#[gpui_kit::test]
fn the_active_tick_follows_the_prompt_being_read(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    let harness = open_with_rail(transport, open_of(SESSION, &turns(1, 5, 40), None), cx);
    assert!(following_tail(&harness, cx));
    assert_eq!(active(&harness, cx).as_deref(), Some("t5"), "the newest, following the tail");

    // Scrolled into Turn 2's reply: Turn 2 is at the top.
    scroll_to(
        &harness,
        RowKey::Item { turn_id: "t2".into(), key: ItemKey::Text("t2-a".into()) },
        cx,
    );
    assert_eq!(active(&harness, cx).as_deref(), Some("t2"));

    // Turn 3's footer on the top line is still Turn 3.
    scroll_to(&harness, RowKey::Footer { turn_id: "t3".into() }, cx);
    assert_eq!(active(&harness, cx).as_deref(), Some("t3"));

    // Back at the end, the newest again.
    harness.with_window(cx, |window, cx| {
        window.focus_next(cx);
        window.press("end", cx);
    });
    assert!(following_tail(&harness, cx));
    assert_eq!(active(&harness, cx).as_deref(), Some("t5"));
}

#[gpui_kit::test]
fn a_click_scrolls_the_prompt_to_the_top(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    let harness = open_with_rail(transport, open_of(SESSION, &turns(1, 5, 40), None), cx);
    draw(&harness, cx);
    harness.with_window(cx, |window, cx| window.click(tick_id("t2"), cx));
    let (top_line, _) = transcript_geometry(&harness, cx);
    assert_near(row_top(&harness, user_row("t2"), cx), top_line, "Turn 2's prompt at the top");
    assert!(!following_tail(&harness, cx));
    assert_eq!(active(&harness, cx).as_deref(), Some("t2"));

    // From the keyboard: Enter on a focused tick.
    harness.with_window(cx, |window, cx| {
        for _ in 0..200 {
            if window.find(tick_id("t4")).focused() == Some(true) {
                break;
            }
            window.focus_next(cx);
            window.render_frame(cx);
        }
        assert_eq!(window.find(tick_id("t4")).focused(), Some(true), "a Tab stop");
        window.press("enter", cx);
    });
    assert_near(row_top(&harness, user_row("t4"), cx), top_line, "Turn 4's prompt at the top");
    assert_eq!(active(&harness, cx).as_deref(), Some("t4"));
    assert!(
        harness.transport.requests("session.transcript.page").is_empty(),
        "nothing older was read"
    );
}

#[gpui_kit::test]
fn the_index_is_read_only_while_older_history_lies_outside_the_transcript(cx: &mut TestAppContext) {
    // Without the rail, nothing asks for the index.
    let transport = Arc::new(ScriptedHost::default());
    transport.reply("subscription.open", Ok(open_of(SESSION, &turns(4, 5, 40), Some("c1"))));
    let harness = Harness::open(transport, EPOCH, cx);
    harness.select(SESSION, cx);
    assert!(harness.transport.requests("session.turn_landmarks.query").is_empty());

    // With it, once for the transcript, and its Turns get ticks before
    // they are read.
    let transport = Arc::new(ScriptedHost::default());
    let index = (1..=5).map(|n| landmark(n, &format!("Prompt {n}"))).collect();
    transport.reply("session.turn_landmarks.query", Ok(index_of(SESSION, index)));
    let harness = open_with_rail(transport, open_of(SESSION, &turns(4, 5, 40), Some("c1")), cx);
    assert_eq!(
        harness.transport.requests("session.turn_landmarks.query"),
        [json!({"sessionId": SESSION, "maxLandmarks": SESSION_TURN_LANDMARK_MAX_ITEMS,
                "turnId": null})]
    );
    assert_eq!(ticks(&harness, cx), ["t1", "t2", "t3", "t4", "t5"]);

    // Once the transcript holds the first row, the index goes; the held
    // Turns keep their ticks, and nothing asks again.
    harness.transport.reply("session.transcript.page", Ok(page_of(SESSION, &turns(1, 3, 1), None)));
    harness.state.update(cx, |state, cx| state.load_older_history(cx));
    settle(cx);
    assert!(harness.state.read_with(cx, |state, _| state.turn_index().is_empty()));
    assert_eq!(ticks(&harness, cx), ["t1", "t2", "t3", "t4", "t5"]);
    assert_eq!(harness.transport.requests("session.turn_landmarks.query").len(), 1);

    // A session whose tail holds its whole history never asks.
    harness.transport.reply("subscription.open", Ok(open_of("s2", &turns(1, 3, 1), None)));
    harness.select("s2", cx);
    assert_eq!(ticks(&harness, cx), ["t1", "t2", "t3"]);
    assert_eq!(harness.transport.requests("session.turn_landmarks.query").len(), 1);
}

#[gpui_kit::test]
fn a_click_on_a_prompt_not_held_reads_back_to_it_then_scrolls(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    let index = (1..=5).map(|n| landmark(n, &format!("Prompt {n}"))).collect();
    transport.reply("session.turn_landmarks.query", Ok(index_of(SESSION, index)));
    let harness = open_with_rail(transport, open_of(SESSION, &turns(4, 5, 40), Some("c1")), cx);
    assert_eq!(ticks(&harness, cx), ["t1", "t2", "t3", "t4", "t5"]);
    assert!(harness.transport.requests("session.transcript.page").is_empty());

    // Turn 1 is two pages back.
    let nearer = harness.transport.hold("session.transcript.page");
    let oldest = harness.transport.hold("session.transcript.page");
    harness.with_window(cx, |window, cx| window.click(tick_id("t1"), cx));
    settle(cx);
    assert_eq!(harness.transport.requests("session.transcript.page").len(), 1);
    nearer.try_send(Ok(page_of(SESSION, &turns(2, 3, 40), Some("c2")))).expect("release");
    settle(cx);
    let reads = harness.transport.requests("session.transcript.page");
    assert_eq!(reads.len(), 2, "Turn 1 is still older: the next page is read");
    assert_eq!(reads[1]["cursor"], "c2");
    oldest.try_send(Ok(page_of(SESSION, &turns(1, 1, 40), None))).expect("release");
    settle(cx);

    let (top_line, _) = transcript_geometry(&harness, cx);
    assert_near(row_top(&harness, user_row("t1"), cx), top_line, "Turn 1's prompt at the top");
    assert_eq!(active(&harness, cx).as_deref(), Some("t1"));
    assert_eq!(harness.transport.requests("session.transcript.page").len(), 2);
}

#[gpui_kit::test]
fn a_tick_shows_its_prompt_and_the_start_of_its_reply(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    // Turn 6's reply: whitespace runs and more than 140 characters.
    let long_reply = format!("First   line\n\n{}", "word ".repeat(60));
    let mut rows = turns(4, 5, 40);
    rows.extend(turn_rows("t6", 3 * 6 + 1, "  Ship\n the  rail  ", 0));
    let reply = rows.len() - 2;
    rows[reply].1["text"] = json!(long_reply);
    // Turn 1 is known only from the index, with no label.
    let index = vec![landmark(1, ""), landmark(4, "Prompt 4"), landmark(5, "Prompt 5")];
    transport.reply("session.turn_landmarks.query", Ok(index_of(SESSION, index)));
    let harness = open_with_rail(transport, open_of(SESSION, &rows, Some("c1")), cx);
    assert_eq!(ticks(&harness, cx), ["t1", "t4", "t5", "t6"]);

    let card = |harness: &Harness, turn: &str, cx: &mut TestAppContext| {
        harness.with_window(cx, |window, cx| window.hover(tick_id(turn), cx));
        cx.executor().advance_clock(Duration::from_millis(120));
        draw(harness, cx);
        harness.with_window(cx, |window, _| {
            let card = window.within(shared::domain_element_id("prompt-rail-card", turn));
            let prompt = card.find("prompt-rail-card-prompt").label().map(str::to_owned);
            let reply = card
                .try_find("prompt-rail-card-reply")
                .and_then(|reply| reply.label().map(str::to_owned));
            (prompt, reply)
        })
    };
    let (prompt, reply) = card(&harness, "t6", cx);
    assert_eq!(prompt.as_deref(), Some("Ship the rail"));
    let expected: String = format!("First line {}", "word ".repeat(60)).chars().take(140).collect();
    assert_eq!(reply.as_deref(), Some(expected.as_str()));
    assert_eq!(expected.chars().count(), 140);
    harness.with_window(cx, |window, _| {
        assert_eq!(window.find(tick_id("t6")).label(), Some("Jump to prompt: Ship the rail"));
        // The card opens toward the conversation.
        let card = window
            .within(shared::domain_element_id("prompt-rail-card", "t6"))
            .find("prompt-rail-card")
            .bounds();
        assert!(card.right() <= window.find(tick_id("t6")).bounds().left() + px(1.));
    });

    let (prompt, reply) = card(&harness, "t1", cx);
    assert_eq!(prompt.as_deref(), Some(copy::EMPTY_PROMPT.en()));
    assert_eq!(reply, None, "a Turn not held has no reply to show");
    harness.with_window(cx, |window, _| {
        assert_eq!(window.find(tick_id("t1")).label(), Some("Jump to prompt: (empty prompt)"));
    });
}

/// Resizes the window so that the transcript, inset 5 px either side of
/// it, is `width` wide.
fn transcript_width(harness: &Harness, width: f32, cx: &mut TestAppContext) {
    cx.simulate_window_resize(harness.window.into(), size(px(width + 10.), px(800.)));
    draw(harness, cx);
    let region =
        harness.with_window(cx, |window, _| window.find("conversation-transcript").bounds());
    assert_near(region.size.width - px(2.), px(width), "the transcript inside its border");
}

/// Turn `turn`'s prompt row, which fills the reading column: its gaps from
/// the transcript's left and right edges inside its border, and its width.
fn column(harness: &Harness, turn: &str, cx: &mut TestAppContext) -> (Pixels, Pixels, Pixels) {
    draw(harness, cx);
    harness.with_window(cx, |window, _| {
        let region = window.find("conversation-transcript").bounds();
        let row = window.find(user_row(turn)).bounds();
        (row.left() - region.left() - px(1.), region.right() - px(1.) - row.right(), row.size.width)
    })
}

#[gpui_kit::test]
fn the_column_keeps_the_rails_room_before_the_rail_shows(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    transport.reply("subscription.open", Ok(open_of("s0", &turns(1, 2, 1), None)));
    transport.reply("subscription.open", Ok(open_of(SESSION, &turns(1, 3, 1), None)));
    let harness = Harness::open(transport, EPOCH, cx);
    harness.view.update(cx, |view, cx| view.set_prompt_rail(true, cx));
    // A 1440 px window with the sidebar and the workbar open leaves a
    // transcript about this wide: too narrow to centre the 720 px column
    // with the rail's 53 px beside it.
    harness.select("s0", cx);
    transcript_width(&harness, 700., cx);
    let (left, right, width) = column(&harness, "t1", cx);
    assert!(!rail_shows(&harness, cx), "two prompts show no rail");
    assert_near(left, px(24.), "the left gutter");
    assert_near(right, px(53.), "the rail's room, kept with two prompts");
    assert_near(width, px(623.), "the column between them");

    // The third prompt brings the rail into that room; the column stays.
    harness.select(SESSION, cx);
    assert!(rail_shows(&harness, cx), "three prompts show the rail");
    assert_eq!(column(&harness, "t1", cx), (left, right, width));
    harness.with_window(cx, |window, _| {
        let row = window.find(user_row("t1")).bounds();
        let rail = window.find("prompt-rail").bounds();
        assert!(rail.left() >= row.right(), "the rail beside the column: {rail:?} {row:?}");
    });
}

#[gpui_kit::test]
fn a_wide_transcript_centres_the_column_as_before(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    let harness = open_with_rail(transport, open_of(SESSION, &turns(1, 3, 1), None), cx);
    transcript_width(&harness, 900., cx);
    assert!(rail_shows(&harness, cx));
    let (left, right, width) = column(&harness, "t1", cx);
    assert_near(width, px(720.), "the column's widest");
    assert_near(left, px(90.), "centred");
    assert_near(right, px(90.), "centred");
    // From 826 px the column centres with the rail's room beside it.
    transcript_width(&harness, 826., cx);
    let (left, right, _) = column(&harness, "t1", cx);
    assert_near(left, px(53.), "centred at 826");
    assert_near(right, px(53.), "centred at 826");
}

#[gpui_kit::test]
fn below_560_px_the_rail_hides_and_the_gutters_are_even(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    let harness = open_with_rail(transport, open_of(SESSION, &turns(1, 3, 1), None), cx);
    transcript_width(&harness, 560., cx);
    assert!(rail_shows(&harness, cx), "560 px keeps the rail's room");
    let (left, right, _) = column(&harness, "t1", cx);
    assert_near(left, px(24.), "the left gutter");
    assert_near(right, px(53.), "the rail's room");

    transcript_width(&harness, 540., cx);
    assert!(!rail_shows(&harness, cx));
    assert!(harness.view.read_with(cx, |view, _| view.prompt_rail_ticks().is_empty()));
    let (left, right, width) = column(&harness, "t1", cx);
    assert_near(left, px(24.), "even gutters");
    assert_near(right, px(24.), "even gutters");
    assert_near(width, px(492.), "the column between them");
}

#[gpui_kit::test]
fn the_composer_keeps_the_transcripts_column(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    transport.reply("subscription.open", Ok(open_of(SESSION, &turns(1, 3, 1), None)));
    let harness = Harness::open_with_composer(transport, EPOCH, cx);
    harness.view.update(cx, |view, cx| view.set_prompt_rail(true, cx));
    harness.select(SESSION, cx);
    for width in [700., 900., 540.] {
        transcript_width(&harness, width, cx);
        harness.with_window(cx, |window, _| {
            let row = window.find(user_row("t1")).bounds();
            let dock = window.find("composer").bounds();
            assert_near(dock.left(), row.left(), &format!("the left edges at {width}"));
            assert_near(dock.right(), row.right(), &format!("the right edges at {width}"));
        });
    }
}

/// How far the hovered rail's ticks sit from its right edge.
fn tick_inset(harness: &Harness, turn: &str, cx: &mut TestAppContext) -> Pixels {
    harness.with_window(cx, |window, _| {
        window.find("prompt-rail").bounds().right() - window.find(tick_id(turn)).bounds().right()
    })
}

#[gpui_kit::test]
fn the_hovered_rail_eases_inward(cx: &mut TestAppContext) {
    let transport = Arc::new(ScriptedHost::default());
    let harness = open_with_rail(transport, open_of(SESSION, &turns(1, 3, 1), None), cx);
    assert!(rail_shows(&harness, cx));
    assert_near(tick_inset(&harness, "t2", cx), px(4.), "at rest, the rail's padding");
    harness.with_window(cx, |window, cx| window.hover(tick_id("t2"), cx));
    draw(&harness, cx);
    cx.executor().advance_clock(Duration::from_millis(30));
    draw(&harness, cx);
    let moving = tick_inset(&harness, "t2", cx);
    assert!(moving > px(4.) && moving < px(7.), "on its way: {moving:?}");
    cx.executor().advance_clock(Duration::from_millis(200));
    draw(&harness, cx);
    assert_near(tick_inset(&harness, "t2", cx), px(7.), "settled 3 px further in");
}

#[gpui_kit::test]
fn without_motion_a_tick_is_drawn_where_it_ends(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let transport = Arc::new(ScriptedHost::default());
    let harness = open_with_rail(transport, open_of(SESSION, &turns(1, 3, 1), None), cx);
    assert!(rail_shows(&harness, cx));
    harness.with_window(cx, |window, cx| window.hover(tick_id("t2"), cx));
    draw(&harness, cx);
    // Hovered, the ticks have settled 3 px further in at once.
    assert_near(tick_inset(&harness, "t2", cx), px(7.), "the hovered rail's right padding");
}
