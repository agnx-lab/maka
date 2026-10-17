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
const inputValid = ajv.compile(contracts.textInput.interface.props);
const methodValid = Object.fromEntries(
  Object.entries(contracts.settings.interface.methods).map(([key, method]) => [
    key,
    ajv.compile(method.input),
  ]),
);
function slots(value, found = []) {
  if (!value || typeof value !== 'object') return found;
  if (value.binding === 'input' && value.props) found.push(value);
  for (const child of Object.values(value)) if (typeof child === 'object') slots(child, found);
  return found;
}
for (const target of ['tui'])
  test(`Insights ${target} retains settlement fence, captures reviewed prices and recovers the original mutation`, async () => {
    const { component } = await import(`../dist/packages/insights/${target}.js`);
    const props = { scope: null, theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    finish([]);
    assert.ok(methodValid.usageSummary(out.actions[0].input));
    const totals = {
      calls: 2,
      success: 1,
      error: 1,
      aborted: 0,
      unknown: 0,
      durationMs: 12,
      input: { known: 1, missing: 1 },
      output: { known: 2, missing: 1 },
      cacheRead: { known: 0, missing: 2 },
      cacheWrite: { known: 0, missing: 2 },
      reasoning: { known: 0, missing: 2 },
      cost: { knownUsd: 0.123, unvalued: 1, unpriced: 1 },
    };
    finish({
      through: 19,
      nextOffset: null,
      summary: {
        models: totals,
        tools: { calls: 0, error: 0, unknown: 0, rejected: 0 },
        pending: { models: 1, tools: 0 },
        byProvider: [],
        byModel: [],
        byTool: [],
      },
    });
    assert.match(JSON.stringify(out.frame), /unvalued/);
    raw('tab-activity');
    assert.ok(methodValid.usage(out.actions[0].input));
    assert.equal(out.actions[0].input.range.through, 19);
    finish({ through: 19, attempts: [], total: 33, nextOffset: 32 });
    raw('activity-next');
    assert.equal(out.actions[0].input.offset, 32);
    assert.equal(out.actions[0].input.range.through, 19);
    finish({ through: 19, attempts: [], total: 33, nextOffset: null });
    raw('tab-pricing');
    finish({ kind: 'page', revision: 2, offset: 0, entries: [], nextOffset: null });
    raw('price-new');
    const form = {
      model: 'custom:model',
      input: '1.25',
      output: '5',
      cacheRead: '',
      cacheWrite: '0',
    };
    const observations = {};
    for (const [i, [name, value]] of Object.entries(form).entries()) {
      const id = `insights-field-${out.frame.state.editing.key}-${name}`;
      let observation = {
        revision: 1,
        snapshot: {
          marked: false,
          text: {
            owner: `00000000-0000-0000-0000-${String(i + 1).padStart(12, '0')}`,
            revision: 1,
            bytes: 0,
          },
        },
        receipt: null,
      };
      event({ kind: 'component', child: id, name: 'changed', event: observation });
      const command = out.frame.state.fields[name].command;
      observation = {
        ...observation,
        revision: 2,
        receipt: {
          owner: command.owner,
          sequence: command.sequence,
          outcome: { status: 'applied' },
        },
      };
      event({ kind: 'component', child: id, name: 'changed', event: observation });
      observations[name] = {
        ...observation,
        revision: 3,
        receipt: null,
        snapshot: {
          marked: false,
          text: { ...observation.snapshot.text, revision: 3, bytes: Buffer.byteLength(value) },
        },
      };
      event({ kind: 'component', child: id, name: 'changed', event: observations[name] });
    }
    const editors = slots(out.frame);
    assert.equal(editors.length, 6);
    for (const slot of editors)
      assert.ok(inputValid(slot.props), JSON.stringify(inputValid.errors));
    raw('tab-overview');
    assert.equal(
      slots(out.frame).length,
      6,
      'switching tabs retains the actual native editor owners',
    );
    raw('tab-pricing');
    assert.deepEqual(out.frame.state.fields.input.observation, observations.input);
    raw('price-save');
    for (const [name, value] of Object.entries(form)) {
      assert.equal(out.actions[0].method, 'read');
      assert.deepEqual(out.actions[0].resources.text.input, {
        kind: 'at_revision',
        owner: observations[name].snapshot.text.owner,
        revision: 3,
      });
      finish({ text: value, metadata: observations[name].snapshot.text });
    }
    const action = out.actions[0];
    assert.equal(action.method, 'updatePrice');
    assert.ok(
      methodValid.updatePrice(action.input),
      JSON.stringify(methodValid.updatePrice.errors),
    );
    assert.deepEqual(action.input, {
      expectedRevision: 2,
      mutation: {
        kind: 'upsert',
        pricing: {
          modelKey: 'custom:model',
          inputUsdPer1M: 1.25,
          outputUsdPer1M: 5,
          cacheWriteUsdPer1M: 0,
        },
      },
    });
    const reference = {
      contract: { name: 'maka.settings.Settings', version: 1 },
      method: 'updatePrice',
      operation: 'original-price',
    };
    event({
      kind: 'action',
      key: action.key,
      event: {
        status: 'operation',
        reply: {
          reference,
          receipt: { kind: 'recorded', outcome: { kind: 'unknown', reason: 'lost reply' } },
        },
      },
    });
    raw('price-save');
    assert.equal(out.actions.length, 0, 'unknown outcome cannot issue another write');
    out = component.mount(props, {});
    finish([{ cursor: 7, reference, recovery: action.recovery }]);
    assert.deepEqual(out.actions[0].input, { after: 7, limit: 16 });
    finish([]);
    assert.equal(out.actions[0].method, 'read');
    assert.deepEqual(out.actions[0].input, reference);
    finish({
      reference,
      receipt: {
        kind: 'recorded',
        outcome: {
          kind: 'returned',
          value: { kind: 'updated', result: { kind: 'committed', revision: 3 } },
        },
      },
    });
    assert.equal(out.actions[0].method, 'usageSummary');
    assert.equal(out.frame.state.current, null);
  });

test('Usage search, group pages and saved view retain the requested session fence without restoring another session', async () => {
  const { component } = await import('../dist/packages/insights/tui.js');
  const props = {
    theme,
    locale: 'en',
    open: true,
    busy: false,
    shell: { back: 0 },
    scope: { sequence: 1, sessionId: 'session-a', label: 'First session' },
  };
  let out = component.mount(props, {});
  const event = (input) => (out = component.event(out.frame.state, input, props, {}));
  const raw = (key, value) => event({ kind: 'raw', key, event: value ? { key: value } : {} });
  const finish = (value) =>
    event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
  const checkpoint = (sequence, revision, value) =>
    event({
      kind: 'component',
      child: 'insights-bookmark',
      name: 'changed',
      event: { sequence, state: { kind: 'ready', revision, value } },
    });
  const totals = {
    calls: 0,
    error: 0,
    aborted: 0,
    unknown: 0,
    input: { known: 0, missing: 0 },
    output: { known: 0, missing: 0 },
    cacheRead: { known: 0, missing: 0 },
    cacheWrite: { known: 0, missing: 0 },
    reasoning: { known: 0, missing: 0 },
    cost: { knownUsd: 0, unvalued: 0 },
  };
  const report = {
    through: 71,
    nextOffset: 32,
    summary: {
      models: totals,
      tools: { calls: 0, error: 0, unknown: 0, rejected: 0 },
      pending: { models: 0, tools: 0 },
      byProvider: [],
      byModel: [],
      byTool: [],
    },
  };
  finish([]);
  assert.equal(out.actions[0].input.range.sessionId, 'session-a');
  finish(report);
  raw('group-next');
  assert.equal(out.actions[0].input.offset, 32);
  assert.equal(out.actions[0].input.range.through, 71);
  finish({ ...report, nextOffset: null });
  raw('tab-activity');
  finish({ through: 71, attempts: [], total: 0, nextOffset: null });
  const child = 'insights-search-input';
  const text = { owner: '00000000-0000-0000-0000-000000000099', revision: 0, bytes: 0 };
  const observe = (observation) =>
    event({ kind: 'component', child, name: 'changed', event: observation });
  observe({ revision: 0, snapshot: { marked: false, text }, receipt: null });
  const command = out.frame.state.search.command;
  observe({
    revision: 1,
    snapshot: { marked: false, text: { ...text, revision: 1 } },
    receipt: { owner: text.owner, sequence: command.sequence, outcome: { status: 'applied' } },
  });
  const capture = { ...text, revision: 2, bytes: 13 };
  observe({ revision: 2, snapshot: { marked: false, text: capture }, receipt: null });
  raw('search-apply');
  assert.equal(out.actions[0].resources.text.input.revision, 2);
  finish({ metadata: capture, text: 'fixture-model' });
  assert.equal(out.actions[0].input.selection.search, 'fixture-model');
  assert.equal(out.actions[0].input.range.through, 71);
  assert.equal(out.actions[0].input.range.sessionId, 'session-a');
  finish({ through: 71, attempts: [], total: 0, nextOffset: null });
  checkpoint(0, 0, null);
  raw('view-save');
  const saved = structuredClone(out.frame.state.bookmark.command);
  assert.equal(saved.kind, 'write');
  assert.equal(saved.value.selection.search, 'fixture-model');
  assert.equal(saved.value.sessionId, undefined);
  checkpoint(saved.sequence, 1, saved.value);
  props.scope = { sequence: 2, sessionId: 'session-b', label: 'Second session' };
  out = component.update(out.frame.state, props, {});
  assert.equal(out.actions[0].input.range.sessionId, 'session-b');
  assert.equal(out.actions[0].input.range.through, null);
  finish({ ...report, through: 92 });
  finish({ through: 92, attempts: [], total: 0, nextOffset: null });
  raw('view-load');
  assert.equal(out.actions[0].input.range.sessionId, 'session-b');
  assert.equal(out.actions[0].input.range.through, null);
  finish({ ...report, through: 93 });
  assert.equal(out.actions[0].input.selection.search, 'fixture-model');
  assert.equal(out.actions[0].input.range.through, 93);
});
