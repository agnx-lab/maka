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

// Production Settings, locale and receipt handling run unchanged. Only native
// input state, element builders and the Host boundary are supplied by the test.
const nativeModules = {
  'gpui-kit': `const ui = (...args) => globalThis[Symbol.for('language.test.element')](...args);
    export { ui as div, ui as list, ui as svg, ui as uniform_list, ui as PathBuilder };
    export class View {}`,
  'gpui-base': `const ui = (...args) => globalThis[Symbol.for('language.test.element')](...args);
    const state = { new: ({placeholder}) => ({
      placeholder, on() {}, set_masked() {}, set_auto_grow() {}, value: () => '',
      set_placeholder(value) { this.placeholder = value; }
    }) };
    export { state as InputState, state as TextareaState };
    export const Button = { new: ui };
    export const VirtualListScrollHandle = { new: () => ({ scroll_to_item() {}, release() { return true; } }) };
    export { ui as v_flex, ui as h_flex, ui as TextView, ui as Scrollbar, ui as v_virtual_list,
      ui as set_theme, ui as Input, ui as Checkbox, ui as Switch,
      ui as Popover, ui as Textarea, ui as h_resizable, ui as resizable_panel };`,
  'maka-locale': `export const system = () => 'en';`,
  'maka-files': `const unexpected = () => { throw new Error('unexpected native file call'); };
    export { unexpected as openText, unexpected as chooseFolder, unexpected as chooseFiles,
      unexpected as release, unexpected as open, unexpected as reveal };`,
  'maka-desktop': `export const Terminal = {}; export const TerminalState = {};`,
  'maka-host': `export const query = (...args) => globalThis[Symbol.for('language.test.query')](...args);
    export const mutate = (...args) => globalThis[Symbol.for('language.test.mutate')](...args);
    const unexpected = () => { throw new Error('unexpected native subscription'); };
    export { unexpected as subscribe, unexpected as next, unexpected as close };`,
};
const hooks = registerHooks({
  resolve(specifier, context, nextResolve) {
    if (Object.hasOwn(nativeModules, specifier)) {
      return { url: `language-test:${specifier}`, shortCircuit: true };
    }
    return nextResolve(specifier, context);
  },
  load(url, context, nextLoad) {
    if (url.startsWith('language-test:')) {
      return { format: 'module', source: nativeModules[url.slice(14)], shortCircuit: true };
    }
    return nextLoad(url, context);
  },
});
const { Settings, settingsPage } = await import(
  '../../products/maka/desktop-shell/app/settings.js'
);
const { default: MakaDesktop } = await import('../../products/maka/desktop-shell/app/main.js');
const { language, t } = await import('../../products/maka/desktop-shell/app/locale.js');
hooks.deregister();

function setup(preferences) {
  const requests = [];
  const reports = [];
  const read = async (endpoint, method, input) => {
    requests.push({ endpoint, method, input });
    assert.equal(endpoint, 'settings');
    switch (method) {
      case 'assistantPreferences':
        return preferences;
      case 'connections':
        return { kind: 'ready', revision: 0, connections: [], next: null };
      case 'models':
        return {
          kind: 'ready',
          revision: 0,
          choices: [],
          defaultModel: { kind: 'none' },
          next: null,
        };
      default:
        throw new Error(`unexpected query ${method}`);
    }
  };
  globalThis[Symbol.for('language.test.query')] = read;
  globalThis[Symbol.for('language.test.mutate')] = () => {
    throw new Error('unexpected mutation');
  };
  const settings = new Settings({
    changed() {},
    report: (message) => reports.push(message),
    modelsChanged() {},
    presetsChanged() {},
  });
  return { settings, requests, reports, read };
}

test('opening Settings uses the Host language for labels and retained inputs', async () => {
  const { settings, requests, reports } = setup({
    revision: 7,
    language: null,
    resolvedLanguage: 'zh-TW',
  });
  await settings.load();
  assert.equal(language(), 'zh-TW');
  assert.equal(t('newTask'), '新任務');
  assert.equal(settings.modelQuery.placeholder, '搜尋模型');
  assert.equal(settings.preferences.language, null);
  assert.deepEqual(
    requests.filter((request) => request.method === 'assistantPreferences'),
    [{ endpoint: 'settings', method: 'assistantPreferences', input: {} }],
  );
  assert.deepEqual(reports, []);
});

