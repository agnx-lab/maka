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

//! User-visible checkpoints and work measurements for the original product.
//! The companion benchmark-tui-parity.py runs this one fixture against two CLIs.

use super::*;

struct Measurement {
    started: Instant,
    visible_after: Option<Duration>,
    cpu: Option<u64>,
    bytes: u64,
}

impl Measurement {
    fn start(tui: &Pty) -> Self {
        Self {
            started: Instant::now(),
            visible_after: None,
            cpu: cpu_ticks(tui.child.id()),
            bytes: tui.output_bytes,
        }
    }

    fn mark_visible(&mut self) {
        self.visible_after = Some(self.started.elapsed());
    }

    fn finish(self, tui: &Pty, visible_after: Option<Duration>) -> Value {
        let (rss_bytes, peak_rss_bytes) = memory_bytes(tui.child.id());
        let cpu_ms = self
            .cpu
            .zip(cpu_ticks(tui.child.id()))
            .and_then(|(start, end)| {
                let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
                (hz > 0).then(|| end.saturating_sub(start) as f64 * 1000.0 / hz as f64)
            });
        json!({
            "latency_ms": visible_after.map(|duration| duration.as_secs_f64() * 1000.0),
            "observation_ms": self.started.elapsed().as_secs_f64() * 1000.0,
            "ui_cpu_ms": cpu_ms,
            "ansi_bytes": tui.output_bytes - self.bytes,
            "ui_rss_bytes": rss_bytes,
            "ui_peak_rss_bytes": peak_rss_bytes,
        })
    }
}

// Linux counts the UI process and its threads, excluding the separate Host and
// the PTY/model fixture. Other systems retain latency/bytes and report null CPU.
fn cpu_ticks(pid: u32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let fields: Vec<_> = text[text.rfind(')')? + 1..].split_whitespace().collect();
    Some(fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?)
}

// VmRSS is resident memory at this checkpoint. VmHWM is the process-wide peak
// since startup, not a peak scoped to this measurement's observation window.
#[cfg(target_os = "linux")]
fn memory_bytes(pid: u32) -> (Option<u64>, Option<u64>) {
    let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
        return (None, None);
    };
    let field = |name: &str| {
        let mut value = status
            .lines()
            .find_map(|line| line.strip_prefix(name))?
            .split_whitespace();
        let kilobytes = value.next()?.parse::<u64>().ok()?;
        (value.next()? == "kB").then_some(())?;
        kilobytes.checked_mul(1024)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

#[cfg(not(target_os = "linux"))]
fn memory_bytes(_pid: u32) -> (Option<u64>, Option<u64>) {
    (None, None)
}

fn drain(tui: &mut Pty) {
    // Keep first-visible latency separately, but charge follow-up observations
    // and redraws to this input's CPU/byte work through the final checkpoint.
    // Continuously drain stdout throughout.
    let until = Instant::now() + Duration::from_millis(200);
    let deadline = until + Duration::from_secs(5);
    while Instant::now() < until || !tui.frames.ready() {
        tui.read();
        assert!(Instant::now() < deadline, "complete frame did not arrive");
    }
    assert!(
        tui.frames.ready(),
        "checkpoint must end at a complete synchronized frame"
    );
}

fn checkpoint(tui: &mut Pty, name: &str, measurement: Measurement, work: Value) {
    record_checkpoint(tui, name, measurement, work, true);
}

fn record_checkpoint(
    tui: &mut Pty,
    name: &str,
    measurement: Measurement,
    work: Value,
    compare_frame: bool,
) {
    let visible_after = measurement
        .visible_after
        .unwrap_or_else(|| measurement.started.elapsed());
    record_observed_checkpoint(
        tui,
        name,
        measurement,
        work,
        compare_frame,
        Some(visible_after),
        Value::Null,
    );
}

fn record_observed_checkpoint(
    tui: &mut Pty,
    name: &str,
    measurement: Measurement,
    work: Value,
    compare_frame: bool,
    visible_after: Option<Duration>,
    observations: Value,
) {
    drain(tui);
    let metrics = measurement.finish(tui, visible_after);
    let snapshot = tui.screen.snapshot().unwrap();
    assert!(snapshot.alternate_screen);
    let record = json!({
        "checkpoint": name,
        "compare_frame": compare_frame,
        "metrics": metrics,
        "work": work,
        "observations": observations,
        "frame": {
            "screen": snapshot.screen,
            "size": snapshot.size,
            "cursor": snapshot.cursor,
            "input": snapshot.input,
        },
    });
    if let Some(directory) = std::env::var_os("MAKA_TUI_PARITY_EVIDENCE") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&record).unwrap(),
        )
        .unwrap();
        std::fs::write(directory.join(format!("{name}.ansi")), &tui.output).unwrap();
    }
    eprintln!(
        "MAKA_TUI_PARITY {}",
        serde_json::to_string(&record).unwrap()
    );
}

