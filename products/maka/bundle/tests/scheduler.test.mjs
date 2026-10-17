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
import { contracts } from '../../../../packages/plugin-sdk/src/contracts.ts';
import Ajv from 'ajv/dist/2020.js';
import addFormats from 'ajv-formats';
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const uuid = (n) => `00000000-0000-0000-0000-${String(n).padStart(12, '0')}`;

const ajv = new Ajv({ strict: false, formats: { uint64: true, uint32: true } });
addFormats(ajv);
const widgets = {
  input: ajv.compile(contracts.textInput.interface.props),
  editor: ajv.compile(contracts.textarea.interface.props),
};
function checkFrame(out) {
  for (const node of nodes(out.frame.view)) {
    const validate = widgets[node.binding];
    if (validate) assert.ok(validate(node.props), JSON.stringify(validate.errors));
  }
  return out;
}
const validate = ajv.compile(contracts.agent.interface.methods.schedule.input);
const reference = {
  source: { stateDomain: uuid(50), deployment: uuid(51), revision: 1 },
  namespace: [uuid(52)],
  export: 'agent',
  method: 'schedule',
  operation: uuid(1),
  fingerprint: 'sha256-' + 'a'.repeat(64),
  contract: contracts.agent,
  recovery: 'journal_v1',
};
const unknown = {
  reference,
  receipt: {
    kind: 'recorded',
    outcome: { kind: 'unknown', diagnostic: 'lost reply' },
    cleanup: { kind: 'confirmed' },
  },
};
const session = {
  id: 'original-source',
  title: 'Source',
  archived: false,
  workspace: {
    path: '/work/original',
    identity: 'workspace:v1:' + uuid(10),
    directoryIdentity: 'sha256:' + 'c'.repeat(64),
  },
  permissions: { access: 'read_only', requestPermissions: false },
  boundaryRevision: 3,
};
const model = {
  choice: {
    selection: { connectionId: uuid(9), revision: 2, modelId: 'model' },
    connectionName: 'Fixture',
    label: 'Model',
    thinkingLevels: [],
    defaultThinkingLevel: null,
  },
  thinkingLevel: null,
};
const names = ['title', 'body', 'at', 'seconds', 'cron', 'timezone', 'max', 'expiry'];
const values = {
  title: 'Scheduled café 中😀',
  body: 'Preserve the complete original instructions',
  at: '2026-11-01T09:00:00+08:00',
  seconds: '3600',
  cron: '0 9 * * *',
  timezone: 'Asia/Singapore',
  max: '',
  expiry: '',
};
function nodes(node) {
  return node && typeof node === 'object'
    ? [
        node,
        ...Object.values(node).flatMap((v) => (Array.isArray(v) ? v.flatMap(nodes) : nodes(v))),
      ]
    : [];
}
async function client(target) {
  const { component } = await import(`../dist/packages/scheduler/${target}.js`);
  let props = { theme, locale: 'en', open: true, busy: false };
  let out = checkFrame(component.mount(props, {}));
  return {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    event(input) {
      out = checkFrame(component.event(out.frame.state, input, props, {}));
      return this;
    },
    raw(key, event = { kind: 'submit' }) {
      return this.event({ kind: 'raw', key, event });
    },
    child(child, name, event) {
      return this.event({ kind: 'component', child, name, event });
    },
    done(value, status = 'succeeded') {
      const key = out.actions[0]?.key ?? this.state.pending?.key;
      assert.ok(key);
      return this.event({
        kind: 'action',
        key,
        event: status === 'operation' ? { status, reply: value } : { status, value },
      });
    },
    watch(value) {
      return this.event({
        kind: 'action',
        key: this.state.watch.key,
        event: { status: 'updated', value },
      });
    },
    update(next) {
      props = { ...props, ...next };
      out = checkFrame(component.update(out.frame.state, props, {}));
      return this;
    },
    loaded(tasks = []) {
      return this.done([]).watch({ kind: 'page', revision: 4, tasks, nextCursor: null });
    },
  };
}
function edit(c, name, value) {
  const text = { owner: uuid(20 + names.indexOf(name)), revision: 0, bytes: 0 };
  c.child('schedule-field-' + name, 'changed', {
    revision: 0,
    snapshot: { text, marked: false },
    receipt: null,
  });
  const command = c.state.fields[name].command;
  if (command)
    c.child('schedule-field-' + name, 'changed', {
      revision: 1,
      snapshot: {
        text: { ...text, revision: 1, bytes: Buffer.byteLength(command.edit.text) },
        marked: false,
      },
      receipt: { owner: text.owner, sequence: command.sequence, outcome: { status: 'applied' } },
    });
  c.child('schedule-field-' + name, 'changed', {
    revision: 2,
    snapshot: { text: { ...text, revision: 2, bytes: Buffer.byteLength(value) }, marked: false },
    receipt: null,
  });
}
function capture(c) {
  c.raw('review');
  for (const name of names) {
    assert.equal(c.out.actions[0].resources.text.child, 'schedule-field-' + name);
    const observed = c.state.pending.request.observation.snapshot.text;
    assert.deepEqual(c.out.actions[0].resources.text.input, {
      kind: 'at_revision',
      owner: observed.owner,
      revision: observed.revision,
    });
    c.done({ text: values[name], metadata: observed });
  }
}
function detail(revision = 7) {
  return {
    revision,
    timezone: 'UTC',
    misfire: 'skip',
    approved: true,
    pendingFire: null,
    task: {
      id: 'task-original',
      title: 'Original title',
      intent: { kind: 'text', body: 'Original instruction' },
      schedule: { kind: 'interval', everySeconds: 3600, startAt: 1800000000000 },
      effect: {
        kind: 'session',
        target: {
          sessionId: session.id,
          workspace: session.workspace,
          permissions: session.permissions,
          boundaryRevision: session.boundaryRevision,
        },
        model: model.choice.selection,
        thinkingLevel: null,
      },
      status: 'paused',
      nextFireAt: null,
      lastFireAt: null,
      fireCount: 0,
      maxFires: null,
      expiresAt: null,
      createdBy: { kind: 'user' },
      createdAt: 1700000000000,
      updatedAt: 1700000000000,
      runs: [],
      lastError: null,
    },
  };
}
const row = {
  id: 'task-original',
  revision: 7,
  title: 'Original title',
  status: 'paused',
  approved: true,
  nextFireAt: null,
  pendingFire: null,
  reminder: false,
  lastError: null,
};
for (const target of ['tui']) {
  test(`${target}: scheduler captures retained native fields and recovers the original business receipt without resubmission`, async () => {
    const c = (await client(target)).loaded().raw('new');
    for (const name of names) edit(c, name, values[name]);
    c.raw('effect', { kind: 'change', key: 'agent_run' }).raw('source');
    c.done({
      sessions: [session],
      revision: 'catalog',
      after: null,
      before: null,
      needsYouCount: 0,
    })
      .raw('source-' + session.id)
      .child('schedule-models', 'selected', model);
    const fields = structuredClone(c.state.fields);
    c.update({ open: false }).update({ open: true });
    assert.deepEqual(c.state.fields, fields);
    capture(c);
    const input = c.state.confirm;
    assert.equal(input.kind, 'create');
    assert.ok(validate(input), JSON.stringify(validate.errors));
    assert.equal(input.input.task.schedule.runAt, Date.parse(values.at));
    assert.deepEqual(input.input.task.effect.target, {
      sessionId: session.id,
      workspace: session.workspace,
      permissions: session.permissions,
      boundaryRevision: 3,
    });
    assert.ok(
      nodes(c.out.frame.view).some(
        (n) => n.key === 'schedule-confirm-path' && n.text === session.workspace.path,
      ),
    );
    c.raw('confirm');
    assert.deepEqual(c.out.actions[0].recovery, { kind: 'scheduled-task/v1', input });
    c.done(unknown, 'operation');
    assert.deepEqual(c.out.actions[0].input, { kind: 'receipt', reference });
    c.done({ kind: 'receipt', result: null });
    c.update({ open: false }).update({ open: true }).raw('confirm');
    assert.equal(c.out.actions.length, 0);
    assert.ok(c.state.current);
    c.raw('confirm-receipt');
    assert.deepEqual(c.out.actions[0].input, reference);
    c.done(unknown).done({
      kind: 'receipt',
      result: { kind: 'committed', taskId: 'created-task', revision: 1 },
    });
    assert.equal(c.state.current, null);
    assert.equal(c.state.route, 'detail');
    assert.deepEqual(c.out.subscriptions[0].input, { kind: 'get', taskId: 'created-task' });
  });
  test(`${target}: scheduler direct context actions retain the consent revision while live observations advance`, async () => {
    const c = (await client(target)).loaded([row]);
    c.raw('context-state-task-original');
    assert.deepEqual(c.out.subscriptions[0].input, { kind: 'get', taskId: 'task-original' });
    assert.equal(c.state.confirm, null);
    c.watch({ kind: 'task', detail: detail() });
    assert.deepEqual(c.state.confirm, { kind: 'resume', taskId: 'task-original', revision: 7 });
    const changed = detail(8);
    changed.task.title = 'Changed remotely';
    changed.task.effect.target.workspace = { ...session.workspace, path: '/work/changed' };
    c.watch({ kind: 'task', detail: changed });
    assert.equal(c.state.confirmTask.task.title, 'Original title');
    assert.ok(
      nodes(c.out.frame.view).some(
        (n) => n.key === 'schedule-confirm-path' && n.text === '/work/original',
      ),
    );
    c.raw('confirm');
    assert.equal(c.out.actions[0].input.revision, 7);
    c.done(
      {
        reference,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'returned', value: { kind: 'conflict', actual: 8 } },
          cleanup: { kind: 'confirmed' },
        },
      },
      'operation',
    );
    assert.equal(c.state.notice, 'scheduleChanged');
    assert.equal(c.state.confirm, null);
    c.raw('back')
      .watch({ kind: 'page', revision: 10, tasks: [row], nextCursor: null })
      .raw('context-delete-task-original')
      .watch({ kind: 'task', detail: changed });
    assert.equal(
      c.state.confirm,
      null,
      'a stale clicked row never confirms against a replacement observation',
    );
    assert.equal(c.state.notice, 'scheduleChanged');
  });
  test(`${target}: scheduler remount follows paginated caller history before accepting a reminder deep link`, async () => {
    const c = await client(target);
    const input = { kind: 'delete', taskId: 'old-task', revision: 4 };
    c.update({ requested: { sequence: 1, taskId: 'pending-reminder' } });
    c.done([{ cursor: 12, reference, recovery: { kind: 'scheduled-task/v1', input } }]);
    assert.deepEqual(c.out.actions[0].input, { after: 12, limit: 16 });
    c.done([]);
    assert.deepEqual(c.out.actions[0].input, reference);
    c.done(unknown);
    assert.deepEqual(c.out.actions[0].input, { kind: 'receipt', reference });
    c.done({ kind: 'receipt', result: { kind: 'committed', taskId: 'old-task', revision: 5 } });
    assert.deepEqual(c.out.subscriptions[0].input, { kind: 'get', taskId: 'pending-reminder' });
    assert.equal(c.out.actions.length, 0);
    const d = detail(12);
    d.task.id = 'pending-reminder';
    d.task.effect = { kind: 'reminder' };
    d.pendingFire = 'original-fire';
    c.watch({ kind: 'task', detail: d }).raw('acknowledge');
    assert.deepEqual(c.out.actions[0].input, {
      kind: 'acknowledge',
      taskId: 'pending-reminder',
      revision: 12,
      fireId: 'original-fire',
    });
  });
}
