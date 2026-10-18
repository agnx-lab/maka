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
import { build } from 'esbuild';
import { readFile } from 'node:fs/promises';

async function source(relative) {
  const result = await build({
    entryPoints: [new URL(relative, import.meta.url).pathname],
    bundle: true,
    write: false,
    format: 'esm',
    platform: 'node',
    logLevel: 'silent',
  });
  return import(
    'data:text/javascript;base64,' + Buffer.from(result.outputFiles[0].contents).toString('base64')
  );
}
const core = await source('../../../../plugins/shared/locale.ts');
const terminal = await source('../../../../plugins/shared/tui-locale.ts');
const help = await source('../../../../plugins/agent/help/locale.ts');
const screen = await source('../../../../plugins/agent/agent-screen/locale.ts');
const preferences = await source('../../../../plugins/shared/locale-catalogs/settings.ts');

test('feature catalogs preserve three languages and their locale fallback contracts', () => {
  for (const [locale, expected] of [
    ['en', 'Send message'],
    ['zh-CN', '发送消息'],
    ['zh-TW', '傳送訊息'],
    ['ZH-Hant-HK', '傳送訊息'],
    ['zh-HK', '傳送訊息'],
    ['zh-MO', '傳送訊息'],
    ['zh-SG', '发送消息'],
    ['fr', 'Send message'],
    ['', 'Send message'],
    ['zh_TW', '发送消息'],
  ])
    assert.equal(help.copy(locale).helpSend, expected, locale);
  assert.equal(preferences.tuiText('zh_TW', 'settings-language'), '語言');
  assert.equal(core.resolveLocale('zh-TW', 'en'), 'zh-TW');
  assert.equal(core.resolveLocale('auto', 'zh-CN'), 'zh-CN');
  assert.equal(core.resolveLocale('auto', null), 'en');
  assert.equal(help.copy('zh-HK'), help.copy('zh-TW'));
  assert.equal(help.copy('en'), help.copy('en-US'));
  assert.ok(Object.isFrozen(help.copy('en')));
});

test('terminal translations interpolate values and preserve unknown keys and placeholders', () => {
  const messages = {
    message: [
      'Value { $value }, { $missing }',
      '值 { $value }，{ $missing }',
      '值 { $value }，{ $missing }',
    ],
  };
  assert.equal(
    terminal.translate('en', messages, 'message', { value: 0 }),
    'Value 0, { $missing }',
  );
  assert.equal(
    terminal.translate('zh_TW', messages, 'message', { value: '甲' }),
    '值 甲，{ $missing }',
  );
  assert.equal(terminal.translate('en', messages, 'unknown'), 'unknown');
  assert.equal(
    preferences.tuiText('en', 'language-auto', { language: 'zh-TW' }),
    'Automatic (zh-TW)',
  );
});

test('closed runtime states resolve to translated feature messages', () => {
  for (const [locale, expected] of [
    ['en', ['Awaiting approval', 'Executing Plan', 'In progress', 'Stopping', 'Paused']],
    ['zh-CN', ['等待批准', '正在执行计划', '进行中', '停止中', '已暂停']],
    ['zh-TW', ['等待批准', '正在執行計劃', '進行中', '停止中', '已暫停']],
  ]) {
    const text = screen.copy(locale);
    assert.deepEqual(
      [
        text[screen.planProposals.pending_approval],
        text[screen.planExecutions.active],
        text[screen.planSteps.in_progress],
        text[screen.graphStatuses.closing],
        text[screen.goalStatuses.paused],
      ],
      expected,
    );
    for (const map of [
      screen.planProposals,
      screen.planExecutions,
      screen.planSteps,
      screen.planActions,
      screen.goalStatuses,
      screen.goalActions,
      screen.graphStatuses,
    ])
      for (const key of Object.values(map)) assert.equal(typeof text[key], 'string');
  }
});

