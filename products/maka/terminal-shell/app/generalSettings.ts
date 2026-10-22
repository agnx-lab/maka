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

import { batch, cleanup, signal } from 'nsh';
import { host } from './host';
import { applyLanguagePreference, tr } from './locale';
import { message, UiError } from './text';
import type {
  LanguagePreference,
  SettingsAssistantPreferences,
  SettingsPreferencesMutation,
  SettingsPreferencesUpdated,
} from '../../../../packages/plugin-sdk/src/index.js';
import type { SettingsOperations } from './settingsOperations';

type Editor =
  | {
      kind: 'personalization';
      id: number;
      basis: SettingsAssistantPreferences;
      displayName: string;
      assistantTone: string;
    }
  | {
      kind: 'compaction';
      id: number;
      basis: SettingsAssistantPreferences;
      prompt: string;
      useDefault: boolean;
    };
export type GeneralAction = {
  view: number;
  revision: number | null;
  kind: 'root' | Editor['kind'];
};
export const assistantLanguages = (): { value: LanguagePreference; label: string }[] => [
  { value: 'auto', label: tr('Automatic') },
  { value: 'en', label: 'English' },
  { value: 'zh-CN', label: '简体中文' },
  { value: 'zh-TW', label: '繁體中文' },
];

export function createGeneralSettings(operations: SettingsOperations, active: () => boolean) {
  const facts = signal<SettingsAssistantPreferences | null>(null);
  const editor = signal<Editor | null>(null);
  const loading = signal(false);
  const view = signal(0);
  let alive = true;
  let enabled = false;
  let read = 0;
  const visible = () => alive && enabled && active();
  const action = (): GeneralAction => ({
    view: view(),
    revision: editor()?.basis.revision ?? facts()?.revision ?? null,
    kind: editor()?.kind ?? 'root',
  });
  const current = (request: GeneralAction) => {
    const selected = action();
    return (
      visible() &&
      request.view === selected.view &&
      request.revision === selected.revision &&
      request.kind === selected.kind
    );
  };
  const disabled = () =>
    !visible() ||
    loading() ||
    operations.busy() ||
    !operations.pendingKnown() ||
    operations.pending().length > 0 ||
    facts() === null;
  const cancelRead = () => {
    read += 1;
    view.set(view() + 1);
    loading.set(false);
  };
  const load = async () => {
    if (!visible() || editor()) return;
    const generation = ++read;
    const capturedView = view();
    const owns = () => visible() && generation === read && capturedView === view() && !editor();
    batch(() => {
      facts.set(null);
      loading.set(true);
    });
    try {
      const result: SettingsAssistantPreferences = await host.query(
        'settings',
        'assistantPreferences',
        {},
      );
      applyLanguagePreference(result.language ?? 'auto', result.revision);
      if (owns()) facts.set(result);
    } catch (reason) {
      if (owns()) operations.report(reason);
    } finally {
      if (owns()) loading.set(false);
    }
  };
  const mutate = (
    request: GeneralAction,
    mutation: SettingsPreferencesMutation,
    submitted: Editor | null = null,
  ) => {
    if (!current(request) || disabled() || request.revision === null) return;
    const expectedRevision = request.revision;
    return operations.run(async () => {
      if (!current(request)) return;
      const result: SettingsPreferencesUpdated = await operations.mutate(
        'settings',
        'updateAssistantPreferences',
        { expectedRevision, mutation },
      );
      if (!alive) return;
      switch (result.kind) {
        case 'committed':
          if (mutation.kind === 'language')
            applyLanguagePreference(mutation.preference, result.revision);
          // Retire the submitted editor before reading new facts. A failed read
          // cannot leave a committed draft available to submit a second time.
          batch(() => {
            facts.set(null);
            if (submitted && editor()?.id === submitted.id) editor.set(null);
            cancelRead();
          });
          if (visible()) await load();
          operations.notice.set(message('Preferences saved.'));
          break;
        case 'revision_conflict':
          facts.set(null);
          throw new UiError(
            'Preferences changed elsewhere. Cancel this edit and refresh before saving again.',
          );
        case 'rejected':
          throw new UiError('The Host rejected these preferences. Check the values before saving.');
      }
    });
  };
  const open = (request: GeneralAction, kind: Editor['kind']) => {
    const basis = facts();
    if (!basis || request.kind !== 'root' || !current(request) || disabled()) return;
    cancelRead();
    editor.set(
      kind === 'personalization'
        ? {
            kind,
            id: view(),
            basis,
            displayName: basis.displayName,
            assistantTone: basis.assistantTone,
          }
        : { kind, id: view(), basis, prompt: basis.compactionPrompt, useDefault: false },
    );
  };
  const dirty = () => {
    const form = editor();
    return form?.kind === 'personalization'
      ? form.displayName !== form.basis.displayName ||
          form.assistantTone !== form.basis.assistantTone
      : form?.kind === 'compaction'
        ? form.useDefault || form.prompt !== form.basis.compactionPrompt
        : false;
  };
  const back = () => {
    if (operations.busy()) return false;
    if (!editor()) return true;
    cancelRead();
    editor.set(null);
    if (visible()) void load();
    return false;
  };
  cleanup(() => {
    alive = false;
    enabled = false;
    cancelRead();
  });
  return {
    facts,
    editor,
    loading,
    busy: operations.busy,
    action,
    navigation: () => JSON.stringify(action()),
    disabled,
    dirty,
    activate() {
      if (!alive || enabled) return;
      enabled = true;
      cancelRead();
      if (!editor()) void load();
    },
    deactivate() {
      enabled = false;
      cancelRead();
    },
    reset() {
      cancelRead();
      editor.set(null);
      facts.set(null);
      if (visible()) void load();
    },
    back,
    title: () =>
      editor()?.kind === 'personalization'
        ? tr('Personalization')
        : editor()?.kind === 'compaction'
          ? tr('Compaction prompt')
          : tr('General settings'),
    open: ({ request, kind }: { request: GeneralAction; kind: Editor['kind'] }) =>
      open(request, kind),
    reload(request: GeneralAction) {
      if (!current(request) || request.kind !== 'root' || operations.busy() || loading()) return;
      operations.error.set('');
      void load();
    },
    cancel(request: GeneralAction) {
      if (current(request)) back();
    },
    toggle({
      request,
      kind,
    }: {
      request: GeneralAction;
      kind: 'instructions' | 'privacy' | 'titles';
    }) {
      const saved = facts();
      if (!saved || request.kind !== 'root') return;
      return mutate(
        request,
        kind === 'instructions'
          ? { kind: 'workspace_instructions', enabled: !saved.workspaceInstructions }
          : kind === 'privacy'
            ? { kind: 'privacy', incognitoActive: !saved.incognitoActive }
            : { kind: 'shell_titles', enabled: !saved.shellTitles },
      );
    },
    language({ request, preference }: { request: GeneralAction; preference: LanguagePreference }) {
      if (request.kind !== 'root' || facts()?.language === preference) return;
      return mutate(request, { kind: 'language', preference });
    },
    editName(request: GeneralAction, field: 'displayName' | 'assistantTone', text: string) {
      const form = editor();
      if (!current(request) || disabled() || form?.kind !== 'personalization') return;
      editor.set({ ...form, [field]: text });
    },
    editPrompt(request: GeneralAction, prompt: string) {
      const form = editor();
      if (!current(request) || disabled() || form?.kind !== 'compaction') return;
      editor.set({ ...form, prompt, useDefault: false });
    },
    useDefault(request: GeneralAction) {
      const form = editor();
      if (!current(request) || disabled() || form?.kind !== 'compaction') return;
      editor.set({ ...form, prompt: form.basis.defaultCompactionPrompt, useDefault: true });
    },
    save(request: GeneralAction) {
      const form = editor();
      if (!form || !current(request) || disabled() || !dirty()) return;
      if (form.kind === 'personalization') {
        return mutate(
          request,
          {
            kind: 'personalization',
            displayName: form.displayName,
            assistantTone: form.assistantTone,
          },
          form,
        );
      }
      if (!form.useDefault && !form.prompt.trim()) {
        operations.report(new UiError('Enter a compaction prompt or choose the Host default.'));
        return;
      }
      return mutate(
        request,
        { kind: 'compaction_prompt', prompt: form.useDefault ? null : form.prompt },
        form,
      );
    },
  };
}

export type GeneralSettingsOwner = ReturnType<typeof createGeneralSettings>;
