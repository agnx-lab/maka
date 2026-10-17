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
const { component: timeline } = await import('../dist/packages/timeline/tui.js');
const { component: details, activate: detailExports } = await import(
  '../dist/packages/tool-details/tui.js'
);
const nodes = (node) =>
  node && typeof node === 'object'
    ? [
        node,
        ...Object.values(node).flatMap((value) =>
          Array.isArray(value) ? value.flatMap(nodes) : nodes(value),
        ),
      ]
    : [];
const find = (out, key) => nodes(out.frame.view).find((node) => node.key === key);
function tool(id, call, result, name = id, presentation = null, ancestors = []) {
  return {
    toolUseId: id,
    call: {
      position: { sourceSequence: call, itemIndex: 0 },
      name,
      title: null,
      presentation,
      ancestorToolUseIds: ancestors,
      dispatched: true,
      pending: false,
    },
    result: {
      position: { sourceSequence: result, itemIndex: 0 },
      settlement: 'returned',
      isError: false,
      image: false,
    },
  };
}
function message(id, sequence, metadata, role = 'tool_call', text = '') {
  return {
    id,
    contentKey: 'body-' + id,
    source: { sourceSequence: sequence, itemIndex: 0 },
    role,
    text,
    timestamp: null,
    continued: false,
    image: false,
    tool: metadata,
  };
}
function props(messages, timings = []) {
  return {
    theme,
    locale: 'en',
    messages,
    timings,
    copyRequest: null,
    browse: null,
    markReadEnabled: false,
    toolContext: { sessionId: 'session', through: 20 },
    waitingTools: [],
    liveTools: [],
    source: null,
    search: null,
    loading: false,
    older: false,
    newer: false,
    latest: false,
    landing: { sequence: 0, edge: 'end' },
    live: false,
  };
}
function raw(out, p, key, kind = 'activate') {
  return timeline.event(out.frame.state, { kind: 'raw', key, event: { kind } }, p, {});
}
function detailClient(metadata, expanded = true) {
  const p = {
    theme,
    locale: 'en',
    context: { sessionId: 'session', through: 20 },
    tool: metadata,
    expanded,
    focusOnPointer: true,
  };
  let out = details.mount(p, {});
  return {
    get out() {
      return out;
    },
    get props() {
      return p;
    },
    update(changes) {
      Object.assign(p, changes);
      out = details.update(out.frame.state, p, {});
    },
    reply(action, text, format, totalBytes = Buffer.byteLength(text)) {
      assert.ok(action, 'the component must request the original field');
      out = details.event(
        out.frame.state,
        {
          kind: 'action',
          key: action.key,
          event: {
            status: 'succeeded',
            value: {
              kind: 'ready',
              through: 20,
              source: {
                position: action.input.source.position,
                field: action.input.field,
                ...(action.input.path === null ? {} : { path: action.input.path }),
                format,
                offset: 0,
                totalBytes,
                text,
                tool: metadata,
              },
            },
          },
        },
        p,
        {},
      );
    },
    request(path, field) {
      return out.actions.find(
        (action) =>
          action.input.field === field &&
          JSON.stringify(action.input.path) === JSON.stringify(path),
      );
    },
  };
}

test('a parent result cannot move its turn clock above later visible children', () => {
  const parent = tool('parent', 1, 6, 'exec');
  const child = tool('child', 3, 4, 'cua_operation', null, ['cell', 'parent']);
  const p = props(
    [
      message('parent', 1, parent),
      message('child', 3, child),
      message('child-result', 4, child, 'tool_result'),
      message('parent-result', 6, parent, 'tool_result'),
      message('next-turn', 7, null, 'user', 'Next question'),
    ],
    [{ turnId: 'turn-one', afterMessageId: 'parent-result', startedAt: 10, finished: null }],
  );
  let out = timeline.mount(p, {});
  assert.deepEqual(
    find(out, 'timeline').children.map((node) => node.key),
    ['message-parent', 'message-child', 'time-turn-one', 'message-next-turn'],
  );
  p.timings[0].finished = { at: 20, outcome: 'completed' };
  out = timeline.update(out.frame.state, p, {});
  assert.deepEqual(find(out, 'time-turn-one').props.elapsed, { from: 10, to: 20 });
  p.messages = p.messages.slice(1, 3);
  p.timings[0].afterMessageId = 'child-result';
  out = timeline.update(out.frame.state, p, {});
  assert.deepEqual(
    find(out, 'timeline').children.map((node) => node.key),
    ['message-child', 'time-turn-one'],
  );
});

