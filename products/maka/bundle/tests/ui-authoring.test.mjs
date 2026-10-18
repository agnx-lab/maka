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

const request = {
  id: 'a'.repeat(64),
  command: { method: 'installSource', input: { expectedRevision: 1, files: {} } },
};
const reference = { operation: 'original-operation' };
const recorded = (value) => ({
  reference,
  receipt: {
    kind: 'recorded',
    outcome: { kind: 'returned', value },
    cleanup: { kind: 'confirmed' },
  },
});
async function client(target) {
  const { component } = await import(`../dist/packages/ui-authoring/${target}.js`);
  let out = component.mount({});
  return {
    get out() {
      return out;
    },
    reply(event, key = out.actions[0].key) {
      out = component.event(out.frame.state, { kind: 'action', key, event });
      return this;
    },
    value(value) {
      return this.reply({ status: 'succeeded', value });
    },
    operation(value) {
      return this.reply({ status: 'operation', reply: recorded(value), caller: null });
    },
    watch() {
      return this.reply({ status: 'updated', value: { pending: 1 } }, 'authoring-watch');
    },
  };
}
for (const target of ['tui']) {
  test(`${target} authoring dispatches a large source once without duplicating its document in state`, async () => {
    const c = await client(target);
    const large = structuredClone(request);
    large.command.input.files['tui.js'] = '// ' + 'x'.repeat(600 * 1024);
    c.value([]).operation({ request: large });
    const apply = c.out.actions[0];
    assert.deepEqual(apply.input, large.command.input);
    assert.ok(Buffer.byteLength(JSON.stringify(c.out)) < 1024 * 1024);
    assert.ok(Buffer.byteLength(JSON.stringify(c.out.frame.state)) < 1024);
    c.watch();
    assert.deepEqual(c.out.actions, []);
    c.reply({ status: 'operation', reply: recorded({ status: 'committed' }) }, apply.key);
    assert.equal(c.out.actions[0].method, 'complete');
    assert.equal(c.out.actions[0].input.id, large.id);
  });
  test(`${target} authoring preserves original manager outcome and serializes requests`, async () => {
    const c = await client(target);
    c.value([]).operation({ request });
    const apply = c.out.actions[0];
    assert.equal(apply.method, 'installSource');
    assert.deepEqual(apply.input, request.command.input);
    assert.deepEqual(apply.recovery, { uiAuthoring: 'apply', id: request.id });
    c.watch();
    assert.deepEqual(c.out.actions, []);
    const outcome = {
      status: 'operation',
      reply: {
        reference,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'unknown', diagnostic: 'lost' },
          cleanup: { kind: 'confirmed' },
        },
      },
    };
    c.reply(outcome, apply.key);
    assert.equal(c.out.actions[0].method, 'complete');
    assert.deepEqual(c.out.actions[0].input, { id: request.id, outcome });
    c.operation(true);
    assert.equal(c.out.actions[0].method, 'claim');
  });
  test(`${target} authoring recovers a source update after remount without executing it again`, async () => {
    const c = await client(target);
    c.value([{ cursor: 1, reference, recovery: { uiAuthoring: 'apply', id: request.id } }]).value(
      [],
    );
    assert.equal(c.out.actions[0].binding, 'managerReceipts');
    assert.deepEqual(c.out.actions[0].input, reference);
    const reply = recorded({ kind: 'committed', value: { source: { revision: 2 } } });
    c.value(reply);
    assert.equal(c.out.actions[0].method, 'complete');
    assert.deepEqual(c.out.actions[0].input.outcome, { status: 'operation', reply, caller: null });
    c.operation(true).operation({ request: null });
    assert.deepEqual(c.out.actions, []);
  });
  test(`${target} authoring recovers claim/completion receipts and retains a wake arriving during claim`, async () => {
    const c = await client(target);
    c.value([{ cursor: 1, reference, recovery: { uiAuthoring: 'claim' } }]).value([]);
    assert.equal(c.out.actions[0].binding, 'receipts');
    c.value(recorded({ request }));
    assert.equal(c.out.actions[0].method, 'installSource');
    const resumed = await client(target);
    resumed
      .value([{ cursor: 2, reference, recovery: { uiAuthoring: 'complete', id: request.id } }])
      .value([]);
    assert.equal(resumed.out.actions[0].method, 'read');
    resumed.value(recorded(true));
    const claiming = resumed.out.actions[0].key;
    resumed.watch().reply({ status: 'operation', reply: recorded({ request: null }) }, claiming);
    assert.equal(resumed.out.actions[0].method, 'claim');
    resumed.operation({ request: null });
    assert.deepEqual(resumed.out.actions, []);
    resumed.reply({ status: 'failed', error: 'disconnected' }, 'authoring-watch');
    assert.deepEqual(resumed.out.subscriptions, []);
  });
}

test('TUI AgentScreen mounts metrics with session context and keeps one authoring worker', async () => {
  const agentScreen = await import('../dist/packages/agent-screen/tui.js');
  const appearance = await import('../dist/packages/appearance/tui.js');
  agentScreen.activate({});
  const values = { appearance: [appearance.activate({ scheme: 'dark' }).Appearance.value] };
  const nodes = (node) =>
    node && typeof node === 'object'
      ? [
          node,
          ...Object.values(node).flatMap((v) => (Array.isArray(v) ? v.flatMap(nodes) : nodes(v))),
        ]
      : [];
  const view = agentScreen.component.mount({}, values).frame.view;
  const slot = nodes(view).find((node) => node.kind === 'component' && node.binding === 'footer');
  assert.equal(slot.binding, 'footer');
  assert.equal(slot.props.sessionId, null);
  assert.equal(slot.props.running, false);
  assert.equal(
    nodes(view).filter((node) => node.kind === 'component' && node.binding === 'authoring').length,
    1,
  );
  const embedded = agentScreen.component.mount({ embedded: true }, values).frame.view;
  assert.equal(
    nodes(embedded).filter((node) => node.kind === 'component' && node.binding === 'authoring')
      .length,
    0,
  );
});
