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
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
import Ajv from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'dark' }).Appearance.value;
const ajv = new Ajv({
  strict: false,
  formats: { uint64: true, uint32: true, uint16: true, uint: true },
});
addFormats(ajv);
const manifest = JSON.parse(
  readFileSync(new URL('../dist/packages/composer/maka.extension.json', import.meta.url)),
);
const protocol = manifest.application.entries.tui.exports.Composer.contract.interface;
const validProps = ajv.compile(protocol.props);
const validEvents = Object.fromEntries(
  Object.entries(protocol.events).map(([name, schema]) => [name, ajv.compile(schema)]),
);
const dataSource = {
  stateDomain: 'a'.repeat(64),
  deployment: '00000000-0000-0000-0000-000000000001',
  namespace: ['00000000-0000-0000-0000-000000000002'],
};
const backend = {
  kind: 'model',
  model: { connectionId: 'original', revision: 1, modelId: 'first' },
  thinkingLevel: null,
};
const reference = {
  source: { stateDomain: dataSource.stateDomain, deployment: dataSource.deployment, revision: 1 },
  namespace: dataSource.namespace,
  export: 'agent',
  method: 'send',
  operation: '00000000-0000-0000-0000-000000000003',
  fingerprint: `sha256-${'b'.repeat(64)}`,
  contract: {
    name: 'maka.agent.Agent',
    version: 1,
    transport: 'remote',
    digest: `sha256-${'c'.repeat(64)}`,
  },
  recovery: 'journal_v1',
};
const caller = {
  cursor: 42,
  caller: { ...reference.source, deployment: '00000000-0000-0000-0000-000000000004' },
  consumer: ['00000000-0000-0000-0000-000000000005'],
  reference,
};
const scope = JSON.stringify([
  caller.caller.stateDomain,
  caller.caller.deployment,
  caller.consumer,
]);
const returned = (
  value = {
    kind: 'started',
    sessionId: 'session',
    invocationId: 'run',
    messageId: 'canonical-message',
  },
) => ({
  reference,
  receipt: {
    kind: 'recorded',
    outcome: { kind: 'returned', value },
    cleanup: { kind: 'confirmed' },
  },
});
const observed = (text = 'hello', revision = 1, owner = 'editor') => ({
  revision,
  snapshot: {
    text: { owner, revision, bytes: Buffer.byteLength(text) },
    selection: { start: text.length, end: text.length },
    reversed: false,
    marked: null,
  },
  receipt: null,
});
function slot(node, key = 'draft') {
  if (!node || typeof node !== 'object') return;
  if (node.key === key && node.binding === 'editor') return node;
  return Object.values(node)
    .flatMap((value) => (Array.isArray(value) ? value : [value]))
    .map((child) => slot(child, key))
    .find(Boolean);
}
function findNode(node, key) {
  if (!node || typeof node !== 'object') return;
  if (node.key === key) return node;
  return Object.values(node)
    .flatMap((value) => (Array.isArray(value) ? value : [value]))
    .map((child) => findNode(child, key))
    .find(Boolean);
}
async function client(target) {
  const { component } = await import(`../dist/packages/composer/${target}.js`);
  const props = {
    commands: [
      { id: 'compact-open', command: 'compact', label: '压缩上下文', disabled: false },
      { id: 'create', command: 'new', label: 'New conversation', disabled: false },
      { id: 'settings', command: 'settings', label: 'Settings', disabled: false },
    ],
    backend: structuredClone(backend),
    context: 'session',
    dataSource,
    attachments: [],
    filePaste: null,
    fileCommand: null,
    autoFocus: true,
    theme,
    locale: 'en',
    busy: false,
    running: false,
    enabled: true,
  };
  assert.equal(validProps(props), true, JSON.stringify(validProps.errors));
  let out;
  const accept = (result) => {
    out = structuredClone(result);
    for (const event of out.events ?? [])
      assert.equal(
        validEvents[event.name](event.value),
        true,
        JSON.stringify(validEvents[event.name].errors),
      );
    return out;
  };
  accept(component.mount(props));
  return {
    props,
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    editor(key) {
      return slot(out.frame.view, key).props;
    },
    update() {
      return accept(component.update(out.frame.state, props));
    },
    event(input) {
      return accept(component.event(out.frame.state, input, props));
    },
    emit(name, event, child = 'draft') {
      return this.event({ kind: 'component', child, name, key: name, event });
    },
    raw(key) {
      return this.event({ kind: 'raw', key, event: {} });
    },
    reply(event, key = out.actions[0].key) {
      return this.event({ kind: 'action', key, event });
    },
    ready(text = 'hello') {
      this.emit('checkpoint', {
        owner: 'editor',
        state: { kind: 'ready', initialized: true, saved: true },
      });
      this.emit('changed', observed(text));
    },
    send(text = 'hello') {
      this.emit('submitted', observed(text));
      return this.reply({
        status: 'succeeded',
        value: { metadata: observed(text).snapshot.text, text },
      });
    },
    clear(status = 'applied') {
      const command = this.editor().command;
      const value = observed(status === 'applied' ? '' : 'new text', 2);
      value.receipt = {
        owner: command.owner,
        sequence: command.sequence,
        revision: 2,
        outcome: status === 'applied' ? { status } : { status, reason: 'revision' },
      };
      return this.emit('changed', value);
    },
  };
}
for (const target of ['tui']) {
  test(`${target} sends from its native snapshot without a parent callback and freezes the original destination`, async () => {
    const c = await client(target);
    c.ready();
    for (const saved of [false, true, false, true]) {
      c.emit('checkpoint', { owner: 'editor', state: { kind: 'ready', initialized: true, saved } });
      assert.deepEqual(c.out.events, []);
    }
    c.emit('changed', observed());
    assert.deepEqual(c.out.events, []);
    c.emit('submitted', observed());
    const read = c.out.actions[0];
    assert.equal(read.binding, 'reader');
    assert.equal(read.resources.text.child, 'draft');
    assert.deepEqual(c.out.events, []);
    c.emit('submitted', observed());
    assert.deepEqual(c.out.actions, [], 'duplicate Enter cannot start another capture');
    c.props.context = 'another-session';
    c.props.backend = { kind: 'executor', selection: { executor: 'new', settings: {} } };
    c.update();
    c.reply(
      { status: 'succeeded', value: { metadata: observed().snapshot.text, text: 'hello' } },
      read.key,
    );
    const send = c.out.actions[0];
    assert.equal(send.binding, 'agent');
    assert.equal(send.method, 'send');
    assert.deepEqual(send.input, {
      sessionId: 'session',
      message: 'hello',
      attachments: [],
      model: backend.model,
      thinkingLevel: null,
    });
    assert.equal(c.out.events[0].name, 'submission');
    assert.deepEqual(c.out.events[0].value, {
      context: 'session',
      dataSource,
      attachments: [],
      key: send.key,
      text: 'hello',
      status: 'pending',
      messageId: null,
    });
    assert.deepEqual(send.recovery, {
      format: 'maka.agent.draft-send@2',
      sessionId: 'session',
      editor: { owner: 'editor', revision: 1 },
      attachments: [],
    });
    c.reply({ status: 'operation', reply: returned(), caller });
    assert.equal(c.out.events[0].value.status, 'accepted');
    assert.equal(c.out.events[0].value.messageId, 'canonical-message');
    assert.deepEqual(
      c.out.events.filter((event) => event.name === 'sent'),
      [{ name: 'sent', value: { context: 'session', dataSource, attachments: [] } }],
    );
    assert.equal(c.editor().command.owner, 'editor');
    assert.equal(
      c.editor().persistence.metadata.historyAfter,
      0,
      'admission alone cannot advance a draft past an unapplied clear',
    );
    c.clear();
    assert.deepEqual(c.out.events, []);
    assert.equal(c.editor().persistence.metadata.historyAfter, 42);
    assert.equal(c.editor().persistence.metadata.historyScope, scope);
  });
  test(`${target} retains text on native conflicts, rejected capture and uncertain original receipts`, async () => {
    for (const value of [
      { metadata: { ...observed().snapshot.text, owner: 'foreign' }, text: 'hello' },
      { metadata: observed().snapshot.text, text: 'longer' },
    ]) {
      const c = await client(target);
      c.ready();
      c.emit('submitted', observed());
      c.reply({ status: 'succeeded', value });
      assert.deepEqual(c.out.actions, []);
      assert.equal(c.state.entries[0].notice, 'draftKept');
      assert.equal(c.editor().command, null);
    }
    const c = await client(target);
    c.ready();
    c.send();
    c.reply({
      status: 'operation',
      caller,
      reply: { reference, receipt: { kind: 'unavailable', diagnostic: 'unknown' } },
    });
    assert.equal(c.editor().command, null);
    c.emit('submitted', observed());
    assert.deepEqual(c.out.actions, []);
    c.raw('draft-retry');
    assert.equal(c.out.actions[0].binding, 'receipts');
    assert.deepEqual(c.out.actions[0].input, reference);
    c.reply({ status: 'succeeded', value: returned() });
    assert.equal(c.out.events.filter((e) => e.name === 'sent').length, 1);
    c.clear('conflict');
    assert.equal(c.editor().persistence.metadata.historyAfter, 0);
    assert.equal(c.state.entries[0].notice, 'draftKept');
    assert.deepEqual(c.out.actions, [], 'recovery and clearing never replay a mutation');
  });
  test(`${target} sends a 64 KiB Unicode snapshot intact and keeps rejected text editable`, async () => {
    const text = '🙂'.repeat(16384),
      c = await client(target);
    c.ready(text);
    c.emit('submitted', observed(text));
    assert.deepEqual(c.out.actions[0].input, { start: 0, end: 65536 });
    c.reply({ status: 'succeeded', value: { metadata: observed(text).snapshot.text, text } });
    assert.equal(c.out.actions[0].input.message, text);
    c.reply({
      status: 'operation',
      caller,
      reply: returned({ kind: 'rejected', sessionId: 'session', reason: 'skills_unavailable' }),
    });
    assert.equal(c.editor().command, null);
    assert.equal(c.editor().persistence.metadata.historyAfter, 0);
    assert.equal(c.state.entries[0].notice, 'skillInputUnavailable');
    assert.equal(c.out.events[0].value.status, 'removed');
    c.emit('submitted', observed(text));
    assert.equal(c.out.actions[0].binding, 'reader');
    c.reply({ status: 'failed', reason: 'capture retired' });
    assert.equal(c.state.entries[0].notice, 'draftKept');
    const ime = observed(text);
    ime.snapshot.marked = { start: 0, end: 2 };
    c.emit('submitted', ime);
    assert.deepEqual(c.out.actions, []);
  });
  test(`${target} restores its own checked prefix without consulting the parent journal`, async () => {
    const c = await client(target);
    c.emit('checkpoint', {
      owner: 'new-editor',
      state: {
        kind: 'restore',
        version: 3,
        snapshot: observed().snapshot,
        metadata: { attachments: [], historyAfter: 17, historyScope: scope },
      },
    });
    assert.deepEqual(c.out.actions[0].input, { after: 0, limit: 1 });
    c.reply({ status: 'succeeded', value: [{ ...caller, cursor: 1, recovery: null }] });
    assert.deepEqual(c.out.actions[0].input, { after: 17, limit: 1 });
    c.reply({ status: 'succeeded', value: [] });
    assert.deepEqual(c.editor().persistence.restore, {
      owner: 'new-editor',
      version: 3,
      clearText: false,
    });
    c.emit('checkpoint', {
      owner: 'new-editor',
      state: { kind: 'ready', initialized: false, saved: false },
    });
    assert.equal(c.editor().persistence.metadata.historyAfter, 17);
    assert.equal(c.editor().persistence.metadata.historyScope, scope);
    assert.deepEqual(c.out.events, [], 'native application precedes its first durable save');
    c.emit('checkpoint', {
      owner: 'new-editor',
      state: { kind: 'ready', initialized: true, saved: true },
    });
    assert.equal(c.editor().persistence.metadata.historyAfter, 17);
    assert.equal(c.editor().persistence.metadata.historyScope, scope);
    assert.equal(c.out.events[0].name, 'readiness');
  });
  test(`${target} resolves an interrupted original send before clearing only its consumed draft and attachments`, async () => {
    const c = await client(target);
    const file = {
      sessionId: 'session',
      id: 'attachment',
      name: 'one.txt',
      mimeType: 'text/plain',
      bytes: 3,
    };
    c.emit('checkpoint', {
      owner: 'new-editor',
      state: {
        kind: 'restore',
        version: 3,
        snapshot: observed().snapshot,
        metadata: { attachments: [file], historyAfter: 17, historyScope: 'another-caller' },
      },
    });
    const original = {
      ...caller,
      cursor: 18,
      recovery: {
        format: 'maka.agent.draft-send@2',
        sessionId: 'session',
        editor: { owner: 'editor', revision: 1 },
        attachments: ['attachment'],
      },
    };
    c.reply({ status: 'succeeded', value: [original] });
    assert.equal(c.out.actions[0].binding, 'receipts');
    c.reply({
      status: 'succeeded',
      value: { reference, receipt: { kind: 'unavailable', diagnostic: 'unknown' } },
    });
    assert.equal(c.editor().persistence.restore, null);
    assert.deepEqual(c.out.actions, []);
    c.raw('draft-retry');
    assert.equal(c.out.actions[0].binding, 'receipts');
    c.reply({ status: 'succeeded', value: returned() });
    assert.deepEqual(c.out.actions[0].input, { after: 18, limit: 1 });
    c.reply({ status: 'succeeded', value: [] });
    assert.deepEqual(c.editor().persistence.restore, {
      owner: 'new-editor',
      version: 3,
      clearText: true,
    });
    assert.deepEqual(c.out.events, [], 'consumed attachments are not restored');
    c.emit('checkpoint', {
      owner: 'new-editor',
      state: { kind: 'ready', initialized: true, saved: true },
    });
    assert.equal(c.editor().persistence.metadata.historyAfter, 18);
  });
  test(`${target} preserves a newer unsent draft, restores unused attachments, and rejects missing recovery metadata`, async () => {
    const c = await client(target);
    const file = {
      sessionId: 'session',
      id: 'attachment',
      name: 'one.txt',
      mimeType: 'text/plain',
      bytes: 3,
    };
    c.emit('checkpoint', {
      owner: 'new-editor',
      state: {
        kind: 'restore',
        version: 3,
        snapshot: observed('new text', 2).snapshot,
        metadata: { attachments: [file], historyAfter: 0, historyScope: null },
      },
    });
    c.reply({
      status: 'succeeded',
      value: [
        {
          ...caller,
          cursor: 18,
          recovery: {
            format: 'maka.agent.draft-send@2',
            sessionId: 'session',
            editor: { owner: 'editor', revision: 1 },
            attachments: [],
          },
        },
      ],
    });
    assert.equal(
      c.out.actions[0].binding,
      'history',
      'older accepted text does not clear a newer draft',
    );
    c.reply({ status: 'succeeded', value: [] });
    assert.equal(c.editor().persistence.restore.clearText, false);
    assert.deepEqual(c.out.events, [
      { name: 'restored', value: { files: [{ ...file, dataSource }] } },
    ]);
    const invalid = await client(target);
    invalid.emit('checkpoint', {
      owner: 'new-editor',
      state: {
        kind: 'restore',
        version: 3,
        snapshot: observed().snapshot,
        metadata: { attachments: [] },
      },
    });
    assert.deepEqual(invalid.out.actions, []);
    assert.equal(invalid.editor().persistence.restore, null);
    assert.equal(invalid.state.records[0].work.status, 'failed');
  });
}

