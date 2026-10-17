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
const selection = {
  rootId: 'root-1',
  segments: ['example'],
  directoryIdentity: 'sha256:' + 'd'.repeat(64),
};
const reference = (n = 1) => ({
  source: { stateDomain: uuid(50), deployment: uuid(51), revision: 1 },
  namespace: [uuid(52)],
  export: 'agent',
  method: 'import',
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
  const { component } = await import(`../dist/packages/conversation-import/${target}.js`);
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
    raw(key, kind = 'activate') {
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
    loaded(rows = [source]) {
      this.done([]);
      assert.equal(out.actions[0].method, 'imports');
      return this.done({ kind: 'sources', revision: 1, sources: rows });
    },
  };
}
const source = {
  id: uuid(11),
  name: 'Codex source',
  path: '/sources/codex',
  location: { kind: 'codex', directory: selection },
};
const sourceRef = { id: source.id, revision: 1 };
const entry = {
  id: 'foreign',
  path: 'sessions/rollout-foreign.jsonl',
  title: 'Original conversation',
  cwd: '/foreign',
  updatedAt: 123,
  archived: false,
};
const reviewed = {
  selection: {
    source: sourceRef,
    entry: { sessionId: entry.id, path: entry.path },
    digest: 'f'.repeat(64),
  },
  title: entry.title,
  cwd: entry.cwd,
  records: 2,
  bytes: 100,
  incompleteTail: false,
  items: [{ index: 0, kind: 'user', text: 'Hello source', truncated: false }],
  next: 1,
};
const directory = { name: 'Example', path: '/work/example', selection };
const session = {
  id: 'owned-copy',
  title: 'Imported copy',
  revision: 1,
  workspace: { path: directory.path },
  archived: false,
};
function edit(c, name, value) {
  const text = {
    owner: uuid(20 + ['name', 'database', 'title', 'search'].indexOf(name)),
    revision: 0,
    bytes: 0,
  };
  c.child('import-' + name, 'changed', {
    revision: 0,
    snapshot: { text, marked: false },
    receipt: null,
  });
  const command = c.state.fields[name].command;
  if (command)
    c.child('import-' + name, 'changed', {
      revision: 1,
      snapshot: {
        text: { ...text, revision: 1, bytes: Buffer.byteLength(command.edit.text) },
        marked: false,
      },
      receipt: { owner: text.owner, sequence: command.sequence, outcome: { status: 'applied' } },
    });
  c.child('import-' + name, 'changed', {
    revision: 2,
    snapshot: { text: { ...text, revision: 2, bytes: Buffer.byteLength(value) }, marked: false },
    receipt: null,
  });
}
function readField(c, value) {
  const captured = c.state.pending.request.observation;
  c.done({ text: value, metadata: captured.snapshot.text });
}
function chooseDirectory(c, purpose) {
  c.raw(purpose === 'source' ? 'source-directory' : 'destination-directory');
  c.child('import-' + purpose + '-picker', 'selected', selection);
  assert.equal(c.out.actions[0].method, purpose === 'source' ? 'importDirectories' : 'directories');
  c.done({ kind: 'children', directory, children: [], after: null });
}
function openPreview(c) {
  c.raw('browse-' + source.id).done({
    kind: 'catalog',
    source: sourceRef,
    entries: [entry],
    next: null,
  });
  c.raw('entry-0', 'submit').done({ kind: 'preview', preview: reviewed });
}
for (const target of ['tui']) {
  test(`${target}: import source form uses original native snapshots and only reads its original unknown receipt`, async () => {
    const c = (await client(target)).loaded([]);
    c.raw('add').raw('format-open_code');
    edit(c, 'name', 'OpenCode research');
    edit(c, 'database', 'opencode.db');
    chooseDirectory(c, 'source');
    c.raw('save-source');
    assert.equal(c.out.actions[0].resources.text.child, 'import-name');
    readField(c, 'OpenCode research');
    assert.equal(c.out.actions[0].resources.text.child, 'import-database');
    readField(c, 'opencode.db');
    const input = c.out.actions[0].input;
    assert.deepEqual(input, {
      kind: 'save_source',
      revision: 1,
      id: null,
      name: 'OpenCode research',
      location: { kind: 'open_code', directory: selection, database: 'opencode.db' },
    });
    assert.deepEqual(c.out.actions[0].recovery, { kind: 'conversation-import/v1', input });
    c.done(
      {
        reference: reference(),
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'unknown', diagnostic: 'lost reply' },
          cleanup: { kind: 'confirmed' },
        },
      },
      'operation',
    );
    c.update({ open: false }).update({ open: true }).raw('save-source').raw('confirm');
    assert.equal(c.out.actions.length, 0);
    assert.equal(c.state.fields.name.command, null);
    c.raw('receipt');
    assert.equal(c.out.actions[0].method, 'read');
    assert.deepEqual(c.out.actions[0].input, reference());
    c.done(reply(reference(), { kind: 'sources', revision: 2, sources: [source] })).done({
      kind: 'sources',
      revision: 2,
      sources: [source],
    });
    assert.equal(c.state.current, null);
    assert.equal(c.state.route, 'sources');
    assert.equal(c.state.sourcesRevision, 2);
    {
      const kinds = new Set([
        'text',
        'button',
        'stack',
        'component',
        'dialog',
        'reading',
        'item',
        'context_menu',
        'scroll',
      ]);
      const keys = nodes(c.out.frame.view)
        .filter((node) => node.key && kinds.has(node.kind))
        .map((node) => node.key);
      assert.equal(new Set(keys).size, keys.length, 'all retained page and modal keys are unique');
    }
  });
  test(`${target}: import preview freezes source digest and chosen target, rejects changes, and opens the imported session`, async () => {
    const c = (await client(target)).loaded();
    openPreview(c);
    edit(c, 'title', 'Imported copy');
    chooseDirectory(c, 'destination');
    c.raw('review-copy');
    readField(c, 'Imported copy');
    assert.deepEqual(c.state.confirm, {
      kind: 'copy',
      title: 'Imported copy',
      selection: reviewed.selection,
      directory: selection,
    });
    c.raw('add').raw('save-source');
    assert.equal(c.out.actions.length, 0, 'background actions cannot escape the confirmation');
    c.raw('confirm');
    assert.equal(c.out.actions[0].method, 'import');
    assert.equal(c.out.actions[0].input.selection.digest, reviewed.selection.digest);
    c.done(reply(reference(), { kind: 'changed' }), 'operation');
    assert.equal(c.state.notice, 'importsChanged');
    c.raw('preview-first');
    assert.equal(
      c.out.actions[0].input.digest,
      null,
      'explicit refresh can accept a new source observation',
    );
    c.done({ kind: 'preview', preview: reviewed });
    c.raw('review-copy');
    readField(c, 'Imported copy');
    c.raw('confirm');
    c.done(reply(reference(2), { kind: 'imported', session }), 'operation');
    assert.deepEqual(c.out.events, [{ name: 'changed', value: {} }]);
    assert.equal(c.out.actions[0].input.kind, 'copies');
    c.done({ kind: 'copies', sessions: [session], next: null }).raw('open-latest');
    assert.deepEqual(c.out.events, [{ name: 'selected', value: { session } }]);
  });
  test(`${target}: remount scans all caller history and settles only the matching original import`, async () => {
    const c = await client(target);
    const input = {
      kind: 'copy',
      title: 'Imported copy',
      selection: reviewed.selection,
      directory: selection,
    };
    const earlier = reference(8),
      original = reference(9);
    c.done([
      { cursor: 1, reference: earlier, recovery: { kind: 'conversation-import/v1', input } },
    ]);
    c.done([
      { cursor: 2, reference: original, recovery: { kind: 'conversation-import/v1', input } },
    ]);
    c.done([{ cursor: 3, reference: { ...reference(10), method: 'send' }, recovery: null }]);
    c.done([]);
    assert.deepEqual(c.out.actions[0].input, original);
    c.done(reply(earlier, { kind: 'imported', session }));
    assert.ok(c.state.current);
    c.raw('confirm');
    assert.equal(c.out.actions.length, 0);
    c.raw('receipt').done(reply(original, { kind: 'imported', session }));
    assert.equal(c.state.current, null);
    assert.equal(c.out.actions[0].method, 'imports');
    assert.deepEqual(c.out.actions[0].input, { kind: 'copies', after: null });
    c.done({ kind: 'copies', sessions: [session], next: null });
    assert.equal(c.state.latest.id, session.id);
  });
}
