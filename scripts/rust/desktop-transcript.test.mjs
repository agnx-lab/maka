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
import { registerHooks } from 'node:module';
import test from 'node:test';

// Run the production Transcript and host wrappers. Only native module boundaries
// are replaced; calling any UI export fails instead of emulating a toolkit.
const nativeModules = {
  'gpui-kit': `const ui = () => { throw new Error('unexpected native UI call'); };
    export { ui as div, ui as list, ui as svg };`,
  'gpui-base': `const ui = () => { throw new Error('unexpected native UI call'); };
    export { ui as v_flex, ui as h_flex, ui as TextView, ui as Scrollbar,
      ui as InputState, ui as set_theme, ui as Button, ui as Input };`,
  'maka-locale': `export const system = () => 'en';`,
  'maka-host': `export const query = (...args) => globalThis[Symbol.for('transcript.test.query')](...args);
    const unexpected = () => { throw new Error('unexpected native host call'); };
    export { unexpected as mutate, unexpected as subscribe, unexpected as next, unexpected as close };`,
};
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    if (Object.hasOwn(nativeModules, specifier)) {
      return { url: `transcript-test:${specifier}`, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (url.startsWith('transcript-test:')) {
      return { format: 'module', source: nativeModules[url.slice(16)], shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});
const { Transcript } = await import('../../products/maka/desktop-shell/app/transcript.js');
hooks.deregister();

function deferred() {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function chunk(messageId, sequence, text, options = {}) {
  return {
    position: { sourceSequence: sequence, itemIndex: 0 },
    messageId,
    turnId: 'turn',
    field: 'body',
    role: 'assistant',
    offset: 0,
    totalBytes: Buffer.byteLength(text),
    text,
    attachments: [],
    ...options,
  };
}

function page(through, chunks, after = null, before = null) {
  return { through, chunks, after, before };
}

const part = (messageId, field = 'body') => ({
  invocationId: 'run',
  stepId: 'step',
  partId: field,
  messageId,
  field,
});
const cursor = (sequence, offset = 0) => ({
  position: { sourceSequence: sequence, itemIndex: 0 },
  field: 'body',
  offset,
});
const afterSeed = { kind: 'after', cursor: cursor(5) };

function setup(responses = []) {
  const requests = [];
  const reports = [];
  let changes = 0;
  globalThis[Symbol.for('transcript.test.query')] = async (endpoint, method, input) => {
    requests.push({ endpoint, method, input });
    const response = responses.shift();
    if (response === undefined) throw new Error(`unexpected query ${endpoint}.${method}`);
    return typeof response === 'function' ? response(input) : response;
  };
  const transcript = new Transcript(
    { id: 'session', title: 'Test' },
    {
      changed: () => {
        changes += 1;
      },
      report: (message) => reports.push(message),
    },
  );
  transcript.ready = true;
  return { transcript, requests, reports, changes: () => changes };
}

function historyRequests(requests) {
  return requests.map(({ endpoint, method, input }) => ({
    endpoint,
    method,
    ...input,
  }));
}
function request(through, position) {
  return {
    endpoint: 'agent',
    method: 'history',
    sessionId: 'session',
    through,
    position,
    includeThinking: true,
    view: 'conversation',
  };
}
function seeded(responses) {
  const state = setup(responses);
  state.transcript.append([chunk('seed', 5, 'Earlier question', { role: 'user' })]);
  return state;
}
function held(response) {
  const entered = deferred();
  const release = deferred();
  return {
    entered: entered.promise,
    release: release.resolve,
    respond: async () => {
      entered.resolve();
      await release.promise;
      return response;
    },
  };
}

// These are data and identity checks, not evidence about native pixel offsets.
test('body, thinking and pending identities survive canonical handoff', async () => {
  const { transcript: t, reports } = setup([
    page(20, [
      chunk('reply', 18, 'Answer'),
      chunk('thought', 19, 'Reason', { field: 'thinking' }),
      chunk('sent', 20, 'Question', { role: 'user' }),
    ]),
  ]);
  t.appendLive(part('reply'), 'Answer', 10);
  t.appendLive(part('thought', 'thinking'), 'Reason', 11);
  t.addPending('sent', 'Question');
  const before = [...t.live, ...t.pending].map(({ key, text }) => ({ key, text }));
  t.latestKnown = 20;
  await t.follow();
  const expected = [
    { key: 'reply/body', text: 'Answer' },
    { key: 'thought/thinking', text: 'Reason' },
    { key: 'sent/body', text: 'Question' },
  ];
  assert.deepEqual(
    { before, after: t.rows.map(({ key, text }) => ({ key, text })) },
    { before: expected, after: expected },
  );
  assert.deepEqual(
    { live: t.live, pending: t.pending, reports },
    { live: [], pending: [], reports: [] },
  );
});

test('Unicode follow stages every page and accepts replayed deltas until the complete handoff', async () => {
  const head = '开头🙂'.repeat(3000);
  const tail = '结尾é'.repeat(1000);
  const continuation = cursor(20, Buffer.byteLength(head));
  const last = held(
    page(20, [
      chunk('reply', 20, tail, {
        offset: Buffer.byteLength(head),
        totalBytes: Buffer.byteLength(head + tail),
      }),
    ]),
  );
  const {
    transcript: t,
    requests,
    reports,
  } = seeded([
    page(
      20,
      [chunk('reply', 20, head, { totalBytes: Buffer.byteLength(head + tail) })],
      continuation,
    ),
    last.respond,
  ]);
  t.appendLive(part('reply'), head, 10);
  t.latestKnown = 20;
  const following = t.follow();
  await last.entered;
  let during;
  try {
    t.appendLive(part('reply'), tail, 19);
    during = {
      rows: t.rows.map(({ messageId }) => messageId),
      liveText: t.live[0].text,
      liveBytes: Buffer.byteLength(t.live[0].text),
      covered: t.covered.has('reply/body'),
    };
  } finally {
    last.release();
  }
  await following;
  assert.deepEqual(
    { rows: during.rows, covered: during.covered, liveBytes: during.liveBytes },
    { rows: ['seed'], covered: false, liveBytes: 38000 },
  );
  assert.equal(during.liveText, head + tail);
  assert.deepEqual(
    t.rows.map(({ key, text }) => ({ key, text })),
    [
      { key: 'seed/body', text: 'Earlier question' },
      { key: 'reply/body', text: head + tail },
    ],
  );
  assert.deepEqual({ live: t.live, reports }, { live: [], reports: [] });
  assert.deepEqual(historyRequests(requests), [
    request(20, afterSeed),
    request(20, { kind: 'after', cursor: continuation }),
  ]);
});

test('empty follow starts at Start and retains a long pending message attachment through all fragments', async () => {
  const head = '用户🙂'.repeat(3000);
  const tail = '尾段🙂'.repeat(1000) + '最后一行\n↳ note.txt · 26 B';
  const attachments = [{ name: 'note.txt', bytes: 26 }];
  const continuation = cursor(10, Buffer.byteLength(head));
  const {
    transcript: t,
    requests,
    reports,
  } = setup([
    page(
      10,
      [
        chunk('sent', 10, head, {
          role: 'user',
          attachments,
          totalBytes: Buffer.byteLength(head + tail),
        }),
      ],
      continuation,
    ),
    page(10, [
      chunk('sent', 10, tail, {
        role: 'user',
        attachments: [],
        offset: Buffer.byteLength(head),
        totalBytes: Buffer.byteLength(head + tail),
      }),
    ]),
  ]);
  t.addPending('sent', head + tail, attachments);
  const pendingKey = t.pending[0].key;
  t.latestKnown = 10;
  await t.follow();
  assert.deepEqual(
    {
      requests: historyRequests(requests),
      pendingKey,
      rows: t.rows.map(({ key, attachments: files }) => ({ key, files })),
      pending: t.pending,
      reports,
    },
    {
      requests: [
        request(10, { kind: 'start' }),
        request(10, { kind: 'after', cursor: continuation }),
      ],
      pendingKey: 'sent/body',
      rows: [{ key: 'sent/body', files: attachments }],
      pending: [],
      reports: [],
    },
  );
  assert.equal(t.rows[0].text, head + tail);
  // Offset zero remains authoritative when metadata is intentionally removed.
  t.append([
    chunk('sent', 10, head, {
      role: 'user',
      attachments: [],
      totalBytes: Buffer.byteLength(head + tail),
    }),
  ]);
  assert.deepEqual(
    { text: t.rows[0].text, attachments: t.rows[0].attachments },
    { text: head + tail, attachments: [] },
  );
});

test('a captured fence completes before followAgain and preserves later closed live text', async () => {
  const boundary = cursor(10);
  const firstEnd = held(page(20, [chunk('new-user', 10, 'Next question', { role: 'user' })]));
  const secondPass = held(page(21, [chunk('reply', 21, 'Final answer')]));
  const {
    transcript: t,
    requests,
    reports,
  } = seeded([page(20, [], boundary), firstEnd.respond, secondPass.respond]);
  t.appendLive(part('reply'), 'Final answer', 19);
  t.latestKnown = 20;
  const following = t.follow();
  await firstEnd.entered;
  t.latestKnown = 21;
  t.closeLive(21, 'run', 'step');
  await t.follow();
  firstEnd.release();
  await secondPass.entered;
  const between = { rows: t.rows.map(({ text }) => text), live: t.live.map(({ text }) => text) };
  secondPass.release();
  await following;
  assert.deepEqual(between, {
    rows: ['Earlier question', 'Next question'],
    live: ['Final answer'],
  });
  assert.deepEqual(
    t.rows.map(({ text }) => text),
    ['Earlier question', 'Next question', 'Final answer'],
  );
  assert.deepEqual({ live: t.live, reports }, { live: [], reports: [] });
  assert.deepEqual(historyRequests(requests), [
    request(20, afterSeed),
    request(20, { kind: 'after', cursor: boundary }),
    request(21, { kind: 'after', cursor: cursor(10) }),
  ]);
});

test('a concurrent older page survives the staged forward commit', async () => {
  const continuation = cursor(20, 4);
  const last = held(page(20, [chunk('reply', 20, 'answer', { offset: 4, totalBytes: 10 })]));
  const {
    transcript: t,
    requests,
    reports,
  } = seeded([
    page(20, [chunk('reply', 20, 'The ', { totalBytes: 10 })], continuation),
    last.respond,
    page(5, [chunk('older', 1, 'Oldest question', { role: 'user' })]),
  ]);
  t.before = cursor(5);
  t.olderFence = 5;
  t.latestKnown = 20;
  const following = t.follow();
  await last.entered;
  let calls;
  try {
    calls = await t.loadOlder();
  } finally {
    last.release();
  }
  await following;
  assert.equal(calls, 1);
  assert.deepEqual(
    t.rows.map(({ text }) => text),
    ['Oldest question', 'Earlier question', 'The answer'],
  );
  assert.deepEqual(reports, []);
  assert.deepEqual(historyRequests(requests), [
    request(20, afterSeed),
    request(20, { kind: 'after', cursor: continuation }),
    request(5, { kind: 'before', cursor: cursor(5) }),
  ]);
});

test('history failure commits no partial row, coverage or forward cursor and keeps live text', async () => {
  const continuation = cursor(20, 4);
  const { transcript: t, reports } = seeded([
    page(20, [chunk('reply', 20, 'The ', { totalBytes: 10 })], continuation),
    () => {
      throw new Error('history unavailable');
    },
  ]);
  t.appendLive(part('reply'), 'The answer', 19);
  t.latestKnown = 20;
  await t.follow();
  assert.deepEqual(
    {
      rows: t.rows.map(({ text }) => text),
      after: t.after,
      covered: t.covered.has('reply/body'),
      live: t.live.map(({ text }) => text),
    },
    { rows: ['Earlier question'], after: null, covered: false, live: ['The answer'] },
  );
  assert.deepEqual(reports, ['Transcript: history unavailable']);
});

test('navigation closing a transcript abandons staged pages without publishing', async () => {
  const continuation = cursor(20, 4);
  const last = held(page(20, [chunk('reply', 20, 'answer', { offset: 4, totalBytes: 10 })]));
  const {
    transcript: t,
    changes,
    reports,
  } = seeded([
    page(20, [chunk('reply', 20, 'The ', { totalBytes: 10 })], continuation),
    last.respond,
  ]);
  t.appendLive(part('reply'), 'The answer', 19);
  t.latestKnown = 20;
  const following = t.follow();
  await last.entered;
  await t.close();
  const atClose = changes();
  last.release();
  await following;
  assert.deepEqual(
    {
      rows: t.rows.map(({ text }) => text),
      after: t.after,
      covered: t.covered.has('reply/body'),
      live: t.live.map(({ text }) => text),
      notificationsAfterClose: changes() - atClose,
      reports,
    },
    {
      rows: ['Earlier question'],
      after: null,
      covered: false,
      live: ['The answer'],
      notificationsAfterClose: 0,
      reports: [],
    },
  );
});

test('a completed authoritative empty answer clears live text at its terminal fence', async () => {
  const { transcript: t, reports, requests } = seeded([page(20, [])]);
  t.appendLive(part('reply'), 'Superseded streaming draft', 10);
  t.closeLive(20, 'run', 'step');
  t.latestKnown = 20;
  await t.follow();
  assert.deepEqual(
    { rows: t.rows.map(({ text }) => text), live: t.live, reports },
    { rows: ['Earlier question'], live: [], reports: [] },
  );
  assert.deepEqual(historyRequests(requests), [request(20, afterSeed)]);
});

test('tool input previews and executor progress do not become prose or reread history', () => {
  const { transcript: t, reports, requests } = seeded([]);
  t.parts = [part('reply')];
  t.appendLive(part('reply'), 'Reading the project.', 6);
  t.deliver({
    kind: 'events',
    through: 9,
    nextAfter: 9,
    events: [
      {
        kind: 'tool_input_started',
        toolUseId: 'tool',
        stepId: 'step',
        name: 'Read',
        providerExecuted: false,
      },
      { kind: 'tool_input_delta', toolUseId: 'tool', text: '{"path":"秘密.txt"}' },
      { kind: 'executor_tool_progress', toolUseId: 'external', text: 'Private progress' },
    ].map((fact, index) => ({
      sequence: index + 7,
      invocationId: 'run',
      turnId: 'turn',
      fact,
    })),
  });
  assert.deepEqual(
    {
      rows: t.rows.map(({ role, text }) => ({ role, text })),
      live: t.live.map(({ text }) => text),
      requests,
      reports,
      nextAfter: t.watchLast,
    },
    {
      rows: [{ role: 'user', text: 'Earlier question' }],
      live: ['Reading the project.'],
      requests: [],
      reports: [],
      nextAfter: 9,
    },
  );
});

for (const fact of [
  {
    kind: 'tool_input_ready',
    toolUseId: 'tool',
    stepId: 'step',
    name: 'Read',
    providerExecuted: false,
  },
  { kind: 'tool_dispatched', toolUseId: 'tool', name: 'Read', title: null },
  { kind: 'tool_rejected', toolUseId: 'tool' },
  { kind: 'tool_settled', toolUseId: 'tool', settlement: 'returned' },
  { kind: 'provider_tool_result', toolUseId: 'tool', isError: false },
  { kind: 'executor_tool_start', toolUseId: 'tool', name: 'Read' },
  { kind: 'executor_tool_result', toolUseId: 'tool', isError: false },
]) {
  test(`${fact.kind} reads the canonical tool row at the delivery fence`, async () => {
    const {
      transcript: t,
      reports,
      requests,
    } = seeded([page(20, [chunk('tool-row', 20, 'Read project.txt', { role: 'tool_call' })])]);
    let following;
    // Capture the production follow promise started by delivery, without a second read.
    t.follow = () => {
      following = Transcript.prototype.follow.call(t);
      return following;
    };
    t.deliver({
      kind: 'events',
      through: 20,
      nextAfter: 20,
      events: [{ sequence: 20, invocationId: 'run', turnId: 'turn', fact }],
    });
    await following;
    assert.deepEqual(
      { rows: t.rows.map(({ role, text }) => ({ role, text })), reports },
      {
        rows: [
          { role: 'user', text: 'Earlier question' },
          { role: 'tool_call', text: 'Read project.txt' },
        ],
        reports: [],
      },
    );
    assert.deepEqual(historyRequests(requests), [request(20, afterSeed)]);
  });
}
