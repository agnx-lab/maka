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
const { component, activate: timelineExports } = await import('../dist/packages/timeline/tui.js');

test('history fragments keep distinct controls and navigate the original tool evidence', () => {
  const tool = {
    toolUseId: 'computer-call',
    call: {
      position: { sourceSequence: 7, itemIndex: 0 },
      name: 'cua_repl',
      title: null,
      presentation: null,
      pending: false,
      ancestorToolUseIds: [],
      dispatched: true,
    },
    result: null,
  };
  const props = {
    theme,
    locale: 'en',
    copyRequest: null,
    browse: null,
    timings: [],
    markReadEnabled: true,
    toolContext: { sessionId: 'session', through: 8 },
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
    messages: [
      'call',
      'call-offset-64',
      'answer',
      'answer-offset-64',
      ...Array.from({ length: 5 }, (_, index) => 'later-answer-' + index),
    ].map((id, index) => ({
      id,
      contentKey: id,
      role: index < 2 ? 'tool_call' : 'assistant',
      text: 'Original fragment ' + index,
      timestamp: null,
      source: { sourceSequence: index < 2 ? 7 : 8, itemIndex: 0 },
      image: false,
      continued: true,
      tool: index < 2 ? tool : null,
    })),
  };
  props.timings = props.messages.map((message, index) => ({
    turnId: 'turn-' + index,
    afterMessageId: message.id,
    startedAt: index,
    finished: { at: index + 1, outcome: 'completed' },
  }));
  const schema = timelineExports().Timeline.contract.interface.props;
  const validTimings = new Ajv({ strict: false }).compile({
    ...schema.properties.timings,
    $defs: schema.$defs,
  });
  assert.equal(validTimings(props.timings), true, JSON.stringify(validTimings.errors));
  let out = component.mount(props, {});
  const raw = (key) => {
    out = component.event(
      out.frame.state,
      { kind: 'raw', key, event: { kind: 'activate' } },
      props,
      {},
    );
  };
  raw('toggle-call');
  raw('toggle-call-offset-64');
  const keys = [];
  const visit = (value) => {
    if (!value || typeof value !== 'object') return;
    if (typeof value.key === 'string') keys.push(value.key);
    for (const child of Object.values(value)) {
      if (Array.isArray(child)) child.forEach(visit);
      else visit(child);
    }
  };
  visit(out.frame.view);
  assert.equal(keys.filter((key) => key.startsWith('time-')).length, 9);
  assert.equal(
    new Set(keys).size,
    keys.length,
    'all rendered fragments must have unique control identities',
  );
  raw('tool-source-call-offset-64-arguments');
  assert.deepEqual(out.events, [
    {
      name: 'toolSource',
      value: { position: { sourceSequence: 7, itemIndex: 0 }, field: 'arguments' },
    },
  ]);
  raw('source-answer-offset-64');
  assert.deepEqual(out.events, [
    { name: 'source', value: { position: { sourceSequence: 8, itemIndex: 0 }, field: 'body' } },
  ]);
  raw('mark-read-answer-offset-64');
  assert.deepEqual(out.events, [
    {
      name: 'markRead',
      value: { sessionId: 'session', position: { sourceSequence: 8, itemIndex: 0 }, field: 'body' },
    },
  ]);
  const find = (node, key) => {
    if (!node || typeof node !== 'object') return undefined;
    if (node.key === key) return node;
    for (const value of Object.values(node)) {
      for (const child of Array.isArray(value) ? value : [value]) {
        const found = find(child, key);
        if (found) return found;
      }
    }
  };
  for (const [settlement, isError, hint, tone] of [
    ['returned', false, 'Returned', 'success'],
    ['returned', true, 'Tool reported an error', 'error'],
    ['failed', false, 'Tool execution failed', 'error'],
    ['rejected', false, 'Not executed', 'error'],
    ['unknown', false, 'Tool outcome is unknown', 'warning'],
  ]) {
    props.messages[0].tool = {
      ...tool,
      result: { position: { sourceSequence: 8, itemIndex: 0 }, isError, settlement, image: false },
    };
    out = component.update(out.frame.state, props, {});
    const marker = find(out.frame.view, 'marker-call');
    assert.equal(marker.hint, hint);
    assert.equal(marker.color, theme.terminal[tone]);
    assert.equal(find(out.frame.view, 'tool-status-call'), undefined);
  }
  const title = {
    fallback: 'Read skill · demo',
    translations: { 'zh-CN': '读取技能 · demo', 'zh-TW': '讀取技能 · demo' },
  };
  props.messages[0].tool.call.title = title;
  for (const locale of ['en', 'zh-CN', 'zh-TW']) {
    props.locale = locale;
    out = component.update(out.frame.state, props, {});
    assert.equal(
      find(out.frame.view, 'toggle-call').label,
      '⌄ ' + (title.translations[locale] ?? title.fallback),
    );
  }
});

