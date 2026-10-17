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
      title: 'Goal test',
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
    inspector: 'goal',
    dialog: null,
    modelRefreshing: false,
    pending: {},
    restoring: false,
  });
  out.frame.state.goal.page = { enabled: true, current: null };
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
        child: 'goal-field-' + name,
        name: 'changed',
        event: observation,
      });
    },
    reply(value, key = out.actions[0].key) {
      return c.event({ kind: 'action', key, event: { status: 'succeeded', value } });
    },
    ready() {
      for (const name of ['objective', 'iterations', 'budget']) {
        c.field(name, observed(name, 0, ''));
        const cmd = c.state.goal.fields[name].command;
        c.field(
          name,
          observed(name, 1, cmd.edit.text, {
            owner: cmd.owner,
            sequence: cmd.sequence,
            outcome: { status: 'applied' },
          }),
        );
      }
    },
    capture() {
      const texts = { objective: 'Verify 中😀', iterations: '2', budget: '100' };
      for (const [name, text] of Object.entries(texts)) c.field(name, observed(name, 2, text));
      c.raw('goal-start');
      for (const [name, text] of Object.entries(texts)) {
        assert.equal(c.out.actions[0].resources.text.child, 'goal-field-' + name);
        c.reply({ metadata: c.state.goal.fields[name].observation.snapshot.text, text });
      }
    },
  };
  return c;
}
for (const target of ['tui']) {
  test(`${target} Goal captures exact Unicode fields and immutable approval before a mutation`, async () => {
    const c = await client(target);
    c.ready();
    c.capture();
    const frozen = structuredClone(c.state.goal.confirmation);
    assert.equal(frozen.objective, 'Verify 中😀');
    assert.equal(frozen.maxIterations, 2);
    assert.equal(frozen.tokenBudget, 100);
    assert.equal(frozen.approval.target.boundaryRevision, 7);
    assert.equal(frozen.approval.target.permissions.access, 'read_only');
    assert.equal(c.out.actions.length, 0, 'review does not execute');
    c.state.selected.permissions.access = 'workspace_write';
    c.state.selected.workspace.path = '/changed';
    c.state.model.choice.selection.modelId = 'changed';
    c.raw('goal-confirm');
    assert.equal(c.out.actions[0].method, 'changeGoal');
    assert.deepEqual(c.out.actions[0].input, frozen);
  });
  test(`${target} Goal rejects changed text and stale control confirmation`, async () => {
    const c = await client(target);
    c.ready();
    c.field('objective', observed('objective', 2, 'Original'));
    c.raw('goal-start');
    const request = c.out.actions[0],
      metadata = c.state.goal.fields.objective.observation.snapshot.text;
    c.field('objective', observed('objective', 3, 'Changed'));
    c.reply({ metadata, text: 'Original' }, request.key);
    assert.equal(c.state.goal.confirmation, null);
    assert.equal(c.state.goal.capture, null);
    assert.match(JSON.stringify(c.out.frame.view), /Enter an objective, 1–100 iterations/);
    c.state.goal.page.current = {
      id: 'goal',
      revision: 1,
      objective: 'Original',
      status: 'paused',
      iterations: 1,
      maxIterations: 3,
      tokenBudget: null,
      consumed: { known: 3, missing: 0 },
      pending: false,
      report: null,
      note: '',
    };
    c.raw('goal-resume');
    assert.equal(c.state.goal.confirmation.revision, 1);
    c.state.goal.watch = { key: 'watch-goal', input: { sessionId: 'session' } };
    c.event({
      kind: 'action',
      key: 'watch-goal',
      event: {
        status: 'updated',
        value: {
          enabled: true,
          current: { ...c.state.goal.page.current, revision: 2, status: 'cancelled' },
        },
      },
    });
    assert.equal(c.state.goal.confirmation, null);
    c.raw('goal-confirm');
    assert.equal(c.out.actions.length, 0);
  });
  test(`${target} Goal recovers an unknown original operation without creating another objective`, async () => {
    const c = await client(target);
    c.ready();
    c.capture();
    c.raw('goal-confirm');
    const original = c.out.actions[0];
    const reference = {
      source: {
        stateDomain: 'a'.repeat(64),
        deployment: '00000000-0000-0000-0000-000000000001',
        revision: 1,
      },
      namespace: ['00000000-0000-0000-0000-000000000002'],
      export: 'agent',
      method: 'changeGoal',
      operation: '00000000-0000-0000-0000-000000000004',
      fingerprint: 'sha256-' + 'b'.repeat(64),
      contract: {},
      recovery: 'journal_v1',
    };
    c.event({
      kind: 'action',
      key: original.key,
      event: { status: 'operation', reply: { reference, receipt: { kind: 'unknown' } } },
    });
    assert.equal(c.out.actions[0].method, 'goalReceipt');
    assert.deepEqual(c.out.actions[0].input, { reference });
    const recovery = c.out.actions[0];
    c.raw('goal-start');
    assert.equal(c.out.actions.length, 0);
    c.reply({ kind: 'committed', goalId: 'original-goal', revision: 1 }, recovery.key);
    assert.equal(c.state.uncertain, null);
    assert.equal(c.state.goal.confirmation, null);
    assert.equal(c.state.goal.watch.input.sessionId, 'session');
    assert.ok(c.out.actions.every((action) => action.method !== 'changeGoal'));
  });
}
