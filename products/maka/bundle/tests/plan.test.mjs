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
const theme = activate({ scheme: 'dark' }).Appearance.value;
async function client(target) {
  const module = await import(`../dist/packages/agent-screen/${target}.js`);
  module.activate({ locale: 'en' });
  const values = { appearance: [theme] };
  let out = module.component.mount({}, values);
  Object.assign(out.frame.state, {
    selected: {
      id: 'session',
      title: 'Plan test',
      labels: [],
      revision: 1,
      updatedAt: 1,
      archived: false,
      activity: 'idle',
      boundaryRevision: 7,
      hasUnread: false,
      dataSource: {
        stateDomain: 'a'.repeat(64),
        deployment: '00000000-0000-0000-0000-000000000001',
        namespace: ['00000000-0000-0000-0000-000000000002'],
      },
      permissions: { access: 'read_only', requestPermissions: true },
      workspace: { path: '/original', directoryIdentity: 'a'.repeat(64), identity: 'original' },
    },
    model: {
      choice: {
        selection: { connectionId: '00000000-0000-0000-0000-000000000003', modelId: 'fixture' },
        name: 'fixture',
      },
      thinkingLevel: null,
    },
    inspector: 'plan',
    dialog: null,
    modelRefreshing: false,
    pending: {},
    restoring: false,
  });
  out.frame.state.plan.page = {
    enabled: true,
    current: {
      revision: 1,
      proposal: {
        id: 'proposal',
        planId: 'plan',
        revision: 1,
        turnId: 'turn',
        status: 'pending_approval',
        supersedes: null,
        sourceExecutionId: null,
        submittedAt: 1,
        artifact: {
          title: 'Reviewed 中😀',
          overview: 'Exact version',
          steps: [
            {
              id: 'a',
              title: 'Implement',
              description: 'Original step',
              files: [],
              complexity: 'low',
            },
          ],
          risks: ['Original risk'],
        },
      },
      execution: null,
    },
  };
  const c = {
    get state() {
      return out.frame.state;
    },
    get out() {
      return out;
    },
    event(input) {
      out = module.component.event(c.state, input, {}, values);
      return c;
    },
    raw(key) {
      return c.event({ kind: 'raw', key, event: { kind: 'activate' } });
    },
    reply(value, key = out.actions[0].key) {
      return c.event({ kind: 'action', key, event: { status: 'succeeded', value } });
    },
  };
  return c;
}

test('tui checkpoints confirmed live operations without blocking new mutations', async () => {
  const c = await client('tui');
  c.event({
    kind: 'component',
    child: 'recovery-checkpoint',
    name: 'changed',
    key: 'bookmark',
    event: { sequence: 0, state: { kind: 'ready', revision: null, value: null } },
  });
  c.state.pending.confirm = { kind: 'markRead', sessionId: 'session' };
  const reference = { operation: 'known-operation' };
  c.event({
    kind: 'action',
    key: 'confirm',
    event: {
      status: 'operation',
      reply: {
        reference,
        receipt: {
          kind: 'recorded',
          outcome: { kind: 'returned', value: { kind: 'observed', session: c.state.selected } },
          cleanup: { kind: 'confirmed' },
        },
      },
    },
  });
  assert.equal(c.state.restoring, false);
  assert.equal(c.out.actions[0].method, 'page');
  const original = {
    cursor: 7,
    caller: { stateDomain: 'ui-domain', deployment: 'ui' },
    consumer: ['consumer'],
    reference,
  };
  c.state.pending.newMutation = { kind: 'selectBackend' };
  c.reply([original]);
  assert.equal(
    c.state.uncertain,
    null,
    'a concurrent submission is not an unknown recovered operation',
  );
  assert.deepEqual(c.out.actions, []);
  delete c.state.pending.newMutation;
  c.raw('unused');
  c.reply([original]);
  assert.equal(
    c.out.actions[0].method,
    'page',
    'the original returned receipt needs no second query',
  );
  c.reply([]);
  assert.equal(c.state.restoreThrough, 7);
  assert.equal(c.state.recoveryCheckpoint.command.value.through, 7);
  c.state.recoveryDirty = true;
  c.raw('unused');
  c.reply([{ ...original, cursor: 8, reference: { operation: 'other-mount' } }]);
  assert.equal(c.out.actions[0].method, 'read');
  c.reply({
    reference: { operation: 'other-mount' },
    receipt: {
      kind: 'recorded',
      outcome: { kind: 'unresolved' },
      cleanup: { kind: 'unconfirmed' },
    },
  });
  assert.equal(c.state.uncertain, null);
  assert.equal(
    c.state.restoreThrough,
    7,
    'a background observation never checkpoints an unfinished operation',
  );
});

