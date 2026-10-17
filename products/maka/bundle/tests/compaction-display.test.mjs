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
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'dark' }).Appearance.value;
const nodes = (node) =>
  node && typeof node === 'object'
    ? [
        node,
        ...Object.values(node).flatMap((value) =>
          Array.isArray(value) ? value.flatMap(nodes) : nodes(value),
        ),
      ]
    : [];
for (const target of ['tui']) {
  test(`${target} active context keeps an explicit last-known reading alongside token speed`, async () => {
    const { component } = await import(`../dist/packages/composer-footer/${target}.js`);
    const props = {
      theme,
      locale: 'en',
      sessionId: 'session',
      dataSource: null,
      running: true,
      compacting: false,
      metrics: {
        modelId: 'fixture',
        connectionId: 'connection',
        contextTokens: 100000,
        contextWindow: 200000,
        approximate: true,
        contextLastKnown: true,
        outputTokensPerSecond: 15.6,
      },
    };
    const last = component.mount(props);
    const node = (out, key) => nodes(out.frame.view).find((node) => node.key === key);
    assert.equal(node(last, 'session-context-text').text, 'Context ≈100k / 200k · 50% (last)');
    assert.equal(node(last, 'session-rate-text').text, ' · 15.6 tok/s');
    {
      assert.equal(
        node(last, 'session-context-text').hint,
        'Last known context usage; newer additions are not yet counted.',
      );
      assert.equal(node(last, 'metrics-percent-text').text, '≈50% (last)');
      assert.equal(node(last, 'metrics-percent-rate').text, ' · 15.6 tok/s');
      const noCapacity = component.mount({
        ...props,
        metrics: { ...props.metrics, contextWindow: null },
      });
      assert.equal(node(noCapacity, 'metrics-percent-text').text, 'Context ≈100k (last)');
      assert.equal(node(noCapacity, 'metrics-percent-rate').text, ' · 15.6 tok/s');
    }
    const fresh = component.update(last.frame.state, {
      ...props,
      metrics: { ...props.metrics, contextTokens: 120000, contextLastKnown: false },
    });
    assert.equal(node(fresh, 'session-context-text').text, 'Context ≈120k / 200k · 60%');
    const unknown = component.update(fresh.frame.state, {
      ...props,
      metrics: { ...props.metrics, contextTokens: null, contextLastKnown: false },
    });
    assert.equal(node(unknown, 'session-context-text').text, 'Context — / 200k');
    assert.equal(node(unknown, 'session-rate-text').text, ' · 15.6 tok/s');
    {
      assert.equal(node(unknown, 'metrics-percent-text').text, 'Context —');
      assert.equal(node(unknown, 'metrics-percent-rate').text, ' · 15.6 tok/s');
    }
    const firstRequest = component.mount({ ...props, metrics: null });
    assert.equal(node(firstRequest, 'session-context-text').text, 'Context —');
    const idleUnknown = component.update(unknown.frame.state, {
      ...props,
      running: false,
      metrics: { ...props.metrics, contextTokens: null, contextLastKnown: false },
    });
    assert.equal(node(idleUnknown, 'session-rate-text').text, '15.6 tok/s');
    assert.equal(
      nodes(idleUnknown.frame.view).filter((node) => node.key === 'session-context-text').length,
      0,
    );
  });
  test(`${target} compaction tints only context and restores the ordinary footer`, async () => {
    const { component } = await import(`../dist/packages/composer-footer/${target}.js`);
    const props = {
      theme,
      locale: 'en',
      sessionId: 'session',
      dataSource: null,
      running: true,
      compacting: false,
      metrics: {
        modelId: 'fixture',
        connectionId: null,
        contextTokens: 100000,
        contextWindow: 200000,
        approximate: false,
        outputTokensPerSecond: 42,
      },
    };
    const idle = component.mount(props);
    const busy = component.update(idle.frame.state, { ...props, compacting: true });
    const restored = component.update(busy.frame.state, props);
    const node = (out, key) => nodes(out.frame.view).find((node) => node.key === key);
    assert.equal(node(busy, 'session-context-text').text, 'Context 100k / 200k · 50%');
    assert.notDeepEqual(
      node(busy, 'session-context-text').color,
      node(idle, 'session-context-text').color,
    );
    assert.deepEqual(node(busy, 'session-rate-text').color, node(idle, 'session-rate-text').color);
    assert.deepEqual(
      node(restored, 'session-context-text').color,
      node(idle, 'session-context-text').color,
    );
    const unknown = component.mount({ ...props, metrics: null, compacting: true });
    assert.equal(node(unknown, 'session-context-text').text, 'Compressing context');
  });
  test(`${target} successful compaction is a quiet history divider without message controls`, async () => {
    const { component } = await import(`../dist/packages/timeline/${target}.js`);
    const props = {
      theme,
      locale: 'zh-CN',
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
          id: 'checkpoint',
          contentKey: 'checkpoint',
          role: 'compaction',
          compaction: { durationMs: 1250 },
          text: '',
          timestamp: 10000,
          source: null,
          image: false,
          continued: false,
          tool: null,
        },
      ],
    };
    const tree = nodes(component.mount(props).frame.view);
    assert.ok(tree.some((node) => node.text === '上下文已压缩（1.3秒）'));
    assert.ok(tree.some((node) => node.key === 'compact-rule-leftcheckpoint'));
    assert.ok(tree.some((node) => node.key === 'compact-rule-rightcheckpoint'));
    assert.ok(!tree.some((node) => node.key === 'source-checkpoint'));
    assert.ok(!tree.some((node) => node.key === 'mark-read-checkpoint'));
    {
      props.browse = 1;
      const ordinary = (id) => ({
        ...props.messages[0],
        id,
        contentKey: id,
        role: 'assistant',
        compaction: undefined,
        text: 'Answer',
      });
      props.messages = [ordinary('before'), props.messages[0], ordinary('after')];
      const started = component.mount(props);
      assert.equal(started.frame.state.browse.id, 'before');
      const moved = component.event(
        started.frame.state,
        { kind: 'raw', key: 'browse-next', event: { kind: 'activate' } },
        props,
      );
      assert.equal(moved.frame.state.browse.id, 'after');
    }
  });
}
