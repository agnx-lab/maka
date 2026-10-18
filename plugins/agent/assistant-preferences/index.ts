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

import {
  globalInstructionsMount,
  globalInstructionsEvent,
  globalInstructionsView,
  type GlobalInstructionsState,
} from './global-instructions.js';
import { editorMenu } from '../../shared/editor-menu.js';
import type {
  Action,
  ActionOutcome,
  CallerIntent,
  Component,
  EditorCommand,
  EditorObservation,
  Emission,
  InputProps,
  OperationReply,
  RecoveryRef,
  SettingsAssistantPreferences,
  SettingsPreferencesUpdate,
  SettingsPreferencesUpdated,
  TextChunk,
  TextareaProps,
} from '../../../packages/plugin-sdk/src/index.js';
import { assistantPreferencesContract, type ProjectsProps } from '../../shared/contracts.js';
import { withPageShell, pageHeader, type PageState } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { copy, tuiText } from './locale.js';
import { tuiText as settingsText } from '../../shared/locale-catalogs/settings.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

type Name = 'name' | 'compaction';
interface Field {
  observation: EditorObservation | null;
  command: EditorCommand | null;
  reset: string | null;
  sequence: number;
  ready: boolean;
}
type Request =
  | {
      kind: 'preferences';
      reset: boolean | 'compaction' | 'personalization';
      languageCommitted: boolean;
    }
  | { kind: 'mutate' | 'receipt' }
  | { kind: 'history'; after: number }
  | { kind: 'field'; name: Name; captured: EditorObservation };
