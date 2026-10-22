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

// The messages waiting in a task while its turn runs. A message sent then is
// queued by the Host (`agent.send` answers `queued`) and starts the next turn
// when this one ends; Send now hands it to the running turn, which takes it
// at its next step. The watch delivers the queue (the transcript keeps it),
// `agent.queueMessage` reads one message whole for editing and
// `agent.editQueue` changes the queue, as terminal-shell's QueueDock does
// (products/maka/terminal-shell/app/QueueDock.tsx).

import { div } from 'gpui-kit';
import { v_flex, h_flex, Textarea, TextareaState } from 'gpui-base';
import { query, operation, log } from './host.js';
import {
  palette,
  typeset,
  text,
  icon,
  iconButton,
  ghostButton,
  controlButton,
  focusInput,
} from './theme.js';
import { t, tn, onLanguageChange } from './locale.js';

/** Why the Host did not change the queue (`AgentQueueRejection`), in words. */
const REJECTIONS = {
  closed: 'queueClosed',
  missing: 'queueMissing',
  in_flight: 'queueMissing',
  invalid_edit: 'queueInvalidEdit',
  execution_changed: 'queueExecutionChanged',
  not_running: 'queueNotRunning',
};

export class QueueDock {
  /**
   * Keeps a focus handle, so it belongs in the view's `init`.
   * @param {{ changed: () => void, giveBack: () => void }} hooks `giveBack`
   *   returns the keyboard to the composer when editing ends
   * @param {() => import("./transcript.js").Transcript | null} target the conversation shown
   */
  constructor(hooks, target, cx) {
    this.hooks = hooks;
    this.target = target;
    this.cx = cx;
    /**
     * sessionId -> the message being edited in that task: `{ messageId,
     * revision, original, hasAttachments, field, empty, focused, note,
     * unsubscribe }`. `revision` is the queue's when the message was read and
     * `original` its text then; `field` holds the edit, and `note` says what
     * the last save could not do. Each task keeps its own until it is saved
     * or cancelled.
     */
    this.edits = new Map();
    /** The session whose queue is being read or changed, or null. */
    this.busy = null;
    /**
     * What the last action could not do, `{ sessionId, revision, message }`:
     * shown under that task's queue while the queue stays at `revision`.
     */
    this.problem = null;
    /** Around the field: what the keyboard is moved from to reach it (focusInput). */
    this.fieldFocus = cx.focus_handle();
  }

  /** The queued messages `transcript` lists: not the one starting the turn now. */
  entries(transcript) {
    return transcript.queue.entries.filter((entry) => entry.kind !== 'in_flight');
  }

  /** The message of `transcript` being edited, or null. */
  edited(transcript) {
    return this.edits.get(transcript.id) ?? null;
  }

  /** What the last action in `transcript`'s queue could not do, while the queue is as it was then. */
  problemFor(transcript) {
    const problem = this.problem;
    return problem !== null &&
      problem.sessionId === transcript.id &&
      problem.revision === transcript.queue.revision
      ? problem.message
      : null;
  }

  /** Whether Send now can hand `entry` to the running turn; the Host refuses it for an executor's. */
  canSendNow(transcript, entry) {
    return (
      entry.kind === 'followup' &&
      transcript.run !== null &&
      transcript.backend?.kind !== 'executor'
    );
  }

  fail(transcript, message) {
    this.problem = { sessionId: transcript.id, revision: transcript.queue.revision, message };
    log(`queue: ${message}`);
  }

  begin(transcript) {
    this.busy = transcript.id;
    this.problem = null;
    this.hooks.changed();
  }

  /** Reads the queue again, so a change shows without waiting for the watch. */
  async refresh(transcript) {
    const queue = await query(transcript.endpoint, 'queue', { sessionId: transcript.id });
    transcript.acceptQueue(queue);
    return queue;
  }

  /** Reads the queue again where the watch brings it too: a failed read is only logged. */
  async settle(transcript) {
    try {
      await this.refresh(transcript);
    } catch (error) {
      log(`queue: reading the queue again failed: ${error.message}`);
    }
  }

