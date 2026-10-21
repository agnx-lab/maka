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

use super::activation::{Admission, CapturedActivation};
use super::{PaintedScene, Presenter, registry, sanitize};
use crate::terminal;
use crossterm::event::{self, Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use maka_transport::ExitMode;
use serde_json::json;
use std::time::{Duration, Instant};
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

#[derive(Default)]
struct TurnWake(tokio::sync::Notify);
impl Wake for TurnWake {
    fn wake(self: Arc<Self>) {
        self.0.notify_one();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.notify_one();
    }
}
async fn deadline(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at.into()).await,
        None => std::future::pending().await,
    }
}

struct PendingOutput {
    sequence: u64,
    generation: uuid::Uuid,
    surface: maka_plugin_core::runtime::Mount,
    receipt: tokio::sync::oneshot::Receiver<std::io::Result<()>>,
    scene: Option<PaintedScene>,
}
enum QueuedInput {
    Physical(Event, Vec<super::dialog::Scope>),
    Activation(CapturedActivation),
}
// Input readiness can win the same select as a completed write. Publish that
// original receipt before resolving coordinates or keyboard focus, including
// the first visible frame. Pending writes never make the foreground wait.
fn poll_output(
    pending: &mut Option<PendingOutput>,
    written_sequence: &mut u64,
    screen: Option<&terminal::Screen>,
    presenter: &mut Presenter,
) -> Result<(), String> {
    let Some(output) = pending.as_mut() else {
        return Ok(());
    };
    let destination = presenter.application.waker();
    let mut cx = Context::from_waker(&destination);
    let Poll::Ready(result) = Pin::new(&mut output.receipt).poll(&mut cx) else {
        return Ok(());
    };
    let output = pending.take().expect("original output receipt");
    result
        .map_err(|error| sanitize(&error.to_string()))?
        .map_err(|error| sanitize(&error.to_string()))?;
    if output.sequence != *written_sequence + 1 {
        return Err("terminal output receipt sequence mismatch".into());
    }
    *written_sequence = output.sequence;
    if screen.is_some_and(|screen| screen.generation == output.generation)
        && presenter.surface() == output.surface
        && let Some(scene) = output.scene
    {
        presenter.present_written(scene);
    }
    Ok(())
}

fn retain_clipboard(queue: &mut VecDeque<Vec<u8>>, bytes: Vec<u8>) -> Result<(), String> {
    if bytes.is_empty() {
        return Ok(());
    }
    if queue.len() >= 16 {
        return Err("terminal clipboard output queue full".into());
    }
    queue.push_back(bytes);
    Ok(())
}

fn input_needs_presentation(presenter: &Presenter, event: &Event) -> bool {
    !presenter.physical_admitted() && matches!(event, Event::Key(_) | Event::Paste(_))
}