test('feature-owned dynamic notices render in all three locales', async () => {
  for (const target of ['tui']) {
    const { component } = await import(`../dist/packages/recall/${target}.js`);
    const { activate } = await import(`../dist/packages/appearance/${target}.js`);
    const theme = activate({ scheme: 'light' }).Appearance.value;
    const nodes = (value) =>
      value && typeof value === 'object'
        ? [
            value,
            ...Object.values(value).flatMap((child) =>
              Array.isArray(child) ? child.flatMap(nodes) : nodes(child),
            ),
          ]
        : [];
    for (const [locale, expected] of [
      ['en', 'History search is unavailable. Try again.'],
      ['zh-CN', '暂时无法搜索历史，请重试。'],
      ['zh-TW', '暫時無法搜尋歷史，請重試。'],
    ]) {
      const props = { theme, locale, open: true, busy: false };
      let output = component.mount(props, {});
      output.frame.state.notice = 'recallUnavailable';
      output = component.update(output.frame.state, props, {});
      assert.equal(
        nodes(output.frame.view).find((node) => node.key === 'recall-notice').text,
        expected,
      );
    }
  }
});

test('built Help payload carries only the feature messages', async () => {
  for (const target of ['tui']) {
    const bytes = await readFile(new URL(`../dist/packages/help/${target}.js`, import.meta.url));
    assert.ok(bytes.byteLength < 50_000, `${target}: ${bytes.byteLength} bytes`);
    const { component } = await import(`../dist/packages/help/${target}.js`);
    const { activate } = await import(`../dist/packages/appearance/${target}.js`);
    const theme = activate({ scheme: 'light' }).Appearance.value;
    const nodes = (value) =>
      value && typeof value === 'object'
        ? [
            value,
            ...Object.values(value).flatMap((child) =>
              Array.isArray(child) ? child.flatMap(nodes) : nodes(child),
            ),
          ]
        : [];
    for (const [locale, label] of [
      ['en', 'Send message'],
      ['zh-CN', '发送消息'],
      ['zh-TW', '傳送訊息'],
    ]) {
      const output = component.mount({ theme, locale, open: true });
      assert.equal(
        nodes(output.frame.view).find((node) => node.key === 'shortcut-description-0').text,
        label,
      );
    }
  }
});

test('appearance checkpoint ignores its former locale while preserving and saving custom themes', async () => {
  const appearanceEditor = await source('../../../../plugins/shared/appearance-editor.ts');
  const settings = await source('../../../../plugins/shared/settings.ts');
  const { appearance } = await source('../../../../plugins/settings/appearance/index.ts');
  const { presentations } = await source('../../../../plugins/shared/view.ts');
  const base = appearance('dark', true, { scheme: 'dark', theme: 'maka' });
  const state = appearanceEditor.initialAppearance();
  const customTheme = {
    version: 1,
    name: 'My retained palette',
    base: 'dusk',
    colors: { accent: '#123456' },
    syntax: { comment: '#778899' },
  };
  const savedAppearance = {
    scheme: 'dark',
    theme: 'custom',
    customTheme,
    ascii: false,
  };
  appearanceEditor.appearanceEvent(
    state,
    {
      kind: 'component',
      child: 'appearance-checkpoint',
      name: 'changed',
      event: {
        sequence: 0,
        state: {
          kind: 'ready',
          revision: 8,
          value: { version: 1, appearance: savedAppearance, locale: 'zh-TW' },
        },
      },
    },
    base,
    'en',
    [],
  );
  assert.deepEqual(state.saved, { version: 1, appearance: savedAppearance });
  assert.equal(
    appearanceEditor.effectiveAppearance(state, base).configuration.customTheme.name,
    'My retained palette',
  );
  const settingsState = {
    preferences: state,
    settingsCategory: 'appearance',
    pages: [],
  };
  settings.settingsEvent(
    settingsState,
    { kind: 'raw', key: 'settings-motion', event: { key: 'off' } },
    base,
  );
  assert.deepEqual(state.command, {
    kind: 'write',
    sequence: 1,
    expectedRevision: 8,
    value: { version: 1, appearance: { ...savedAppearance, motion: false } },
  });
  const view = appearanceEditor.appearanceView(presentations.tui, state, base, 'en', {});
  const all = (node) =>
    node && typeof node === 'object' ? [node, ...Object.values(node).flatMap(all)] : [];
  const checkpoint = all(view).find((node) => node.key === 'appearance-checkpoint');
  assert.deepEqual(checkpoint.props.command.value, {
    version: 1,
    appearance: { ...savedAppearance, motion: false },
  });
  assert.equal(
    all(view).some((node) => node.key === 'appearance-language'),
    false,
  );
});