  /**
   * Opens a queued message for editing, with its whole text: the queue lists
   * only the start of it. A queue that changed meanwhile is read again, and
   * a message a turn took meanwhile is not opened.
   */
  async edit(transcript, entry) {
    if (this.busy !== null || this.edited(transcript) !== null) return;
    this.begin(transcript);
    try {
      let revision = transcript.queue.revision;
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const result = await query(transcript.endpoint, 'queueMessage', {
          sessionId: transcript.id,
          messageId: entry.messageId,
          expectedRevision: revision,
        });
        if (result.kind === 'revision_conflict') {
          revision = result.actual;
          continue;
        }
        if (result.kind === 'missing' || result.revision !== transcript.queue.revision) {
          await this.settle(transcript);
        }
        if (
          result.kind === 'missing' ||
          !this.entries(transcript).some((other) => other.messageId === entry.messageId)
        ) {
          this.fail(transcript, t('queueMissing'));
          return;
        }
        this.open(transcript, entry.messageId, result);
        return;
      }
      await this.settle(transcript);
      this.fail(transcript, t('queueChanged'));
    } catch (error) {
      this.fail(transcript, t('queueFailed', { value: error.message }));
    } finally {
      this.busy = null;
      this.hooks.changed();
    }
  }

  /** Shows the message read (`queueMessage`'s `ready`) in a field of its own, with the keyboard. */
  open(transcript, messageId, read) {
    const sessionId = transcript.id;
    const field = TextareaState.new({ placeholder: t('queuedMessagePlaceholder'), rows: 1 });
    field.set_auto_grow(1, 6);
    field.set_value(read.message);
    const edit = {
      messageId,
      revision: read.revision,
      original: read.message,
      hasAttachments: read.hasAttachments,
      field,
      empty: read.message.trim() === '',
      focused: false,
      note: null,
      unsubscribe: onLanguageChange(() => field.set_placeholder(t('queuedMessagePlaceholder'))),
    };
    // Save turns on and off with the text; redraw only when that flips.
    field.on('change', () => {
      const empty = field.value().trim() === '';
      if (empty !== edit.empty) {
        edit.empty = empty;
        this.hooks.changed();
      }
    });
    // Enter saves, Shift+Enter inserts a newline, as in the composer;
    // Escape is handled around the field (queueEditor).
    field.set_submit_on_enter(true);
    field.on('submit', (event) => {
      const shown = this.target();
      if (!event.shift && shown?.id === sessionId) this.save(shown);
    });
    field.on('focus', () => {
      edit.focused = true;
    });
    field.on('blur', () => {
      edit.focused = false;
    });
    this.edits.set(sessionId, edit);
    log(`queue: editing ${messageId} at revision ${read.revision}`);
    this.cx.spawn(async (cx) => {
      await focusInput(
        this.fieldFocus,
        () => edit.focused,
        () => this.edits.get(sessionId) === edit,
        cx,
      );
    });
  }

  /**
   * Saves the edited text. When another change to the queue came first and
   * left this message as it was read, it is saved at the queue's new
   * revision. When this message itself changed, the edit is kept and its
   * note says so: saving again replaces that version.
   */
  async save(transcript) {
    const edit = this.edited(transcript);
    if (edit === null || this.busy !== null) return;
    const message = edit.field.value();
    if (message.trim() === '' && !edit.hasAttachments) return;
    if (message === edit.original) {
      this.close(transcript);
      return;
    }
    this.begin(transcript);
    edit.note = null;
    try {
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const result = await operation(transcript.endpoint, 'editQueue', {
          sessionId: transcript.id,
          expectedRevision: edit.revision,
          edit: { kind: 'update', messageId: edit.messageId, message },
        });
        log(`queue: update ${edit.messageId} ${result.kind}`);
        if (result.kind === 'committed') {
          this.close(transcript);
          await this.settle(transcript);
          return;
        }
        if (result.kind === 'rejected') {
          if (result.reason === 'missing' || result.reason === 'in_flight') {
            // The editor says the message left the queue once the queue shows it.
            await this.settle(transcript);
          } else {
            edit.note = t(REJECTIONS[result.reason] ?? 'queueRefused', { value: result.reason });
          }
          return;
        }
        const latest = await query(transcript.endpoint, 'queueMessage', {
          sessionId: transcript.id,
          messageId: edit.messageId,
          expectedRevision: result.actual,
        });
        if (latest.kind === 'missing') {
          await this.settle(transcript);
          return;
        }
        if (latest.kind !== 'ready') break;
        edit.revision = latest.revision;
        edit.hasAttachments = latest.hasAttachments;
        if (latest.message !== edit.original) {
          edit.original = latest.message;
          edit.note = t('queueEditedElsewhere');
          return;
        }
      }
      edit.note = t('queueChanged');
    } catch (error) {
      edit.note = t('queueFailed', { value: error.message });
    } finally {
      this.busy = null;
      this.hooks.changed();
    }
  }

  /** Leaves the queued message as it is (Cancel, Escape). */
  cancel(transcript) {
    if (this.busy === transcript.id) return;
    this.close(transcript);
  }

  /** Ends editing in `transcript`; the keyboard goes back to the composer when it was in the field. */
  close(transcript) {
    const edit = this.edited(transcript);
    if (edit === null) return;
    this.edits.delete(transcript.id);
    edit.unsubscribe();
    edit.field.release();
    this.hooks.changed();
    if (edit.focused) this.hooks.giveBack();
  }

  /**
   * Before Stop, which drops the queue (the Host's `stop_run` retracts every
   * message): the whole text of each message waiting in `transcript`'s
   * queue, in order, with an open edit's text for its message, for the
   * composer to put back in the draft; and `done`, which ends that edit once
   * it has. A message that cannot be read is left out.
   */
  async takeBack(transcript) {
    const edit = this.edited(transcript);
    const texts = [];
    let revision = transcript.queue.revision;
    for (const entry of this.entries(transcript)) {
      if (edit?.messageId === entry.messageId) {
        texts.push(edit.field.value());
        continue;
      }
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const result = await query(transcript.endpoint, 'queueMessage', {
          sessionId: transcript.id,
          messageId: entry.messageId,
          expectedRevision: revision,
        });
        if (result.kind === 'revision_conflict') {
          revision = result.actual;
          continue;
        }
        if (result.kind === 'ready') texts.push(result.message);
        break;
      }
    }
    return {
      texts: texts.filter((value) => value.trim() !== ''),
      done: () => {
        if (this.edited(transcript) === edit) this.close(transcript);
      },
    };
  }

  /** Removes a queued message. */
  remove(transcript, entry) {
    return this.change(transcript, entry, 'retract');
  }

  /** Hands a message waiting for the next turn to the running one (`promote`). */
  sendNow(transcript, entry) {
    if (!this.canSendNow(transcript, entry)) return Promise.resolve();
    return this.change(transcript, entry, 'promote');
  }

  /**
   * Removes or promotes a message. When another change to the queue came
   * first, the queue is read again, and the change made again only while
   * the message waits as its row showed it.
   */
  async change(transcript, entry, kind) {
    if (this.busy !== null || this.edited(transcript) !== null) return;
    this.begin(transcript);
    try {
      let revision = transcript.queue.revision;
      for (let attempt = 0; attempt < 2; attempt += 1) {
        const result = await operation(transcript.endpoint, 'editQueue', {
          sessionId: transcript.id,
          expectedRevision: revision,
          edit: { kind, messageId: entry.messageId },
        });
        log(`queue: ${kind} ${entry.messageId} ${result.kind}`);
        if (result.kind === 'committed') {
          await this.settle(transcript);
          return;
        }
        if (result.kind === 'rejected') {
          this.fail(
            transcript,
            t(REJECTIONS[result.reason] ?? 'queueRefused', { value: result.reason }),
          );
          return;
        }
        const queue = await this.refresh(transcript);
        const still = queue.entries.find((other) => other.messageId === entry.messageId);
        // Another window already handed it to the running turn.
        if (kind === 'promote' && still?.kind === 'steering') return;
        if (still === undefined || still.kind === 'in_flight') {
          this.fail(transcript, t('queueMissing'));
          return;
        }
        if (still.kind !== entry.kind || still.preview !== entry.preview) {
          this.fail(transcript, t('queueChanged'));
          return;
        }
        revision = queue.revision;
      }
      this.fail(transcript, t('queueChanged'));
    } catch (error) {
      this.fail(transcript, t('queueFailed', { value: error.message }));
    } finally {
      this.busy = null;
      this.hooks.changed();
    }
  }
}

