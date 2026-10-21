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
  TextChunk,
  WorkHubPreset,
  WorkHubPresets,
  WorkHubPresetsResult,
  AgentToolNames,
} from '../../../packages/plugin-sdk/src/index.js';
import {
  subagentSettingsContract,
  type ProjectsProps,
  type SelectedModel,
  type ModelsProps,
} from '../../shared/contracts.js';
import { withPageShell, pageHeader, type PageState } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { copy, tuiText } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

const names = ['name', 'description'] as const;
type Name = (typeof names)[number];
interface Field {
  observation: EditorObservation | null;
  command: EditorCommand | null;
  reset: string | null;
  ready: boolean;
  sequence: number;
}
type Request =
  | { kind: 'read' | 'tools' | 'mutate' | 'receipt' }
  | { kind: 'history'; after: number }
  | { kind: 'field'; name: Name };
interface State extends PageState {
  revision: number;
  serial: number;
  saved: WorkHubPresets | null;
  tools: string[] | null;
  draft: WorkHubPreset | null;
  draftRevision: number | null;
  selection: SelectedModel | null;
  modelEpoch: number;
  fields: Record<Name, Field>;
  pending: { key: string; request: Request } | null;
  current: { input: WorkHubPresets; reference: RecoveryRef | null } | null;
  capture: {
    observations: Record<Name, EditorObservation>;
    values: Partial<Record<Name, string>>;
  } | null;
  removing: boolean;
  restoring: boolean;
  notice: string | null;
}
function field(): Field {
  return { observation: null, command: null, reset: null, ready: false, sequence: 0 };
}
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
  r: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  if (state.pending) return;
  const key = 'subagents-' + ++state.serial;
  state.pending = { key, request: r };
  actions.push({ key, binding, member: 0, method, input, resources });
}
function read(state: State, actions: Action[]) {
  request(state, actions, { kind: 'read' }, 'workhub', 'presets', {});
}
function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  if (!after) state.current = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}
function busy(state: State, props: ProjectsProps) {
  return (
    !props.open ||
    props.busy ||
    !!state.pending ||
    !!state.current ||
    !!state.capture ||
    state.restoring
  );
}
function edit(state: State, preset: WorkHubPreset) {
  state.draft = { ...preset, tools: [...preset.tools] };
  state.draftRevision = state.saved?.revision ?? null;
  state.selection = null;
  state.modelEpoch++;
  for (const name of names) {
    state.fields[name].ready = false;
    state.fields[name].reset = preset[name];
    initialize(state.fields[name]);
  }
}
function mutate(state: State, actions: Action[], entries: WorkHubPreset[]) {
  const input = { revision: state.draftRevision, entries };
  state.current = { input, reference: null };
  state.notice = null;
  request(state, actions, { kind: 'mutate' }, 'workhub', 'savePresets', input);
  actions.at(-1)!.recovery = { kind: 'subagent-settings/v1', input };
}
function settle(state: State, reply: OperationReply, actions: Action[]) {
  const current = state.current;
  if (!current || (current.reference && !sameValue(current.reference, reply.reference))) return;
  current.reference = reply.reference;
  if (
    reply.receipt.kind !== 'recorded' ||
    reply.receipt.outcome.kind !== 'returned' ||
    reply.receipt.cleanup.kind !== 'confirmed'
  ) {
    state.notice = 'subagents-unknown';
    return;
  }
  const result = reply.receipt.outcome.value as WorkHubPresetsResult;
  if (!['saved', 'conflict', 'invalid'].includes(result.kind)) {
    state.notice = 'subagents-unknown';
    return;
  }
  state.current = null;
  state.notice =
    result.kind === 'saved'
      ? 'subagents-saved'
      : result.kind === 'conflict'
        ? 'subagents-conflict'
        : 'subagents-invalid';
  if (result.kind === 'saved') {
    state.saved = result.snapshot;
    state.draftRevision = result.snapshot.revision;
    if (state.draft && !result.snapshot.entries.some((p) => p.id === state.draft!.id))
      state.draft = null;
  }
  // Recovery always follows the original operation with a fresh catalog read.
  if (!state.draft) read(state, actions);
}
function capture(state: State, actions: Action[], name: Name) {
  const observed = state.capture!.observations[name];
  request(
    state,
    actions,
    { kind: 'field', name },
    'reader',
    'read',
    { start: 0, end: observed.snapshot.text.bytes },
    {
      text: {
        child: 'subagent-' + name,
        method: 'snapshot',
        input: {
          kind: 'at_revision',
          owner: observed.snapshot.text.owner,
          revision: observed.snapshot.text.revision,
        },
      },
    },
  );
}
export function subagentSettings<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ProjectsProps, N> {
  function frame(
    state: State,
    props: ProjectsProps,
    actions: Action[] = [],
    events: Emission[] = [],
  ) {
    const ui = presentation.paint(props.theme),
      t = (key: string) => tuiText(props.locale, key),
      text = copy(props.locale);
    const disabled = busy(state, props) || !state.saved || !state.tools || state.removing;
    const editor = (name: Name) =>
      ui.box(
        'subagent-field-' + name,
        [
          ui.text('subagent-label-' + name, t('subagents-' + name), props.theme.muted),
          presentation.slot(
            'subagent-' + name,
            'input',
            {
              menu: editorMenu(props.theme, props.locale),
              accessibilityLabel: t('subagents-' + name),
              placeholder: '',
              autoFocus: name === 'name',
              tabIndex: names.indexOf(name) + 1,
              readOnly: false,
              disabled:
                disabled ||
                !state.draft ||
                !state.fields[name].ready ||
                !!state.fields[name].command,
              command: state.fields[name].command,
              appearance: {
                ...props.theme.input,
                borderWidth: 0,
                padding: { top: 0, right: 0, bottom: 0, left: 0 },
              },
            } satisfies InputProps,
            { changed: 'changed' },
          ),
        ],
        { gap: 0 },
      );
    const draft = state.draft;
    const content = ui.box(
      'subagents-content',
      [
        ui.text('subagents-help', t('subagents-help'), props.theme.muted),
        ...(state.saved?.entries ?? [])
          .filter((p) => p.worker === 'worker')
          .map((p) =>
            ui.button(
              'edit-' + p.id,
              p.name + (p.enabled ? '' : ' · ' + text.settingOff),
              disabled,
            ),
          ),
        ui.button('new', t('subagents-new'), disabled),
        ui.box(
          'subagent-form',
          [
            ...names.map(editor),
            ui.text('subagent-id', t('subagents-id') + ': ' + (draft?.id ?? ''), props.theme.muted),
            ui.toggle(
              'enabled',
              t('subagents-enabled'),
              draft?.enabled ?? false,
              disabled,
              t('subagents-enabled-help'),
            ),
            ui.toggle(
              'isolated',
              t('subagents-isolated'),
              draft?.isolatedWorkspace ?? false,
              disabled,
              t('subagents-isolated-help'),
            ),
            ui.text(
              'subagent-current-model',
              t('subagents-model') +
                ': ' +
                (draft?.model.modelId || t('subagents-choose-model')) +
                (draft?.thinkingLevel ? ' · ' + draft.thinkingLevel : ''),
              props.theme.text,
            ),
            presentation.slot(
              'subagent-models',
              'models',
              {
                catalogEpoch: state.modelEpoch,
                openRequest: null,
                theme: props.theme,
                locale: props.locale,
                selection: state.selection,
                suggestDefault: false,
                busy: disabled || !draft,
              } satisfies ModelsProps,
              { selected: 'model', setup: 'setup', connections: 'connections' },
            ),
            ui.text('subagent-tools-help', t('subagents-tools-help'), props.theme.muted),
            ui.box(
              'subagent-profiles',
              [
                ui.button('profile-read', t('subagents-read'), disabled),
                ui.button('profile-web', t('subagents-web'), disabled),
                ui.button('profile-all', t('subagents-all'), disabled),
                ui.button('profile-none', t('subagents-none'), disabled),
              ],
              { gap: 0 },
            ),
            ui.text(
              'subagents-selected',
              t('subagents-selected') + ': ' + (draft?.tools.length ?? 0),
              props.theme.muted,
            ),
            ...(state.tools ?? []).map((name, at) =>
              ui.button(
                'tool-' + at,
                (draft?.tools.includes(name) ? '[x] ' : '[ ] ') + name,
                disabled,
              ),
            ),
            ...(draft?.tools.filter((name) => !state.tools?.includes(name)) ?? []).map((name) =>
              ui.text(
                'missing-' + name,
                t('subagents-missing-tool') + ': ' + name,
                props.theme.muted,
              ),
            ),
            ui.text(
              'subagents-save-notice',
              state.pending ? t('subagents-loading') : state.notice ? t(state.notice) : ' ',
              props.theme.muted,
            ),
            ui.button(
              'save',
              t('subagents-save'),
              disabled ||
                !draft?.model.modelId ||
                names.some((name) => !state.fields[name].ready || !!state.fields[name].command),
            ),
            ui.button(
              'remove',
              t('subagents-remove'),
              disabled || !state.saved?.entries.some((p) => p.id === draft?.id),
            ),
          ],
          { visible: !!draft, gap: 1 },
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
        ui.button('reload', t('subagents-reload'), busy(state, props) || state.removing),
      ],
      { gap: 1, scroll: 'manual', terminal: { grow: 1 } },
    );
    const confirmation = ui.box(
      'subagent-confirmation',
      [
        ui.text('subagent-remove-question', t('subagents-remove-question'), props.theme.text),
        ui.button('confirm-remove', t('subagents-remove'), busy(state, props)),
        ui.button('cancel', text.cancel),
      ],
      {
        gap: 1,
        shortcuts: [{ key: 'escape', modifiers: [], input: 'cancel', enabled: state.removing }],
      },
    );
    const result = output(
      state,
      ui.box(
        'subagent-settings',
        [
          ...pageHeader(presentation, state, props, events, t('settings-subagents')),
          ...((state.pending || state.restoring) && !draft
            ? [ui.text('subagents-loading', t('subagents-loading'), props.theme.muted)]
            : []),
          ...(state.notice && !state.pending && !draft
            ? [ui.text('subagents-notice', t(state.notice), props.theme.muted)]
            : []),
          ui.dialog(
            'subagent-removal',
            t('subagents-remove'),
            content,
            confirmation,
            state.removing,
            { width: 52, height: 10 },
          ),
        ],
        {
          visible: props.open,
          gap: 0,
          terminal: { grow: 1 },
          shortcuts: [
            {
              key: 'escape',
              modifiers: [],
              input: state.removing ? 'cancel' : 'back',
              enabled: true,
            },
          ],
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
          serial: 0,
          saved: null,
          tools: null,
          draft: null,
          draftRevision: null,
          selection: null,
          modelEpoch: 0,
          fields: { name: field(), description: field() },
          pending: null,
          current: null,
          capture: null,
          removing: false,
          restoring: true,
          notice: null,
        };
        const actions: Action[] = [];
        history(state, actions);
        return frame(state, props, actions);
      },
      update(state, props) {
        state.revision++;
        return frame(state, props);
      },
      event(state, input, props) {
        state.revision++;
        const actions: Action[] = [],
          events: Emission[] = [];
        if (input.kind === 'component' && input.child === 'subagent-models') {
          if (input.name === 'selected' && state.draft && !busy(state, props) && !state.removing) {
            const selected = input.event as SelectedModel;
            state.selection = selected;
            state.draft.model = {
              connectionId: selected.choice.selection.connectionId,
              modelId: selected.choice.selection.modelId,
            };
            state.draft.thinkingLevel = selected.thinkingLevel;
          } else if (input.name === 'connections') state.notice = 'subagents-connections';
        } else if (input.kind === 'component' && input.name === 'changed') {
          const name = names.find(
            (name) => input.kind === 'component' && input.child === 'subagent-' + name,
          );
          if (name) {
            const field = state.fields[name],
              observed = input.event as EditorObservation;
            if (
              field.observation &&
              field.observation.snapshot.text.owner !== observed.snapshot.text.owner
            ) {
              field.command = null;
              field.ready = false;
              field.reset = state.draft?.[name] ?? '';
            }
            field.observation = observed;
            if (
              field.command &&
              observed.receipt?.owner === field.command.owner &&
              observed.receipt.sequence === field.command.sequence
            ) {
              field.ready = observed.receipt.outcome.status === 'applied';
              field.command = null;
              field.reset = null;
              if (!field.ready) state.notice = 'subagents-unavailable';
            } else initialize(field);
          }
        } else if (input.kind === 'action' && input.key === state.pending?.key) {
          const r = state.pending.request,
            result = input.event as ActionOutcome;
          state.pending = null;
          if (r.kind === 'history') {
            if (result.status !== 'succeeded') state.notice = 'subagents-unavailable';
            else {
              const items = result.value as CallerIntent[];
              for (const item of items) {
                const recovery = item.recovery as { kind?: string; input?: WorkHubPresets } | null;
                if (
                  item.reference.contract.name === 'maka.workhub.Coordinator' &&
                  item.reference.method === 'savePresets' &&
                  recovery?.kind === 'subagent-settings/v1' &&
                  recovery.input
                )
                  state.current = { input: recovery.input, reference: item.reference };
              }
              const last = items.at(-1);
              if (last && last.cursor > r.after) history(state, actions, last.cursor);
              else if (last) state.notice = 'subagents-unavailable';
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
          } else if (r.kind === 'read') {
            if (result.status !== 'succeeded') state.notice = 'subagents-unavailable';
            else {
              state.saved = result.value as WorkHubPresets;
              if (!state.tools) request(state, actions, { kind: 'tools' }, 'agent', 'tools', {});
            }
          } else if (r.kind === 'tools') {
            if (result.status === 'succeeded') state.tools = (result.value as AgentToolNames).names;
            else state.notice = 'subagents-unavailable';
          } else if (r.kind === 'mutate') {
            if (result.status === 'operation') settle(state, result.reply, actions);
            else if (result.status === 'not_accepted') {
              state.current = null;
              state.notice = 'subagents-invalid';
            } else state.notice = 'subagents-unknown';
          } else if (r.kind === 'receipt') {
            if (result.status === 'succeeded')
              settle(state, result.value as OperationReply, actions);
            else state.notice = 'subagents-unknown';
          } else if (r.kind === 'field') {
            const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null,
              captured = state.capture;
            if (
              !captured ||
              !chunk ||
              !sameValue(chunk.metadata, captured.observations[r.name].snapshot.text) ||
              !sameValue(state.fields[r.name].observation?.snapshot.text, chunk.metadata)
            ) {
              state.capture = null;
              state.notice = 'subagents-invalid';
            } else {
              captured.values[r.name] = chunk.text;
              if (r.name === 'name') capture(state, actions, 'description');
              else {
                state.capture = null;
                if (
                  !state.draft ||
                  !state.saved ||
                  !captured.values.name?.trim() ||
                  names.some(
                    (name) =>
                      !sameValue(
                        state.fields[name].observation?.snapshot.text,
                        captured.observations[name].snapshot.text,
                      ),
                  )
                )
                  state.notice = 'subagents-invalid';
                else {
                  state.draft.name = captured.values.name;
                  state.draft.description = captured.values.description ?? '';
                  const entries = state.saved.entries.filter((p) => p.id !== state.draft!.id);
                  entries.push({ ...state.draft, tools: [...state.draft.tools] });
                  mutate(state, actions, entries);
                }
              }
            }
          }
        } else if (input.kind === 'raw' && props.open) {
          if (input.key === 'back') {
            if (state.removing) state.removing = false;
            else events.push({ name: 'closed', value: {} });
          } else if (input.key === 'cancel') state.removing = false;
          else if (input.key === 'receipt' && !state.pending && state.current?.reference)
            request(
              state,
              actions,
              { kind: 'receipt' },
              'receipts',
              'read',
              state.current.reference,
            );
          else if (
            input.key === 'history' &&
            !state.pending &&
            (state.restoring || (state.current && !state.current.reference))
          )
            history(state, actions);
          else if (!busy(state, props) && state.saved && state.tools) {
            if (input.key === 'confirm-remove' && state.removing && state.draft) {
              state.removing = false;
              mutate(
                state,
                actions,
                state.saved.entries.filter((p) => p.id !== state.draft!.id),
              );
            } else if (!state.removing) {
              if (input.key === 'reload') {
                state.draft = null;
                state.tools = null;
                read(state, actions);
              } else if (input.key === 'new') {
                let id = 1;
                while (state.saved.entries.some((p) => p.id === 'agent-' + id)) id++;
                edit(state, {
                  id: 'agent-' + id,
                  name: '',
                  description: '',
                  worker: 'worker',
                  isolatedWorkspace: false,
                  tools: [],
                  model: { connectionId: '', modelId: '' },
                  thinkingLevel: null,
                  enabled: true,
                });
              } else if (input.key.startsWith('edit-')) {
                const preset = state.saved.entries.find(
                  (p) => p.id === input.key.slice(5) && p.worker === 'worker',
                );
                if (preset) edit(state, preset);
              } else if (state.draft) {
                if (input.key === 'enabled') state.draft.enabled = !state.draft.enabled;
                else if (input.key === 'isolated')
                  state.draft.isolatedWorkspace = !state.draft.isolatedWorkspace;
                else if (input.key === 'remove') state.removing = true;
                else if (input.key === 'profile-read')
                  state.draft.tools = state.tools.filter((name) =>
                    ['Read', 'ReadImage', 'Glob', 'Grep'].includes(name),
                  );
                else if (input.key === 'profile-web')
                  state.draft.tools = state.tools.filter((name) =>
                    ['WebFetch', 'WebSearch'].includes(name),
                  );
                else if (input.key === 'profile-all') state.draft.tools = [...state.tools];
                else if (input.key === 'profile-none') state.draft.tools = [];
                else if (input.key.startsWith('tool-')) {
                  const name = state.tools[Number(input.key.slice(5))];
                  if (name)
                    state.draft.tools = state.draft.tools.includes(name)
                      ? state.draft.tools.filter((n) => n !== name)
                      : [...state.draft.tools, name];
                } else if (input.key === 'save') {
                  if (
                    !state.draft.model.modelId ||
                    names.some(
                      (name) =>
                        !state.fields[name].ready ||
                        !!state.fields[name].command ||
                        !state.fields[name].observation ||
                        state.fields[name].observation!.snapshot.marked,
                    )
                  )
                    state.notice = 'subagents-invalid';
                  else {
                    state.capture = {
                      observations: {
                        name: state.fields.name.observation!,
                        description: state.fields.description.observation!,
                      },
                      values: {},
                    };
                    capture(state, actions, 'name');
                  }
                }
              }
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
    SubagentSettings: {
      kind: 'component',
      contract: subagentSettingsContract,
      component: ['component'],
    },
  };
}
