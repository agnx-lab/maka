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
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const uuid = (n) => `00000000-0000-0000-0000-${String(n).padStart(12, '0')}`;
const session = {
  id: 'session',
  title: 'Session',
  labels: [],
  revision: 1,
  updatedAt: 1,
  archived: false,
  activity: 'idle',
  boundaryRevision: 0,
  permissions: { access: 'workspace_write', requestPermissions: true },
  workspace: {
    path: '/project',
    directoryIdentity: 'sha256:' + 'a'.repeat(64),
    identity: 'workspace:v1:' + uuid(99),
  },
};
function resource(ref = 'shell:first', revision = 1, status = 'running') {
  return {
    sessionId: session.id,
    ownership: { kind: 'local' },
    sourceTurnId: 'user',
    sourceToolCallId: 'user',
    result: {
      kind: 'shell_run',
      ref,
      mode: 'pty',
      status,
      cwd: '/project',
      cmd: '/bin/sh',
      startedAt: 1,
      updatedAt: revision,
      revision,
      output: {
        mode: 'pty',
        screen: 'prompt',
        scrollback: '',
        cols: 80,
        rows: 12,
        cursor: { x: 6, y: 0, visible: true },
        alternateScreen: false,
        truncated: false,
        redacted: false,
      },
    },
  };
}
function reference(method, operation = 30) {
  return {
    source: { stateDomain: uuid(1), deployment: uuid(2), revision: 1 },
    namespace: [uuid(3)],
    export: 'terminal',
    method,
    operation: uuid(operation),
    fingerprint: 'sha256-' + 'a'.repeat(64),
    contract: contracts.terminal,
    recovery: 'journal_v1',
  };
}
const reply = (ref, value) => ({
  reference: ref,
  receipt: {
    kind: 'recorded',
    outcome: { kind: 'returned', value: { kind: 'completed', value } },
    cleanup: { kind: 'confirmed' },
  },
});
function nodes(node) {
  return node && typeof node === 'object'
    ? [
        node,
        ...Object.values(node).flatMap((value) =>
          Array.isArray(value) ? value.flatMap(nodes) : nodes(value),
        ),
      ]
    : [];
}
async function client(target) {
  const { component } = await import(`../dist/packages/terminal-panel/${target}.js`);
  let props = { theme, locale: 'en', open: true, busy: false, enabled: true, session };
  let out = component.mount(props, {});
  const c = {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    update(next) {
      props = { ...props, ...next };
      out = component.update(c.state, props, {});
      return c;
    },
    event(input) {
      out = component.event(c.state, input, props, {});
      return c;
    },
    raw(key) {
      return c.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    action(key, event) {
      return c.event({ kind: 'action', key, event });
    },
    sub(method, value) {
      const sub = out.subscriptions.find((s) => s.method === method);
      assert.ok(sub, method);
      return c.action(sub.key, { status: 'updated', value: { kind: 'completed', value } });
    },
    component(child, name, event) {
      return c.event({ kind: 'component', child, name, event });
    },
    get screen() {
      return nodes(out.frame.view).find((n) => n.key === 'terminal-screen').props;
    },
    get mutation() {
      return out.actions.find((a) => ['start', 'control', 'stop', 'release'].includes(a.method));
    },
    loaded() {
      c.sub('watchList', { resources: [resource()], revision: 'one', total: 1, nextOffset: null });
      c.sub('watch', resource());
      return c;
    },
    connect(sequence = 37) {
      c.raw('connect');
      c.sub('acquire', {
        controllerId: uuid(10),
        nextSequence: sequence,
        held: true,
        resource: resource(),
      });
      return c;
    },
    input(owner = uuid(11), sequence = 1, revision = 1) {
      return c.component('terminal-screen', 'observed', {
        owner,
        revision,
        source: 'session:shell:first',
        controller: uuid(10),
        captured: true,
        batch: {
          controller: uuid(10),
          sequence,
          actions: [{ type: 'text', text: 'x' }],
          size: null,
        },
        error: null,
      });
    },
  };
  return c;
}
for (const target of ['tui']) {
  test(`${target} full service pages remain traversable within the native input budget`, async () => {
    const c = await client(target);
    const resources = Array.from({ length: 64 }, (_, i) => resource(`shell:${i}`));
    c.sub('watchList', { resources, revision: 'full', total: 64, nextOffset: null });
    assert.equal(c.state.page.resources.length, 16);
    assert.equal(c.state.page.nextOffset, 16);
    assert.ok(nodes(c.out.frame.view).filter((n) => n.kind === 'button').length < 64);
    c.raw('next');
    assert.equal(c.out.subscriptions.find((s) => s.method === 'watchList').input.offset, 16);
    c.sub('watchList', {
      resources: resources.slice(16),
      revision: 'full',
      total: 64,
      nextOffset: null,
    });
    assert.equal(c.state.page.resources[0].result.ref, 'shell:16');
    assert.equal(c.state.page.nextOffset, 32);
    c.raw('previous');
    assert.equal(c.out.subscriptions.find((s) => s.method === 'watchList').input.offset, 0);
  });
  test(`${target} history reads exact original receipts with bounded outstanding work`, async () => {
    const c = (await client(target)).loaded().raw('history');
    const page = c.out.actions[0];
    const entries = Array.from({ length: 4 }, (_, i) => ({
      cursor: i + 1,
      reference: reference('start', 30 + i),
    }));
    c.action(page.key, { status: 'succeeded', value: entries });
    const reads = c.out.actions;
    assert.deepEqual(
      reads.map((a) => a.input),
      entries.map((e) => e.reference),
    );
    c.raw('history-refresh');
    assert.equal(c.out.actions.length, 0);
    for (const [index, read] of reads.entries()) {
      const result = reply(entries[index].reference, resource(`shell:${index}`));
      if (index === 0) result.receipt.outcome.value = { kind: 'failed', message: 'launch refused' };
      c.action(read.key, { status: 'succeeded', value: result });
    }
    assert.equal(c.state.historyNext, 4);
    assert.ok(
      nodes(c.out.frame.view).some(
        (n) => n.key === 'terminal-operation-state-1' && n.text.includes('failed'),
      ),
    );
    c.raw('history-next');
    const next = c.out.actions[0];
    assert.deepEqual(next.input, { after: 4, limit: 4 });
    c.update({ open: false });
    c.action(next.key, { status: 'succeeded', value: entries });
    assert.equal(c.out.actions.length, 0);
    assert.equal(c.state.selected, null);
  });
  test(`${target} opaque long resource references retain bounded stable row identities`, async () => {
    const c = await client(target),
      ref = 'maka://runtime/background-tasks/ma_' + 'a'.repeat(64) + '_' + uuid(50);
    const page = { resources: [resource(ref)], revision: 'first', total: 1, nextOffset: null };
    c.sub('watchList', page);
    const key = c.state.rowKeys[ref];
    assert.ok(key.length < 128);
    c.sub('watchList', { ...page, revision: 'second', resources: [resource(ref, 2)] });
    assert.equal(c.state.rowKeys[ref], key);
    c.raw(key);
    assert.equal(c.state.selected, ref);
    c.sub('watchList', { ...page, revision: 'third', resources: [resource('new')] });
    c.raw(key);
    assert.equal(c.state.selected, ref, 'retired row cannot redirect to a replacement');
    assert.equal(Object.keys(c.state.rowKeys).length, 1);
    assert.ok(
      nodes(c.out.frame.view)
        .filter((n) => n.kind && n.key)
        .every((n) => n.key.length <= 128),
    );
  });
  test(`${target} terminal uses Host sequence across native remount and never replays a settled batch`, async () => {
    const c = (await client(target)).loaded().connect().input();
    const a = c.mutation;
    assert.equal(a.method, 'control');
    assert.equal(a.input.sequence, 37);
    c.action(a.key, {
      status: 'operation',
      reply: reply(reference('control'), {
        controllerId: uuid(10),
        sequence: 37,
        acceptedBytes: 1,
        resized: false,
        resizeChanged: false,
      }),
    });
    assert.deepEqual(c.screen.acknowledgement, {
      owner: uuid(11),
      controller: uuid(10),
      sequence: 1,
      accepted: true,
    });
    c.input(uuid(11), 1, 2);
    assert.equal(c.mutation, undefined);
    c.sub('acquire', {
      controllerId: uuid(10),
      nextSequence: 37,
      held: true,
      resource: resource(),
    });
    c.input(uuid(12), 1, 1);
    assert.equal(c.mutation.input.sequence, 38);
    const pending = c.mutation;
    c.raw('release');
    assert.equal(
      c.out.subscriptions.some((s) => s.method === 'acquire'),
      false,
    );
    assert.equal(
      c.mutation,
      undefined,
      'accepted control remains the original outstanding mutation',
    );
    c.action(pending.key, {
      status: 'operation',
      reply: reply(reference('control', 31), {
        controllerId: uuid(10),
        sequence: 38,
        acceptedBytes: 1,
        resized: false,
        resizeChanged: false,
      }),
    });
    assert.equal(c.state.current, null);
    assert.equal(c.screen.controller, null);
    assert.equal(c.screen.acknowledgement, null);
  });
  test(`${target} terminal close and session switch cancel leases while preserving unknown operation custody`, async () => {
    const c = (await client(target)).loaded().connect().input(),
      a = c.mutation,
      ref = reference('control');
    c.update({ open: false });
    assert.deepEqual(c.out.subscriptions, []);
    c.action(a.key, {
      status: 'operation',
      reply: { reference: ref, receipt: { kind: 'unavailable', diagnostic: 'lost reply' } },
    });
    assert.deepEqual(c.state.current.reference, ref);
    c.update({ open: true, session: { ...session, id: 'another' } });
    c.raw('new');
    assert.equal(c.mutation, undefined);
    c.raw('check');
    const check = c.out.actions[0];
    assert.deepEqual(check.input, ref);
    c.action(check.key, {
      status: 'succeeded',
      value: reply(
        { ...ref, source: { ...ref.source, stateDomain: uuid(999) } },
        { controllerId: uuid(10), sequence: 37 },
      ),
    });
    assert.ok(c.state.current);
    c.raw('check');
    c.action(c.out.actions[0].key, {
      status: 'succeeded',
      value: reply(ref, {
        controllerId: uuid(10),
        sequence: 37,
        acceptedBytes: 1,
        resized: false,
        resizeChanged: false,
      }),
    });
    assert.equal(c.state.current, null);
    assert.equal(c.screen.controller, null);
    assert.equal(c.screen.acknowledgement, null);
    assert.equal(c.out.actions.length, 0);
  });
  test(`${target} stop requires current confirmation and lease cancellation precedes the mutation`, async () => {
    const c = (await client(target)).loaded().connect();
    c.raw('confirm');
    assert.equal(c.mutation, undefined);
    c.raw('stop');
    assert.equal(c.mutation, undefined);
    assert.equal(
      c.out.subscriptions.some((s) => s.method === 'acquire'),
      false,
    );
    c.raw('confirm');
    const stop = c.mutation;
    assert.equal(stop.method, 'stop');
    assert.deepEqual(stop.input, { sessionId: 'session', ref: 'shell:first' });
    c.action(stop.key, {
      status: 'operation',
      reply: reply(reference('stop'), {
        applied: true,
        resource: resource('shell:first', 2, 'cancelled'),
      }),
    });
    c.raw('connect');
    assert.equal(
      c.out.subscriptions.some((s) => s.method === 'acquire'),
      false,
    );
    c.sub('watch', resource('shell:first', 1));
    assert.equal(c.state.resource.result.status, 'cancelled');
  });
  test(`${target} command capture cannot launch after its original session closes`, async () => {
    const c = (await client(target)).loaded();
    const draft = {
      revision: 1,
      snapshot: {
        text: { owner: uuid(15), revision: 1, bytes: 4 },
        selection: { start: 4, end: 4 },
        reversed: false,
        marked: null,
      },
      receipt: null,
    };
    c.raw('command').component('terminal-command', 'changed', draft).raw('run');
    const read = c.out.actions[0];
    assert.equal(read.method, 'read');
    assert.equal(read.resources.text.child, 'terminal-command');
    c.update({ open: false });
    c.action(read.key, {
      status: 'succeeded',
      value: { text: 'true', metadata: draft.snapshot.text, start: 0, end: 4 },
    });
    assert.equal(c.mutation, undefined);
  });
  test(`${target} unavailable, archived and source-owned terminals remain non-mutating`, async () => {
    const c = await client(target);
    c.update({ enabled: false }).raw('new');
    assert.deepEqual(c.out.subscriptions, []);
    assert.equal(c.mutation, undefined);
    c.update({ enabled: true })
      .loaded()
      .update({ session: { ...session, archived: true } })
      .raw('connect')
      .raw('new');
    assert.equal(c.mutation, undefined);
    assert.equal(
      c.out.subscriptions.some((s) => s.method === 'acquire'),
      false,
    );
    c.update({ session }).sub('watch', {
      ...resource('shell:first', 2),
      ownership: { kind: 'source_owned', sourceSessionId: 'source', ownerSessionId: 'owner' },
    });
    c.raw('connect').raw('stop');
    assert.equal(c.mutation, undefined);
    assert.equal(c.state.confirmation, null);
  });
  test(`${target} command snapshots launch once and keep bounded canonical output indicators`, async () => {
    const c = (await client(target)).loaded();
    const draft = {
      revision: 1,
      snapshot: {
        text: { owner: uuid(15), revision: 1, bytes: 4 },
        selection: { start: 4, end: 4 },
        reversed: false,
        marked: null,
      },
      receipt: null,
    };
    c.raw('command').component('terminal-command', 'changed', draft).raw('run');
    const read = c.out.actions[0];
    c.action(read.key, {
      status: 'succeeded',
      value: { text: 'true', metadata: draft.snapshot.text, start: 0, end: 4 },
    });
    const start = c.mutation;
    assert.deepEqual(start.input, { sessionId: 'session', command: 'true' });
    c.raw('run');
    assert.equal(c.mutation, undefined);
    const pipes = resource('shell:pipes');
    pipes.result.mode = 'pipes';
    pipes.result.output = {
      mode: 'pipes',
      stdout: 'hello',
      stderr: 'error',
      stdoutTruncated: true,
      stderrTruncated: false,
      redacted: false,
    };
    c.action(start.key, { status: 'operation', reply: reply(reference('start'), pipes) });
    assert.equal(c.state.commandOpen, false);
    assert.equal(c.state.selected, 'shell:pipes');
    assert.ok(nodes(c.out.frame.view).some((n) => n.key === 'terminal-truncated'));
    assert.ok(
      nodes(c.out.frame.view).some((n) => n.key === 'terminal-stdout' && n.text === 'hello'),
    );
    assert.equal(nodes(c.out.frame.view).find((n) => n.key === 'copy').copy_text, 'hello\nerror');
    c.sub('watch', {
      ...pipes,
      result: { ...pipes.result, revision: 2, output: { ...pipes.result.output, redacted: true } },
    });
    assert.equal(
      nodes(c.out.frame.view).some((n) => n.key === 'terminal-stdout'),
      false,
    );
    assert.equal(nodes(c.out.frame.view).find((n) => n.key === 'copy').copy_text, undefined);
    assert.equal(nodes(c.out.frame.view).find((n) => n.key === 'copy').disabled, true);
  });
  test(`${target} explicit release removes the lease immediately and reconnect initializes a fresh Host sequence`, async () => {
    const c = (await client(target)).loaded().connect();
    c.raw('release');
    const release = c.mutation;
    assert.equal(release.method, 'release');
    assert.equal(c.screen.controller, null);
    assert.equal(
      c.out.subscriptions.some((s) => s.method === 'acquire'),
      false,
    );
    c.action(release.key, {
      status: 'operation',
      reply: reply(reference('release'), { controllerId: uuid(10), released: true }),
    });
    c.raw('connect');
    c.sub('acquire', {
      controllerId: uuid(20),
      nextSequence: 1,
      held: true,
      resource: resource(),
    });
    assert.deepEqual(c.state.lease, { id: uuid(20), next: 1 });
    assert.equal(c.state.current, null);
  });
  test(`${target} unconfirmed cleanup settles the result but stops native input pumping`, async () => {
    const c = (await client(target)).loaded().connect().input(),
      a = c.mutation;
    assert.equal(
      nodes(c.out.frame.view).some((n) => n.key === 'terminal-recovery'),
      false,
    );
    const r = reply(reference('control'), {
      controllerId: uuid(10),
      sequence: 37,
      acceptedBytes: 1,
      resized: false,
      resizeChanged: false,
    });
    r.receipt.cleanup = { kind: 'unconfirmed' };
    c.action(a.key, { status: 'operation', reply: r });
    assert.equal(c.state.current, null);
    assert.equal(c.state.notice, 'terminalCleanupUnknown');
    assert.equal(c.screen.controller, null);
    assert.equal(c.screen.acknowledgement, null);
    assert.equal(c.out.actions.length, 0);
  });
  test(`${target} a late recovery read cannot settle a new operation after explicit leave-unresolved`, async () => {
    const c = (await client(target)).loaded().raw('new'),
      old = c.mutation,
      ref = reference('start');
    c.action(old.key, {
      status: 'operation',
      reply: { reference: ref, receipt: { kind: 'not_recorded' } },
    });
    c.raw('check');
    const check = c.out.actions[0];
    c.raw('forget').raw('confirm').raw('new');
    const fresh = c.mutation;
    assert.ok(fresh);
    c.action(check.key, { status: 'succeeded', value: reply(ref, resource('shell:old')) });
    assert.ok(c.state.current);
    assert.equal(c.state.current.reference, null);
    assert.equal(c.state.selected, 'shell:first');
    c.action(fresh.key, {
      status: 'operation',
      reply: reply(reference('start', 32), resource('shell:new')),
    });
    assert.equal(c.state.selected, 'shell:new');
  });
}