test('history prefetches before an edge without treating stale or commanded viewports as scrolling', () => {
  const props = {
    theme,
    locale: 'en',
    copyRequest: null,
    browse: null,
    timings: [],
    markReadEnabled: false,
    toolContext: null,
    waitingTools: [],
    liveTools: [],
    messages: Array.from({ length: 8 }, (_, i) => ({
      id: String(i),
      contentKey: String(i),
      role: 'assistant',
      text: 'Message ' + i,
      timestamp: null,
      source: { sourceSequence: i + 1, itemIndex: 0 },
      image: false,
      continued: false,
      tool: null,
    })),
    source: null,
    search: null,
    loading: false,
    older: true,
    newer: true,
    latest: false,
    landing: { sequence: 0, edge: 'end' },
    live: true,
  };
  let out = component.mount(props, {});
  const observed = (revision, direction, visibleChildKeys, extra = {}) => {
    out = component.event(
      out.frame.state,
      {
        kind: 'component',
        child: 'reading',
        name: 'observed',
        event: {
          readingRevision: revision,
          scroll: { readingRevision: revision, direction },
          pendingCommand: null,
          viewport: { readingRevision: revision, atStart: false, atEnd: false, visibleChildKeys },
          ...extra,
        },
      },
      props,
      {},
    );
    return out.events.filter((event) => event.name === 'older' || event.name === 'newer');
  };
  assert.deepEqual(observed(1, 'up', ['message-3', 'message-4']), []);
  assert.deepEqual(observed(2, 'up', ['message-1', 'message-2']), [{ name: 'older', value: {} }]);
  assert.deepEqual(observed(2, 'up', ['message-1']), [], 'one request for each original scroll');
  assert.deepEqual(observed(3, 'down', ['message-5', 'message-6']), [{ name: 'newer', value: {} }]);
  assert.deepEqual(
    observed(4, 'up', [], {
      viewport: {
        readingRevision: 3,
        atStart: true,
        atEnd: false,
        visibleChildKeys: ['message-0'],
      },
    }),
    [],
    'wait for the viewport actually produced by the scroll',
  );
  assert.deepEqual(observed(4, 'up', ['message-0'], { pendingCommand: 7 }), []);
  assert.deepEqual(observed(4, 'up', ['message-0']), [{ name: 'older', value: {} }]);
  props.messages = props.messages.slice(0, 2);
  assert.deepEqual(
    observed(5, 'up', ['message-1']),
    [],
    'do not evict the visible tail of a two-part message',
  );
  assert.deepEqual(observed(6, 'up', ['message-0']), [{ name: 'older', value: {} }]);
  assert.deepEqual(observed(7, 'down', ['message-0']), []);
});