for (const target of ['tui']) {
  test(`${target} pending submissions settle only on original admission`, async () => {
    const c = await client(target);
    c.ready();
    c.send();
    const pending = c.out.events.find((event) => event.name === 'submission').value;
    assert.equal(pending.status, 'pending');
    c.reply({ status: 'failed', reason: 'offline' });
    assert.equal(c.out.events[0].value.status, 'removed');
    assert.equal(c.editor().command, null);
    c.send();
    c.reply({
      status: 'operation',
      caller,
      reply: { reference, receipt: { kind: 'unavailable', diagnostic: 'unknown' } },
    });
    assert.equal(c.out.events[0].value.status, 'unknown');
    assert.equal(
      c.out.events.some((event) => event.name === 'sent'),
      false,
    );
    assert.equal(c.editor().command, null);
    c.raw('draft-retry');
    c.reply({
      status: 'succeeded',
      value: returned({
        kind: 'queued',
        sessionId: 'session',
        messageId: 'queued-message',
        revision: 1,
      }),
    });
    assert.equal(c.out.events[0].value.status, 'removed');
    assert.equal(
      c.out.events.some((event) => event.name === 'sent'),
      true,
    );
  });
}

test('active TUI Enter and queue button submit follow-ups and clear only after queued admission', async () => {
  for (const input of ['enter', 'click']) {
    const c = await client('tui');
    c.props.running = true;
    c.props.sendLabel = 'Queue for next turn';
    c.ready('多刷一会');
    assert.equal(c.editor().submit, 'enter');
    if (input === 'enter') c.emit('submitted', observed('多刷一会'));
    else {
      c.raw('send');
      c.reply({
        status: 'succeeded',
        value: { metadata: observed('多刷一会').snapshot.text, text: '' },
      });
    }
    const read = c.out.actions.find((action) => action.key.startsWith('read-'));
    c.reply(
      {
        status: 'succeeded',
        value: { metadata: observed('多刷一会').snapshot.text, text: '多刷一会' },
      },
      read.key,
    );
    assert.deepEqual(c.out.actions[0].input, {
      sessionId: 'session',
      message: '多刷一会',
      attachments: [],
      model: backend.model,
      thinkingLevel: null,
    });
    assert.equal(c.editor().command, null);
    c.reply({
      status: 'operation',
      reply: returned({
        kind: 'queued',
        sessionId: 'session',
        messageId: 'queued-followup',
        revision: 7,
      }),
      caller,
    });
    assert.deepEqual(c.editor().command.edit, { kind: 'reset', text: '' });
    assert.deepEqual(
      c.out.events.filter((event) => event.name === 'sent'),
      [{ name: 'sent', value: { context: 'session', dataSource, attachments: [] } }],
    );
    c.clear();
    assert.equal(c.state.entries[0].delivery, null);
    assert.equal(c.editor().persistence.metadata.historyAfter, 42);
  }
});

