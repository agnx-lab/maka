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
const ajv = new Ajv({
  strict: false,
  formats: { uint64: true, uint32: true, uint16: true, int16: true, uint: true },
});
addFormats(ajv);
const methods = Object.fromEntries(
  Object.entries(contracts.workspaceJev.interface.methods).map(([name, method]) => [
    name,
    ajv.compile(method.input),
  ]),
);
const inputProps = ajv.compile(contracts.textInput.interface.props);
const secretProps = ajv.compile(contracts.secretInput.interface.props);
function slots(value) {
  if (!value || typeof value !== 'object') return [];
  return [
    ...(value.props && ['input', 'secret'].includes(value.binding) ? [value] : []),
    ...Object.values(value).flatMap(slots),
  ];
}
for (const target of ['tui'])
  test(`Jev ${target} preserves the settings CAS basis during private credential changes and original-receipt recovery`, async () => {
    const { component } = await import(`../dist/packages/jev-settings/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    const observe = (name, observation) =>
      event({ kind: 'component', child: 'jev-' + name, name: 'changed', event: observation });
    let snapshot = {
      revision: 1,
      settings: {
        enabled: true,
        url: 'https://first.example/decision',
        model: 'saved',
        timeoutMs: 8000,
      },
      credentialRevision: null,
      configured: false,
      apiKeyConfigured: false,
      headerNames: [],
    };
    finish([]);
    finish(snapshot);
    const values = {
      url: 'https://draft.example/decision',
      model: 'unsaved-model',
      timeoutMs: '4000',
      headerName: 'X-Tenant',
    };
    let index = 0;
    for (const name of Object.keys(values)) {
      const owner = '00000000-0000-0000-0000-' + String(++index).padStart(12, '0');
      observe(name, {
        revision: 0,
        snapshot: { marked: false, text: { owner, revision: 0, bytes: 0 } },
        receipt: null,
      });
      const command = out.frame.state.fields[name].command;
      observe(name, {
        revision: 1,
        snapshot: { marked: false, text: { owner, revision: 1, bytes: values[name].length } },
        receipt: { owner, sequence: command.sequence, outcome: { status: 'applied' } },
      });
    }
    const secret = {
      revision: 3,
      snapshot: {
        marked: false,
        text: { owner: '00000000-0000-0000-0000-000000000010', revision: 3, bytes: 21 },
      },
      receipt: null,
    };
    observe('secret', secret);
    for (const slot of slots(out.frame))
      assert.ok((slot.binding === 'secret' ? secretProps : inputProps)(slot.props));
    raw('save-header');
    assert.equal(out.actions[0].binding, 'reader');
    finish({
      metadata: out.frame.state.fields.headerName.observation.snapshot.text,
      text: values.headerName,
    });
    const action = out.actions[0];
    assert.equal(action.method, 'credential');
    assert.ok(methods.credential(action.input));
    assert.deepEqual(action.input, {
      url: snapshot.settings.url,
      revision: null,
      target: { kind: 'header', name: 'X-Tenant' },
      change: 'replace',
    });
    assert.deepEqual(action.resources.secret.input, {
      kind: 'at_revision',
      owner: secret.snapshot.text.owner,
      revision: 3,
    });
    const reference = {
      contract: { name: 'maka.workspace.Jev', version: 1 },
      method: 'credential',
      operation: 'original-credential',
    };
    const reply = (cleanup = 'confirmed') => ({
      reference,
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'returned', value: { kind: 'updated', snapshot } },
        cleanup: { kind: cleanup },
      },
    });
    event({ kind: 'action', key: action.key, event: { status: 'operation', reply: reply() } });
    // Another caller has edited settings while the credential operation completed.
    snapshot = {
      ...snapshot,
      revision: 5,
      settings: { ...snapshot.settings, model: 'other-caller' },
      credentialRevision: 1,
      configured: true,
      headerNames: ['X-Tenant'],
    };
    finish(snapshot);
    assert.equal(
      out.frame.state.fields.model.command,
      null,
      'credential result cannot reset unsaved fields',
    );
    raw('save');
    for (const name of ['url', 'model', 'timeoutMs'])
      finish({
        metadata: out.frame.state.fields[name].observation.snapshot.text,
        text: values[name],
      });
    assert.ok(methods.save(out.actions[0].input));
    assert.equal(
      out.actions[0].input.revision,
      1,
      'unrelated refresh must not authorize overwriting another caller',
    );
    assert.equal(out.actions[0].input.settings.model, 'unsaved-model');
    // Observer loss blocks new writes, and restoration uses the original operation.
    event({
      kind: 'action',
      key: out.actions[0].key,
      event: { status: 'failed', message: 'reply lost' },
    });
    raw('save-key');
    assert.equal(out.actions.length, 0);
    out = component.mount(props, {});
    finish([{ cursor: 9, reference, recovery: action.recovery }]);
    finish([]);
    assert.deepEqual(out.actions[0].input, reference);
    finish(reply('unconfirmed'));
    raw('save-key');
    assert.equal(out.actions.length, 0);
    raw('receipt');
    finish(reply());
    assert.equal(out.actions[0].method, 'snapshot');
    finish(snapshot);
    assert.equal(out.frame.state.draftRevision, 5);
    assert.equal(out.frame.state.saved.settings.model, 'other-caller');
    assert.equal(out.frame.state.current, null);
  });