/**
 * The queue above the composer: a header with the count, then one row per
 * message, its start and its actions, or, while one is edited, its whole
 * text in a field; and what the last action could not do. Null while
 * `transcript` has nothing queued or edited.
 */
export function queueTray(dock, transcript, cx) {
  if (transcript === null) return null;
  const entries = dock.entries(transcript);
  const edit = dock.edited(transcript);
  if (entries.length === 0 && edit === null) return null;
  const m = palette(cx);
  const problem = dock.problemFor(transcript);
  // Gone once the queue is known without it: a task opened again has no
  // queue until the watch brings one.
  const gone =
    edit !== null &&
    transcript.queue.revision >= 0 &&
    !entries.some((entry) => entry.messageId === edit.messageId);
  return v_flex()
    .mx(16)
    .px(8)
    .pt(4)
    .pb(18)
    .mb(-14)
    .rounded_t(16)
    .border_t_1()
    .border_x_1()
    .border_color(m.border)
    .bg(m.wash)
    .child(
      h_flex()
        .h(28)
        .px(4)
        .gap(6)
        .items_center()
        .child(icon('queue', 14, m.inkMuted))
        .child(
          text(
            entries.length === 0 ? t('queued') : tn(entries.length, 'queuedOne', 'queuedMany'),
            'smallLabel',
            m.inkMuted,
          ),
        ),
    )
    .child(
      edit !== null
        ? queueEditor(dock, transcript, edit, gone, cx)
        : v_flex()
            .id('queue-rows')
            .max_h(160)
            .overflow_y_scroll()
            .children(entries.map((entry) => queueRow(dock, transcript, entry, cx))),
    )
    .when(problem !== null, (el) =>
      el.child(typeset(div(), 'small').px(4).pt(2).text_color(m.warning).child(problem)),
    );
}

