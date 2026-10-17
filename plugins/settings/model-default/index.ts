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
  Component,
  ComponentInput,
  Emission,
  ConnectionTarget,
  RecoveryRef,
  OperationReply,
  CallerIntent,
  CatalogMutationResult,
  SettingsModelToolPreferences,
  SettingsModelLimits,
  ModelSelection,
  EditorCommand,
  EditorObservation,
  EditorTextRef,
  TextChunk,
  InputProps,
} from '../../../packages/plugin-sdk/src/index.js';
import { modelDefaultContract, type ModelDefaultProps } from '../../shared/contracts.js';
import { scrollRows, visibleRows, resetScroll } from '../../shared/scroll.js';
import { copy, type Copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Presentation, type Node } from '../../shared/view.js';

const limitNames = [
  'contextWindow',
  'maxOutputTokens',
  'inputLimit',
  'compactionThreshold',
] as const;
type LimitName = (typeof limitNames)[number];
interface LimitField {
  observation: EditorObservation | null;
  command: EditorCommand | null;
  initial: EditorTextRef | null;
  reset: string | null;
  sequence: number;
}
const limitLabels = {
  contextWindow: 'modelContextWindow',
  inputLimit: 'modelInputLimit',
  maxOutputTokens: 'modelMaxOutput',
  compactionThreshold: 'modelCompactionThreshold',
} as const;
function initializeLimit(field: LimitField) {
  if (field.reset !== null && field.observation && !field.command)
    field.command = {
      owner: field.observation.snapshot.text.owner,
      sequence: ++field.sequence,
      basedOn: field.observation.revision,
      edit: { kind: 'reset', text: field.reset },
    };
}
function limitsChanged(state: State) {
  return limitNames.some((name) => {
    const f = state.limits[name];
    return f.initial && f.observation && !sameValue(f.initial, f.observation.snapshot.text);
  });
}
function limitsReady(state: State) {
  return limitNames.every(
    (name) => state.limits[name].initial && state.limits[name].reset === null,
  );
}
type Request =
  | { kind: 'limit'; name: LimitName; basis: EditorTextRef; attempt: number }
  | { kind: 'mutate' }
  | { kind: 'receipt'; reference: RecoveryRef }
  | { kind: 'history'; after: number }
  | { kind: 'earlier'; reference: RecoveryRef; cursor: number };