test('TUI queue click captures the painted native draft before its changed observation arrives', async () => {
  const c = await client('tui');
  c.props.running = true;
  c.ready('');
  assert.equal(findNode(c.out.frame.view, 'send').disabled, false);
  c.raw('send');
  const capture = c.out.actions[0];
  assert.deepEqual(capture.input, { start: 0, end: 0 });
  assert.deepEqual(capture.resources.text, {
    child: 'draft',
    method: 'snapshot',
    input: { kind: 'current' },
  });
  c.reply({
    status: 'succeeded',
    value: { metadata: observed('多刷一会', 2).snapshot.text, text: '' },
  });
  assert.deepEqual(c.out.actions[0].input, { start: 0, end: 12 });
  assert.deepEqual(c.out.actions[0].resources.text.input, {
    kind: 'at_revision',
    owner: 'editor',
    revision: 2,
  });
  c.reply({
    status: 'succeeded',
    value: { metadata: observed('多刷一会', 2).snapshot.text, text: '多刷一会' },
  });
  assert.deepEqual(c.out.actions[0].input, {
    sessionId: 'session',
    message: '多刷一会',
    attachments: [],
    model: backend.model,
    thinkingLevel: null,
  });
  c.reply({
    status: 'operation',
    reply: returned({ kind: 'queued', sessionId: 'session', messageId: 'next', revision: 7 }),
    caller,
  });
  assert.equal(c.editor().command.basedOn, 2);
  assert.deepEqual(c.editor().command.edit, { kind: 'reset', text: '' });
});

