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
const entry = {
  reference: 'source:local:review',
  source: 'Local',
  name: 'Review',
  description: 'Review a change',
  valid: true,
  installed: false,
  bundled: false,
  preference: { enabled: true, pinned: false },
  shadowedBy: null,
  metadataTruncated: false,
  issues: [],
};
const digest = 'sha256:' + 'a'.repeat(64);
const reference = {
  source: {
    deployment: '00000000-0000-0000-0000-000000000001',
    revision: 4,
    stateDomain: 'a'.repeat(64),
  },
  namespace: ['00000000-0000-0000-0000-000000000002'],
  export: 'skills',
  method: 'apply',
  operation: '00000000-0000-0000-0000-000000000003',
  fingerprint: 'sha256-' + 'a'.repeat(64),
  contract: contracts.skillLibrary,
  recovery: 'journal_v1',
};
async function client(target, writable = true, preferencesWritable = false) {
  const { component } = await import(`../dist/packages/skill-library/${target}.js`);
  let props = { theme, locale: 'en', open: true };
  let out = component.mount(props, {});
  const c = {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    event(input) {
      out = component.event(c.state, input, props, {});
      return c;
    },
    raw(key) {
      return c.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    update(next) {
      props = { ...props, ...next };
      out = component.update(c.state, props, {});
      return c;
    },
    reply(action, value) {
      return c.event({ kind: 'action', key: action.key, event: { status: 'succeeded', value } });
    },
    action(method) {
      return out.actions.find((action) =>
        method === 'outcome' ? action.binding === 'outcomes' : action.method === method,
      );
    },
    open(selected = entry) {
      c.reply(c.action('list'), {
        kind: 'page',
        writable,
        preferencesWritable,
        preferenceRevision: 7,
        revision: digest,
        entries: [selected],
        after: null,
        diagnostics: [],
      });
      c.raw(`${c.state.pageKey}-row-0`);
      c.reply(c.action('read'), {
        kind: 'page',
        writable,
        preferencesWritable,
        preferenceRevision: 7,
        revision: digest,
        entry: selected,
        text: 'Instructions',
        offset: 0,
        before: null,
        after: null,
        totalBytes: 12,
      });
      return c;
    },
    preview() {
      c.raw('install-skill');
      const action = c.action('preview');
      assert.ok(action);
      c.reply(action, {
        kind: 'ready',
        review: { change: action.input, digest },
        target: 'installed:review',
        entries: 5,
        paths: ['SKILL.md', 'scripts', 'scripts/check'],
        pathsTruncated: false,
        locallyModified: false,
      });
      return c;
    },
    operation(action, receipt, ref = reference) {
      return c.event({
        kind: 'action',
        key: action.key,
        event: { status: 'operation', reply: { reference: ref, receipt } },
      });
    },
  };
  return c;
}
for (const target of ['tui']) {
  test(`${target} retained import and browser buttons have distinct native keys`, async () => {
    const c = await client(target);
    const unique = () => {
      const keys = new Set();
      const visit = (value) => {
        if (!value || typeof value !== 'object') return;
        if (typeof value.key === 'string' && value.kind === 'button') {
          assert.ok(!keys.has(value.key), `duplicate native key ${value.key}`);
          keys.add(value.key);
        }
        Object.values(value).forEach(visit);
      };
      visit(c.out.frame.view);
    };
    unique();
    c.open().raw('open-import');
    unique();
    c.raw('close-import');
    assert.equal(c.state.open, false);
  });
  test(`${target} imports freeze native file and ID snapshots and retain unknown operations across close`, async () => {
    const c = await client(target);
    c.open().raw('open-import');
    const id = {
      revision: 1,
      snapshot: {
        text: { owner: 'source-id-owner', revision: 1, bytes: 6 },
        selection: { start: 6, end: 6 },
        reversed: false,
        marked: null,
      },
      receipt: null,
    };
    const file = {
      owner: 'source-file-owner',
      revision: 2,
      phase: 'ready',
      file: { name: 'SKILL.md', bytes: 80, digest },
      error: null,
    };
    c.event({ kind: 'component', child: 'skills-import-id', name: 'changed', event: id });
    c.event({ kind: 'component', child: 'skills-import-file', name: 'changed', event: file });
    c.raw('review-import');
    assert.ok(c.state.importer.confirmation);
    // Changed native material invalidates the review; it cannot silently retarget.
    c.event({
      kind: 'component',
      child: 'skills-import-file',
      name: 'changed',
      event: { ...file, revision: 3 },
    });
    c.raw('confirm-import');
    assert.equal(c.action('importSource'), undefined);
    c.raw('review-import').raw('confirm-import');
    const action = c.action('importSource');
    assert.deepEqual(action.input, { bytes: 80, digest });
    assert.deepEqual(action.resources, {
      file: {
        child: 'skills-import-file',
        method: 'snapshot',
        input: { owner: file.owner, revision: 3 },
      },
      id: {
        child: 'skills-import-id',
        method: 'snapshot',
        input: { kind: 'at_revision', owner: id.snapshot.text.owner, revision: 1 },
      },
    });
    const original = { ...reference, method: 'importSource' };
    c.operation(
      action,
      { kind: 'recorded', outcome: { kind: 'unresolved' }, cleanup: { kind: 'unconfirmed' } },
      original,
    );
    c.raw('close');
    c.update({ open: false });
    c.update({ open: true });
    const receipt = c.action('read');
    assert.deepEqual(receipt.input, original);
    c.reply(receipt, {
      reference: original,
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'unresolved' },
        cleanup: { kind: 'unconfirmed' },
      },
    });
    assert.equal(c.action('outcome').binding, 'outcomes');
    c.reply(c.action('outcome'), {
      original: {
        reference: original,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'unresolved' },
          cleanup: { kind: 'unconfirmed' },
        },
      },
      published: { kind: 'applied', reference: 'imported:review' },
    });
    assert.equal(c.state.current, null);
    assert.equal(c.state.importer.open, false);
    assert.equal(c.state.cleanupUnconfirmed, true);
    assert.ok(c.action('list'));
  });
  test(`${target} imported sources cannot enable themselves and deletion uses its original store method`, async () => {
    const c = await client(target, true, true);
    const source = {
      ...entry,
      imported: true,
      reference: 'imported:review',
      preference: { enabled: false, pinned: false },
    };
    c.open(source).raw('toggle-enabled');
    assert.equal(c.action('setPreference'), undefined);
    c.raw('delete-skill');
    const preview = c.action('preview');
    assert.equal(preview.input.reference, source.reference);
    c.reply(preview, {
      kind: 'ready',
      review: { change: preview.input, digest },
      target: source.reference,
      entries: 1,
      paths: ['SKILL.md'],
      pathsTruncated: false,
      locallyModified: false,
    });
    c.raw('confirm-change');
    assert.ok(c.action('deleteSource'));
    assert.equal(c.action('apply'), undefined);
    assert.equal(c.state.current.method, 'deleteSource');
  });
  test(`${target} long project references fit native document keys and refreshed pages replace them`, async () => {
    const c = await client(target);
    const project = { ...entry, reference: `project:${'a'.repeat(64)}:agents:${'b'.repeat(255)}` };
    const bodyKey = () => {
      const visit = (value) => {
        if (!value || typeof value !== 'object') return null;
        if (typeof value.key === 'string' && value.key.startsWith('skill-body-')) return value.key;
        for (const child of Object.values(value)) {
          const found = visit(child);
          if (found) return found;
        }
        return null;
      };
      const key = visit(c.out.frame.view);
      assert.ok(key);
      assert.ok(Buffer.byteLength(key) <= 128);
      return key;
    };
    c.open(project);
    const original = bodyKey();
    c.raw('refresh');
    c.open({ ...project, reference: project.reference.replace('agents:', 'maka:') });
    assert.notEqual(bodyKey(), original);
  });
  test(`${target} changing workspace drops old reads and previews without mixing projects`, async () => {
    const a = {
      path: '/project-a',
      directoryIdentity: digest,
      identity: 'workspace:v1:00000000-0000-4000-8000-000000000001',
    };
    const b = {
      ...a,
      path: '/project-b',
      identity: 'workspace:v1:00000000-0000-4000-8000-000000000002',
    };
    const c = await client(target);
    c.update({ workspace: a });
    const old = c.action('list');
    assert.deepEqual(old.input.workspace, a);
    c.update({ workspace: b });
    const current = c.action('list');
    assert.deepEqual(current.input.workspace, b);
    assert.notEqual(current.key, old.key);
    c.reply(old, { kind: 'page', entries: [entry] });
    assert.equal(c.state.page, null);
    c.reply(current, {
      kind: 'page',
      writable: true,
      preferencesWritable: false,
      preferenceRevision: 0,
      revision: digest,
      entries: [entry],
      after: null,
      diagnostics: [],
    });
    c.raw(`${c.state.pageKey}-row-0`);
    assert.deepEqual(c.action('read').input.workspace, b);
    c.update({ workspace: a });
    assert.equal(c.state.document, null);
    assert.deepEqual(c.action('list').input.workspace, a);
    c.open().preview();
    assert.deepEqual(c.state.preview.review.change.workspace, a);
    c.update({ workspace: b });
    assert.equal(c.state.preview, null);
    c.raw('confirm-change');
    assert.equal(c.action('apply'), undefined);
  });
  test(`${target} changing workspace keeps an unknown mutation on its original receipt`, async () => {
    const a = {
      path: '/project-a',
      directoryIdentity: digest,
      identity: 'workspace:v1:00000000-0000-4000-8000-000000000001',
    };
    const b = {
      ...a,
      path: '/project-b',
      identity: 'workspace:v1:00000000-0000-4000-8000-000000000002',
    };
    const c = await client(target);
    c.update({ workspace: a }).open().preview().raw('confirm-change');
    const action = c.action('apply');
    assert.deepEqual(action.input.change.workspace, a);
    c.operation(action, { kind: 'unavailable', diagnostic: 'Reply lost' });
    c.update({ workspace: b });
    assert.deepEqual(c.state.workspace, a);
    c.raw('check-change');
    assert.deepEqual(c.action('read').input, reference);
    c.reply(c.action('read'), {
      reference,
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'returned', value: { kind: 'applied', reference: 'installed:review' } },
        cleanup: { kind: 'confirmed' },
      },
    });
    assert.deepEqual(c.state.workspace, b);
    assert.deepEqual(c.action('list').input.workspace, b);
  });
  test(`${target} rejected original changes refresh the newly selected workspace after settlement`, async () => {
    const workspace = {
      path: '/next',
      directoryIdentity: digest,
      identity: 'workspace:v1:00000000-0000-4000-8000-000000000002',
    };
    for (const method of ['apply', 'setPreference']) {
      for (const accepted of [true, false]) {
        const c = (await client(target, true, true)).open();
        if (method === 'apply') c.preview().raw('confirm-change');
        else c.raw('toggle-enabled');
        const action = c.action(method);
        c.update({ workspace });
        assert.equal(c.state.workspace, null);
        if (accepted)
          c.operation(
            action,
            {
              kind: 'recorded',
              outcome: { kind: 'returned', value: { kind: 'changed' } },
              cleanup: { kind: 'confirmed' },
            },
            { ...reference, method },
          );
        else
          c.event({
            kind: 'action',
            key: action.key,
            event: { status: 'not_accepted', reason: 'denied' },
          });
        assert.equal(c.state.current, null);
        assert.deepEqual(c.action('list').input.workspace, workspace);
        assert.equal(c.state.notice, accepted ? 'skillsChanged' : 'skillsFailed');
      }
    }
  });
  test(`${target} built-in instructions show their origin and permit only preference changes`, async () => {
    const c = await client(target, true, true);
    const builtin = { ...entry, bundled: true, reference: 'bundled:review', source: 'Built-in' };
    c.reply(c.action('list'), {
      kind: 'page',
      writable: true,
      preferencesWritable: true,
      preferenceRevision: 7,
      revision: digest,
      entries: [builtin],
      after: null,
      diagnostics: [],
    });
    assert.match(JSON.stringify(c.out), /Built-in/);
    c.raw(`${c.state.pageKey}-row-0`);
    c.reply(c.action('read'), {
      kind: 'page',
      writable: false,
      preferencesWritable: true,
      preferenceRevision: 7,
      revision: digest,
      entry: builtin,
      text: 'Built-in instructions',
      offset: 0,
      before: null,
      after: null,
      totalBytes: 21,
    });
    for (const action of ['install-skill', 'update-skill', 'delete-skill']) {
      c.raw(action);
      assert.equal(c.out.actions.length, 0);
    }
    c.raw('toggle-enabled');
    assert.equal(c.action('setPreference').input.reference, 'bundled:review');
  });
  test(`${target} skill changes require writable access and current explicit confirmation`, async () => {
    const readonly = (await client(target, false)).open();
    readonly.raw('install-skill');
    assert.equal(readonly.out.actions.length, 0);
    const c = (await client(target)).open().preview();
    c.raw('cancel-change').raw('confirm-change');
    assert.equal(c.out.actions.length, 0);
    c.preview().raw('close').raw('confirm-change');
    assert.equal(c.out.actions.length, 0);
    c.preview().raw('confirm-change');
    assert.equal(c.action('apply').input.change.reference, entry.reference);
    c.raw('confirm-change');
    assert.equal(
      c.out.actions.length,
      0,
      'a second confirmation cannot dispatch another operation',
    );
  });
  test(`${target} closing skills retains the accepted operation and recovers only its original receipt`, async () => {
    const c = (await client(target)).open().preview().raw('confirm-change');
    const mutation = c.action('apply');
    c.raw('close').update({ open: false });
    c.operation(mutation, { kind: 'unavailable', diagnostic: 'reply lost' });
    assert.deepEqual(c.state.current.reference, reference);
    assert.equal(c.out.actions.length, 0);
    c.update({ open: true });
    const recovery = c.action('read');
    assert.equal(recovery.binding, 'receipts');
    assert.deepEqual(recovery.input, reference);
    c.reply(recovery, {
      reference: { ...reference, namespace: ['00000000-0000-0000-0000-000000000004'] },
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'returned', value: { kind: 'applied', reference: 'installed:review' } },
        cleanup: { kind: 'confirmed' },
      },
    });
    assert.deepEqual(c.state.current.reference, reference);
    c.raw('install-skill').raw('confirm-change');
    assert.equal(c.out.actions.length, 0);
    c.raw('check-change');
    const original = c.action('read');
    assert.deepEqual(original.input, reference);
    c.reply(original, {
      reference,
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'returned', value: { kind: 'applied', reference: 'installed:review' } },
        cleanup: { kind: 'confirmed' },
      },
    });
    assert.equal(c.state.current, null);
    assert.ok(c.action('list'));
    assert.ok(!c.action('apply'), 'receipt observation must not replay installation');
  });
  test(`${target} canonical skill outcomes preserve original cleanup and never replay a lost mutation`, async () => {
    const c = (await client(target)).open().preview().raw('confirm-change');
    c.operation(c.action('apply'), { kind: 'unavailable', diagnostic: 'Host disconnected' });
    const receipt = {
      kind: 'recorded',
      outcome: { kind: 'unresolved' },
      cleanup: { kind: 'unconfirmed' },
    };
    c.raw('check-change');
    c.reply(c.action('read'), { reference, receipt });
    let query = c.action('outcome');
    assert.equal(query.binding, 'outcomes');
    assert.deepEqual(query.input, reference);
    c.reply(query, { original: { reference, receipt }, published: null });
    assert.deepEqual(c.state.current.reference, reference);
    assert.equal(c.state.notice, 'skillsUnknown');
    assert.equal(c.out.actions.length, 0, 'missing publication evidence cannot authorize a retry');
    c.raw('check-change');
    c.reply(c.action('read'), { reference, receipt });
    query = c.action('outcome');
    c.reply(query, {
      original: {
        reference: { ...reference, operation: '00000000-0000-0000-0000-000000000009' },
        receipt,
      },
      published: { kind: 'applied', reference: 'installed:review' },
    });
    assert.deepEqual(
      c.state.current.reference,
      reference,
      'a replacement cannot settle the original operation',
    );
    c.raw('check-change');
    c.reply(c.action('read'), { reference, receipt });
    query = c.action('outcome');
    c.raw('close').update({ open: false });
    c.reply(query, {
      original: { reference, receipt },
      published: { kind: 'applied', reference: 'installed:review' },
    });
    assert.equal(c.state.current, null);
    assert.equal(c.state.cleanupUnconfirmed, true);
    assert.equal(
      c.out.actions.length,
      0,
      'closed library consumes the result without replay or background refresh',
    );
  });
  test(`${target} unavailable original receipts and denied domain reads retain the original skill operation`, async () => {
    const c = (await client(target)).open().preview().raw('confirm-change');
    c.operation(c.action('apply'), { kind: 'unavailable', diagnostic: 'Host disconnected' });
    for (const receipt of [
      { kind: 'not_recorded' },
      { kind: 'unavailable', diagnostic: 'unreachable' },
    ]) {
      c.raw('check-change');
      c.reply(c.action('read'), { reference, receipt });
      assert.deepEqual(c.state.current.reference, reference);
      assert.equal(c.out.actions.length, 0);
    }
    c.raw('check-change');
    c.reply(c.action('read'), {
      reference,
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'unknown', diagnostic: 'reply lost' },
        cleanup: { kind: 'confirmed' },
      },
    });
    const query = c.action('outcome');
    c.event({
      kind: 'action',
      key: query.key,
      event: { status: 'failed', error: 'recovery denied' },
    });
    assert.deepEqual(c.state.current.reference, reference);
    assert.equal(c.state.notice, 'skillsUnknown');
    assert.equal(c.out.actions.length, 0);
  });
  test(`${target} rejected skill results retain cleanup uncertainty separately from the rejection`, async () => {
    const c = (await client(target)).open().preview().raw('confirm-change');
    c.operation(c.action('apply'), {
      kind: 'recorded',
      outcome: { kind: 'returned', value: { kind: 'changed' } },
      cleanup: { kind: 'failed', diagnostic: 'resource settlement failed' },
    });
    assert.equal(c.state.current, null);
    assert.equal(c.state.notice, 'skillsChanged');
    assert.equal(c.state.cleanupUnconfirmed, true);
    assert.ok(
      JSON.stringify(c.out).includes(
        'The operation result is known, but resource cleanup is not confirmed.',
      ),
    );
  });
}

