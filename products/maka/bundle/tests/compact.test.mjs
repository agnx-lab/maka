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
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'dark' }).Appearance.value;
const observed = (name, revision, text, receipt) => ({
  revision,
  snapshot: { text: { owner: name, revision, bytes: Buffer.byteLength(text) }, marked: null },
  ...(receipt ? { receipt } : {}),
});
async function client(target) {
  const module = await import(`../dist/packages/agent-screen/${target}.js`);
  module.activate({ locale: 'en' });
  const values = { appearance: [theme] };
  let out = module.component.mount({}, values);
  Object.assign(out.frame.state, {
    selected: {
      id: 'session',
      title: 'Compact test',
      labels: [],
      revision: 1,
      updatedAt: 1,
      archived: false,
      activity: 'idle',
      boundaryRevision: 7,
      hasUnread: false,
      dataSource: {
        stateDomain: 'a'.repeat(64),
        deployment: '00000000-0000-0000-0000-000000000001',
        namespace: ['00000000-0000-0000-0000-000000000002'],
      },
      permissions: { access: 'read_only', requestPermissions: true },
      workspace: { path: '/original', directoryIdentity: 'a'.repeat(64), identity: 'original' },
    },
    model: {
      choice: {
        selection: { connectionId: '00000000-0000-0000-0000-000000000003', modelId: 'fixture' },
        name: 'fixture',
      },
      thinkingLevel: null,
    },
    inspector: null,
    dialog: null,
    modelRefreshing: false,
    pending: {},
    restoring: false,
  });
  const c = {
    get state() {
      return out.frame.state;
    },
    get out() {
      return out;
    },
    event(input) {
      out = module.component.event(c.state, input, {}, values);
      return c;
    },
    raw(key) {
      return c.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    field(name, observation) {
      return c.event({
        kind: 'component',
        child: 'compact-prompt',
        name: 'changed',
        event: observation,
      });
    },
    reply(value, key = out.actions[0].key) {
      return c.event({ kind: 'action', key, event: { status: 'succeeded', value } });
    },
    ready() {
      c.field('', observed('prompt', 0, ''));
      const cmd = c.state.compact.command;
      c.field(
        '',
        observed('prompt', 1, cmd.edit.text, {
          owner: cmd.owner,
          sequence: cmd.sequence,
          outcome: { status: 'applied' },
        }),
      );
    },
  };
  return c;
}
for (const target of ['tui']) {
  test(`${target} compact confirms the edited one-shot prompt without saving preferences`, async () => {
    const c = await client(target);
    c.event({
      kind: 'component',
      child: 'composer',
      name: 'command',
      event: { context: 'session', dataSource: c.state.selected.dataSource, id: 'compact-open' },
    });
    assert.equal(c.state.dialog, 'compact');
    assert.deepEqual(c.out.actions[0].input, { sessionId: 'session' });
    assert.equal(c.out.actions[0].method, 'compactionPrompt');
    c.reply({
      compactionPrompt: 'Global instructions',
      defaultCompactionPrompt: 'Built-in instructions',
    });
    c.ready();
    assert.equal(c.state.compact.observation.snapshot.text.bytes, 19);
    const draft = observed('prompt', 2, 'Preserve 中文 decisions');
    c.field('', draft);
    c.raw('compact-confirm');
    assert.equal(c.out.actions[0].resources.text.child, 'compact-prompt');
    c.reply({ metadata: draft.snapshot.text, text: 'Preserve 中文 decisions' });
    assert.equal(c.out.actions[0].method, 'compact');
    assert.deepEqual(c.out.actions[0].input, {
      sessionId: 'session',
      prompt: 'Preserve 中文 decisions',
      model: { connectionId: '00000000-0000-0000-0000-000000000003', modelId: 'fixture' },
      thinkingLevel: null,
    });
    const key = c.out.actions[0].key;
    c.event({
      kind: 'action',
      key,
      event: {
        status: 'operation',
        reply: {
          reference: { operation: 'compact-1', method: 'compact' },
          receipt: {
            kind: 'recorded',
            outcome: { kind: 'returned', value: { kind: 'started', execution: {} } },
          },
        },
      },
    });
    assert.equal(c.state.dialog, null);
    assert.equal(
      c.out.actions.find((action) => action.method === 'observe').input.sessionId,
      'session',
    );
  });
  test(`${target} compact cancels locally and refuses a stale session destination`, async () => {
    const c = await client(target);
    const open = () =>
      c.event({
        kind: 'component',
        child: 'composer',
        name: 'command',
        event: { context: 'session', dataSource: c.state.selected.dataSource, id: 'compact-open' },
      });
    open();
    c.reply({ compactionPrompt: 'Default', defaultCompactionPrompt: 'Default' });
    c.ready();
    c.raw('compact-cancel');
    assert.equal(c.state.dialog, null);
    open();
    c.reply({ compactionPrompt: 'Default', defaultCompactionPrompt: 'Default' });
    c.ready();
    c.state.selected.id = 'another-session';
    c.raw('compact-confirm');
    assert.equal(c.state.compact.sessionId, 'session');
    assert.equal(
      c.out.actions.some((action) => action.method === 'compact'),
      false,
    );
  });
}

for (const target of ['tui'])
  test(`${target} compact rejects a prompt edited during capture and checks an uncertain receipt`, async () => {
    const c = await client(target);
    c.event({
      kind: 'component',
      child: 'composer',
      name: 'command',
      event: { context: 'session', dataSource: c.state.selected.dataSource, id: 'compact-open' },
    });
    c.reply({ compactionPrompt: 'Default', defaultCompactionPrompt: 'Default' });
    c.ready();
    const original = observed('prompt', 2, 'Keep original');
    c.field('', original);
    c.raw('compact-confirm');
    const readKey = c.out.actions[0].key;
    c.field('', observed('prompt', 3, 'Keep new text'));
    c.reply({ metadata: original.snapshot.text, text: 'Keep original' }, readKey);
    assert.equal(c.state.compact.failed, true);
    c.raw('compact-confirm');
    c.reply({ metadata: c.state.compact.observation.snapshot.text, text: 'Keep new text' });
    assert.equal(c.out.actions[0].input.prompt, 'Keep new text');
    const reference = { operation: 'original-compact', method: 'compact' };
    c.event({
      kind: 'action',
      key: c.out.actions[0].key,
      event: { status: 'operation', reply: { reference, receipt: { kind: 'pending' } } },
    });
    c.raw('compact-check');
    assert.equal(c.out.actions[0].binding, 'receipts');
    assert.deepEqual(c.out.actions[0].input, reference);
    c.reply({
      reference,
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'returned', value: { kind: 'started', execution: {} } },
      },
    });
    assert.equal(c.state.dialog, null);
  });
