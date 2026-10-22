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

// The production composer, Workbar and window layout run unchanged. Only
// native input state, element builders and the Host boundary are supplied.
const nativeModules = {
  'gpui-kit': `const ui = () => new Proxy({}, { get: (target, key, proxy) => () => proxy });
    export { ui as div, ui as list, ui as svg, ui as uniform_list, ui as PathBuilder };
    export class View {}`,
  'gpui-base': `const ui = () => new Proxy({}, { get: (target, key, proxy) => () => proxy });
    const state = { new: () => ({
      text: '', on() {}, set_auto_grow() {}, set_submit_on_enter() {}, set_placeholder() {},
      set_masked() {}, value() { return this.text; }, set_value(text) { this.text = text; },
    }) };
    export { state as InputState, state as TextareaState };
    export const Button = { new: ui };
    export { ui as v_flex, ui as h_flex, ui as TextView, ui as Scrollbar,
      ui as set_theme, ui as Input, ui as Checkbox, ui as Switch,
      ui as Popover, ui as Textarea, ui as h_resizable, ui as resizable_panel };`,
  'maka-locale': `export const system = () => 'en';`,
  'maka-files': `const unexpected = () => { throw new Error('unexpected native file call'); };
    export { unexpected as openText, unexpected as chooseFolder, unexpected as chooseFiles,
      unexpected as release, unexpected as open, unexpected as reveal };`,
  'maka-desktop': `export const Terminal = {}; export const TerminalState = {};`,
  'maka-host': `export const query = (...args) => globalThis[Symbol.for('composer.test.query')](...args);
    export const mutate = (...args) => globalThis[Symbol.for('composer.test.mutate')](...args);
    const unexpected = () => { throw new Error('unexpected native subscription'); };
    export { unexpected as subscribe, unexpected as next, unexpected as close };`,
};
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    if (Object.hasOwn(nativeModules, specifier)) {
      return { url: `composer-test:${specifier}`, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (url.startsWith('composer-test:')) {
      return { format: 'module', source: nativeModules[url.slice(14)], shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});
const { Composer } = await import('../../products/maka/desktop-shell/app/composer.js');
const { Workbar } = await import('../../products/maka/desktop-shell/app/workbar.js');
const { Transcript } = await import('../../products/maka/desktop-shell/app/transcript.js');
const { default: MakaDesktop } = await import('../../products/maka/desktop-shell/app/main.js');
hooks.deregister();

const THINKING = {
  selection: { connectionId: 'c1', revision: 3, modelId: 'thinking' },
  connectionName: 'One',
  label: 'Thinking',
  thinkingLevels: ['low', 'medium', 'high'],
  defaultThinkingLevel: 'medium',
};
const PLAIN = {
  selection: { connectionId: 'c1', revision: 3, modelId: 'plain' },
  connectionName: 'One',
  label: 'Plain',
  thinkingLevels: [],
  defaultThinkingLevel: null,
};

function returned(value) {
  return { receipt: { kind: 'recorded', outcome: { kind: 'returned', value } } };
}

function session(id, backend) {
  return { id, revision: 1, title: '', labels: [], permissions: null, backend };
}

function transcriptOf(id, backend) {
  return {
    id,
    ready: true,
    endpoint: 'agent',
    session: session(id, backend),
    backend,
    pending: [],
    addPending(messageId, message) {
      this.pending.push({ messageId, message });
    },
  };
}

/**
 * A composer whose Host offers THINKING (the default) and PLAIN, and answers
 * `selectBackend` with `answer` (committed by default) and `send` as started.
 */
async function setup({ answer = null, created = null } = {}) {
  const mutations = [];
  const reports = [];
  globalThis[Symbol.for('composer.test.query')] = async (endpoint, method, input) => {
    assert.equal(`${endpoint}.${method}`, 'settings.models');
    const choices = [THINKING, PLAIN].filter((choice) =>
      choice.selection.modelId.includes(input.query ?? ''),
    );
    return {
      kind: 'ready',
      revision: 1,
      choices,
      defaultModel: { kind: 'ready', choice: THINKING },
    };
  };
  globalThis[Symbol.for('composer.test.mutate')] = async (endpoint, method, input) => {
    mutations.push([method, structuredClone(input)]);
    if (method === 'selectBackend') {
      return returned(
        answer ?? {
          kind: 'committed',
          session: { ...session(input.sessionId, input.backend), revision: 2 },
        },
      );
    }
    if (method === 'send') {
      return returned({ kind: 'started', invocationId: 'run', messageId: 'message' });
    }
    throw new Error(`unexpected mutation ${method}`);
  };
  const composer = new Composer(
    { changed() {}, report: (message) => reports.push(message) },
    () => null,
    async () => created,
    { focus_handle: () => ({}) },
  );
  await composer.loadModel();
  return { composer, mutations, reports };
}

function backend(modelId, thinkingLevel) {
  return { kind: 'model', connectionId: 'c1', modelId, thinkingLevel };
}

test('a thinking level picked for a new task is saved with the task before its first message', async () => {
  const created = transcriptOf('new', null);
  const { composer, mutations, reports } = await setup({ created });
  assert.deepEqual(composer.current(null), { choice: THINKING, thinkingLevel: null });
  await composer.chooseLevel(null, 'high');
  assert.deepEqual(mutations, []);
  composer.draft.set_value('hello');
  await composer.send(null);
  assert.deepEqual(mutations, [
    ['selectBackend', { sessionId: 'new', backend: backend('thinking', 'high') }],
    [
      'send',
      { sessionId: 'new', message: 'hello', model: THINKING.selection, thinkingLevel: 'high' },
    ],
  ]);
  assert.equal(composer.newBackend, null);
  assert.deepEqual(reports, []);
});

test("a task's model and level are its backend preference; another model starts at its default", async () => {
  const { composer, mutations, reports } = await setup();
  const transcript = transcriptOf('task', backend('thinking', 'low'));
  assert.deepEqual(composer.current(transcript), { choice: THINKING, thinkingLevel: 'low' });

  await composer.choose(transcript, PLAIN);
  assert.deepEqual(transcript.backend, backend('plain', null));
  assert.deepEqual(composer.current(transcript), { choice: PLAIN, thinkingLevel: null });

  await composer.choose(transcript, THINKING);
  await composer.chooseLevel(transcript, 'high');
  // The same model keeps its level, and an unchanged choice is not sent again.
  await composer.choose(transcript, THINKING);
  await composer.chooseLevel(transcript, 'high');
  assert.deepEqual(
    mutations.map(([method, input]) => [method, input.backend]),
    [
      ['selectBackend', backend('plain', null)],
      ['selectBackend', backend('thinking', null)],
      ['selectBackend', backend('thinking', 'high')],
    ],
  );

  composer.draft.set_value('one');
  await composer.send(transcript);
  await composer.chooseLevel(transcript, null);
  composer.draft.set_value('two');
  await composer.send(transcript);
  const sends = mutations.filter(([method]) => method === 'send').map(([, input]) => input);
  assert.equal(sends[0].thinkingLevel, 'high');
  // The model's default: the level is left out, and the Host applies it.
  assert.equal(Object.hasOwn(sends[1], 'thinkingLevel'), false);
  assert.deepEqual(reports, []);
});

test('an observation read before a backend change does not undo it', async () => {
  const { composer } = await setup();
  const transcript = transcriptOf('task', backend('thinking', null));
  Object.assign(transcript, {
    latestKnown: 0,
    runThrough: 0,
    run: null,
    runsEnded: 0,
    backendRevision: 0,
    acceptObservation: Transcript.prototype.acceptObservation,
  });
  await composer.chooseLevel(transcript, 'high');
  assert.equal(transcript.backendRevision, 2);
  // A reading of the session from before the commit.
  transcript.acceptObservation({
    through: 1,
    session: { revision: 1, backend: backend('thinking', null) },
    run: null,
  });
  assert.deepEqual(transcript.backend, backend('thinking', 'high'));
  transcript.acceptObservation({
    through: 2,
    session: { revision: 2, backend: backend('thinking', 'low') },
    run: null,
  });
  assert.deepEqual(transcript.backend, backend('thinking', 'low'));
});

test('a refused backend change is reported and leaves the task as it was', async () => {
  const { composer, reports } = await setup({
    answer: { kind: 'rejected', reason: 'tools_unavailable' },
  });
  const transcript = transcriptOf('task', backend('thinking', null));
  assert.equal(await composer.chooseLevel(transcript, 'low'), false);
  assert.deepEqual(transcript.backend, backend('thinking', null));
  assert.equal(composer.locked(transcript), false);
  assert.equal(reports.length, 1);
  assert.match(reports[0], /tools_unavailable/);
});

test('picking the model again drops a saved level it no longer offers', async () => {
  const { composer, mutations } = await setup();
  const transcript = transcriptOf('task', backend('thinking', 'max'));
  assert.deepEqual(composer.current(transcript), { choice: THINKING, thinkingLevel: 'max' });
  await composer.choose(transcript, THINKING);
  assert.deepEqual(mutations, [
    ['selectBackend', { sessionId: 'task', backend: backend('thinking', null) }],
  ]);
});

test("the chips wait for a task's own model, and a change to one task does not hold up a new one", async () => {
  const created = transcriptOf('new', null);
  const { composer, mutations, reports } = await setup({ created });
  const opening = { ...transcriptOf('opening', null), ready: false };
  assert.equal(composer.locked(opening), true);
  assert.equal(composer.locked(null), false);

  const other = transcriptOf('other', backend('thinking', null));
  let answer;
  globalThis[Symbol.for('composer.test.mutate')] = async (endpoint, method, input) => {
    mutations.push([method, structuredClone(input)]);
    if (method === 'selectBackend' && input.sessionId === 'other') {
      return new Promise((resolve) => {
        answer = () =>
          resolve(returned({ kind: 'committed', session: session('other', input.backend) }));
      });
    }
    if (method === 'selectBackend') {
      return returned({ kind: 'committed', session: session(input.sessionId, input.backend) });
    }
    return returned({ kind: 'started', invocationId: 'run', messageId: 'message' });
  };
  const changing = composer.chooseLevel(other, 'low');
  assert.equal(composer.locked(other), true);
  await composer.chooseLevel(null, 'high');
  composer.draft.set_value('hello');
  await composer.send(null);
  assert.deepEqual(
    mutations.map(([method, input]) => [
      method,
      input.sessionId,
      input.thinkingLevel ?? input.backend?.thinkingLevel,
    ]),
    [
      ['selectBackend', 'other', 'low'],
      ['selectBackend', 'new', 'high'],
      ['send', 'new', 'high'],
    ],
  );
  answer();
  assert.equal(await changing, true);
  assert.equal(composer.locked(other), false);
  assert.deepEqual(reports, []);
});

/** The window's layout state, as MakaDesktop keeps it, at `width`, with a Workbar that may be open. */
function desktop() {
  const app = {
    sidebarHidden: false,
    narrow: false,
    narrowSidebarHidden: undefined,
    workbarAsked: false,
    workbarRequests: 0,
    workbar: { open: false, requests: 0, yielded: false, shown: () => app.workbar.open },
    cx: { notify() {} },
    width: 1280,
    layout: MakaDesktop.prototype.layout,
    toggleSidebar: MakaDesktop.prototype.toggleSidebar,
  };
  globalThis.window = { viewport_size: () => ({ width: app.width }) };
  return app;
}

test('the sidebar folds away in a narrow window, and a choice made there outlasts it', () => {
  const app = desktop();
  assert.deepEqual(app.layout(), { sidebar: true, workbar: null });
  app.width = 800;
  assert.deepEqual(app.layout(), { sidebar: false, workbar: null });
  app.toggleSidebar();
  assert.deepEqual(app.layout(), { sidebar: true, workbar: null });
  assert.equal(app.sidebarHidden, false);
  app.toggleSidebar();
  app.width = 1280;
  // Hidden while narrow, it stays hidden once the window widens.
  assert.deepEqual(app.layout(), { sidebar: false, workbar: null });
  app.toggleSidebar();
  app.width = 800;
  assert.deepEqual(app.layout(), { sidebar: false, workbar: null });
  app.width = 1280;
  // Narrowing and widening again without a toggle keeps the wide choice.
  assert.deepEqual(app.layout(), { sidebar: true, workbar: null });
});

test('where the sidebar, the conversation and the Workbar do not fit, the Workbar takes room only when asked for last', () => {
  const app = desktop();
  app.workbar.open = true;
  app.width = 1280;
  assert.deepEqual(app.layout(), { sidebar: true, workbar: 'beside' });
  app.width = 900;
  // Opened at startup, the Workbar gives way to the sidebar without closing.
  assert.deepEqual(app.layout(), { sidebar: true, workbar: null });
  assert.equal(app.workbar.yielded, true);
  app.workbar.requests += 1;
  assert.deepEqual(app.layout(), { sidebar: false, workbar: 'beside' });
  assert.equal(app.workbar.yielded, false);
  app.toggleSidebar();
  assert.deepEqual(app.layout(), { sidebar: true, workbar: null });
  // The sidebar's preference was never changed by the room.
  assert.equal(app.sidebarHidden, false);
  app.width = 1280;
  assert.deepEqual(app.layout(), { sidebar: true, workbar: 'beside' });
  app.width = 640;
  // Narrower than the conversation and the Workbar side by side: the
  // Workbar, last asked for before the sidebar, leaves the conversation be.
  assert.deepEqual(app.layout(), { sidebar: false, workbar: null });
  app.workbar.requests += 1;
  assert.deepEqual(app.layout(), { sidebar: false, workbar: 'alone' });
  app.toggleSidebar();
  assert.deepEqual(app.layout(), { sidebar: true, workbar: null });
  // Hiding the sidebar again gives its room to the conversation, not the Workbar.
  app.toggleSidebar();
  assert.deepEqual(app.layout(), { sidebar: false, workbar: null });
});

test('asking for the tool a yielded Workbar has shows it instead of closing it', () => {
  const changed = [];
  const workbar = Object.assign(Object.create(Workbar.prototype), {
    transcript: {},
    open: true,
    tool: 'changes',
    yielded: true,
    requests: 0,
    hooks: { changed: () => changed.push('changed') },
    changedState() {
      changed.push('state');
    },
  });
  workbar.toggle('changes');
  assert.equal(workbar.open, true);
  assert.equal(workbar.requests, 1);
  workbar.yielded = false;
  workbar.toggle('changes');
  assert.equal(workbar.open, false);
  assert.deepEqual(changed, ['changed', 'state']);
});
