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

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { DatabaseSync } from 'node:sqlite';
import { join } from 'node:path';
import { fixture } from './host-fixture.js';
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
async function until(check: () => Promise<boolean>) {
  for (let i = 0; i < 250; i++) {
    if (await check()) return;
    await sleep(10);
  }
  throw Error('Shared maintenance did not settle');
}
async function setup(t: any) {
  const f = await fixture({ tickMs: 10, retryMs: 999999 });
  const cursor = (await f.invoke('MemoryRange', {})).to;
  const ids: string[] = [];
  for (const name of ['Events', 'Preferences', 'Paused']) {
    const result = await f.invoke('MemoryIndexCreate', {
      name,
      instructions: name,
      cursor,
      background: false,
    });
    ids.push(result.index.id);
  }
  await f.invoke('MemoryIndexControl', { indexId: ids[2], action: 'pause' });
  const db = new DatabaseSync(join(f.root, 'data/network.sqlite'));
  t.after(async () => {
    await f.close();
    db.close();
  });
  const schedule = () =>
    JSON.parse(
      String(
        db.prepare("SELECT value FROM memory_meta WHERE key='maintenance-schedule'").get()!.value,
      ),
    );
  const due = () => {
    const old = schedule();
    db.prepare("UPDATE memory_meta SET value=? WHERE key='maintenance-schedule'").run(
      JSON.stringify({
        ...old,
        nextCheckAt: old.remainingIds?.length ? old.nextCheckAt : 0,
        retryAt: null,
      }),
    );
  };
  return { f, ids, schedule, due };
}
const members = (prompt: string) => JSON.parse(prompt.slice(prompt.indexOf('\n') + 1));
async function finish(f: any, session: string, id: string, complete = true) {
  const s = await f.invokeAs(session, 'MemoryIndexRead', { indexId: id });
  await f.invokeAs(session, 'MemoryIndexCheckpoint', {
    indexId: id,
    rangeId: s.range.rangeId,
    expectedRevision: s.index.revision,
    complete,
    notes: 'Reviewed; no content changes needed',
  });
  return s.range.rangeId;
}
test('one shared clock gives independent indexes to one Agent, preserves pause and independent completion', async (t) => {
  const { f, ids, schedule, due } = await setup(t);
  const before = schedule().nextCheckAt;
  await f.invoke('MemoryIndexMaintain', { indexId: ids[0] });
  assert.equal(schedule().nextCheckAt, before, 'manual work does not reset the clock');
  const configs = await Promise.all(ids.map((indexId) => f.invoke('MemoryIndexRead', { indexId })));
  assert.equal(configs[0].freshness.nextCheckAt, configs[1].freshness.nextCheckAt);
  assert.equal(configs[2].freshness.nextCheckAt, null);
  const workersBefore = f.workers.size;
  const sessions = new Set<string>();
  const batches: string[][] = [];
  f.setWorkerRunner(async (session, prompt) => {
    sessions.add(session);
    const list = members(prompt).map((x: any) => x.indexId);
    batches.push(list);
    assert.ok(!list.includes(ids[2]));
    if (batches.length === 1) {
      assert.deepEqual(list, ids.slice(0, 2));
      await finish(f, session, ids[1]);
      await finish(f, session, ids[0], false);
    } else {
      assert.deepEqual(list, [ids[0]]);
      await finish(f, session, ids[0]);
    }
  });
  due();
  await until(async () => batches.length === 2 && !schedule().remainingIds?.length);
  assert.equal(sessions.size, 1);
  assert.equal(
    f.workers.size,
    workersBefore + 1,
    'one shared worker, no per-index or coordinator spawns',
  );
  await sleep(50);
  assert.equal(batches.length, 2);
  const next = schedule().nextCheckAt;
  await f.restart({ 'memory-network': { tickMs: 10 } });
  assert.equal(schedule().nextCheckAt, next);
  let secondRound = 0;
  f.setWorkerRunner(async (session, prompt) => {
    assert.ok(sessions.has(session), 'restart keeps the shared maintenance Session');
    assert.deepEqual(
      members(prompt).map((x: any) => x.indexId),
      ids.slice(0, 2),
    );
    secondRound++;
    for (const id of ids.slice(0, 2)) await finish(f, session, id);
  });
  due();
  await until(async () => secondRound === 1 && !schedule().remainingIds?.length);
  assert.equal(f.workers.size, workersBefore + 1);
  const configured = await f.invoke('MemoryIndexControl', {
    action: 'configure',
    intervalMs: 3600000,
  });
  assert.equal(configured.effect.target, 'global_index_maintenance');
  const updated = await Promise.all(ids.map((indexId) => f.invoke('MemoryIndexRead', { indexId })));
  assert.ok(updated.every((s) => s.freshness.intervalMs === 3600000));
  assert.equal(updated[0].freshness.nextCheckAt, updated[1].freshness.nextCheckAt);
  assert.equal(updated[2].freshness.maintenanceEnabled, false);
});
test('failed shared round resumes only unfinished members after restart and retains exact range', async (t) => {
  const { f, ids, schedule, due } = await setup(t);
  let sessionId = '',
    savedRange = '';
  f.setWorkerRunner(async (session, prompt) => {
    sessionId = session;
    if (members(prompt).length === 1) return;
    assert.equal(members(prompt).length, 2);
    await finish(f, session, ids[0]);
    savedRange = (await f.invokeAs(session, 'MemoryIndexRead', { indexId: ids[1] })).range.rangeId;
    // Return without a checkpoint: fail this round without losing its committed member.
  });
  due();
  await until(async () => !!schedule().retryAt);
  assert.deepEqual(schedule().remainingIds, [ids[1]]);
  f.sessions.get('chat-b')!.push({ id: 'later', type: 'user', text: 'A later arrival' });
  await f.restart({ 'memory-network': { tickMs: 10, retryMs: 999999 } });
  let resumed = false;
  f.setWorkerRunner(async (session, prompt) => {
    assert.equal(session, sessionId);
    assert.deepEqual(
      members(prompt).map((x: any) => x.indexId),
      [ids[1]],
    );
    assert.equal(
      (await f.invokeAs(session, 'MemoryIndexRead', { indexId: ids[1] })).range.rangeId,
      savedRange,
    );
    await finish(f, session, ids[1]);
    resumed = true;
  });
  due();
  await until(async () => resumed && !schedule().remainingIds?.length);
  const result = await f.invoke('MemoryIndexRead', { indexId: ids[1] });
  const delta = await f.invoke('MemoryHistory', {
    from: result.freshness.coveredCursor,
    to: result.freshness.observedCursor,
    mode: 'messages',
  });
  assert.deepEqual(
    delta.items.map((x: any) => x.message.id),
    ['later'],
  );
});