test('empty summary still opens original computer input and result; empty unreferenced rows cannot expand', () => {
  const computer = tool('computer', 1, 2, 'cua_repl');
  const p = props([
    message('computer', 1, computer),
    message('computer-result', 2, computer, 'tool_result'),
    message('empty', 3, null),
  ]);
  let out = timeline.mount(p, {});
  out = raw(out, p, 'toggle-computer');
  assert.equal(find(out, 'tool-details-computer').props.expanded, true);
  assert.equal(find(out, 'toggle-empty'), undefined);
  out = raw(out, p, 'toggle-empty');
  assert.deepEqual(out.frame.state.expanded, ['computer']);
});

for (const name of ['exec', 'cua_repl', 'cua_operation', 'ReadSkill']) {
  test(`${name} reads original fields on expansion, preserving text and JSON formats`, () => {
    const metadata = tool(name, 1, 2, name);
    const c = detailClient(metadata, false);
    assert.deepEqual(c.out.actions, []);
    c.update({ expanded: true });
    const argumentsRequest = c.request(null, 'arguments');
    const resultRequest = c.request(null, 'result');
    assert.deepEqual(argumentsRequest.input, {
      sessionId: 'session',
      through: 20,
      source: { kind: 'row', position: { sourceSequence: 1, itemIndex: 0 } },
      field: 'arguments',
      path: null,
      position: { kind: 'start' },
    });
    assert.deepEqual(resultRequest.input, {
      sessionId: 'session',
      through: 20,
      source: { kind: 'row', position: { sourceSequence: 2, itemIndex: 0 } },
      field: 'result',
      path: null,
      position: { kind: 'start' },
    });
    c.reply(argumentsRequest, '{"code":"await browser.tabs.list()"}', 'json');
    c.reply(resultRequest, 'Chrome tabs: Inbox, Docs', 'text');
    assert.equal(
      find(c.out, 'arguments-body').props.source,
      '{"code":"await browser.tabs.list()"}',
    );
    assert.equal(find(c.out, 'result-body').props.source, 'Chrome tabs: Inbox, Docs');
  });
}

test('process evidence drives heading colors without treating a returned failure as success', () => {
  const metadata = tool('shell', 1, 2, 'Shell', 'maka.workspace.Processes@1/execute');
  const p = props([message('shell', 1, metadata)]);
  let out = timeline.mount(p, {});
  const c = detailClient(metadata, false);
  const statusRequest = c.request(['kind'], 'result');
  c.reply(statusRequest, 'completed', 'text');
  c.reply(c.request(['value', 'kind'], 'result'), 'terminal', 'text');
  c.reply(c.request(['value', 'status'], 'result'), 'completed', 'text');
  c.reply(c.request(['value', 'exitCode'], 'result'), '7', 'json');
  const context = c.out.events.find((event) => event.name === 'context');
  assert.equal(context.value.status, 'failed');
  const valid = new Ajv({ strict: false }).compile(
    detailExports().ToolDetails.contract.interface.events.context,
  );
  assert.equal(valid(context.value), true, JSON.stringify(valid.errors));
  out = timeline.event(
    out.frame.state,
    { kind: 'component', child: 'tool-details-shell', name: 'context', event: context.value },
    p,
    {},
  );
  assert.equal(find(out, 'marker-shell').color, theme.terminal.error);
  assert.equal(find(out, 'marker-shell').text, '×');
  out = timeline.event(
    out.frame.state,
    {
      kind: 'component',
      child: 'tool-details-shell',
      name: 'context',
      event: { ...context.value, result: { sourceSequence: 2, itemIndex: 1 }, status: 'succeeded' },
    },
    p,
    {},
  );
  assert.notEqual(
    find(out, 'marker-shell').color,
    theme.terminal.success,
    'stale evidence cannot claim success for another result',
  );
});

