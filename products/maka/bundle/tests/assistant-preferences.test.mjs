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
import Ajv from 'ajv/dist/2020.js';
const { activate } = await import('../dist/packages/appearance/tui.js');
const theme = activate({ scheme: 'light' }).Appearance.value;
const valid = new Ajv({ strict: false, formats: { uint64: true } }).compile(
  contracts.settings.interface.methods.updateAssistantPreferences.input,
);
for (const target of ['tui'])
  test(`Assistant ${target} mounts independently addressable preference editors`, async () => {
    const { component } = await import(`../dist/packages/assistant-preferences/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    const out = component.mount(props, {});
    const editors = new Set();
    const visit = (node) => {
      if (!node || typeof node !== 'object') return;
      if (node.kind === 'component') {
        assert.ok(!editors.has(node.key), `duplicate editor identity: ${node.key}`);
        editors.add(node.key);
      }
      Object.values(node).forEach(visit);
    };
    visit(out.frame.view);
    assert.deepEqual(
      editors,
      new Set(['assistant-name', 'assistant-global', 'assistant-compaction']),
    );
  });

for (const target of ['tui'])
  test(`Assistant ${target} privacy recovers the original operation without overwriting editor drafts`, async () => {
    const { component } = await import(`../dist/packages/assistant-preferences/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    const saved = {
      revision: 4,
      displayName: 'Saved',
      assistantTone: '',
      compactionPrompt: 'Default compression prompt',
      defaultCompactionPrompt: 'Default compression prompt',
      workspaceInstructions: true,
      incognitoActive: false,
      shellTitles: true,
      language: 'en',
      resolvedLanguage: 'en',
    };
    finish([]);
    finish(saved);
    const fields = structuredClone(out.frame.state.fields);
    raw('privacy');
    const action = out.actions[0];
    assert.ok(valid(action.input), JSON.stringify(valid.errors));
    assert.deepEqual(action.input, {
      expectedRevision: 4,
      mutation: { kind: 'privacy', incognitoActive: true },
    });
    event({
      kind: 'action',
      key: action.key,
      event: { status: 'failed', diagnostic: 'response interrupted' },
    });
    raw('privacy');
    assert.equal(out.actions.length, 0);
    raw('history');
    const reference = {
      contract: { name: 'maka.settings.Settings', version: 1 },
      method: 'updateAssistantPreferences',
      operation: 'original-privacy-toggle',
    };
    finish([{ cursor: 2, reference, recovery: action.recovery }]);
    finish([]);
    assert.equal(out.actions[0].method, 'read');
    assert.deepEqual(out.actions[0].input, reference);
    finish({
      reference,
      receipt: {
        kind: 'recorded',
        cleanup: { kind: 'confirmed' },
        outcome: { kind: 'returned', value: { kind: 'committed', revision: 5 } },
      },
    });
    assert.equal(out.actions[0].method, 'assistantPreferences');
    finish({ ...saved, revision: 6, incognitoActive: true, displayName: 'Changed elsewhere' });
    assert.equal(out.frame.state.saved.revision, 6);
    assert.deepEqual(
      out.frame.state.fields,
      fields,
      'a policy toggle must retain the existing editor state',
    );
  });

for (const target of ['tui'])
  test(`Assistant ${target} shell titles toggle saves the observed preference revision`, async () => {
    const { component } = await import(`../dist/packages/assistant-preferences/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const finish = (value) => {
      out = component.event(
        out.frame.state,
        { kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } },
        props,
        {},
      );
    };
    finish([]);
    const saved = {
      revision: 7,
      displayName: '',
      assistantTone: '',
      compactionPrompt: 'Default compression prompt',
      defaultCompactionPrompt: 'Default compression prompt',
      workspaceInstructions: true,
      incognitoActive: false,
      shellTitles: true,
      language: 'en',
      resolvedLanguage: 'en',
    };
    finish(saved);
    const toggle = () => {
      out = component.event(
        out.frame.state,
        { kind: 'raw', key: 'shell-titles', event: {} },
        props,
        {},
      );
    };
    toggle();
    assert.deepEqual(
      out.actions.map(({ method, input }) => ({ method, input })),
      [
        {
          method: 'updateAssistantPreferences',
          input: { expectedRevision: 7, mutation: { kind: 'shell_titles', enabled: false } },
        },
      ],
    );
    assert.ok(valid(out.actions[0].input), JSON.stringify(valid.errors));
    out = component.event(
      out.frame.state,
      {
        kind: 'action',
        key: out.actions[0].key,
        event: {
          status: 'operation',
          reply: {
            reference: {
              contract: { name: 'maka.settings.Settings', version: 1 },
              method: 'updateAssistantPreferences',
              operation: 'shell-title-toggle',
            },
            receipt: {
              kind: 'recorded',
              cleanup: { kind: 'confirmed' },
              outcome: { kind: 'returned', value: { kind: 'committed', revision: 8 } },
            },
          },
        },
      },
      props,
      {},
    );
    finish({ ...saved, revision: 8, shellTitles: false });
    toggle();
    assert.deepEqual(out.actions[0].input, {
      expectedRevision: 8,
      mutation: { kind: 'shell_titles', enabled: true },
    });
  });

for (const target of ['tui'])
  test(`Assistant ${target} saves a custom compression prompt and restores the built-in default`, async () => {
    const { component } = await import(`../dist/packages/assistant-preferences/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({ kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } });
    const observed = (text, revision, receipt = null) => ({
      revision,
      snapshot: {
        text: { owner: 'prompt', revision, bytes: Buffer.byteLength(text) },
        marked: null,
      },
      receipt,
    });
    const field = (value) =>
      event({ kind: 'component', child: 'assistant-compaction', name: 'changed', event: value });
    const saved = {
      revision: 4,
      displayName: 'Saved',
      assistantTone: '',
      workspaceInstructions: true,
      incognitoActive: false,
      shellTitles: true,
      language: 'en',
      resolvedLanguage: 'en',
      compactionPrompt: 'Built-in instructions',
      defaultCompactionPrompt: 'Built-in instructions',
    };
    finish([]);
    finish(saved);
    field(observed('', 0));
    const initial = out.frame.state.fields.compaction.command;
    assert.deepEqual(initial.edit, { kind: 'reset', text: 'Built-in instructions' });
    field(
      observed('Built-in instructions', 1, {
        owner: initial.owner,
        sequence: initial.sequence,
        outcome: { status: 'applied' },
      }),
    );
    const draft = observed('Keep 中文 decisions', 2);
    field(draft);
    raw('save-compaction');
    assert.equal(out.actions[0].resources.text.child, 'assistant-compaction');
    finish({ metadata: draft.snapshot.text, text: 'Keep 中文 decisions' });
    assert.deepEqual(out.actions[0].input, {
      expectedRevision: 4,
      mutation: { kind: 'compaction_prompt', prompt: 'Keep 中文 decisions' },
    });
    assert.equal(valid(out.actions[0].input), true, JSON.stringify(valid.errors));
    const commit = (revision) =>
      event({
        kind: 'action',
        key: out.actions[0].key,
        event: {
          status: 'operation',
          reply: {
            reference: {
              contract: { name: 'maka.settings.Settings', version: 1 },
              method: 'updateAssistantPreferences',
              operation: `prompt-${revision}`,
            },
            receipt: {
              kind: 'recorded',
              outcome: { kind: 'returned', value: { kind: 'committed', revision } },
            },
          },
        },
      });
    const unrelated = structuredClone(out.frame.state.fields.name);
    commit(5);
    finish({ ...saved, revision: 5, compactionPrompt: 'Keep 中文 decisions' });
    assert.deepEqual(out.frame.state.fields.name, unrelated);
    const savedReset = out.frame.state.fields.compaction.command;
    field(
      observed(savedReset.edit.text, 3, {
        owner: savedReset.owner,
        sequence: savedReset.sequence,
        outcome: { status: 'applied' },
      }),
    );
    raw('reset-compaction');
    assert.deepEqual(out.actions[0].input, {
      expectedRevision: 5,
      mutation: { kind: 'compaction_prompt', prompt: null },
    });
    commit(6);
    finish({ ...saved, revision: 6 });
    const pending = out.frame.state.fields.compaction.command;
    assert.deepEqual(pending.edit, { kind: 'reset', text: 'Built-in instructions' });
    field(
      observed(pending.edit.text, 4, {
        owner: pending.owner,
        sequence: pending.sequence,
        outcome: { status: 'applied' },
      }),
    );
    assert.equal(out.frame.state.saved.compactionPrompt, 'Built-in instructions');
  });

for (const target of ['tui']) {
  const globalFixture = async () => {
    const { component } = await import(`../dist/packages/assistant-preferences/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({
        kind: 'action',
        key: out.frame.state.global.pending.key,
        event: { status: 'succeeded', value },
      });
    const observed = (content, revision, receipt = null, owner = 'global-editor') => ({
      revision,
      snapshot: { text: { owner, revision, bytes: Buffer.byteLength(content) }, marked: null },
      receipt,
    });
    const field = (value) =>
      event({ kind: 'component', child: 'assistant-global', name: 'changed', event: value });
    const source = {
      kind: 'available',
      source: 'agents',
      path: '/remote/home/.agents/AGENTS.md',
      revision: 'source-v1',
      content: 'Original 中文',
      exists: true,
    };
    const ack = (revision) => {
      const command = out.frame.state.global.command;
      field(
        observed(command.edit.text, revision, {
          owner: command.owner,
          sequence: command.sequence,
          outcome: { status: 'applied' },
        }),
      );
    };
    finish([]);
    assert.equal(out.actions[0].method, 'globalInstructions');
    finish(source);
    field(observed('', 0));
    ack(1);
    const reply = (
      kind,
      reference = {
        contract: { name: 'maka.agent.Agent', version: 1 },
        method: 'updateGlobalInstructions',
        operation: 'global-save-1',
      },
    ) => ({
      reference,
      receipt: { kind: 'recorded', outcome: { kind: 'returned', value: { kind } } },
    });
    return {
      get out() {
        return out;
      },
      event,
      raw,
      finish,
      observed,
      field,
      source,
      ack,
      reply,
    };
  };

  test(`Assistant ${target} global instructions save exact source bytes and preserve unrelated preferences`, async () => {
    const f = await globalFixture();
    const preferences = structuredClone(f.out.frame.state.fields);
    const content = 'Keep all 中文 instructions.\n' + 'x'.repeat(1200);
    const draft = f.observed(content, 2);
    f.field(draft);
    f.raw('global-save');
    assert.deepEqual(f.out.actions[0].input, { start: 0, end: Buffer.byteLength(content) });
    assert.deepEqual(f.out.actions[0].resources.text.input, {
      kind: 'at_revision',
      owner: 'global-editor',
      revision: 2,
    });
    f.finish({ metadata: draft.snapshot.text, text: content });
    const action = f.out.actions[0];
    assert.equal(action.binding, 'agent');
    assert.equal(action.method, 'updateGlobalInstructions');
    assert.deepEqual(action.input, { source: 'agents', revision: 'source-v1', content });
    assert.equal(action.recovery.kind, 'assistant-global-instructions/v1');
    f.event({
      kind: 'action',
      key: action.key,
      event: { status: 'operation', reply: f.reply('saved') },
    });
    f.finish({ ...f.source, revision: 'source-v2', content });
    assert.equal(f.out.frame.state.global.command.edit.text, content);
    assert.deepEqual(f.out.frame.state.fields, preferences);
    assert.equal(f.out.frame.state.global.saved.path, f.source.path);
  });

  test(`Assistant ${target} source conflict preserves draft until explicit reload`, async () => {
    const f = await globalFixture();
    const draft = f.observed('My unsaved draft', 2);
    f.field(draft);
    f.raw('global-save');
    f.finish({ metadata: draft.snapshot.text, text: 'My unsaved draft' });
    f.event({
      kind: 'action',
      key: f.out.actions[0].key,
      event: { status: 'operation', reply: f.reply('changed') },
    });
    f.finish({
      ...f.source,
      source: 'maka',
      path: '/remote/home/.maka/AGENTS.md',
      revision: 'external',
      content: 'External edit',
    });
    assert.deepEqual(f.out.frame.state.global.observation, draft);
    assert.equal(f.out.frame.state.global.command, null);
    assert.equal(f.out.frame.state.global.conflict, true);
    f.raw('global-save');
    assert.equal(f.out.actions.length, 0);
    f.raw('global-reload');
    f.finish({ ...f.source, revision: 'external', content: 'External edit' });
    assert.equal(f.out.frame.state.global.command.edit.text, 'External edit');
    assert.equal(f.out.frame.state.global.conflict, false);
  });

  test(`Assistant ${target} unknown global save recovers its original receipt without another write`, async () => {
    const f = await globalFixture();
    const draft = f.observed('Draft', 2);
    f.field(draft);
    f.raw('global-save');
    f.finish({ metadata: draft.snapshot.text, text: 'Draft' });
    const action = f.out.actions[0];
    f.event({
      kind: 'action',
      key: action.key,
      event: { status: 'failed', diagnostic: 'interrupted' },
    });
    f.raw('global-save');
    assert.equal(f.out.actions.length, 0);
    f.raw('global-history');
    const reply = f.reply('saved');
    f.finish([{ cursor: 1, reference: reply.reference, recovery: action.recovery }]);
    f.finish([]);
    assert.equal(f.out.actions[0].binding, 'receipts');
    assert.deepEqual(f.out.actions[0].input, reply.reference);
    f.finish(reply);
    f.finish({ ...f.source, revision: 'saved', content: 'Draft' });
    assert.equal(f.out.frame.state.global.current, null);
  });

  test(`Assistant ${target} rejects oversized and stale global drafts without truncation`, async () => {
    const f = await globalFixture();
    f.field(f.observed('中'.repeat(22000), 2));
    f.raw('global-save');
    assert.equal(f.out.actions.length, 0);
    assert.equal(f.out.frame.state.global.notice, 'assistant-global-limit');
    const draft = f.observed('first', 3);
    f.field(draft);
    f.raw('global-save');
    f.field(f.observed('changed during read', 4));
    f.finish({ metadata: draft.snapshot.text, text: 'first' });
    assert.equal(f.out.actions.length, 0);
    assert.equal(f.out.frame.state.global.current, null);
  });

  test(`Assistant ${target} name save clears the legacy tone without reading another editor`, async () => {
    const { component } = await import(`../dist/packages/assistant-preferences/${target}.js`);
    const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const finish = (value) =>
      event({
        kind: 'action',
        key: out.frame.state.pending.key,
        event: { status: 'succeeded', value },
      });
    finish([]);
    finish({
      revision: 7,
      displayName: 'Old',
      assistantTone: 'Legacy',
      compactionPrompt: 'Prompt',
      defaultCompactionPrompt: 'Prompt',
      workspaceInstructions: true,
      incognitoActive: false,
      shellTitles: true,
      language: 'en',
      resolvedLanguage: 'en',
    });
    const observed = (text, revision, receipt = null) => ({
      revision,
      snapshot: { text: { owner: 'name', revision, bytes: Buffer.byteLength(text) }, marked: null },
      receipt,
    });
    const field = (value) =>
      event({ kind: 'component', child: 'assistant-name', name: 'changed', event: value });
    field(observed('', 0));
    const command = out.frame.state.fields.name.command;
    field(
      observed('Old', 1, {
        owner: command.owner,
        sequence: command.sequence,
        outcome: { status: 'applied' },
      }),
    );
    const draft = observed('New', 2);
    field(draft);
    event({ kind: 'raw', key: 'save', event: {} });
    finish({ metadata: draft.snapshot.text, text: 'New' });
    assert.deepEqual(out.actions[0].input, {
      expectedRevision: 7,
      mutation: { kind: 'personalization', displayName: 'New', assistantTone: '' },
    });
  });
}

const languageSaved = {
  revision: 12,
  language: 'en',
  resolvedLanguage: 'en',
  displayName: 'Saved name',
  assistantTone: '',
  compactionPrompt: 'Original custom prompt 中文',
  defaultCompactionPrompt: 'Default prompt',
  workspaceInstructions: true,
  incognitoActive: false,
  shellTitles: true,
};
const preferenceNodes = (node) =>
  node && typeof node === 'object' ? [node, ...Object.values(node).flatMap(preferenceNodes)] : [];
test('language choice has no selection until preferences load', async () => {
  const { component } = await import('../dist/packages/assistant-preferences/tui.js');
  const props = { theme, locale: 'en', open: true, busy: false, shell: { back: 0 } };
  let out = component.mount(props, {});
  const selection = () => preferenceNodes(out.frame.view).find((node) => node.key === 'language');
  const finish = (value) => {
    out = component.event(
      out.frame.state,
      { kind: 'action', key: out.actions[0].key, event: { status: 'succeeded', value } },
      props,
      {},
    );
  };
  assert.equal(selection().current, null);
  assert.equal(selection().disabled, true);
  assert.equal(selection().label, 'Loading language…');
  finish([]);
  assert.equal(selection().current, null);
  assert.equal(selection().disabled, true);
  assert.equal(selection().label, 'Loading language…');
  finish(languageSaved);
  assert.equal(selection().current, 'en');
  assert.equal(selection().disabled, false);
  assert.equal(selection().label, 'Language');
  out = component.update(out.frame.state, { ...props, open: false }, {});
  out = component.update(out.frame.state, props, {});
  assert.equal(selection().current, 'en');
  assert.equal(selection().disabled, true);
  assert.equal(selection().label, 'Loading language…');
  finish({ ...languageSaved, language: 'zh-CN', resolvedLanguage: 'zh-CN' });
  assert.equal(selection().current, 'zh-CN');
  assert.equal(selection().disabled, false);
  assert.equal(selection().label, 'Language');
});
async function languageEditor() {
  const { component } = await import('../dist/packages/assistant-preferences/tui.js');
  let props = {
    theme,
    locale: 'en',
    open: true,
    busy: false,
    shell: { back: 0 },
  };
  let out = component.mount(props, {});
  const fixture = {
    get out() {
      return out;
    },
    event(input) {
      out = component.event(out.frame.state, input, props, {});
      return out;
    },
    finish(value) {
      return fixture.event({
        kind: 'action',
        key: out.frame.state.pending.key,
        event: { status: 'succeeded', value },
      });
    },
    update(open) {
      props = { ...props, open };
      out = component.update(out.frame.state, props, {});
      return out;
    },
    select(preference) {
      return fixture.event({
        kind: 'raw',
        key: 'language',
        event: { key: preference },
      });
    },
    selection() {
      return preferenceNodes(out.frame.view).find((node) => node.key === 'language');
    },
  };
  fixture.finish([]);
  fixture.finish(languageSaved);
  return fixture;
}
const languageReference = {
  contract: { name: 'maka.settings.Settings', version: 1 },
  method: 'updateAssistantPreferences',
  operation: 'language-save-original',
};
function languageReply(value) {
  return {
    reference: languageReference,
    receipt: {
      kind: 'recorded',
      cleanup: { kind: 'confirmed' },
      outcome: { kind: 'returned', value },
    },
  };
}

test('language save uses the observed revision and publishes only a confirmed native read without resetting drafts', async () => {
  const f = await languageEditor();
  f.event({
    kind: 'component',
    child: 'assistant-name',
    name: 'changed',
    event: {
      revision: 9,
      snapshot: {
        text: { owner: 'draft-name', revision: 9, bytes: 11 },
        marked: null,
      },
      receipt: null,
    },
  });
  const fields = structuredClone(f.out.frame.state.fields);
  f.select('zh-TW');
  const action = f.out.actions.find((action) => action.method === 'updateAssistantPreferences');
  assert.deepEqual(action.input, {
    expectedRevision: 12,
    mutation: { kind: 'language', preference: 'zh-TW' },
  });
  assert.ok(valid(action.input), JSON.stringify(valid.errors));
  assert.equal(f.selection().current, 'en');
  assert.deepEqual(f.out.events, []);
  f.event({
    kind: 'action',
    key: action.key,
    event: {
      status: 'operation',
      reply: languageReply({ kind: 'committed', revision: 13 }),
    },
  });
  assert.deepEqual(
    f.out.actions.map(({ binding, method, input }) => ({
      binding,
      method,
      input,
    })),
    [{ binding: 'settings', method: 'assistantPreferences', input: {} }],
  );
  assert.equal(f.out.frame.state.saved.language, 'en');
  assert.deepEqual(f.out.events, []);
  f.finish({
    ...languageSaved,
    revision: 13,
    language: 'zh-TW',
    resolvedLanguage: 'zh-TW',
  });
  assert.equal(f.selection().current, 'zh-TW');
  assert.deepEqual(f.out.events, [{ name: 'changed', value: {} }]);
  assert.deepEqual(f.out.frame.state.fields, fields);
  f.update(false);
  f.update(true);
  assert.equal(f.out.actions[0].method, 'assistantPreferences');
  f.finish({
    ...languageSaved,
    revision: 14,
    language: 'auto',
    resolvedLanguage: 'zh-CN',
  });
  assert.equal(f.out.frame.state.saved.language, 'auto');
  assert.equal(f.out.frame.state.saved.resolvedLanguage, 'zh-CN');
  assert.deepEqual(f.out.events, [{ name: 'changed', value: {} }]);
  assert.deepEqual(f.out.frame.state.fields, fields);
});

test('unknown language save recovers the original receipt; rejected saves leave the native selection intact', async () => {
  const f = await languageEditor();
  f.select('zh-CN');
  const action = f.out.actions[0];
  f.event({
    kind: 'action',
    key: action.key,
    event: { status: 'failed', diagnostic: 'lost reply' },
  });
  assert.equal(f.out.frame.state.saved.language, 'en');
  assert.deepEqual(f.out.events, []);
  f.select('zh-TW');
  assert.equal(f.out.actions.length, 0);
  f.event({ kind: 'raw', key: 'history', event: {} });
  f.finish([{ cursor: 3, reference: languageReference, recovery: action.recovery }]);
  f.finish([]);
  assert.deepEqual(
    f.out.actions.map(({ method, input }) => ({ method, input })),
    [{ method: 'read', input: languageReference }],
  );
  f.finish(languageReply({ kind: 'committed', revision: 13 }));
  assert.equal(f.out.actions[0].method, 'assistantPreferences');
  assert.equal(f.out.frame.state.saved.language, 'en');
  f.finish({
    ...languageSaved,
    revision: 13,
    language: 'zh-CN',
    resolvedLanguage: 'zh-CN',
  });
  assert.equal(f.out.frame.state.saved.language, 'zh-CN');
  assert.deepEqual(f.out.events, [{ name: 'changed', value: {} }]);
  f.select('en');
  f.event({
    kind: 'action',
    key: f.out.actions[0].key,
    event: {
      status: 'operation',
      reply: languageReply({ kind: 'revision_conflict', actualRevision: 14 }),
    },
  });
  assert.equal(f.out.frame.state.saved.language, 'zh-CN');
  assert.deepEqual(f.out.events, []);
  assert.equal(f.out.frame.state.notice, 'assistant-conflict');
});