test('floating output is one truncated row only away from the live end', () => {
  const props = {
    theme,
    locale: 'en',
    copyRequest: null,
    browse: null,
    timings: [],
    markReadEnabled: false,
    toolContext: null,
    waitingTools: [],
    liveTools: [],
    messages: [],
    source: null,
    search: null,
    loading: false,
    older: false,
    newer: false,
    latest: false,
    landing: { sequence: 0, edge: 'end' },
    live: true,
    activeOutput: {
      id: 'live',
      contentKey: 'live',
      role: 'thinking',
      text: 'First line\nSecond line',
      timestamp: null,
      source: null,
      image: false,
      continued: false,
      tool: null,
    },
  };
  let out = component.mount(props, {});
  const nodes = (n) =>
    n && typeof n === 'object'
      ? [n, ...Object.values(n).flatMap((v) => (Array.isArray(v) ? v.flatMap(nodes) : nodes(v)))]
      : [];
  const find = (key) => nodes(out.frame.view).find((n) => n.key === key);
  assert.equal(find('active-output'), undefined);
  const observe = (revision, atEnd) => {
    out = component.event(
      out.frame.state,
      {
        kind: 'component',
        child: 'reading',
        name: 'observed',
        event: {
          readingRevision: revision,
          pendingCommand: null,
          viewport: { readingRevision: revision, atStart: false, atEnd, visibleChildKeys: [] },
        },
      },
      props,
      {},
    );
  };
  observe(1, false);
  assert.equal(find('active-output-text').text, 'Thinking · First line Second line');
  assert.equal(find('active-output-text').truncate, true);
  assert.ok(find('reading-control-space'));
  assert.ok(find('reading-control-space-end'));
  assert.deepEqual(out.events.find((e) => e.name === 'detached')?.value, {
    landing: 0,
    value: true,
  });
  observe(2, true);
  assert.equal(find('active-output'), undefined);
  observe(3, false);
  props.activeOutput = null;
  out = component.update(out.frame.state, props, {});
  assert.equal(find('active-output'), undefined);
});

test('prompt header follows the visible answer, yields to user rows, and never flickers at a boundary', () => {
  const message = (id, role, text) => ({
    id,
    contentKey: id,
    role,
    text,
    source: { sourceSequence: Number(id), itemIndex: 0 },
    timestamp: 1,
    tool: null,
    image: false,
    continued: false,
  });
  const props = {
    theme,
    locale: 'en',
    copyRequest: null,
    browse: null,
    timings: [],
    markReadEnabled: false,
    toolContext: null,
    waitingTools: [],
    liveTools: [],
    source: null,
    search: null,
    loading: false,
    older: false,
    newer: false,
    latest: false,
    live: false,
    landing: { sequence: 0, edge: 'start' },
    messages: [
      message('1', 'user', 'First question'),
      message('2', 'assistant', 'First answer'),
      message('3', 'user', '?'),
      message('4', 'assistant', 'Latest answer'),
    ],
  };
  let out = component.mount(props, {});
  const nodes = (n) =>
    n && typeof n === 'object'
      ? [n, ...Object.values(n).flatMap((v) => (Array.isArray(v) ? v.flatMap(nodes) : nodes(v)))]
      : [];
  const find = (key) => nodes(out.frame.view).find((n) => n.key === key);
  const observe = (revision, first) => {
    out = component.event(
      out.frame.state,
      {
        kind: 'component',
        child: 'reading',
        name: 'observed',
        event: {
          readingRevision: revision,
          pendingCommand: null,
          viewport: {
            readingRevision: revision,
            atStart: false,
            atEnd: false,
            visibleChildKeys: ['message-' + first],
          },
        },
      },
      props,
      {},
    );
  };
  const events = timelineExports().Timeline.contract.interface.events;
  const ajv = new Ajv({ strict: false });
  const validateEvents = () => {
    for (const event of out.events) {
      const valid = ajv.compile(events[event.name]);
      assert.ok(valid(event.value), JSON.stringify(valid.errors));
    }
  };
  assert.equal(find('prompt-bar'), undefined);
  observe(1, '2');
  assert.equal(find('prompt-text').label, '❯ First question');
  out = component.event(out.frame.state, { kind: 'raw', key: 'prompt-text', event: {} }, props, {});
  assert.deepEqual(out.frame.state.command.target, { kind: 'reveal_child', key: 'message-1' });
  assert.ok(out.events.some((event) => event.name === 'detached'));
  validateEvents();
  observe(2, '3');
  assert.equal(find('prompt-bar'), undefined);
  observe(2, '2');
  assert.equal(find('prompt-bar'), undefined, 'layout feedback cannot reinsert the canceled row');
  observe(3, '4');
  assert.equal(
    find('prompt-text').label,
    '❯ ?',
    'short prompts remain valid context for their own answer',
  );
  observe(4, '1');
  assert.equal(find('prompt-bar'), undefined);
  props.messages = [message('4', 'assistant', 'Latest answer')];
  props.leadingPrompt = {
    position: { sourceSequence: 3, itemIndex: 0 },
    text: 'A question outside the retained page',
  };
  out = component.update(out.frame.state, props, {});
  observe(5, '4');
  assert.equal(find('prompt-text').label, '❯ A question outside the retained page');
  out = component.event(out.frame.state, { kind: 'raw', key: 'prompt-text', event: {} }, props, {});
  assert.ok(
    out.events.some(
      (event) =>
        event.name === 'prompt' &&
        event.value.position.sourceSequence === 3 &&
        event.value.position.itemIndex === 0,
    ),
  );
  validateEvents();
  assert.deepEqual(
    find('reading').props.content.children.map((n) => n.key),
    ['message-4'],
    'header is never copied or searched as transcript text',
  );
});

