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
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const uuid = (n) => `00000000-0000-0000-0000-${String(n).padStart(12, '0')}`;
const project = (n, extra = {}) => ({
  reference: { id: uuid(n), revision: 'a'.repeat(64) },
  name: 'Project ' + n,
  path: '/work/' + n,
  archived: false,
  available: true,
  selection,
  ...extra,
});
const selection = {
  rootId: 'root-1',
  segments: ['example'],
  directoryIdentity: 'sha256:' + 'd'.repeat(64),
};
const reference = (n = 1) => ({
  source: { stateDomain: uuid(50), deployment: uuid(51), revision: 1 },
  namespace: [uuid(52)],
  export: 'agent',
  method: 'project',
  operation: uuid(n),
  fingerprint: 'sha256-' + 'c'.repeat(64),
  contract: contracts.agent,
  recovery: 'journal_v1',
});
const reply = (ref, value) => ({
  reference: ref,
  receipt: {
    kind: 'recorded',
    outcome: { kind: 'returned', value },
    cleanup: { kind: 'confirmed' },
  },
});
function nodes(node) {
  return node && typeof node === 'object'
    ? [
        node,
        ...Object.values(node).flatMap((v) => (Array.isArray(v) ? v.flatMap(nodes) : nodes(v))),
      ]
    : [];
}
async function client(target) {
  const { component } = await import(`../dist/packages/projects/${target}.js`);
  let props = { theme, locale: 'en', open: true, busy: false };
  let out = component.mount(props, {});
  return {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    event(input) {
      out = component.event(out.frame.state, input, props, {});
      return this;
    },
    raw(key) {
      return this.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    child(child, name, event) {
      return this.event({ kind: 'component', child, name, event });
    },
    done(value, status = 'succeeded') {
      assert.ok(out.actions[0]);
      return this.event({
        kind: 'action',
        key: out.actions[0].key,
        event: status === 'operation' ? { status, reply: value } : { status, value },
      });
    },
    update(next) {
      props = { ...props, ...next };
      out = component.update(out.frame.state, props, {});
      return this;
    },
    loaded(rows = [project(1)]) {
      this.done([]);
      assert.equal(out.actions[0].method, 'projects');
      return this.done({ kind: 'projects', revision: 'a'.repeat(64), projects: rows, next: null });
    },
  };
}
for (const target of ['tui']) {
  test(`${target}: project registration and relocation capture inspected destination and recover without replay`, async () => {
    const c = (await client(target)).loaded();
    const keys = nodes(c.out.frame.view)
      .filter(
        (n) =>
          ['stack', 'text', 'button', 'slot', 'item', 'context_menu', 'dialog'].includes(n.kind) &&
          typeof n.key === 'string',
      )
      .map((n) => n.key);
    assert.equal(
      new Set(keys).size,
      keys.length,
      'all scene identities are unique, including hidden dialogs',
    );
    c.raw('add').child('project-directory', 'selected', selection);
    assert.deepEqual(c.out.actions[0].input, { kind: 'directory', selection });
    const location = { selection, path: '/work/example', name: 'example', project: null };
    c.done({ kind: 'directory', location });
    assert.equal(c.out.actions.length, 0);
    c.raw('confirm');
    const action = c.out.actions[0];
    assert.equal(action.method, 'project');
    assert.deepEqual(action.input, { kind: 'register', selection, expected: null });
    assert.deepEqual(action.recovery, { kind: 'projects/v1', input: action.input });
    const ref = reference();
    c.done(
      {
        reference: ref,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'unknown', diagnostic: 'lost' },
          cleanup: { kind: 'confirmed' },
        },
      },
      'operation',
    );
    assert.equal(c.state.notice, 'projectsUnknown');
    assert.ok(nodes(c.out.frame.view).some((n) => n.key === 'confirm-receipt'));
    c.update({ open: false }).update({ open: true }).raw('confirm');
    assert.equal(c.out.actions.length, 0);
    c.raw('confirm-receipt');
    assert.equal(c.out.actions[0].method, 'read');
    assert.deepEqual(c.out.actions[0].input, ref);
    c.done(reply(ref, { kind: 'committed', project: project(2) }));
    assert.equal(c.out.actions[0].method, 'projects');
    assert.ok(c.out.events.some((e) => e.name === 'changed'));
    c.done({
      kind: 'projects',
      revision: 'b'.repeat(64),
      projects: [project(1), project(2)],
      next: null,
    });
    c.raw('relocate-' + uuid(1)).child('project-directory', 'selected', selection);
    c.done({ kind: 'directory', location: { ...location, project: project(2) } });
    assert.ok(nodes(c.out.frame.view).some((n) => n.key === 'merge-help'));
    c.raw('confirm');
    assert.deepEqual(c.out.actions[0].input, {
      kind: 'relocate',
      project: project(1).reference,
      selection,
      destination: project(2).reference,
    });
    c.done(reply(reference(2), { kind: 'changed' }), 'operation');
    assert.equal(c.state.notice, 'projectsChanged');
    assert.equal(c.state.current, null);
    assert.equal(c.out.actions.length, 0);
  });
  test(`${target}: remount follows complete original history and reads only the latest project receipt`, async () => {
    const c = await client(target),
      older = reference(7),
      latest = reference(8);
    const input = { kind: 'archive', project: project(1).reference };
    c.done([{ cursor: 1, reference: older, recovery: { kind: 'projects/v1', input } }]);
    assert.deepEqual(c.out.actions[0].input, { after: 1, limit: 16 });
    c.done([{ cursor: 2, reference: latest, recovery: { kind: 'projects/v1', input } }]);
    c.done([{ cursor: 3, reference: { ...reference(9), method: 'send' }, recovery: null }]);
    c.done([]);
    assert.deepEqual(c.out.actions[0].input, latest);
    assert.equal(c.out.actions[0].method, 'read');
    c.done({ reference: older, receipt: { kind: 'not_recorded' } });
    assert.deepEqual(c.state.current.reference, latest);
    c.raw('confirm')
      .raw('add')
      .raw('create-' + uuid(1));
    assert.equal(c.out.actions.length, 0);
    c.raw('receipt').done(
      reply(latest, { kind: 'committed', project: project(1, { archived: true }) }),
    );
    c.done({
      kind: 'projects',
      revision: 'b'.repeat(64),
      projects: [project(1, { archived: true })],
      next: null,
    });
    c.raw('create-' + uuid(1));
    assert.equal(c.out.events.length, 0);
    c.raw('archive-' + uuid(1)).raw('confirm');
    assert.equal(c.out.actions[0].input.kind, 'restore');
  });
  test(`${target}: native rename waits for reset receipt, preserves hidden editor, and submits exact snapshot`, async () => {
    const c = (await client(target)).loaded();
    c.raw('rename-' + uuid(1));
    const text = { owner: uuid(20), revision: 0, bytes: 0 };
    const observation = { revision: 0, snapshot: { text, marked: false }, receipt: null };
    c.child('project-name', 'changed', observation);
    assert.equal(c.state.command.edit.text, project(1).name);
    c.raw('rename-save');
    assert.equal(c.out.actions.length, 0);
    const command = c.state.command;
    c.child('project-name', 'changed', {
      ...observation,
      revision: 1,
      snapshot: { text: { ...text, revision: 1, bytes: 9 }, marked: false },
      receipt: { owner: text.owner, sequence: command.sequence, outcome: { status: 'applied' } },
    });
    c.update({ open: false }).update({ open: true });
    assert.equal(c.state.command, null);
    c.raw('rename-save');
    const read = c.out.actions[0];
    assert.equal(read.resources.text.input.revision, 1);
    c.done({ text: 'Research!', metadata: { ...text, revision: 1, bytes: 9 } });
    assert.equal(c.out.actions[0].method, 'project');
    assert.deepEqual(c.out.actions[0].input, {
      kind: 'rename',
      project: project(1).reference,
      name: 'Research!',
    });
  });
}
