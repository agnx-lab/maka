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
fn props() -> TerminalViewProps {
    serde_json::from_value(serde_json::json!({
        "accessibilityLabel":"Terminal", "source":"resource-A", "controller":Uuid::new_v4(), "acknowledgement":null,
        "output":{"mode":"pty","screen":"prompt","scrollback":"one\ntwo\nthree", "cols":40,"rows":3,"cursor":{"x":6,"y":0,"visible":true},"alternateScreen":false,"truncated":false,"redacted":false},
        "appearance":{"font":{"family":"monospace","fallbacks":[],"size":14,"lineHeight":1,"weight":400},"foreground":4294967295u32},
        "background":255,"caret":4294967295u32,"rows":3,"tabIndex":0
    })).unwrap()
}
fn acknowledge(state: &mut State, props: &mut TerminalViewProps, accepted: bool) {
    let batch = state.observation().batch.as_ref().unwrap();
    props.acknowledgement = Some(TerminalInputAcknowledgement {
        owner: state.observation().owner,
        controller: batch.controller,
        sequence: batch.sequence,
        accepted,
    });
    state.apply(props);
}
#[test]
fn queued_keys_share_a_bounded_receipt_without_changing_an_inflight_batch() {
    let mut state = State::new(&NativeBudget::default());
    let mut props = props();
    state.apply(&props);
    state.presentation(true);
    state.capture(true);
    state.resize(80, 24);
    let resize = state.observation().batch.clone().unwrap();
    let actions: Vec<_> = (0..MAX_INPUT_ACTIONS + 2)
        .map(|n| InputAction::Text(format!("{n}")))
        .collect();
    for action in &actions {
        state.write(action.clone());
    }
    assert_eq!(state.observation().batch.as_ref(), Some(&resize));
    acknowledge(&mut state, &mut props, true);
    let first = state.observation().batch.clone().unwrap();
    assert_eq!(first.actions, actions[..MAX_INPUT_ACTIONS]);
    assert_eq!(first.size, None);
    state.write(InputAction::Paste("中文🙂\n".into()));
    state.apply(&props); // The previous resize receipt cannot consume this batch.
    assert_eq!(state.observation().batch.as_ref(), Some(&first));
    acknowledge(&mut state, &mut props, true);
    let second = state.observation().batch.as_ref().unwrap();
    assert_eq!(second.sequence, first.sequence + 1);
    assert_eq!(
        second.actions,
        [
            actions[MAX_INPUT_ACTIONS].clone(),
            actions[MAX_INPUT_ACTIONS + 1].clone(),
            InputAction::Paste("中文🙂\n".into()),
        ]
    );
    acknowledge(&mut state, &mut props, true);
    assert!(state.observation().batch.is_none());
    assert!(state.queue.is_empty());
    assert_eq!(state.queued_bytes, 0);
    assert_eq!(state.pending_bytes, 0);
}
#[test]
fn batches_reserve_paste_framing_and_failed_receipts_never_advance_the_queue() {
    let mut state = State::new(&NativeBudget::default());
    let mut props = props();
    state.apply(&props);
    state.presentation(true);
    state.capture(true);
    state.resize(80, 24);
    state.write(InputAction::Text("a".repeat(MAX_BATCH_BYTES - 16)));
    state.paste("界"); // Three UTF-8 bytes plus twelve bracketed-paste bytes.
    state.write(InputAction::Text("b".into()));
    state.write(InputAction::Text("c".into()));
    acknowledge(&mut state, &mut props, true);
    let batch = state.observation().batch.as_ref().unwrap();
    assert_eq!(batch.actions.len(), 3);
    assert_eq!(
        encoded_actions_byte_len(&batch.actions).unwrap(),
        MAX_BATCH_BYTES
    );
    assert_eq!(
        state.queue.front().unwrap().0,
        InputAction::Text("c".into())
    );
    acknowledge(&mut state, &mut props, false);
    assert!(state.observation().batch.is_none());
    assert_eq!(
        state.observation().error,
        Some(TerminalViewError::InputFailed)
    );
    assert!(state.queue.is_empty());
    assert_eq!(state.queued_bytes, 0);
    assert_eq!(state.pending_bytes, 0);
    state.apply(&props);
    state.capture(true);
    state.write(InputAction::Text("no replay".into()));
    assert!(state.observation().batch.is_none());
}
#[test]
fn resize_and_backpressure_preserve_exact_queued_input_until_its_own_receipt() {
    let mut state = State::new(&NativeBudget::default());
    let mut props = props();
    state.apply(&props);
    state.presentation(true);
    state.capture(true);
    state.resize(80, 24);
    let resize = state.observation().batch.clone().unwrap();
    state.write(InputAction::Text("A".repeat(32000)));
    state.write(InputAction::Text("B".repeat(32000)));
    state.write(InputAction::Text("C".repeat(32000)));
    assert_eq!(
        state.observation().error,
        Some(TerminalViewError::InputFull)
    );
    assert_eq!(state.observation().batch.as_ref(), Some(&resize));
    props.acknowledgement = Some(TerminalInputAcknowledgement {
        owner: Uuid::new_v4(),
        controller: resize.controller,
        sequence: resize.sequence,
        accepted: true,
    });
    state.apply(&props);
    assert_eq!(state.observation().batch.as_ref(), Some(&resize));
    acknowledge(&mut state, &mut props, true);
    assert_eq!(
        state.observation().batch.as_ref().unwrap().actions,
        vec![InputAction::Text("A".repeat(32000))]
    );
    let first = state.observation().batch.clone();
    state.apply(&props);
    assert_eq!(state.observation().batch, first); // duplicate resize receipt
    acknowledge(&mut state, &mut props, true);
    assert_eq!(
        state.observation().batch.as_ref().unwrap().actions,
        vec![InputAction::Text("B".repeat(32000))]
    );
    acknowledge(&mut state, &mut props, true);
    assert!(state.observation().batch.is_none());
    assert_eq!(state.queued_bytes, 0);
}
#[test]
fn hidden_owner_retains_pending_work_but_cannot_publish_new_input_or_replay_failure() {
    let mut state = State::new(&NativeBudget::default());
    let mut props = props();
    state.apply(&props);
    state.presentation(true);
    state.capture(true);
    state.write(InputAction::Text("first".into()));
    state.write(InputAction::Text("second".into()));
    state.presentation(false);
    acknowledge(&mut state, &mut props, true);
    assert!(state.observation().batch.is_none());
    assert!(!state.captured());
    state.write(InputAction::Text("hidden".into()));
    state.presentation(true);
    assert!(state.observation().batch.is_none());
    state.resize(80, 24);
    assert_eq!(
        state.observation().batch.as_ref().unwrap().size,
        Some(TerminalSize::new(80, 24).unwrap())
    );
    acknowledge(&mut state, &mut props, true);
    assert_eq!(
        state.observation().batch.as_ref().unwrap().actions,
        vec![InputAction::Text("second".into())]
    );
    acknowledge(&mut state, &mut props, false);
    state.capture(true);
    state.write(InputAction::Text("never".into()));
    assert_eq!(
        state.observation().error,
        Some(TerminalViewError::InputFailed)
    );
    assert!(state.observation().batch.is_none());
    let old_ack = props.acknowledgement.clone();
    props.controller = Some(Uuid::new_v4());
    state.apply(&props);
    state.presentation(true);
    state.resize(40, 3);
    state.capture(true);
    state.paste("ok\u{1b}[201~\r\nnext");
    assert_eq!(
        state.observation().batch.as_ref().unwrap().actions,
        vec![InputAction::Paste("ok[201~\nnext".into())]
    );
    props.acknowledgement = old_ack;
    state.apply(&props);
    assert!(state.observation().batch.is_some());
}
#[test]
fn canonical_scrollback_and_redaction_never_replay_raw_text_and_budget_follows_owner() {
    let budget = NativeBudget::default();
    let mut state = State::new(&budget);
    let mut props = props();
    state.apply(&props);
    state.presentation(true);
    state.capture(true);
    state.scroll_by(3);
    assert_eq!(state.text(3), "one\ntwo\nthree");
    if let Some(ShellOutput::Pty { redacted, .. }) = &mut props.output {
        *redacted = true;
    }
    state.apply(&props);
    assert!(!state.captured());
    assert!(state.text(3).is_empty());
    assert_eq!(budget.usage().0, 384 * 1024);
    drop(state);
    assert_eq!(budget.usage(), (0, 0, 0));
    let exhausted = NativeBudget::new(crate::budget::BudgetLimits {
        encoded_bytes: 1,
        pixel_bytes: 1,
        ..Default::default()
    });
    let mut state = State::new(&exhausted);
    state.apply(&props);
    assert_eq!(
        state.observation().error,
        Some(TerminalViewError::BudgetExhausted)
    );
}