for (const target of ['tui']) {
  test(`${target} pending user message uses muted text and normalizes on admission`, async () => {
    const { component } = await import(`../dist/packages/timeline/${target}.js`);
    const props = {
      theme,
      locale: 'en',
      copyRequest: null,
      browse: null,
      timings: [],
      markReadEnabled: false,
      toolContext: null,
      waitingTools: [],
      liveTools: [],
      source: null,
      search: null,
      loading: false,
      older: false,
      newer: false,
      latest: false,
      landing: { sequence: 0, edge: 'end' },
      live: true,
      messages: [
        {
          id: 'submission-send-1',
          contentKey: 'submission-send-1',
          role: 'user',
          text: 'Hello',
          timestamp: null,
          source: null,
          image: false,
          continued: false,
          tool: null,
          pending: true,
        },
      ],
    };
    const nodes = (node) =>
      node && typeof node === 'object'
        ? [
            node,
            ...Object.values(node).flatMap((value) =>
              Array.isArray(value) ? value.flatMap(nodes) : nodes(value),
            ),
          ]
        : [];
    let out = component.mount(props, {});
    const source = () =>
      nodes(out.frame.view).find((node) => node.key === 'text-submission-send-1');
    assert.equal(source().props.appearance.foreground, theme.muted);
    assert.equal(source().props.terminal.foreground, theme.terminal.muted);
    props.messages[0].pending = false;
    out = component.update(out.frame.state, props, {});
    assert.equal(source().props.appearance.foreground, theme.markdown.body.foreground);
    assert.equal(source().props.terminal.foreground, theme.terminal.foreground);
  });
}

test('trace keeps visible ancestry and keyboard disclosure across bounded history windows', () => {
  const message = (id, sequence, ancestors = []) => ({
    id,
    contentKey: id,
    role: 'tool_call',
    text: '{}',
    timestamp: null,
    source: { sourceSequence: sequence, itemIndex: 0 },
    image: false,
    continued: false,
    tool: {
      toolUseId: id,
      call: {
        position: { sourceSequence: sequence, itemIndex: 0 },
        name: id,
        title: { fallback: id },
        presentation: null,
        pending: false,
        dispatched: true,
        ancestorToolUseIds: ancestors,
      },
      result: {
        position: { sourceSequence: sequence + 10, itemIndex: 0 },
        isError: false,
        settlement: 'returned',
        image: false,
      },
    },
  });
  const props = {
    theme,
    locale: 'en',
    copyRequest: null,
    browse: 1,
    timings: [],
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
    messages: [
      message('run', 1),
      message('shell', 3, ['hidden-cell', 'run']),
      message('nested', 4, ['shell', 'hidden-cell', 'run']),
    ],
  };
  let out = component.mount(props, {});
  const nodes = (node) =>
    node && typeof node === 'object'
      ? [
          node,
          ...Object.values(node).flatMap((value) =>
            Array.isArray(value) ? value.flatMap(nodes) : nodes(value),
          ),
        ]
      : [];
  const find = (key) => nodes(out.frame.view).find((node) => node.key === key);
  const raw = (key, kind = 'activate') => {
    out = component.event(out.frame.state, { kind: 'raw', key, event: { kind } }, props, {});
  };
  assert.equal(find('marker-run').text, '·');
  assert.equal(find('indent-run').layout.basis, 2);
  assert.equal(find('marker-shell').text, '·');
  assert.equal(find('indent-shell').layout.basis, 4);
  assert.equal(find('marker-nested').text, '·');
  assert.equal(find('indent-nested').layout.basis, 6);
  raw('browse-next');
  assert.deepEqual(out.frame.state.command.target, { kind: 'focus_item', key: 'browse-shell' });
  assert.equal(find('toggle-shell').label, '› shell');
  raw('browse-shell', 'submit');
  assert.equal(find('toggle-shell').label, '⌄ shell');
  raw('browse-collapse');
  assert.equal(find('toggle-shell').label, '› shell');
  raw('browse-expand');
  assert.equal(find('toggle-shell').label, '⌄ shell');
  raw('browse-next');
  raw('browse-previous');
  assert.deepEqual(out.frame.state.command.target, { kind: 'focus_item', key: 'browse-shell' });
  raw('browse-close');
  assert.ok(out.events.some((event) => event.name === 'browseClosed' && event.value.request === 1));
  props.messages = props.messages.slice(1);
  out = component.update(out.frame.state, props, {});
  assert.equal(find('marker-shell').text, '·');
  assert.equal(find('indent-shell').layout.basis, 2);
  assert.equal(find('marker-nested').text, '·');
  assert.equal(find('indent-nested').layout.basis, 4);
  assert.equal(find('toggle-shell').label, '⌄ shell');
});