fn draft(bytes: usize) -> String {
    let line = "中文草稿，保持换行、光标与滚动位置。Latin text 与 emoji 🙂。\n";
    let mut text = line.repeat(bytes / line.len());
    text.push_str("草稿末尾");
    text
}

fn first_scroll_row(screen: &str) -> Option<(usize, String)> {
    screen.lines().enumerate().find_map(|(index, line)| {
        let at = line.find("Scroll row ")?;
        Some((index, line[at..at + 16].to_owned()))
    })
}

#[derive(Clone)]
struct ReadingMarker {
    order: (u8, usize, usize),
    row: usize,
    text: String,
}

impl ReadingMarker {
    fn evidence(&self) -> Value {
        json!({"screen_row": self.row, "marker": self.text})
    }

    fn moved_up_from(&self, before: &Self) -> bool {
        self.order < before.order || (self.order == before.order && self.row > before.row)
    }
}

fn first_reading_marker(screen: &str) -> Option<ReadingMarker> {
    screen.lines().enumerate().find_map(|(row, line)| {
        if let Some((_, tail)) = line.split_once("Scroll row ") {
            let (label, _) = tail.split_once(':')?;
            let (message, paragraph) = label.split_once('-')?;
            Some(ReadingMarker {
                order: (0, message.parse().ok()?, paragraph.parse().ok()?),
                row,
                text: format!("Scroll row {label}"),
            })
        } else {
            let (_, tail) = line.split_once("并发片段 ")?;
            let (label, _) = tail.split_once('：')?;
            Some(ReadingMarker {
                order: (1, label.parse().ok()?, 0),
                row,
                text: format!("并发片段 {label}"),
            })
        }
    })
}

fn scroll_return_visible(screen: &str) -> bool {
    screen.contains("Scroll to end") || screen.contains("Latest messages")
}

fn command_query_ready(screen: &str, query: &str) -> bool {
    screen.contains(&format!("│{query}"))
        && screen.contains(&format!("› {query}"))
        && !screen.contains("Searching conversations…")
}

fn turn_elapsed_visible(screen: &str) -> bool {
    let number = |field: &str, unit: char| {
        field.strip_suffix(unit).is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
    };
    screen.lines().any(|line| {
        let Some((_, content)) = line.split_once('│') else {
            return false;
        };
        let text = content.trim().trim_end_matches(['│', '┃']).trim();
        let mut fields = text.split_whitespace();
        match (fields.next(), fields.next(), fields.next(), fields.next()) {
            (Some(seconds), None, None, None) => number(seconds, 's'),
            (Some(minutes), Some(seconds), None, None) => {
                number(minutes, 'm') && number(seconds, 's')
            }
            (Some(hours), Some(minutes), Some(seconds), None) => {
                number(hours, 'h') && number(minutes, 'm') && number(seconds, 's')
            }
            _ => false,
        }
    })
}