/** One queued message: its start, then Edit, Send now and Remove. */
function queueRow(dock, transcript, entry, cx) {
  const m = palette(cx);
  const idle = dock.busy === null;
  const steering = entry.kind === 'steering';
  const sendable = dock.canSendNow(transcript, entry);
  return h_flex()
    .h(32)
    .pl(4)
    .gap(2)
    .items_center()
    .child(
      h_flex()
        .flex_1()
        .min_w_0()
        .gap(8)
        .items_center()
        // Handed to the running turn: said apart from the message, which it is not part of.
        .when(steering, (el) => el.child(text(t('queuedSteering'), 'smallLabel', m.primary)))
        .child(
          typeset(div(), 'body')
            .flex_1()
            .min_w_0()
            .truncate()
            .text_color(m.ink)
            .child(entry.preview),
        ),
    )
    .child(
      rowButton(`queue-edit-${entry.messageId}`, t('edit'), !idle, cx).on_click(() =>
        dock.edit(transcript, entry),
      ),
    )
    .when(!steering, (el) =>
      el.child(
        rowButton(`queue-send-${entry.messageId}`, t('queueSendNow'), !idle || !sendable, cx)
          .tooltip(sendable ? t('queueSendNowHint') : t('queueSendNowIdle'))
          .on_click(() => dock.sendNow(transcript, entry)),
      ),
    )
    .child(
      iconButton(`queue-remove-${entry.messageId}`, 'close', cx, 28, 14)
        .disabled(!idle)
        .when(!idle, (el) => el.opacity(0.5))
        .tooltip(t('remove'))
        .on_click(() => dock.remove(transcript, entry)),
    );
}

/** A small text button in a queued message's row. */
function rowButton(id, label, disabled, cx) {
  const m = palette(cx);
  return typeset(ghostButton(id, cx), 'smallLabel')
    .h(28)
    .px(8)
    .text_color(m.inkMuted)
    .disabled(disabled)
    .when(disabled, (el) => el.opacity(0.5))
    .child(label);
}

/**
 * The message being edited: its whole text in a field, what the last save
 * could not do, then Cancel and Save. Once it no longer waits (`gone`: it
 * started a turn or was removed), the edit stays in the field, to copy, and
 * cannot be saved.
 */
function queueEditor(dock, transcript, edit, gone, cx) {
  const m = palette(cx);
  const busy = dock.busy === transcript.id;
  const note =
    edit.note ??
    (gone ? t('queueEditGone') : edit.hasAttachments ? t('queueAttachmentsKept') : null);
  const warning = edit.note !== null || gone;
  return v_flex()
    .mt(2)
    .mb(4)
    .p(8)
    .gap(6)
    .rounded(12)
    .bg(m.plate)
    .border_1()
    .border_color(m.borderStrong)
    .child(
      div()
        .w_full()
        .track_focus(dock.fieldFocus)
        .on_key_down((event) => {
          if (event.key === 'escape') dock.cancel(transcript);
        })
        .child(
          typeset(Textarea.new(edit.field), 'body')
            .w_full()
            .px(4)
            .bg('#00000000')
            .border_0()
            .text_color(m.ink)
            .disabled(busy),
        ),
    )
    .when(note !== null, (el) =>
      el.child(
        typeset(div(), 'small')
          .px(4)
          .text_color(warning ? m.warning : m.inkMuted)
          .child(note),
      ),
    )
    .child(
      h_flex()
        .gap(8)
        .justify_end()
        .child(
          controlButton('queue-edit-cancel', t('cancel'), 'quiet', busy, cx)
            .h(28)
            .on_click(() => dock.cancel(transcript)),
        )
        .child(
          controlButton(
            'queue-edit-save',
            t('save'),
            'primary',
            busy || gone || (edit.empty && !edit.hasAttachments),
            cx,
          )
            .h(28)
            .on_click(() => dock.save(transcript)),
        ),
    );
}
