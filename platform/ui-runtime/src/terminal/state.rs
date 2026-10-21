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

//! Original bounded queue and canonical screen, independent of App and transport.
use super::*;
use crate::budget::{Kind, NativeBudget, Permit};
use maka_agent_facts::terminal::TerminalCursor;
/// Scheduling target for recoverable native batches, not an action admission limit.
const MAX_INPUT_ACTIONS: usize = 64;
use maka_agent_facts::terminal::input::encoded_actions_byte_len;
use std::collections::VecDeque;

const MAX_BATCH_BYTES: usize = 32 * 1024;

pub struct Display {
    pub screen: String,
    pub scrollback: String,
    pub size: TerminalSize,
    pub cursor: TerminalCursor,
}
pub struct State {
    observation: TerminalViewObservation,
    pub display: Option<Display>,
    pub scroll: usize,
    size: Option<TerminalSize>,
    desired_size: Option<TerminalSize>,
    next_sequence: u64,
    queue: VecDeque<(InputAction, usize)>,
    queued_bytes: usize,
    pending_bytes: usize,
    dirty: bool,
    physical: bool,
    permit: Option<Permit>,
}
impl State {
    pub fn new(budget: &NativeBudget) -> Self {
        // Retain native presentation custody. The input window separately packs
        // complete actions and retains their actual bytes until the exact ACK.
        // No worker or process lives in this owner.
        let permit = budget.reserve(Kind::Encoded, 384 * 1024).ok();
        Self {
            observation: TerminalViewObservation {
                owner: Uuid::new_v4(),
                revision: 0,
                source: String::new(),
                controller: None,
                captured: false,
                batch: None,
                error: permit
                    .is_none()
                    .then_some(TerminalViewError::BudgetExhausted),
            },
            display: None,
            scroll: 0,
            size: None,
            desired_size: None,
            next_sequence: 1,
            queue: VecDeque::new(),
            queued_bytes: 0,
            pending_bytes: 0,
            dirty: true,
            physical: false,
            permit,
        }
    }
    fn changed(&mut self) {
        self.observation.revision = self
            .observation
            .revision
            .checked_add(1)
            .expect("terminal revision");
        self.dirty = true;
    }
    pub fn observation(&self) -> &TerminalViewObservation {
        &self.observation
    }
    pub fn pending_observation(&self) -> Option<TerminalViewObservation> {
        self.dirty.then(|| self.observation.clone())
    }
    pub fn published(&mut self, revision: u64) {
        if self.observation.revision == revision {
            self.dirty = false;
        }
    }
    pub fn apply(&mut self, props: &TerminalViewProps) {
        if self.observation.source != props.source
            || self.observation.controller != props.controller
        {
            self.observation.source.clone_from(&props.source);
            self.observation.controller = props.controller;
            self.observation.captured = false;
            self.physical = false;
            self.observation.batch = None;
            self.observation.error = self
                .permit
                .is_none()
                .then_some(TerminalViewError::BudgetExhausted);
            self.queue.clear();
            self.queued_bytes = 0;
            self.pending_bytes = 0;
            self.next_sequence = 1;
            self.size = None;
            self.desired_size = None;
            self.scroll = 0;
            self.changed();
        }
        self.display = if self.permit.is_some() {
            match &props.output {
                Some(ShellOutput::Pty {
                    screen,
                    scrollback,
                    cols,
                    rows,
                    cursor,
                    redacted: false,
                    ..
                }) => TerminalSize::new(*cols, *rows).ok().map(|size| Display {
                    screen: screen.clone(),
                    scrollback: scrollback.clone(),
                    size,
                    cursor: cursor.clone(),
                }),
                _ => None,
            }
        } else {
            None
        };
        self.scroll = self.scroll.min(
            self.display
                .as_ref()
                .map_or(0, |d| d.scrollback.lines().count()),
        );
        if self.size.is_none() {
            self.size = self.display.as_ref().map(|d| d.size);
        }
        if !self.ready() {
            self.capture(false);
        }
        if let Some(ack) = &props.acknowledgement
            && ack.owner == self.observation.owner
            && let Some(batch) = &self.observation.batch
            && ack.controller == batch.controller
            && ack.sequence == batch.sequence
        {
            if ack.accepted {
                if let Some(size) = batch.size {
                    self.size = Some(size);
                }
                self.queued_bytes -= self.pending_bytes;
                self.pending_bytes = 0;
                self.observation.batch = None;
                if self.observation.error == Some(TerminalViewError::InputFull) {
                    self.observation.error = None;
                }
                self.changed();
            } else {
                self.fail();
            }
        }
        self.pump();
    }
    pub fn ready(&self) -> bool {
        self.observation.controller.is_some()
            && self.display.is_some()
            && matches!(
                self.observation.error,
                None | Some(TerminalViewError::InputFull)
            )
    }
    pub fn captured(&self) -> bool {
        self.observation.captured
    }
    pub fn capture(&mut self, capture: bool) {
        let capture = capture && self.physical && self.ready();
        if self.observation.captured != capture {
            self.observation.captured = capture;
            self.changed();
        }
    }
    pub fn presentation(&mut self, physical: bool) {
        self.physical = physical;
        if !physical {
            self.capture(false);
        }
    }
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.desired_size = Some(TerminalSize::fit(cols, rows));
        self.pump();
    }
    pub fn write(&mut self, action: InputAction) {
        if !self.physical || !self.ready() || !self.captured() {
            return;
        }
        let bytes = serde_json::to_vec(&action)
            .expect("typed terminal input")
            .len();
        if encoded_actions_byte_len(std::slice::from_ref(&action)).is_err()
            || self.queued_bytes.checked_add(bytes).is_none()
            || (self.queued_bytes != 0 && self.queued_bytes.saturating_add(bytes) > 64 * 1024)
        {
            if self.observation.error != Some(TerminalViewError::InputFull) {
                self.observation.error = Some(TerminalViewError::InputFull);
                self.changed();
            }
            return;
        }
        self.queued_bytes += bytes;
        self.queue.push_back((action, bytes));
        self.pump();
    }
    pub fn paste(&mut self, text: &str) {
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
            .collect();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if !text.is_empty() {
            self.write(InputAction::Paste(text));
        }
    }
    fn fail(&mut self) {
        self.observation.error = Some(TerminalViewError::InputFailed);
        self.observation.batch = None;
        self.observation.captured = false;
        self.queue.clear();
        self.queued_bytes = 0;
        self.pending_bytes = 0;
        self.changed();
    }
    fn pump(&mut self) {
        if !self.physical || !self.ready() || self.observation.batch.is_some() {
            return;
        }
        let size = self.desired_size.filter(|size| Some(*size) != self.size);
        // A resize settles before any queued key or mouse action, as in the original View.
        let actions = if size.is_some() {
            vec![]
        } else if !self.queue.is_empty() {
            // Batch queued actions to a scheduling target, admitting a complete
            // oversized first action. This original batch stays immutable until
            // its exact receipt; a paste is never split into new actions.
            let mut actions = Vec::new();
            let mut encoded_bytes = 0;
            while let Some((action, _)) = self.queue.front() {
                let bytes = encoded_actions_byte_len(std::slice::from_ref(action))
                    .expect("validated queued input");
                if actions.len() == MAX_INPUT_ACTIONS
                    || (!actions.is_empty()
                        && bytes > MAX_BATCH_BYTES.saturating_sub(encoded_bytes))
                {
                    break;
                }
                let (action, retained_bytes) = self.queue.pop_front().unwrap();
                encoded_bytes += bytes;
                self.pending_bytes += retained_bytes;
                actions.push(action);
            }
            actions
        } else {
            return;
        };
        if self.next_sequence > 9_007_199_254_740_991 {
            self.fail();
            return;
        }
        self.observation.batch = Some(TerminalInputBatch {
            controller: self.observation.controller.unwrap(),
            sequence: self.next_sequence,
            actions,
            size,
        });
        self.next_sequence += 1;
        self.changed();
    }
    pub fn scroll_by(&mut self, lines: i32) {
        let max = self
            .display
            .as_ref()
            .map_or(0, |d| d.scrollback.lines().count());
        self.scroll = self.scroll.saturating_add_signed(lines as isize).min(max);
    }
    pub fn text(&self, rows: usize) -> String {
        let Some(screen) = &self.display else {
            return String::new();
        };
        if self.scroll == 0 {
            return screen.screen.clone();
        }
        let mut lines: Vec<_> = screen
            .scrollback
            .lines()
            .chain(screen.screen.lines())
            .collect();
        let blanks = usize::from(screen.size.rows()).saturating_sub(screen.screen.lines().count());
        lines.extend(std::iter::repeat_n("", blanks));
        let end = lines.len().saturating_sub(self.scroll);
        lines[end.saturating_sub(rows)..end].join("\n")
    }
}

#[cfg(test)]
mod tests;