test('user-to-response spacing is one plain row above the response, not part of the user band', () => {
  const message = (role, index) => ({
    id: `${role}-${index}`,
    contentKey: `${role}-${index}`,
    role,
    text: 'Message',
    timestamp: null,
    source: { sourceSequence: index + 1, itemIndex: 0 },
    image: false,
    continued: false,
    tool:
      role === 'tool_call'
        ? {
            toolUseId: `${role}-${index}`,
            call: {
              position: { sourceSequence: index + 1, itemIndex: 0 },
              name: 'Read',
              title: null,
              presentation: null,
              pending: false,
              dispatched: true,
              ancestorToolUseIds: [],
            },
            result: null,
          }
        : null,
  });
  for (const roles of [
    ['user', 'assistant'],
    ['user', 'thinking', 'assistant'],
    ['user', 'tool_call', 'tool_call', 'assistant'],
    ['user', 'user', 'assistant'],
  ]) {
    const props = {
      theme,
      locale: 'en',
      copyRequest: null,
      browse: null,
      timings: [],
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
      messages: roles.map(message),
    };
    const out = component.mount(props, {});
    const paths = [];
    const visit = (node, ancestors = []) => {
      if (!node || typeof node !== 'object') return;
      const path = [...ancestors, node];
      if (node.key) paths.push(path);
      for (const child of Object.values(node)) {
        if (Array.isArray(child)) child.forEach((value) => visit(value, path));
        else visit(child, path);
      }
    };
    visit(out.frame.view);
    const find = (key) => paths.find((path) => path.at(-1).key === key);
    const responseIndex = roles.findIndex((role) => role !== 'user');
    const responseId = props.messages[responseIndex].id;
    const userId = props.messages[responseIndex - 1].id;
    const userCard = find('message-card-' + userId).at(-1);
    const responseCard = (find('message-card-' + responseId) ?? find('message-' + responseId)).at(
      -1,
    );
    const gap = find('message-space-before-' + responseId);
    assert.ok(gap, `missing response gap: ${roles}`);
    const response = find('message-' + responseId).at(-1);
    assert.equal(response.children[0], gap.at(-1));
    const content = response.children[1];
    assert.equal(content.kind, 'context_menu');
    assert.equal(content.background, responseCard);
    assert.ok(!gap.includes(content), 'the gap must stay outside the menu trigger rectangle');
    assert.ok(
      !responseCard.children[0].key.startsWith('message-space-'),
      'the menu trigger must align with the first content row',
    );
    const keys = paths.map((path) => path.at(-1).key);
    assert.equal(new Set(keys).size, keys.length, 'wrapping must retain unique scene keys');
    assert.equal(gap.at(-1).layout.basis, 1);
    assert.ok(!find('message-space-' + userId), 'the previous user must not retain another gap');
    assert.notEqual(userCard.background, undefined, 'the user body retains its colored band');
    assert.ok(
      gap.every((node) => node.background === undefined || node.background !== userCard.background),
      'the response gap must not inherit the user background',
    );
    assert.equal(
      find('band-bottom-' + userId).at(-1).layout.basis,
      1,
      'intrinsic user padding remains unchanged',
    );
    const separators = paths.filter((path) => path.at(-1).key.startsWith('message-space-'));
    const expected = roles
      .slice(1)
      .filter((role, index) => !(role === 'tool_call' && roles[index] === 'tool_call')).length;
    assert.equal(
      separators.reduce((rows, path) => rows + path.at(-1).layout.basis, 0),
      expected,
      'moving the gap must not add rows or spread adjacent tool calls apart',
    );
  }
});