/// Run on the foreground thread inside a multithreaded Tokio runtime.
/// Accepted service workers remain owned after the terminal surface closes.
/// 在前台线程运行；关闭界面后仍等待真实服务执行与资源回执。
#[allow(clippy::too_many_arguments)]
pub fn run(
    store: std::sync::Arc<maka_event_log::EventLog>,
    loaded: maka_event_log::applications::LoadedDeployment,
    registrations: Vec<maka_ui_runtime::actions::RemoteService>,
    scheduler: tokio::runtime::Handle,
    mut updates: maka_transport::Updates,
    execution: maka_ui_runtime::ExecutionOptions,
    startup: Option<crate::startup::Startup>,
    mut startup_errors: Vec<String>,
) -> Result<ExitMode, String> {
    let mut presenter = scheduler
        .block_on(Presenter::start_loaded(
            store,
            loaded,
            scheduler.clone(),
            registry(),
            json!({}),
            registrations,
            execution,
        ))
        .map_err(|e| sanitize(&e.to_string()))?;
    let (mut guard, previous_screen) = match startup {
        Some(startup) => (startup.guard, Some(startup.screen)),
        None => (terminal::Guard::new(), None),
    };
    let mut failure = None;
    let mut screen = match previous_screen
        .map_or_else(|| guard.setup("an interactive terminal is required"), Ok)
    {
        Ok(screen) => Some(screen),
        Err(error) => {
            failure = Some(sanitize(&error.to_string()));
            None
        }
    };
    if startup_errors.is_empty()
        && let Some(error) = presenter.deployment_status().error
    {
        startup_errors.push(error);
    }
    if !startup_errors.is_empty()
        && let Some(screen) = screen.as_mut()
    {
        let diagnostic = startup_errors.join("\n\n");
        match crate::startup::issues(screen, &diagnostic) {
            Ok(true) => {}
            Ok(false) => failure = Some(diagnostic),
            Err(error) => failure = Some(format!("{diagnostic}\n{error}")),
        }
    }
    let wake = Arc::new(TurnWake::default());
    let waker = Waker::from(wake.clone());
    presenter.application.set_waker(waker.clone());
    updates.set_waker(waker);
    // Rejected terminal setup has no input source. Keep draining the application
    // without asking crossterm to construct a reader for a missing terminal.
    let mut events = screen.as_ref().map(|_| EventStream::new());
    let output = terminal::Output::new().map_err(|e| e.to_string())?;
    let mut pending_output: Option<PendingOutput> = None;
    let mut clipboard_output = VecDeque::new();
    let mut issued_sequence = 0u64;
    let mut written_sequence = 0u64;
    let mut exit_mode = ExitMode::Quit;
    let mut exit_key = ExitKey::default();
    let mut interrupt = InterruptWindow::default();
    let mut queued_input: Option<QueuedInput> = None;
    loop {
        if let Err(error) = poll_output(
            &mut pending_output,
            &mut written_sequence,
            screen.as_ref(),
            &mut presenter,
        ) {
            failure.get_or_insert(error);
        }
        if pending_output.is_none()
            && let Some(bytes) = clipboard_output.pop_front()
        {
            issued_sequence += 1;
            match output.submit(bytes) {
                Ok(receipt) => {
                    pending_output = Some(PendingOutput {
                        sequence: issued_sequence,
                        generation: screen.as_ref().expect("original terminal").generation,
                        surface: presenter.surface(),
                        receipt,
                        scene: None,
                    });
                    wake.0.notify_one();
                }
                Err(error) => {
                    failure.get_or_insert(sanitize(&error.to_string()));
                }
            }
        }
        if failure.is_some()
            || ((!presenter.surface.is_live()
                || presenter.application.is_retiring(&presenter.root))
                && !presenter.requires_surface_replacement())
        {
            updates.stop();
            if let Err(error) = presenter.retire() {
                presenter.report(&error);
            }
        }
        match updates.poll(&presenter.application) {
            Ok(Some(update)) => {
                if let Err(error) = presenter.adopt_prepared(update.prepared, update.registrations)
                {
                    presenter.report(&error);
                }
            }
            Ok(None) => {}
            Err(error) => presenter.report(&error),
        }
        if let Err(error) = presenter
            .pump_with_input_priority(matches!(queued_input, Some(QueuedInput::Activation(_))))
        {
            let error = sanitize(&error.to_string());
            presenter.report(&error);
            failure.get_or_insert(error);
        }
        let mut bytes = Vec::new();
        if let Err(error) = presenter.flush_clipboard(&mut bytes) {
            presenter.report(&error);
        }
        if let Err(error) = retain_clipboard(&mut clipboard_output, bytes) {
            presenter.report(&error);
        }
        // A blocked write already owns its completion wakeup. Queued clipboard
        // bytes become runnable only after it finishes; waking ourselves while
        // it is pending would spin the foreground under output backpressure.
        if pending_output.is_none() && !clipboard_output.is_empty() {
            wake.0.notify_one();
        }
        if failure.is_some()
            || !presenter.surface.is_live()
            || presenter.application.is_retiring(&presenter.root)
        {
            if (failure.is_some() || !presenter.requires_surface_replacement())
                && let Err(error) = presenter.retire()
            {
                presenter.report(&error);
            }
            if let super::SurfaceState::Retiring { cleanup: None, .. } = &presenter.surface {
                let original = presenter.surface();
                // Backend destruction and Guard restoration belong to this exact
                // original root. A late receipt cannot close a replacement.
                if pending_output.is_some() || !clipboard_output.is_empty() {
                    scheduler.block_on(async {
                        tokio::select! {
                            _ = wake.0.notified() => {},
                            _ = events.as_mut().expect("initialized terminal").next() => {},
                        }
                    });
                    continue;
                }
                screen.take();
                exit_key = ExitKey::default();
                queued_input = None;
                let cleanup = guard.finish().map_err(|e| sanitize(&e.to_string()));
                if let Err(error) = presenter.surface_closed(&original, cleanup) {
                    presenter.report(&error);
                    failure.get_or_insert(sanitize(&error.to_string()));
                }
            }
            if failure.is_some() || !presenter.requires_surface_replacement() {
                break;
            }
            match presenter.try_replace_surface(json!({})) {
                Ok(true) => match guard.setup("an interactive terminal is required") {
                    Ok(replacement) => screen = Some(replacement),
                    Err(error) => {
                        failure = Some(sanitize(&error.to_string()));
                    }
                },
                Ok(false) => scheduler.block_on(async {
                    tokio::select! {
                        _ = wake.0.notified() => {},
                        _ = deadline(updates.next_deadline()) => {},
                    }
                }),
                Err(error) => {
                    presenter.report(&error);
                    failure = Some(sanitize(&error.to_string()));
                }
            }
            continue;
        }
        let result = (|| -> Result<bool, String> {
            if let Some(mode) = exit_key.ready(None, Instant::now()) {
                exit_mode = mode;
                return Ok(true);
            }
            let screen = screen.as_mut().expect("active terminal");
            if pending_output.is_none()
                && (presenter.resources.redraw
                    || presenter.next_frame.is_some_and(|at| Instant::now() >= at))
            {
                let mut scene = None;
                let mut paint_error = None;
                let bytes = terminal::render(screen, |frame| {
                    match presenter.paint(frame.area(), frame.buffer_mut()) {
                        Ok(painted) => {
                            if let Some(cursor) = painted.cursor_position() {
                                frame.set_cursor_position(cursor);
                            }
                            scene = Some(painted);
                        }
                        Err(error) => paint_error = Some(error.to_string()),
                    }
                })
                .map_err(|e| e.to_string())?;
                if let Some(error) = paint_error {
                    return Err(error);
                }
                issued_sequence += 1;
                pending_output = Some(PendingOutput {
                    sequence: issued_sequence,
                    generation: screen.generation,
                    surface: presenter.surface(),
                    receipt: output.submit(bytes).map_err(|e| e.to_string())?,
                    scene,
                });
                wake.0.notify_one();
                presenter.resources.redraw = false;
                presenter.next_frame = None;
            }
            if queued_input.as_ref().is_some_and(|input| match input {
                QueuedInput::Physical(_, openings) => !presenter.scene_openings_current(openings),
                QueuedInput::Activation(input) => {
                    presenter.activation_admission(input) == Admission::Discard
                }
            }) {
                queued_input = None;
            }
            let admission_pending = match &queued_input {
                Some(QueuedInput::Physical(event, _)) => {
                    (written_sequence == 0 && pending_output.is_some())
                        || input_needs_presentation(&presenter, event)
                }
                Some(QueuedInput::Activation(input)) => {
                    presenter.activation_admission(input) == Admission::Pending
                }
                None => false,
            };
            let mut replay = None;
            let input = if queued_input.is_some() && !admission_pending {
                queued_input.take().map(|input| match input {
                    QueuedInput::Physical(event, _) => event,
                    QueuedInput::Activation(input) => {
                        let event = input.event();
                        replay = Some(input);
                        event
                    }
                })
            } else {
                let at = [
                    presenter.next_frame.filter(|_| pending_output.is_none()),
                    exit_key.0.map(|(_, _, at)| at),
                    updates.next_deadline(),
                ]
                .into_iter()
                .flatten()
                .min();
                scheduler
                    .block_on(async {
                        tokio::select! {
                            input = events.as_mut().expect("initialized terminal").next(), if queued_input.is_none() => input.transpose(),
                            _ = wake.0.notified() => Ok(None),
                            _ = deadline(at) => Ok(None),
                        }
                    })
                    .map_err(|e| e.to_string())?
            };
            let mut next_input = input;
            while let Some(mut input) = next_input.take() {
                poll_output(
                    &mut pending_output,
                    &mut written_sequence,
                    Some(screen),
                    &mut presenter,
                )?;
                if let Some(activation) = replay.take() {
                    match presenter.activation_admission(&activation) {
                        Admission::Ready => match presenter
                            .release_activation(activation)
                            .map_err(|error| error.to_string())?
                        {
                            Some(event) => input = event,
                            None => return Ok(false),
                        },
                        Admission::Discard => return Ok(false),
                        Admission::Pending => {
                            queued_input = Some(QueuedInput::Activation(activation));
                            wake.0.notify_one();
                            return Ok(false);
                        }
                    }
                }
                if let Some(mode) = exit_key.ready(Some(&input), Instant::now()) {
                    exit_mode = mode;
                    return Ok(true);
                }
                interrupt.observe(&input);
                if let Some(activation) = presenter.capture_activation(&input)
                    && presenter.activation_admission(&activation) == Admission::Pending
                {
                    queued_input = Some(QueuedInput::Activation(activation));
                    presenter.resources.redraw = true;
                    wake.0.notify_one();
                    return Ok(false);
                }
                // The terminal can expose a new surface or dialog before its
                // write receipt admits input. Keep that input for this opening;
                // ordinary updates still use the already presented geometry.
                if ((written_sequence == 0 && pending_output.is_some())
                    || input_needs_presentation(&presenter, &input))
                    && !is_exit(&input)
                {
                    queued_input = Some(QueuedInput::Physical(
                        input,
                        presenter.opening_witnesses().map_err(|e| e.to_string())?,
                    ));
                    presenter.resources.redraw = true;
                    wake.0.notify_one();
                    return Ok(false);
                }
                if let Some(selection_only) = copy_gesture(&input, cfg!(target_os = "macos")) {
                    let mut bytes = Vec::new();
                    let copied = match presenter.copy_scope(&mut bytes, selection_only) {
                        Ok(copied) => copied,
                        Err(error) => {
                            presenter.report(&error);
                            true
                        }
                    };
                    if let Err(error) = retain_clipboard(&mut clipboard_output, bytes) {
                        presenter.report(&error);
                    }
                    if copied || !is_interrupt(&input) {
                        interrupt.0 = None;
                        wake.0.notify_one();
                        return Ok(false);
                    }
                }
                if is_exit(&input) && !presenter.captures_keyboard().map_err(|e| e.to_string())? {
                    let mut captured = None;
                    let handled = presenter
                        .dispatch_focused_shortcut_with_capture(&input, Some(&mut captured))
                        .map_err(|error| error.to_string())?;
                    if let Some(activation) = captured {
                        queued_input = Some(QueuedInput::Activation(activation));
                        presenter.resources.redraw = true;
                        wake.0.notify_one();
                        return Ok(false);
                    }
                    let Event::Key(key) = input else {
                        unreachable!()
                    };
                    if handled {
                        interrupt.0 = None;
                        return Ok(false);
                    }
                    let now = Instant::now();
                    if matches!(key.code, KeyCode::Char('c' | 'C')) && !interrupt.request(now) {
                        return Ok(false);
                    }
                    let mode = requested_exit_mode(&Event::Key(key)).expect("exit gesture");
                    if let Some(mode) =
                        exit_key.request(key.code, mode, guard.reports_key_releases(), now)
                    {
                        exit_mode = mode;
                        return Ok(true);
                    }
                    return Ok(false);
                }
                // Captured terminal input belongs to its child process, never
                // to a previously armed application exit gesture.
                if is_interrupt(&input) {
                    interrupt.0 = None;
                }
                let scrolling = is_wheel(&input);
                let captured = if scrolling {
                    presenter.wheel(input).map_err(|error| error.to_string())?;
                    None
                } else {
                    presenter
                        .input_with_capture(input)
                        .map_err(|error| error.to_string())?
                };
                if let Some(activation) = captured {
                    queued_input = Some(QueuedInput::Activation(activation));
                    presenter.resources.redraw = true;
                    wake.0.notify_one();
                    return Ok(false);
                }
                // A trackpad gesture arrives as many small wheel events. Apply
                // the queued gesture before another live update and repaint,
                // so streaming cannot repeatedly move the tail ahead of it.
                // A drained non-wheel event uses this same intake before a
                // background commit can replace its original hit geometry.
                let until = Instant::now() + Duration::from_millis(8);
                while scrolling && Instant::now() < until {
                    // The level-triggered tty backend does not drain its parser
                    // on a zero-time poll. Give EventStream's reader a short
                    // chance to publish already buffered input before repainting.
                    let idle = until.min(Instant::now() + Duration::from_millis(1));
                    let Ok(Some(input)) = scheduler.block_on(async {
                        tokio::time::timeout_at(
                            idle.into(),
                            events.as_mut().expect("initialized terminal").next(),
                        )
                        .await
                    }) else {
                        break;
                    };
                    let input = input.map_err(|e| e.to_string())?;
                    poll_output(
                        &mut pending_output,
                        &mut written_sequence,
                        Some(screen),
                        &mut presenter,
                    )?;
                    if !is_wheel(&input) {
                        next_input = Some(input);
                        break;
                    }
                    presenter.wheel(input).map_err(|e| e.to_string())?;
                }
                if scrolling {
                    presenter.sync_native().map_err(|e| e.to_string())?;
                }
                let mut bytes = Vec::new();
                if let Err(error) = presenter.flush_clipboard(&mut bytes) {
                    presenter.report(&error);
                }
                if let Err(error) = retain_clipboard(&mut clipboard_output, bytes) {
                    presenter.report(&error);
                }
                if pending_output.is_none() && !clipboard_output.is_empty() {
                    wake.0.notify_one();
                }
            }
            Ok(false)
        })();
        match result {
            Ok(false) => {}
            Ok(true) => {
                if let Err(error) = presenter.retire() {
                    failure = Some(sanitize(&error.to_string()));
                }
            }
            Err(error) => {
                failure = Some(sanitize(&error));
            }
        }
    }
    updates.stop();
    scheduler.block_on(updates.drain());
    // Failures remain inspectable in this owner; dropping it cannot substitute
    // for a service/terminal cleanup receipt. Hard process exit is not recovery.
    loop {
        if let Err(error) = presenter.pump() {
            presenter.report(&error);
        }
        if presenter.can_shutdown() {
            match presenter.shutdown() {
                Ok(()) => {
                    for diagnostic in &presenter.reported {
                        if failure.as_ref() != Some(diagnostic) {
                            eprintln!("{diagnostic}");
                        }
                    }
                    return failure.map_or(Ok(exit_mode), Err);
                }
                Err(maka_ui_runtime::Error::Core(maka_plugin_core::Error::CleanupPending)) => {}
                Err(error) => presenter.report(&error),
            }
        }
        scheduler.block_on(wake.0.notified());
    }
}
fn is_wheel(event: &Event) -> bool {
    matches!(event, Event::Mouse(mouse) if matches!(mouse.kind,
        crossterm::event::MouseEventKind::ScrollUp | crossterm::event::MouseEventKind::ScrollDown))
}
// Ctrl+C copies a selection before falling back to interrupt/clear. Cmd+C
// also works when a macOS terminal forwards it; Ctrl+Shift+C copies the scope.
fn copy_gesture(event: &Event, macos: bool) -> Option<bool> {
    let Event::Key(key) = event else { return None };
    if key.kind != KeyEventKind::Press || !matches!(key.code, KeyCode::Char('c' | 'C')) {
        return None;
    }
    if key.modifiers == KeyModifiers::CONTROL | KeyModifiers::SHIFT {
        Some(false)
    } else if key.modifiers == KeyModifiers::CONTROL
        || (macos && key.modifiers == KeyModifiers::SUPER)
    {
        Some(true)
    } else {
        None
    }
}
pub(super) fn is_interrupt(event: &Event) -> bool {
    matches!(event,Event::Key(key) if key.kind==KeyEventKind::Press && matches!(key.code,KeyCode::Char('c' | 'C')) && key.modifiers==KeyModifiers::CONTROL)
}
fn is_exit(event: &Event) -> bool {
    requested_exit_mode(event).is_some()
}
fn requested_exit_mode(event: &Event) -> Option<ExitMode> {
    let Event::Key(key) = event else { return None };
    if key.kind != KeyEventKind::Press {
        return None;
    }
    if matches!(key.code, KeyCode::Char('q' | 'Q'))
        && key.modifiers == KeyModifiers::CONTROL | KeyModifiers::ALT
    {
        Some(ExitMode::KeepHost)
    } else if is_interrupt(event)
        || (key.code == KeyCode::Char('q') && key.modifiers == KeyModifiers::CONTROL)
    {
        Some(ExitMode::Quit)
    } else {
        None
    }
}

