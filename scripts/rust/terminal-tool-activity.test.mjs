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
import test from 'node:test';
import {
  updateToolActivity,
  latestToolActivity,
} from '../../products/maka/terminal-shell/app/toolActivity.ts';

import { replayStartCursor } from '../../products/maka/terminal-shell/app/history.ts';

test('complete active tool identities survive 129 starts, updates and exact terminal removal', () => {
  let active = [];
  const event = (index, fact, invocationId = 'invocation') => ({
    invocationId,
    turnId: 'turn',
    sequence: index + 1,
    fact,
  });
  const id = (index) => 'tool_' + index.toString(16).padStart(64, '0');
  for (let index = 0; index < 129; index++) {
    active = updateToolActivity(
      active,
      event(index, {
        kind: 'executor_tool_start',
        toolUseId: id(index),
        name: 'echo',
      }),
    );
  }
  assert.deepEqual(
    active.map((tool) => tool.toolUseId),
    Array.from({ length: 129 }, (_, index) => id(index)),
  );
  assert.equal(latestToolActivity(active, 'invocation', 'turn').toolUseId, id(128));
  active = updateToolActivity(
    active,
    event(130, {
      kind: 'executor_tool_progress',
      toolUseId: id(128),
      text: 'original progress',
    }),
  );
  assert.equal(active.length, 129);
  assert.equal(active[128].detail, 'original progress');
  assert.equal(active[128].startSequence, 129);
  active = updateToolActivity(
    active,
    event(131, { kind: 'executor_tool_start', toolUseId: id(0), name: 'independent' }, 'other'),
  );
  assert.equal(active.length, 130);
  active = updateToolActivity(
    active,
    event(132, { kind: 'executor_tool_result', toolUseId: id(128) }),
  );
  assert.equal(active.length, 129);
  assert.equal(latestToolActivity(active, 'invocation', 'turn').toolUseId, id(127));
  assert.equal(latestToolActivity(active, 'other', 'turn').name, 'independent');
  active = updateToolActivity(active, event(133, { kind: 'ended' }));
  assert.deepEqual(
    active.map((tool) => [tool.invocationId, tool.toolUseId, tool.name]),
    [['other', id(0), 'independent']],
  );
});

test('replay cursor preserves the earliest complete part and tool seed without argument limits', () => {
  assert.equal(replayStartCursor(9, [], []), 9);
  assert.equal(replayStartCursor(2, [{ startSequence: 8 }], [{ startSequence: 7 }]), 2);
  assert.equal(
    replayStartCursor(100, [{ startSequence: 9 }, { startSequence: 3 }], [{ startSequence: 8 }]),
    2,
  );
  assert.equal(
    replayStartCursor(100, [{ startSequence: 9 }], [{ startSequence: 8 }, { startSequence: 2 }]),
    1,
  );
  const parts = Array.from({ length: 200000 }, (_, index) => ({
    startSequence: index + 100,
  }));
  const tools = Array.from({ length: 200000 }, (_, index) => ({
    startSequence: index + 200,
  }));
  parts[parts.length - 1] = { startSequence: 3 };
  assert.equal(replayStartCursor(900000, parts, tools), 2);
  tools[tools.length - 1] = { startSequence: 1 };
  assert.equal(replayStartCursor(900000, parts, tools), 0);
});