test('an empty TUI queue click finishes without a latent send and a later draft needs its own intent', async () => {
  const c = await client('tui');
  c.props.running = true;
  c.ready('');
  c.raw('send');
  c.reply({ status: 'succeeded', value: { metadata: observed('').snapshot.text, text: '' } });
  assert.equal(c.state.entries[0].delivery, null);
  assert.equal(c.state.entries[0].notice, 'empty');
  c.emit('changed', observed('later', 2));
  assert.equal(c.state.entries[0].delivery, null);
  assert.equal(
    c.out.actions.some((action) => action.binding === 'agent'),
    false,
  );
  c.raw('send');
  c.reply({
    status: 'succeeded',
    value: { metadata: observed('later', 2).snapshot.text, text: '' },
  });
  c.reply({
    status: 'succeeded',
    value: { metadata: observed('later', 2).snapshot.text, text: 'later' },
  });
  assert.equal(c.out.actions[0].input.message, 'later');
});

test('TUI current capture freezes its destination and a changed native revision keeps the newer draft', async () => {
  const c = await client('tui');
  c.props.running = true;
  c.ready('');
  c.raw('send');
  c.reply({
    status: 'succeeded',
    value: { metadata: observed('captured', 2).snapshot.text, text: '' },
  });
  const read = c.out.actions[0];
  c.emit('changed', observed('newer draft', 3));
  c.reply({ status: 'failed', error: 'editor revision changed' }, read.key);
  assert.equal(c.state.entries[0].delivery, null);
  assert.equal(c.state.entries[0].notice, 'draftKept');
  assert.equal(c.editor().command, null);
  c.raw('send');
  const capture = c.out.actions[0];
  c.props.context = 'another-session';
  c.update();
  c.reply(
    {
      status: 'succeeded',
      value: { metadata: observed('newer draft', 3).snapshot.text, text: '' },
    },
    capture.key,
  );
  assert.equal(c.out.actions[0].resources.text.child, 'draft');
  c.reply({
    status: 'succeeded',
    value: { metadata: observed('newer draft', 3).snapshot.text, text: 'newer draft' },
  });
  assert.equal(c.out.actions[0].input.sessionId, 'session');
  assert.equal(c.out.actions[0].input.message, 'newer draft');
});

