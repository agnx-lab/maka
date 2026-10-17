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
import { readFileSync, readdirSync } from 'node:fs';
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const source = {
  stateDomain: 'a'.repeat(64),
  deployment: '00000000-0000-0000-0000-000000000001',
  revision: 2,
};
function nodes(value) {
  return value && typeof value === 'object'
    ? [
        value,
        ...Object.values(value).flatMap((child) =>
          Array.isArray(child) ? child.flatMap(nodes) : nodes(child),
        ),
      ]
    : [];
}
function instances(start, count = 8) {
  return Array.from({ length: count }, (_, index) => {
    const at = start + index;
    return {
      instance: [`id-${at}`],
      path: [`plugin-${at}`],
      package: { id: `example.plugin-${at}`, revision: 'b'.repeat(64) },
      structural: false,
      disabled: false,
      parentDisabled: false,
      presentation: {
        displayName: `Plugin ${at}`,
        description: at === 79 ? 'Write notes on Saturn.' : `Purpose ${at}`,
        localizations: {
          'zh-CN': {
            displayName: `插件 ${at}`,
            description: at === 79 ? '记录土星观察笔记。' : `用途 ${at}`,
          },
        },
      },
    };
  });
}
async function client(target, locale = 'en') {
  const { component } = await import(`../dist/packages/deployment/${target}.js`);
  const props = { theme, locale, open: true, busy: false };
  let out = component.mount(props, {});
  const c = {
    get out() {
      return out;
    },
    get state() {
      return out.frame.state;
    },
    event(input) {
      out = component.event(c.state, input, props, {});
      return c;
    },
    done(value, key = c.state.pending.key) {
      return c.event({ kind: 'action', key, event: { status: 'succeeded', value } });
    },
    page(start, count = 8, next = String(start + count)) {
      return c.done({ source, instances: instances(start, count), next });
    },
    scroll(revision, direction = 'down', options = {}) {
      return c.event({
        kind: 'component',
        child: 'deployment-list',
        name: 'observed',
        event: {
          readingRevision: revision,
          scroll: { readingRevision: revision, direction },
          viewport: {
            readingRevision: revision,
            atStart: direction === 'up',
            atEnd: direction === 'down',
            visibleChildKeys: [],
            ...options.viewport,
          },
          pendingCommand: options.pendingCommand ?? null,
        },
      });
    },
    checkpoint(sequence, state) {
      return c.event({
        kind: 'component',
        child: 'settings-checkpoint',
        name: 'changed',
        event: { sequence, state },
      });
    },
    query(text, revision = 1) {
      return c.event({
        kind: 'component',
        child: 'deployment-query',
        name: 'changed',
        event: {
          revision,
          snapshot: {
            text: { owner: 'query', revision, bytes: Buffer.byteLength(text) },
            marked: null,
          },
        },
      });
    },
  };
  return c;
}
test('bundled plugins describe their purpose in their own manifests', () => {
  const directory = new URL('../dist/packages/', import.meta.url);
  const packages = readdirSync(directory);
  assert.ok(packages.length > 50);
  for (const name of packages) {
    const manifest = JSON.parse(readFileSync(new URL(`${name}/maka.extension.json`, directory)));
    assert.ok(manifest.displayName?.trim(), name);
    assert.ok(manifest.description?.trim(), name);
    assert.ok(manifest.localizations?.['zh-CN']?.description?.trim(), name);
  }
});
for (const target of ['tui']) {
  test(`${target} plugin scrolling retains previous records and consumes only current physical boundaries`, async () => {
    const c = await client(target, 'zh-CN');
    c.page(0);
    c.checkpoint(0, { kind: 'ready', revision: null, value: null });
    assert.ok(
      nodes(c.out.frame.view).some(
        (node) => typeof node.text === 'string' && node.text.includes('用途 0'),
      ),
    );
    assert.equal(
      nodes(c.out.frame.view).some((node) =>
        ['deployment-next', 'deployment-previous'].includes(node.key),
      ),
      false,
    );
    c.scroll(1, 'down', { viewport: { readingRevision: 0 } });
    assert.deepEqual(c.out.actions, []);
    c.scroll(1, 'down', { pendingCommand: 8 });
    assert.deepEqual(c.out.actions, []);
    c.scroll(1);
    assert.deepEqual(c.out.actions[0].input, { kind: 'continue', source, after: '8' });
    c.page(8);
    assert.equal(c.state.page.instances.length, 16);
    c.scroll(1);
    assert.deepEqual(c.out.actions, [], 'layout repetition is not another scroll');
    for (let at = 16, revision = 2; at < 80; at += 8, revision++) {
      c.scroll(revision);
      // The existing window moves before another backend request is needed.
      if (!c.state.pending) c.scroll(revision + 100);
      c.page(at, 8, at + 8 < 80 ? String(at + 8) : null);
    }
    assert.equal(c.state.page.instances.length, 80);
    c.scroll(200);
    assert.ok(c.state.listWindow > 0);
    c.scroll(201, 'up');
    assert.equal(c.state.listWindow, 0);
    assert.ok(
      nodes(c.out.frame.view).some(
        (node) => node.key === 'deployment-select-0' || node.key === 'deployment-path-0',
      ),
    );
  });
  test(`${target} plugin search scans later data and matches the author's translated purpose`, async () => {
    const c = await client(target, 'zh-CN');
    c.page(0);
    c.checkpoint(0, { kind: 'ready', revision: null, value: null });
    c.query('土星');
    assert.equal(c.out.actions[0].binding, 'reader');
    c.done({ text: '土星', metadata: { owner: 'query', revision: 1, bytes: 6 } });
    for (let at = 8; at < 80; at += 8) {
      assert.equal(c.out.actions[0].method, 'instances');
      c.page(at, 8, at + 8 < 80 ? String(at + 8) : null);
    }
    const view = nodes(c.out.frame.view);
    assert.ok(
      view.some((node) => typeof node.text === 'string' && node.text.includes('记录土星观察笔记')),
    );
    assert.equal(
      view.some((node) => node.key === 'deployment-instance-0'),
      false,
    );
    assert.ok(view.some((node) => node.key === 'deployment-instance-79'));
    assert.deepEqual(c.out.actions, []);
  });
}
test('a failed preference read recovers automatically and repeated permanent failures stop', async () => {
  const c = await client('tui');
  c.page(0, 0, null);
  c.checkpoint(0, { kind: 'failed', diagnostic: 'temporary read failure' });
  assert.deepEqual(c.state.bookmark.command, { kind: 'read', sequence: 1 });
  c.checkpoint(1, { kind: 'ready', revision: null, value: null });
  assert.equal(c.state.bookmark.restored, true);
  assert.equal(c.state.bookmark.command, null);
  const failed = await client('tui');
  failed.page(0, 0, null);
  for (let sequence = 0; sequence <= 2; sequence++)
    failed.checkpoint(sequence, { kind: 'failed', diagnostic: 'storage unavailable' });
  assert.equal(
    failed.state.bookmark.command.sequence,
    2,
    'permanent failure does not create an infinite retry loop',
  );
  const view = nodes(failed.out.frame.view);
  assert.ok(view.some((node) => node.key === 'settings-save-message'));
  assert.ok(view.some((node) => node.key === 'settings-checkpoint-retry'));
});