test('only an observed latest tail acknowledges the displayed message', () => {
  const props = {
    theme,
    locale: 'en',
    copyRequest: null,
    browse: null,
    timings: [],
    markReadEnabled: true,
    toolContext: { sessionId: 'session', through: 8 },
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
    messages: [
      {
        id: 'answer',
        contentKey: 'answer',
        role: 'assistant',
        text: 'Answer',
        timestamp: null,
        source: { sourceSequence: 8, itemIndex: 0 },
        image: false,
        continued: false,
        tool: null,
      },
    ],
  };
  const observation = {
    readingRevision: 1,
    pendingCommand: null,
    viewport: {
      readingRevision: 1,
      atStart: false,
      atEnd: true,
      visibleChildKeys: ['message-answer'],
    },
  };
  const read = (changes = {}, observed = observation) => {
    const current = { ...props, ...changes };
    const mounted = component.mount(current, {});
    assert.equal(
      mounted.events.some((event) => event.name === 'markRead'),
      false,
    );
    const out = component.event(
      mounted.frame.state,
      {
        kind: 'component',
        child: 'reading',
        name: 'observed',
        event: observed,
      },
      current,
      {},
    );
    return out.events.find((event) => event.name === 'markRead');
  };
  assert.deepEqual(read(), {
    name: 'markRead',
    value: { sessionId: 'session', position: { sourceSequence: 8, itemIndex: 0 }, field: 'body' },
  });
  assert.ok(
    read({ messages: [{ ...props.messages[0], continued: true }] }),
    'the visible final fragment of a long message still acknowledges its original identity',
  );
  for (const changes of [
    { markReadEnabled: false },
    { loading: true },
    { latest: true },
    { newer: true },
    { browse: 1 },
    { toolContext: null },
    { messages: [{ ...props.messages[0], source: null }] },
    { messages: [{ ...props.messages[0], role: 'thinking' }] },
  ])
    assert.equal(read(changes), undefined, JSON.stringify(changes));
  assert.equal(read({}, { ...observation, pendingCommand: 1 }), undefined);
  assert.equal(
    read({
      messages: [
        ...props.messages,
        { ...props.messages[0], id: 'new-answer', source: { sourceSequence: 9, itemIndex: 0 } },
      ],
    }),
    undefined,
    'a queued observation of the previous tail cannot acknowledge a newly appended message',
  );
  assert.equal(
    read({}, { ...observation, viewport: { ...observation.viewport, visibleChildKeys: [] } }),
    undefined,
    'being at the end is insufficient without the actual tail in the visible children',
  );
  assert.equal(
    read({}, { ...observation, viewport: { ...observation.viewport, atEnd: false } }),
    undefined,
  );
  assert.equal(
    read({}, { ...observation, viewport: { ...observation.viewport, readingRevision: 0 } }),
    undefined,
  );
});

function descendants(value) {
  if (!value || typeof value !== 'object') return [];
  return [
    value,
    ...Object.values(value).flatMap((child) =>
      Array.isArray(child) ? child.flatMap(descendants) : descendants(child),
    ),
  ];
}

