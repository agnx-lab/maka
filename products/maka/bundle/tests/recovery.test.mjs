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
const appearance = await import('../dist/packages/appearance/tui.js');
const values = { appearance: [appearance.activate({ scheme: 'light' }).Appearance.value] };
const id = (n) => `00000000-0000-0000-0000-${String(n).padStart(12, '0')}`;
const reference = (n) => ({
  source: { stateDomain: 'a'.repeat(64), deployment: id(1), revision: 1 },
  namespace: [id(2)],
  export: 'terminal',
  method: 'start',
  operation: id(n),
  fingerprint: 'sha256-' + 'b'.repeat(64),
  contract: contracts.terminal,
  recovery: 'journal_v1',
});
const intent = (n) => ({
  cursor: n,
  caller: { stateDomain: 'c'.repeat(64), deployment: id(3), revision: 1 },
  consumer: [id(4)],
  reference: reference(n),
});
const recorded = (ref, value = { kind: 'completed', value: { ref: 'original-pty' } }) => ({
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
  const module = await import(`../dist/packages/recovery/${target}.js`);
  module.activate({ locale: 'en' });
  const component = module.component;
  let out = component.mount({}, values);
  const c = {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    event(event) {
      out = component.event(c.state, event, {}, values);
      return c;
    },
    raw(key) {
      return c.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    reply(value, key = c.state.pending.key) {
      return c.event({ kind: 'action', key, event: { status: 'succeeded', value } });
    },
    source() {
      return nodes(out.frame.view).find((n) => n.binding === 'source')?.props.source;
    },
  };
  return c;
}
for (const target of ['tui']) {
  test(`${target} recovery uses original references and never dispatches another effect`, async () => {
    const c = await client(target);
    assert.deepEqual(
      c.out.actions.map((a) => [a.binding, a.method, a.input]),
      [['history', 'page', { after: 0, limit: 4 }]],
    );
    const entry = intent(1);
    c.reply([entry]).event({
      kind: 'component',
      child: 'recovery-reading',
      name: 'input',
      key: 'reading-input',
      event: { kind: 'raw', key: 'check-0', event: { kind: 'activate' } },
    });
    const request = c.out.actions[0];
    assert.equal(request.binding, 'receipts');
    assert.equal(request.method, 'read');
    assert.deepEqual(request.input, entry.reference);
    c.raw('deployment');
    assert.deepEqual(c.out.actions, []);
    c.reply(recorded(entry.reference), request.key).raw('operations');
    assert.deepEqual(c.state.reply, recorded(entry.reference));
    assert.match(c.source(), /original-pty/);
    assert.deepEqual(c.out.actions, []);
    c.raw('check-again');
    assert.equal(c.out.actions[0].method, 'read');
    assert.deepEqual(c.out.actions[0].input, entry.reference);
    c.event({
      kind: 'action',
      key: c.state.pending.key,
      event: { status: 'failed', diagnostic: 'private' },
    });
    c.event({
      kind: 'action',
      key: c.state.pending.key,
      event: { status: 'failed', diagnostic: 'private' },
    });
    assert.deepEqual(c.state.reply, recorded(entry.reference));
    assert.match(JSON.stringify(c.out.frame.view), /Could not refresh/);
    assert.ok(!JSON.stringify(c.out.frame.view).includes('private'));
  });
  test(`${target} recovery refuses retargeted replies and checks explicit manager reader without replay`, async () => {
    const c = await client(target);
    const entry = intent(1);
    c.reply([entry]).event({
      kind: 'component',
      child: 'recovery-reading',
      name: 'input',
      key: 'reading-input',
      event: { kind: 'raw', key: 'check-0', event: { kind: 'activate' } },
    });
    c.reply(recorded(reference(99)));
    assert.equal(c.state.reply, null);
    assert.equal(c.out.actions[0].binding, 'managementReceipts');
    assert.deepEqual(c.out.actions[0].input, entry.reference);
    c.reply({
      reference: entry.reference,
      receipt: { kind: 'unavailable', diagnostic: 'private-details' },
    });
    assert.equal(c.state.reply, null);
    assert.equal(c.state.pending, null);
    assert.deepEqual(c.out.actions, []);
    assert.ok(!JSON.stringify(c.out.frame.view).includes('private-details'));
    c.raw('check-again');
    assert.deepEqual(c.out.actions[0].input, entry.reference);
    c.reply(recorded(entry.reference));
    assert.deepEqual(c.state.reply, recorded(entry.reference));
  });
  test(`${target} recovery retains bounded history cursor and ignores late observations`, async () => {
    const c = await client(target);
    const first = c.state.pending.key;
    c.reply([intent(1), intent(2), intent(3), intent(4)]);
    c.raw('next');
    assert.deepEqual(c.out.actions[0].input, { after: 4, limit: 4 });
    const pending = c.state.pending.key;
    c.reply([intent(99)], first);
    assert.equal(c.state.pending.key, pending);
    assert.equal(c.state.entries[0].cursor, 1);
    c.reply([intent(5)], pending);
    assert.deepEqual(
      c.state.entries.map((entry) => entry.cursor),
      [1, 2, 3, 4, 5],
    );
    assert.equal(c.state.next, 5, 'byte-bounded short pages may have later entries');
    c.raw('next');
    assert.deepEqual(c.out.actions[0].input, { after: 5, limit: 4 });
    c.reply([]);
    assert.equal(c.state.next, null);
  });
  test(`${target} recovery exposes all recorded source through bounded Unicode excerpts`, async () => {
    const c = await client(target),
      entry = intent(1);
    c.reply([entry]).event({
      kind: 'component',
      child: 'recovery-reading',
      name: 'input',
      key: 'reading-input',
      event: { kind: 'raw', key: 'check-0', event: { kind: 'activate' } },
    });
    const reply = recorded(entry.reference, { result: '界🙂'.repeat(20000) });
    c.reply(reply);
    let actual = '';
    for (let n = 0; n < 32; n++) {
      const source = c.source();
      assert.ok(Buffer.byteLength(source) <= 65536);
      assert.equal(source.isWellFormed(), true);
      actual += source;
      assert.equal(
        nodes(c.out.frame.view).some(
          (node) => node.key === 'next-source' || node.key === 'previous-source',
        ),
        false,
      );
      if (actual.length === JSON.stringify(reply.receipt, null, 2).length) break;
      c.event({
        kind: 'component',
        child: 'recovery-reading',
        name: 'observed',
        event: {
          readingRevision: n + 1,
          scroll: { readingRevision: n + 1, direction: 'down' },
          viewport: { readingRevision: n + 1, atStart: false, atEnd: true, visibleChildKeys: [] },
          pendingCommand: null,
        },
      });
    }
    assert.equal(actual, JSON.stringify(reply.receipt, null, 2));
    assert.deepEqual(c.out.actions, []);
  });
  test(`${target} a fresh recovery root reads persisted skill publication evidence independently of the library`, async () => {
    for (const method of ['apply', 'importSource', 'deleteSource']) {
      const entry = intent(1);
      entry.reference = {
        ...entry.reference,
        export: 'skills',
        method,
        contract: contracts.skillLibrary,
      };
      const c = await client(target);
      c.reply([entry]).raw('check-0');
      const original = {
        reference: entry.reference,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'unresolved' },
          cleanup: { kind: 'unconfirmed' },
        },
      };
      c.reply(original);
      assert.equal(c.out.actions[0].binding, 'skillOutcomes');
      assert.equal(c.out.actions[0].method, 'read');
      assert.deepEqual(c.out.actions[0].input, entry.reference);
      const published = {
        kind: 'applied',
        reference: method === 'apply' ? 'installed:review' : 'imported:review',
      };
      c.reply({ original, published });
      assert.deepEqual(c.state.reply, original);
      assert.deepEqual(c.state.published, published);
      assert.deepEqual(JSON.parse(c.source()), { host: original.receipt, publication: published });
      assert.match(JSON.stringify(c.out.frame.view), /Original skill result confirmed/);
      assert.equal(c.out.actions.length, 0);
      c.raw('check-again').reply(original);
      c.reply({ original, published: null });
      assert.deepEqual(
        c.state.published,
        published,
        'a later missing observation cannot discard an immutable known result',
      );
    }
  });
  test(`${target} recovery does not replace original Host facts with a foreign or denied skill result`, async () => {
    const entry = intent(1);
    entry.reference = {
      ...entry.reference,
      export: 'skills',
      method: 'apply',
      contract: contracts.skillLibrary,
    };
    const original = recorded(entry.reference, { kind: 'applied', reference: 'installed:review' });
    const c = await client(target);
    c.reply([entry]).raw('check-0').reply(original);
    assert.equal(c.out.actions[0].binding, 'skillOutcomes');
    c.reply({
      original: recorded(reference(99)),
      published: { kind: 'applied', reference: 'installed:other' },
    });
    assert.deepEqual(c.state.reply, original);
    assert.equal(c.state.published, null);
    assert.equal(c.state.failed, true);
    assert.equal(c.out.actions.length, 0);
    c.raw('check-again').reply(original);
    c.event({
      kind: 'action',
      key: c.state.pending.key,
      event: { status: 'failed', diagnostic: 'recovery denied' },
    });
    assert.deepEqual(c.state.reply, original);
    assert.equal(c.state.published, null);
    assert.equal(c.out.actions.length, 0);
  });
}
