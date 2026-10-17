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

import type {
  Action,
  RecoveryRef,
  WorkHubGraphView,
  WorkHubGraphRead,
  WorkHubGraphChange,
  WorkHubGraphChanged,
  WorkHubGraphHistory,
  WorkHubGraphResult,
  GraphWorkDefinition,
  GraphStatus,
  GraphWorkState,
} from '../../../packages/plugin-sdk/src/index.js';
import type { State } from './state.js';
import type { Theme } from '../../shared/contracts.js';
import { copy, graphStatuses } from './locale.js';
import { type Node, type Presentation } from '../../shared/view.js';
import { intent, mutationPending } from './actions.js';

export type GraphRequest =
  | { kind: 'graphChange'; epoch: number; sessionId: string }
  | { kind: 'graphReceipt'; reference: RecoveryRef }
  | { kind: 'graphRead'; epoch: number; attempt: number; read: 'history' | 'work' | 'result' };
export interface GraphState {
  enabled: boolean;
  watch: { key: string; input: WorkHubGraphRead } | null;
  page: WorkHubGraphView | null;
  failed: boolean;
  message: string | null;
  pinned: string | null;
  after: string | null;
  back: (string | null)[];
  history: WorkHubGraphHistory | null;
  historyBefore: number | null;
  historyBack: (number | null)[];
  showHistory: boolean;
  loading: number | null;
  work: GraphWorkDefinition | null;
  result: WorkHubGraphResult | null;
  resultOffset: number;
  resultBack: number[];
}
export function newGraphState(enabled: boolean): GraphState {
  return {
    enabled,
    watch: null,
    page: null,
    failed: false,
    message: null,
    pinned: null,
    after: null,
    back: [],
    history: null,
    historyBefore: null,
    historyBack: [],
    showHistory: false,
    loading: null,
    work: null,
    result: null,
    resultOffset: 0,
    resultBack: [],
  };
}
export function watchGraph(state: State) {
  if (!state.graph.enabled || !state.selected) return;
  state.graph.watch = {
    key: `graph-${++state.serial}`,
    input: {
      sessionId: state.selected.id,
      graphId: state.graph.pinned,
      after: state.graph.after,
    },
  };
  state.graph.failed = false;
}
export function graphUpdate(state: State, page: WorkHubGraphView) {
  const g = state.graph;
  if (g.page?.epoch?.graphId !== page.epoch?.graphId) {
    g.work = null;
    g.result = null;
    g.loading = null;
    g.resultOffset = 0;
    g.resultBack = [];
  }
  g.page = page;
}
export function graphChanged(state: State, value: WorkHubGraphChanged) {
  state.graph.message =
    value.kind === 'rejected' ? value.reason : value.kind === 'conflict' ? 'graph_changed' : null;
  watchGraph(state);
}
function read(
  state: State,
  actions: Action[],
  kind: 'history' | 'work' | 'result',
  input: unknown,
) {
  const attempt = ++state.serial;
  state.graph.loading = attempt;
  intent(
    state,
    actions,
    { kind: 'graphRead', epoch: state.epoch, attempt, read: kind },
    'workhub',
    kind === 'history' ? 'graphHistory' : kind === 'work' ? 'graphWork' : 'graphResult',
    input,
  );
}
export function graphReadResult(
  state: State,
  request: Extract<GraphRequest, { kind: 'graphRead' }>,
  value: unknown,
) {
  if (request.epoch !== state.epoch || request.attempt !== state.graph.loading) return;
  state.graph.loading = null;
  if (request.read === 'history') state.graph.history = value as WorkHubGraphHistory;
  if (request.read === 'work') state.graph.work = value as GraphWorkDefinition;
  if (request.read === 'result') state.graph.result = value as WorkHubGraphResult;
}
function history(state: State, actions: Action[]) {
  if (state.selected)
    read(state, actions, 'history', {
      sessionId: state.selected.id,
      before: state.graph.historyBefore,
    });
}
function result(state: State, actions: Action[], workId: string, resultId: string, offset: number) {
  if (!state.selected || !state.graph.page?.epoch) return;
  read(state, actions, 'result', {
    sessionId: state.selected.id,
    graphId: state.graph.page.epoch.graphId,
    workId,
    resultId,
    offset,
  });
}
export function graphAction(state: State, actions: Action[], key: string): boolean {
  if (!key.startsWith('graph-') || key === 'graph-open') return false;
  const g = state.graph;
  if (!g.enabled || !state.selected) return true;
  if (key === 'graph-close') {
    if (state.dialog === 'graph') state.dialog = null;
    if (state.inspector === 'graph') state.inspector = null;
  } else if (key === 'graph-check' && state.uncertain?.reference.method === 'changeGraph') {
    intent(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.uncertain.reference);
  } else if (key === 'graph-refresh') watchGraph(state);
  else if (key === 'graph-current' || key.startsWith('graph-epoch-')) {
    const id = key === 'graph-current' ? null : key.slice('graph-epoch-'.length);
    if (id !== null && !g.history?.epochs.some((e) => e.graphId === id)) return true;
    g.pinned = id;
    g.after = null;
    g.back = [];
    g.work = null;
    g.result = null;
    g.loading = null;
    g.page = null;
    watchGraph(state);
  } else if (key === 'graph-history') {
    g.showHistory = !g.showHistory;
    if (g.showHistory) history(state, actions);
  } else if (key === 'graph-history-next' && g.history?.nextBefore != null && g.loading === null) {
    g.historyBack.push(g.historyBefore);
    g.historyBefore = g.history.nextBefore;
    history(state, actions);
  } else if (key === 'graph-history-back' && g.historyBack.length && g.loading === null) {
    g.historyBefore = g.historyBack.pop()!;
    history(state, actions);
  } else if (key === 'graph-next' && g.page?.after) {
    g.back.push(g.after);
    g.after = g.page.after;
    g.work = null;
    g.result = null;
    g.loading = null;
    watchGraph(state);
  } else if (key === 'graph-back' && g.back.length) {
    g.after = g.back.pop()!;
    g.work = null;
    g.result = null;
    g.loading = null;
    watchGraph(state);
  } else if (key.startsWith('graph-work-') && g.page?.epoch && g.loading === null) {
    const id = key.slice('graph-work-'.length);
    if (g.page.work.some((w) => w.workId === id)) {
      g.work = null;
      read(state, actions, 'work', {
        sessionId: state.selected.id,
        graphId: g.page.epoch.graphId,
        workId: id,
      });
    }
  } else if (key === 'graph-result-next' && g.result?.nextOffset != null && g.loading === null) {
    g.resultBack.push(g.resultOffset);
    g.resultOffset = g.result.nextOffset;
    result(state, actions, g.result.workId, g.result.resultId, g.resultOffset);
  } else if (key === 'graph-result-back' && g.result && g.resultBack.length && g.loading === null) {
    g.resultOffset = g.resultBack.pop()!;
    result(state, actions, g.result.workId, g.result.resultId, g.resultOffset);
  } else if (key.startsWith('graph-result-') && g.loading === null) {
    const work = g.page?.work.find((w) => w.workId === key.slice('graph-result-'.length));
    if (work?.resultId) {
      g.result = null;
      g.resultOffset = 0;
      g.resultBack = [];
      result(state, actions, work.workId, work.resultId, 0);
    }
  } else if (!g.pinned && g.page?.enabled && !state.selected.archived && !mutationPending(state)) {
    let action: WorkHubGraphChange['action'] | null = null;
    if (key === 'graph-start-graph' || key === 'graph-start-swarm')
      action = { kind: 'start', mode: key === 'graph-start-graph' ? 'graph' : 'swarm' };
    else if (key === 'graph-stop') action = { kind: 'stop' };
    else if (key === 'graph-leave') action = { kind: 'leave' };
    if (action) {
      g.message = null;
      intent(
        state,
        actions,
        { kind: 'graphChange', epoch: state.epoch, sessionId: state.selected.id },
        'workhub',
        'changeGraph',
        {
          sessionId: state.selected.id,
          expectedGraphId: g.page.currentEpoch?.graphId ?? null,
          action,
        } satisfies WorkHubGraphChange,
      );
    }
  }
  return true;
}
export function graphView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    g = state.graph,
    page = g.page;
  const open = state.inspector === 'graph' && !state.dialog;
  const editable =
    !!page?.enabled && !g.pinned && !state.selected?.archived && !mutationPending(state);
  const closed = !page?.epoch || ['completed', 'stopped'].includes(page.status);
  const idle =
    state.selected?.activity === 'idle' &&
    !state.executor &&
    state.selected.collaborationMode !== 'plan';
  const label = (status: GraphStatus | GraphWorkState) => text[graphStatuses[status]];
  return ui.box(
    'graph-panel',
    [
      ui.box(
        'graph-heading',
        [ui.text('graph-title', text.graphTitle), ui.button('graph-close', text.closeModels)],
        { row: true, gap: 1 },
      ),
      ...(g.message
        ? [
            ui.text(
              'graph-message',
              g.message === 'graph_changed'
                ? text.graphChanged
                : [
                      'graph_session_busy',
                      'graph_epoch_active',
                      'graph_previous_epoch_settling',
                    ].includes(g.message)
                  ? text.graphBusy
                  : [
                        'graph_goal_conflict',
                        'graph_plan_conflict',
                        'graph_session_unavailable',
                      ].includes(g.message)
                    ? text.graphConflict
                    : g.message === 'graph_incognito'
                      ? text.graphIncognito
                      : text.graphRejected,
              theme.warning,
            ),
          ]
        : []),
      ...(g.failed
        ? [ui.text('graph-failed', text.graphUnavailable), ui.button('graph-refresh', text.check)]
        : []),
      ...(!page
        ? [ui.text('graph-loading', text.loading)]
        : [
            ui.text('graph-mode', `${text.graphMode}: ${page.selectedMode ?? text.graphOrdinary}`),
            ui.text(
              'graph-status',
              `${page.epoch ? `${page.epoch.mode} #${page.epoch.epoch} · ` : ''}${label(page.status)}${!page.quiescent ? ` · ${text.graphSettling}` : ''}`,
            ),
            ...(g.pinned
              ? [
                  ui.text('graph-pinned', text.graphReadOnly, theme.muted),
                  ui.button('graph-current', text.graphCurrent),
                ]
              : [
                  ui.box(
                    'graph-modes',
                    [
                      ui.button(
                        'graph-start-graph',
                        page.epoch ? text.graphRestart : text.graphStart,
                        !editable || !closed || !page.quiescent || !idle,
                      ),
                      ui.button(
                        'graph-start-swarm',
                        page.epoch ? text.graphRestartSwarm : text.graphStartSwarm,
                        !editable || !closed || !page.quiescent || !idle,
                      ),
                      ui.button('graph-stop', text.graphStop, !editable || !page.epoch || closed),
                      ui.button(
                        'graph-leave',
                        text.graphLeave,
                        !editable || page.selectedMode === null,
                      ),
                    ],
                    { row: true, gap: 1, terminal: { wrap: true } },
                  ),
                  ui.text('graph-help', text.graphHelp, theme.muted),
                ]),
            ui.button('graph-history', text.graphHistory),
            ...(g.showHistory
              ? [
                  ...(g.history?.epochs ?? []).map((e) =>
                    ui.button(
                      `graph-epoch-${e.graphId}`,
                      `${text.graphEpoch} ${e.mode} #${e.epoch}`,
                      false,
                      e.graphId === g.pinned,
                    ),
                  ),
                  ui.box(
                    'graph-history-nav',
                    [
                      ui.button(
                        'graph-history-back',
                        text.planHistoryBack,
                        !g.historyBack.length || g.loading !== null,
                      ),
                      ui.button(
                        'graph-history-next',
                        text.planHistoryNext,
                        g.history?.nextBefore == null || g.loading !== null,
                      ),
                    ],
                    { row: true, gap: 1 },
                  ),
                ]
              : []),
            ...(page.work.length
              ? page.work.map((w) =>
                  ui.box(
                    `graph-item-${w.workId}`,
                    [
                      ui.text(`graph-item-title-${w.workId}`, `${w.workId} · ${label(w.status)}`),
                      ui.text(`graph-item-instruction-${w.workId}`, w.instructionExcerpt),
                      ...(w.failure
                        ? [ui.text(`graph-item-error-${w.workId}`, w.failure, theme.warning)]
                        : []),
                      ui.button(
                        `graph-work-${w.workId}`,
                        text.graphInstruction,
                        g.loading !== null,
                      ),
                      ...(w.resultId
                        ? [
                            ui.button(
                              `graph-result-${w.workId}`,
                              `${text.graphResult}${page.finish?.resultIds.includes(w.resultId) ? ` · ${text.graphSelected}` : ''}`,
                              g.loading !== null,
                            ),
                          ]
                        : []),
                    ],
                    { gap: 1 },
                  ),
                )
              : [ui.text('graph-empty', text.graphEmpty)]),
            ui.box(
              'graph-work-nav',
              [
                ui.button('graph-back', text.planHistoryBack, !g.back.length),
                ui.button('graph-next', text.planHistoryNext, !page.after),
              ],
              { row: true, gap: 1 },
            ),
            ...(page.finish
              ? [ui.text('graph-finish', `${text.graphFinal}: ${page.finish.reason}`)]
              : []),
          ]),
      ...(g.loading !== null ? [ui.text('graph-read-loading', text.loading)] : []),
      ...(g.work
        ? [
            ui.text('graph-full-instruction', g.work.instruction),
            ui.text(
              'graph-inputs',
              [
                ...g.work.inputIds,
                ...(g.work.selectedResultInputs ?? []).map(
                  (i) => `${i.sourceGraphId}: ${i.resultId}`,
                ),
              ].join('\n'),
              theme.muted,
            ),
          ]
        : []),
      ...(g.result
        ? [
            ui.text('graph-result-id', g.result.resultId, theme.muted),
            ui.text('graph-result-text', g.result.text),
            ui.box(
              'graph-result-nav',
              [
                ui.button(
                  'graph-result-back',
                  text.planHistoryBack,
                  !g.resultBack.length || g.loading !== null,
                ),
                ui.button(
                  'graph-result-next',
                  text.planHistoryNext,
                  g.result.nextOffset === null || g.loading !== null,
                ),
              ],
              { row: true, gap: 1 },
            ),
          ]
        : []),
      ...(state.uncertain?.reference.method === 'changeGraph'
        ? [ui.button('graph-check', text.check)]
        : []),
    ],
    { visible: open, gap: 1, scroll: 'manual', terminal: { grow: 1 } },
  );
}
