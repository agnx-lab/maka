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
import { activate, component } from '../dist/packages/agent-screen/tui.js';
import { activate as appearance } from '../dist/packages/appearance/tui.js';
activate({});
const values = { appearance: [appearance({ scheme: 'dark' }).Appearance.value] };
const saved = {
  revision: 12,
  language: 'en',
  resolvedLanguage: 'en',
  displayName: '',
  assistantTone: '',
  compactionPrompt: null,
  defaultCompactionPrompt: 'Default',
  workspaceInstructions: true,
  incognitoActive: false,
  shellTitles: true,
};
const reference = {
  contract: { name: 'maka.settings.Settings', version: 1 },
  method: 'updateAssistantPreferences',
  operation: 'original-language-save',
};
const recorded = (value) => ({
  reference,
  receipt: {
    kind: 'recorded',
    cleanup: { kind: 'confirmed' },
    outcome: { kind: 'returned', value },
  },
});
const nodes = (node) =>
  node && typeof node === 'object' ? [node, ...Object.values(node).flatMap(nodes)] : [];
function fixture() {
  let out = component.mount({}, values);
  const f = {
    get out() {
      return out;
    },
    event(input) {
      out = component.event(out.frame.state, input, {}, values);
      return out;
    },
    raw(key, event = {}) {
      return f.event({ kind: 'raw', key, event });
    },
    action(method) {
      const a = out.actions.find((a) => a.method === method);
      assert.ok(a, method);
      return a;
    },
    reply(action, event) {
      return f.event({ kind: 'action', key: action.key, event });
    },
    finish(method, value) {
      return f.reply(f.action(method), { status: 'succeeded', value });
    },
    finishHistory() {
      const entry = Object.entries(out.frame.state.pending).find(
        ([, request]) => request.kind === 'restorePage',
      );
      assert.ok(entry, 'pending history checkpoint');
      return f.reply({ key: entry[0] }, { status: 'succeeded', value: [] });
    },
    selection() {
      return nodes(out.frame.view).find((n) => n.key === 'settings-language');
    },
    select(key) {
      f.raw('settings-language', { key });
    },
    ready() {
      f.finish('assistantPreferences', saved);
      f.event({
        kind: 'component',
        child: 'recovery-checkpoint',
        name: 'changed',
        event: {
          sequence: 0,
          state: { kind: 'ready', revision: 0, value: null },
        },
      });
      f.finish('page', []);
      f.finish('assistantPreferences', saved);
      f.raw('settings');
      f.finish('assistantPreferences', saved);
      return f;
    },
  };
  return f;
}
test('Appearance language loads, saves through the native revision and publishes only a confirmed read', () => {
  const f = fixture().ready();
  assert.equal(f.out.frame.state.settingsCategory, 'appearance');
  assert.equal(f.selection().current, 'en');
  assert.equal(f.selection().options[0].label, 'Auto (system language)');
  assert.equal(f.selection().disabled, false);
  f.raw('settings-category-appearance');
  assert.equal(f.selection().label, 'Loading…');
  assert.equal(f.selection().disabled, true);
  f.finish('assistantPreferences', saved);
  f.select('zh-TW');
  const write = f.action('updateAssistantPreferences');
  assert.deepEqual(write.input, {
    expectedRevision: 12,
    mutation: { kind: 'language', preference: 'zh-TW' },
  });
  assert.equal(f.selection().current, 'en');
  assert.equal(f.selection().disabled, true);
  assert.deepEqual(f.out.events, []);
  f.reply(write, { status: 'operation', reply: recorded({ kind: 'committed', revision: 13 }) });
  assert.equal(f.selection().current, 'en');
  assert.deepEqual(f.out.events, []);
  f.finish('assistantPreferences', {
    ...saved,
    revision: 13,
    language: 'zh-TW',
    resolvedLanguage: 'zh-TW',
  });
  assert.equal(f.selection().current, 'zh-TW');
  assert.deepEqual(f.out.events, [{ name: 'languageChanged', value: {} }]);
  // A held background checkpoint must complete before another language write.
  assert.equal(f.selection().disabled, true);
  f.select('en');
  assert.equal(
    f.out.actions.some((a) => a.method === 'updateAssistantPreferences'),
    false,
  );
  f.finishHistory();
  assert.equal(f.selection().label, '語言');
  assert.equal(f.selection().disabled, false);
  f.select('en');
  assert.equal(f.action('updateAssistantPreferences').input.expectedRevision, 13);
});
test('language revision conflict rereads the basis and never replays the write', () => {
  const f = fixture().ready();
  f.select('zh-CN');
  f.reply(f.action('updateAssistantPreferences'), {
    status: 'operation',
    reply: recorded({ kind: 'revision_conflict', actualRevision: 19 }),
  });
  const read = f.action('assistantPreferences');
  f.select('zh-TW');
  assert.equal(
    f.out.actions.some((a) => a.method === 'updateAssistantPreferences'),
    false,
  );
  f.reply(read, {
    status: 'succeeded',
    value: { ...saved, revision: 19, language: 'auto', resolvedLanguage: 'zh-CN' },
  });
  assert.equal(f.selection().current, 'auto');
  f.finishHistory();
  f.select('zh-TW');
  assert.equal(f.action('updateAssistantPreferences').input.expectedRevision, 19);
});
test('a lost language reply restores the original operation and blocks duplicate writes', () => {
  const f = fixture().ready();
  f.select('zh-CN');
  f.reply(f.action('updateAssistantPreferences'), { status: 'failed', diagnostic: 'lost reply' });
  const page = f.action('page');
  assert.equal(f.selection().disabled, true);
  f.select('zh-TW');
  assert.equal(
    f.out.actions.some((a) => a.method === 'updateAssistantPreferences'),
    false,
  );
  f.reply(page, {
    status: 'succeeded',
    value: [
      {
        cursor: 3,
        reference,
        caller: { stateDomain: 'root', deployment: 'app' },
        consumer: 'screen',
      },
    ],
  });
  assert.deepEqual(f.action('read').input, reference);
  f.finish('read', { reference, receipt: { kind: 'unknown' } });
  assert.equal(f.selection().disabled, true);
  f.raw('settings-language-check');
  assert.deepEqual(f.action('read').input, reference);
  f.finish('read', recorded({ kind: 'committed', revision: 13 }));
  const nextPage = f.action('page');
  const read = f.action('assistantPreferences');
  f.reply(read, {
    status: 'succeeded',
    value: { ...saved, revision: 13, language: 'zh-CN', resolvedLanguage: 'zh-CN' },
  });
  assert.deepEqual(f.out.events, [{ name: 'languageChanged', value: {} }]);
  f.reply(nextPage, { status: 'succeeded', value: [] });
  f.finish('assistantPreferences', {
    ...saved,
    revision: 13,
    language: 'zh-CN',
    resolvedLanguage: 'zh-CN',
  });
  f.finishHistory();
  assert.equal(f.selection().current, 'zh-CN');
  assert.equal(f.selection().disabled, false);
});
test('restart recovers a returned language receipt and refreshes native state before allowing edits', () => {
  const f = fixture();
  f.finish('assistantPreferences', saved);
  f.event({
    kind: 'component',
    child: 'recovery-checkpoint',
    name: 'changed',
    event: { sequence: 0, state: { kind: 'ready', revision: 0, value: null } },
  });
  f.finish('page', [
    {
      cursor: 3,
      reference,
      caller: { stateDomain: 'root', deployment: 'app' },
      consumer: 'screen',
    },
  ]);
  f.finish('read', recorded({ kind: 'committed', revision: 13 }));
  f.finish('page', []);
  f.finish('assistantPreferences', {
    ...saved,
    revision: 13,
    language: 'zh-CN',
    resolvedLanguage: 'zh-CN',
  });
  assert.equal(f.out.frame.state.language.observed.language, 'zh-CN');
  assert.deepEqual(f.out.events, [{ name: 'languageChanged', value: {} }]);
});
test('failed language reads can retry and not-accepted writes do not start receipt recovery', () => {
  const f = fixture().ready();
  f.raw('settings-category-appearance');
  f.reply(f.action('assistantPreferences'), { status: 'failed', diagnostic: 'unavailable' });
  assert.equal(f.selection().disabled, true);
  assert.equal(
    nodes(f.out.frame.view).find((n) => n.key === 'settings-language-check').disabled,
    false,
  );
  f.raw('settings-language-check');
  f.finish('assistantPreferences', saved);
  f.select('zh-CN');
  f.reply(f.action('updateAssistantPreferences'), {
    status: 'not_accepted',
    diagnostic: 'unavailable',
  });
  assert.equal(
    f.out.actions.some((a) => a.binding === 'history'),
    false,
  );
  assert.equal(f.selection().disabled, false);
  assert.equal(f.selection().current, 'en');
});