interface Intent {
  revision: number;
  target: ConnectionTarget | null;
  identity: string[];
}
interface State {
  toolsIdentity: string;
  tools: SettingsModelToolPreferences;
  limits: Record<LimitName, LimitField>;
  capture: {
    attempt: number;
    selection: ModelSelection;
    tools: SettingsModelToolPreferences;
    values: Partial<SettingsModelLimits>;
    remaining: LimitName[];
  } | null;
  revision: number;
  serial: number;
  confirmation: Intent | null;
  current: {
    kind: 'default' | 'tools';
    revision: number;
    target: ConnectionTarget | null;
    reference: RecoveryRef | null;
  } | null;
  pending: Record<string, Request>;
  notice: keyof Copy | null;
  conflictRevision: number | null;
  historyOpen: boolean;
  earlier: { cursor: number; reference: RecoveryRef; result: OperationReply | null }[];
  historyNext: number | null;
  historyFailed: boolean;
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
  const key = 'model-default-' + ++state.serial;
  state.pending[key] = r;
  actions.push({ key, binding, member: 0, method, input, resources });
}
function syncTools(state: State, props: ModelDefaultProps) {
  const identity = JSON.stringify(props.selection?.selection ?? null);
  if (state.toolsIdentity === identity) return;
  state.toolsIdentity = identity;
  for (const name of limitNames) {
    const field = state.limits[name];
    const value = props.selection?.limits?.overrides[name];
    field.reset = value == null ? '' : String(value);
    field.initial = null;
    initializeLimit(field);
  }
  state.tools = {
    codeMode: props.selection?.tools?.codeMode ?? null,
    applyPatch: props.selection?.tools?.applyPatch ?? null,
  };
}
function toolsChanged(state: State, props: ModelDefaultProps) {
  return (
    limitsChanged(state) ||
    state.tools.codeMode !== (props.selection?.tools?.codeMode ?? null) ||
    state.tools.applyPatch !== (props.selection?.tools?.applyPatch ?? null)
  );
}
function canStart(state: State, props: ModelDefaultProps) {
  return (
    !props.busy &&
    props.catalogRevision !== null &&
    props.defaultModel !== null &&
    !state.current &&
    !state.confirmation &&
    !state.historyOpen &&
    props.catalogRevision !== state.conflictRevision
  );
}
function canConfirm(state: State, props: ModelDefaultProps) {
  return (
    !props.busy &&
    !state.current &&
    !!state.confirmation &&
    props.catalogRevision === state.confirmation.revision
  );
}
function settle(state: State, reply: OperationReply, events: Emission[]) {
  const current = state.current;
  if (!current || (current.reference && !sameValue(current.reference, reply.reference))) return;
  current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'defaultUnknown';
    return;
  }
  const value = reply.receipt.outcome.value as CatalogMutationResult;
  if (
    value.kind === 'committed' &&
    (current.kind === 'tools'
      ? value.connection?.connectionId === current.target?.connectionId
      : value.connection === undefined)
  ) {
    state.notice =
      current.kind === 'tools'
        ? 'modelToolsSaved'
        : current.target
          ? 'defaultSaved'
          : 'defaultCleared';
    state.current = null;
    events.push({ name: 'changed', value: {} });
  } else if (value.kind === 'committed') state.notice = 'defaultUnknown';
  else {
    state.current = null;
    const conflict = value.kind === 'revision_conflict' || value.kind === 'connection_stale';
    state.conflictRevision = conflict ? current.revision : null;
    state.notice = conflict
      ? 'defaultConflict'
      : current.kind === 'tools'
        ? 'modelToolsRejected'
        : 'defaultRejected';
  }
}
function history(state: State, actions: Action[], after: number) {
  if (Object.values(state.pending).some((r) => r.kind === 'history' || r.kind === 'earlier'))
    return;
  if (!after) resetScroll(state, 'default-history-reading');
  state.historyFailed = false;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 2 });
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: ModelDefaultProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const saved = props.defaultModel;
  const summary =
    saved === null
      ? text.loadingModels
      : saved.kind === 'ready'
        ? compactLabel(saved.choice.label, 36)
        : saved?.kind === 'unavailable'
          ? text.defaultUnavailable
          : text.noDefault;
  const main = ui.box(
    'default-main',
    [
      ...(props.selection
        ? [
            ui.text('model-tools-name', props.selection.label, props.theme.text, 550),
            ui.text('model-tools-connection', props.selection.connectionName, props.theme.muted),
            ui.text('model-limits-help', text.modelLimitsHelp, props.theme.muted),
            ...limitNames.map((name) => {
              const field = state.limits[name];
              const known = name === 'compactionThreshold' ? null : props.selection?.limits?.[name];
              return ui.box(
                'limit-row-' + name,
                [
                  ui.text('limit-label-' + name, text[limitLabels[name]], props.theme.text),
                  presentation.slot(
                    'limit-' + name,
                    'input',
                    {
                      menu: editorMenu(props.theme, props.locale),
                      appearance: props.theme.input,
                      accessibilityLabel: text[limitLabels[name]],
                      placeholder: text.modelToolsAuto,
                      command: field.command,
                      readOnly: !canStart(state, props) || field.reset !== null,
                      disabled: false,
                      tabIndex: 0,
                    } satisfies InputProps,
                    { changed: 'limit-changed' },
                  ),
                  ...(known == null
                    ? []
                    : [
                        ui.text(
                          'limit-known-' + name,
                          `${text[name === 'maxOutputTokens' ? 'modelOutputCapacity' : 'modelCurrentLimit']}: ${known.toLocaleString('en-US')}`,
                          props.theme.muted,
                        ),
                      ]),
                ],
                { gap: 0 },
              );
            }),
            ...(['codeMode', 'applyPatch'] as const).map((field) =>
              ui.box(
                'tools-' + field,
                [
                  ui.text(
                    'tools-label-' + field,
                    field === 'codeMode' ? 'Code Mode' : 'Apply Patch',
                    props.theme.text,
                    550,
                  ),
                  ui.box(
                    'tools-options-' + field,
                    (['auto', 'on', 'off'] as const).map((option) => {
                      const value = option === 'auto' ? null : option === 'on';
                      return ui.button(
                        `tools-${field}-${option}`,
                        text[
                          option === 'auto'
                            ? 'modelToolsAuto'
                            : option === 'on'
                              ? 'modelToolsOn'
                              : 'modelToolsOff'
                        ],
                        !canStart(state, props),
                        state.tools[field] === value,
                      );
                    }),
                    { row: true, gap: 1 },
                  ),
                  ui.text(
                    'tools-help-' + field,
                    text[field === 'codeMode' ? 'modelCodeModeHelp' : 'modelApplyPatchHelp'],
                    props.theme.muted,
                  ),
                ],
                { gap: 0 },
              ),
            ),
            ui.text('model-tools-scope', text.modelToolsScope, props.theme.muted),
            ui.box(
              'tools-actions',
              [
                ui.button(
                  'tools-save',
                  text.modelToolsSave,
                  !canStart(state, props) || !limitsReady(state) || !toolsChanged(state, props),
                  true,
                ),
                ui.button(
                  'tools-reset',
                  text.cancel,
                  !canStart(state, props) || !toolsChanged(state, props),
                ),
              ],
              { row: true, gap: 1 },
            ),
          ]
        : []),
      ui.text('default-heading', text.defaultModel, props.theme.text, 550),
      ui.text('default-summary', summary, props.theme.muted),
      ui.box(
        'default-actions',
        [
          ui.button('default-set', text.makeDefault, !canStart(state, props) || !props.selection),
          ...(saved && saved.kind !== 'none'
            ? [ui.button('default-clear', text.clearDefault, !canStart(state, props))]
            : []),
          ...(state.current?.reference
            ? [
                ui.button(
                  'default-check',
                  text.check,
                  Object.values(state.pending).some(
                    (r) => r.kind === 'receipt' || r.kind === 'mutate',
                  ),
                ),
              ]
            : []),
          ui.button('default-history', text.earlierDefaultOperations),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
      ...(state.notice ? [ui.text('default-notice', text[state.notice], props.theme.muted)] : []),
    ],
    {
      visible: !state.confirmation && !state.historyOpen,
      gap: 0,
      scroll: 'manual',
      terminal: { grow: 1 },
    },
  );
  const confirmation = ui.box(
    'default-confirmation',
    [
      ...(state.confirmation
        ? [
            ui.box(
              'default-identity',
              [
                ui.text(
                  'default-question',
                  state.confirmation?.target ? text.confirmSetDefault : text.confirmClearDefault,
                ),
                ...(!canConfirm(state, props)
                  ? [ui.text('default-stale', text.defaultConflict, props.theme.muted)]
                  : []),
                ...(state.confirmation?.identity ?? []).map((value, at) =>
                  ui.text('default-line-' + at, value),
                ),
              ],
              { scroll: 'manual', autoFocus: true, gap: 1, terminal: { grow: 1, basis: 0 } },
            ),
          ]
        : []),
      ui.box(
        'default-confirm-actions',
        [
          ui.button('default-cancel', text.cancel),
          ui.button('default-confirm', text.confirmDefault, !canConfirm(state, props), true),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    {
      visible: !!state.confirmation,
      gap: 1,
      terminal: { grow: 1 },
    },
  );
  const earlier = state.historyOpen
    ? presentation.reading(
        'default-history-reading',
        {
          content: ui.box(
            'default-earlier',
            [
              ui.box(
                'default-history-actions',
                [
                  ui.button('default-history-back', text.back, false, false, {
                    background: 0,
                    align: 'start',
                  }),
                  ui.button(
                    'default-history-refresh',
                    props.theme.ascii ? 'r' : '↻',
                    false,
                    false,
                    { background: 0 },
                    undefined,
                    text.refreshDefaultOperations,
                  ),
                ],
                { row: true, gap: 1, terminal: { wrap: true } },
              ),
              ...(state.historyFailed
                ? [ui.text('default-history-failed', text.historyUnavailable, props.theme.muted)]
                : []),
              ...visibleRows(state, 'default-history-reading', state.earlier).map(
                ({ entry, at }) => {
                  const receipt = entry.result?.receipt;
                  const value =
                    receipt?.kind === 'recorded' && receipt.outcome.kind === 'returned'
                      ? (receipt.outcome.value as CatalogMutationResult)
                      : null;
                  return ui.box(
                    'default-entry-' + at,
                    [
                      ui.button(
                        'default-earlier-' + at,
                        text.check,
                        Object.values(state.pending).some(
                          (r) => r.kind === 'earlier' && r.cursor === entry.cursor,
                        ),
                      ),
                      ui.text(
                        'default-result-' + at,
                        value?.kind === 'committed'
                          ? text.defaultHistorySaved
                          : value
                            ? text.historyRejected
                            : text.historyUnknown,
                      ),
                    ],
                    { gap: 0 },
                  );
                },
              ),
            ],
            { visible: state.historyOpen, gap: 1, scroll: 'anchor', terminal: { grow: 1 } },
          ),
        },
        { input: 'history-input', observed: 'history-observed' },
      )
    : ui.box('history-hidden', [], { visible: false });

  const out = output(
    state,
    ui.box(
      'model-default',
      [
        ui.dialog(
          'default-dialog',
          state.historyOpen ? text.earlierDefaultOperations : text.defaultModel,
          main,
          ui.box('default-dialog-content', [confirmation, earlier], {
            gap: 0,
            terminal: { grow: 1 },
          }),
          !!state.confirmation || state.historyOpen,
          { width: 64, height: state.historyOpen ? 18 : 14 },
        ),
      ],
      { gap: 0, terminal: { grow: 1 } },
    ),
    events,
  );
  out.actions = actions;
  return out;
}
export function modelDefault<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ModelDefaultProps, N> {
  return {
    mount(props) {
      const state: State = {
        toolsIdentity: '',
        tools: { codeMode: null, applyPatch: null },
        limits: Object.fromEntries(
          limitNames.map((name) => [
            name,
            {
              observation: null,
              command: null,
              initial: null,
              reset: null,
              sequence: 0,
            },
          ]),
        ) as Record<LimitName, LimitField>,
        capture: null,
        revision: 0,
        serial: 0,
        confirmation: null,
        current: null,
        pending: {},
        notice: null,
        conflictRevision: null,
        historyOpen: false,
        earlier: [],
        historyNext: null,
        historyFailed: false,
      };
      syncTools(state, props);
      return frame(presentation, state, props);
    },
    update(state, props) {
      state.revision++;
      syncTools(state, props);
      return frame(presentation, state, props);
    },
    event(state, input, props) {
      if (
        input.kind === 'component' &&
        input.child === 'default-history-reading' &&
        input.name === 'input'
      )
        input = input.event as ComponentInput;
      if (
        state.historyOpen &&
        scrollRows(state, input, 'default-history-reading', state.earlier.length)
      )
        input = { kind: 'raw', key: 'default-history-more', event: {} };
      if (input.kind === 'raw' && input.key === 'default-dialog')
        input = { ...input, key: state.historyOpen ? 'default-history-back' : 'default-cancel' };
      state.revision++;
      const actions: Action[] = [],
        events: Emission[] = [];
      if (input.kind === 'component' && input.child.startsWith('limit-')) {
        const field = state.limits[input.child.slice(6) as LimitName];
        if (field && input.name === 'changed') {
          field.observation = input.event as EditorObservation;
          const receipt = field.observation.receipt;
          if (
            field.command &&
            receipt?.owner === field.command.owner &&
            receipt.sequence === field.command.sequence
          ) {
            if (
              receipt.outcome.status === 'applied' &&
              field.command.edit.kind === 'reset' &&
              field.command.edit.text === field.reset
            ) {
              field.initial = field.observation.snapshot.text;
              field.reset = null;
            } else if (receipt.outcome.status !== 'applied') {
              state.notice = 'modelLimitsInvalid';
              field.reset = null;
            }
            field.command = null;
            initializeLimit(field);
          } else initializeLimit(field);
        }
      } else if (input.kind === 'action') {
        const r = state.pending[input.key];
        if (!r) return frame(presentation, state, props);
        delete state.pending[input.key];
        const outcome = input.event as ActionOutcome;
        if (r.kind === 'limit') {
          const capture = state.capture;
          if (!capture || capture.attempt !== r.attempt) return frame(presentation, state, props);
          const chunk = outcome.status === 'succeeded' ? (outcome.value as TextChunk) : null;
          const value = chunk?.text.trim() ?? '';
          const parsed = value === '' ? null : Number(value);
          if (
            !sameValue(capture.selection, props.selection?.selection) ||
            !chunk ||
            !sameValue(chunk.metadata, r.basis) ||
            new TextEncoder().encode(chunk.text).length !== r.basis.bytes ||
            (value !== '' &&
              (!/^[0-9]+$/.test(value) || !Number.isSafeInteger(parsed) || parsed! <= 0))
          ) {
            state.capture = null;
            state.current = null;
            state.notice = 'modelLimitsInvalid';
          } else {
            capture.values[r.name] = parsed;
            capture.remaining = capture.remaining.filter((name) => name !== r.name);
            if (!capture.remaining.length) {
              state.capture = null;
              request(state, actions, { kind: 'mutate' }, 'settings', 'updateModelSettings', {
                selection: capture.selection,
                tools: capture.tools,
                limits: capture.values,
              });
            }
          }
        } else if (r.kind === 'mutate') {
          if (outcome.status === 'operation') settle(state, outcome.reply, events);
          else if (outcome.status === 'not_accepted') {
            state.notice =
              state.current?.kind === 'tools' ? 'modelToolsRejected' : 'defaultRejected';
            state.current = null;
          } else state.notice = 'defaultUnknown';
        } else if (r.kind === 'receipt') {
          if (
            outcome.status === 'succeeded' &&
            sameValue((outcome.value as OperationReply).reference, r.reference)
          )
            settle(state, outcome.value as OperationReply, events);
          else state.notice = 'defaultUnknown';
        } else if (r.kind === 'history') {
          if (outcome.status !== 'succeeded') state.historyFailed = true;
          else {
            const page = outcome.value as CallerIntent[];
            state.historyNext = page.at(-1)?.cursor ?? null;
            state.earlier = [
              ...(r.after ? state.earlier : []),
              ...page
                .filter(
                  (entry) =>
                    entry.reference.contract.name === 'maka.settings.Settings' &&
                    ['setDefault', 'updateModelSettings'].includes(entry.reference.method),
                )
                .map((entry) => ({
                  cursor: entry.cursor,
                  reference: entry.reference,
                  result: null,
                })),
            ];
          }
        } else if (
          outcome.status === 'succeeded' &&
          sameValue((outcome.value as OperationReply).reference, r.reference)
        ) {
          const entry = state.earlier.find(
            (entry) => entry.cursor === r.cursor && sameValue(entry.reference, r.reference),
          );
          if (entry) entry.result = outcome.value as OperationReply;
        }
      } else if (input.kind === 'raw') {
        const key = input.key;
        if (key === 'default-cancel') state.confirmation = null;
        else if (key === 'default-confirm' && canConfirm(state, props)) {
          const intent = state.confirmation!;
          state.confirmation = null;
          state.current = {
            kind: 'default',
            revision: intent.revision,
            target: intent.target,
            reference: null,
          };
          state.notice = null;
          request(state, actions, { kind: 'mutate' }, 'settings', 'setDefault', {
            expectedCatalogRevision: intent.revision,
            target: intent.target,
          });
        } else if (
          key === 'default-check' &&
          state.current?.reference &&
          !Object.values(state.pending).some((r) => r.kind === 'mutate' || r.kind === 'receipt')
        )
          request(
            state,
            actions,
            { kind: 'receipt', reference: state.current.reference },
            'receipts',
            'read',
            state.current.reference,
          );
        else if (key === 'default-history') {
          state.historyOpen = true;
          history(state, actions, 0);
        } else if (key === 'default-history-back') state.historyOpen = false;
        else if (key === 'default-history-refresh' && state.historyOpen) history(state, actions, 0);
        else if (key === 'default-history-more' && state.historyOpen && state.historyNext !== null)
          history(state, actions, state.historyNext);
        else if (state.historyOpen && /^default-earlier-(0|[1-9][0-9]*)$/.test(key)) {
          const entry = state.earlier[Number(key.slice(16))];
          if (
            entry &&
            !Object.values(state.pending).some(
              (r) => r.kind === 'earlier' && r.cursor === entry.cursor,
            )
          )
            request(
              state,
              actions,
              { kind: 'earlier', reference: entry.reference, cursor: entry.cursor },
              'receipts',
              'read',
              entry.reference,
            );
        } else if (canStart(state, props)) {
          const selected = props.selection,
            saved = props.defaultModel!;
          if (key.startsWith('tools-') && selected) {
            if (key === 'tools-reset') {
              state.toolsIdentity = '';
              syncTools(state, props);
            } else if (key === 'tools-save' && limitsReady(state) && toolsChanged(state, props)) {
              state.current = {
                kind: 'tools',
                revision: props.catalogRevision!,
                target: {
                  connectionId: selected.selection.connectionId,
                  modelId: selected.selection.modelId,
                },
                reference: null,
              };
              state.notice = null;
              state.capture = {
                attempt: ++state.serial,
                selection: selected.selection,
                tools: { ...state.tools },
                values: {},
                remaining: [...limitNames],
              };
              for (const name of limitNames) {
                const basis = state.limits[name].observation!.snapshot.text;
                request(
                  state,
                  actions,
                  { kind: 'limit', name, basis, attempt: state.capture.attempt },
                  'reader',
                  'read',
                  { start: 0, end: basis.bytes },
                  {
                    text: {
                      child: 'limit-' + name,
                      method: 'snapshot',
                      input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
                    },
                  },
                );
              }
            } else {
              const match = /^tools-(codeMode|applyPatch)-(auto|on|off)$/.exec(key);
              if (match)
                state.tools[match[1] as keyof SettingsModelToolPreferences] =
                  match[2] === 'auto' ? null : match[2] === 'on';
            }
          } else if (key === 'default-set' && selected) {
            state.confirmation = {
              revision: props.catalogRevision!,
              target: {
                connectionId: selected.selection.connectionId,
                modelId: selected.selection.modelId,
              },
              identity: [
                ...new Set([selected.connectionName, selected.label, selected.selection.modelId]),
              ],
            };
          } else if (key === 'default-clear' && saved.kind !== 'none') {
            state.confirmation = {
              revision: props.catalogRevision!,
              target: null,
              identity:
                saved.kind === 'ready'
                  ? [
                      ...new Set([
                        saved.choice.connectionName,
                        saved.choice.label,
                        saved.choice.selection.modelId,
                      ]),
                    ]
                  : [copy(props.locale).defaultUnavailable, saved.target.modelId],
            };
          }
        }
      }
      return frame(presentation, state, props, actions, events);
    },
  };
}
export function activate() {
  return {
    ModelDefault: { kind: 'component', contract: modelDefaultContract, component: ['component'] },
  };
}
