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
  module.activate({ graphs: true });
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
      collaborationMode: 'agent',
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
    inspector: 'graph',
    dialog: null,
    modelRefreshing: false,
    pending: {},
    restoring: false,
  });
  out.frame.state.graph.page = {
    enabled: true,
    currentEpoch: null,
    selectedMode: null,
    epoch: null,
    revision: 0,
    status: 'empty',
    quiescent: true,
    work: [],
    after: null,
    finish: null,
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

const epoch = { graphId: 'agent_graph_1', rootSessionId: 'session', epoch: 1, mode: 'swarm' };
for (const target of ['tui']) {
  test(`${target} Graph control keeps original receipt and stale conversation replies cannot redirect it`, async () => {
    const c = await client(target);
    c.raw('graph-start-swarm');
    const action = c.out.actions[0];
    assert.equal(action.method, 'changeGraph');
    assert.equal(action.binding, 'workhub');
    assert.deepEqual(action.input, {
      sessionId: 'session',
      expectedGraphId: null,
      action: { kind: 'start', mode: 'swarm' },
    });
    const reference = {
      source: {
        stateDomain: 'a'.repeat(64),
        deployment: '00000000-0000-0000-0000-000000000001',
        revision: 1,
      },
      namespace: [],
      export: 'workhub',
      method: 'changeGraph',
      operation: '00000000-0000-0000-0000-000000000002',
      fingerprint: 'b'.repeat(64),
      contract: contracts.workhubCoordinator,
      recovery: 'journal_v1',
    };
    c.event({
      kind: 'action',
      key: action.key,
      event: { status: 'operation', reply: { reference, receipt: { kind: 'pending' } } },
    });
    assert.equal(c.out.actions[0].method, 'graphReceipt');
    assert.deepEqual(c.out.actions[0].input, { reference });
    const receiptKey = c.out.actions[0].key;
    c.state.selected.id = 'other-conversation';
    c.state.epoch++;
    c.reply({ kind: 'committed', epoch }, receiptKey);
    assert.equal(c.state.graph.page.epoch, null);
    assert.equal(c.state.uncertain, null);
    assert.equal(c.out.actions.filter((a) => a.method === 'changeGraph').length, 0);
  });
  test(`${target} pinned Graph history is read only and exact result cursors survive paging`, async () => {
    const c = await client(target);
    Object.assign(c.state.graph.page, {
      currentEpoch: epoch,
      epoch,
      selectedMode: 'swarm',
      status: 'completed',
      revision: 2,
    });
    c.raw('graph-history');
    c.reply({ epochs: [epoch], currentEpoch: 1, nextBefore: null });
    c.raw('graph-epoch-agent_graph_1');
    assert.equal(c.state.graph.watch.input.graphId, 'agent_graph_1');
    const key = c.state.graph.watch.key;
    c.event({
      kind: 'action',
      key,
      event: {
        status: 'updated',
        value: {
          enabled: true,
          currentEpoch: epoch,
          epoch,
          selectedMode: 'swarm',
          status: 'completed',
          revision: 2,
          quiescent: true,
          work: [
            {
              workId: 'graph_work_1',
              taskId: 'wt_' + '1'.repeat(64),
              target: { kind: 'agent', agentId: 'worker' },
              status: 'completed',
              instructionExcerpt: 'Exact task',
              instructionTruncated: false,
              resultId: 'result_1',
              failure: null,
            },
          ],
          after: null,
          finish: { resultIds: ['result_1'], reason: 'Reviewed final' },
        },
      },
    });
    c.raw('graph-start-graph');
    assert.equal(c.out.actions.length, 0);
    c.raw('graph-stop');
    assert.equal(c.out.actions.length, 0);
    c.raw('graph-result-graph_work_1');
    assert.deepEqual(c.out.actions[0].input, {
      sessionId: 'session',
      graphId: 'agent_graph_1',
      workId: 'graph_work_1',
      resultId: 'result_1',
      offset: 0,
    });
    c.reply({
      workId: 'graph_work_1',
      resultId: 'result_1',
      text: '中😀',
      nextOffset: 7,
      execution: {},
    });
    c.raw('graph-result-next');
    assert.equal(c.out.actions[0].input.offset, 7);
    c.reply({
      workId: 'graph_work_1',
      resultId: 'result_1',
      text: 'tail',
      nextOffset: null,
      execution: {
        sessionId: 'worker-session',
        turnId: 'turn',
        runId: 'run',
        invocationId: 'invocation',
        highWater: 5,
        state: 'completed',
      },
    });
    c.raw('graph-result-back');
    assert.equal(c.out.actions[0].input.offset, 0);
    c.reply({
      workId: 'graph_work_1',
      resultId: 'result_1',
      text: '中😀',
      nextOffset: 7,
      execution: {},
    });
    c.raw('graph-current');
    assert.equal(c.state.graph.watch.input.graphId, null);
    assert.equal(c.state.graph.result, null);
  });
}
