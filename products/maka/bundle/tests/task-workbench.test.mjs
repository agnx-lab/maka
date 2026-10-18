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
const appearance = await import('../dist/packages/appearance/tui.js');
const theme = appearance.activate({ scheme: 'dark' }).Appearance.value;
const module = await import('../dist/packages/agent-screen/tui.js');
module.activate({});
const values = { appearance: [theme] };
const dataSource = {
  stateDomain: 'a'.repeat(64),
  deployment: '00000000-0000-0000-0000-000000000001',
  namespace: ['00000000-0000-0000-0000-000000000002'],
};
const session = (id) => ({
  id,
  title: id,
  labels: [],
  revision: 1,
  updatedAt: 1,
  archived: false,
  activity: 'idle',
  boundaryRevision: 0,
  hasUnread: false,
  dataSource,
  permissions: { access: 'workspace_write', requestPermissions: true },
  workspace: { path: '/project', directoryIdentity: 'a'.repeat(64), identity: 'workspace-a' },
});
const chunk = (sequence, text, extra = {}) => ({
  messageId: 'message-' + sequence,
  turnId: 'turn-1',
  timestamp: 1,
  role: 'assistant',
  field: 'body',
  position: { sourceSequence: sequence, itemIndex: 0 },
  offset: 0,
  totalBytes: Buffer.byteLength(text),
  text,
  tool: null,
  ...extra,
});
const page = (through, chunks, extra = {}) => ({
  kind: 'ready',
  through,
  chunks,
  before: null,
  after: null,
  ...extra,
});
function nodes(node) {
  return node && typeof node === 'object'
    ? [
        node,
        ...Object.values(node).flatMap((v) => (Array.isArray(v) ? v.flatMap(nodes) : nodes(v))),
      ]
    : [];
}
const nativeLanguage = (locale) => ({
  revision: 1,
  language: locale,
  resolvedLanguage: locale,
  displayName: '',
  assistantTone: '',
  compactionPrompt: 'Default',
  defaultCompactionPrompt: 'Default',
  workspaceInstructions: true,
  incognitoActive: false,
  shellTitles: true,
});
function client(locale = 'en', options = {}) {
  const screen = module;
  screen.activate({ ...options.configuration });
  let out = screen.component.mount({}, values);
  const c = {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    event(input) {
      out = screen.component.event(c.state, input, {}, values);
      return c;
    },
    raw(key) {
      return c.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    child(child, name, event = {}) {
      return c.event({ kind: 'component', key: name, child, name, event });
    },
    reply(action, value) {
      assert.ok(action);
      return c.event({ kind: 'action', key: action.key, event: { status: 'succeeded', value } });
    },
    action(method) {
      return out.actions.find((a) => a.method === method);
    },
    observe(through, run = null) {
      return c.reply(c.action('observe'), { session: c.state.selected, through, parts: [], run });
    },
    history(value) {
      return c.reply(c.action('history'), value);
    },
    select(id) {
      return c.child('navigation', 'select', { sessionId: id });
    },
    find(key) {
      return nodes(out.frame.view).find((node) => node.key === key);
    },
    reader() {
      return c.state.visits.find((visit) => visit.id === c.state.selected?.id).key;
    },
  };
  c.reply(c.action('assistantPreferences'), nativeLanguage(locale));
  c.child('recovery-checkpoint', 'changed', {
    sequence: 0,
    state: { kind: 'ready', revision: null, value: null },
  });
  c.reply(c.action('page'), []);
  c.event({
    kind: 'action',
    key: c.state.catalogWatch.key,
    event: {
      status: 'updated',
      value: {
        sessions: options.sessions ?? [
          session('Task A'),
          session('Task B'),
          session('Task C'),
          session('Task D'),
        ],
        before: null,
        after: null,
      },
    },
  });
  return c;
}

test('message browsing follows the displayed page and live content, including empty bodies', () => {
  const live = {
    sequence: 5,
    messageId: 'live-answer',
    invocationId: 'run-a',
    stepId: 'step-a',
    text: 'Live answer',
    closedAt: null,
    field: 'body',
  };
  for (const [chunks, latest, fragments, displayed, enabled] of [
    [[], true, [], [], false],
    [[chunk(1, '')], true, [], [''], true],
    [[chunk(1, 'Older answer')], false, [live], ['Older answer'], true],
    [[], true, [live], ['Live answer'], true],
    [[], false, [live], [], false],
  ]) {
    const c = client().observe(4).history(page(4, []));
    c.state.window = { page: page(4, chunks), latest };
    c.state.live = fragments;
    c.raw('palette-open');
    const command = c.find('command-palette').props.commands.find(({ id }) => id === 'browse');
    assert.equal(command.disabled, !enabled);
    assert.deepEqual(
      c.find(c.reader()).props.messages.map(({ text }) => text),
      displayed,
    );
    c.child('command-palette', 'invoked', { id: 'browse' });
    assert.equal(c.state.dialog, enabled ? null : 'palette');
    assert.equal(c.state.browse === null, !enabled);
  }
});

test('an active turn retains compact history across pages without a tool or message count cap', async () => {
  const { activate } = await import('../dist/packages/timeline/tui.js');
  const schema = activate().Timeline.contract.interface.props;
  const validate = new Ajv({ strict: false }).compile(schema.properties.liveTools);
  const calls = Array.from({ length: 40 }, (_, index) => {
    const sequence = index + 1;
    return chunk(sequence, '', {
      role: 'tool_call',
      tool: {
        toolUseId: 'tool-' + sequence,
        call: {
          position: { sourceSequence: sequence, itemIndex: 0 },
          name: 'Read',
          title: null,
          presentation: null,
          pending: false,
          ancestorToolUseIds: [],
          dispatched: true,
        },
        result: null,
      },
    });
  });
  const c = client()
    .observe(40, { invocationId: 'run-a', turnId: 'turn-1', state: 'running' })
    .history(
      page(40, calls.slice(32), {
        before: { position: { sourceSequence: 33, itemIndex: 0 }, offset: 0, field: 'body' },
      }),
    );
  const reader = c.reader();
  for (let start = 24; start >= 0; start -= 8) {
    c.child(reader, 'older').history(
      page(40, calls.slice(start, start + 8), {
        before: start
          ? { position: { sourceSequence: start + 1, itemIndex: 0 }, offset: 0, field: 'body' }
          : null,
        after: { position: { sourceSequence: start + 9, itemIndex: 0 }, offset: 0, field: 'body' },
      }),
    );
  }
  const props = c.find(reader).props;
  assert.equal(props.messages.length, 40);
  assert.deepEqual(
    props.liveTools,
    calls.map((entry) => entry.tool.toolUseId),
  );
  assert.equal(validate(props.liveTools), true, JSON.stringify(validate.errors));
});

test('switching tasks preserves the native reading owner, page, search basis and auxiliary context', () => {
  const c = client()
    .observe(20)
    .history(page(20, [chunk(10, 'Older Chinese 中文🙂'), chunk(18, 'Newer answer')]));
  const first = c.reader();
  c.raw('search-open');
  const searchEpoch = c.find(first).props.search.epoch;
  c.raw('outputs-open');
  assert.equal(c.state.inspector, 'outputs');
  const landing = structuredClone(c.state.landing);
  c.select('Task B')
    .observe(2)
    .history(page(2, [chunk(1, 'Task B answer')]));
  assert.notEqual(c.reader(), first);
  assert.ok(c.find(first), 'the original reading owner remains mounted while hidden');
  assert.equal(c.find('task-reader-' + first).visible, false);
  c.select('Task A');
  assert.equal(c.reader(), first);
  assert.deepEqual(c.state.landing, landing);
  assert.equal(c.state.window.page.through, 20);
  assert.equal(c.state.inspector, 'outputs');
  assert.equal(
    c.find(first).props.search.epoch,
    searchEpoch,
    'returning does not reset the search input',
  );
  c.observe(25);
  assert.equal(
    c.action('history'),
    undefined,
    'background progress does not replace the saved reading page',
  );
  assert.equal(c.find(first).props.latest, true);
  assert.equal(c.find('task-latest'), undefined);
  c.child('timeline-noncurrent', 'source', { position: { sourceSequence: 10, itemIndex: 0 } });
  assert.equal(
    c.state.source,
    null,
    'late callbacks from another task do not redirect this reader',
  );
});

test('a restored empty task loads new failure facts without requiring a tail receipt', () => {
  const c = client().observe(0).history(page(0, []));
  c.select('Task B')
    .observe(1)
    .history(page(1, [chunk(1, 'Another task')]));
  c.select('Task A').observe(0);
  c.event({
    kind: 'action',
    key: c.state.watch.key,
    event: {
      status: 'updated',
      value: {
        kind: 'events',
        through: 30,
        nextAfter: 30,
        events: [
          { sequence: 1, invocationId: 'failed-run', turnId: 'turn-1', fact: { kind: 'opened' } },
          {
            sequence: 30,
            invocationId: 'failed-run',
            turnId: 'turn-1',
            fact: { kind: 'ended', failed: true },
          },
        ],
      },
    },
  });
  assert.equal(c.action('history').input.through, 30);
  c.history(
    page(30, [
      chunk(20, 'Failed request', {
        role: 'user',
        timing: { startedAt: 1, finished: { at: 2, outcome: 'failed' } },
      }),
    ]),
  );
  assert.equal(c.find('task-introduction').visible, false);
});

test('failure arriving during the initial empty history read is not stranded behind the introduction', () => {
  const c = client().observe(0);
  const pending = c.action('history');
  c.event({
    kind: 'action',
    key: c.state.watch.key,
    event: {
      status: 'updated',
      value: {
        kind: 'events',
        through: 30,
        nextAfter: 30,
        events: [
          { sequence: 1, invocationId: 'failed-run', turnId: 'turn-1', fact: { kind: 'opened' } },
          {
            sequence: 30,
            invocationId: 'failed-run',
            turnId: 'turn-1',
            fact: { kind: 'ended', failed: true },
          },
        ],
      },
    },
  });
  c.reply(pending, page(0, []));
  assert.equal(c.action('history').input.through, 30);
  c.history(
    page(30, [
      chunk(20, 'Failed request', {
        role: 'user',
        timing: { startedAt: 1, finished: { at: 2, outcome: 'failed' } },
      }),
    ]),
  );
  assert.equal(c.find('task-introduction').visible, false);
});

test('a failure with no assistant reply traverses empty state pages and retains the failure timing', () => {
  const c = client()
    .observe(30)
    .history(
      page(30, [], {
        before: { position: { sourceSequence: 25, itemIndex: 0 }, offset: 0, field: 'body' },
      }),
    );
  assert.deepEqual(c.action('history').input.position, {
    kind: 'before',
    cursor: { position: { sourceSequence: 25, itemIndex: 0 }, offset: 0, field: 'body' },
  });
  assert.equal(c.action('history').input.through, 30);
  c.history(
    page(
      30,
      [
        chunk(20, 'The failed request', {
          role: 'user',
          timing: { startedAt: 1, finished: { at: 2, outcome: 'failed' } },
        }),
      ],
      { after: { position: { sourceSequence: 25, itemIndex: 0 }, offset: 0, field: 'body' } },
    ),
  );
  assert.equal(c.state.window.latest, true);
});

test('returning to a streaming task preserves its visible prefix until latest is explicitly requested', () => {
  const c = client()
    .observe(1, { invocationId: 'run-a', turnId: 'turn-1', state: 'running' })
    .history(page(1, [chunk(1, 'Task goal', { role: 'user' })]));
  c.event({
    kind: 'action',
    key: c.state.watch.key,
    event: {
      status: 'updated',
      value: {
        kind: 'events',
        through: 3,
        nextAfter: 3,
        events: [
          {
            sequence: 2,
            invocationId: 'run-a',
            turnId: 'turn-1',
            fact: {
              kind: 'part_started',
              messageId: 'stream-a',
              stepId: 'step-a',
              partId: 'text',
              textKind: 'text',
            },
          },
          {
            sequence: 3,
            invocationId: 'run-a',
            turnId: 'turn-1',
            fact: { kind: 'delta', stepId: 'step-a', partId: 'text', text: '正在阅读的中文🙂' },
          },
        ],
      },
    },
  });
  const reader = c.reader();
  const visible = c.find(reader).props.messages;
  assert.ok(visible.some((message) => message.text === '正在阅读的中文🙂'));
  c.select('Task B').observe(1).history(page(1, []));
  assert.deepEqual(
    c.find(reader).props.messages,
    visible,
    'hidden readers retain streaming content too',
  );
  c.select('Task A').observe(50);
  c.child(reader, 'tailReady', { landing: c.state.landing.sequence, readingRevision: 9 });
  assert.equal(
    c.action('history'),
    undefined,
    'returning at the old tail does not jump to new results',
  );
  assert.deepEqual(c.find(reader).props.messages, visible);
  c.raw('task-latest');
  assert.equal(c.action('history').input.through, 50);
});

test('returning to a running task buffers every delta while preserving the reading position', () => {
  const run = { invocationId: 'run-a', turnId: 'turn-1', state: 'running' };
  const c = client()
    .observe(1, run)
    .history(page(1, [chunk(1, 'Goal', { role: 'user' })]));
  const reader = c.reader();
  c.select('Task B').observe(1).history(page(1, []));
  c.select('Task A').observe(1, run);
  const landing = structuredClone(c.state.landing);
  const events = [
    {
      sequence: 2,
      invocationId: 'run-a',
      turnId: 'turn-1',
      fact: {
        kind: 'part_started',
        messageId: 'stream-a',
        stepId: 'step-a',
        partId: 'text',
        textKind: 'text',
      },
    },
    ...Array.from({ length: 100 }, (_, i) => ({
      sequence: i + 3,
      invocationId: 'run-a',
      turnId: 'turn-1',
      fact: { kind: 'delta', stepId: 'step-a', partId: 'text', text: `片段${i}🙂 ` },
    })),
  ];
  c.event({
    kind: 'action',
    key: c.state.watch.key,
    event: {
      status: 'updated',
      value: {
        kind: 'events',
        through: 102,
        nextAfter: 102,
        events,
      },
    },
  });
  assert.equal(c.find(reader).props.live, false, 'new text does not re-enable following');
  assert.deepEqual(c.state.landing, landing);
  assert.equal(c.action('history'), undefined);
  const expected = events
    .slice(1)
    .map((event) => event.fact.text)
    .join('');
  assert.equal(c.state.live.map((fragment) => fragment.text).join(''), expected);
  c.raw('task-latest').history(page(102, [chunk(1, 'Goal', { role: 'user' })]));
  assert.equal(c.find(reader).props.live, true);
  assert.ok(c.find(reader).props.messages.some((message) => message.text === expected));
});

test('output review opens original structured evidence and full output at the same history fence', () => {
  const tool = {
    toolUseId: 'write-1',
    call: {
      position: { sourceSequence: 4, itemIndex: 0 },
      name: 'Write',
      title: null,
      presentation: 'maka.workspace.Files@1/write',
      pending: false,
      ancestorToolUseIds: [],
      dispatched: true,
    },
    result: {
      position: { sourceSequence: 4, itemIndex: 256 },
      settlement: 'returned',
      isError: false,
      image: false,
    },
  };
  const c = client()
    .observe(20)
    .history(
      page(20, [
        chunk(4, 'write arguments', { role: 'tool_call', tool }),
        chunk(8, 'write result', {
          role: 'tool_result',
          position: { sourceSequence: 4, itemIndex: 256 },
          tool,
        }),
        chunk(10, 'Delivery description'),
      ]),
    );
  c.raw('outputs-open');
  const evidence = nodes(c.out.frame.view).find((node) => node.binding === 'tools');
  assert.equal(evidence.props.expanded, false);
  assert.deepEqual(evidence.props.context, { sessionId: 'Task A', through: 20 });
  c.event({ kind: 'raw', key: 'output-item-write-1', event: { kind: 'submit' } });
  assert.equal(
    nodes(c.out.frame.view).find((node) => node.binding === 'tools').props.expanded,
    true,
  );
  c.raw('output-result-body-message-4');
  assert.equal(c.state.inspector, null);
  assert.equal(c.action('source').input.through, 20);
  assert.deepEqual(c.action('source').input.source.position, { sourceSequence: 4, itemIndex: 256 });
  assert.equal(c.action('source').input.field, 'result');
});

test('output evidence resolves the original row when two turns reuse a tool id', () => {
  const tool = (call, result) => ({
    toolUseId: 'reused-call',
    call: {
      position: { sourceSequence: call, itemIndex: 0 },
      name: 'Write',
      title: null,
      presentation: 'maka.workspace.Files@1/write',
      pending: false,
      ancestorToolUseIds: [],
      dispatched: true,
    },
    result: {
      position: { sourceSequence: result, itemIndex: 0 },
      settlement: 'returned',
      isError: false,
      image: false,
    },
  });
  const first = tool(4, 8);
  const second = tool(12, 16);
  const c = client()
    .observe(20)
    .history(
      page(20, [
        chunk(4, 'first arguments', { turnId: 'first-turn', role: 'tool_call', tool: first }),
        chunk(8, 'first result', { turnId: 'first-turn', role: 'tool_result', tool: first }),
        chunk(12, 'second arguments', { turnId: 'second-turn', role: 'tool_call', tool: second }),
        chunk(16, 'second result', { turnId: 'second-turn', role: 'tool_result', tool: second }),
      ]),
    );
  c.raw('outputs-open');
  assert.ok(c.find('output-result-body-message-4'));
  assert.ok(c.find('output-result-body-message-12'));
  c.raw('output-result-body-message-12');
  assert.deepEqual(c.action('source').input, {
    sessionId: 'Task A',
    through: 20,
    source: { kind: 'row', position: { sourceSequence: 16, itemIndex: 0 } },
    field: 'result',
    position: { kind: 'start' },
  });
});

test('history paging orders sibling rows from one fact and retains their exact identities', () => {
  const first = chunk(10, 'First sibling', {
    messageId: 'first-sibling',
    position: { sourceSequence: 10, itemIndex: 255 },
  });
  const second = chunk(10, 'Second sibling', {
    messageId: 'second-sibling',
    position: { sourceSequence: 10, itemIndex: 256 },
  });
  const boundary = {
    position: { sourceSequence: 10, itemIndex: 256 },
    offset: 0,
    field: 'body',
  };
  const c = client()
    .observe(10)
    .history(page(10, [second], { before: boundary }));
  const reader = c.reader();
  c.child(reader, 'older').history(page(10, [first], { after: boundary }));
  const messages = c.find(reader).props.messages;
  assert.deepEqual(
    messages.map(({ text }) => text),
    ['First sibling', 'Second sibling'],
  );
  assert.deepEqual(
    messages.map(({ source }) => source),
    [
      { sourceSequence: 10, itemIndex: 255 },
      { sourceSequence: 10, itemIndex: 256 },
    ],
  );
  assert.equal(new Set(messages.map(({ id }) => id)).size, 2);
  c.state.selected.hasUnread = true;
  c.child(reader, 'markRead', {
    sessionId: 'Task A',
    position: { sourceSequence: 10, itemIndex: 256 },
    field: 'body',
  });
  assert.equal(c.action('markRead').input.messageId, 'second-sibling');
});

test('long task visits stay within the UI call envelope while recent readers remain retained', () => {
  const c = client()
    .observe(20)
    .history(page(20, [chunk(10, '界🙂'.repeat(12000))]));
  c.select('Task B')
    .observe(30)
    .history(page(30, [chunk(20, '中文'.repeat(12000))]));
  assert.ok(c.state.visits.length >= 2);
  c.select('Task C')
    .observe(40)
    .history(page(40, [chunk(30, 'long output '.repeat(9000))]));
  c.select('Task D')
    .observe(50)
    .history(page(50, [chunk(40, 'long output '.repeat(9000))]));
  assert.ok(c.state.visits.length <= 3);
  assert.ok(Buffer.byteLength(JSON.stringify(c.out)) < 1024 * 1024);
});

test('prefetching adjacent sections of one long message preserves the visible section and anchor', () => {
  const part = (offset, text) => chunk(10, text, { offset, totalBytes: 60000 });
  const cursor = (offset) => ({
    position: { sourceSequence: 10, itemIndex: 0 },
    offset,
    field: 'body',
  });
  const c = client()
    .observe(20)
    .history(page(20, [part(40000, 'Tail')], { before: cursor(40000) }));
  const reader = c.reader();
  const landing = structuredClone(c.state.landing);
  const tail = c.find(reader).props.messages[0];
  c.child(reader, 'older').history(
    page(20, [part(20000, 'Middle')], {
      before: cursor(20000),
      after: cursor(40000),
    }),
  );
  assert.deepEqual(c.state.landing, landing);
  assert.deepEqual(c.find(reader).props.messages.at(-1), tail);
  assert.equal(new Set(c.find(reader).props.messages.map((message) => message.id)).size, 2);
  c.child(reader, 'older').history(page(20, [part(0, 'Start')], { after: cursor(20000) }));
  assert.deepEqual(c.state.landing, landing);
  assert.deepEqual(
    c.find(reader).props.messages.map((message) => message.text),
    ['Start', 'Middle', 'Tail'],
  );
  assert.equal(new Set(c.find(reader).props.messages.map((message) => message.id)).size, 3);
});

test('scrolling traverses empty history pages without another gesture or losing the visible anchor', () => {
  const cursor = (sequence) => ({
    position: { sourceSequence: sequence, itemIndex: 0 },
    offset: 0,
    field: 'body',
  });
  for (const direction of ['older', 'newer']) {
    const backward = direction === 'older';
    const c = client()
      .observe(100)
      .history(
        page(100, [chunk(50, 'Visible')], {
          before: cursor(40),
          after: cursor(60),
        }),
      );
    const reader = c.reader();
    const landing = structuredClone(c.state.landing);
    c.child(reader, direction).history(
      page(100, [], {
        before: cursor(30),
        after: cursor(70),
      }),
    );
    assert.deepEqual(c.action('history').input.position, {
      kind: backward ? 'before' : 'after',
      cursor: cursor(backward ? 30 : 70),
    });
    assert.equal(c.action('history').input.through, 100);
    assert.deepEqual(
      c.find(reader).props.messages.map((message) => message.text),
      ['Visible'],
    );
    c.history(
      page(100, [chunk(backward ? 20 : 80, 'Adjacent')], {
        before: backward ? null : cursor(70),
        after: backward ? cursor(30) : null,
      }),
    );
    assert.deepEqual(c.state.landing, landing);
    assert.deepEqual(
      c.find(reader).props.messages.map((message) => message.text),
      backward ? ['Adjacent', 'Visible'] : ['Visible', 'Adjacent'],
    );
  }
});

test('reading prompt context belongs to the retained page and ignores late replies after paging', () => {
  const c = client()
    .observe(20)
    .history(page(20, [chunk(1, 'Old question', { role: 'user' })]));
  const reader = c.reader();
  assert.equal(c.find(reader).props.leadingPrompt.text, 'Old question');
  c.raw('task-latest').history(
    page(20, [chunk(18, 'New answer', { turnId: 'turn-2' })], {
      before: { position: { sourceSequence: 18, itemIndex: 0 }, offset: 0, field: 'body' },
    }),
  );
  const pending = c.action('historyPrompt');
  assert.equal(c.find(reader).props.leadingPrompt, null);
  c.raw('task-latest').history(page(20, [chunk(8, 'Earlier question', { role: 'user' })]));
  c.reply(pending, { position: { sourceSequence: 16, itemIndex: 0 }, text: 'New question' });
  assert.equal(c.find(reader).props.leadingPrompt.text, 'Earlier question');
  c.child(reader, 'prompt', { position: { sourceSequence: 8, itemIndex: 0 } });
  assert.deepEqual(c.action('history').input.position, {
    kind: 'row',
    position: { sourceSequence: 8, itemIndex: 0 },
    field: 'body',
  });
  assert.equal(c.action('history').input.through, 20);
  assert.equal(c.find('task-goal'), undefined);
});

test('reading older content on a short page suspends automatic following including an in-flight tail reply', () => {
  const c = client()
    .observe(10, { invocationId: 'run-a', turnId: 'turn-1', state: 'running' })
    .history(
      page(10, [chunk(9, 'Visible')], {
        before: { position: { sourceSequence: 8, itemIndex: 0 }, offset: 0, field: 'body' },
      }),
    );
  const reader = c.reader();
  c.state.latestKnown = 20;
  c.child(reader, 'tailReady', { landing: c.state.landing.sequence, readingRevision: 1 });
  const pending = c.action('history');
  assert.ok(pending);
  c.child(reader, 'older');
  c.reply(pending, page(20, [chunk(19, 'New output')]));
  assert.deepEqual(
    c.find(reader).props.messages.map((message) => message.text),
    ['Visible'],
  );
  c.child(reader, 'older').history(
    page(10, [chunk(7, 'Older')], {
      after: { position: { sourceSequence: 8, itemIndex: 0 }, offset: 0, field: 'body' },
    }),
  );
  c.child(reader, 'tailReady', { landing: c.state.landing.sequence, readingRevision: 2 });
  assert.equal(c.action('history'), undefined);
  assert.equal(c.find(reader).props.live, false);
  assert.deepEqual(
    c.find(reader).props.messages.map((message) => message.text),
    ['Older', 'Visible'],
  );
  c.raw('task-latest').history(page(20, [chunk(19, 'New output')]));
  assert.equal(c.find(reader).props.live, true);
});

test('a settled stop observation clears the stopping notice and shows the canonical stopped outcome', () => {
  const c = client();
  c.state.notice = 'stopping';
  c.observe(10).history(
    page(10, [
      chunk(9, 'Partial output', {
        timing: { startedAt: 1, finished: { at: 2, outcome: 'aborted' } },
      }),
    ]),
  );
  assert.equal(c.state.notice, null);
});

test('the checklist follows its session, pages live progress, and rejects stale observations', () => {
  const c = client()
    .observe(1)
    .history(page(1, [chunk(1, 'Working')]));
  const firstWatch = c.out.subscriptions.find((watch) => watch.method === 'watchChecklist');
  assert.equal(firstWatch.input.sessionId, 'Task A');
  const items = Array.from({ length: 20 }, (_, index) => ({
    content: `Step ${index + 1} 中文🙂`,
    status: index === 0 ? 'completed' : index === 1 ? 'in_progress' : 'pending',
  }));
  const update = (watch, value) =>
    c.event({
      kind: 'action',
      key: watch.key,
      event: { status: 'updated', value },
    });
  const first = {
    enabled: true,
    revision: 1,
    total: 25,
    completed: 1,
    current: items[1],
    offset: 0,
    items,
    nextOffset: 20,
  };
  update(firstWatch, first);
  assert.match(JSON.stringify(c.find('checklist-open')), /1\/25/u);
  c.raw('checklist-open');
  assert.equal(c.state.inspector, 'checklist');
  assert.match(JSON.stringify(c.find('checklist-items')), /\[x\] Step 1 中文🙂/u);
  assert.match(JSON.stringify(c.find('checklist-items')), /\[>\] Step 2 中文🙂/u);
  c.raw('checklist-next');
  const nextWatch = c.out.subscriptions.find((watch) => watch.method === 'watchChecklist');
  assert.equal(nextWatch.input.offset, 20);
  assert.notEqual(nextWatch.key, firstWatch.key);
  update(firstWatch, first);
  assert.equal(c.state.checklist.page, null);
  update(nextWatch, {
    ...first,
    offset: 20,
    items: [{ content: 'Final step 中文🙂', status: 'pending' }],
    nextOffset: null,
  });
  assert.match(JSON.stringify(c.find('checklist-items')), /Final step 中文🙂/u);
  c.select('Task B').observe(0).history(page(0, []));
  const otherWatch = c.out.subscriptions.find((watch) => watch.method === 'watchChecklist');
  assert.equal(otherWatch.input.sessionId, 'Task B');
  update(nextWatch, first);
  assert.equal(c.state.checklist.page, null);
  update(otherWatch, {
    enabled: true,
    revision: null,
    total: 0,
    completed: 0,
    current: null,
    offset: 0,
    items: [],
    nextOffset: null,
  });
  assert.equal(c.find('checklist-open'), undefined);
  c.raw('checklist-open');
  assert.match(JSON.stringify(c.find('checklist-empty')), /No checklist yet/u);
  c.event({
    kind: 'action',
    key: otherWatch.key,
    event: { status: 'failed', message: 'disconnected' },
  });
  assert.match(JSON.stringify(c.find('checklist-error')), /Unable to load/u);
  c.raw('checklist-retry');
  assert.equal(
    c.out.subscriptions.find((watch) => watch.method === 'watchChecklist').input.sessionId,
    'Task B',
  );
  assert.equal(c.state.checklist.failed, false);
});

const recapModel = {
  choice: {
    selection: { connectionId: 'fixture', revision: 1, modelId: 'model' },
    connectionName: 'Fixture',
    label: 'Model',
    thinkingLevels: [],
    defaultThinkingLevel: null,
  },
  thinkingLevel: null,
};
const recapReady = {
  kind: 'ready',
  operationId: '00000000-0000-0000-0000-000000000099',
  through: 8,
  text: 'Tests passed; deployment is next. 中文🙂',
  modelId: 'model',
};
const recapReference = {
  source: { ...dataSource, revision: 1 },
  namespace: dataSource.namespace,
  export: 'agent',
  method: 'generateRecap',
  operation: '00000000-0000-0000-0000-000000000098',
  fingerprint: 'a'.repeat(64),
  recovery: 'journal_v1',
  contract: {},
};

test('recap requires explicit model fee confirmation, preserves the draft, and refreshes the original cache', () => {
  const c = client()
    .observe(8)
    .history(page(8, [chunk(1, 'Verified source')]));
  c.child('models', 'resolved', { selectionKey: String(c.state.epoch), selection: recapModel });
  const draft = structuredClone({ draft: c.state.draft, command: c.state.command });
  c.raw('recap-open');
  assert.equal(c.state.inspector, 'recap');
  assert.equal(c.action('generateRecap'), undefined);
  const read = c.action('recap');
  assert.equal(read.input.sessionId, 'Task A');
  c.reply(read, null);
  assert.match(JSON.stringify(c.find('recap-empty')), /No saved recap/u);
  c.raw('recap-generate');
  assert.match(JSON.stringify(c.find('recap-fee')), /may incur model charges.*model/u);
  assert.equal(c.action('generateRecap'), undefined);
  c.raw('recap-confirm');
  const generate = c.action('generateRecap');
  assert.deepEqual(generate.input, { sessionId: 'Task A', model: recapModel.choice.selection });
  c.raw('recap-confirm');
  assert.equal(
    c.action('generateRecap'),
    undefined,
    'confirmation cannot admit duplicate paid work',
  );
  c.event({
    kind: 'action',
    key: generate.key,
    event: {
      status: 'operation',
      reply: {
        reference: recapReference,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'returned', value: recapReady },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  assert.match(JSON.stringify(c.find('recap-text')), /Tests passed; deployment is next/u);
  c.raw('recap-refresh');
  assert.deepEqual(c.action('recap').input, {
    sessionId: 'Task A',
    operationId: recapReady.operationId,
  });
  c.reply(c.action('recap'), recapReady);
  c.raw('recap-close');
  assert.deepEqual({ draft: c.state.draft, command: c.state.command }, draft);
  c.raw('recap-open');
  c.reply(c.action('recap'), recapReady);
  assert.equal(c.action('generateRecap'), undefined);
  c.raw('recap-close');
  c.select('Task B')
    .observe(2)
    .history(page(2, [chunk(1, 'Other session')]));
  c.raw('recap-open');
  const stale = c.action('recap');
  c.raw('recap-close');
  c.select('Task C')
    .observe(2)
    .history(page(2, [chunk(1, 'Current session')]));
  c.reply(stale, recapReady);
  assert.equal(c.state.recap.receipt, null, 'a late result cannot become another session recap');
});

test('pending recap and unknown original operation allow read-only recovery without another generation', () => {
  const c = client()
    .observe(8)
    .history(page(8, [chunk(1, 'Verified source')]));
  c.child('models', 'resolved', { selectionKey: String(c.state.epoch), selection: recapModel });
  c.raw('recap-open');
  c.reply(c.action('recap'), { kind: 'pending', operationId: recapReady.operationId, through: 8 });
  c.raw('recap-generate');
  c.raw('recap-confirm');
  assert.equal(c.action('generateRecap'), undefined);
  c.raw('recap-refresh');
  assert.equal(c.action('recap').input.operationId, recapReady.operationId);
  c.reply(c.action('recap'), recapReady);
  c.raw('recap-generate');
  c.raw('recap-confirm');
  const original = c.action('generateRecap');
  c.event({
    kind: 'action',
    key: original.key,
    event: {
      status: 'operation',
      reply: {
        reference: recapReference,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'unknown', diagnostic: 'connection lost' },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  assert.deepEqual(c.state.uncertain.reference, recapReference);
  c.raw('recap-refresh');
  assert.equal(c.action('recap').method, 'recap');
  c.reply(c.action('recap'), recapReady);
  c.raw('recap-generate');
  c.raw('recap-confirm');
  assert.equal(c.action('generateRecap'), undefined);
  assert.deepEqual(
    c.state.uncertain.reference,
    recapReference,
    'a cached result cannot settle another original operation',
  );
  c.raw('recap-check');
  assert.equal(c.action('read').binding, 'receipts');
  assert.deepEqual(c.action('read').input, recapReference);
  c.reply(c.action('read'), {
    reference: recapReference,
    receipt: {
      kind: 'recorded',
      outcome: { kind: 'returned', value: recapReady },
      cleanup: { kind: 'confirmed' },
    },
  });
  assert.equal(c.state.uncertain, null);
  assert.equal(c.action('generateRecap'), undefined);
});

test('floating activity follows deltas and canonical tools without moving the history window', () => {
  const run = { invocationId: 'run-a', turnId: 'turn-1', state: 'running' };
  const c = client()
    .observe(1, run)
    .history(page(1, [chunk(1, 'Current goal', { role: 'user' })]));
  const reader = c.reader();
  const window = structuredClone(c.state.window);
  const landing = structuredClone(c.state.landing);
  c.child(reader, 'detached', { landing: landing.sequence, value: true });
  const deliver = (through, facts) =>
    c.event({
      kind: 'action',
      key: c.state.watch.key,
      event: {
        status: 'updated',
        value: {
          kind: 'events',
          through,
          nextAfter: through,
          events: facts.map(([sequence, fact]) => ({
            sequence,
            fact,
            invocationId: 'run-a',
            turnId: 'turn-1',
          })),
        },
      },
    });
  deliver(3, [
    [
      2,
      {
        kind: 'part_started',
        messageId: 'thinking',
        stepId: 'step',
        partId: 'p',
        textKind: 'thinking',
      },
    ],
    [3, { kind: 'delta', stepId: 'step', partId: 'p', text: 'Reasoning now' }],
  ]);
  assert.equal(c.find(reader).props.activeOutput.role, 'thinking');
  assert.equal(c.find(reader).props.activeOutput.text, 'Reasoning now');
  assert.equal(c.action('history'), undefined, 'streaming alone does not read canonical history');
  deliver(4, [[4, { kind: 'transcript_changed' }]]);
  const pending = c.action('history');
  const tool = {
    toolUseId: 'call',
    call: {
      position: { sourceSequence: 4, itemIndex: 0 },
      name: 'read',
      title: null,
      presentation: null,
      pending: true,
      ancestorToolUseIds: [],
      dispatched: true,
    },
    result: null,
  };
  c.reply(pending, page(4, [chunk(4, '', { role: 'tool_call', tool })]));
  assert.equal(c.find(reader).props.activeOutput.tool.toolUseId, 'call');
  deliver(6, [
    [
      5,
      { kind: 'part_started', messageId: 'answer', stepId: 'next', partId: 'p', textKind: 'text' },
    ],
    [6, { kind: 'delta', stepId: 'next', partId: 'p', text: 'x'.repeat(17000) + ' newest text' }],
  ]);
  assert.ok(
    c.find(reader).props.activeOutput.text.endsWith('newest text'),
    'floating output continues beyond the transcript live-prefix budget',
  );
  assert.deepEqual(c.state.window, window);
  assert.deepEqual(c.state.landing, landing);
  assert.equal(c.find('task-goal'), undefined);
  deliver(7, [[7, { kind: 'transcript_changed' }]]);
  const stale = c.action('history');
  deliver(8, [[8, { kind: 'ended', failed: false }]]);
  assert.equal(c.find(reader).props.activeOutput, null);
  c.reply(stale, page(7, [chunk(7, 'Late canonical reply')]));
  assert.equal(c.find(reader).props.activeOutput, null, 'late reads cannot revive an ended turn');
  assert.deepEqual(c.state.window, window);
});

test('a cold long conversation queries its preceding prompt without paging the reader', () => {
  const c = client()
    .observe(20)
    .history(
      page(20, [chunk(18, 'Long answer tail')], {
        before: { position: { sourceSequence: 18, itemIndex: 0 }, offset: 0, field: 'thinking' },
      }),
    );
  const window = structuredClone(c.state.window);
  const landing = structuredClone(c.state.landing);
  assert.deepEqual(c.action('historyPrompt').input, {
    sessionId: 'Task A',
    through: 20,
    position: { sourceSequence: 18, itemIndex: 0 },
  });
  assert.equal(c.action('history'), undefined, 'context never walks answer pages');
  c.reply(c.action('historyPrompt'), {
    position: { sourceSequence: 2, itemIndex: 0 },
    text: 'Earlier user message',
  });
  assert.equal(c.find(c.reader()).props.leadingPrompt.text, 'Earlier user message');
  assert.deepEqual(c.state.window, window);
  assert.deepEqual(c.state.landing, landing);
  assert.equal(c.action('historyPrompt'), undefined);
});

test('footer sandbox changes use the current revision and settle the original receipt before another change', () => {
  const c = client().observe(0).history(page(0, []));
  assert.match(c.find('sandbox-open').label, /^Workspace write$/u);
  c.raw('sandbox-open').raw('sandbox-danger_full_access');
  const original = c.action('metadata');
  assert.deepEqual(original.input, {
    sessionId: 'Task A',
    expectedRevision: 1,
    title: 'Task A',
    labels: [],
    permissions: {
      settings: { access: 'danger_full_access', requestPermissions: true },
      revokeGrants: true,
    },
  });
  assert.equal(c.state.selected.permissions.access, 'workspace_write');
  c.raw('sandbox-read_only');
  assert.equal(c.action('metadata'), undefined);
  const reference = { ...recapReference, method: 'metadata' };
  c.event({
    kind: 'action',
    key: original.key,
    event: {
      status: 'operation',
      reply: {
        reference,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'unknown', diagnostic: 'lost reply' },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  c.raw('sandbox-danger_full_access');
  assert.equal(c.action('metadata'), undefined);
  c.raw('sandbox-check');
  assert.equal(c.action('read').binding, 'receipts');
  c.reply(c.action('read'), {
    reference,
    receipt: {
      kind: 'recorded',
      outcome: {
        kind: 'returned',
        value: {
          kind: 'committed',
          session: {
            ...c.state.selected,
            revision: 2,
            boundaryRevision: 1,
            permissions: { access: 'danger_full_access', requestPermissions: true },
          },
        },
      },
      cleanup: { kind: 'confirmed' },
    },
  });
  assert.equal(c.state.dialog, null);
  assert.equal(c.state.uncertain, null);
  assert.match(c.find('sandbox-open').label, /Sandbox off/u);
  c.raw('sandbox-open').raw('sandbox-read_only');
  assert.equal(c.action('metadata').input.expectedRevision, 2);
  assert.equal(c.action('metadata').input.permissions.settings.access, 'read_only');
});

const submission = (context, extra = {}) => ({
  context,
  dataSource,
  key: 'send-2',
  text: 'same text',
  attachments: [],
  status: 'pending',
  messageId: null,
  ...extra,
});
test('submitted history replaces the current-run queue notice while queued messages keep their actions', () => {
  const c = client().observe(0).history(page(0, []));
  c.state.queue = {
    revision: 1,
    entries: [
      { messageId: 'next', kind: 'followup', preview: 'next message' },
      { messageId: 'current', kind: 'in_flight', preview: 'same text' },
    ],
  };
  c.child('composer', 'submission', submission(c.state.selected.id));
  const queue = c.find('queue').props;
  assert.equal(c.find(c.reader()).props.messages[0].pending, true);
  assert.deepEqual(
    queue.rows.map((row) => row.key),
    ['next'],
  );
  assert.equal(queue.entry, 'next');
  assert.ok(queue.rows[0].actions.some((action) => action.key === 'edit'));
  c.state.queue.entries = c.state.queue.entries.filter((entry) => entry.kind === 'in_flight');
  c.child('composer', 'submission', submission(c.state.selected.id));
  assert.deepEqual(c.find('queue').props.rows, []);
  assert.equal(c.find('queue').props.entry, null);
});
test('submitted message is muted until admission and reconciles by canonical identity, including overtaking history', () => {
  const c = client()
    .observe(1)
    .history(page(1, [chunk(1, 'same text', { role: 'user' })]));
  const id = c.state.selected.id;
  c.child('composer', 'submission', submission(id));
  let messages = c.find(c.reader()).props.messages;
  assert.equal(messages.length, 2);
  assert.equal(messages[1].pending, true);
  c.state.window = {
    latest: true,
    page: page(3, [
      chunk(1, 'same text', { role: 'user' }),
      chunk(2, 'same text', { role: 'user', messageId: 'canonical-message' }),
      chunk(3, 'reply'),
    ]),
  };
  c.child('composer', 'submission', submission(id));
  assert.equal(
    c.find(c.reader()).props.messages[2].text,
    'reply',
    'pending send cannot hide canonical assistant output',
  );
  c.child('composer', 'submission', submission(id, { status: 'unknown' }));
  messages = c.find(c.reader()).props.messages;
  assert.equal(
    messages.length,
    4,
    'canonical facts stay visible while the original receipt is unknown',
  );
  assert.equal(messages[2].text, 'reply');
  assert.equal(messages[3].pending, true);
  c.child(
    'composer',
    'submission',
    submission(id, { status: 'accepted', messageId: 'canonical-message' }),
  );
  messages = c.find(c.reader()).props.messages;
  assert.equal(messages.length, 3);
  assert.deepEqual(
    messages.map((message) => message.id),
    ['message-1', 'canonical-message', 'message-3'],
  );
  assert.equal(
    messages.some((message) => message.pending),
    false,
  );
  assert.equal(c.state.visits.find((visit) => visit.id === id).submissions.length, 0);
});
test('accepted preview turns normal before history; session switches and failures keep original ownership', () => {
  const c = client().observe(1).history(page(1, []));
  const id = c.state.selected.id,
    reader = c.reader();
  c.child('composer', 'submission', submission(id));
  c.child(
    'composer',
    'submission',
    submission(id, { status: 'accepted', messageId: 'canonical-message' }),
  );
  assert.equal(c.find(reader).props.messages[0].pending, false);
  c.select('Task B').observe(1).history(page(1, []));
  assert.equal(c.find(c.reader()).props.messages.length, 0);
  assert.equal(c.find(reader).props.messages[0].text, 'same text');
  c.child('composer', 'submission', submission('Task B'));
  c.child('composer', 'submission', submission('Task B', { status: 'removed' }));
  assert.equal(c.find(c.reader()).props.messages.length, 0);
  c.child(
    'composer',
    'submission',
    submission(id, { status: 'removed', dataSource: { ...dataSource, deployment: 'foreign' } }),
  );
  assert.equal(
    c.find(reader).props.messages.length,
    1,
    'foreign delivery cannot change original session',
  );
});

test('attachment preview captures only original identities and survives attachment cleanup', () => {
  const c = client().observe(1).history(page(1, []));
  const id = c.state.selected.id;
  const file = {
    sessionId: id,
    dataSource,
    id: 'attachment',
    name: 'original.txt',
    mimeType: 'text/plain',
    bytes: 3,
  };
  const files = Array.from({ length: 9 }, (_, index) => ({
    ...file,
    id: `attachment-${index}`,
    name: `original-${index}.txt`,
  }));
  const ids = files.map((file) => file.id);
  const preview =
    'original-0.txt\noriginal-1.txt\noriginal-2.txt\noriginal-3.txt\noriginal-4.txt\noriginal-5.txt\noriginal-6.txt\noriginal-7.txt\noriginal-8.txt';
  c.state.attachments.files = [
    ...files,
    { ...files[0], dataSource: { ...dataSource, deployment: 'foreign' }, name: 'foreign.txt' },
    { ...file, id: 'later', name: 'later.txt' },
  ];
  c.child('composer', 'submission', submission(id, { text: '', attachments: ids }));
  assert.equal(c.find(c.reader()).props.messages[0].text, preview);
  c.child(
    'composer',
    'submission',
    submission(id, {
      text: '',
      attachments: ids,
      status: 'accepted',
      messageId: 'canonical',
    }),
  );
  c.child('composer', 'sent', { context: id, dataSource, attachments: ids });
  assert.equal(c.find(c.reader()).props.messages[0].text, preview);
  assert.deepEqual(
    c.state.attachments.files.map((file) => file.name),
    ['foreign.txt', 'later.txt'],
  );
});

test('a catalogue-selected new session retains its file gesture until the original create receipt', () => {
  const c = client('en', { sessions: [] });
  c.raw('home-new');
  c.reply(c.action('directories'), {
    kind: 'roots',
    roots: [
      {
        name: 'Workspace',
        path: '/project',
        selection: {
          rootId: 'workspace',
          segments: [],
          directoryIdentity: 'sha256:' + 'a'.repeat(64),
        },
      },
    ],
  });
  const create = c.action('create');
  assert.ok(create);
  const created = session('Fresh');
  c.event({
    kind: 'action',
    key: c.state.catalogWatch.key,
    event: {
      status: 'updated',
      value: { sessions: [created], before: null, after: null },
    },
  });
  assert.equal(c.find('composer').props.enabled, true);
  assert.deepEqual(c.find('composer').props.filePaste, {
    context: created.id,
    allowFiles: true,
  });
  c.child('composer', 'readiness', [
    { context: created.id, dataSource: created.dataSource, ready: true },
  ]);
  const capture = { owner: '00000000-0000-0000-0000-000000000060', revision: 3 };
  const file = {
    name: 'original.gif',
    bytes: 43,
    digest: 'sha256:ab99355f27cb10c630ac0ac0921a683e4c812a89066cd731299ead501fa6797a',
  };
  c.child('composer', 'files', {
    context: created.id,
    dataSource: created.dataSource,
    remaining: 1,
    retryable: false,
    selection: { ...capture, phase: 'ready', file },
  });
  assert.equal(c.action('uploadAttachment'), undefined, 'the original create is still pending');
  c.event({
    kind: 'action',
    key: create.key,
    event: {
      status: 'operation',
      reply: {
        reference: { ...recapReference, method: 'create' },
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'returned', value: { kind: 'created', session: created } },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  const upload = c.action('uploadAttachment');
  assert.ok(upload, 'the original receipt resumes the retained file without another gesture');
  assert.deepEqual(upload.input, { sessionId: created.id, ...file });
  assert.deepEqual(upload.resources, {
    file: { child: 'composer', method: 'file', input: capture },
  });
});

test('a held backend selection retains the original file gesture until its receipt', () => {
  const c = client().observe(0).history(page(0, []));
  const original = c.state.selected;
  c.child('models', 'backends', { available: true });
  c.child('models', 'resolved', {
    selectionKey: String(c.state.epoch),
    selection: recapModel,
  });
  // The actual model picker emits selected when it applies its initial choice.
  c.child('models', 'selected', recapModel);
  const selection = c.action('selectBackend');
  assert.ok(selection);
  assert.equal(selection.input.sessionId, original.id);
  assert.equal(c.find('composer').props.enabled, true);
  assert.deepEqual(c.find('composer').props.filePaste, {
    context: original.id,
    allowFiles: true,
  });
  c.child('composer', 'readiness', [
    { context: original.id, dataSource: original.dataSource, ready: true },
  ]);
  const capture = { owner: '00000000-0000-0000-0000-000000000060', revision: 3 };
  const file = {
    name: 'original.gif',
    bytes: 43,
    digest: 'sha256:ab99355f27cb10c630ac0ac0921a683e4c812a89066cd731299ead501fa6797a',
  };
  c.child('composer', 'files', {
    context: original.id,
    dataSource: original.dataSource,
    remaining: 1,
    retryable: false,
    selection: { ...capture, phase: 'ready', file },
  });
  assert.equal(c.action('uploadAttachment'), undefined, 'backend persistence remains pending');
  c.event({
    kind: 'action',
    key: selection.key,
    event: {
      status: 'operation',
      reply: {
        reference: { ...recapReference, method: 'selectBackend' },
        receipt: {
          kind: 'recorded',
          outcome: {
            kind: 'returned',
            value: {
              kind: 'committed',
              session: { ...original, revision: 2, backend: selection.input.backend },
            },
          },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  const upload = c.action('uploadAttachment');
  assert.ok(upload, 'the backend receipt resumes the same file without another gesture');
  assert.deepEqual(upload.input, { sessionId: original.id, ...file });
  assert.deepEqual(upload.resources, {
    file: { child: 'composer', method: 'file', input: capture },
  });
});

test('an editable selected session accepts original file resources while history is pending', () => {
  const c = client().observe(1).history(page(1, []));
  c.select('Task B');
  const original = c.state.selected;
  assert.ok(c.action('observe'), 'the selected session observation remains pending');
  assert.equal(c.find('composer').props.enabled, true);
  assert.deepEqual(c.find('composer').props.filePaste, {
    context: original.id,
    allowFiles: true,
  });

  c.observe(0);
  assert.ok(c.action('history'), 'the initial history read remains pending');
  assert.equal(c.find('composer').props.enabled, true);
  assert.deepEqual(c.find('composer').props.filePaste, {
    context: original.id,
    allowFiles: true,
  });
  c.child('composer', 'readiness', [
    { context: original.id, dataSource: original.dataSource, ready: true },
  ]);
  const capture = { owner: '00000000-0000-0000-0000-000000000060', revision: 3 };
  const file = {
    name: 'original.gif',
    bytes: 43,
    digest: 'sha256:ab99355f27cb10c630ac0ac0921a683e4c812a89066cd731299ead501fa6797a',
  };
  c.child('composer', 'files', {
    context: original.id,
    dataSource: original.dataSource,
    remaining: 1,
    retryable: false,
    selection: { ...capture, phase: 'ready', file },
  });
  const upload = c.action('uploadAttachment');
  assert.ok(upload, 'history loading does not prevent the ready original resource upload');
  assert.deepEqual(upload.input, { sessionId: original.id, ...file });
  assert.deepEqual(upload.resources, {
    file: { child: 'composer', method: 'file', input: capture },
  });
});

test('the ninth attachment can be selected and removed without changing another source', () => {
  const c = client().observe(1).history(page(1, []));
  const id = c.state.selected.id;
  const files = Array.from({ length: 9 }, (_, index) => ({
    sessionId: id,
    dataSource,
    id: `attachment-${index}`,
    name: `file-${index}.txt`,
    mimeType: 'text/plain',
    bytes: 3,
  }));
  const foreign = { ...files[8], dataSource: { ...dataSource, deployment: 'foreign' } };
  c.state.attachments.files = [...files, foreign];
  c.child('composer', 'readiness', [{ context: id, dataSource, ready: true }]);
  c.raw('attachments-open');
  assert.ok(c.find('attachment-select-8'), 'the ninth item is rendered');
  c.raw('attachment-select-8');
  assert.equal(c.state.attachments.selection.selected, 'attachment-8');
  c.raw('attachment-remove-attachment-8');
  assert.deepEqual(c.state.attachments.files, [...files.slice(0, 8), foreign]);
  assert.equal(c.state.attachments.selection.selected, 'attachment-7');
});

test('pending preview truncation preserves complete Unicode attachment names', () => {
  const c = client().observe(1).history(page(1, []));
  const id = c.state.selected.id;
  c.state.attachments.files = [
    {
      sessionId: id,
      dataSource,
      id: 'attachment',
      name: '🙂.txt',
      mimeType: 'text/plain',
      bytes: 3,
    },
  ];
  c.child(
    'composer',
    'submission',
    submission(id, { text: 'a'.repeat(23998), attachments: ['attachment'] }),
  );
  const text = c.find(c.reader()).props.messages[0].text;
  assert.equal(text, 'a'.repeat(23998) + '\n');
  assert.equal(text.isWellFormed(), true);
});

test('pending and unknown submissions never replace canonical activity output', () => {
  for (const status of ['pending', 'unknown']) {
    const c = client()
      .observe(1, { invocationId: 'run-a', turnId: 'turn-1', state: 'running' })
      .history(page(1, []));
    const id = c.state.selected.id,
      reader = c.reader();
    c.child('composer', 'submission', submission(id));
    if (status === 'unknown') c.child('composer', 'submission', submission(id, { status }));
    c.child(reader, 'detached', { landing: c.state.landing.sequence, value: true });
    c.event({
      kind: 'action',
      key: c.state.watch.key,
      event: {
        status: 'updated',
        value: {
          kind: 'events',
          through: 2,
          nextAfter: 2,
          events: [
            {
              sequence: 2,
              invocationId: 'run-a',
              turnId: 'turn-1',
              fact: { kind: 'transcript_changed' },
            },
          ],
        },
      },
    });
    c.reply(c.action('history'), page(2, [chunk(2, 'Canonical agent output')]));
    assert.equal(c.find(reader).props.activeOutput.role, 'assistant');
    assert.equal(c.find(reader).props.activeOutput.text, 'Canonical agent output');
    assert.equal(c.state.visits.find((visit) => visit.id === id).submissions[0].status, status);
  }
});

test('viewing the selected latest tail clears unread only after its exact read receipt', async () => {
  const timeline = await import('../dist/packages/timeline/tui.js');
  const c = client()
    .observe(4)
    .history(page(4, [chunk(4, 'Latest answer')]));
  c.state.selected.hasUnread = true;
  c.raw('sidebar-toggle');
  assert.equal(
    c.state.selected.hasUnread,
    true,
    'selection and painting alone are not acknowledgement',
  );
  const props = c.find(c.reader()).props;
  const mounted = timeline.component.mount(props, {});
  const out = timeline.component.event(
    mounted.frame.state,
    {
      kind: 'component',
      child: 'reading',
      name: 'observed',
      event: {
        readingRevision: 1,
        pendingCommand: null,
        receipt: mounted.frame.state.command
          ? { sequence: mounted.frame.state.command.sequence }
          : null,
        viewport: {
          readingRevision: 1,
          atStart: false,
          atEnd: true,
          visibleChildKeys: ['message-' + props.messages.at(-1).id],
        },
      },
    },
    props,
    {},
  );
  const seen = out.events.find((event) => event.name === 'markRead');
  assert.ok(seen, 'the latest visible tail acknowledges its original message');
  c.child(c.reader(), seen.name, seen.value);
  const action = c.action('markRead');
  assert.deepEqual(action.input, { sessionId: 'Task A', messageId: 'message-4' });
  assert.equal(c.state.selected.hasUnread, true, 'the durable receipt owns read state');
  c.event({
    kind: 'action',
    key: action.key,
    event: {
      status: 'operation',
      reply: {
        reference: { operation: 'read-tail' },
        receipt: {
          kind: 'recorded',
          outcome: {
            kind: 'returned',
            value: {
              kind: 'observed',
              session: { ...c.state.selected, revision: 2, hasUnread: false },
            },
          },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  assert.equal(c.state.selected.hasUnread, false);
  assert.equal(c.state.catalog.sessions.find((entry) => entry.id === 'Task A').hasUnread, false);
});

test('read acknowledgement cannot replace newer unread observations or clear a different session', () => {
  const c = client()
    .observe(4)
    .history(page(4, [chunk(4, 'Old answer')]));
  c.state.selected.hasUnread = true;
  c.child(c.reader(), 'markRead', {
    sessionId: 'Task A',
    position: { sourceSequence: 4, itemIndex: 0 },
    field: 'body',
  });
  const action = c.action('markRead');
  assert.equal(action.input.messageId, 'message-4');
  c.state.selected = { ...c.state.selected, revision: 3, hasUnread: true };
  c.state.catalog.sessions[0] = c.state.selected;
  c.event({
    kind: 'action',
    key: action.key,
    event: {
      status: 'operation',
      reply: {
        reference: { operation: 'old-read-tail' },
        receipt: {
          kind: 'recorded',
          outcome: {
            kind: 'returned',
            value: {
              kind: 'observed',
              session: { ...c.state.selected, revision: 2, hasUnread: false },
            },
          },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  assert.equal(c.state.selected.hasUnread, true);
  assert.equal(c.state.catalog.sessions[0].hasUnread, true);
  c.child(c.reader(), 'markRead', {
    sessionId: 'Task B',
    position: { sourceSequence: 4, itemIndex: 0 },
    field: 'body',
  });
  assert.equal(c.action('markRead'), undefined);
  c.child(c.reader(), 'markRead', {
    sessionId: 'Task A',
    position: { sourceSequence: 99, itemIndex: 0 },
    field: 'body',
  });
  assert.equal(c.action('markRead'), undefined, 'an unexposed position cannot be acknowledged');
});

test('background readers, inspectors, dialogs and other pages disable read acknowledgements', () => {
  for (const change of [
    { inspector: 'outputs' },
    { dialog: 'palette' },
    { pages: ['settings'] },
    { image: { sessionId: 'Task A', through: 4, position: { sourceSequence: 4, itemIndex: 0 } } },
  ]) {
    const c = client()
      .observe(4)
      .history(page(4, [chunk(4, 'Answer')]));
    c.state.selected.hasUnread = true;
    Object.assign(c.state, change);
    c.raw('unhandled-test-key');
    const reader = c.find(c.reader());
    if (reader) assert.equal(reader.props.markReadEnabled, false);
  }
  const c = client()
    .observe(4)
    .history(page(4, [chunk(4, 'Answer')]));
  c.state.selected.hasUnread = true;
  const oldReader = c.reader();
  c.select('Task B')
    .observe(5)
    .history(page(5, [chunk(5, 'Another answer')]));
  assert.equal(c.find(oldReader).props.markReadEnabled, false);
  c.child(oldReader, 'markRead', {
    sessionId: 'Task A',
    position: { sourceSequence: 4, itemIndex: 0 },
    field: 'body',
  });
  assert.equal(c.action('markRead'), undefined);
});

test('structured WorkHub history reaches localized captions and pending queue labels', async () => {
  const targets = await Promise.all(
    ['tui'].map((target) => import(`../dist/packages/timeline/${target}.js`)),
  );
  for (const [locale, taskCaption, graphCaption, ordinaryCaption, nextCaption] of [
    ['en', 'WorkHub task', 'WorkHub graph', 'You', 'Next turn'],
    ['zh-CN', 'WorkHub 任务', 'WorkHub 图', '你', '下一轮'],
    ['zh-TW', 'WorkHub 任務', 'WorkHub 圖', '你', '下一輪'],
  ]) {
    const taskOrigin = { kind: 'workhub_task', taskId: 'wt_original_assignment' };
    const graphOrigin = { kind: 'workhub_graph', graphId: 'agent_graph_original' };
    const ordinaryId = `m_wh_${'a'.repeat(64)}`;
    const c = client(locale)
      .observe(3)
      .history(
        page(3, [
          chunk(1, 'Worker finished', { role: 'user', origin: taskOrigin }),
          chunk(2, 'Graph finished', { role: 'user', origin: graphOrigin }),
          chunk(3, 'WorkHub task wt_original_assignment has ended. Report its outcome.', {
            role: 'user',
            messageId: ordinaryId,
          }),
        ]),
      );
    const props = c.find(c.reader()).props;
    assert.deepEqual(
      props.messages.map((message) => message.origin),
      [taskOrigin, graphOrigin, undefined],
    );
    for (const timeline of targets) {
      const schema = timeline.activate().Timeline.contract.interface.props;
      const validate = new Ajv({ strict: false }).compile({
        ...schema.properties.messages,
        $defs: schema.$defs,
      });
      assert.equal(validate(props.messages), true, JSON.stringify(validate.errors));
      const output = timeline.component.mount(props, {});
      const tree = nodes(output.frame.view);
      assert.equal(
        tree.find((node) => node.key === 'role-message-1').text.split(' · ')[0],
        taskCaption,
      );
      assert.equal(
        tree.find((node) => node.key === 'role-message-2').text.split(' · ')[0],
        graphCaption,
      );
      // The ordinary message retains its normal caption; matching text/ID is not provenance.
      const ordinary = tree.find((node) => node.key === `role-${ordinaryId}`);
      if (ordinary) assert.equal(ordinary.text.split(' · ')[0], ordinaryCaption);
      else
        assert.equal(
          tree.find((node) => node.key === `message-${ordinaryId}`).label,
          ordinaryCaption,
        );
    }
    assert.ok(c.state.watch);
    c.event({
      kind: 'action',
      key: c.state.watch.key,
      event: {
        status: 'updated',
        value: {
          kind: 'queue',
          queue: {
            revision: 1,
            entries: [
              {
                messageId: 'task-pending',
                kind: 'followup',
                preview: 'Task update',
                origin: taskOrigin,
              },
              {
                messageId: 'graph-pending',
                kind: 'followup',
                preview: 'Graph update',
                origin: graphOrigin,
              },
              {
                messageId: ordinaryId,
                kind: 'followup',
                preview: 'WorkHub task update',
              },
            ],
          },
        },
      },
    });
    assert.deepEqual(
      c.find('queue').props.rows.map((row) => row.text),
      [
        `${taskCaption} · ${nextCaption} · Task update`,
        `${graphCaption} · ${nextCaption} · Graph update`,
        `${nextCaption} · WorkHub task update`,
      ],
    );
  }
});

test('WorkHub browses with opaque cursors and uses task detail metadata instead of list placeholders', async () => {
  for (const [target, locale, accepted] of [
    ['tui', 'en', 'Accepted'],
    ['tui', 'zh-CN', '受理时间'],
  ]) {
    const hub = await import(`../dist/packages/workhub-root/${target}.js`);
    hub.activate({ worker: 'worker' });
    const component = hub.component;
    let out = component.mount({}, values);
    const event = (input) => {
      out = component.event(out.frame.state, input, {}, values);
    };
    const raw = (key) => event({ kind: 'raw', key, event: { kind: 'activate' } });
    const reply = (method, value) => {
      const action = out.actions.find((action) => action.method === method);
      assert.ok(action, `Expected ${method} request`);
      event({ kind: 'action', key: action.key, event: { status: 'succeeded', value } });
    };
    const find = (key) => nodes(out.frame.view).find((node) => node.key === key);
    const scroll = (direction, readingRevision) =>
      event({
        kind: 'component',
        child: 'workhub-reading',
        name: 'observed',
        event: {
          readingRevision,
          viewport: { readingRevision, atStart: direction === 'up', atEnd: direction === 'down' },
          scroll: { readingRevision, direction },
          pendingCommand: null,
        },
      });
    reply('assistantPreferences', nativeLanguage(locale));
    // The independent history request survives the initial language read.
    const historyKey = out.frame.state.pending.key;
    event({
      kind: 'action',
      key: historyKey,
      event: { status: 'succeeded', value: [] },
    });
    reply('coordinator', { kind: 'missing' });
    raw('workhub-route-tasks');
    const taskId = 'wt_' + 'a'.repeat(64);
    const token = 'opaque:+/==?keep%2Fme';
    reply('tasks', {
      entries: [{ taskId, worker: 'worker', title: 'List placeholder', acceptedAt: 1000 }],
      after: token,
    });
    assert.equal(find('task-accepted-1-0').text, `${accepted}: 1970-01-01T00:00:01.000Z`);
    raw(nodes(out.frame.view).find((node) => node.label === 'List placeholder').key);
    assert.equal(find('task-title').text, 'List placeholder');
    const watch = out.subscriptions.find((subscription) => subscription.method === 'watchTask');
    const detail = {
      taskId,
      worker: 'worker',
      title: 'Canonical detail title',
      acceptedAt: 1500,
      operation: '00000000-0000-0000-0000-000000000001',
      status: { kind: 'pending' },
    };
    event({ kind: 'action', key: watch.key, event: { status: 'updated', value: detail } });
    assert.equal(find('task-title').text, 'Canonical detail title');
    assert.equal(find('task-accepted').text, `${accepted}: 1970-01-01T00:00:01.500Z`);
    raw('task-check');
    assert.deepEqual(out.actions.find((action) => action.method === 'task').input, {
      taskId,
      cursor: null,
    });
    reply('task', { ...detail, title: 'Detail loaded by ID', acceptedAt: 2000 });
    assert.equal(find('task-title').text, 'Detail loaded by ID');
    assert.equal(find('task-accepted').text, `${accepted}: 1970-01-01T00:00:02.000Z`);
    raw('task-back');
    scroll('down', 1);
    assert.deepEqual(out.actions.find((action) => action.method === 'tasks').input, {
      after: token,
    });
    reply('tasks', {
      entries: [
        {
          taskId: 'wt_' + 'c'.repeat(64),
          worker: 'worker',
          title: 'Second page',
          acceptedAt: 250,
        },
      ],
      after: null,
    });
    assert.equal(
      out.subscriptions.find((subscription) => subscription.method === 'watchTasks').input.after,
      token,
    );
    assert.equal(find('task-open-2-0').label, 'Second page');
    scroll('up', 2);
    assert.deepEqual(out.actions.find((action) => action.method === 'tasks').input, {
      after: null,
    });
  }
});

test('historical captions use recorded response models and keep requested and imported evidence distinct', async () => {
  const native = {
    kind: 'native',
    requestedModelId: 'request-alias',
    reportedModelId: 'response-model-2026',
  };
  const requested = { kind: 'native', requestedModelId: 'unreported-alias', reportedModelId: null };
  const imported = { kind: 'imported', modelId: 'source-recorded-model' };
  const targets = await Promise.all(
    ['tui'].map(async (target) => ({
      target,
      timeline: await import(`../dist/packages/timeline/${target}.js`),
    })),
  );
  for (const [locale, modelLabel, requestedLabel, importedLabel, thinkingLabel] of [
    ['en', 'Model', 'Requested model', 'Imported model', 'Thinking'],
    ['zh-CN', '模型', '请求模型', '导入来源模型', '思考'],
    ['zh-TW', '模型', '請求模型', '匯入來源模型', '思考'],
  ]) {
    const c = client(locale)
      .observe(5)
      .history(
        page(5, [
          chunk(1, 'Answer from reported model', { model: native }),
          chunk(2, 'Reasoning from reported model', { field: 'thinking', model: native }),
          chunk(3, 'Answer without reported model', { model: requested }),
          chunk(4, 'Imported answer', { model: imported }),
          chunk(5, 'Answer without recorded model'),
        ]),
      );
    c.child('models', 'resolved', {
      selectionKey: String(c.state.epoch),
      selection: {
        ...recapModel,
        choice: {
          ...recapModel.choice,
          selection: { ...recapModel.choice.selection, modelId: 'current-picker-model' },
        },
      },
    });
    const props = c.find(c.reader()).props;
    assert.deepEqual(
      props.messages.map((message) => message.model),
      [native, native, requested, imported, undefined],
    );
    for (const { target, timeline } of targets) {
      const tree = nodes(timeline.component.mount(props, {}).frame.view);
      for (const [id, expected] of [
        ['message-1', `Maka · ${modelLabel}: response-model-2026`],
        ['thinking-message-2', `${thinkingLabel} · ${modelLabel}: response-model-2026`],
        ['message-3', `Maka · ${requestedLabel}: unreported-alias`],
        ['message-4', `Maka · ${importedLabel}: source-recorded-model`],
      ]) {
        const caption = tree.find((node) => node.key === 'role-' + id).text;
        assert.equal(caption.split(' · ').slice(0, 2).join(' · '), expected);
      }
      assert.equal(tree.find((node) => node.key === 'message-message-5').label, 'Maka');
    }
  }
});

test('Diagnostics reads common failures and refreshes late background results without reloading the conversation', () => {
  const c = client()
    .observe(100)
    .history(page(100, [chunk(90, 'Completed main answer')]));
  c.raw('palette-open');
  const command = c
    .find('command-palette')
    .props.commands.find((entry) => entry.id === 'diagnostics-open');
  assert.ok(
    command && !command.disabled,
    'Diagnostics entry must be available after the main answer completes',
  );
  c.child('command-palette', 'invoked', { id: 'diagnostics-open' });
  assert.deepEqual(c.action('diagnostics').input, {
    sessionId: 'Task A',
    position: { kind: 'latest' },
  });
  const invocation = {
    session_id: 'Task A',
    turn_id: 'original-turn',
    run_id: 'original-run',
    invocation_id: 'original-invocation',
  };
  const main = {
    sequence: 90,
    eventId: 'main-failure',
    recordedAt: 1000,
    detail: {
      kind: 'execution',
      invocation,
      reason: 'execution_failed',
      modelFailure: { kind: 'http', status: 400 },
    },
  };
  const preparation = {
    sequence: 80,
    eventId: 'prepare-failure',
    recordedAt: 900,
    detail: {
      kind: 'execution',
      invocation,
      reason: 'message_preparation_failed',
      modelFailure: null,
    },
  };
  c.reply(c.action('diagnostics'), { through: 100, entries: [main, preparation], nextBefore: 80 });
  assert.equal(c.find('diagnostic-cause-90').text, 'HTTP 400');
  assert.equal(c.find('diagnostic-cause-80').text, 'Message preparation failed');
  const diagnosticWatch = c.out.subscriptions.find(
    (subscription) => subscription.method === 'watchDiagnostics',
  );
  assert.ok(diagnosticWatch, 'Diagnostics must subscribe independently of transcript progress');
  assert.deepEqual(diagnosticWatch.input, { sessionId: 'Task A' });
  c.raw('diagnostics-refresh');
  const olderRead = c.action('diagnostics');
  c.event({
    kind: 'action',
    key: diagnosticWatch.key,
    event: { status: 'updated', value: { revision: 120 } },
  });
  const latestRead = c.action('diagnostics');
  assert.notEqual(latestRead.key, olderRead.key);
  assert.equal(c.action('observe'), undefined);
  assert.equal(c.action('history'), undefined);
  const title = {
    sequence: 120,
    eventId: 'title-failure',
    recordedAt: 2000,
    detail: { kind: 'session_title', invocation, cause: { kind: 'empty_output' } },
  };
  c.reply(latestRead, { through: 120, entries: [title, main, preparation], nextBefore: 80 });
  assert.equal(c.find('diagnostic-cause-120').text, 'Empty output');
  assert.equal(c.state.run, null);
  assert.deepEqual(
    c.find(c.reader()).props.messages.map((message) => message.text),
    ['Completed main answer'],
  );
  assert.equal(c.action('observe'), undefined);
  assert.equal(c.action('history'), undefined);
  c.reply(olderRead, { through: 100, entries: [], nextBefore: null });
  assert.equal(
    c.find('diagnostic-cause-120').text,
    'Empty output',
    'An older empty query cannot clear the newer diagnostic page',
  );
  c.raw('diagnostic-open-120');
  assert.equal(c.find('diagnostic-detail-cause').text, 'Empty output');
  assert.equal(c.find('diagnostic-detail-source').text, 'Automatic title');
  c.raw('diagnostics-back').raw('diagnostics-older');
  assert.deepEqual(c.action('diagnostics').input, {
    sessionId: 'Task A',
    position: { kind: 'before', through: 120, sequence: 80 },
  });
  c.reply(c.action('diagnostics'), {
    through: 120,
    entries: [
      {
        sequence: 70,
        eventId: 'step-failure',
        recordedAt: 800,
        detail: {
          kind: 'model_step',
          invocation,
          stepId: 'retryable-step',
          status: 'retryable_failure',
          modelFailure: { kind: 'provider', reason: 'network' },
        },
      },
    ],
    nextBefore: null,
  });
  assert.equal(c.find('diagnostic-cause-70').text, 'Network failure');
  c.select('Task B')
    .observe(130)
    .history(page(130, [chunk(130, 'Another conversation')]));
  assert.equal(c.find('diagnostic-cause-120'), undefined);
  assert.deepEqual(
    c.find(c.reader()).props.messages.map((message) => message.text),
    ['Another conversation'],
  );
});

test('Host logs open Diagnostics without a selected conversation through their own capability', () => {
  const c = client('en', { configuration: { hostLogs: true }, sessions: [] });
  assert.equal(c.state.selected, null);
  c.raw('palette-open');
  const command = c
    .find('command-palette')
    .props.commands.find((entry) => entry.id === 'diagnostics-open');
  assert.ok(
    command && !command.disabled,
    'Diagnostics must remain available for Host logs without a selected conversation',
  );
  c.child('command-palette', 'invoked', { id: 'diagnostics-open' });
  const read = c.action('read');
  assert.ok(read, 'Opening Host diagnostics must query the fixed Host log source');
  assert.equal(read.binding, 'hostLogs');
  assert.deepEqual(read.input, { position: { kind: 'tail' } });
  assert.equal(c.action('diagnostics'), undefined);
  assert.equal(c.action('observe'), undefined);
  assert.equal(c.action('history'), undefined);
  c.reply(read, {
    kind: 'data',
    cursor: 'opaque-host-cursor:+/==',
    bytes: 'SG9zdCBzdGFydGVkCg==',
    reset: null,
    truncated: false,
    hasMore: false,
  });
  assert.equal(c.find('host-log-text').props.source, 'Host started\n');
});

function openDiagnostics(c) {
  c.raw('palette-open');
  c.child('command-palette', 'invoked', { id: 'diagnostics-open' });
  return c;
}
function hostPage(cursor, bytes, extra = {}) {
  return { kind: 'data', cursor, bytes, reset: null, truncated: false, hasMore: false, ...extra };
}
function hostUpdate(c, watch, value) {
  c.event({ kind: 'action', key: watch.key, event: { status: 'updated', value } });
}
test('Host logs render literal split UTF-8 in both presentations and stop their independent follow', () => {
  for (const target of ['tui']) {
    const c = openDiagnostics(
      client('en', {
        target,
        configuration: { hostLogs: true },
        sessions: [],
      }),
    );
    // The first byte of 中 is deliberately separated from the remaining bytes.
    c.reply(c.action('read'), hostPage('opaque:+/=first', '5A=='));
    assert.equal(c.find('host-log-text').props.source, '');
    c.raw('host-log-follow');
    const watch = c.out.subscriptions.find((entry) => entry.binding === 'hostLogs');
    assert.ok(watch);
    assert.equal(watch.method, 'watch');
    assert.deepEqual(watch.input, { position: { kind: 'after', cursor: 'opaque:+/=first' } });
    hostUpdate(c, watch, hostPage('opaque:+/=second', 'uK0='));
    assert.equal(c.find('host-log-text').props.source, '中');
    const raw = '\x1b[31m<command>🙂\n';
    hostUpdate(c, watch, hostPage('opaque:+/=third', Buffer.from(raw).toString('base64')));
    assert.equal(c.find('host-log-text').props.source, '中' + raw);
    assert.deepEqual(
      c.out.subscriptions.find((entry) => entry.binding === 'hostLogs'),
      watch,
    );
    assert.equal(
      c.out.actions.some((entry) => entry.binding === 'terminals'),
      false,
    );
    c.raw('host-log-stop');
    assert.equal(
      c.out.subscriptions.some((entry) => entry.binding === 'hostLogs'),
      false,
    );
    hostUpdate(c, watch, hostPage('late', 'bGF0ZQ=='));
    assert.equal(c.find('host-log-text').props.source, '中' + raw);
    c.raw('host-log-follow');
    assert.deepEqual(c.out.subscriptions.find((entry) => entry.binding === 'hostLogs').input, {
      position: { kind: 'after', cursor: 'opaque:+/=third' },
    });
    c.raw('diagnostics-close');
    assert.equal(
      c.out.subscriptions.some((entry) => entry.binding === 'hostLogs'),
      false,
    );
  }
});
test('Host logs retain a bounded tail, clear replaced bytes, and distinguish missing from empty capture', () => {
  const c = openDiagnostics(client('zh-CN', { configuration: { hostLogs: true }, sessions: [] }));
  c.reply(c.action('read'), { kind: 'missing' });
  assert.equal(c.find('host-log-missing').text, '当前没有 Host 日志捕获文件。');
  c.raw('host-log-follow');
  const watch = c.out.subscriptions.find((entry) => entry.binding === 'hostLogs');
  hostUpdate(c, watch, hostPage('empty', ''));
  assert.equal(c.find('host-log-missing'), undefined);
  const block = 'a'.repeat(32 * 1024);
  for (const cursor of ['one', 'two', 'three'])
    hostUpdate(c, watch, hostPage(cursor, Buffer.from(block).toString('base64')));
  assert.equal(c.find('host-log-text').props.source, block + block);
  assert.equal(c.find('host-log-omitted').text, '已省略较早的日志。');
  hostUpdate(c, watch, hostPage('new-file', 'bmV3Cg==', { reset: 'replaced' }));
  assert.equal(c.find('host-log-text').props.source, 'new\n');
  assert.equal(c.find('host-log-omitted'), undefined);
  assert.equal(c.find('host-log-reset').text, '日志文件已替换，正在显示新文件。');
  hostUpdate(c, watch, hostPage('shrunk', 'dGFpbA==', { reset: 'truncated', truncated: true }));
  assert.equal(c.find('host-log-text').props.source, 'tail');
  assert.equal(c.find('host-log-omitted').text, '已省略较早的日志。');
  assert.equal(c.find('host-log-reset').text, '日志文件已截断，正在显示当前末尾。');
});
test('Host log source survives session changes and stale closed reads cannot replace its new page', () => {
  const c = openDiagnostics(
    client('en', { configuration: { hostLogs: true } })
      .observe(10)
      .history(page(10, [])),
  );
  c.raw('diagnostics-source-host');
  c.reply(c.action('read'), hostPage('kept-cursor', 'a2VwdA=='));
  c.raw('host-log-follow');
  const watch = c.out.subscriptions.find((entry) => entry.binding === 'hostLogs');
  const transcript = c.out.subscriptions.find(
    (entry) => entry.binding === 'agent' && entry.method === 'watch',
  );
  c.select('Task B');
  assert.equal(c.find('host-log-text').props.source, 'kept');
  assert.deepEqual(
    c.out.subscriptions.find((entry) => entry.binding === 'hostLogs'),
    watch,
  );
  c.raw('diagnostics-source-conversation');
  assert.equal(
    c.out.subscriptions.some((entry) => entry.binding === 'hostLogs'),
    false,
  );
  assert.deepEqual(c.action('diagnostics').input, {
    sessionId: 'Task B',
    position: { kind: 'latest' },
  });
  assert.ok(transcript, 'Host follow coexists with the transcript subscription');
  c.raw('diagnostics-source-host');
  const oldRead = c.action('read');
  c.raw('diagnostics-close');
  openDiagnostics(c);
  const newRead = c.action('read');
  assert.notEqual(oldRead.key, newRead.key);
  c.reply(newRead, hostPage('fresh', 'ZnJlc2g='));
  c.reply(oldRead, hostPage('stale', 'c3RhbGU='));
  assert.equal(c.find('host-log-text').props.source, 'fresh');
});
test('Omitting the Host logs feature creates no optional capability demands', () => {
  const c = client('en', { sessions: [] });
  c.raw('palette-open');
  assert.equal(
    c.find('command-palette').props.commands.find((entry) => entry.id === 'diagnostics-open')
      .disabled,
    true,
  );
  assert.equal(
    c.out.actions.some((entry) => entry.binding === 'hostLogs'),
    false,
  );
  assert.equal(
    c.out.subscriptions.some((entry) => entry.binding === 'hostLogs'),
    false,
  );
  assert.equal(c.find('host-log-text'), undefined);
  const selected = openDiagnostics(client().observe(10).history(page(10, [])));
  assert.ok(selected.action('diagnostics'));
  assert.equal(selected.find('diagnostics-source-host'), undefined);
  assert.equal(selected.find('host-log-text'), undefined);
});

test('Host log follow preserves its visible suffix through missing and same-file return while tail refresh starts over', () => {
  const c = openDiagnostics(client('en', { configuration: { hostLogs: true }, sessions: [] }));
  c.reply(
    c.action('read'),
    hostPage('same-file-cursor', 'a2VwdA==', { truncated: true, reset: 'replaced' }),
  );
  c.raw('host-log-follow');
  let watch = c.out.subscriptions.find((entry) => entry.binding === 'hostLogs');
  hostUpdate(c, watch, { kind: 'missing' });
  assert.equal(c.find('host-log-text').props.source, 'kept');
  assert.equal(c.find('host-log-missing').text, 'Host log capture file is currently missing.');
  assert.equal(c.find('host-log-omitted').text, 'Earlier log bytes omitted.');
  assert.equal(c.find('host-log-reset').text, 'Log file replaced; showing the new file.');
  c.raw('host-log-stop');
  c.raw('host-log-follow');
  watch = c.out.subscriptions.find((entry) => entry.binding === 'hostLogs');
  assert.deepEqual(watch.input, { position: { kind: 'after', cursor: 'same-file-cursor' } });
  hostUpdate(c, watch, hostPage('same-file-cursor', ''));
  assert.equal(c.find('host-log-missing'), undefined);
  assert.equal(c.find('host-log-text').props.source, 'kept');
  hostUpdate(c, watch, { kind: 'missing' });
  hostUpdate(c, watch, hostPage('same-file-next-cursor', 'LXN1ZmZpeA=='));
  assert.equal(c.find('host-log-text').props.source, 'kept-suffix');
  assert.equal(c.find('host-log-missing'), undefined);
  assert.equal(c.find('host-log-omitted').text, 'Earlier log bytes omitted.');
  c.raw('host-log-refresh');
  c.reply(c.action('read'), { kind: 'missing' });
  assert.equal(c.find('host-log-text').props.source, '');
  assert.equal(c.find('host-log-omitted'), undefined);
  assert.equal(c.find('host-log-reset'), undefined);
  c.raw('host-log-follow');
  assert.deepEqual(c.out.subscriptions.find((entry) => entry.binding === 'hostLogs').input, {
    position: { kind: 'tail' },
  });
});

test('Host log invalid UTF-8 replacements stay within the rendered text budget and retain the newest suffix', () => {
  const c = openDiagnostics(client('en', { configuration: { hostLogs: true }, sessions: [] }));
  c.reply(
    c.action('read'),
    hostPage('binary-tail', Buffer.alloc(32 * 1024, 0xff).toString('base64')),
  );
  let displayed = c.find('host-log-text').props.source;
  assert.ok(
    Buffer.byteLength(displayed) <= 65536,
    'Decoded log text must fit the SourceText UTF-8 budget',
  );
  assert.ok(displayed.includes('\ufffd'));
  assert.equal(c.find('host-log-omitted').text, 'Earlier log bytes omitted.');
  c.raw('host-log-follow');
  const watch = c.out.subscriptions.find((entry) => entry.binding === 'hostLogs');
  hostUpdate(c, watch, hostPage('text-suffix', 'dGFpbAo='));
  displayed = c.find('host-log-text').props.source;
  assert.ok(Buffer.byteLength(displayed) <= 65536);
  assert.ok(displayed.endsWith('tail\n'));
  assert.ok(displayed.includes('\ufffd'));
});
