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

// The production transcript runs unchanged against a fake Host. The element
// builders record what they are given, so a drawn pane can be searched for
// the rail and the button back to the newest row, and clicked.
const recorder = `const element = (kind) => (...args) => {
    const node = { kind, args, calls: [], children: [] };
    const proxy = new Proxy(node, {
      get(target, key) {
        if (key === 'node') return node;
        return (...given) => {
          if (key === 'when') return given[0] ? given[1](proxy) : proxy;
          if (key === 'child') node.children.push(given[0]);
          else if (key === 'children') node.children.push(...given[0]);
          else node.calls.push([key, ...given]);
          return proxy;
        };
      },
    });
    return proxy;
  };`;
const nativeModules = {
  'gpui-kit': `${recorder}
    export const div = element('div');
    export const list = element('list');
    export const svg = element('svg');`,
  'gpui-base': `${recorder}
    const ui = element('flex');
    export const Button = { new: element('Button') };
    export const Scrollbar = { vertical: element('Scrollbar') };
    export const v_virtual_list = element('v_virtual_list');
    export const VirtualListScrollHandle = { new: () => ({
      scrolls: [], released: false,
      scroll_to_item(index, strategy) { this.scrolls.push([index, strategy]); },
      release() { this.released = true; return true; },
    }) };
    export { ui as v_flex, ui as h_flex, ui as TextView, ui as InputState, ui as set_theme,
      ui as Input };`,
  'maka-locale': `export const system = () => 'en';`,
  'maka-host': `export const query = (...args) => globalThis[Symbol.for('navigation.test.query')](...args);
    const unexpected = () => { throw new Error('unexpected native host call'); };
    export { unexpected as mutate, unexpected as subscribe, unexpected as next, unexpected as close };`,
};
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    if (Object.hasOwn(nativeModules, specifier)) {
      return { url: `navigation-test:${specifier}`, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (url.startsWith('navigation-test:')) {
      return { format: 'module', source: nativeModules[url.slice(16)], shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});
const { Transcript, transcriptPane } = await import(
  '../../products/maka/desktop-shell/app/transcript.js'
);
const { t } = await import('../../products/maka/desktop-shell/app/locale.js');
hooks.deregister();

const cx = { theme: () => ({ is_dark: false }) };
/** Wide enough for the rail beside the reading column, and not. */
const WIDE = 1000;
const NARROW = 800;

const at = (sequence) => ({ sourceSequence: sequence, itemIndex: 0 });
const cursor = (sequence, offset = 0) => ({ position: at(sequence), field: 'body', offset });

function chunk(turnId, sequence, text, role = 'assistant') {
  return {
    position: at(sequence),
    messageId: `${turnId}-${sequence}`,
    turnId,
    field: 'body',
    role,
    offset: 0,
    totalBytes: Buffer.byteLength(text),
    text,
    attachments: [],
  };
}

const page = (through, chunks, before = null) => ({ through, chunks, after: null, before });
const landmark = (turnId, sequence, label = '') => ({
  turnId,
  position: at(sequence),
  lastPosition: at(sequence + 1),
  label,
});

function deferred() {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/**
 * A transcript with turn `t3` loaded (rows 10 and 11), older history before
 * row 10 at fence 30, and the Host's landmarks for `t1` (row 1) and `t2`
 * (row 5). `responses` answer the history reads in order.
 */
function setup(responses = []) {
  const requests = [];
  const reports = [];
  let changes = 0;
  globalThis[Symbol.for('navigation.test.query')] = async (endpoint, method, input) => {
    requests.push({ method, input: structuredClone(input) });
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
  transcript.olderFence = 30;
  transcript.before = cursor(10);
  transcript.landmarks = {
    through: 30,
    items: [landmark('t1', 1, 'First question'), landmark('t2', 5, 'Second question')],
  };
  transcript.append([chunk('t3', 10, 'Third question', 'user'), chunk('t3', 11, 'Third answer')]);
  return { transcript, requests, reports, changes: () => changes };
}

const turn = (transcript, turnId) => transcript.turns().find((each) => each.turnId === turnId);

/** The nodes of a drawn tree, depth first. */
function nodes(tree) {
  const found = [];
  const walk = (value) => {
    const node = value?.node;
    if (node === undefined) return;
    found.push(node);
    for (const child of node.children) walk(child);
  };
  walk(tree);
  return found;
}
const calls = (node, name) => node.calls.filter(([key]) => key === name).map(([, ...args]) => args);
const rail = (tree) => nodes(tree).find((node) => node.kind === 'v_virtual_list');
const latest = (tree) =>
  nodes(tree).find((node) => node.kind === 'Button' && node.args[0] === 'transcript-latest');
/** The rail's ticks for the visible `range`, as its renderer draws them. */
const ticks = (tree, range) => rail(tree).args[4](range, cx);
const tooltip = (tick) => calls(tick.node, 'tooltip')[0][0];
const clickContext = () => ({
  stopped: 0,
  stop_propagation() {
    this.stopped += 1;
  },
});

test('the rail lists the Host’s landmarks with the loaded turns, labelled where either knows the message', async () => {
  const { transcript: tr, requests } = setup();
  tr.landmarks = { through: 30, items: [landmark('t1', 1, 'First question'), landmark('t2', 5)] };
  tr.prepend([chunk('t2', 5, 'Second question', 'user'), chunk('t2', 6, 'Second answer')]);
  tr.append([chunk('t4', 20, '  Fourth\n\n  question  ', 'user'), chunk('t5', 22, 'Continued')]);
  assert.deepEqual(
    tr
      .turns()
      .map(({ turnId, label, position, loaded }) => [
        turnId,
        label,
        position.sourceSequence,
        loaded,
      ]),
    [
      ['t1', 'First question', 1, false],
      ['t2', 'Second question', 5, true],
      ['t3', 'Third question', 10, true],
      ['t4', 'Fourth question', 20, true],
      ['t5', '', 22, true],
    ],
  );
  assert.equal(tr.turns(), tr.turns(), 'built again only when rows or landmarks change');
  // Landmarks at or before the fence read already are not read again.
  await tr.readLandmarks(30);
  assert.deepEqual(requests, []);
});

test('landmarks are read one fence at a time, the newest asked for after, and a failed read leaves them', async () => {
  const held = deferred();
  const { transcript: tr, requests } = setup([
    async () => {
      await held.promise;
      return { through: 40, landmarks: [landmark('t5', 30, 'Fifth')] };
    },
    { through: 42, landmarks: [landmark('t5', 30, 'Fifth'), landmark('t6', 40, 'Sixth')] },
    () => {
      throw new Error('landmarks unavailable');
    },
  ]);
  const reading = tr.readLandmarks(40);
  await tr.readLandmarks(41);
  await tr.readLandmarks(42);
  held.resolve();
  await reading;
  assert.deepEqual(
    tr.turns().map(({ turnId }) => turnId),
    ['t3', 't5', 't6'],
  );
  await tr.readLandmarks(43);
  assert.deepEqual(
    requests.map(({ method, input }) => [method, input.through]),
    [
      ['historyLandmarks', 40],
      ['historyLandmarks', 42],
      ['historyLandmarks', 43],
    ],
  );
  assert.equal(tr.landmarks.through, 42);
});

test('a turn not loaded is read back to its start, then shown at the top of the list', async () => {
  const {
    transcript: tr,
    requests,
    reports,
  } = setup([
    page(
      30,
      [chunk('t2', 5, 'Second question', 'user'), chunk('t2', 6, 'Second answer')],
      cursor(3),
    ),
    page(30, [chunk('t1', 1, 'First question', 'user'), chunk('t1', 2, 'First answer')]),
  ]);
  tr.toLatest();
  await tr.goToTurn(turn(tr, 't1'));
  assert.deepEqual(
    requests.map(({ input }) => input.position),
    [
      { kind: 'before', cursor: cursor(10) },
      { kind: 'before', cursor: cursor(3) },
    ],
  );
  assert.deepEqual(tr.jump, { position: at(1), field: null, drawn: false });
  assert.equal(tr.anchored, true);
  // The list made for the jump must not take the earlier request for the end as its own.
  assert.equal(tr.scrollToEnd, null);
  assert.equal(tr.goingTo, null);
  assert.deepEqual(reports, []);
});

test('a first row loaded only in part is read to its start', async () => {
  const { transcript: tr, requests } = setup([
    page(30, [chunk('t3', 10, 'Third question', 'user')]),
  ]);
  tr.before = cursor(10, 6);
  await tr.goToTurn(turn(tr, 't3'));
  assert.equal(requests.length, 1);
  assert.deepEqual(tr.jump?.position, at(10));
});

test('history that does not move back stops the jump with a message', async () => {
  const { transcript: tr, reports } = setup([
    page(30, [chunk('t3', 10, 'Third question', 'user')], cursor(10)),
  ]);
  await tr.goToTurn(turn(tr, 't1'));
  assert.deepEqual(reports, [t('turnUnreachable')]);
  assert.equal(tr.jump, null);
  assert.equal(tr.goingTo, null);
});

test('a page that cannot be read stops the jump; the history row offers it again', async () => {
  const { transcript: tr, reports } = setup([
    () => {
      throw new Error('history unavailable');
    },
  ]);
  await tr.goToTurn(turn(tr, 't1'));
  assert.deepEqual(reports, [t('earlierMessagesFailed', { value: 'history unavailable' })]);
  assert.equal(tr.olderError, 'history unavailable');
  assert.equal(tr.jump, null);
  assert.equal(tr.goingTo, null);
});

test('the same turn asked for again stops loading it, and another turn takes its place', async () => {
  const held = deferred();
  const entered = deferred();
  const respond = async () => {
    entered.resolve();
    await held.promise;
    return page(30, [chunk('t1', 1, 'First question', 'user')]);
  };
  const first = setup([respond]);
  const going = first.transcript.goToTurn(turn(first.transcript, 't1'));
  await entered.promise;
  assert.equal(first.transcript.goingTo, 't1');
  await first.transcript.goToTurn(turn(first.transcript, 't1'));
  assert.equal(first.transcript.goingTo, null);
  held.resolve();
  await going;
  assert.equal(first.transcript.jump, null, 'stopped: nothing shown');
  assert.equal(first.requests.length, 1, 'and nothing more read');

  const again = deferred();
  const reached = deferred();
  const second = setup([
    async () => {
      reached.resolve();
      await again.promise;
      return page(30, [chunk('t1', 1, 'First question', 'user')]);
    },
  ]);
  const older = second.transcript.goToTurn(turn(second.transcript, 't1'));
  await reached.promise;
  await second.transcript.goToTurn(turn(second.transcript, 't3'));
  assert.deepEqual(second.transcript.jump?.position, at(10));
  again.resolve();
  await older;
  assert.deepEqual(second.transcript.jump?.position, at(10), 'the later turn stays shown');
  assert.equal(second.transcript.jumps, 1);
});

test('a page being read is shared: the jump waits for the one the list asked for', async () => {
  const held = deferred();
  const { transcript: tr, requests } = setup([
    async () => {
      await held.promise;
      return page(30, [chunk('t2', 5, 'Second question', 'user')], cursor(3));
    },
    page(30, [chunk('t1', 1, 'First question', 'user')]),
  ]);
  const listed = tr.loadOlder();
  const going = tr.goToTurn(turn(tr, 't1'));
  assert.equal(tr.loadOlder(), listed);
  held.resolve();
  await going;
  assert.equal(requests.length, 2);
  assert.deepEqual(tr.jump?.position, at(1));
});

test('the list says which turn is at its top and whether the newest row is on screen', () => {
  const { transcript: tr, changes } = setup();
  tr.prepend([chunk('t2', 5, 'Second question', 'user'), chunk('t2', 6, 'Second answer')]);
  tr.before = null;
  // Items: history start, t2's two rows, t3's two rows.
  const before = changes();
  tr.showing({ start: 0, end: 2 }, 5);
  assert.deepEqual([tr.topTurn, tr.atLatest, changes() - before], ['t2', false, 1]);
  tr.showing({ start: 1, end: 3 }, 5);
  assert.equal(changes() - before, 1, 'the same turn and place draw nothing again');
  tr.showing({ start: 3, end: 5 }, 5);
  assert.deepEqual([tr.topTurn, tr.atLatest, changes() - before], ['t3', true, 2]);
  // Newer rows not read yet: the newest drawn is not the newest there is.
  tr.after = cursor(11);
  tr.showing({ start: 3, end: 5 }, 5);
  assert.deepEqual([tr.atLatest, changes() - before], [false, 3]);
});

test('the pane draws the rail where it fits and the button while the newest row is off screen', () => {
  const { transcript: tr } = setup();
  tr.showing({ start: 0, end: 1 }, 3);
  let pane = transcriptPane(tr, () => {}, cx, null, WIDE);
  const drawn = ticks(pane, { start: 0, end: 3 });
  assert.deepEqual(
    drawn.map((tick) => tooltip(tick)),
    ['First question', 'Second question', 'Third question'],
  );
  assert.deepEqual(tr.railScroll.scrolls, [[2, 'center']], 'the marked turn is kept in view');
  assert.equal(calls(rail(pane), 'track_scroll')[0][0], tr.railScroll);
  assert.ok(latest(pane), 'the newest row is off screen');

  // The rows under the rail and the button must not take their clicks too.
  const clicked = clickContext();
  calls(rail(pane), 'on_item_click')[0][0]('t3', clicked);
  assert.deepEqual(tr.jump?.position, at(10));
  calls(latest(pane), 'on_click')[0][0]({}, clicked);
  assert.deepEqual([tr.anchored, tr.scrollToEnd], [false, 'latest/1']);
  assert.equal(clicked.stopped, 2);
  const listed = nodes(transcriptPane(tr, () => {}, cx, null, WIDE)).find(
    (node) => node.kind === 'list',
  );
  assert.deepEqual(calls(listed, 'scroll_to_end'), [['latest/1']]);

  tr.showing({ start: 1, end: 3 }, 3);
  pane = transcriptPane(tr, () => {}, cx, null, NARROW);
  assert.equal(rail(pane), undefined, 'no room beside the column');
  assert.equal(latest(pane), undefined, 'at the newest row');

  const alone = setup().transcript;
  alone.landmarks = { through: 30, items: [] };
  assert.equal(
    rail(transcriptPane(alone, () => {}, cx, null, WIDE)),
    undefined,
    'one turn has no rail',
  );
});

test('a tick says when its turn is being loaded and that a click stops it', async () => {
  const held = deferred();
  const { transcript: tr } = setup([
    async () => {
      await held.promise;
      return page(30, [chunk('t1', 1, 'First question', 'user')]);
    },
  ]);
  const going = tr.goToTurn(turn(tr, 't1'));
  const pane = transcriptPane(tr, () => {}, cx, null, WIDE);
  assert.equal(
    tooltip(ticks(pane, { start: 0, end: 1 })[0]),
    t('turnLoading', { value: 'First question' }),
  );
  held.resolve();
  await going;
  await tr.close();
  assert.equal(tr.railScroll, null, 'released on close');
});

test('sending, going back to the newest row or scrolling stops a jump still loading', async () => {
  const pageOfFirst = () => page(30, [chunk('t1', 1, 'First question', 'user')]);
  for (const leave of [
    (tr) => tr.addPending('sent', 'Hello'),
    (tr) => tr.toLatest(),
    (tr) => {
      const area = nodes(transcriptPane(tr, () => {}, cx, null, WIDE))[0];
      calls(area, 'on_scroll_wheel')[0][0]({}, cx);
    },
  ]) {
    const held = deferred();
    const { transcript: tr, reports } = setup([
      async () => {
        await held.promise;
        return pageOfFirst();
      },
    ]);
    const going = tr.goToTurn(turn(tr, 't1'));
    leave(tr);
    assert.equal(tr.goingTo, null);
    const end = tr.scrollToEnd;
    held.resolve();
    await going;
    assert.deepEqual(
      { jump: tr.jump, anchored: tr.anchored, scrollToEnd: tr.scrollToEnd, reports },
      { jump: null, anchored: false, scrollToEnd: end, reports: [] },
    );
  }
  const idle = setup().transcript;
  const area = nodes(transcriptPane(idle, () => {}, cx, null, WIDE))[0];
  assert.deepEqual(calls(area, 'on_scroll_wheel'), [], 'no wheel handler while no jump loads');
});

test('a jump whose transcript closes meanwhile says nothing', async () => {
  const held = deferred();
  const { transcript: tr, reports } = setup([
    async () => {
      await held.promise;
      return page(30, [chunk('t1', 1, 'First question', 'user')]);
    },
  ]);
  const going = tr.goToTurn(turn(tr, 't1'));
  await tr.close();
  held.resolve();
  await going;
  assert.deepEqual([reports, tr.jump], [[], null]);
});

test('a turn with no row to show says so', async () => {
  const { transcript: tr, reports } = setup();
  tr.landmarks = { through: 30, items: [landmark('t9', 12, 'Ninth')] };
  await tr.goToTurn(turn(tr, 't9'));
  assert.deepEqual([reports, tr.jump], [[t('turnNotShown')], null]);
});

test('reading to the end without a new item looks at the last range again', async () => {
  const { transcript: tr } = setup([page(30, [])]);
  tr.anchored = true;
  tr.after = cursor(11, 12);
  tr.latestKnown = 30;
  tr.showing({ start: 0, end: 2 }, 2);
  assert.deepEqual([tr.atLatest, tr.anchored], [false, true]);
  await tr.follow();
  assert.deepEqual([tr.atLatest, tr.anchored], [true, false]);
});
