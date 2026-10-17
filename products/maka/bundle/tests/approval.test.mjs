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
const appearance = await import('../dist/packages/appearance/tui.js');
const theme = appearance.activate({ scheme: 'light' }).Appearance.value;
const permissions = {
  filesystem: [{ access: 'write', scope: 'subtree', path: '/project' }],
  network: 'denied',
};
const requests = {
  permission: {
    kind: 'permissions',
    toolUseId: 'tool',
    baseRevision: 1,
    request: { reason: 'Write project files', permissions },
  },
  service: {
    kind: 'service_approval',
    toolUseId: 'tool',
    toolName: 'write',
    service: 'files',
    method: 'write',
    binding: 'files',
    through: 7,
  },
  computer: {
    kind: 'client_capability',
    toolUseId: 'tool',
    baseRevision: 1,
    target: {
      providerId: 'native',
      contractId: 'computer',
      serverId: '',
      toolName: '',
      capability: 'computer_use',
      scope: { kind: 'capability' },
    },
  },
};
function nodes(node) {
  return node && typeof node === 'object'
    ? [
        node,
        ...Object.values(node).flatMap((value) =>
          Array.isArray(value) ? value.flatMap(nodes) : nodes(value),
        ),
      ]
    : [];
}
async function client(target, kind) {
  const plugin = kind === 'permission' ? 'permission-approval' : 'service-approval';
  const { component } = await import(`../dist/packages/${plugin}/${target}.js`);
  let props = {
    theme,
    locale: 'en',
    open: true,
    busy: false,
    checking: false,
    status: '',
    record: {
      sessionId: 'session',
      turnId: 'turn',
      runId: 'run',
      requestId: 'first',
      createdAt: 1,
      request: structuredClone(requests[kind]),
      outcome: null,
    },
  };
  let out = component.mount(props);
  const c = {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    get props() {
      return props;
    },
    node(key) {
      return nodes(out.frame.view).find((node) => node.key === key);
    },
    event(input) {
      out = component.event(c.state, input, props);
      return c;
    },
    raw(key) {
      return c.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    click(key) {
      assert.ok(c.node(key), `${key} must be rendered`);
      assert.equal(c.node(key).disabled, false);
      return c.raw(key);
    },
    update(changes) {
      props = { ...props, ...changes };
      out = component.update(c.state, props);
      return c;
    },
    stale(changes, key) {
      props = { ...props, ...changes };
      return c.raw(key);
    },
    ready() {
      const source = {
        sequence: 6,
        timestamp: 1,
        role: 'assistant',
        fields: ['arguments'],
        field: 'arguments',
        format: 'json',
        offset: 0,
        text: '{}',
        totalBytes: 2,
        tool: { toolUseId: 'tool', call: { sequence: 6, name: 'write' } },
      };
      return c.event({
        kind: 'action',
        key: c.state.pending.key,
        event: { status: 'succeeded', value: { kind: 'ready', through: 7, source } },
      });
    },
  };
  return c;
}
const submitted = (requestId, decision, scope) => [
  { name: 'submit', value: { requestId, decision, ...(scope ? { scope } : {}) } },
];
for (const target of ['tui']) {
  test(`${target} permission Allow directly submits its displayed scope`, async () => {
    for (const [scope, label] of [
      ['once', 'Once'],
      ['turn', 'This turn'],
      ['session', 'This conversation'],
    ]) {
      const c = await client(target, 'permission');
      c.click(scope);
      assert.deepEqual(c.out.events, [], 'scope selection alone must not decide');
      c.update({ status: 'Refreshed' });
      assert.equal(c.node('submit'), undefined);
      assert.equal(c.node('allow').kind, 'button');
      assert.match(c.node('allow').label, new RegExp(label, 'i'));
      c.click('allow');
      assert.deepEqual(c.out.events, submitted('first', 'allow', scope));
    }
    const c = await client(target, 'permission');
    c.click('session').click('deny');
    assert.deepEqual(c.out.events, submitted('first', 'deny', 'session'));
    c.raw('submit');
    assert.deepEqual(c.out.events, [], 'the removed generic submit cannot decide');
  });
  test(`${target} service and computer approval buttons submit their own decision`, async () => {
    for (const kind of ['service', 'computer']) {
      const c = await client(target, kind);
      assert.equal(c.node('submit'), undefined);
      assert.equal(c.node('allow').kind, 'button');
      if (kind === 'service') {
        assert.equal(c.node('allow').disabled, true);
        c.raw('allow');
        assert.deepEqual(c.out.events, [], 'service Allow requires the reviewed source');
        c.click('deny');
        assert.deepEqual(c.out.events, submitted('first', 'deny'));
        c.ready();
      }
      c.update({ status: 'Refreshed' }).click('allow');
      assert.deepEqual(c.out.events, submitted('first', 'allow'));
      c.click('deny');
      assert.deepEqual(c.out.events, submitted('first', 'deny'));
      c.raw('submit');
      assert.deepEqual(c.out.events, []);
    }
  });
  test(`${target} approval keeps Close available while decisions are busy, closed or settled`, async () => {
    for (const kind of ['permission', 'service', 'computer']) {
      const c = await client(target, kind);
      const outcome =
        kind === 'permission'
          ? { kind: 'permissions_decision', decision: { decision: 'deny' }, committedAt: 2 }
          : {
              kind: kind === 'service' ? 'service_approval_decision' : 'client_capability_decision',
              decision: 'deny',
              committedAt: 2,
            };
      for (const changes of [
        { busy: true },
        { busy: false, record: { ...c.props.record, outcome } },
      ]) {
        c.update(changes);
        assert.equal(c.node('allow').disabled, true);
        assert.equal(c.node('deny').disabled, true);
        for (const key of ['allow', 'deny', 'once', 'turn', 'session']) {
          c.raw(key);
          assert.deepEqual(c.out.events, []);
        }
        c.click('close');
        assert.deepEqual(c.out.events, [{ name: 'closed', value: {} }]);
      }
      c.update({ open: false }).raw('close');
      assert.deepEqual(c.out.events, []);
    }
  });
  test(`${target} replacement approval rejects the stale request event and resets scope/source`, async () => {
    for (const kind of ['permission', 'service', 'computer']) {
      const c = await client(target, kind);
      if (kind === 'permission') c.click('session');
      if (kind === 'service') c.ready();
      c.click('allow');
      const record = { ...c.props.record, requestId: 'second' };
      c.stale({ record }, 'deny');
      assert.deepEqual(c.out.events, []);
      if (kind === 'service') {
        assert.equal(c.node('allow').disabled, true);
        c.ready();
      }
      c.click('allow');
      assert.deepEqual(
        c.out.events,
        submitted('second', 'allow', kind === 'permission' ? 'once' : undefined),
      );
      const changed = structuredClone(c.props.record);
      changed.request.toolUseId = 'another-tool';
      c.stale({ record: changed }, 'deny');
      assert.deepEqual(c.out.events, [], 'a changed request body also invalidates the old frame');
    }
  });
}