test('pending approval keeps the entire composer border amber through focus and execution, then restores it', async () => {
  const c = await client('tui');
  const find = (node) =>
    node?.key === 'composer-main'
      ? node
      : node && typeof node === 'object'
        ? Object.values(node)
            .flatMap((value) => (Array.isArray(value) ? value : [value]))
            .map(find)
            .find(Boolean)
        : undefined;
  for (const running of [false, true]) {
    c.props.running = running;
    c.props.approvalPending = true;
    c.update();
    assert.equal(validProps(c.props), true, JSON.stringify(validProps.errors));
    const border = find(c.out.frame.view).border;
    assert.equal(border.color, theme.terminal.warning);
    assert.equal(border.focused_color, theme.terminal.warning);
    assert.equal(border.breathing, false);
    c.props.approvalPending = false;
    c.update();
    assert.equal(
      find(c.out.frame.view).border.color,
      running ? theme.terminal.accent : theme.terminal.border,
    );
    assert.equal(find(c.out.frame.view).border.focused_color, theme.terminal.accent);
  }
});

for (const target of ['tui']) {
  test(`${target} /compact opens a local command and clears its captured draft`, async () => {
    const c = await client(target);
    c.ready('/compact');
    c.send('/compact');
    assert.deepEqual(c.out.events, [
      { name: 'command', value: { context: 'session', dataSource, id: 'compact-open' } },
    ]);
    assert.deepEqual(c.editor().command.edit, { kind: 'reset', text: '' });
    c.clear();
    const prose = await client(target);
    prose.ready('/compact is a command');
    prose.send('/compact is a command');
    assert.deepEqual(prose.out.actions[0].input, {
      sessionId: 'session',
      message: '/compact is a command',
      attachments: [],
      model: backend.model,
      thinkingLevel: null,
    });
  });
}