interface State extends PageState {
  global: GlobalInstructionsState;
  revision: number;
  opened: boolean;
  serial: number;
  saved: SettingsAssistantPreferences | null;
  fields: Record<Name, Field>;
  pending: { key: string; request: Request } | null;
  current: { input: SettingsPreferencesUpdate; reference: RecoveryRef | null } | null;
  capture: { name: EditorObservation } | null;
  compactionCapture: EditorObservation | null;
  restoring: boolean;
  notice: string | null;
}
const field = (): Field => ({
  observation: null,
  command: null,
  reset: null,
  sequence: 0,
  ready: false,
});
function initialize(field: Field) {
  if (field.reset !== null && field.observation && !field.command)
    field.command = {
      owner: field.observation.snapshot.text.owner,
      sequence: ++field.sequence,
      basedOn: field.observation.revision,
      edit: { kind: 'reset', text: field.reset },
    };
}
function request(
  state: State,
  actions: Action[],
  request: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  if (state.pending) return;
  const key = 'assistant-preferences-' + ++state.serial;
  state.pending = { key, request };
  actions.push({ key, binding, member: 0, method, input, resources });
}
function read(
  state: State,
  actions: Action[],
  reset: boolean | 'compaction' | 'personalization' = true,
  languageCommitted = false,
) {
  request(
    state,
    actions,
    { kind: 'preferences', reset, languageCommitted },
    'settings',
    'assistantPreferences',
    {},
  );
}
function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  if (!after) state.current = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}
function disabled(state: State, props: ProjectsProps) {
  return (
    !props.open ||
    props.busy ||
    !!state.pending ||
    !!state.current ||
    !!state.capture ||
    !!state.compactionCapture ||
    state.restoring
  );
}
function mutate(state: State, actions: Action[], input: SettingsPreferencesUpdate) {
  state.current = { input, reference: null };
  state.notice = null;
  request(state, actions, { kind: 'mutate' }, 'settings', 'updateAssistantPreferences', input);
  actions.at(-1)!.recovery = { kind: 'assistant-preferences/v1', input };
}
function settle(state: State, reply: OperationReply, actions: Action[]) {
  if (
    !state.current ||
    (state.current.reference && !sameValue(state.current.reference, reply.reference))
  )
    return;
  state.current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'assistant-unknown';
    return;
  }
  const result = reply.receipt.outcome.value as SettingsPreferencesUpdated;
  const languageCommitted = state.current.input.mutation.kind === 'language';
  const reset =
    state.current.input.mutation.kind === 'compaction_prompt'
      ? 'compaction'
      : state.current.input.mutation.kind === 'personalization'
        ? 'personalization'
        : !state.saved;
  state.current = null;
  state.notice =
    result.kind === 'committed'
      ? 'assistant-saved'
      : result.kind === 'revision_conflict'
        ? 'assistant-conflict'
        : 'assistant-invalid';
  if (result.kind === 'committed' || !state.saved)
    read(
      state,
      actions,
      languageCommitted ? false : reset,
      result.kind === 'committed' && languageCommitted,
    );
}
function capture(state: State, actions: Action[], name: Name) {
  const captured = name === 'compaction' ? state.compactionCapture! : state.capture![name];
  request(
    state,
    actions,
    { kind: 'field', name, captured },
    'reader',
    'read',
    { start: 0, end: captured.snapshot.text.bytes },
    {
      text: {
        child: 'assistant-' + name,
        method: 'snapshot',
        input: {
          kind: 'at_revision',
          owner: captured.snapshot.text.owner,
          revision: captured.snapshot.text.revision,
        },
      },
    },
  );
}
export function assistantPreferences<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ProjectsProps, N> {
  function frame(
    state: State,
    props: ProjectsProps,
    actions: Action[] = [],
    events: Emission[] = [],
  ) {
    const ui = presentation.paint(props.theme),
      t = (key: string) => tuiText(props.locale, key);
    const busy = disabled(state, props),
      text = copy(props.locale);
    const fields = (['name', 'compaction'] as const).map((name) => {
      const common = {
        accessibilityLabel: t('assistant-' + name),
        placeholder: '',
        autoFocus: name === 'name',
        tabIndex: name === 'name' ? 1 : 3,
        readOnly: false,
        disabled: busy || !state.fields[name].ready || !!state.fields[name].command,
        command: state.fields[name].command,
      } satisfies Omit<InputProps, 'appearance'>;
      const editor =
        name === 'name'
          ? presentation.slot(
              'assistant-name',
              'input',
              {
                menu: editorMenu(props.theme, props.locale),
                ...common,
                appearance: props.theme.input,
              } satisfies InputProps,
              { changed: 'changed' },
            )
          : presentation.slot(
              'assistant-' + name,
              'editor',
              {
                menu: editorMenu(props.theme, props.locale),
                ...common,
                appearance: props.theme.textarea,
                softWrap: true,
                terminalRows: { min: 4, max: 24, resizable: true },
              } satisfies TextareaProps,
              { changed: 'changed' },
            );
      return ui.box(
        'assistant-field-' + name,
        [ui.text('assistant-label-' + name, t('assistant-' + name), props.theme.muted), editor],
        { gap: 0 },
      );
    });
    const content = ui.box(
      'assistant-preferences-content',
      [
        ui.choice!(
          'language',
          state.restoring || state.pending?.request.kind === 'preferences'
            ? t('assistant-language-loading')
            : settingsText(props.locale, 'settings-language'),
          [
            { key: 'auto', label: text.languageAuto },
            { key: 'en', label: 'English' },
            { key: 'zh-CN', label: '简体中文' },
            { key: 'zh-TW', label: '繁體中文' },
          ],
          state.saved?.language ?? null,
          busy || !state.saved,
        ),
        fields[0]!,
        ...(state.notice && !state.pending
          ? [ui.text('assistant-notice', t(state.notice), props.theme.muted)]
          : []),
        ui.button(
          'save',
          t('assistant-save'),
          busy || !state.fields.name.ready || !!state.fields.name.command,
        ),
        globalInstructionsView(state.global, props, presentation),
        fields[1]!,
        ui.button(
          'save-compaction',
          t('assistant-compaction-save'),
          busy || !state.fields.compaction.ready || !!state.fields.compaction.command,
        ),
        ui.button(
          'reset-compaction',
          t('assistant-compaction-reset'),
          busy ||
            !state.saved ||
            !state.fields.compaction.ready ||
            !!state.fields.compaction.command,
        ),
        ui.toggle(
          'instructions',
          t('assistant-instructions'),
          state.saved?.workspaceInstructions ?? false,
          busy || !state.saved,
          t('assistant-instructions-help'),
        ),
        ui.toggle(
          'shell-titles',
          t('assistant-shell-titles'),
          state.saved?.shellTitles ?? false,
          busy || !state.saved,
          t('assistant-shell-titles-help'),
        ),
        ui.toggle(
          'privacy',
          t('assistant-privacy'),
          state.saved?.incognitoActive ?? false,
          busy || !state.saved,
          t('assistant-privacy-help'),
        ),
        ...(state.current
          ? [
              ui.button(
                'receipt',
                text.executionReceipt,
                !!state.pending || !state.current.reference,
              ),
            ]
          : []),
        ...((state.restoring || (state.current && !state.current.reference)) && !state.pending
          ? [ui.button('history', text.refreshDefaultOperations)]
          : []),
        ui.button('reload', t('assistant-reload'), busy),
      ],
      { gap: 1, scroll: 'manual', terminal: { grow: 1 } },
    );
    const result = output(
      state,
      ui.box(
        'assistant-preferences-root',
        [...pageHeader(presentation, state, props, events, t('settings-assistant')), content],
        {
          visible: props.open,
          gap: 1,
          terminal: { grow: 1 },
          shortcuts: [{ key: 'escape', modifiers: [], input: 'back', enabled: true }],
        },
      ),
      events,
    );
    result.actions = actions;
    return result;
  }
  return withPageShell<State, ProjectsProps, N>(
    {
      mount(props) {
        const state: State = {
          revision: 0,
          opened: props.open,
          serial: 0,
          saved: null,
          fields: { name: field(), compaction: field() },
          global: globalInstructionsMount(),
          pending: null,
          current: null,
          capture: null,
          compactionCapture: null,
          restoring: true,
          notice: null,
        };
        const actions: Action[] = [];
        history(state, actions);
        globalInstructionsEvent(state.global, null, props, actions);
        return frame(state, props, actions);
      },
      update(state, props) {
        state.revision++;
        const actions: Action[] = [];
        if (props.open && !state.opened && !disabled(state, props)) read(state, actions, false);
        state.opened = props.open;
        return frame(state, props, actions);
      },
      event(state, input, props) {
        state.revision++;
        const actions: Action[] = [],
          events: Emission[] = [];
        globalInstructionsEvent(state.global, input, props, actions);
        if (input.kind === 'component') {
          const name = (['name', 'compaction'] as const).find(
            (name) => input.kind === 'component' && input.child === 'assistant-' + name,
          );
          if (name && input.name === 'changed') {
            const field = state.fields[name],
              observed = input.event as EditorObservation;
            if (
              field.observation &&
              field.observation.snapshot.text.owner !== observed.snapshot.text.owner
            ) {
              field.command = null;
              field.ready = false;
              field.reset =
                name === 'name'
                  ? (state.saved?.displayName ?? '')
                  : (state.saved?.compactionPrompt ?? '');
            }
            field.observation = observed;
            const receipt = observed.receipt;
            if (
              field.command &&
              receipt?.owner === field.command.owner &&
              receipt.sequence === field.command.sequence
            ) {
              field.ready = receipt.outcome.status === 'applied';
              field.command = null;
              field.reset = null;
              if (!field.ready) state.notice = 'assistant-unavailable';
            } else initialize(field);
          }
        } else if (input.kind === 'action' && input.key === state.pending?.key) {
          const r = state.pending.request,
            result = input.event as ActionOutcome;
          state.pending = null;
          if (r.kind === 'history') {
            if (result.status !== 'succeeded') state.notice = 'assistant-unavailable';
            else {
              const items = result.value as CallerIntent[];
              for (const item of items) {
                const recovery = item.recovery as {
                  kind?: string;
                  input?: SettingsPreferencesUpdate;
                } | null;
                if (
                  item.reference.method === 'updateAssistantPreferences' &&
                  item.reference.contract.name === 'maka.settings.Settings' &&
                  recovery?.kind === 'assistant-preferences/v1' &&
                  recovery.input
                )
                  state.current = { input: recovery.input, reference: item.reference };
              }
              const last = items.at(-1);
              if (last && last.cursor > r.after) history(state, actions, last.cursor);
              else if (last) state.notice = 'assistant-unavailable';
              else {
                state.restoring = false;
                if (state.current?.reference)
                  request(
                    state,
                    actions,
                    { kind: 'receipt' },
                    'receipts',
                    'read',
                    state.current.reference,
                  );
                else read(state, actions);
              }
            }
          } else if (r.kind === 'preferences') {
            if (result.status !== 'succeeded') state.notice = 'assistant-unavailable';
            else {
              const observed = result.value as SettingsAssistantPreferences;
              if (
                r.languageCommitted ||
                (state.saved &&
                  (state.saved.language !== observed.language ||
                    state.saved.resolvedLanguage !== observed.resolvedLanguage))
              )
                events.push({ name: 'changed', value: {} });
              state.saved = observed;
              if (r.reset)
                for (const name of (r.reset === 'compaction'
                  ? ['compaction']
                  : r.reset === 'personalization'
                    ? ['name']
                    : ['name', 'compaction']) as Name[]) {
                  state.fields[name].ready = false;
                  state.fields[name].reset =
                    name === 'name' ? state.saved.displayName : state.saved.compactionPrompt;
                  initialize(state.fields[name]);
                }
            }
          } else if (r.kind === 'mutate') {
            if (result.status === 'operation') settle(state, result.reply, actions);
            else if (result.status === 'not_accepted') {
              state.current = null;
              state.notice = 'assistant-unavailable';
            } else state.notice = 'assistant-unknown';
          } else if (r.kind === 'receipt') {
            if (result.status === 'succeeded')
              settle(state, result.value as OperationReply, actions);
            else state.notice = 'assistant-unknown';
          } else if (r.kind === 'field') {
            const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
            if (r.name === 'compaction') {
              const captured = state.compactionCapture;
              state.compactionCapture = null;
              if (
                captured &&
                chunk &&
                state.saved &&
                sameValue(chunk.metadata, captured.snapshot.text) &&
                sameValue(
                  state.fields.compaction.observation?.snapshot.text,
                  captured.snapshot.text,
                ) &&
                new TextEncoder().encode(chunk.text).length === captured.snapshot.text.bytes &&
                chunk.text.trim()
              )
                mutate(state, actions, {
                  expectedRevision: state.saved.revision,
                  mutation: { kind: 'compaction_prompt', prompt: chunk.text },
                });
              else state.notice = 'assistant-invalid';
            } else {
              const captured = state.capture?.name;
              state.capture = null;
              if (
                captured &&
                chunk &&
                state.saved &&
                sameValue(chunk.metadata, captured.snapshot.text) &&
                sameValue(state.fields.name.observation?.snapshot.text, captured.snapshot.text) &&
                new TextEncoder().encode(chunk.text).length === captured.snapshot.text.bytes &&
                [...chunk.text].length <= 256
              )
                mutate(state, actions, {
                  expectedRevision: state.saved.revision,
                  mutation: { kind: 'personalization', displayName: chunk.text, assistantTone: '' },
                });
              else state.notice = 'assistant-invalid';
            }
          }
        } else if (input.kind === 'raw' && props.open) {
          if (input.key === 'back') events.push({ name: 'closed', value: {} });
          else if (
            input.key === 'history' &&
            !state.pending &&
            (state.restoring || (state.current && !state.current.reference))
          )
            history(state, actions);
          else if (input.key === 'receipt' && !state.pending && state.current?.reference)
            request(
              state,
              actions,
              { kind: 'receipt' },
              'receipts',
              'read',
              state.current.reference,
            );
          else if (!disabled(state, props)) {
            if (input.key === 'reload') read(state, actions);
            else if (input.key === 'language' && state.saved) {
              const preference = (input.event as { key?: string }).key;
              if (
                preference === 'auto' ||
                preference === 'en' ||
                preference === 'zh-CN' ||
                preference === 'zh-TW'
              )
                mutate(state, actions, {
                  expectedRevision: state.saved.revision,
                  mutation: { kind: 'language', preference },
                });
            } else if (
              input.key === 'reset-compaction' &&
              state.saved &&
              state.fields.compaction.ready &&
              !state.fields.compaction.command
            )
              mutate(state, actions, {
                expectedRevision: state.saved.revision,
                mutation: { kind: 'compaction_prompt', prompt: null },
              });
            else if (input.key === 'save-compaction' && state.saved) {
              const draft = state.fields.compaction.observation;
              if (
                draft &&
                !draft.snapshot.marked &&
                state.fields.compaction.ready &&
                !state.fields.compaction.command
              ) {
                state.compactionCapture = draft;
                capture(state, actions, 'compaction');
              }
            } else if (input.key === 'instructions' && state.saved)
              mutate(state, actions, {
                expectedRevision: state.saved.revision,
                mutation: {
                  kind: 'workspace_instructions',
                  enabled: !state.saved.workspaceInstructions,
                },
              });
            else if (input.key === 'privacy' && state.saved)
              mutate(state, actions, {
                expectedRevision: state.saved.revision,
                mutation: { kind: 'privacy', incognitoActive: !state.saved.incognitoActive },
              });
            else if (input.key === 'shell-titles' && state.saved)
              mutate(state, actions, {
                expectedRevision: state.saved.revision,
                mutation: { kind: 'shell_titles', enabled: !state.saved.shellTitles },
              });
            else if (input.key === 'save' && state.saved) {
              const name = state.fields.name.observation;
              if (
                name &&
                !name.snapshot.marked &&
                name.snapshot.text.bytes <= 1024 &&
                !state.fields.name.command &&
                state.fields.name.ready
              ) {
                state.capture = { name };
                capture(state, actions, 'name');
              } else state.notice = 'assistant-invalid';
            }
          }
        }
        return frame(state, props, actions, events);
      },
    },
    () => 'back',
  );
}
export function activate() {
  return {
    AssistantPreferences: {
      kind: 'component',
      contract: assistantPreferencesContract,
      component: ['component'],
    },
  };
}
