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
const save = ajv.compile(contracts.workhubCoordinator.interface.methods.savePresets.input);
const inputProps = ajv.compile(contracts.textInput.interface.props);
function slots(value) {
  if (!value || typeof value !== 'object') return [];
  return [
    ...(value.binding === 'input' && value.props ? [value] : []),
    ...Object.values(value).flatMap(slots),
  ];
}
for (const target of ['tui'])
  test(`Subagents ${target} freezes native drafts and recovers only the original save`, async () => {
    const { component } = await import(`../dist/packages/subagent-settings/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    const selected = {
      choice: {
        selection: {
          connectionId: '00000000-0000-0000-0000-000000000001',
          modelId: 'chosen',
          revision: 2,
        },
        connectionName: 'Fixture',
        label: 'Chosen',
        thinkingLevels: ['medium'],
        defaultThinkingLevel: null,
      },
      thinkingLevel: 'medium',
    };
    finish([]);
    finish({ revision: null, entries: [] });
    finish({ names: ['Read', 'Write', 'WebSearch'] });
    raw('new');
    for (const [index, name] of ['name', 'description'].entries()) {
      const owner = '00000000-0000-0000-0000-' + String(index + 1).padStart(12, '0');
      event({
        kind: 'component',
        child: 'subagent-' + name,
        name: 'changed',
        event: {
          revision: 0,
          snapshot: { marked: false, text: { owner, revision: 0, bytes: 0 } },
          receipt: null,
        },
      });
      const command = out.frame.state.fields[name].command;
      event({
        kind: 'component',
        child: 'subagent-' + name,
        name: 'changed',
        event: {
          revision: 1,
          snapshot: { marked: false, text: { owner, revision: 1, bytes: 5 } },
          receipt: { owner, sequence: command.sequence, outcome: { status: 'applied' } },
        },
      });
    }
    for (const slot of slots(out.frame)) assert.ok(inputProps(slot.props));
    event({ kind: 'component', child: 'subagent-models', name: 'selected', event: selected });
    raw('profile-read');
    raw('isolated');
    out = component.update(out.frame.state, { ...props, open: false }, {});
    out = component.update(out.frame.state, props, {});
    assert.equal(out.frame.state.fields.name.command, null);
    raw('save');
    for (const name of ['name', 'description'])
      finish({
        metadata: out.frame.state.fields[name].observation.snapshot.text,
        text: name === 'name' ? 'Reader' : 'Audit',
      });
    const action = out.actions[0];
    assert.equal(action.method, 'savePresets');
    assert.ok(save(action.input));
    assert.deepEqual(action.input.entries[0].tools, ['Read']);
    assert.equal(action.input.entries[0].isolatedWorkspace, true);
    assert.deepEqual(action.input.entries[0].model, {
      connectionId: '00000000-0000-0000-0000-000000000001',
      modelId: 'chosen',
    });
    assert.equal(action.input.entries[0].thinkingLevel, 'medium');
    assert.deepEqual(action.recovery.input.entries[0].model, {
      connectionId: '00000000-0000-0000-0000-000000000001',
      modelId: 'chosen',
    });
    const reference = {
      contract: { name: 'maka.workhub.Coordinator', version: 1 },
      method: 'savePresets',
      operation: 'original-save',
    };
    const reply = (cleanup = 'confirmed') => ({
      reference,
      receipt: {
        kind: 'recorded',
        outcome: {
          kind: 'returned',
          value: { kind: 'saved', snapshot: { revision: 1, entries: action.input.entries } },
        },
        cleanup: { kind: cleanup },
      },
    });
    event({ kind: 'action', key: action.key, event: { status: 'failed', message: 'reply lost' } });
    raw('save');
    assert.equal(out.actions.length, 0);
    out = component.mount(props, {});
    finish([{ cursor: 3, reference, recovery: action.recovery }]);
    finish([]);
    assert.deepEqual(out.actions[0].input, reference);
    finish(reply('unconfirmed'));
    raw('new');
    assert.equal(out.actions.length, 0);
    raw('receipt');
    finish(reply());
    assert.equal(out.actions[0].method, 'presets');
    finish({ revision: 4, entries: action.input.entries });
    finish({ names: ['Read', 'Write', 'WebSearch'] });
    raw('edit-agent-1');
    assert.equal(out.frame.state.draftRevision, 4);
    assert.deepEqual(out.frame.state.draft.model, {
      connectionId: '00000000-0000-0000-0000-000000000001',
      modelId: 'chosen',
    });
    assert.equal(out.frame.state.draft.thinkingLevel, 'medium');
    raw('remove');
    assert.equal(out.frame.state.removing, true);
    raw('cancel');
    assert.equal(out.frame.state.removing, false);
    assert.equal(out.actions.length, 0);
  });