for (const target of ['tui']) {
  const { component: timeline } = await import(`../dist/packages/timeline/${target}.js`);
  const { component: details } = await import(`../dist/packages/tool-details/${target}.js`);
  test(`${target}: every active compact summary loads before scroll and across viewport changes`, () => {
    const props = {
      theme,
      locale: 'en',
      active: true,
      copyRequest: null,
      browse: null,
      timings: [],
      markReadEnabled: false,
      toolContext: { sessionId: 'session', through: 8 },
      waitingTools: [],
      liveTools: [],
      source: null,
      search: { sessionId: 'session', epoch: 1, through: 8 },
      loading: false,
      older: false,
      newer: false,
      latest: false,
      landing: { sequence: 0, edge: 'end' },
      live: false,
      messages: Array.from({ length: 40 }, (_, i) => ({
        id: 'tool-' + i,
        contentKey: 'tool-' + i,
        role: 'tool_call',
        text: 'Read',
        timestamp: null,
        source: { sourceSequence: 7, itemIndex: 0 },
        image: false,
        continued: false,
        tool: {
          toolUseId: 'read-' + i,
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
      })),
    };
    let out = timeline.mount(props, {});
    const slots = () =>
      descendants(out.frame.view).filter((node) => node.key?.startsWith('tool-details-'));
    assert.equal(slots().length, 40);
    const readers = new Map();
    for (const slot of slots()) {
      const child = details.mount(slot.props, {});
      assert.equal(child.actions.length, 2, 'offscreen compact paths and status eagerly load');
      assert.equal(slot.props.load, true);
      readers.set(slot.key, child);
    }
    // Complete the first, currently offscreen summary through its actual source actions.
    const slot = slots()[0];
    let child = readers.get(slot.key);
    for (const action of child.actions) {
      const text = action.input.field === 'arguments' ? '/src/a0.rs' : 'completed';
      child = details.event(
        child.frame.state,
        {
          kind: 'action',
          key: action.key,
          event: {
            status: 'succeeded',
            value: {
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
                tool: slot.props.tool,
              },
            },
          },
        },
        slot.props,
        {},
      );
      for (const event of child.events) {
        out = timeline.event(
          out.frame.state,
          {
            kind: 'component',
            child: slot.key,
            name: event.name,
            event: event.value,
          },
          props,
          {},
        );
      }
    }
    {
      const query = '/src/a0.rs';
      const basis = { owner: 'find-editor', revision: 1, bytes: Buffer.byteLength(query) };
      out = timeline.event(
        out.frame.state,
        {
          kind: 'component',
          child: out.frame.state.search.key,
          name: 'changed',
          event: { snapshot: { text: basis, marked: null } },
        },
        props,
        {},
      );
      const read = out.actions.find((action) => action.binding === 'reader');
      assert.ok(read, 'search reads the actual query editor snapshot');
      assert.equal(read.method, 'read');
      assert.deepEqual(read.input, { start: 0, end: Buffer.byteLength(query) });
      out = timeline.event(
        out.frame.state,
        {
          kind: 'action',
          key: read.key,
          event: { status: 'succeeded', value: { metadata: basis, text: query } },
        },
        props,
        {},
      );
    }
    const reading = descendants(out.frame.view).find((node) => node.key === 'reading');
    assert.deepEqual(reading.props.command.target, {
      kind: 'find',
      query: '/src/a0.rs',
      step: 'first',
    });
    const resolvedContent = JSON.parse(JSON.stringify(reading.props.content), (_, value) =>
      value?.key === slot.key ? child.frame.view : value,
    );
    assert.ok(
      descendants(resolvedContent).some(
        (node) => typeof node.text === 'string' && node.text.includes('/src/a0.rs'),
      ),
      'the original compact path reaches actual retained ReadingArea content before scrolling',
    );
    const source = descendants(resolvedContent).find(
      (node) => node.binding === 'source' && node.props?.source === '/src/a0.rs',
    );
    assert.ok(
      source,
      'the compact path metadata populates the native source document, including folded content',
    );
    for (const visible of [['message-tool-4'], ['message-tool-20'], []]) {
      out = timeline.event(
        out.frame.state,
        {
          kind: 'component',
          child: 'reading',
          name: 'observed',
          event: {
            readingRevision: 1,
            pendingCommand: null,
            viewport: {
              readingRevision: 1,
              atStart: false,
              atEnd: false,
              visibleChildKeys: visible,
            },
          },
        },
        props,
        {},
      );
      assert.ok(slots().every((node) => node.props.load === true));
    }
    props.browse = 1;
    out = timeline.update(out.frame.state, props, {});
    assert.ok(
      slots().every((node) => node.props.load === true),
      'browsing keeps all summaries',
    );
    props.active = false;
    out = timeline.update(out.frame.state, props, {});
    assert.ok(slots().every((node) => node.props.load === false));
    for (const background of slots())
      assert.equal(details.mount(background.props, {}).actions.length, 0);
    props.active = true;
    out = timeline.update(out.frame.state, props, {});
    const resumed = slots().find((node) => node.key === slot.key);
    assert.equal(
      details.update(child.frame.state, resumed.props, {}).actions.length,
      0,
      'returning to the conversation reuses completed summary parts',
    );
  });
}