#[test]
#[ignore = "real product PTY: named frame parity and input/scroll/stream responsiveness"]
fn default_product_preserves_frames_and_responsiveness() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 1, 0);
    let (_directory, ui) = large_history_fixture(Some(&base));
    let host_guard = HostRootGuard::new(&ui.join("host"));
    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    tui.wait_until_with_timeout(|s| s.contains("Scroll fixture"), Duration::from_secs(60));
    let mut measure = Measurement::start(&tui);
    click(&mut tui, "Scroll fixture");
    tui.wait_until_with_timeout(
        |s| {
            s.contains("Scroll row 31-29")
                && s.contains("Message…")
                && s.contains("fixture-model")
                && !s.contains("Restoring draft…")
        },
        Duration::from_secs(60),
    );
    measure.mark_visible();
    tui.wait_until_with_timeout(
        |s| {
            s.contains("❯ Question 31")
                && s.contains("Scroll row 31-29")
                && s.contains("Message…")
                && s.contains("fixture-model")
                && !s.contains("Restoring draft…")
        },
        Duration::from_secs(60),
    );
    checkpoint(
        &mut tui,
        "01-history-ready",
        measure,
        json!({"history_turns": 32}),
    );

    click(&mut tui, "Message…");
    for (case, bytes) in [("08k", 8 * 1024), ("48k", 48 * 1024)] {
        let text = draft(bytes);
        let measure = Measurement::start(&tui);
        paste(&mut tui, &text);
        tui.wait_for("草稿末尾");
        checkpoint(
            &mut tui,
            &format!("02-draft-{case}"),
            measure,
            json!({"draft_bytes": text.len()}),
        );
        let mut suffix = String::from("草稿末尾");
        for character in "输入验证".chars() {
            let measure = Measurement::start(&tui);
            tui.send(character.to_string().as_bytes());
            suffix.push(character);
            tui.wait_for(&suffix);
            checkpoint(
                &mut tui,
                &format!("03-type-{case}-{character}"),
                measure,
                json!({"character": character.to_string()}),
            );
        }
        // A visible suffix alone would miss dropped/reordered text above the
        // viewport. Copy the original native selection and verify every byte.
        let copied_at = tui.output.len();
        tui.send(b"\x01\x03");
        clipboard_output(&mut tui, copied_at, &format!("{text}输入验证"));
        let measure = Measurement::start(&tui);
        tui.send(b"\x01\x7f");
        tui.wait_for("Message…");
        checkpoint(
            &mut tui,
            &format!("04-clear-{case}"),
            measure,
            json!({"cleared_bytes": text.len() + "输入验证".len()}),
        );
    }

    let mut measure = Measurement::start(&tui);
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    measure.mark_visible();
    tui.wait_until(|s| {
        s.contains("Close commands")
            && !s.contains("Loading full message…")
            && s.lines()
                .any(|line| line.split('│').any(|cell| cell.trim() == "Copy message"))
    });
    checkpoint(
        &mut tui,
        "05-palette-open",
        measure,
        json!({"palette": "open"}),
    );
    let measure = Measurement::start(&tui);
    tui.send(b"New conversation");
    tui.wait_until(|s| command_query_ready(s, "New conversation"));
    checkpoint(
        &mut tui,
        "06-palette-filter",
        measure,
        json!({"query": "New conversation"}),
    );
    let measure = Measurement::start(&tui);
    tui.send(b"\x1b");
    tui.wait_until(|s| s.contains("Message…") && !s.contains("Close commands"));
    checkpoint(
        &mut tui,
        "07-palette-close",
        measure,
        json!({"palette": "closed"}),
    );

    // Detaching shows the return control and shrinks the viewport, which may
    // retain the first visible anchor. The next wheel must move actual content;
    // include its screen row because a one-line step may retain the same marker.
    tui.send(b"\x1b[<64;70;10M");
    tui.wait_until(scroll_return_visible);
    let before = first_scroll_row(&tui.screen.snapshot().unwrap().screen).unwrap();
    tui.send(b"\x1b[<64;70;10M");
    tui.wait_until(|s| first_scroll_row(s).is_some_and(|row| row != before));
    for (name, event) in [
        ("08-wheel-up", b"\x1b[<64;70;10M"),
        ("09-wheel-down", b"\x1b[<65;70;10M"),
    ] {
        let before = first_scroll_row(&tui.screen.snapshot().unwrap().screen).unwrap();
        let measure = Measurement::start(&tui);
        tui.send(event);
        tui.wait_until(|s| first_scroll_row(s).is_some_and(|row| row != before));
        checkpoint(&mut tui, name, measure, json!({"wheel_events": 1}));
    }

    for (cols, rows) in [(80, 24), (140, 45), (120, 40)] {
        let measure = Measurement::start(&tui);
        tui.resize(cols, rows);
        tui.wait_until(|s| s.contains("Scroll row ") && s.contains("Message…"));
        checkpoint(
            &mut tui,
            &format!("10-resize-{cols}x{rows}"),
            measure,
            json!({"cols": cols, "rows": rows}),
        );
    }

    let measure = Measurement::start(&tui);
    let screen = tui.screen.snapshot().unwrap().screen;
    // Resize may already clamp this short remaining distance to the tail.
    // Compare that frame above and record whether a return action was needed.
    let return_control_clicked = scroll_return_visible(&screen);
    if return_control_clicked {
        click(
            &mut tui,
            if screen.contains("Latest messages") {
                "Latest messages"
            } else {
                "Scroll to end"
            },
        );
    }
    tui.wait_until(|s| s.contains("Scroll row 31-29") && !scroll_return_visible(s));
    checkpoint(
        &mut tui,
        "10-return-tail",
        measure,
        json!({"return_to_tail": true, "return_control_clicked": return_control_clicked}),
    );

    click(&mut tui, "Message…");
    let prompt = "帧一致性与流式响应验证";
    paste(&mut tui, prompt);
    tui.wait_for(prompt);
    let mut measure = Measurement::start(&tui);
    tui.send(b"\r");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(
        sent.to_string().contains(prompt),
        "the original draft must reach the model"
    );
    tui.wait_for(&format!("❯ {prompt}"));
    measure.mark_visible();
    // The prompt preview can arrive before its recorded turn metadata. Require
    // the native timing row before sending deltas so the streaming checkpoints
    // all start with the same mounted geometry; preserve prompt-first latency.
    tui.wait_until(turn_elapsed_visible);
    checkpoint(
        &mut tui,
        "11-submitted",
        measure,
        json!({"model_requests": 1, "prompt": prompt}),
    );
    for (index, text, visible) in [
        (
            0,
            "流式开始：**中文强调**与 `inline code`。\n\n",
            "流式开始",
        ),
        (
            1,
            "```rust\nlet message = \"中文\";\n```\n\n",
            "let message",
        ),
        (2, "流式结束标记", "流式结束标记"),
    ] {
        let measure = Measurement::start(&tui);
        delta(&mut socket, text);
        tui.wait_for(visible);
        checkpoint(
            &mut tui,
            &format!("12-stream-{index}"),
            measure,
            json!({"delta_bytes": text.len(), "delta_index": index}),
        );
    }
    let mut measure = Measurement::start(&tui);
    complete(&mut socket);
    tui.wait_until(|s| {
        s.contains("流式结束标记")
            && s.contains("Message…")
            && s.contains("Send")
            && !s.contains("Queue for next turn")
    });
    measure.mark_visible();
    // `Send` follows the ended delivery, independently of the history reply.
    // The caption proves canonical text replaced the live preview. With no
    // active run, a timing row is emitted only for a recorded finished turn.
    tui.wait_until(|s| {
        s.contains("流式结束标记")
            && s.contains("Maka · Requested model: fixture-model")
            && s.contains("Send")
            && !s.contains("Queue for next turn")
            && turn_elapsed_visible(s)
    });
    checkpoint(
        &mut tui,
        "13-completed",
        measure,
        json!({"completed_model_requests": 1}),
    );
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    server.join().unwrap();
    assert!(!tui.screen.snapshot().unwrap().alternate_screen);
}

