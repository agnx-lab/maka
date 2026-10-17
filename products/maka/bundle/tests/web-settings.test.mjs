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
const ajv = new Ajv({ strict: false, formats: { uint64: true } });
addFormats(ajv);
const secretProps = ajv.compile(contracts.secretInput.interface.props);
const methods = Object.fromEntries(
  Object.entries(contracts.workspaceWeb.interface.methods).map(([name, method]) => [
    name,
    ajv.compile(method.input),
  ]),
);
function slots(value) {
  if (!value || typeof value !== 'object') return [];
  return [
    ...(value.binding === 'secret' && value.props ? [value] : []),
    ...Object.values(value).flatMap(slots),
  ];
}
for (const target of ['tui'])
  test(`Web ${target} keeps secrets out of operation data and recovers without repeating writes`, async () => {
    const { component } = await import(`../dist/packages/web-settings/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    let snapshot = {
      revision: null,
      settings: { enabled: false, source: 'model' },
      credential: { revision: null, configured: false, check: null },
    };
    finish([]);
    finish(snapshot);
    assert.ok(secretProps(slots(out.frame)[0].props), JSON.stringify(secretProps.errors));
    raw('enabled');
    assert.ok(methods.save(out.actions[0].input));
    assert.deepEqual(out.actions[0].input, {
      revision: null,
      settings: { enabled: true, source: 'model' },
    });
    const reference = (method) => ({
      contract: { name: 'maka.workspace.Web', version: 1 },
      method,
      operation: 'original-' + method,
    });
    const reply = (method, value) => ({
      reference: reference(method),
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'returned', value },
        cleanup: { kind: 'confirmed' },
      },
    });
    snapshot = { ...snapshot, revision: 1, settings: { enabled: true, source: 'model' } };
    event({
      kind: 'action',
      key: out.actions[0].key,
      event: { status: 'operation', reply: reply('save', { kind: 'updated', snapshot }) },
    });
    finish(snapshot);
    const observation = {
      revision: 2,
      snapshot: {
        marked: false,
        text: {
          owner: '00000000-0000-0000-0000-000000000001',
          revision: 2,
          bytes: 20,
        },
      },
      receipt: null,
    };
    const observe = (value) =>
      event({ kind: 'component', child: 'web-secret', name: 'changed', event: value });
    observe(observation);
    raw('save-key');
    const action = out.actions[0];
    assert.ok(methods.saveKey(action.input));
    assert.deepEqual(action.input, { revision: null });
    assert.deepEqual(action.resources.secret, {
      child: 'web-secret',
      method: 'snapshot',
      input: {
        kind: 'at_revision',
        owner: observation.snapshot.text.owner,
        revision: 2,
      },
    });
    assert.deepEqual(action.recovery, {
      kind: 'web-settings/v1',
      method: 'saveKey',
      input: { revision: null },
    });
    event({
      kind: 'action',
      key: action.key,
      event: {
        status: 'operation',
        reply: {
          reference: reference('saveKey'),
          receipt: {
            kind: 'recorded',
            outcome: { kind: 'unknown', diagnostic: 'lost reply' },
            cleanup: { kind: 'confirmed' },
          },
        },
      },
    });
    raw('save-key');
    assert.equal(out.actions.length, 0);
    // A later native edit cannot be wiped by confirmation of the older capture.
    observe({
      ...observation,
      revision: 3,
      snapshot: {
        ...observation.snapshot,
        text: { ...observation.snapshot.text, revision: 3, bytes: 25 },
      },
    });
    raw('receipt');
    assert.deepEqual(out.actions[0].input, reference('saveKey'));
    snapshot = { ...snapshot, credential: { revision: 1, configured: true, check: null } };
    finish(reply('saveKey', { kind: 'updated', snapshot }));
    assert.equal(out.frame.state.command.basedOn, 2);
    finish(snapshot);
    observe({
      ...observation,
      revision: 3,
      receipt: {
        owner: observation.snapshot.text.owner,
        sequence: out.frame.state.command.sequence,
        outcome: { status: 'rejected', reason: 'revision' },
      },
    });
    raw('remove-key');
    assert.equal(out.actions.length, 0);
    assert.equal(out.frame.state.confirming, true);
    raw('cancel');
    assert.equal(out.actions.length, 0);
    raw('remove-key');
    raw('confirm-remove');
    assert.equal(out.actions[0].method, 'deleteKey');
    assert.deepEqual(out.actions[0].input, { revision: 1 });
    assert.ok(methods.deleteKey(out.actions[0].input));
    event({
      kind: 'action',
      key: out.actions[0].key,
      event: { status: 'operation', reply: reply('deleteKey', { kind: 'changed' }) },
    });
    assert.equal(out.frame.state.notice, 'web-conflict');
    finish(snapshot);
    out = component.mount(props, {});
    finish([{ cursor: 7, reference: reference('saveKey'), recovery: action.recovery }]);
    assert.deepEqual(out.actions[0].input, { after: 7, limit: 16 });
    finish([]);
    assert.equal(out.actions[0].method, 'read');
    assert.deepEqual(out.actions[0].input, reference('saveKey'));
    finish(reply('saveKey', { kind: 'updated', snapshot }));
    assert.equal(
      out.frame.state.command,
      null,
      'restoration cannot clear a new native input owner',
    );
    assert.equal(out.actions[0].method, 'snapshot', 'old receipts cannot become current settings');
    finish({ ...snapshot, revision: 4, settings: { enabled: false, source: 'tavily' } });
    assert.equal(out.frame.state.snapshot.revision, 4);
  });

test('Web recovers an interrupted action through caller history when no reference reached the component', async () => {
  const { component } = await import('../dist/packages/web-settings/tui.js');
  const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
  let out = component.mount(props, {});
  const event = (input) => (out = component.event(out.frame.state, input, props, {}));
  const raw = (key) => event({ kind: 'raw', key, event: {} });
  const finish = (value) =>
    event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
  finish([]);
  finish({
    revision: null,
    settings: { enabled: false, source: 'model' },
    credential: { revision: null, configured: false, check: null },
  });
  raw('enabled');
  const action = out.actions[0];
  event({
    kind: 'action',
    key: action.key,
    event: { status: 'failed', diagnostic: 'interrupted response' },
  });
  raw('enabled');
  assert.equal(out.actions.length, 0);
  raw('history');
  assert.equal(out.actions[0].method, 'page');
  const reference = {
    contract: { name: 'maka.workspace.Web', version: 1 },
    method: 'save',
    operation: 'interrupted-original',
  };
  finish([{ cursor: 12, reference, recovery: action.recovery }]);
  finish([]);
  assert.equal(out.actions[0].method, 'read');
  assert.deepEqual(out.actions[0].input, reference);
});
