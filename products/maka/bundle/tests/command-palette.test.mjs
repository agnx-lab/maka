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
const session = {
  id: 'session-a',
  title: 'Review alpha',
  labels: [],
  workspace: { path: '/alpha', directoryIdentity: 'a'.repeat(64), identity: 'workspace-a' },
  dataSource: {
    stateDomain: 'a'.repeat(64),
    deployment: '00000000-0000-0000-0000-000000000001',
    namespace: ['00000000-0000-0000-0000-000000000002'],
  },
  permissions: { access: 'workspace_write', requestPermissions: true },
  boundaryRevision: 0,
  revision: 1,
  updatedAt: 1,
  archived: false,
  hasUnread: false,
  activity: 'idle',
};
const commands = [
  { id: 'new', label: 'New session', hint: '', disabled: false },
  { id: 'settings', label: 'Settings', hint: '', disabled: false },
];
async function client(target) {
  const { component } = await import(`../dist/packages/command-palette/${target}.js`);
  const props = {
    theme,
    locale: 'en',
    open: true,
    busy: false,
    selectionDisabled: false,
    commands,
  };
  let out = structuredClone(component.mount(props));
  const accept = (next) => {
    out = structuredClone(next);
  };
  return {
    get out() {
      return out;
    },
    event(input) {
      accept(component.event(out.frame.state, input, props));
      return this;
    },
    update(next) {
      Object.assign(props, next);
      accept(component.update(out.frame.state, props));
      return this;
    },
    query(text, revision) {
      return this.event({
        kind: 'component',
        child: 'palette-query',
        name: 'changed',
        event: {
          revision,
          receipt: null,
          snapshot: {
            text: { owner: 'query', revision, bytes: Buffer.byteLength(text) },
            marked: null,
            selection: { start: Buffer.byteLength(text), end: Buffer.byteLength(text) },
            reversed: false,
          },
        },
      });
    },
    enter() {
      return this.event({
        kind: 'component',
        child: 'palette-query',
        name: 'submitted',
        event: {},
      });
    },
    action(method) {
      return out.actions.find((a) => a.method === method);
    },
    result(action, value) {
      assert.ok(action, 'expected an actual action');
      return this.event({ kind: 'action', key: action.key, event: { status: 'succeeded', value } });
    },
    reply(action, text) {
      return this.result(action, {
        text,
        metadata: {
          owner: 'query',
          revision: action.resources.text.input.revision,
          bytes: Buffer.byteLength(text),
        },
      });
    },
  };
}
for (const target of ['tui']) {
  test(`${target} Enter waits for the submitted query and invokes its matching command`, async () => {
    const c = await client(target);
    c.query('', 0).query('Settings', 8);
    const query = c.action('read');
    c.enter();
    assert.deepEqual(c.out.events, []);
    c.reply(query, 'Settings');
    const selection = c.action('read');
    assert.ok(selection, 'submission must continue after its query resolves');
    assert.deepEqual(selection.resources.text.input, {
      kind: 'at_revision',
      owner: 'query',
      revision: 8,
    });
    c.reply(selection, 'Settings');
    assert.deepEqual(c.out.events, [{ name: 'invoked', value: { id: 'settings' } }]);
    c.reply(query, 'Settings');
    assert.deepEqual(c.out.events, []);
  });

  test(`${target} Enter follows its query past an older in-flight read`, async () => {
    const c = await client(target);
    c.query('', 0).query('S', 1);
    const old = c.action('read');
    c.query('Settings', 8).enter();
    c.reply(old, 'S');
    const current = c.action('read');
    assert.deepEqual(current.resources.text.input, {
      kind: 'at_revision',
      owner: 'query',
      revision: 8,
    });
    c.reply(current, 'Settings');
    c.reply(c.action('read'), 'Settings');
    assert.deepEqual(c.out.events, [{ name: 'invoked', value: { id: 'settings' } }]);
  });
  for (const interruption of ['edit', 'close', 'busy']) {
    test(`${target} ${interruption} cancels a deferred Enter without replaying it later`, async () => {
      const c = await client(target);
      c.query('', 0).query('Settings', 8);
      const query = c.action('read');
      c.enter();
      if (interruption === 'edit') c.query('New', 9);
      else if (interruption === 'close') {
        c.event({ kind: 'raw', key: 'close', event: {} });
        assert.deepEqual(c.out.events, [{ name: 'closed', value: {} }]);
        c.update({ open: false }).update({ open: true });
      } else c.update({ busy: true }).update({ busy: false });
      c.reply(query, 'Settings');
      const text = interruption === 'edit' ? 'New' : 'Settings';
      if (interruption === 'edit') c.reply(c.action('read'), text);
      assert.deepEqual(c.out.events, []);
      assert.equal(c.action('read'), undefined);
      c.enter();
      c.reply(c.action('read'), text);
      assert.deepEqual(c.out.events, [
        { name: 'invoked', value: { id: interruption === 'edit' ? 'new' : 'settings' } },
      ]);
    });
  }
  test(`${target} changing text during selection verification cannot invoke the old command`, async () => {
    const c = await client(target);
    c.query('', 0).query('Settings', 8);
    const query = c.action('read');
    c.enter().reply(query, 'Settings');
    const selection = c.action('read');
    c.query('New', 9).reply(selection, 'Settings');
    c.reply(c.action('read'), 'New');
    assert.deepEqual(c.out.events, []);
    c.enter().reply(c.action('read'), 'New');
    assert.deepEqual(c.out.events, [{ name: 'invoked', value: { id: 'new' } }]);
  });
  test(`${target} deferred Enter waits for a matching session page`, async () => {
    const c = await client(target);
    c.query('', 0).query('alpha', 5);
    c.reply(c.action('read'), 'alpha');
    const directory = c.action('list');
    c.enter().result(directory, {
      revision: 'catalog-7',
      before: null,
      needsYouCount: 0,
      sessions: [session],
      after: null,
    });
    c.reply(c.action('read'), 'alpha');
    assert.deepEqual(c.out.events, [{ name: 'selected', value: session }]);
  });
  test(`${target} disabled commands and clipboard actions cannot be admitted by deferred Enter`, async () => {
    for (const option of [{ disabled: true }, { copyText: 'private command' }]) {
      const c = await client(target);
      c.update({ commands: [{ ...commands[1], ...option }] })
        .query('', 0)
        .query('Settings', 8);
      const query = c.action('read');
      c.enter().reply(query, 'Settings');
      assert.deepEqual(c.out.events, []);
      assert.equal(c.action('read'), undefined);
      c.update({ commands });
      c.enter().reply(c.action('read'), 'Settings');
      assert.deepEqual(c.out.events, [{ name: 'invoked', value: { id: 'settings' } }]);
    }
  });
  test(`${target} repeated Enter while waiting invokes once`, async () => {
    const c = await client(target);
    c.query('', 0).query('Settings', 8);
    const query = c.action('read');
    c.enter().enter().reply(query, 'Settings');
    const selection = c.action('read');
    c.enter().reply(selection, 'Settings');
    assert.deepEqual(c.out.events, [{ name: 'invoked', value: { id: 'settings' } }]);
    c.reply(selection, 'Settings');
    assert.deepEqual(c.out.events, []);
  });
  test(`${target} a failed query cancels Enter across refresh`, async () => {
    const c = await client(target);
    c.query('', 0).query('Settings', 8);
    const query = c.action('read');
    c.enter().event({
      kind: 'action',
      key: query.key,
      event: { status: 'failed', reason: 'unavailable' },
    });
    c.event({ kind: 'raw', key: 'refresh', event: {} });
    c.reply(c.action('read'), 'Settings');
    assert.deepEqual(c.out.events, []);
    assert.equal(c.action('read'), undefined);
    c.enter().reply(c.action('read'), 'Settings');
    assert.deepEqual(c.out.events, [{ name: 'invoked', value: { id: 'settings' } }]);
  });
  test(`${target} a changed directory cancels Enter across refresh`, async () => {
    const c = await client(target);
    c.query('', 0).query('alpha', 5);
    c.reply(c.action('read'), 'alpha');
    c.result(c.action('list'), {
      revision: 'catalog-7',
      before: null,
      needsYouCount: 0,
      sessions: [],
      after: 'next-page',
    });
    const next = c.action('list');
    c.enter().result(next, {
      revision: 'catalog-8',
      before: null,
      needsYouCount: 0,
      sessions: [session],
      after: null,
    });
    assert.deepEqual(c.out.events, []);
    c.event({ kind: 'raw', key: 'refresh', event: {} });
    c.result(c.action('list'), {
      revision: 'catalog-8',
      before: null,
      needsYouCount: 0,
      sessions: [session],
      after: null,
    });
    assert.deepEqual(c.out.events, []);
    assert.equal(c.action('read'), undefined);
    c.enter().reply(c.action('read'), 'alpha');
    assert.deepEqual(c.out.events, [{ name: 'selected', value: session }]);
  });
}