function deferred() {
  let resolve;
  const promise = new Promise((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function returned(value) {
  return { receipt: { kind: 'recorded', outcome: { kind: 'returned', value } } };
}

test('save sends one CAS mutation and applies only the confirmed readback', async () => {
  const { settings } = setup({ revision: 10, language: 'en', resolvedLanguage: 'en' });
  await settings.loadPreferences();
  const mutation = deferred();
  const reading = deferred();
  const snapshot = deferred();
  const mutations = [];
  globalThis[Symbol.for('language.test.mutate')] = (...args) => {
    mutations.push(args);
    return mutation.promise;
  };
  globalThis[Symbol.for('language.test.query')] = () => {
    reading.resolve();
    return snapshot.promise;
  };
  const saving = settings.setLanguage('zh-TW');
  await settings.setLanguage('zh-CN');
  assert.equal(language(), 'en');
  assert.equal(settings.preferences.language, 'en');
  mutation.resolve(returned({ kind: 'committed', revision: 11 }));
  await reading.promise;
  assert.equal(t('newTask'), 'New task');
  assert.equal(settings.preferences.revision, 10);
  snapshot.resolve({ revision: 11, language: 'zh-TW', resolvedLanguage: 'zh-TW' });
  await saving;
  assert.equal(t('newTask'), '新任務');
  assert.equal(settings.modelQuery.placeholder, '搜尋模型');
  assert.equal(settings.preferences.language, 'zh-TW');
  assert.deepEqual(mutations, [
    [
      'settings',
      'updateAssistantPreferences',
      { expectedRevision: 10, mutation: { kind: 'language', preference: 'zh-TW' } },
      undefined,
    ],
  ]);
  assert.equal(settings.busy, null);
  assert.equal(settings.error, null);
});

test('conflict refreshes the actual Host value without replaying the mutation', async () => {
  const { settings, reports } = setup({ revision: 20, language: 'en', resolvedLanguage: 'en' });
  await settings.loadPreferences();
  const mutations = [];
  globalThis[Symbol.for('language.test.mutate')] = (...args) => {
    mutations.push(args);
    return returned({ kind: 'revision_conflict', expectedRevision: 20, actualRevision: 21 });
  };
  globalThis[Symbol.for('language.test.query')] = async () => ({
    revision: 21,
    language: 'auto',
    resolvedLanguage: 'en',
  });
  await settings.setLanguage('zh-CN');
  assert.deepEqual(settings.preferences, {
    revision: 21,
    language: 'auto',
    resolvedLanguage: 'en',
  });
  assert.equal(t('newTask'), 'New task');
  assert.deepEqual(reports, [
    'Settings: Preferences changed elsewhere. The current language was reloaded; choose again to save.',
  ]);
  assert.deepEqual(
    mutations.map((args) => args[2]),
    [{ expectedRevision: 20, mutation: { kind: 'language', preference: 'zh-CN' } }],
  );
});

for (const [name, response, error] of [
  ['rejected', returned({ kind: 'rejected' }), 'Settings: The Host rejected the language change.'],
  [
    'unknown receipt',
    { receipt: { kind: 'unknown' } },
    'Settings: settings.updateAssistantPreferences was not recorded as returned: {"kind":"unknown"}',
  ],
  ['failed readback', returned({ kind: 'committed', revision: 31 }), 'Settings: read unavailable'],
]) {
  test(`${name} preserves the confirmed language and reports failure`, async () => {
    const { settings, reports } = setup({ revision: 30, language: 'en', resolvedLanguage: 'en' });
    await settings.loadPreferences();
    const mutations = [];
    globalThis[Symbol.for('language.test.mutate')] = (...args) => {
      mutations.push(args);
      return response;
    };
    globalThis[Symbol.for('language.test.query')] = () => {
      throw new Error('read unavailable');
    };
    await settings.setLanguage('auto');
    assert.equal(language(), 'en');
    assert.deepEqual(settings.preferences, {
      revision: 30,
      language: 'en',
      resolvedLanguage: 'en',
    });
    assert.deepEqual(reports, [error]);
    assert.deepEqual(
      mutations.map((args) => args[2]),
      [{ expectedRevision: 30, mutation: { kind: 'language', preference: 'auto' } }],
    );
  });
}

test('late read cannot roll back a newer Host snapshot', async () => {
  const { settings } = setup({ revision: 40, language: 'en', resolvedLanguage: 'en' });
  await settings.loadPreferences();
  const old = deferred();
  globalThis[Symbol.for('language.test.query')] = () => old.promise;
  const pending = settings.loadPreferences();
  globalThis[Symbol.for('language.test.query')] = async () => ({
    revision: 42,
    language: 'auto',
    resolvedLanguage: 'zh-CN',
  });
  await settings.loadPreferences();
  old.resolve({ revision: 41, language: 'en', resolvedLanguage: 'en' });
  await pending;
  assert.deepEqual(settings.preferences, {
    revision: 42,
    language: 'auto',
    resolvedLanguage: 'zh-CN',
  });
  assert.equal(t('newTask'), '新任务');
});

test('the production startup reads Host language before starting conversation loads', async () => {
  const { settings, requests } = setup({
    revision: 50,
    language: 'auto',
    resolvedLanguage: 'zh-TW',
  });
  const labels = [];
  const app = {
    settings,
    rootFocus: { focus() {} },
    composer: { loadModel: () => labels.push(t('newTask')) },
    directories: { loadRoots() {} },
    catalog: { run() {} },
  };
  await MakaDesktop.prototype.start.call(app, { bind_keys() {}, spawn() {} });
  assert.deepEqual(labels, ['新任務']);
  assert.deepEqual(requests, [{ endpoint: 'settings', method: 'assistantPreferences', input: {} }]);
});

test('reopening Settings reads another client’s language without local storage', async () => {
  const { settings } = setup({ revision: 60, language: 'zh-TW', resolvedLanguage: 'zh-TW' });
  await settings.load();
  const finished = deferred();
  const { read } = setup({ revision: 61, language: 'en', resolvedLanguage: 'en' });
  globalThis[Symbol.for('language.test.query')] = read;
  settings.hooks.changed = () => {
    if (!settings.loading && settings.preferences?.revision === 61) finished.resolve();
  };
  settings.show();
  await finished.promise;
  assert.equal(settings.open, true);
  assert.equal(settings.preferences.language, 'en');
  assert.equal(settings.modelQuery.placeholder, 'Find a model');
  assert.equal(t('newTask'), 'New task');
});

// Native element builders record the callback/disabled state, not pixels or
// layout. All Settings rendering and event wiring remains production code.
function elements() {
  const buttons = new Map();
  const labels = [];
  globalThis[Symbol.for('language.test.element')] = (id) => {
    const value = { disabled: false, click: null };
    let element;
    element = new Proxy(value, {
      get(target, name) {
        if (name === 'when') return (condition, render) => (condition ? render(element) : element);
        if (name === 'child') {
          return (child) => {
            if (typeof child === 'string') labels.push(child);
            return element;
          };
        }
        if (name === 'disabled') {
          return (disabled) => {
            target.disabled = disabled;
            return element;
          };
        }
        if (name === 'on_click') {
          return (click) => {
            target.click = click;
            return element;
          };
        }
        return () => element;
      },
    });
    if (typeof id === 'string') buttons.set(id, value);
    return element;
  };
  return { buttons, labels };
}

test('the General language button uses Host CAS; inherited default is not saved auto', async () => {
  const { settings } = setup({ revision: 70, language: null, resolvedLanguage: 'zh-TW' });
  const { buttons, labels } = elements();
  const cx = { theme: () => ({ is_dark: false }) };
  settingsPage(settings, cx);
  assert.equal(buttons.get('language-auto').disabled, true);
  await settings.load();
  settingsPage(settings, cx);
  assert.equal(buttons.get('language-auto').disabled, false);
  assert.deepEqual(
    labels.filter((label) => label.endsWith('\nHost 預設：繁體中文')),
    ['儲存到目前連線的 Host，用於介面與新任務。已受理的任務保留原語言。\nHost 預設：繁體中文'],
  );
  const mutations = [];
  globalThis[Symbol.for('language.test.mutate')] = (...args) => {
    mutations.push(args);
    return returned({ kind: 'committed', revision: 71 });
  };
  globalThis[Symbol.for('language.test.query')] = async () => ({
    revision: 71,
    language: 'auto',
    resolvedLanguage: 'zh-CN',
  });
  await buttons.get('language-auto').click();
  assert.deepEqual(
    mutations.map((args) => args[2]),
    [{ expectedRevision: 70, mutation: { kind: 'language', preference: 'auto' } }],
  );
  assert.equal(settings.preferences.language, 'auto');
  assert.equal(t('newTask'), '新任务');
});