#[test]
fn oversized_paste_stays_one_original_batch_until_its_exact_ack() {
    let mut state = State::new(&NativeBudget::default());
    let mut props = props();
    state.apply(&props);
    state.presentation(true);
    state.capture(true);
    state.resize(80, 24);
    let resize = state.observation().batch.clone().unwrap();
    let text = format!("{}\nlast", "中🙂".repeat(12_000));
    assert!(text.len() > 64 * 1024);
    state.paste(&text);
    assert!(state.queued_bytes > 64 * 1024);
    assert_eq!(state.pending_bytes, 0);
    assert_eq!(state.observation().batch.as_ref(), Some(&resize));
    state.write(InputAction::Text("must retry after ACK".into()));
    assert_eq!(
        state.observation().error,
        Some(TerminalViewError::InputFull)
    );
    acknowledge(&mut state, &mut props, true);
    let paste = state.observation().batch.clone().unwrap();
    assert_eq!(paste.sequence, resize.sequence + 1);
    assert_eq!(paste.actions, vec![InputAction::Paste(text)]);
    assert_eq!(state.pending_bytes, state.queued_bytes);
    let pending = state.pending_bytes;
    state.apply(&props); // The preceding resize ACK cannot release the paste.
    assert_eq!(state.observation().batch.as_ref(), Some(&paste));
    assert_eq!(state.pending_bytes, pending);
    props.acknowledgement.as_mut().unwrap().sequence = paste.sequence + 1;
    state.apply(&props);
    assert_eq!(state.pending_bytes, pending);
    acknowledge(&mut state, &mut props, true);
    assert_eq!(state.queued_bytes, 0);
    assert_eq!(state.pending_bytes, 0);
    assert!(state.observation().batch.is_none());
    state.write(InputAction::Text("after original paste".into()));
    let after = state.observation().batch.clone().unwrap();
    assert_eq!(after.sequence, paste.sequence + 1);
    assert_eq!(
        after.actions,
        vec![InputAction::Text("after original paste".into())]
    );
    state.apply(&props); // Duplicate paste ACK must not consume the later action.
    assert_eq!(state.observation().batch.as_ref(), Some(&after));
    acknowledge(&mut state, &mut props, false);
    state.apply(&props);
    assert!(state.observation().batch.is_none());
    assert!(state.queue.is_empty());
    assert_eq!(state.queued_bytes, 0);
    assert_eq!(state.pending_bytes, 0);
    assert_eq!(
        state.observation().error,
        Some(TerminalViewError::InputFailed)
    );
}
