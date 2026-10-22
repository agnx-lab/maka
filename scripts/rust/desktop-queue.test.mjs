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

import assert from 'node:assert/strict';
import { registerHooks } from 'node:module';
import test from 'node:test';

// The production queue dock and transcript run unchanged. Native input state
// and the Host boundary are supplied; the element builders record what they
// are given, so a drawn tray can be searched for its buttons and clicked.
const recorder = `const element = (kind) => (...args) => {
    const node = { kind, args, calls: [], children: [] };
    const proxy = new Proxy(node, {
      get(target, key) {
        if (key === 'node') return node;
        return (...given) => {
          if (key === 'when') return given[0] ? given[1](proxy) : proxy;
          if (key === 'child') node.children.push(given[0]);
          else if (key === 'children') node.children.push(...given[0]);
          else node.calls.push([key, ...given]);
          return proxy;
        };
      },
    });
    return proxy;
  };`;
const nativeModules = {
  'gpui-kit': `${recorder}
    const ui = element('div');
    export { ui as div, ui as list, ui as uniform_list, ui as PathBuilder };
    export const svg = element('svg');
    export class View {}`,
  'gpui-base': `${recorder}
    const ui = element('flex');
    const state = { new: () => ({
      text: '', released: false, listeners: {},
      on(name, listener) { this.listeners[name] = listener; },
      set_auto_grow() {}, set_submit_on_enter() {}, set_placeholder() {}, set_masked() {},
      value() { return this.text; }, set_value(text) { this.text = text; },
      type(text) { this.text = text; this.listeners.change?.(); },
      release() { this.released = true; return true; },
    }) };
    export { state as InputState, state as TextareaState };
    export const Button = { new: element('Button') };
    export const Textarea = { new: element('Textarea') };
    export { ui as v_flex, ui as h_flex, ui as TextView, ui as Scrollbar,
      ui as set_theme, ui as Input, ui as Checkbox, ui as Switch,
      ui as Popover, ui as h_resizable, ui as resizable_panel };`,
  'maka-locale': `export const system = () => 'en';`,
  'maka-files': `const unexpected = () => { throw new Error('unexpected native file call'); };
    export { unexpected as openText, unexpected as chooseFolder, unexpected as chooseFiles,
      unexpected as release, unexpected as open, unexpected as reveal };`,
  'maka-host': `export const query = (...args) => globalThis[Symbol.for('queue.test.query')](...args);
    export const mutate = (...args) => globalThis[Symbol.for('queue.test.mutate')](...args);
    const unexpected = () => { throw new Error('unexpected native subscription'); };
    export { unexpected as subscribe, unexpected as next, unexpected as close };`,
};
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    if (Object.hasOwn(nativeModules, specifier)) {
      return { url: `queue-test:${specifier}`, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (url.startsWith('queue-test:')) {
      return { format: 'module', source: nativeModules[url.slice(11)], shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});
const { QueueDock, queueTray } = await import('../../products/maka/desktop-shell/app/queue.js');
const { Transcript } = await import('../../products/maka/desktop-shell/app/transcript.js');
const { t } = await import('../../products/maka/desktop-shell/app/locale.js');
hooks.deregister();

const cx = { theme: () => ({ is_dark: false }) };

function returned(value) {
  return { receipt: { kind: 'recorded', outcome: { kind: 'returned', value } } };
}

function entry(messageId, kind = 'followup', preview = messageId) {
  return { messageId, kind, preview };
}

function transcriptOf(id, queue) {
  const transcript = new Transcript({ id, title: id }, { changed() {}, report() {} }, 'agent');
  transcript.run = { invocationId: 'run', turnId: 'turn', state: 'running' };
  if (queue !== undefined) transcript.acceptQueue(structuredClone(queue));
  return transcript;
}

/**
 * A dock and a running task whose queue the Host holds in `host.queue` and
 * whose messages in `host.messages`. `editQueue` answers from `answers`, in
 * order, then commits each change to `host.queue`.
 */
function setup({ entries = [entry('a'), entry('b')], answers = [] } = {}) {
  const host = {
    queue: { revision: 4, entries },
    messages: new Map(entries.map((row) => [row.messageId, `${row.messageId} in full`])),
    calls: [],
  };
  const transcript = transcriptOf('task', host.queue);
  let shown = transcript;
  globalThis[Symbol.for('queue.test.query')] = async (endpoint, method, input) => {
    host.calls.push([method, structuredClone(input)]);
    if (method === 'queue') return structuredClone(host.queue);
    assert.equal(method, 'queueMessage');
    if (input.expectedRevision !== host.queue.revision) {
      return {
        kind: 'revision_conflict',
        expected: input.expectedRevision,
        actual: host.queue.revision,
      };
    }
    const message = host.messages.get(input.messageId);
    return message === undefined
      ? { kind: 'missing' }
      : { kind: 'ready', revision: host.queue.revision, hasAttachments: false, message };
  };
  globalThis[Symbol.for('queue.test.mutate')] = async (endpoint, method, input) => {
    assert.equal(method, 'editQueue');
    host.calls.push([method, structuredClone(input)]);
    const answer = answers.shift();
    if (answer !== undefined) return returned(answer(host, input));
    if (input.expectedRevision !== host.queue.revision) {
      return returned({
        kind: 'revision_conflict',
        expected: input.expectedRevision,
        actual: host.queue.revision,
      });
    }
    const { edit } = input;
    const rows = host.queue.entries;
    if (edit.kind === 'retract') {
      host.queue.entries = rows.filter((row) => row.messageId !== edit.messageId);
      host.messages.delete(edit.messageId);
    } else if (edit.kind === 'promote') {
      host.queue.entries = rows.map((row) =>
        row.messageId === edit.messageId ? { ...row, kind: 'steering' } : row,
      );
    } else if (edit.kind === 'update') {
      host.messages.set(edit.messageId, edit.message);
      host.queue.entries = rows.map((row) =>
        row.messageId === edit.messageId ? { ...row, preview: edit.message.slice(0, 80) } : row,
      );
    }
    host.queue.revision += 1;
    return returned({ kind: 'committed', revision: host.queue.revision });
  };
  const given = [];
  const dock = new QueueDock({ changed() {}, giveBack: () => given.push(1) }, () => shown, {
    focus_handle: () => ({}),
    spawn() {},
  });
  return {
    dock,
    transcript,
    host,
    given,
    show(other) {
      shown = other;
    },
  };
}

/** Another window's change: the queue moves on without this one hearing of it yet. */
function elsewhere(host, change) {
  change(host);
  host.queue.revision += 1;
}

const edits = (host) => host.calls.filter(([method]) => method === 'editQueue');

/** The nodes of a drawn tree, depth first. */
function nodes(tree) {
  const found = [];
  const walk = (value) => {
    const node = value?.node;
    if (node === undefined) return;
    found.push(node);
    for (const child of node.children) walk(child);
  };
  walk(tree);
  return found;
}
const calls = (node, name) => node.calls.filter(([key]) => key === name).map(([, ...args]) => args);
const button = (tree, id) =>
  nodes(tree).find((node) => node.kind === 'Button' && node.args[0] === id);
const disabled = (node) => calls(node, 'disabled').at(-1)?.[0] === true;
const click = (node) => calls(node, 'on_click').at(-1)[0]();
const words = (tree) =>
  nodes(tree).flatMap((node) => node.children.filter((child) => typeof child === 'string'));

test('the watch delivers the queue; an older revision is not taken', () => {
  const { transcript } = setup();
  transcript.deliver({ kind: 'queue', queue: { revision: 6, entries: [entry('c')] } });
  transcript.deliver({ kind: 'queue', queue: { revision: 5, entries: [entry('a')] } });
  assert.deepEqual(
    transcript.queue.entries.map((row) => row.messageId),
    ['c'],
  );
});

test('the tray lists the waiting messages, not the one starting the turn', () => {
  const { dock, transcript } = setup({ entries: [entry('now', 'in_flight'), entry('later')] });
  const tray = queueTray(dock, transcript, cx);
  assert.ok(words(tray).includes('later'));
  assert.ok(button(tray, 'queue-edit-later'));
  assert.equal(button(tray, 'queue-edit-now'), undefined);
  transcript.acceptQueue({ revision: 9, entries: [entry('now', 'in_flight')] });
  assert.equal(queueTray(dock, transcript, cx), null);
});

test('Remove takes the message out and shows the new queue without waiting for the watch', async () => {
  const { dock, transcript, host } = setup();
  await click(button(queueTray(dock, transcript, cx), 'queue-remove-a'));
  assert.deepEqual(edits(host)[0][1], {
    sessionId: 'task',
    expectedRevision: 4,
    edit: { kind: 'retract', messageId: 'a' },
  });
  assert.equal(transcript.queue.revision, 5);
  assert.deepEqual(
    transcript.queue.entries.map((row) => row.messageId),
    ['b'],
  );
});

test('a change crossing another one is made again only while the message waits as shown', async () => {
  const { dock, transcript, host } = setup();
  // Another window removed a; b is as the row shows it.
  elsewhere(host, (h) => {
    h.queue.entries = h.queue.entries.filter((row) => row.messageId !== 'a');
  });
  await dock.remove(transcript, transcript.queue.entries[1]);
  assert.deepEqual(
    edits(host).map(([, input]) => input.expectedRevision),
    [4, 5],
  );
  assert.deepEqual(transcript.queue.entries, []);

  // Another window rewrote a: what the row showed is not what would be removed.
  const changed = setup();
  elsewhere(changed.host, (h) => {
    h.queue.entries = h.queue.entries.map((row) =>
      row.messageId === 'a' ? { ...row, preview: 'a, rewritten' } : row,
    );
  });
  await changed.dock.remove(changed.transcript, changed.transcript.queue.entries[0]);
  assert.equal(edits(changed.host).length, 1);
  assert.ok(changed.host.queue.entries.some((row) => row.messageId === 'a'));
  assert.ok(words(queueTray(changed.dock, changed.transcript, cx)).includes(t('queueChanged')));

  // A message gone meanwhile: the tray says so until the queue moves on.
  const gone = setup();
  elsewhere(gone.host, (h) => {
    h.queue.entries = h.queue.entries.filter((row) => row.messageId !== 'a');
  });
  await gone.dock.remove(gone.transcript, gone.transcript.queue.entries[0]);
  assert.equal(edits(gone.host).length, 1);
  assert.ok(words(queueTray(gone.dock, gone.transcript, cx)).includes(t('queueMissing')));
  gone.transcript.acceptQueue({ revision: 9, entries: [entry('b')] });
  assert.ok(!words(queueTray(gone.dock, gone.transcript, cx)).includes(t('queueMissing')));
});

test('Send now hands a waiting message to the running turn, and is offered only then', async () => {
  const { dock, transcript, host } = setup();
  const tray = queueTray(dock, transcript, cx);
  assert.equal(disabled(button(tray, 'queue-send-a')), false);
  await click(button(tray, 'queue-send-a'));
  assert.deepEqual(edits(host)[0][1].edit, { kind: 'promote', messageId: 'a' });
  assert.equal(transcript.queue.entries[0].kind, 'steering');
  // Given to the turn: the row says so and offers it no more.
  const after = queueTray(dock, transcript, cx);
  assert.ok(words(after).includes(t('queuedSteering')));
  assert.equal(button(after, 'queue-send-a'), undefined);
  // No turn running, or an executor's, which the Host refuses it for.
  transcript.run = null;
  assert.equal(disabled(button(queueTray(dock, transcript, cx), 'queue-send-b')), true);
  transcript.run = { invocationId: 'run', turnId: 'turn', state: 'running' };
  transcript.backend = { kind: 'executor', executor: 'codex' };
  assert.equal(disabled(button(queueTray(dock, transcript, cx), 'queue-send-b')), true);
  await dock.sendNow(transcript, transcript.queue.entries[1]);
  assert.equal(edits(host).length, 1);

  // Another window gave it to the turn first: that is what was asked for.
  const second = setup();
  elsewhere(second.host, (h) => {
    h.queue.entries = h.queue.entries.map((row) => ({ ...row, kind: 'steering' }));
  });
  await second.dock.sendNow(second.transcript, second.transcript.queue.entries[0]);
  assert.equal(edits(second.host).length, 1);
  assert.equal(second.dock.problemFor(second.transcript), null);
});

test('a refused change is said in the tray, in words', async () => {
  const { dock, transcript } = setup({
    answers: [() => ({ kind: 'rejected', reason: 'execution_changed' })],
  });
  await dock.sendNow(transcript, transcript.queue.entries[0]);
  assert.ok(words(queueTray(dock, transcript, cx)).includes(t('queueExecutionChanged')));
  assert.equal(dock.busy, null);
});

test('Edit puts the whole message in a field in place of the rows, and saves it', async () => {
  const { dock, transcript, host, given } = setup();
  await click(button(queueTray(dock, transcript, cx), 'queue-edit-b'));
  const edit = dock.edited(transcript);
  assert.equal(edit.field.value(), 'b in full');
  let tray = queueTray(dock, transcript, cx);
  assert.equal(button(tray, 'queue-edit-a'), undefined);
  assert.equal(disabled(button(tray, 'queue-edit-save')), false);
  edit.field.type('  ');
  assert.equal(disabled(button(queueTray(dock, transcript, cx), 'queue-edit-save')), true);

  // Saving the text as it was changes nothing.
  edit.field.type('b in full');
  await dock.save(transcript);
  assert.equal(dock.edited(transcript), null);
  assert.equal(edit.field.released, true);
  assert.equal(edits(host).length, 0);

  await dock.edit(transcript, transcript.queue.entries[1]);
  dock.edited(transcript).field.type('b, shorter');
  tray = queueTray(dock, transcript, cx);
  await click(button(tray, 'queue-edit-save'));
  assert.deepEqual(edits(host)[0][1], {
    sessionId: 'task',
    expectedRevision: 4,
    edit: { kind: 'update', messageId: 'b', message: 'b, shorter' },
  });
  assert.equal(dock.edited(transcript), null);
  assert.equal(transcript.queue.entries[1].preview, 'b, shorter');
  assert.equal(given.length, 0);
});

test('an edit crossing another change is saved when its message is as it was read', async () => {
  const { dock, transcript, host } = setup();
  await dock.edit(transcript, transcript.queue.entries[1]);
  dock.edited(transcript).field.type('b, mine');
  // The turn took a: the queue moved on, b did not change.
  elsewhere(host, (h) => {
    h.queue.entries = h.queue.entries.filter((row) => row.messageId !== 'a');
  });
  await dock.save(transcript);
  assert.equal(host.messages.get('b'), 'b, mine');
  assert.equal(dock.edited(transcript), null);
});

test('an edit of a message changed elsewhere is kept with a note; saving again replaces it', async () => {
  const { dock, transcript, host } = setup();
  await dock.edit(transcript, transcript.queue.entries[1]);
  const edit = dock.edited(transcript);
  edit.field.type('b, mine');
  elsewhere(host, (h) => h.messages.set('b', 'b, theirs'));
  await dock.save(transcript);
  assert.equal(host.messages.get('b'), 'b, theirs');
  assert.equal(edit.field.value(), 'b, mine');
  assert.ok(words(queueTray(dock, transcript, cx)).includes(t('queueEditedElsewhere')));
  // Their version is now the one read: a later change to the queue alone
  // does not stop the second save.
  elsewhere(host, (h) => {
    h.queue.entries = h.queue.entries.filter((row) => row.messageId !== 'a');
  });
  await dock.save(transcript);
  assert.equal(host.messages.get('b'), 'b, mine');
  assert.equal(dock.edited(transcript), null);
});

test('an edit whose message left the queue stays to copy and cannot be saved', async () => {
  const { dock, transcript, host } = setup();
  await dock.edit(transcript, transcript.queue.entries[0]);
  const edit = dock.edited(transcript);
  edit.field.type('a, mine');
  elsewhere(host, (h) => {
    h.queue.entries = h.queue.entries.filter((row) => row.messageId !== 'a');
    h.messages.delete('a');
  });
  await dock.save(transcript);
  const tray = queueTray(dock, transcript, cx);
  assert.ok(words(tray).includes(t('queueEditGone')));
  assert.equal(disabled(button(tray, 'queue-edit-save')), true);
  assert.equal(edit.field.value(), 'a, mine');
  click(button(tray, 'queue-edit-cancel'));
  assert.equal(dock.edited(transcript), null);
  assert.equal(edit.field.released, true);
});

test('a message the turn took meanwhile is not opened for editing', async () => {
  const { dock, transcript, host } = setup();
  elsewhere(host, (h) => {
    h.queue.entries = h.queue.entries.map((row) =>
      row.messageId === 'a' ? { ...row, kind: 'in_flight' } : row,
    );
  });
  await dock.edit(transcript, transcript.queue.entries[0]);
  assert.equal(dock.edited(transcript), null);
  assert.ok(words(queueTray(dock, transcript, cx)).includes(t('queueMissing')));
});

test('each task keeps its own edit, and a task opened again shows it before its queue comes', async () => {
  const { dock, transcript, host, show } = setup();
  await dock.edit(transcript, transcript.queue.entries[0]);
  dock.edited(transcript).field.type('a, mine');
  const other = transcriptOf('other', host.queue);
  show(other);
  await dock.edit(other, other.queue.entries[0]);
  dock.edited(other).field.type('a, elsewhere');
  assert.equal(dock.edited(transcript).field.value(), 'a, mine');

  // Opened again, the task has a new transcript with no queue yet.
  const again = transcriptOf('task');
  show(again);
  const tray = queueTray(dock, again, cx);
  assert.ok(!words(tray).includes(t('queueEditGone')));
  assert.equal(disabled(button(tray, 'queue-edit-save')), false);
});

test('before Stop, the queued messages are read whole, with an open edit as it stands', async () => {
  const { dock, transcript } = setup({ entries: [entry('a'), entry('b'), entry('c')] });
  await dock.edit(transcript, transcript.queue.entries[1]);
  dock.edited(transcript).field.type('b, mine');
  const back = await dock.takeBack(transcript);
  assert.deepEqual(back.texts, ['a in full', 'b, mine', 'c in full']);
  assert.notEqual(dock.edited(transcript), null);
  back.done();
  assert.equal(dock.edited(transcript), null);
});