test('a truly empty original result closes its disclosure and removes the empty expansion target', () => {
  const metadata = {
    toolUseId: 'empty-result',
    call: null,
    result: {
      position: { sourceSequence: 2, itemIndex: 0 },
      settlement: 'returned',
      isError: false,
      image: false,
    },
  };
  const p = props([message('empty-result', 2, metadata, 'tool_result')]);
  let out = raw(timeline.mount(p, {}), p, 'toggle-empty-result');
  const c = detailClient(metadata);
  c.reply(c.request(null, 'result'), '', 'text');
  const context = c.out.events.find((event) => event.name === 'context');
  assert.equal(context.value.expandable, false);
  out = timeline.event(
    out.frame.state,
    {
      kind: 'component',
      child: 'tool-details-empty-result',
      name: 'context',
      event: context.value,
    },
    p,
    {},
  );
  assert.equal(find(out, 'toggle-empty-result'), undefined);
  assert.deepEqual(out.frame.state.expanded, []);
});

for (const [processStatus, exit, expectedStatus, tone, symbol] of [
  ['completed', 0, 'succeeded', 'success', '·'],
  ['failed', 7, 'failed', 'error', '×'],
  ['running', null, 'running', 'accent', '◌'],
  ['cancelled', 143, 'cancelled', 'warning', '×'],
  ['orphaned', null, 'unknown', 'warning', '?'],
]) {
  test(`process ${processStatus} uses its recorded outcome color`, () => {
    const metadata = tool('shell', 1, 2, 'Shell', 'maka.workspace.Processes@1/execute');
    const p = props([message('shell', 1, metadata)]);
    let out = timeline.mount(p, {});
    const c = detailClient(metadata, false);
    c.reply(c.request(['kind'], 'result'), 'completed', 'text');
    c.reply(c.request(['value', 'kind'], 'result'), 'terminal', 'text');
    c.reply(c.request(['value', 'status'], 'result'), processStatus, 'text');
    let context = c.out.events.find((event) => event.name === 'context');
    if (exit !== null) {
      c.reply(c.request(['value', 'exitCode'], 'result'), String(exit), 'json');
      context = c.out.events.find((event) => event.name === 'context') ?? context;
    }
    assert.equal(context.value.status, expectedStatus);
    out = timeline.event(
      out.frame.state,
      { kind: 'component', child: 'tool-details-shell', name: 'context', event: context.value },
      p,
      {},
    );
    assert.equal(find(out, 'marker-shell').color, theme.terminal[tone]);
    assert.equal(find(out, 'marker-shell').text, symbol);
  });
}

test('bottom collapse preserves other expanded tools and returns to its own heading', () => {
  const p = props([
    message('first', 1, tool('first', 1, 2)),
    message('second', 3, tool('second', 3, 4)),
  ]);
  let out = timeline.mount(p, {});
  assert.equal(find(out, 'collapse-first'), undefined);
  out = raw(out, p, 'toggle-first');
  out = raw(out, p, 'toggle-second');
  assert.equal(find(out, 'collapse-first').label, '↑ Hide details');
  assert.equal(find(out, 'tool-panel-first').children.at(-1).key, 'collapse-first');
  out = raw(out, p, 'collapse-first');
  assert.deepEqual(out.frame.state.expanded, ['second']);
  assert.equal(find(out, 'collapse-first'), undefined);
  assert.deepEqual(out.frame.state.command.target, { kind: 'reveal_child', key: 'message-first' });
  assert.ok(out.events.some(({ name, value }) => name === 'detached' && value.value === true));
});