for (const target of ['tui']) {
  test(`${target} preference writes have separate authority and retain original unknown operations`, async () => {
    const readonly = (await client(target, true, false)).open();
    readonly.raw('toggle-enabled').raw('toggle-pinned');
    assert.equal(readonly.out.actions.length, 0);
    const c = (await client(target, false, true)).open().raw('toggle-enabled');
    const mutation = c.action('setPreference');
    assert.deepEqual(mutation.input, {
      reference: entry.reference,
      revision: 7,
      preference: { enabled: false, pinned: false },
    });
    const original = { ...reference, method: 'setPreference' };
    c.raw('close').update({ open: false });
    c.operation(mutation, { kind: 'unavailable', diagnostic: 'reply lost' }, original);
    c.update({ open: true });
    const receipt = {
      kind: 'recorded',
      outcome: { kind: 'unresolved' },
      cleanup: { kind: 'unconfirmed' },
    };
    c.reply(c.action('read'), { reference: original, receipt });
    assert.equal(c.state.notice, 'skillsUnknown');
    assert.equal(c.out.actions.length, 0, 'preference writes cannot use file publication outcomes');
    c.raw('toggle-pinned');
    assert.equal(c.out.actions.length, 0);
    c.raw('check-change');
    c.reply(c.action('read'), {
      reference: original,
      receipt: {
        ...receipt,
        outcome: { kind: 'returned', value: { kind: 'applied', revision: 8 } },
      },
    });
    assert.equal(c.state.current, null);
    assert.equal(c.state.cleanupUnconfirmed, true);
    assert.ok(c.action('list'));
    assert.ok(!c.action('setPreference'));
  });
  test(`${target} stale preference revision requires refresh before another write`, async () => {
    const c = (await client(target, false, true)).open().raw('toggle-pinned');
    const mutation = c.action('setPreference');
    assert.equal(mutation.input.preference.pinned, true);
    c.operation(
      mutation,
      {
        kind: 'recorded',
        outcome: { kind: 'returned', value: { kind: 'changed' } },
        cleanup: { kind: 'confirmed' },
      },
      { ...reference, method: 'setPreference' },
    );
    assert.equal(c.state.notice, 'skillsChanged');
    c.raw('toggle-enabled');
    assert.equal(c.out.actions.length, 0);
  });
}