#[derive(Default)]
struct InterruptWindow(Option<Instant>);
impl InterruptWindow {
    fn request(&mut self, now: Instant) -> bool {
        if self.0.take().is_some_and(|until| now < until) {
            return true;
        }
        self.0 = Some(now + Duration::from_secs(2));
        false
    }
    fn observe(&mut self, event: &Event) {
        if !is_interrupt(event)
            && matches!(event,
            Event::Key(key) if key.kind == KeyEventKind::Press)
            || matches!(
                event,
                Event::Paste(_)
                    | Event::FocusLost
                    | Event::Mouse(event::MouseEvent {
                        kind: event::MouseEventKind::Down(_),
                        ..
                    })
            )
        {
            self.0 = None;
        }
    }
}

/// Keep a negotiated physical quit gesture inside its original raw terminal.
/// A lost release must not strand shutdown; another physical action cancels it.
#[derive(Default)]
struct ExitKey(Option<(KeyCode, ExitMode, Instant)>);
impl ExitKey {
    fn request(
        &mut self,
        key: KeyCode,
        mode: ExitMode,
        releases: bool,
        now: Instant,
    ) -> Option<ExitMode> {
        if !releases {
            return Some(mode);
        }
        self.0 = Some((key, mode, now + Duration::from_secs(2)));
        None
    }
    fn ready(&mut self, event: Option<&Event>, now: Instant) -> Option<ExitMode> {
        let (code, mode, deadline) = self.0?;
        if now >= deadline
            || matches!(event, Some(Event::Key(key)) if key.kind == KeyEventKind::Release && key.code == code)
        {
            self.0 = None;
            return Some(mode);
        }
        if matches!(event, Some(Event::Key(key)) if key.kind == KeyEventKind::Press)
            || matches!(event, Some(Event::Mouse(mouse)) if matches!(mouse.kind, event::MouseEventKind::Down(_)))
        {
            self.0 = None;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    #[test]
    fn interrupt_requires_two_unused_presses_within_two_seconds() {
        let now = Instant::now();
        let mut window = InterruptWindow::default();
        assert!(!window.request(now));
        assert!(window.request(now + Duration::from_millis(1999)));
        assert!(!window.request(now + Duration::from_secs(3)));
        assert!(!window.request(now + Duration::from_secs(5)));
        let other = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        window.observe(&other);
        assert!(!window.request(now + Duration::from_millis(5500)));
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let event = Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
                kind,
            ));
            assert!(!is_interrupt(&event));
            window.observe(&event);
        }
        assert!(window.request(now + Duration::from_secs(6)));
    }
    #[test]
    fn selection_copy_accepts_control_on_all_terminals_and_command_on_mac() {
        let command = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::SUPER));
        let control = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(copy_gesture(&command, true), Some(true));
        assert_eq!(copy_gesture(&control, true), Some(true));
        assert_eq!(copy_gesture(&control, false), Some(true));
        assert_eq!(copy_gesture(&command, false), None);
        assert!(!is_exit(&command));
    }
    #[test]
    fn quit_release_keeps_original_key_and_cancels_on_a_new_gesture() {
        let now = Instant::now();
        let mut exit = ExitKey::default();
        assert_eq!(
            exit.request(KeyCode::Char('q'), ExitMode::Quit, false, now),
            Some(ExitMode::Quit)
        );
        assert_eq!(
            exit.request(KeyCode::Char('q'), ExitMode::Quit, true, now),
            None
        );
        let released = |code| {
            Event::Key(KeyEvent::new_with_kind(
                code,
                KeyModifiers::NONE,
                KeyEventKind::Release,
            ))
        };
        assert_eq!(exit.ready(Some(&released(KeyCode::Char('c'))), now), None);
        assert_eq!(
            exit.ready(Some(&released(KeyCode::Char('q'))), now),
            Some(ExitMode::Quit)
        );
        assert_eq!(exit.ready(Some(&released(KeyCode::Char('q'))), now), None);
        assert_eq!(
            exit.request(KeyCode::Char('c'), ExitMode::Quit, true, now),
            None
        );
        let repeat = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Repeat,
        ));
        assert_eq!(exit.ready(Some(&repeat), now), None);
        let other = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(exit.ready(Some(&other), now), None);
        assert_eq!(exit.ready(Some(&released(KeyCode::Char('c'))), now), None);
        assert_eq!(exit.ready(None, now + Duration::from_secs(3)), None);
        assert_eq!(
            exit.request(KeyCode::Char('q'), ExitMode::Quit, true, now),
            None
        );
        assert_eq!(exit.ready(None, now + Duration::from_secs(1)), None);
        assert_eq!(
            exit.ready(None, now + Duration::from_secs(2)),
            Some(ExitMode::Quit)
        );
    }
    #[test]
    fn explicit_ui_only_exit_is_a_distinct_physical_gesture() {
        let gesture = Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        ));
        assert_eq!(requested_exit_mode(&gesture), Some(ExitMode::KeepHost));
        assert!(is_exit(&gesture));
        assert!(!is_interrupt(&gesture));
        assert_eq!(copy_gesture(&gesture, false), None);
        for modifiers in [KeyModifiers::CONTROL, KeyModifiers::NONE, KeyModifiers::ALT] {
            let input = Event::Key(KeyEvent::new(KeyCode::Char('q'), modifiers));
            let expected = (modifiers == KeyModifiers::CONTROL).then_some(ExitMode::Quit);
            assert_eq!(requested_exit_mode(&input), expected);
        }
        for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
            let input = Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char('q'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
                kind,
            ));
            assert_eq!(requested_exit_mode(&input), None);
        }
    }
    #[test]
    fn cancelled_ui_only_exit_does_not_change_the_next_default_quit() {
        let now = Instant::now();
        let mut exit = ExitKey::default();
        assert_eq!(
            exit.request(KeyCode::Char('q'), ExitMode::KeepHost, true, now),
            None
        );
        let other = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(exit.ready(Some(&other), now), None);
        assert_eq!(exit.ready(None, now + Duration::from_secs(3)), None);
        assert_eq!(
            exit.request(KeyCode::Char('q'), ExitMode::Quit, true, now),
            None
        );
        let release = Event::Key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
            KeyEventKind::Release,
        ));
        assert_eq!(exit.ready(Some(&release), now), Some(ExitMode::Quit));
        assert_eq!(
            exit.request(KeyCode::Char('q'), ExitMode::KeepHost, true, now),
            None
        );
        assert_eq!(exit.ready(Some(&release), now), Some(ExitMode::KeepHost));
        assert_eq!(
            exit.request(KeyCode::Char('q'), ExitMode::KeepHost, false, now),
            Some(ExitMode::KeepHost)
        );
    }
    #[test]
    fn source_copy_does_not_overlap_exit_or_release() {
        let copy = Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(copy_gesture(&copy, true), Some(false));
        assert!(!is_exit(&copy));
        let uppercase = Event::Key(KeyEvent::new(
            KeyCode::Char('C'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(copy_gesture(&uppercase, false), Some(false));
        assert!(!is_exit(&uppercase));
        let exit = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(is_exit(&exit));
        assert_eq!(copy_gesture(&exit, true), Some(true));
        assert_eq!(copy_gesture(&exit, false), Some(true));
        let quit = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert!(is_exit(&quit));
        assert_eq!(copy_gesture(&quit, false), None);
        let mut held = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        held.kind = KeyEventKind::Repeat;
        assert!(!is_exit(&Event::Key(held)));
        let mut released = KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        released.kind = KeyEventKind::Release;
        assert_eq!(copy_gesture(&Event::Key(released), false), None);
        assert!(!is_exit(&Event::Key(released)));
    }
}