#[test]
#[ignore = "real product PTY: type and scroll while a finite model stream remains active"]
fn default_product_accepts_input_and_scroll_during_continuous_stream() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    const FRAGMENTS: usize = 160;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (requests, received) = mpsc::channel();
    let server = model_server(listener, requests, 1, 0);
    let (_directory, ui) = large_history_fixture(Some(&base));
    let host_guard = HostRootGuard::new(&ui.join("host"));
    let mut tui = Pty::spawn(&["application", "launch", "--root", ui.to_str().unwrap()]);
    tui.wait_until_with_timeout(|s| s.contains("Scroll fixture"), Duration::from_secs(60));
    let mut measure = Measurement::start(&tui);
    click(&mut tui, "Scroll fixture");
    tui.wait_until_with_timeout(
        |s| {
            s.contains("Scroll row 31-29")
                && s.contains("Message…")
                && s.contains("fixture-model")
                && !s.contains("Restoring draft…")
        },
        Duration::from_secs(60),
    );
    measure.mark_visible();
    tui.wait_until_with_timeout(
        |s| {
            s.contains("❯ Question 31")
                && s.contains("Scroll row 31-29")
                && s.contains("Message…")
                && s.contains("fixture-model")
                && !s.contains("Restoring draft…")
        },
        Duration::from_secs(60),
    );
    checkpoint(
        &mut tui,
        "01-history-ready",
        measure,
        json!({"history_turns": 32}),
    );
    click(&mut tui, "Message…");
    let prompt = "持续流式期间输入与滚动验证";
    paste(&mut tui, prompt);
    tui.wait_for(prompt);
    tui.send(b"\r");
    let (sent, mut socket) = next_request(&mut tui, &received);
    assert!(sent.to_string().contains(prompt));
    let fragments: Vec<_> = (0..FRAGMENTS)
        .map(|index| format!("并发片段 {index:03}：中文与 **Markdown**。\n\n"))
        .collect();
    let expected = fragments.concat() + "并发输出结束";
    let progress = Arc::new(AtomicUsize::new(0));
    let sending = progress.clone();
    let stream_measure = Measurement::start(&tui);
    let writer = std::thread::spawn(move || {
        for (index, fragment) in fragments.iter().enumerate() {
            delta(&mut socket, fragment);
            sending.store(index + 1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(50));
        }
        delta(&mut socket, "并发输出结束");
        complete(&mut socket);
    });
    tui.wait_for("并发片段 000");
    click(&mut tui, "Message…");
    let original = draft(8 * 1024);
    paste(&mut tui, &original);
    tui.wait_for("草稿末尾");
    let mut suffix = String::from("草稿末尾");
    let mut input_conditions = Vec::new();
    for character in "并发输入".chars() {
        let measure = Measurement::start(&tui);
        let sent_fragments = progress.load(Ordering::SeqCst);
        tui.send(character.to_string().as_bytes());
        suffix.push(character);
        tui.wait_for(&suffix);
        let visible_after = measure.started.elapsed();
        let response_fragments = progress.load(Ordering::SeqCst);
        let condition = json!({
            "character": character.to_string(),
            "sent_fragments": sent_fragments,
            "response_fragments": response_fragments,
            "sent_while_streaming": sent_fragments < FRAGMENTS,
            "response_while_streaming": response_fragments < FRAGMENTS,
            "ok": sent_fragments < FRAGMENTS && response_fragments < FRAGMENTS,
        });
        eprintln!(
            "continuous-stream typed {character} while {response_fragments}/{FRAGMENTS} fragments were sent"
        );
        record_observed_checkpoint(
            &mut tui,
            &format!("concurrent-type-{character}"),
            measure,
            json!({"character": character.to_string()}),
            false,
            Some(visible_after),
            condition.clone(),
        );
        input_conditions.push(condition);
    }
    let before_wheel = tui.screen.snapshot().unwrap().screen;
    let mut previous = first_reading_marker(&before_wheel);
    let guest_visible_before = scroll_return_visible(&before_wheel);
    let measure = Measurement::start(&tui);
    let sent_at_wheel = progress.load(Ordering::SeqCst);
    tui.send(b"\x1b[<64;70;10M");
    let deadline = measure.started + Duration::from_secs(10);
    let mut native = None;
    let mut guest = None;
    loop {
        tui.read();
        if tui.frames.ready() {
            let screen = tui.screen.snapshot().unwrap().screen;
            let current = first_reading_marker(&screen);
            if native.is_none()
                && let (Some(before), Some(after)) = (&previous, &current)
                && after.moved_up_from(before)
            {
                native = Some((
                    measure.started.elapsed(),
                    progress.load(Ordering::SeqCst),
                    before.clone(),
                    after.clone(),
                ));
            }
            previous = current;
            if guest.is_none() && !guest_visible_before && scroll_return_visible(&screen) {
                guest = Some((measure.started.elapsed(), progress.load(Ordering::SeqCst)));
            }
        }
        if (native.is_some() && guest.is_some()) || Instant::now() >= deadline {
            break;
        }
    }
    let observations = json!({
        "sent_fragments": sent_at_wheel,
        "fragment_total": FRAGMENTS,
        "guest_visible_before": guest_visible_before,
        "native_viewport": native.as_ref().map(|(elapsed, sent, before, after)| json!({
            "latency_ms": elapsed.as_secs_f64() * 1000.0,
            "sent_fragments": sent,
            "while_streaming": *sent < FRAGMENTS,
            "before": before.evidence(),
            "after": after.evidence(),
        })),
        "guest_return_control": guest.as_ref().map(|(elapsed, sent)| json!({
            "latency_ms": elapsed.as_secs_f64() * 1000.0,
            "sent_fragments": sent,
            "while_streaming": *sent < FRAGMENTS,
        })),
        "both_observed": native.is_some() && guest.is_some(),
    });
    eprintln!("continuous-stream wheel observations {observations}");
    // Persist both clocks and the complete observation window before enforcing
    // responsiveness, so a slow baseline retains the actual failure evidence.
    record_observed_checkpoint(
        &mut tui,
        "concurrent-wheel-up",
        measure,
        json!({"wheel_events": 1}),
        false,
        native.as_ref().map(|(elapsed, _, _, _)| *elapsed),
        observations.clone(),
    );
    click(&mut tui, &suffix);
    let copied_at = tui.output.len();
    tui.send(b"\x01\x03");
    clipboard_output(&mut tui, copied_at, &format!("{original}并发输入"));
    tui.send(b"\x01\x7f");
    tui.wait_for("Message…");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !writer.is_finished() {
        tui.read();
        assert!(
            Instant::now() < deadline,
            "continuous stream did not finish"
        );
    }
    writer.join().unwrap();
    server.join().unwrap();
    tui.wait_until(|s| !s.contains("Queue for next turn"));
    let screen = tui.screen.snapshot().unwrap().screen;
    if scroll_return_visible(&screen) {
        click(
            &mut tui,
            if screen.contains("Latest messages") {
                "Latest messages"
            } else {
                "Scroll to end"
            },
        );
    }
    tui.wait_until(|s| s.contains("并发输出结束") && s.contains("Send") && turn_elapsed_visible(s));
    // Copy the complete original message through the product command, not just
    // the tail visible in the virtual reading window.
    // Establish an observable composer -> source focus transition before the
    // modal captures its return target. Each click must reach a complete frame.
    for (label, cursor_visible) in [("Message…", true), ("并发输出结束", false)] {
        click(&mut tui, label);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            tui.read();
            let snapshot = tui.screen.snapshot().unwrap();
            if tui.frames.ready()
                && snapshot.cursor.visible == cursor_visible
                && snapshot.screen.contains("并发输出结束")
                && snapshot.screen.contains("Send")
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "click on {label} did not establish the expected focus:\n{}",
                snapshot.screen
            );
        }
    }
    tui.send(b"\x0b");
    tui.wait_for("Close commands");
    tui.send(b"Copy message source");
    tui.wait_until(|s| {
        command_query_ready(s, "Copy message source")
            && s.lines().any(|line| {
                line.split('│')
                    .any(|cell| cell.trim().trim_start_matches('›').trim() == "Copy message source")
            })
    });
    let copied_at = tui.output.len();
    tui.send(b"\r");
    clipboard_output(&mut tui, copied_at, &expected);
    tui.wait_until(|s| s.contains("并发输出结束") && !s.contains("Close commands"));
    checkpoint(
        &mut tui,
        "13-completed",
        stream_measure,
        json!({
            "fragments": FRAGMENTS, "response_bytes": expected.len(),
            "verified_draft_bytes": original.len() + "并发输入".len(),
            "verified_complete_response": true,
            "verified_focus_transition": ["composer", "source"],
            "measurement": "whole_stream_and_verification_including_interleaved_input",
        }),
    );
    assert!(
        !tui.screen.snapshot().unwrap().cursor.visible,
        "copy palette did not restore the original source focus"
    );
    tui.send(b"\x11");
    tui.finish();
    host_guard.stop();
    assert!(!tui.screen.snapshot().unwrap().alternate_screen);

    let native_ok = sent_at_wheel < FRAGMENTS
        && native
            .as_ref()
            .is_some_and(|(_, sent, _, _)| *sent < FRAGMENTS);
    let guest_ok =
        sent_at_wheel < FRAGMENTS && guest.as_ref().is_some_and(|(_, sent)| *sent < FRAGMENTS);
    let mut failures: Vec<String> = input_conditions
        .iter()
        .filter(|condition| condition["ok"] != true)
        .map(|condition| format!("input:{}", condition["character"].as_str().unwrap()))
        .collect();
    if !native_ok {
        failures.push("native_wheel".to_owned());
    }
    if !guest_ok {
        failures.push("guest_feedback".to_owned());
    }
    let ok = failures.is_empty();
    let completed_fragments = progress.load(Ordering::SeqCst);
    assert_eq!(completed_fragments, FRAGMENTS);
    let outcome = json!({
        "schema": 1,
        "case": "concurrent",
        "ok": ok,
        "stream_fragments": FRAGMENTS,
        "completed_fragments": completed_fragments,
        "draft_verified": true,
        "source_copy_verified": true,
        "checkpoint": "13-completed",
        "cleanup_complete": true,
        "conditions": {
            "inputs": input_conditions,
            "native_wheel": {
                "observed": native.is_some(),
                "sent_fragments": sent_at_wheel,
                "response_fragments": native.as_ref().map(|(_, sent, _, _)| sent),
                "ok": native_ok,
            },
            "guest_feedback": {
                "observed": guest.is_some(),
                "sent_fragments": sent_at_wheel,
                "response_fragments": guest.as_ref().map(|(_, sent)| sent),
                "ok": guest_ok,
            },
        },
        "failures": failures,
    });
    eprintln!("MAKA_TUI_RESPONSIVENESS {outcome}");
    assert!(ok, "MAKA_TUI_RESPONSIVENESS_STRICT_FAILURE");
}
