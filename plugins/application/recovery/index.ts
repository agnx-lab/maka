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

import { scrollBoundary, scrollRows, visibleRows, resetScroll } from '../../shared/scroll.js';
import type {
  Action,
  ActionOutcome,
  CallerIntent,
  Component,
  ComponentInput,
  ReadingCommand,
  ComponentValues,
  OperationReply,
  RecoveryRef,
  SkillLibraryOutcome,
  SkillLibraryChangeResult,
} from '../../../packages/plugin-sdk/src/index.js';
import { recoveryContract, type Theme } from '../../shared/contracts.js';
import { resolveLocale } from '../../shared/locale.js';
import { copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';

type Request =
  | { kind: 'history'; after: number }
  | { kind: 'receipt'; reference: RecoveryRef; manager: boolean }
  | { kind: 'skill-outcome'; reference: RecoveryRef };
interface State {
  revision: number;
  serial: number;
  route: 'operations' | 'deployment';
  pending: { key: string; request: Request } | null;
  entries: CallerIntent[];
  next: number | null;
  selected: RecoveryRef | null;
  reply: OperationReply | null;
  published: SkillLibraryChangeResult | null;
  page: number;
  command: ReadingCommand | null;
  failed: boolean;
}
function query(state: State, actions: Action[], request: Request) {
  if (state.pending) return;
  const key = 'recovery-' + ++state.serial;
  state.pending = { key, request };
  state.failed = false;
  actions.push({
    key,
    binding:
      request.kind === 'history'
        ? 'history'
        : request.kind === 'skill-outcome'
          ? 'skillOutcomes'
          : request.manager
            ? 'managementReceipts'
            : 'receipts',
    member: 0,
    method: request.kind === 'history' ? 'page' : 'read',
    input: request.kind === 'history' ? { after: request.after, limit: 4 } : request.reference,
    resources: {},
  });
}
function receiptSource(state: State) {
  if (state.reply?.receipt.kind !== 'recorded') return '';
  return JSON.stringify(
    state.published
      ? { host: state.reply.receipt, publication: state.published }
      : state.reply.receipt,
    null,
    2,
  );
}
function boundary(text: string, page: number) {
  const at = Math.min(text.length, page * 16384);
  const unit = text.charCodeAt(at);
  return unit >= 0xdc00 && unit <= 0xdfff ? at - 1 : at;
}
export function recovery<N extends Node>(
  presentation: Presentation<N>,
  configuredLocale: string,
): Component<State, Record<string, never>, N> {
  function frame(state: State, values: ComponentValues, actions: Action[] = []) {
    const locale = resolveLocale(configuredLocale, values.locale?.[0]);
    const theme = values.appearance![0] as unknown as Theme;
    const text = copy(locale),
      ui = presentation.paint(theme);
    const busy = !!state.pending;
    const recorded = state.reply?.receipt.kind === 'recorded' ? state.reply.receipt : null;
    const source = receiptSource(state);
    const start = boundary(source, state.page),
      end = boundary(source, state.page + 1);
    const out = output(
      state,
      ui.box(
        'recovery-root',
        [
          ui.text('recovery-title', text.recoveryTitle, theme.text, 600),
          ui.text('recovery-help', text.recoveryHelp, theme.muted),
          ui.box(
            'recovery-navigation',
            [
              ui.button('operations', text.recoveryOperations, false, state.route === 'operations'),
              ui.button('deployment', text.deploymentTitle, false, state.route === 'deployment'),
            ],
            { row: true, gap: 1, terminal: { wrap: true } },
          ),
          ui.box(
            'recovery-operations',
            [
              ...(state.failed
                ? [ui.text('recovery-refresh-failed', text.recoveryRefreshFailed, theme.muted)]
                : []),
              presentation.reading(
                'recovery-reading',
                {
                  command: state.command,
                  content: ui.box(
                    'recovery-records',
                    state.selected
                      ? [
                          ui.text(
                            'recovery-original',
                            compactLabel(
                              state.selected.contract.name + ' / ' + state.selected.method,
                              120,
                            ),
                          ),
                          ui.text(
                            'recovery-observation',
                            busy
                              ? text.loading
                              : recorded
                                ? text.deploymentRecorded
                                : text.recoveryUnknown,
                          ),
                          ...(state.published
                            ? [ui.text('recovery-skill-result', text.skillsResultConfirmed)]
                            : []),
                          ...(recorded
                            ? [
                                presentation.slot(
                                  'recovery-source-' + state.page,
                                  'source',
                                  {
                                    source: source.slice(start, end),
                                    appearance: theme.markdown.code,
                                    selection: theme.markdown.selection,
                                    terminal: theme.terminal,
                                    disabled: false,
                                  },
                                  {},
                                ),
                              ]
                            : []),
                        ]
                      : [
                          ...(state.entries.length
                            ? visibleRows(state, 'recovery-reading', state.entries).map(
                                ({ entry, at: index }) =>
                                  ui.box(
                                    'recovery-entry-' + index,
                                    [
                                      ui.text(
                                        'recovery-name-' + index,
                                        compactLabel(
                                          '#' +
                                            entry.cursor +
                                            ' · ' +
                                            entry.reference.contract.name +
                                            ' / ' +
                                            entry.reference.method,
                                          120,
                                        ),
                                      ),
                                      ui.button('check-' + index, text.check, busy),
                                    ],
                                    { gap: 0 },
                                  ),
                              )
                            : [
                                ui.text(
                                  'recovery-empty',
                                  busy
                                    ? text.loading
                                    : state.failed
                                      ? text.historyUnavailable
                                      : text.recoveryEmpty,
                                  theme.muted,
                                ),
                              ]),
                        ],
                    { gap: 1, scroll: 'anchor', terminal: { grow: 1, basis: 0 } },
                  ),
                },
                { input: 'reading-input', observed: 'reading-observed' },
              ),
              ui.box(
                'recovery-history-controls',
                [ui.button('refresh', text.refreshDefaultOperations, busy)],
                { visible: !state.selected, row: true, gap: 1, terminal: { wrap: true } },
              ),
              ui.box(
                'recovery-result-controls',
                [ui.button('back', text.back, busy), ui.button('check-again', text.check, busy)],
                { visible: !!state.selected, row: true, gap: 1, terminal: { wrap: true } },
              ),
            ],
            { visible: state.route === 'operations', terminal: { grow: 1 } },
          ),
          ui.box(
            'recovery-deployment',
            [
              presentation.slot(
                'recovery-deployment-content',
                'deployment',
                { theme, locale },
                { closed: 'closed' },
              ),
            ],
            { visible: state.route === 'deployment', terminal: { grow: 1 } },
          ),
        ],
        { padding: 1, gap: 1, terminal: { grow: 1 } },
      ),
    );
    out.actions = actions;
    out.frame.surface = { kind: 'terminal', binding: 'surface' };
    return out;
  }
  return {
    mount(_props, values) {
      const state: State = {
        revision: 0,
        serial: 0,
        route: 'operations',
        pending: null,
        entries: [],
        next: null,
        selected: null,
        reply: null,
        published: null,
        page: 0,
        command: null,
        failed: false,
      };
      const actions: Action[] = [];
      query(state, actions, { kind: 'history', after: 0 });
      return frame(state, values, actions);
    },
    update(state, _props, values) {
      return frame({ ...state, revision: state.revision + 1 }, values);
    },
    event(previous, event, _props, values) {
      const state = { ...previous };
      state.revision++;
      const actions: Action[] = [];
      if (
        event.kind === 'component' &&
        event.child === 'recovery-reading' &&
        event.name === 'input'
      ) {
        const forwarded = event.event as ComponentInput;
        if (forwarded.kind !== 'raw') return frame(state, values);
        event = forwarded;
      }
      const direction = state.selected ? scrollBoundary(state, event, 'recovery-reading') : null;
      if (!state.pending) {
        if (direction)
          event = {
            kind: 'raw',
            key: direction === 'up' ? 'previous-source' : 'next-source',
            event: {},
          };
        else if (
          !state.selected &&
          scrollRows(state, event, 'recovery-reading', state.entries.length)
        )
          event = { kind: 'raw', key: 'next', event: {} };
      }
      if (event.kind === 'action' && event.key === state.pending?.key) {
        const request = state.pending.request,
          result = event.event as ActionOutcome;
        state.pending = null;
        if (request.kind === 'history') {
          state.failed = result.status !== 'succeeded';
          if (result.status === 'succeeded') {
            const entries = result.value as CallerIntent[];
            state.entries = request.after ? [...state.entries, ...entries] : entries;
            if (!request.after) resetScroll(state, 'recovery-reading');
            // The byte cap can end a page before its requested count.
            state.next = entries.length ? entries[entries.length - 1].cursor : null;
            state.selected = null;
            state.reply = null;
            state.published = null;
            state.page = 0;
          }
        } else if (state.selected && sameValue(state.selected, request.reference)) {
          if (request.kind === 'skill-outcome') {
            const outcome =
              result.status === 'succeeded' ? (result.value as SkillLibraryOutcome) : null;
            if (
              outcome &&
              sameValue(outcome.original.reference, request.reference) &&
              outcome.original.receipt.kind === 'recorded'
            ) {
              state.reply = outcome.original;
              if (outcome.published) state.published = outcome.published;
            } else state.failed = true;
            return frame(state, values, actions);
          }
          const reply = result.status === 'succeeded' ? (result.value as OperationReply) : null;
          if (
            reply &&
            sameValue(reply.reference, request.reference) &&
            reply.receipt.kind === 'recorded'
          ) {
            state.reply = reply;
            if (
              reply.reference.contract.name === 'maka.skills.Library' &&
              ['apply', 'importSource', 'deleteSource'].includes(reply.reference.method)
            )
              query(state, actions, { kind: 'skill-outcome', reference: request.reference });
          } else if (!request.manager) query(state, actions, { ...request, manager: true });
          else state.failed = true;
        }
      } else if (event.kind === 'component' && event.key === 'closed') state.route = 'operations';
      else if (event.kind === 'raw') {
        const key = event.key;
        if (key === 'operations' || key === 'deployment') state.route = key;
        else if (state.route === 'operations' && !state.pending) {
          if (key === 'refresh') query(state, actions, { kind: 'history', after: 0 });
          else if (key === 'next' && state.next !== null)
            query(state, actions, { kind: 'history', after: state.next });
          else if (key === 'back') {
            state.selected = null;
            state.reply = null;
            state.published = null;
            state.page = 0;
          } else if (key.startsWith('check-') && key !== 'check-again') {
            const entry = state.entries[Number(key.slice(6))];
            if (entry) {
              state.selected = entry.reference;
              state.reply = null;
              state.published = null;
              state.page = 0;
              query(state, actions, {
                kind: 'receipt',
                reference: entry.reference,
                manager: false,
              });
            }
          } else if (key === 'check-again' && state.selected)
            query(state, actions, { kind: 'receipt', reference: state.selected, manager: false });
          else if (key === 'previous-source') {
            state.page = Math.max(0, state.page - 1);
            state.command = {
              sequence: (state.command?.sequence ?? 0) + 1,
              target: { kind: 'end' },
            };
          } else if (key === 'next-source' && state.reply?.receipt.kind === 'recorded') {
            const source = receiptSource(state);
            if (boundary(source, state.page + 1) < source.length) {
              state.page++;
              state.command = {
                sequence: (state.command?.sequence ?? 0) + 1,
                target: { kind: 'start' },
              };
            }
          }
        }
      }
      return frame(state, values, actions);
    },
  };
}
export function activate() {
  return { Recovery: { kind: 'component', contract: recoveryContract, component: ['component'] } };
}
