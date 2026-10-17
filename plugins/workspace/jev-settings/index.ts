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
  JevSnapshot,
  JevSettings,
  JevSet,
  JevCredential,
  JevEvaluation,
  JevResult,
  JevUpdate,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { jevSettingsContract, type ProjectsProps } from '../../shared/contracts.js';
import { withPageShell, pageHeader, type PageState } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { copy, tuiText } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

const settingsFields = ['url', 'model', 'timeoutMs'] as const;
const names = [...settingsFields, 'headerName', 'secret'] as const;
type Name = (typeof names)[number];
interface Field {
  observation: EditorObservation | null;
  command: EditorCommand | null;
  reset: string | null;
  sequence: number;
  ready: boolean;
}
type Mutation =
  | { method: 'save'; input: JevSet }
  | { method: 'credential'; input: JevCredential }
  | { method: 'evaluate'; input: JevEvaluation };
type Request =
  | { kind: 'snapshot'; reset: boolean }
  | { kind: 'mutate' | 'receipt' }
  | { kind: 'history'; after: number }
  | { kind: 'field'; name: Name };
interface State extends PageState {
  revision: number;
  serial: number;
  saved: JevSnapshot | null;
  draft: JevSettings | null;
  draftRevision: number | null;
  fields: Record<Name, Field>;
  pending: { key: string; request: Request } | null;
  current: {
    mutation: Mutation;
    reference: RecoveryRef | null;
    secret: EditorObservation | null;
  } | null;
  capture: {
    names: Name[];
    observations: Partial<Record<Name, EditorObservation>>;
    values: Partial<Record<Name, string>>;
    secret: EditorObservation | null;
  } | null;
  confirming: JevCredential | null;
  restoring: boolean;
  notice: string | null;
  testResult: string | null;
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
  r: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  if (state.pending) return;
  const key = 'jev-settings-' + ++state.serial;
  state.pending = { key, request: r };
  actions.push({ key, binding, member: 0, method, input, resources });
}
function read(state: State, actions: Action[], reset = false) {
  request(state, actions, { kind: 'snapshot', reset }, 'jev', 'snapshot', {});
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
function resource(observed: EditorObservation, name: Name) {
  return {
    child: 'jev-' + name,
    method: 'snapshot',
    input: {
      kind: 'at_revision',
      owner: observed.snapshot.text.owner,
      revision: observed.snapshot.text.revision,
    },
  };
}
function mutate(
  state: State,
  actions: Action[],
  mutation: Mutation,
  secret: EditorObservation | null = null,
) {
  state.current = { mutation, reference: null, secret };
  state.notice = null;
  request(
    state,
    actions,
    { kind: 'mutate' },
    'jev',
    mutation.method,
    mutation.input,
    secret ? { secret: resource(secret, 'secret') } : {},
  );
  actions.at(-1)!.recovery = { kind: 'jev-settings/v1', ...mutation };
}
function credential(
  state: State,
  target: JevCredential['target'],
  change: JevCredential['change'],
): JevCredential {
  return {
    url: state.saved!.settings.url,
    revision: state.saved!.credentialRevision,
    target,
    change,
  };
}
function capture(state: State, actions: Action[], name: Name) {
  const observed = state.capture!.observations[name]!;
  request(
    state,
    actions,
    { kind: 'field', name },
    'reader',
    'read',
    { start: 0, end: observed.snapshot.text.bytes },
    { text: resource(observed, name) },
  );
}
function beginCapture(
  state: State,
  actions: Action[],
  fields: Name[],
  secret: EditorObservation | null = null,
) {
  if (
    fields.some(
      (name) =>
        !state.fields[name].ready ||
        state.fields[name].command ||
        !state.fields[name].observation ||
        state.fields[name].observation!.snapshot.marked ||
        state.fields[name].observation!.snapshot.text.bytes > 16384,
    )
  ) {
    state.notice = 'jev-invalid';
    return;
  }
  state.capture = {
    names: fields,
    observations: Object.fromEntries(fields.map((name) => [name, state.fields[name].observation!])),
    values: {},
    secret,
  };
  capture(state, actions, fields[0]!);
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
    state.notice = 'jev-unknown';
    return;
  }
  if (current.mutation.method === 'evaluate') {
    const result = reply.receipt.outcome.value as JevResult;
    if (result.kind !== 'completed' && result.kind !== 'failed') {
      state.notice = 'jev-unknown';
      return;
    }
    state.testResult = result.kind === 'completed' ? result.value.model : null;
    state.notice = result.kind === 'completed' ? 'jev-tested' : 'jev-test-failed';
    state.current = null;
    read(state, actions);
    return;
  }
  const result = reply.receipt.outcome.value as JevUpdate;
  if (!['updated', 'changed', 'invalid'].includes(result.kind)) {
    state.notice = 'jev-unknown';
    return;
  }
  if (
    result.kind === 'updated' &&
    current.secret &&
    state.fields.secret.observation?.snapshot.text.owner === current.secret.snapshot.text.owner
  )
    state.fields.secret.command = {
      owner: current.secret.snapshot.text.owner,
      sequence: ++state.fields.secret.sequence,
      basedOn: current.secret.revision,
      edit: { kind: 'reset', text: '' },
    };
  state.current = null;
  state.notice =
    result.kind === 'updated'
      ? 'jev-saved'
      : result.kind === 'changed'
        ? 'jev-conflict'
        : 'jev-invalid';
  // Preserve unsaved endpoint/model edits during an independent credential change.
  // Conflicts keep their old CAS basis until the user explicitly reloads.
  if (result.kind === 'updated' || !state.saved)
    read(state, actions, result.kind === 'updated' && current.mutation.method === 'save');
}
export function jevSettings<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ProjectsProps, N> {
  function frame(
    state: State,
    props: ProjectsProps,
    actions: Action[] = [],
    events: Emission[] = [],
  ) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale),
      t = (key: string) => tuiText(props.locale, key);
    const disabled = busy(state, props) || !state.saved || !!state.confirming;
    const secret = state.fields.secret.observation;
    const canSaveSecret =
      !!secret &&
      !secret.snapshot.marked &&
      secret.snapshot.text.bytes > 0 &&
      secret.snapshot.text.bytes <= 4096 &&
      !state.fields.secret.command;
    const editor = (name: Name) =>
      ui.box(
        'jev-field-' + name,
        [
          ui.text('jev-label-' + name, t('jev-' + name), props.theme.muted),
          presentation.slot(
            'jev-' + name,
            name === 'secret' ? 'secret' : 'input',
            {
              menu: editorMenu(props.theme, props.locale),
              accessibilityLabel: t('jev-' + name),
              placeholder: '',
              autoFocus: name === 'url',
              tabIndex: names.indexOf(name) + 1,
              readOnly: false,
              disabled:
                disabled ||
                !!state.fields[name].command ||
                (name !== 'secret' && !state.fields[name].ready),
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
    const content = ui.box(
      'jev-content',
      [
        ui.toggle(
          'enabled',
          t('jev-enabled'),
          state.draft?.enabled ?? false,
          disabled,
          t('jev-help'),
        ),
        ...settingsFields.map(editor),
        ui.button(
          'save',
          t('jev-save'),
          disabled ||
            settingsFields.some(
              (name) => !state.fields[name].ready || !!state.fields[name].command,
            ),
        ),
        ui.text(
          'jev-saved-endpoint',
          t('jev-saved-endpoint') + ': ' + (state.saved?.settings.url ?? '…'),
          props.theme.muted,
        ),
        ui.text(
          'jev-key-status',
          t(state.saved?.apiKeyConfigured ? 'jev-key-present' : 'jev-key-absent'),
          props.theme.muted,
        ),
        ui.text('jev-credential-help', t('jev-credential-help'), props.theme.muted),
        editor('secret'),
        ui.button('save-key', t('jev-save-key'), disabled || !canSaveSecret),
        ui.button('remove-key', t('jev-remove-key'), disabled || !state.saved?.apiKeyConfigured),
        ...(state.saved?.headerNames ?? []).map((name, index) =>
          ui.button('remove-header-' + index, t('jev-remove-header') + ': ' + name, disabled),
        ),
        editor('headerName'),
        ui.button(
          'save-header',
          t('jev-save-header'),
          disabled || !canSaveSecret || !state.fields.headerName.ready,
        ),
        ui.button(
          'test',
          t('jev-test'),
          disabled || !state.saved?.settings.enabled || !state.saved.configured,
        ),
        ui.text('jev-test-help', t('jev-test-help'), props.theme.muted),
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
        ui.button('reload', t('jev-reload'), disabled),
      ],
      { gap: 1, scroll: 'manual', terminal: { grow: 1 } },
    );
    const confirmation = ui.box(
      'jev-confirmation',
      [
        ui.text('jev-remove-question', t('jev-remove-question'), props.theme.text),
        ui.button('confirm-remove', t('jev-remove'), busy(state, props)),
        ui.button('cancel', text.cancel),
      ],
      {
        gap: 1,
        shortcuts: [{ key: 'escape', modifiers: [], input: 'cancel', enabled: !!state.confirming }],
      },
    );
    const result = output(
      state,
      ui.box(
        'jev-settings',
        [
          ...pageHeader(presentation, state, props, events, 'Jev'),
          ...(state.notice && !state.pending
            ? [
                ui.text(
                  'jev-notice',
                  t(state.notice) +
                    (state.notice === 'jev-tested' && state.testResult
                      ? ': ' + state.testResult
                      : ''),
                  props.theme.muted,
                ),
              ]
            : []),
          ui.dialog('jev-removal', t('jev-remove'), content, confirmation, !!state.confirming, {
            width: 52,
            height: 10,
          }),
        ],
        {
          visible: props.open,
          gap: 0,
          terminal: { grow: 1 },
          shortcuts: [
            {
              key: 'escape',
              modifiers: [],
              input: state.confirming ? 'cancel' : 'back',
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
          draft: null,
          draftRevision: null,
          fields: Object.fromEntries(names.map((name) => [name, field()])) as Record<Name, Field>,
          pending: null,
          current: null,
          capture: null,
          confirming: null,
          restoring: true,
          notice: null,
          testResult: null,
        };
        state.fields.headerName.reset = '';
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
        if (input.kind === 'component' && input.name === 'changed') {
          const name = names.find(
            (name) => input.kind === 'component' && input.child === 'jev-' + name,
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
              field.reset =
                name === 'secret' || name === 'headerName'
                  ? ''
                  : state.draft
                    ? String(state.draft[name])
                    : null;
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
              if (!field.ready) state.notice = 'jev-unavailable';
            } else initialize(field);
          }
        } else if (input.kind === 'action' && input.key === state.pending?.key) {
          const r = state.pending.request,
            result = input.event as ActionOutcome;
          state.pending = null;
          if (r.kind === 'history') {
            if (result.status !== 'succeeded') state.notice = 'jev-unavailable';
            else {
              const items = result.value as CallerIntent[];
              for (const item of items) {
                const recovery = item.recovery as (Mutation & { kind?: string }) | null;
                if (
                  item.reference.contract.name === 'maka.workspace.Jev' &&
                  recovery?.kind === 'jev-settings/v1' &&
                  recovery.input &&
                  ['save', 'credential', 'evaluate'].includes(recovery.method) &&
                  recovery.method === item.reference.method
                )
                  state.current = { mutation: recovery, reference: item.reference, secret: null };
              }
              const last = items.at(-1);
              if (last && last.cursor > r.after) history(state, actions, last.cursor);
              else if (last) state.notice = 'jev-unavailable';
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
          } else if (r.kind === 'snapshot') {
            if (result.status !== 'succeeded') state.notice = 'jev-unavailable';
            else {
              state.saved = result.value as JevSnapshot;
              if (r.reset || !state.draft) {
                state.draft = { ...state.saved.settings };
                state.draftRevision = state.saved.revision ?? null;
                for (const name of settingsFields) {
                  state.fields[name].ready = false;
                  state.fields[name].reset = String(state.draft[name]);
                  initialize(state.fields[name]);
                }
              }
            }
          } else if (r.kind === 'mutate') {
            if (result.status === 'operation') settle(state, result.reply, actions);
            else if (result.status === 'not_accepted') {
              state.current = null;
              state.notice = 'jev-invalid';
            } else state.notice = 'jev-unknown';
          } else if (r.kind === 'receipt') {
            if (result.status === 'succeeded')
              settle(state, result.value as OperationReply, actions);
            else state.notice = 'jev-unknown';
          } else if (r.kind === 'field') {
            const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null,
              captured = state.capture;
            if (
              !captured ||
              !chunk ||
              !sameValue(chunk.metadata, captured.observations[r.name]!.snapshot.text) ||
              !sameValue(state.fields[r.name].observation?.snapshot.text, chunk.metadata)
            ) {
              state.capture = null;
              state.notice = 'jev-invalid';
            } else {
              captured.values[r.name] = chunk.text;
              const next = captured.names[captured.names.indexOf(r.name) + 1];
              if (next) capture(state, actions, next);
              else {
                state.capture = null;
                if (
                  !state.saved ||
                  !state.draft ||
                  captured.names.some(
                    (name) =>
                      !sameValue(
                        state.fields[name].observation?.snapshot.text,
                        captured.observations[name]!.snapshot.text,
                      ),
                  )
                )
                  state.notice = 'jev-invalid';
                else if (captured.names[0] === 'headerName')
                  mutate(
                    state,
                    actions,
                    {
                      method: 'credential',
                      input: credential(
                        state,
                        { kind: 'header', name: captured.values.headerName!.trim() },
                        'replace',
                      ),
                    },
                    captured.secret,
                  );
                else {
                  const timeoutMs = Number(captured.values.timeoutMs);
                  if (
                    !/^\d+$/.test(captured.values.timeoutMs!) ||
                    !Number.isInteger(timeoutMs) ||
                    timeoutMs < 100 ||
                    timeoutMs > 60000 ||
                    !captured.values.url ||
                    !captured.values.model?.trim()
                  )
                    state.notice = 'jev-invalid';
                  else
                    mutate(state, actions, {
                      method: 'save',
                      input: {
                        revision: state.draftRevision,
                        settings: {
                          enabled: state.draft.enabled,
                          url: captured.values.url!,
                          model: captured.values.model!,
                          timeoutMs,
                        },
                      },
                    });
                }
              }
            }
          }
        } else if (input.kind === 'raw' && props.open) {
          if (input.key === 'back') {
            if (state.confirming) state.confirming = null;
            else events.push({ name: 'closed', value: {} });
          } else if (input.key === 'cancel') state.confirming = null;
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
          else if (!busy(state, props)) {
            if (input.key === 'confirm-remove' && state.confirming) {
              const input = state.confirming;
              state.confirming = null;
              mutate(state, actions, { method: 'credential', input });
            } else if (!state.confirming) {
              if (input.key === 'reload') read(state, actions, true);
              else if (state.saved && state.draft) {
                const secret = state.fields.secret.observation;
                if (input.key === 'enabled') state.draft.enabled = !state.draft.enabled;
                else if (input.key === 'save') beginCapture(state, actions, [...settingsFields]);
                else if (input.key === 'remove-key' && state.saved.apiKeyConfigured)
                  state.confirming = credential(state, { kind: 'api_key' }, 'remove');
                else if (input.key.startsWith('remove-header-')) {
                  const name = state.saved.headerNames[Number(input.key.slice(14))];
                  if (name)
                    state.confirming = credential(state, { kind: 'header', name }, 'remove');
                } else if (
                  input.key === 'test' &&
                  state.saved.settings.enabled &&
                  state.saved.configured
                )
                  mutate(state, actions, {
                    method: 'evaluate',
                    input: {
                      state: { test: true },
                      questions: { ready: { type: 'noul', instructions: 'Is test true?' } },
                    },
                  });
                else if (input.key === 'save-key' || input.key === 'save-header') {
                  if (
                    !secret ||
                    secret.snapshot.marked ||
                    !secret.snapshot.text.bytes ||
                    secret.snapshot.text.bytes > 4096 ||
                    state.fields.secret.command
                  )
                    state.notice = 'jev-invalid';
                  else if (input.key === 'save-key')
                    mutate(
                      state,
                      actions,
                      {
                        method: 'credential',
                        input: credential(state, { kind: 'api_key' }, 'replace'),
                      },
                      secret,
                    );
                  else beginCapture(state, actions, ['headerName'], secret);
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
    JevSettings: { kind: 'component', contract: jevSettingsContract, component: ['component'] },
  };
}