for (const target of ['tui']) {
  const has = (c, key) => JSON.stringify(c.out.frame.view).includes(`"key":"${key}"`);
  const resolvePrefix = (c, text, revision = 1) =>
    c.reply(
      {
        status: 'succeeded',
        value: { metadata: observed(text, revision).snapshot.text, text },
      },
      c.out.actions.find((item) => item.key.startsWith('prefix-')).key,
    );
  const skill = {
    reference: 'workspace:review',
    name: 'review',
    description: 'Review current changes',
    invocation: '/skill:review-id',
  };
  test(`${target} visible skills use agent authority and click, Enter and Tab insert invocation without sending`, async () => {
    for (const selection of ['click', 'enter', 'tab']) {
      const c = await client(target);
      c.ready('/rev');
      resolvePrefix(c, '/rev');
      const search = c.out.actions.find((action) => action.method === 'skillCandidates');
      assert.deepEqual(search.input, { sessionId: 'session', backend, query: 'rev' });
      c.reply(
        { status: 'succeeded', value: { revision: 'catalog-1', skills: [skill] } },
        search.key,
      );
      assert.ok(has(c, 'slash:skill:workspace:review'));
      assert.ok(JSON.stringify(c.out.frame.view).includes(skill.description));
      if (selection === 'click') c.raw('slash:skill:workspace:review');
      else c.emit('navigated', { key: selection, observation: observed('/rev') });
      assert.deepEqual(c.editor().command.edit, { kind: 'replace', text: '/skill:review-id ' });
      assert.deepEqual(c.out.events, []);
      assert.equal(
        c.out.actions.some((action) => ['send', 'execute'].includes(action.method)),
        false,
      );
    }
    const c = await client(target);
    c.ready('/skill:review-id');
    resolvePrefix(c, '/skill:review-id');
    assert.equal(c.editor().navigation, null);
    c.send('/skill:review-id');
    assert.equal(c.out.actions[0].method, 'send');
    assert.equal(c.out.actions[0].input.message, '/skill:review-id');
  });
  test(`${target} inline skill completion replaces only the cursor token in Unicode multiline drafts`, async () => {
    for (const selection of ['click', 'enter', 'tab']) {
      const c = await client(target);
      const prefix = `请检查😀 ${'context '.repeat(20)}\n`;
      const draft = `${prefix}/review more context\n保留后文`;
      const observation = observed(draft);
      observation.snapshot.selection = { start: prefix.length + 4, end: prefix.length + 4 };
      c.ready(draft);
      resolvePrefix(c, draft);
      c.emit('changed', observation);
      const search = c.out.actions.find((action) => action.method === 'skillCandidates');
      assert.deepEqual(search.input, { sessionId: 'session', backend, query: 'rev' });
      c.reply({ status: 'succeeded', value: { skills: [skill] } }, search.key);
      assert.equal(has(c, 'slash:create'), false);
      if (selection === 'click') c.raw('slash:skill:workspace:review');
      else c.emit('navigated', { key: selection, observation });
      assert.deepEqual(c.editor().command, {
        owner: 'editor',
        sequence: c.editor().command.sequence,
        basedOn: 1,
        edit: {
          kind: 'replace_range',
          range: { start: prefix.length, end: prefix.length + 7 },
          text: '/skill:review-id',
        },
      });
      assert.deepEqual(c.editor().inlineTokens, ['/skill:review-id']);
      const { range, text } = c.editor().command.edit;
      assert.equal(
        draft.slice(0, range.start) + text + draft.slice(range.end),
        `${prefix}/skill:review-id more context\n保留后文`,
      );
      assert.deepEqual(c.out.events, []);
    }
  });
  test(`${target} inline completion follows cursor movement and does not intercept paths or IME`, async () => {
    const c = await client(target);
    const draft = 'before /rev after';
    c.ready(draft);
    resolvePrefix(c, draft);
    assert.equal(c.editor().navigation, null);
    const moved = observed(draft, 2);
    moved.snapshot.text.revision = 1;
    moved.snapshot.selection = { start: 11, end: 11 };
    c.emit('changed', moved);
    const search = c.out.actions.find((action) => action.method === 'skillCandidates');
    assert.deepEqual(search.input, { sessionId: 'session', backend, query: 'rev' });
    const marked = structuredClone(moved);
    marked.revision = 3;
    marked.snapshot.marked = { start: 8, end: 11 };
    c.emit('changed', marked);
    c.reply({ status: 'succeeded', value: { skills: [skill] } }, search.key);
    c.raw('slash:skill:workspace:review');
    assert.equal(c.editor().command, null);
    assert.equal(c.editor().navigation, null);
    for (const text of ['before /tmp/file', 'https://example.com/rev', 'before /skill:review-id']) {
      const normal = await client(target);
      normal.ready(text);
      resolvePrefix(normal, text);
      assert.equal(normal.editor().navigation, null);
      normal.send(text);
      assert.equal(normal.out.actions[0].input.message, text);
    }
  });
  test(`${target} stale skill replies cannot populate another cursor token`, async () => {
    const c = await client(target);
    const draft = 'first /rev second /rev';
    c.ready(draft);
    resolvePrefix(c, draft);
    const oldSearch = c.out.actions.find((action) => action.method === 'skillCandidates');
    c.emit('navigated', { key: 'escape', observation: observed(draft) });
    assert.equal(c.editor().navigation, null);
    const moved = observed(draft, 2);
    moved.snapshot.text.revision = 1;
    moved.snapshot.selection = { start: 10, end: 10 };
    c.emit('changed', moved);
    const search = c.out.actions.find((action) => action.method === 'skillCandidates');
    c.reply({ status: 'succeeded', value: { skills: [skill] } }, oldSearch.key);
    c.raw('slash:skill:workspace:review');
    assert.equal(c.editor().command, null);
    c.reply({ status: 'succeeded', value: { skills: [skill] } }, search.key);
    c.emit('navigated', { key: 'tab', observation: moved });
    assert.deepEqual(c.editor().command.edit, {
      kind: 'replace_range',
      range: { start: 6, end: 10 },
      text: '/skill:review-id',
    });
    assert.equal(c.editor().command.basedOn, 2);
  });
  test(`${target} skill descriptions longer than the terminal tooltip limit remain usable`, async () => {
    const c = await client(target);
    c.ready('/rev');
    resolvePrefix(c, '/rev');
    const search = c.out.actions.find((action) => action.method === 'skillCandidates');
    c.reply(
      { status: 'succeeded', value: { skills: [{ ...skill, description: '审查'.repeat(100) }] } },
      search.key,
    );
    const button = findNode(c.out.frame.view, 'slash:skill:workspace:review');
    assert.equal(button.label, `› /review  ${'审查'.repeat(100)}`);
    assert.equal(button.hint, '');
    c.emit('navigated', { key: 'tab', observation: observed('/rev') });
    assert.deepEqual(c.editor().command.edit, { kind: 'replace', text: '/skill:review-id ' });
  });
  test(`${target} completed skill literals reach the native editor before insertion and survive clear and undo`, async () => {
    const c = await client(target);
    c.ready('/rev');
    resolvePrefix(c, '/rev');
    const search = c.out.actions.find((action) => action.method === 'skillCandidates');
    c.reply({ status: 'succeeded', value: { skills: [skill] } }, search.key);
    assert.deepEqual(c.editor().inlineTokens, ['/skill:review-id']);
    c.emit('navigated', { key: 'tab', observation: observed('/rev') });
    assert.deepEqual(c.editor().inlineTokens, ['/skill:review-id']);
    assert.deepEqual(c.editor().command.edit, { kind: 'replace', text: '/skill:review-id ' });
    c.clear();
    assert.deepEqual(c.editor().inlineTokens, ['/skill:review-id']);
    const restored = '说明😀\n/skill:manual-id /skill:review-id /path/skill:ignored /skill:bad/ref';
    c.emit('changed', observed(restored, 3));
    resolvePrefix(c, restored, 3);
    assert.deepEqual(c.editor().inlineTokens, ['/skill:review-id', '/skill:manual-id']);
    c.emit('changed', observed('undo /skill:review-id ', 4));
    assert.equal(c.state.entries[0].prefix.cached, null);
    assert.deepEqual(c.editor().inlineTokens, ['/skill:review-id', '/skill:manual-id']);
    c.emit('changed', observed('', 5));
    assert.deepEqual(c.editor().inlineTokens, ['/skill:review-id', '/skill:manual-id']);
    c.props.context = 'another-session';
    c.update();
    const another = c.state.entries.find((entry) => entry.context === 'another-session');
    assert.deepEqual(c.editor(another.key).inlineTokens, []);
    assert.deepEqual(c.editor('draft').inlineTokens, ['/skill:review-id', '/skill:manual-id']);
  });
  test(`${target} failed skill searches preserve commands and retry when reopened`, async () => {
    const c = await client(target);
    c.ready('/');
    resolvePrefix(c, '/');
    const search = c.out.actions.find((action) => action.method === 'skillCandidates');
    c.reply({ status: 'failed', error: 'unavailable' }, search.key);
    assert.ok(has(c, 'slash:create'));
    assert.equal(c.editor().command, null);
    c.emit('navigated', { key: 'escape', observation: observed('/') });
    c.emit('changed', observed('/', 2));
    resolvePrefix(c, '/', 2);
    assert.ok(c.out.actions.some((action) => action.method === 'skillCandidates'));
  });
  test(`${target} skill results cannot cross query, session, data source, backend or visibility context`, async () => {
    for (const change of [
      'query',
      'session',
      'dataSource',
      'backend',
      'skillContext',
      'disabled',
    ]) {
      const c = await client(target);
      c.ready('/rev');
      resolvePrefix(c, '/rev');
      const search = c.out.actions.find((action) => action.method === 'skillCandidates');
      if (change === 'query') c.emit('changed', observed('/new', 2));
      else {
        if (change === 'session') c.props.context = 'other-session';
        if (change === 'dataSource')
          c.props.dataSource = {
            ...dataSource,
            deployment: '00000000-0000-0000-0000-000000000099',
          };
        if (change === 'backend')
          c.props.backend = { ...backend, model: { ...backend.model, modelId: 'other' } };
        if (change === 'skillContext') c.props.skillContext = 'changed workspace or tools';
        if (change === 'disabled') c.props.enabled = false;
        c.update();
      }
      c.reply(
        { status: 'succeeded', value: { revision: 'catalog-1', skills: [skill] } },
        search.key,
      );
      assert.equal(has(c, 'slash:skill:workspace:review'), false, change);
      c.raw('slash:skill:workspace:review');
      assert.equal(c.out.events.length, 0, change);
    }
  });
  test(`${target} slash menu filters aliases and localized labels and keeps native text out of state`, async () => {
    const c = await client(target);
    c.ready('/');
    resolvePrefix(c, '/');
    assert.ok(has(c, 'slash:compact-open'));
    assert.ok(has(c, 'slash:create'));
    assert.deepEqual(c.editor().navigation, { owner: 'editor', basedOn: 1 });
    c.emit('changed', observed('/压缩', 2));
    assert.equal(c.editor().navigation, null);
    resolvePrefix(c, '/压缩', 2);
    assert.ok(has(c, 'slash:compact-open'));
    assert.equal(has(c, 'slash:create'), false);
    c.emit('changed', observed('x'.repeat(65537), 3));
    assert.deepEqual(c.out.actions, []);
    assert.equal(has(c, 'slash-menu'), false);
    assert.equal(JSON.stringify(c.state).includes('x'.repeat(65537)), false);
  });
  test(`${target} slash reads coalesce and stale results cannot restore a menu`, async () => {
    const c = await client(target);
    c.ready('/');
    const first = c.out.actions[0].key;
    c.emit('changed', observed('/c', 2));
    c.emit('changed', observed('/new', 3));
    assert.deepEqual(c.out.actions, []);
    c.reply(
      { status: 'succeeded', value: { metadata: observed('/').snapshot.text, text: '/' } },
      first,
    );
    assert.equal(has(c, 'slash-menu'), false);
    assert.equal(c.out.actions.length, 1);
    resolvePrefix(c, '/new', 3);
    assert.ok(has(c, 'slash:create'));
    assert.equal(has(c, 'slash:compact-open'), false);
  });
  test(`${target} slash keyboard selection executes locally without a model and Escape retains draft`, async () => {
    const c = await client(target);
    c.props.backend = null;
    c.ready('/');
    resolvePrefix(c, '/');
    c.emit('navigated', { key: 'down', observation: observed('/') });
    c.emit('navigated', { key: 'enter', observation: observed('/') });
    c.reply({ status: 'succeeded', value: { metadata: observed('/').snapshot.text, text: '/' } });
    assert.deepEqual(c.out.events, [
      { name: 'command', value: { context: 'session', dataSource, id: 'create' } },
    ]);
    assert.equal(
      c.out.actions.some((action) => action.binding === 'agent'),
      false,
    );
    c.clear();
    c.emit('changed', observed('/settings', 3));
    resolvePrefix(c, '/settings', 3);
    c.emit('navigated', { key: 'escape', observation: observed('/settings', 3) });
    assert.equal(c.editor().navigation, null);
    assert.equal(c.editor().command, null);
    assert.equal(c.state.entries[0].observation.snapshot.text.bytes, 9);
  });
  test(`${target} click validates captured command and tab completes without executing`, async () => {
    const c = await client(target);
    c.ready('/co');
    resolvePrefix(c, '/co');
    c.emit('navigated', { key: 'tab', observation: observed('/co') });
    assert.deepEqual(c.editor().command.edit, { kind: 'replace', text: '/compact' });
    assert.deepEqual(c.out.events, []);
    c.clear();
    c.emit('changed', observed('/co', 3));
    resolvePrefix(c, '/co', 3);
    c.raw('slash:compact-open');
    c.reply({
      status: 'succeeded',
      value: { metadata: observed('/co', 3).snapshot.text, text: '/co' },
    });
    assert.equal(c.out.events[0].value.id, 'compact-open');
  });
  test(`${target} unknown and disabled commands never submit and inactive captures retain drafts`, async () => {
    for (const command of ['/unknown', '/compact']) {
      const c = await client(target);
      c.props.commands[0].disabled = true;
      c.ready(command);
      resolvePrefix(c, command);
      assert.ok(has(c, 'slash-empty'));
      c.send(command);
      assert.equal(
        c.out.actions.some((action) => action.binding === 'agent'),
        false,
      );
      assert.equal(c.editor().command, null);
      assert.deepEqual(c.out.events, []);
    }
    const c = await client(target);
    c.ready('/compact');
    c.emit('submitted', observed('/compact'));
    const read = c.out.actions[0].key;
    c.props.context = 'another';
    c.update();
    c.reply(
      {
        status: 'succeeded',
        value: { metadata: observed('/compact').snapshot.text, text: '/compact' },
      },
      read,
    );
    assert.equal(c.editor('draft').command, null);
    assert.equal(c.state.entries[0].notice, 'draftKept');
    assert.equal(
      c.out.events.some((event) => event.name === 'command'),
      false,
    );
  });
  test(`${target} stale navigation and IME retain normal editing; paths and multiline text send normally`, async () => {
    const c = await client(target);
    c.ready('/');
    resolvePrefix(c, '/');
    c.emit('navigated', { key: 'enter', observation: observed('/', 0) });
    assert.deepEqual(c.out.actions, []);
    const marked = observed('/', 2);
    marked.snapshot.marked = { start: 0, end: 1 };
    c.emit('changed', marked);
    assert.equal(c.editor().navigation, null);
    for (const text of ['/tmp/file', '/a\nb']) {
      const normal = await client(target);
      normal.ready(text);
      normal.send(text);
      assert.equal(normal.out.actions[0].input.message, text);
    }
  });
}

for (const target of ['tui']) {
  test(`${target} command menu shows six rows and scrolls selection across the catalog`, async () => {
    const c = await client(target);
    c.props.commands = Array.from({ length: 10 }, (_, i) => ({
      id: `command-${i}`,
      command: `command${i}`,
      label: `Command ${i}`,
      disabled: false,
    }));
    c.ready('/');
    c.reply({ status: 'succeeded', value: { metadata: observed('/').snapshot.text, text: '/' } });
    const rows = () => [...JSON.stringify(c.out.frame.view).matchAll(/"key":"slash:command-\d+"/g)];
    assert.equal(rows().length, 6);
    c.emit('navigated', { key: 'up', observation: observed('/') });
    assert.equal(rows().length, 6);
    assert.ok(JSON.stringify(c.out.frame.view).includes('slash:command-9'));
    c.emit('navigated', { key: 'enter', observation: observed('/') });
    c.reply({ status: 'succeeded', value: { metadata: observed('/').snapshot.text, text: '/' } });
    assert.equal(c.out.events[0].value.id, 'command-9');
  });
}
