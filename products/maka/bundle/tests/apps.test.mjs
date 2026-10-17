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
const source = { stateDomain: uuid(50), deployment: uuid(51), revision: 2 };
const packageRef = { id: 'example.notes', revision: 'a'.repeat(64) };
const target = { kind: 'instance', path: ['editor'], export: 'Editor' };
const candidate = {
  kind: 'package',
  key: 'notes',
  label: 'Notes',
  package: packageRef,
  export: 'Page',
};
const reference = (method = 'editComposition') => ({
  source,
  namespace: [uuid(52)],
  export: 'Management',
  method,
  operation: uuid(1),
  fingerprint: 'sha256-' + 'c'.repeat(64),
  contract: contracts.deploymentManager,
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
async function client(presentation, entries = []) {
  const module = await import(`../dist/packages/apps/${presentation}.js`);
  module.activate({ root: 'apps', entries });
  const component = module.component;
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
    raw(key, kind = 'submit') {
      return this.event({ kind: 'raw', key, event: { kind } });
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
    loaded() {
      return this.done([]).done({
        source,
        configuration: { root: 'apps', entries },
        current: entries.map((e) => ({ kind: 'instance', path: [e.id], export: 'Page' })),
        candidates: [candidate],
        next: null,
      });
    },
  };
}
const settings = {
  source,
  package: packageRef,
  path: [],
  next: null,
  fields: [
    {
      key: 'title',
      label: 'Title',
      kind: 'text',
      required: true,
      present: true,
      editable: true,
      value: { kind: 'text', text: 'Notes' },
      options: [],
    },
  ],
  imports: [{ name: 'editor', contract: 'maka.ui.Textarea', cardinality: { kind: 'one' } }],
};
function edit(c, name, value) {
  const text = { owner: uuid(name === 'name' ? 20 : 21), revision: 0, bytes: 0 };
  c.child('apps-' + name, 'changed', {
    revision: 0,
    snapshot: { text, marked: false },
    receipt: null,
  });
  const command = c.state.fields[name].command;
  if (command)
    c.child('apps-' + name, 'changed', {
      revision: 1,
      snapshot: {
        text: { ...text, revision: 1, bytes: Buffer.byteLength(command.edit.text) },
        marked: false,
      },
      receipt: { owner: text.owner, sequence: command.sequence, outcome: { status: 'applied' } },
    });
  c.child('apps-' + name, 'changed', {
    revision: 2,
    snapshot: { text: { ...text, revision: 2, bytes: Buffer.byteLength(value) }, marked: false },
    receipt: null,
  });
}
function read(c, value) {
  return c.done({ text: value, metadata: c.state.pending.request.observation.snapshot.text });
}
for (const presentation of ['tui']) {
  test(
    presentation +
      ' Apps freezes explicit settings and bindings, then observes an unknown original operation',
    async () => {
      const c = (await client(presentation)).loaded();
      c.raw('add').done({
        source,
        configuration: { root: 'apps', entries: [] },
        current: [],
        candidates: [candidate],
        next: null,
      });
      c.raw('candidate-0');
      assert.equal(c.out.actions[0].method, 'packageSettings');
      c.done(settings);
      edit(c, 'name', 'Personal notes');
      c.raw('review');
      read(c, 'Personal notes');
      assert.equal(c.state.confirm, null);
      assert.equal(c.state.notice, 'appsBindingsMissing');
      c.raw('setting-0');
      edit(c, 'setting', 'My notebook');
      c.raw('setting-apply');
      read(c, 'My notebook');
      assert.deepEqual(c.out.actions[0].input.changes, [
        { path: ['title'], value: { kind: 'text', text: 'My notebook' } },
      ]);
      c.done({
        ...settings,
        fields: [{ ...settings.fields[0], value: { kind: 'text', text: 'My notebook' } }],
      });
      c.raw('choose-binding-0');
      assert.equal(c.out.actions[0].input.includePackages, false);
      c.done({
        source,
        configuration: {},
        current: [],
        candidates: [{ kind: 'bound', key: 'editor', label: 'Editor', target }],
        next: null,
      });
      c.raw('binding-0').raw('bindings-done').raw('review');
      read(c, 'Personal notes');
      const frozen = structuredClone(c.state.confirm);
      assert.equal(frozen.input.edits.length, 2);
      assert.deepEqual(frozen.input.edits[0].bindings, { editor: [target] });
      assert.deepEqual(frozen.input.edits[0].settings, [
        { path: ['title'], value: { kind: 'text', text: 'My notebook' } },
      ]);
      c.raw('confirm');
      assert.equal(c.out.actions[0].method, 'editComposition');
      assert.deepEqual(c.out.actions[0].input, frozen.input);
      const recovery = c.out.actions[0].recovery;
      const ref = reference();
      c.done({ reference: ref, receipt: { kind: 'unknown' } }, 'operation');
      assert.equal(c.state.notice, 'appsUnknown');
      const lookup = c.out.actions[0];
      assert.equal(lookup.binding, 'receipts');
      assert.deepEqual(lookup.input, ref);
      c.raw('confirm');
      assert.equal(c.out.actions.length, 0);
      c.raw('confirm-cancel');
      assert.deepEqual(c.state.confirm, frozen);
      c.raw('confirm-receipt');
      assert.equal(c.state.pending.key, lookup.key, 'the original automatic query stays in flight');
      c.event({
        kind: 'action',
        key: lookup.key,
        event: {
          status: 'succeeded',
          value: reply(ref, { status: 'committed', value: { ...source, revision: 3 } }),
        },
      });
      assert.equal(c.out.actions[0].method, 'bindingCandidates');
      assert.equal(c.state.current, null);
      // A short history page still has a continuation. The mutation is never repeated.
      const recovered = await client(presentation);
      recovered.done([{ cursor: 7, reference: ref, recovery }]);
      assert.deepEqual(recovered.out.actions[0].input, { after: 7, limit: 16 });
      recovered.done([]);
      assert.equal(recovered.out.actions[0].binding, 'receipts');
      assert.deepEqual(recovered.out.actions[0].input, ref);
      recovered.done({ reference: ref, receipt: { kind: 'unknown' } });
      recovered.raw('add');
      assert.equal(recovered.out.actions.length, 0);
    },
  );
  test(
    presentation +
      ' Apps retains hidden pages, closes exact mounts, and only unlinks removed entries',
    async () => {
      const entries = [
        { id: 'notes', label: 'Notes' },
        { id: 'clock', label: 'Clock' },
      ];
      const c = (await client(presentation, entries)).loaded();
      // Frame shapes differ between native presentations; discover through the full output.
      const pages = () => nodes(c.out).filter((n) => n.binding === 'pages');
      assert.equal(pages().length, 0);
      c.raw('open-notes');
      assert.equal(pages().length, 1);
      assert.equal(pages()[0].props.open, true);
      assert.equal(pages()[0].member, 0);
      c.raw('back');
      assert.equal(pages().length, 1);
      assert.equal(pages()[0].props.open, false);
      c.raw('open-clock');
      assert.equal(pages().length, 2);
      assert.equal(pages()[1].member, 1);
      c.update({ open: false });
      assert.ok(pages().every((p) => p.props.open === false));
      c.update({ open: true });
      c.done({
        source,
        configuration: { root: 'apps', entries },
        current: entries.map((e) => ({ kind: 'instance', path: [e.id], export: 'Page' })),
        candidates: [],
        next: null,
      });
      c.raw('close-current');
      assert.deepEqual(c.state.opened, ['notes']);
      c.child('app-notes', 'closed', {});
      assert.equal(pages().length, 0);
      c.raw('remove-0');
      assert.deepEqual(c.state.confirm.input.edits, [
        {
          kind: 'update',
          name: 'apps',
          configuration: { entries: [entries[1]] },
          bindings: { pages: [{ kind: 'instance', path: ['clock'], export: 'Page' }] },
        },
      ]);
      c.raw('confirm-cancel');
      assert.equal(c.state.confirm, null);
      c.raw('open-notes');
      assert.equal(pages().length, 1);
    },
  );
  test(presentation + ' Apps installs only the reviewed native file observation', async () => {
    const c = (await client(presentation)).loaded();
    c.raw('file');
    const file = {
      owner: uuid(60),
      revision: 3,
      phase: 'ready',
      file: { name: 'notes.maka', bytes: 128 },
    };
    c.child('apps-file', 'changed', file).raw('install');
    c.child('apps-file', 'changed', { ...file, revision: 4 });
    c.raw('confirm');
    const action = c.out.actions[0];
    assert.equal(action.method, 'install');
    assert.deepEqual(action.resources.package, {
      child: 'apps-file',
      method: 'snapshot',
      input: { owner: file.owner, revision: 3 },
    });
    assert.deepEqual(action.recovery.input.file, file);
  });
  test(
    presentation + ' Apps exposes URL installation and fences edited or stale links',
    async () => {
      const c = (await client(presentation)).loaded();
      const url = 'https://example.com/notes.maka-plugin';
      const observed = (revision) => ({
        revision,
        snapshot: { text: { owner: uuid(61), revision, bytes: url.length }, marked: null },
      });
      c.raw('file').raw('apps-remote').child('apps-url', 'changed', observed(1)).raw('install');
      assert.deepEqual(c.out.actions[0].resources.text, {
        child: 'apps-url',
        method: 'snapshot',
        input: { kind: 'at_revision', owner: uuid(61), revision: 1 },
      });
      c.done({ text: url, metadata: observed(1).snapshot.text });
      assert.equal(c.state.confirm.url, url);
      c.child('apps-url', 'changed', observed(2)).raw('confirm');
      assert.equal(c.state.confirm, null);
      assert.deepEqual(
        c.out.actions,
        [],
        'an edited URL cannot install the previously reviewed link',
      );
      c.raw('install').done({ text: url, metadata: observed(1).snapshot.text });
      assert.equal(c.state.confirm, null, 'stale native text is rejected');
      c.raw('install')
        .done({ text: url, metadata: observed(2).snapshot.text })
        .raw('confirm');
      assert.equal(c.out.actions[0].method, 'installUrl');
      assert.deepEqual(c.out.actions[0].input, { expectedRevision: 2, url, sha256: null });
      assert.deepEqual(c.out.actions[0].resources, {});
      const ref = reference('installUrl');
      c.done({ reference: ref, receipt: { kind: 'unknown' } }, 'operation');
      const lookup = c.out.actions[0];
      assert.equal(lookup.binding, 'receipts');
      assert.deepEqual(lookup.input, ref);
      c.raw('confirm');
      assert.deepEqual(c.out.actions, [], 'uncertain installs are not repeated');
      c.raw('receipt');
      assert.equal(c.state.pending.key, lookup.key);
      c.event({
        kind: 'action',
        key: lookup.key,
        event: {
          status: 'succeeded',
          value: reply(ref, { status: 'committed', value: { source, package: packageRef } }),
        },
      });
      assert.equal(c.state.route, 'choose');
      assert.equal(c.state.notice, 'appsPackageAdded');
    },
  );
}
