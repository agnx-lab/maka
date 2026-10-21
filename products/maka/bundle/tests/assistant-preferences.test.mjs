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
    const props = {
      theme,
      locale: 'en',
      open: true,
      busy: false,
      shell: { back: 0 },
    };
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
  for (const historyRecovery of ['emitted', 'omitted'])
    test(`Assistant ${target} privacy recovers ${historyRecovery} metadata without overwriting editor drafts`, async () => {
      const { component } = await import(`../dist/packages/assistant-preferences/${target}.js`);
      const props = {
        theme,
        locale: 'en',
        open: true,
        busy: false,
        shell: { back: 0 },
      };
      let out = component.mount(props, {});
      const event = (input) => (out = component.event(out.frame.state, input, props, {}));
      const raw = (key) => event({ kind: 'raw', key, event: {} });
      const finish = (value) =>
        event({
          kind: 'action',
          key: out.actions[0].key,
          event: { status: 'succeeded', value },
        });
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
      const name = (text, revision, receipt = null) =>
        event({
          kind: 'component',
          child: 'assistant-name',
          name: 'changed',
          event: {
            revision,
            snapshot: {
              text: {
                owner: 'privacy-name',
                revision,
                bytes: Buffer.byteLength(text),
              },
              marked: null,
            },
            receipt,
          },
        });
      name('', 0);
      const reset = out.frame.state.fields.name.command;
      name('Saved', 1, {
        owner: reset.owner,
        sequence: reset.sequence,
        outcome: { status: 'applied' },
      });
      name('Unsaved name 中文', 2);
      const fields = structuredClone(out.frame.state.fields);
      raw('privacy');
      const action = out.actions[0];
      assert.deepEqual(action.recovery, { reset: false });
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
      finish([
        {
          cursor: 2,
          reference,
          ...(historyRecovery === 'emitted' ? { recovery: action.recovery } : {}),
        },
      ]);
      assert.equal(out.frame.state.current.reset, false);
      finish([]);
      assert.equal(out.actions[0].method, 'read');
      assert.deepEqual(out.actions[0].input, reference);
      finish({ reference, receipt: { kind: 'recorded', outcome: { kind: 'unknown' } } });
      raw('privacy');
      assert.equal(out.actions.length, 0, 'an unknown original operation blocks another mutation');
      raw('receipt');
      assert.equal(out.actions[0].method, 'read');
      assert.deepEqual(out.actions[0].input, reference);
      finish({
        reference,
        receipt: {
          kind: 'recorded',
          cleanup: { kind: 'confirmed' },
          outcome: {
            kind: 'returned',
            value: { kind: 'committed', revision: 5 },
          },
        },
      });
      assert.equal(out.actions[0].method, 'assistantPreferences');
      finish({
        ...saved,
        revision: 6,
        incognitoActive: true,
        displayName: 'Changed elsewhere',
      });
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
    const props = {
      theme,
      locale: 'en',
      open: true,
      busy: false,
      shell: { back: 0 },
    };
    let out = component.mount(props, {});
    const finish = (value) => {
      out = component.event(
        out.frame.state,
        {
          kind: 'action',
          key: out.actions[0].key,
          event: { status: 'succeeded', value },
        },
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
          input: {
            expectedRevision: 7,
            mutation: { kind: 'shell_titles', enabled: false },
          },
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
              outcome: {
                kind: 'returned',
                value: { kind: 'committed', revision: 8 },
              },
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
    const props = {
      theme,
      locale: 'en',
      open: true,
      busy: false,
      shell: { back: 0 },
    };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({
        kind: 'action',
        key: out.actions[0].key,
        event: { status: 'succeeded', value },
      });
    const observed = (text, revision, receipt = null) => ({
      revision,
      snapshot: {
        text: { owner: 'prompt', revision, bytes: Buffer.byteLength(text) },
        marked: null,
      },
      receipt,
    });
    const field = (value) =>
      event({
        kind: 'component',
        child: 'assistant-compaction',
        name: 'changed',
        event: value,
      });
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
    assert.deepEqual(initial.edit, {
      kind: 'reset',
      text: 'Built-in instructions',
    });
    field(
      observed('Built-in instructions', 1, {
        owner: initial.owner,
        sequence: initial.sequence,
        outcome: { status: 'applied' },
      }),
    );
    const content = 'Compression start 中文\n' + '中'.repeat(13_650) + '\nCompression end 😀';
    assert.ok(Buffer.byteLength(content) > 32 * 1024 && Buffer.byteLength(content) < 64 * 1024);
    const draft = observed(content, 2);
    field(draft);
    raw('save-compaction');
    assert.equal(out.actions[0].resources.text.child, 'assistant-compaction');
    finish({ metadata: draft.snapshot.text, text: content });
    const input = out.actions[0].input;
    assert.deepEqual(Object.keys(input).sort(), ['expectedRevision', 'mutation']);
    assert.equal(input.expectedRevision, 4);
    assert.deepEqual(Object.keys(input.mutation).sort(), ['kind', 'prompt']);
    assert.equal(input.mutation.kind, 'compaction_prompt');
    assert.ok(input.mutation.prompt === content, 'save must retain every custom-prompt byte');
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
              outcome: {
                kind: 'returned',
                value: { kind: 'committed', revision },
              },
            },
          },
        },
      });
    const unrelated = structuredClone(out.frame.state.fields.name);
    commit(5);
    finish({ ...saved, revision: 5, compactionPrompt: content });
    assert.deepEqual(out.frame.state.fields.name, unrelated);
    const savedReset = out.frame.state.fields.compaction.command;
    assert.equal(savedReset.edit.kind, 'reset');
    assert.ok(
      savedReset.edit.text === content,
      'saved reset must contain the complete custom prompt',
    );
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
    assert.deepEqual(pending.edit, {
      kind: 'reset',
      text: 'Built-in instructions',
    });
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
  const globalFixture = async (initialContent = 'Original 中文') => {
    const { component } = await import(`../dist/packages/assistant-preferences/${target}.js`);
    const props = {
      theme,
      locale: 'en',
      open: true,
      busy: false,
      shell: { back: 0 },
    };
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
      snapshot: {
        text: { owner, revision, bytes: Buffer.byteLength(content) },
        marked: null,
      },
      receipt,
    });
    const field = (value) =>
      event({
        kind: 'component',
        child: 'assistant-global',
        name: 'changed',
        event: value,
      });
    const source = {
      kind: 'available',
      source: 'agents',
      path: '/remote/home/.agents/AGENTS.md',
      revision: 'source-v1',
      content: initialContent,
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
    assert.ok(out.frame.state.global.command, 'available global source must initialize the editor');
    assert.ok(
      out.frame.state.global.command.edit.text === initialContent,
      'initial global reset must retain every source byte',
    );
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
      receipt: {
        kind: 'recorded',
        outcome: { kind: 'returned', value: { kind } },
      },
    });
    return {
      get out() {
        return out;
      },
      remount: () => (out = component.mount(props, {})),
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
    const original = 'Original global first\n' + '中😀 '.repeat(140_000) + '\nOriginal global last';
    const f = await globalFixture(original);
    const preferences = structuredClone(f.out.frame.state.fields);
    const content = 'Global start 中文\n' + '中'.repeat(400_000) + '\nGlobal end 😀';
    assert.ok(Buffer.byteLength(content) > 1024 * 1024);
    const draft = f.observed(content, 2);
    f.field(draft);
    f.raw('global-save');
    assert.deepEqual(f.out.actions[0].input, {
      start: 0,
      end: Buffer.byteLength(content),
    });
    assert.deepEqual(f.out.actions[0].resources.text.input, {
      kind: 'at_revision',
      owner: 'global-editor',
      revision: 2,
    });
    f.finish({ metadata: draft.snapshot.text, text: content });
    const action = f.out.actions[0];
    assert.equal(action.binding, 'agent');
    assert.equal(action.method, 'updateGlobalInstructions');
    assert.deepEqual(Object.keys(action.input).sort(), ['content', 'revision', 'source']);
    assert.equal(action.input.source, 'agents');
    assert.equal(action.input.revision, 'source-v1');
    assert.ok(action.input.content === content, 'global save must retain every draft byte');
    assert.ok(action.recovery === undefined, 'global recovery must not duplicate the action input');
    f.event({
      kind: 'action',
      key: action.key,
      event: { status: 'operation', reply: f.reply('saved') },
    });
    f.finish({ ...f.source, revision: 'source-v2', content });
    assert.ok(
      f.out.frame.state.global.command.edit.text === content,
      'saved global reset must contain the complete draft',
    );
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

  test(`Assistant ${target} unknown global save recovers its original receipt after remount without another write`, async () => {
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
    f.remount();
    const reply = f.reply('saved');
    f.finish([{ cursor: 1, reference: reply.reference, recovery: action.recovery }]);
    f.finish([]);
    const receipt = () => {
      assert.equal(f.out.actions[0].binding, 'receipts');
      assert.equal(f.out.actions[0].method, 'read');
      assert.deepEqual(f.out.actions[0].input, reply.reference);
    };
    receipt();
    f.event({
      kind: 'action',
      key: f.out.actions[0].key,
      event: { status: 'failed', diagnostic: 'receipt unavailable' },
    });
    assert.deepEqual(f.out.frame.state.global.current.reference, reply.reference);
    f.raw('global-save');
    assert.equal(f.out.actions.length, 0);
    f.raw('global-receipt');
    receipt();
    f.finish({
      ...reply,
      receipt: { kind: 'recorded', outcome: { kind: 'unknown' } },
    });
    assert.deepEqual(f.out.frame.state.global.current.reference, reply.reference);
    f.raw('global-save');
    assert.equal(f.out.actions.length, 0);
    f.raw('global-receipt');
    receipt();
    f.finish(reply);
    assert.equal(f.out.actions[0].method, 'globalInstructions');
    f.finish({ ...f.source, revision: 'saved', content: 'Draft' });
    assert.equal(f.out.frame.state.global.current, null);
    f.field(f.observed('', 0));
    assert.deepEqual(f.out.frame.state.global.command.edit, {
      kind: 'reset',
      text: 'Draft',
    });
    f.ack(1);
    assert.equal(f.out.frame.state.global.ready, true);
  });

  test(`Assistant ${target} rejects stale global drafts without truncation`, async () => {
    const f = await globalFixture();
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
    const props = {
      theme,
      locale: 'en',
      open: true,
      busy: false,
      shell: { back: 0 },
    };
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
      snapshot: {
        text: { owner: 'name', revision, bytes: Buffer.byteLength(text) },
        marked: null,
      },
      receipt,
    });
    const field = (value) =>
      event({
        kind: 'component',
        child: 'assistant-name',
        name: 'changed',
        event: value,
      });
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
      mutation: {
        kind: 'personalization',
        displayName: 'New',
        assistantTone: '',
      },
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
for (const reset of ['compaction', 'personalization']) {
  test(`Assistant tui initializes both editors after fresh ${reset} receipt recovery`, async () => {
    const { component } = await import('../dist/packages/assistant-preferences/tui.js');
    const props = {
      theme,
      locale: 'en',
      open: true,
      busy: false,
      shell: { back: 0 },
    };
    let out = component.mount(props, {});
    const event = (input) => (out = component.event(out.frame.state, input, props, {}));
    const raw = (key) => event({ kind: 'raw', key, event: {} });
    const finish = (value) =>
      event({
        kind: 'action',
        key: out.frame.state.pending.key,
        event: { status: 'succeeded', value },
      });
    const field = (name, text, revision, receipt = null) =>
      event({
        kind: 'component',
        child: `assistant-${name}`,
        name: 'changed',
        event: {
          revision,
          snapshot: {
            text: {
              owner: `editor-${name}`,
              revision,
              bytes: Buffer.byteLength(text),
            },
            marked: null,
          },
          receipt,
        },
      });
    const acknowledge = (name, text) => {
      const command = out.frame.state.fields[name].command;
      field(name, text, 1, {
        owner: command.owner,
        sequence: command.sequence,
        outcome: { status: 'applied' },
      });
    };
    finish([]);
    finish(languageSaved);
    for (const [name, text] of [
      ['name', languageSaved.displayName],
      ['compaction', languageSaved.compactionPrompt],
    ]) {
      field(name, '', 0);
      acknowledge(name, text);
    }
    if (reset === 'compaction') raw('reset-compaction');
    else {
      field('name', 'Recovered name 中文', 2);
      raw('save');
      finish({
        metadata: out.frame.state.fields.name.observation.snapshot.text,
        text: 'Recovered name 中文',
      });
    }
    const action = out.actions[0];
    assert.equal(action.method, 'updateAssistantPreferences');
    event({
      kind: 'action',
      key: action.key,
      event: { status: 'failed', diagnostic: 'interrupted before receipt' },
    });
    // A genuinely new component has no saved preference snapshot or editor state.
    out = component.mount(props, {});
    assert.equal(out.frame.state.saved, null);
    const reference = {
      contract: { name: 'maka.settings.Settings', version: 1 },
      method: 'updateAssistantPreferences',
      operation: `original-${reset}`,
    };
    // Reuse the actual emitted metadata on both original and candidate source.
    finish([{ cursor: 1, reference, recovery: action.recovery }]);
    finish([]);
    assert.equal(out.actions[0].binding, 'receipts');
    assert.equal(out.actions[0].method, 'read');
    assert.deepEqual(out.actions[0].input, reference);
    finish({
      reference,
      receipt: {
        kind: 'recorded',
        cleanup: { kind: 'confirmed' },
        outcome: {
          kind: 'returned',
          value: { kind: 'committed', revision: 13 },
        },
      },
    });
    assert.equal(out.actions[0].method, 'assistantPreferences');
    const saved = {
      ...languageSaved,
      revision: 14,
      displayName: 'Recovered name 中文',
      compactionPrompt: 'Recovered compression 中文',
    };
    finish(saved);
    assert.equal(out.frame.state.current, null);
    assert.equal(
      out.frame.state.fields.name.reset,
      saved.displayName,
      'fresh recovery must initialize the name even for a compression operation',
    );
    assert.equal(
      out.frame.state.fields.compaction.reset,
      saved.compactionPrompt,
      'fresh recovery must initialize compression even for a name operation',
    );
    assert.deepEqual(action.recovery, { reset });
    for (const [name, text] of [
      ['name', saved.displayName],
      ['compaction', saved.compactionPrompt],
    ]) {
      field(name, '', 0);
      assert.deepEqual(out.frame.state.fields[name].command.edit, {
        kind: 'reset',
        text,
      });
      acknowledge(name, text);
      assert.equal(out.frame.state.fields[name].ready, true);
    }
  });
}

const preferenceNodes = (node) =>
  node && typeof node === 'object' ? [node, ...Object.values(node).flatMap(preferenceNodes)] : [];
test('reopening Assistant preferences preserves unsaved drafts after an external language change', async () => {
  const { component } = await import('../dist/packages/assistant-preferences/tui.js');
  const props = {
    theme,
    locale: 'en',
    open: true,
    busy: false,
    shell: { back: 0 },
  };
  let out = component.mount(props, {});
  const finish = (value) => {
    out = component.event(
      out.frame.state,
      {
        kind: 'action',
        key: out.frame.state.pending.key,
        event: { status: 'succeeded', value },
      },
      props,
      {},
    );
  };
  finish([]);
  finish(languageSaved);
  out = component.event(
    out.frame.state,
    {
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
    },
    props,
    {},
  );
  const fields = structuredClone(out.frame.state.fields);
  assert.equal(
    preferenceNodes(out.frame.view).some((node) => node.key === 'language'),
    false,
  );
  out = component.update(out.frame.state, { ...props, open: false }, {});
  out = component.update(out.frame.state, props, {});
  finish({
    ...languageSaved,
    revision: 13,
    language: 'zh-TW',
    resolvedLanguage: 'zh-TW',
  });
  assert.deepEqual(out.frame.state.fields, fields);
  assert.deepEqual(out.events, [{ name: 'changed', value: {} }]);
});