for (const target of ['tui']) {
  test(`${target} recovery checkpoints preserve caller scope and advance only a settled prefix`, async () => {
    const caller = { stateDomain: 'ui-domain', deployment: 'ui-deployment' };
    const consumer = ['original-consumer'];
    const scope = JSON.stringify([caller.stateDomain, caller.deployment, consumer]);
    for (const sameScope of [false, true]) {
      for (const known of [false, true]) {
        const c = await client(target);
        c.state.restoring = true;
        c.state.pending.initial = { kind: 'restorePage' };
        c.event({
          kind: 'component',
          child: 'recovery-checkpoint',
          name: 'changed',
          key: 'bookmark',
          event: {
            sequence: 0,
            state: {
              kind: 'ready',
              revision: 1,
              value: { through: 41, scope: sameScope ? scope : 'another-consumer' },
            },
          },
        });
        const original = {
          cursor: 1,
          caller,
          consumer,
          reference: { operation: 'first-operation' },
        };
        c.reply([original], 'initial');
        if (sameScope) {
          assert.deepEqual(c.out.actions[0].input, { after: 41, limit: 1 });
          assert.equal(c.state.restoreThrough, 41);
        } else {
          assert.equal(
            c.out.actions[0].method,
            'read',
            'another consumer must check its own history',
          );
          assert.equal(c.state.restoreThrough, 0);
          c.reply({
            reference: original.reference,
            receipt: {
              kind: 'recorded',
              outcome: { kind: 'returned', value: {} },
              cleanup: { kind: 'confirmed' },
            },
          });
        }
        const through = c.state.restoreThrough;
        const reference = { operation: 'next-operation' };
        c.reply([{ cursor: 42, caller, consumer, reference }]);
        assert.equal(c.state.restoreThrough, through);
        c.reply({
          reference,
          receipt: known
            ? {
                kind: 'recorded',
                outcome: { kind: 'returned', value: {} },
                cleanup: { kind: 'confirmed' },
              }
            : { kind: 'unavailable', diagnostic: 'unknown' },
        });
        assert.equal(c.state.restoreThrough, through);
        if (known) {
          c.reply([]);
          assert.equal(c.state.restoreThrough, 42);
          assert.deepEqual(c.state.recoveryCheckpoint.command.value, { through: 42, scope });
        } else {
          assert.ok(c.state.uncertain);
          assert.equal(
            c.state.restoreThrough,
            through,
            'an unknown operation must not advance the saved prefix',
          );
        }
      }
    }
  });
  test(`${target} conversation reacts to composer readiness and admission without owning sends`, async () => {
    const c = await client(target);
    c.state.dialog = null;
    c.state.pending.refresh = { kind: 'observe', epoch: c.state.epoch };
    c.event({
      kind: 'component',
      child: 'composer',
      name: 'readiness',
      key: 'readiness',
      event: [{ context: 'session', dataSource: c.state.selected.dataSource, ready: true }],
    });
    function composer(node) {
      if (!node || typeof node !== 'object') return null;
      if (node.binding === 'composer') return node;
      return Object.values(node)
        .flatMap((value) => (Array.isArray(value) ? value : [value]))
        .map(composer)
        .find(Boolean);
    }
    assert.equal(composer(c.out.frame.view).props.busy, false);
    assert.equal(composer(c.out.frame.view).props.enabled, true);
    const file = {
      sessionId: 'session',
      id: 'attachment',
      name: 'one.txt',
      mimeType: 'text/plain',
      bytes: 3,
      dataSource: c.state.selected.dataSource,
    };
    const restored = {
      kind: 'component',
      child: 'composer',
      name: 'restored',
      key: 'restored',
      event: { files: [file] },
    };
    c.event(restored);
    c.event(restored);
    assert.deepEqual(c.state.attachments.files, [file]);
    c.event({
      kind: 'component',
      child: 'composer',
      name: 'sent',
      key: 'sent',
      event: { context: 'session', dataSource: file.dataSource, attachments: ['attachment'] },
    });
    assert.deepEqual(c.state.attachments.files, []);
    assert.equal(
      c.out.actions.some((action) => ['send', 'execute', 'read'].includes(action.method)),
      false,
    );
  });
  test(`${target} Plan approval freezes reviewed version, model and permission boundary`, async () => {
    const c = await client(target);
    c.raw('plan-approve');
    const confirmation = structuredClone(c.state.plan.confirmation);
    assert.equal(c.out.actions.length, 0);
    assert.equal(confirmation.snapshot.proposal.artifact.steps[0].description, 'Original step');
    assert.equal(confirmation.input.action.approval.target.boundaryRevision, 7);
    c.state.selected.workspace.path = '/later';
    c.state.selected.permissions.access = 'workspace_write';
    c.state.model.choice.selection.modelId = 'later';
    c.raw('plan-confirm');
    assert.equal(c.out.actions[0].method, 'changePlan');
    assert.deepEqual(c.out.actions[0].input, confirmation.input);
  });
  test(`${target} newer Plan revision invalidates approval and history stays read-only`, async () => {
    const c = await client(target);
    c.raw('plan-approve');
    c.state.plan.watch = { key: 'original-plan-watch', input: { sessionId: 'session' } };
    const next = structuredClone(c.state.plan.page);
    next.current.revision = 2;
    next.current.proposal.artifact.steps[0].description = 'Revised step';
    c.event({
      kind: 'action',
      key: 'original-plan-watch',
      event: { status: 'updated', value: next },
    });
    assert.equal(c.state.plan.confirmation, null);
    c.raw('plan-confirm');
    assert.equal(c.out.actions.length, 0);
    c.raw('plan-history');
    assert.equal(c.out.actions[0].method, 'planHistory');
    assert.equal(c.out.actions[0].input.throughRevision, 2);
    const old = structuredClone(next.current);
    old.revision = 1;
    c.reply({ throughRevision: 2, snapshots: [old], nextAfter: 1 });
    c.raw('plan-approve');
    assert.equal(c.state.plan.confirmation, null);
    assert.equal(c.out.actions.length, 0);
    c.raw('plan-history-next');
    assert.equal(c.out.actions[0].input.throughRevision, 2);
    assert.equal(c.out.actions[0].input.after, 1);
  });
  test(`${target} Plan mode is an original metadata mutation preserving conversation settings`, async () => {
    const c = await client(target);
    const original = structuredClone(c.state.selected);
    c.raw('plan-mode-plan');
    assert.equal(c.out.actions[0].method, 'metadata');
    assert.deepEqual(c.out.actions[0].input, {
      sessionId: original.id,
      expectedRevision: original.revision,
      title: original.title,
      labels: original.labels,
      collaborationMode: 'plan',
    });
    assert.equal(
      c.state.selected.collaborationMode,
      undefined,
      'mode is not changed optimistically',
    );
  });
  test(`${target} Plan recovers the unknown original approval without a second mutation`, async () => {
    const c = await client(target);
    c.raw('plan-approve');
    c.raw('plan-confirm');
    const original = c.out.actions[0];
    const reference = {
      source: {
        stateDomain: 'a'.repeat(64),
        deployment: '00000000-0000-0000-0000-000000000001',
        revision: 1,
      },
      namespace: ['00000000-0000-0000-0000-000000000002'],
      export: 'agent',
      method: 'changePlan',
      operation: '00000000-0000-0000-0000-000000000004',
      fingerprint: 'sha256-' + 'b'.repeat(64),
      contract: {},
      recovery: 'journal_v1',
    };
    c.event({
      kind: 'action',
      key: original.key,
      event: { status: 'operation', reply: { reference, receipt: { kind: 'unknown' } } },
    });
    assert.equal(c.out.actions[0].method, 'planReceipt');
    assert.deepEqual(c.out.actions[0].input, { reference });
    const recovery = c.out.actions[0];
    c.raw('plan-approve');
    assert.equal(c.state.plan.confirmation, null);
    assert.equal(c.out.actions.length, 0);
    c.reply({ kind: 'committed', revision: 2 }, recovery.key);
    assert.equal(c.state.uncertain, null);
    assert.equal(c.state.plan.watch.input.sessionId, 'session');
    assert.ok(c.out.actions.every((action) => action.method !== 'changePlan'));
  });
}
