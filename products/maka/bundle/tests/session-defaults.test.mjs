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
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const instance = ['00000000-0000-0000-0000-000000000001'];
const source = {
  stateDomain: 'root',
  deployment: '00000000-0000-0000-0000-000000000002',
  revision: 7,
};
const valid = new Ajv({ strict: false, formats: { uint64: true, uuid: true } }).compile(
  contracts.deploymentManager.interface.methods.configure.input,
);
for (const target of ['tui'])
  test(`Session defaults ${target} recover the original deployment CAS and reread current permissions`, async () => {
    const module = await import(`../dist/packages/session-defaults/${target}.js`);
    module.activate({ instance });
    const component = module.component;
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    const settings = (source) => ({
      source,
      instance,
      path: ['permissions'],
      next: null,
      fields: [
        { key: 'access', editable: true, value: { kind: 'text', text: 'workspace_write' } },
        { key: 'requestPermissions', editable: true, value: { kind: 'boolean', value: true } },
      ],
    });
    finish([]);
    finish({ source, instances: [], next: null });
    assert.deepEqual(out.actions[0].input, {
      source,
      instance,
      path: ['permissions'],
      after: null,
    });
    finish(settings(source));
    raw('access-read_only');
    const original = out.actions[0];
    assert.ok(valid(original.input), JSON.stringify(valid.errors));
    assert.deepEqual(original.input, {
      expectedRevision: 7,
      instance,
      changes: [{ path: ['permissions', 'access'], value: { kind: 'text', text: 'read_only' } }],
    });
    event({
      kind: 'action',
      key: original.key,
      event: { status: 'failed', diagnostic: 'response interrupted' },
    });
    raw('access-read_only');
    assert.equal(out.actions.length, 0);
    raw('history');
    const reference = { method: 'configure', operation: 'original' };
    finish([{ cursor: 5, reference, recovery: original.recovery }]);
    finish([]);
    assert.deepEqual(out.actions[0].input, reference);
    finish({
      reference,
      receipt: {
        kind: 'recorded',
        cleanup: { kind: 'unconfirmed' },
        outcome: { kind: 'returned', value: { status: 'committed' } },
      },
    });
    raw('requests');
    assert.equal(out.actions.length, 0, 'uncertain cleanup does not authorize a repeat');
    raw('receipt');
    finish({
      reference,
      receipt: {
        kind: 'recorded',
        cleanup: { kind: 'confirmed' },
        outcome: { kind: 'returned', value: { status: 'committed' } },
      },
    });
    const current = { ...source, revision: 9 };
    finish({ source: current, instances: [], next: null });
    finish(settings(current));
    assert.equal(
      out.frame.state.saved.access,
      'workspace_write',
      'a historical receipt cannot overwrite a later source',
    );
    raw('requests');
    assert.equal(out.actions[0].input.expectedRevision, 9);
  });
