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
import Ajv from 'ajv/dist/2020.js';
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'dark' }).Appearance.value;
const { component, activate: detailsExports } = await import(
  '../dist/packages/tool-details/tui.js'
);

function props() {
  return {
    theme,
    locale: 'en',
    load: false,
    context: { sessionId: 'session', through: 8 },
    tool: {
      toolUseId: 'read-file',
      call: {
        position: { sourceSequence: 7, itemIndex: 0 },
        name: 'Read',
        title: null,
        presentation: 'maka.workspace.Files@1/readText',
        ancestorToolUseIds: [],
        pending: false,
        dispatched: true,
      },
      result: {
        position: { sourceSequence: 8, itemIndex: 0 },
        isError: false,
        settlement: 'returned',
        image: false,
      },
    },
    expanded: false,
    focusOnPointer: true,
  };
}
function reply(out, action, value, current) {
  return component.event(
    out.frame.state,
    { kind: 'action', key: action.key, event: { status: 'succeeded', value } },
    current,
    {},
  );
}
function page(action, current, text) {
  return {
    kind: 'ready',
    through: 8,
    filePath: null,
    source: {
      position: action.input.source.position,
      field: action.input.field,
      path: action.input.path,
      format: 'text',
      text,
      offset: 0,
      totalBytes: Buffer.byteLength(text),
      tool: current.tool,
    },
  };
}
test('tool summaries pause in background and reuse completed reads on return', () => {
  const current = props();
  let out = component.mount(current, {});
  assert.equal(out.actions.length, 0);
  current.load = true;
  out = component.update(out.frame.state, current, {});
  const actions = out.actions;
  assert.equal(actions.length, 2);
  current.load = false;
  out = component.update(out.frame.state, current, {});
  assert.equal(out.actions.length, 0);
  for (const action of actions) {
    const text = action.input.field === 'arguments' ? '/workspace/source.rs' : 'completed';
    out = reply(out, action, page(action, current, text), current);
    assert.equal(out.actions.length, 0);
  }
  current.load = true;
  out = component.update(out.frame.state, current, {});
  assert.equal(out.actions.length, 0, 'completed summary does not reload on re-entry');
  current.expanded = true;
  out = component.update(out.frame.state, current, {});
  assert.equal(out.actions.length, 4, 'expanded Read fetches content and line metadata');
});

test('a completed source is cached while its reader is in the background', () => {
  const current = props();
  current.load = true;
  let out = component.mount(current, {});
  const action = out.actions[0];
  current.load = false;
  out = reply(out, action, page(action, current, '/workspace/source.rs'), current);
  assert.equal(out.actions.length, 0);
  assert.equal(
    Object.values(out.frame.state.parts).find(
      (part) =>
        part.field === action.input.field &&
        JSON.stringify(part.path) === JSON.stringify(action.input.path),
    ).page.text,
    '/workspace/source.rs',
  );
  current.load = true;
  out = component.update(out.frame.state, current, {});
  assert.equal(out.actions.length, 0, 'completed source does not poll on return');
});

test('omitting load keeps compact summaries eager', () => {
  const current = props();
  delete current.load;
  const schema = detailsExports().ToolDetails.contract.interface.props;
  const valid = new Ajv({ strict: false }).compile(schema);
  assert.equal(valid(current), true, JSON.stringify(valid.errors));
  assert.equal(component.mount(current, {}).actions.length, 2);
});

for (const target of ['tui']) {
  const { component: details } = await import(`../dist/packages/tool-details/${target}.js`);
  test(`${target}: background completion caches values without starting dependent reads`, () => {
    const current = props();
    current.load = true;
    current.tool.call.presentation = 'maka.workspace.Files@1/write';
    let out = details.mount(current, {});
    const initial = out.actions;
    assert.equal(initial.length, 2);
    current.load = false;
    out = details.update(out.frame.state, current, {});
    for (const action of initial) {
      const value = page(
        action,
        current,
        action.input.field === 'arguments' ? '/workspace/written.rs' : 'completed',
      );
      out = details.event(
        out.frame.state,
        { kind: 'action', key: action.key, event: { status: 'succeeded', value } },
        current,
        {},
      );
      assert.equal(out.actions.length, 0, 'background status must not fan out into resolvedPath');
    }
    current.load = true;
    out = details.update(out.frame.state, current, {});
    assert.equal(out.actions.length, 1);
    assert.deepEqual(out.actions[0].input.path, ['value', 'path']);
    const action = out.actions[0];
    out = details.event(
      out.frame.state,
      {
        kind: 'action',
        key: action.key,
        event: { status: 'succeeded', value: page(action, current, '/workspace/resolved.rs') },
      },
      current,
      {},
    );
    assert.equal(out.actions.length, 0);
    current.load = false;
    out = details.update(out.frame.state, current, {});
    current.load = true;
    out = details.update(out.frame.state, current, {});
    assert.equal(out.actions.length, 0, 'all completed parts remain cached');
  });

  test(`${target}: background expanded readers retain a completed excerpt`, () => {
    const current = props();
    current.expanded = true;
    let out = details.mount(current, {});
    assert.equal(out.actions.length, 0);
    current.load = true;
    out = details.update(out.frame.state, current, {});
    const action = out.actions[0];
    current.load = false;
    out = details.event(
      out.frame.state,
      {
        kind: 'action',
        key: action.key,
        event: { status: 'succeeded', value: page(action, current, 'completed excerpt') },
      },
      current,
      {},
    );
    assert.equal(out.actions.length, 0);
    assert.equal(
      Object.values(out.frame.state.parts).find(
        (part) =>
          part.field === action.input.field &&
          JSON.stringify(part.path) === JSON.stringify(action.input.path),
      ).page.text,
      'completed excerpt',
    );
    current.load = true;
    out = details.update(out.frame.state, current, {});
    assert.equal(out.actions.length, 0, 'completed excerpt remains cached');
  });
}