for (const target of ['tui']) {
  test(`${target} directory search reads later entries and refresh replaces the accumulated list`, async () => {
    const { component } = await import(`../dist/packages/directory-picker/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false };
    let out = component.mount(props, {});
    const event = (input) => {
      out = component.event(out.frame.state, input, props, {});
    };
    const done = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    const directory = (name) => ({ name, path: '/' + name, selection: { path: '/' + name } });
    const root = directory('root');
    done({ kind: 'roots', roots: [root] });
    event({
      kind: 'component',
      child: 'directory-query',
      name: 'changed',
      event: {
        revision: 1,
        snapshot: { text: { owner: 'query', revision: 1, bytes: 0 }, marked: null },
      },
    });
    event({ kind: 'raw', key: out.frame.state.pageKey + '-0', event: {} });
    done({ text: '' });
    done({ kind: 'children', directory: root, children: [directory('first')], after: 'later' });
    event({
      kind: 'component',
      child: 'directory-query',
      name: 'changed',
      event: {
        revision: 2,
        snapshot: { text: { owner: 'query', revision: 2, bytes: 6 }, marked: null },
      },
    });
    done({ text: 'needle' });
    assert.equal(out.actions[0].input.after, 'later');
    done({ kind: 'children', directory: root, children: [directory('needle')], after: null });
    assert.equal(out.frame.state.page.children.length, 2);
    assert.ok(JSON.stringify(out.frame.view).includes('needle'));
    event({ kind: 'raw', key: 'refresh', event: {} });
    done({ kind: 'children', directory: root, children: [directory('replacement')], after: null });
    assert.deepEqual(
      out.frame.state.page.children.map((entry) => entry.name),
      ['replacement'],
    );
  });
}

test('tool detail scrolling has no capability cycle through the outer reading area', async () => {
  const { composition, instanceNames, packageNames } = await import(
    '../dist/authoring/composition.mjs'
  );
  for (const presentation of ['tui']) {
    const { ui } = composition({
      presentation,
      locale: 'en',
      scheme: 'light',
      hostRoot: '/fixture',
      hostDeployment: 'host',
      managementDeployment: 'management',
      keys: Object.fromEntries(instanceNames.map((name, index) => [name, `key-${index}`])),
      packages: Object.fromEntries(
        packageNames.map((name) => [name, { id: name, revision: 'a'.repeat(64) }]),
      ),
      agent: { tools: [], workspaceRoots: [] },
    });
    const visiting = new Set(),
      visited = new Set();
    function visit(name) {
      assert.ok(!visiting.has(name), `capability cycle at ${name}`);
      if (visited.has(name)) return;
      visiting.add(name);
      for (const targets of Object.values(ui.instances[name].bindings ?? {}))
        for (const target of targets)
          if (target.kind === 'instance' && target.path.length === 1) visit(target.path[0]);
      visiting.delete(name);
      visited.add(name);
    }
    for (const name of Object.keys(ui.instances)) visit(name);
  }
});
