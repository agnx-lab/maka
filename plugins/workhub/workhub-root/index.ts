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
  ActionOutcome,
  AgentAnswerCursor,
  AgentDirectorySelection,
  CallerIntent,
  Component,
  ComponentInput,
  ReadingCommand,
  ComponentValues,
  OperationReply,
  RecoveryRef,
  WorkHubCoordinator,
  WorkHubTask,
  WorkHubTaskEntry,
  WorkHubTasks,
} from '../../../packages/plugin-sdk/src/index.js';
import { workhubRootContract, type Theme, type AgentScreenProps } from '../../shared/contracts.js';
import { scrollBoundary } from '../../shared/scroll.js';
import {
  initialLanguageObservation,
  readLanguage,
  observeLanguage,
  type LanguageObservation,
} from '../../shared/language-preference.js';
import { copy, type Copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';

type Route = 'coordinator' | 'tasks' | 'sessions';
type Request =
  | { kind: 'history' }
  | { kind: 'receipt'; reference: RecoveryRef }
  | { kind: 'coordinator' }
  | { kind: 'resolve'; workspace: AgentDirectorySelection }
  | { kind: 'tasks'; after: string | null }
  | { kind: 'task'; entry: WorkHubTaskEntry; cursor: AgentAnswerCursor | null; detail: boolean };
interface Selection {
  entry: WorkHubTaskEntry;
  task: WorkHubTask | null;
  cursor: AgentAnswerCursor | null;
  previous: (AgentAnswerCursor | null)[];
  failed: boolean;
}
interface State {
  language: LanguageObservation;
  revision: number;
  serial: number;
  route: Route;
  pending: { key: string; request: Request } | null;
  restoring: boolean;
  historyAfter: number;
  original: RecoveryRef | null;
  cleanup: boolean;
  coordinator: WorkHubCoordinator | null;
  coordinatorSelection: AgentScreenProps['session'];
  workerSelection: AgentScreenProps['session'];
  directories: boolean;
  entries: WorkHubTasks | null;
  pageAfter: string | null;
  previousPages: (string | null)[];
  readingCommand: ReadingCommand | null;
  landing: 'start' | 'end' | null;
  pageKey: number;
  cache: Record<string, WorkHubTask | null>;
  refreshTasks: boolean;
  watchRevision: number;
  watchFailures: string[];
  selected: Selection | null;
  failed: boolean;
  retry: Exclude<Request, { kind: 'resolve' }> | null;
  rejected: boolean;
}
function ask(state: State, actions: Action[], request: Request) {
  if (state.pending) return;
  if (request.kind === 'tasks' || request.kind === 'task') {
    state.watchRevision++;
    state.watchFailures = [];
  }
  const key = `workhub-${++state.serial}`;
  state.pending = { key, request };
  state.failed = false;
  state.retry = null;
  actions.push({
    key,
    member: 0,
    resources: {},
    binding:
      request.kind === 'history' ? 'history' : request.kind === 'receipt' ? 'receipts' : 'workhub',
    method:
      request.kind === 'history' ? 'page' : request.kind === 'receipt' ? 'read' : request.kind,
    input:
      request.kind === 'history'
        ? { after: state.historyAfter, limit: 1 }
        : request.kind === 'receipt'
          ? request.reference
          : request.kind === 'resolve'
            ? { workspace: request.workspace }
            : request.kind === 'tasks'
              ? { after: request.after }
              : request.kind === 'task'
                ? { taskId: request.entry.taskId, cursor: request.cursor }
                : {},
  });
}
function watches(state: State) {
  if (state.route !== 'tasks' || state.pending || !state.entries || state.failed) return [];
  const base = `workhub-watch-${state.watchRevision}`;
  const task = (taskId: string, cursor: AgentAnswerCursor | null) => ({
    key: `${base}-task-${taskId}`,
    binding: 'workhub',
    member: 0,
    method: 'watchTask',
    input: { taskId, cursor },
    resources: {},
  });
  return [
    {
      key: `${base}-list`,
      binding: 'workhub',
      member: 0,
      method: 'watchTasks',
      input: { after: state.pageAfter },
      resources: {},
    },
    ...(state.selected
      ? [task(state.selected.entry.taskId, state.selected.cursor)]
      : state.entries.entries.map((entry) => task(entry.taskId, null))),
  ].filter((watch) => !state.watchFailures.includes(watch.key));
}
function original(state: State, reply: OperationReply, expected?: RecoveryRef) {
  if (expected && !sameValue(reply.reference, expected)) {
    state.failed = true;
    state.retry = { kind: 'receipt', reference: expected };
    return;
  }
  state.original = reply.reference;
  state.cleanup = reply.receipt.kind !== 'recorded' || reply.receipt.cleanup.kind === 'confirmed';
  if (
    reply.receipt.kind === 'recorded' &&
    reply.receipt.outcome.kind === 'returned' &&
    state.cleanup
  ) {
    state.original = null;
    // A historical result never supplies a new live coordinator selection.
    if (
      reply.reference.contract.name === 'maka.workhub.Coordinator' &&
      reply.reference.method === 'resolve'
    ) {
      state.rejected = (reply.receipt.outcome.value as WorkHubCoordinator).kind === 'rejected';
      state.coordinator = null;
    }
    state.restoring = true;
  } else state.restoring = false;
}
function progress(task: WorkHubTask | null) {
  return task && (task.status.kind === 'started' || task.status.kind === 'queued')
    ? task.status.progress
    : null;
}
function status(task: WorkHubTask | null, text: Copy): string {
  if (!task) return text.workhubUnavailable;
  const current = task.status;
  switch (current.kind) {
    case 'blocked':
      return text.workhubDependencies + ' (' + current.dependencies.length + ')';
    case 'cancelled':
      return text.workhubCancelled;
    case 'preparing': {
      if (current.state.kind === 'uncertain') return text.workhubUnknown;
      if (current.state.kind === 'unavailable') return text.workhubUnavailable;
      if (current.state.kind === 'rejected') return text.workhubRejected;
      if (current.state.kind === 'original_capture_required') return text.workhubCapture;
      return current.stage.kind === 'create'
        ? text.workhubCreating
        : current.stage.kind === 'attachment'
          ? `${text.workhubUploading} (${current.stage.index + 1}/${current.stage.total})`
          : text.workhubDelivering;
    }
    case 'pending':
      return text.workhubPending;
    case 'unavailable':
      return text.workhubUnavailable;
    case 'uncertain':
      return text.workhubUnknown;
    case 'rejected':
      return text.workhubRejected;
    case 'started':
    case 'queued': {
      const work = current.progress;
      if (!work || work.kind === 'missing') return text.workhubUnavailable;
      if (work.kind === 'changed') return text.workhubChanged;
      if (work.kind === 'cancelled') return text.workhubCancelled;
      if (work.kind === 'pending') return text.workhubQueued;
      if (work.pendingInteractions) return text.workhubWaiting;
      return {
        admitted: text.workhubPending,
        running: text.workhubRunning,
        waiting: text.workhubWaiting,
        paused: text.workhubPaused,
        completed: text.workhubCompleted,
        failed: text.workhubFailed,
        cancelled: text.workhubCancelled,
      }[work.execution.state];
    }
  }
}
function acceptanceTime(at: number, text: Copy): string {
  return `${text.workhubAccepted}: ${new Date(at).toISOString()}`;
}
function target(task: WorkHubTask | null, worker: string): string | null {
  if (!task || task.worker !== worker) return null;
  return task.status.kind === 'started' || task.status.kind === 'queued'
    ? task.status.sessionId
    : null;
}
export function workhubRoot<N extends Node>(
  presentation: Presentation<N>,
  worker: string,
  initialRoute: 'coordinator' | 'sessions' = 'coordinator',
): Component<State, Record<string, never>, N> {
  function frame(state: State, values: ComponentValues, actions: Action[] = []) {
    if (state.language.sequence === 0) readLanguage(state.language, actions);
    if (!state.pending && !state.failed) {
      if (state.restoring) ask(state, actions, { kind: 'history' });
      else if (state.coordinator === null) ask(state, actions, { kind: 'coordinator' });
      else if (state.route === 'tasks') {
        if (state.refreshTasks || !state.entries) {
          state.refreshTasks = false;
          ask(state, actions, { kind: 'tasks', after: state.pageAfter });
        }
      }
    }
    const locale = state.language.observed?.resolvedLanguage ?? 'en';
    const theme = values.appearance![0] as unknown as Theme;
    const text = copy(locale),
      ui = presentation.paint(theme);
    const busy = !!state.pending;
    const gap = 1;
    const selected = state.selected,
      work = progress(selected?.task ?? null);
    const answer = work?.kind === 'delivered' ? work.answer : null;
    const content: N[] = selected
      ? [
          ui.text(
            'task-title',
            compactLabel((selected.task ?? selected.entry).title, 128) || text.workhubUntitled,
            theme.text,
            600,
          ),
          ui.text('task-worker', compactLabel(selected.entry.worker, 64), theme.muted),
          ui.text(
            'task-accepted',
            acceptanceTime((selected.task ?? selected.entry).acceptedAt, text),
            theme.muted,
          ),
          ui.text(
            'task-status',
            selected.task
              ? status(selected.task, text)
              : selected.failed
                ? text.workhubUnavailable
                : text.loading,
          ),
          ...(selected.failed
            ? [ui.text('task-refresh-failed', text.recoveryRefreshFailed, theme.warning)]
            : []),
          ...(work?.kind === 'delivered' && work.shared
            ? [ui.text('task-shared', text.workhubShared, theme.muted)]
            : []),
          ui.text('answer-title', text.workhubAnswer, theme.text, 600),
          ...(answer
            ? [
                presentation.slot(
                  `answer-${selected.entry.taskId.slice(3, 19)}-${work?.kind === 'delivered' ? work.execution.invocationId : ''}-${selected.cursor?.offset ?? 0}`,
                  'source',
                  {
                    source: answer.text,
                    appearance: theme.markdown.code,
                    selection: theme.markdown.selection,
                    terminal: theme.terminal,
                    disabled: false,
                    focusOnPointer: true,
                  },
                  {},
                ),
              ]
            : [ui.text('answer-empty', text.workhubNoAnswer, theme.muted)]),
        ]
      : (state.entries?.entries.map((entry, at) =>
          ui.box(
            `task-row-${state.pageKey}-${at}`,
            [
              ui.button(
                `task-open-${state.pageKey}-${at}`,
                compactLabel(entry.title, 100) || text.workhubUntitled,
                state.pending?.request.kind === 'tasks',
                false,
                { align: 'start' },
              ),
              ui.text(
                `task-accepted-${state.pageKey}-${at}`,
                acceptanceTime(entry.acceptedAt, text),
                theme.muted,
              ),
              ui.text(
                `task-summary-${state.pageKey}-${at}`,
                `${compactLabel(entry.worker, 64)} · ${Object.hasOwn(state.cache, entry.taskId) ? status(state.cache[entry.taskId], text) : text.loading}`,
                theme.muted,
              ),
            ],
            { gap: 0 },
          ),
        ) ?? []);
    if (!selected && !busy && state.entries?.entries.length === 0)
      content.push(ui.text('tasks-empty', text.workhubEmpty, theme.muted));
    const page = (route: Route, children: N[]) =>
      ui.box(`workhub-${route}`, children, {
        visible: state.route === route,
        gap: 0,
        terminal: { grow: 1 },
      });
    const out = output(
      state,
      ui.box(
        'workhub-root',
        [
          ui.box('authoring-background', [presentation.slot('authoring', 'authoring', {}, {})], {
            visible: false,
          }),
          ui.box(
            'workhub-navigation',
            [
              ui.text('workhub-title', 'WorkHub', theme.text, 600),
              ...(['coordinator', 'tasks', 'sessions'] as Route[]).map((route, at) =>
                ui.button(
                  `workhub-route-${route}`,
                  [text.workhubChat, text.workhubTasks, text.workhubSessions][at],
                  false,
                  state.route === route,
                ),
              ),
            ],
            { visible: state.route !== 'sessions', row: true, gap, terminal: { wrap: true } },
          ),
          ...(state.original
            ? [
                ui.box(
                  'workhub-original',
                  [
                    ui.text(
                      'workhub-original-state',
                      state.cleanup ? text.workhubOriginal : text.workhubCleanup,
                      theme.warning,
                    ),
                    ui.button('workhub-check-original', text.check, busy),
                  ],
                  { row: true, gap, terminal: { wrap: true } },
                ),
              ]
            : []),
          ...(state.failed
            ? [
                ui.box(
                  'workhub-failed',
                  [
                    ui.text('workhub-failed-text', text.recoveryRefreshFailed, theme.warning),
                    ui.button('workhub-retry-read', text.check, busy),
                  ],
                  { row: true, gap },
                ),
              ]
            : []),
          page('coordinator', [
            ...(state.coordinatorSelection
              ? [
                  presentation.slot(
                    'coordinator-agent-screen',
                    'coordinator',
                    {
                      embedded: true,
                      ...(state.language.observed
                        ? { languageRevision: state.language.observed.revision }
                        : {}),
                      session: state.coordinatorSelection,
                    } satisfies AgentScreenProps,
                    { languageChanged: 'language-changed' },
                  ),
                ]
              : [
                  ui.box(
                    'workhub-setup',
                    [
                      ui.text(
                        'workhub-setup-message',
                        busy || state.restoring
                          ? text.loading
                          : state.coordinator?.kind === 'pending'
                            ? text.workhubSetupPending
                            : text.workhubSetup,
                      ),
                      ...(state.rejected
                        ? [ui.text('workhub-rejected', text.workhubRejected, theme.warning)]
                        : []),
                      ui.button(
                        'workhub-setup-start',
                        state.coordinator?.kind === 'pending'
                          ? text.workhubContinue
                          : text.workhubStart,
                        busy || state.restoring || !!state.original || !state.coordinator,
                        true,
                      ),
                      ui.button('workhub-coordinator-refresh', text.check, busy),
                    ],
                    { gap, padding: 1 },
                  ),
                ]),
          ]),
          page('sessions', [
            presentation.slot(
              'worker-agent-screen',
              'worker',
              {
                embedded: true,
                ...(state.language.observed
                  ? { languageRevision: state.language.observed.revision }
                  : {}),
                workhub: true,
                ...(state.workerSelection ? { session: state.workerSelection } : {}),
              } satisfies AgentScreenProps,
              { workhub: 'workhub', languageChanged: 'language-changed' },
            ),
          ]),
          page('tasks', [
            presentation.reading(
              'workhub-reading',
              {
                command: state.readingCommand,
                content: ui.box('workhub-tasks-content', content, {
                  gap,
                  scroll: 'manual',
                  terminal: { grow: 1, basis: 0 },
                }),
              },
              { input: 'reading-input', observed: 'reading-observed' },
            ),
            ui.box(
              'workhub-task-controls',
              selected
                ? [
                    ui.button('task-back', text.back, busy),
                    ui.button('task-check', text.check, busy),
                    ui.button(
                      'task-session',
                      text.workhubOpen,
                      busy || !target(selected.task, worker),
                    ),
                    ui.button('answer-first', text.workhubAnswerFirst, busy || !selected.cursor),
                  ]
                : [
                    ui.button('tasks-refresh', text.check, busy),
                    ui.button('tasks-first', text.workhubFirst, busy || state.pageAfter === null),
                  ],
              { row: true, gap, terminal: { wrap: true } },
            ),
          ]),
        ],
        { gap: 0, background: theme.background, terminal: { grow: 1 } },
      ),
    );
    out.frame.view = ui.dialog(
      'workhub-directory-dialog',
      text.directoryTitle,
      out.frame.view,
      presentation.slot(
        'workhub-directory-picker',
        'directories',
        { theme, locale, open: state.directories, busy },
        { selected: 'selected', closed: 'closed' },
      ),
      state.directories,
    );
    out.frame.surface = { kind: 'terminal', binding: 'surface' };
    out.actions = actions;
    out.subscriptions = watches(state);
    return out;
  }
  return {
    mount(_props, values) {
      return frame(
        {
          language: initialLanguageObservation(),
          revision: 0,
          serial: 0,
          route: initialRoute,
          pending: null,
          restoring: true,
          historyAfter: 0,
          original: null,
          cleanup: true,
          coordinator: null,
          coordinatorSelection: undefined,
          workerSelection: undefined,
          directories: false,
          entries: null,
          pageAfter: null,
          previousPages: [],
          readingCommand: null,
          landing: null,
          pageKey: 0,
          cache: {},
          refreshTasks: false,
          watchRevision: 0,
          watchFailures: [],
          selected: null,
          failed: false,
          retry: null,
          rejected: false,
        },
        values,
      );
    },
    update(state, _props, values) {
      return frame({ ...state, revision: state.revision + 1 }, values);
    },
    event(state, event, _props, values) {
      state.revision++;
      const actions: Action[] = [];
      if (observeLanguage(state.language, event, actions)) return frame(state, values, actions);
      if (
        event.kind === 'component' &&
        ['coordinator-agent-screen', 'worker-agent-screen'].includes(event.child) &&
        event.name === 'languageChanged'
      )
        readLanguage(state.language, actions);
      if (
        event.kind === 'raw' &&
        (event.key.startsWith('workhub-route-') ||
          [
            'workhub-coordinator-refresh',
            'workhub-retry-read',
            'workhub-check-original',
            'tasks-refresh',
            'task-check',
          ].includes(event.key))
      )
        readLanguage(state.language, actions);
      if (event.kind === 'component' && event.child === 'workhub-reading' && event.name === 'input')
        event = event.event as ComponentInput;
      const direction = scrollBoundary(state, event, 'workhub-reading');
      if (direction && !state.pending && state.route === 'tasks')
        event = {
          kind: 'raw',
          key: state.selected
            ? direction === 'up'
              ? 'answer-previous'
              : 'answer-next'
            : direction === 'up'
              ? 'tasks-previous'
              : 'tasks-next',
          event: {},
        };
      const watch =
        event.kind === 'action'
          ? watches(state).find((watch) => watch.key === event.key)
          : undefined;
      if (watch && event.kind === 'action') {
        const outcome = event.event as ActionOutcome;
        if (outcome.status === 'updated') {
          if (watch.method === 'watchTasks') {
            const entries = outcome.value as WorkHubTasks;
            if (!sameValue(entries, state.entries)) {
              state.entries = entries;
              state.pageKey++;
              state.cache = Object.fromEntries(
                entries.entries
                  .filter((entry) => Object.hasOwn(state.cache, entry.taskId))
                  .map((entry) => [entry.taskId, state.cache[entry.taskId]]),
              );
            }
          } else {
            const task = outcome.value as WorkHubTask;
            const entry =
              state.selected?.entry.taskId === task.taskId
                ? state.selected.entry
                : state.entries?.entries.find((entry) => entry.taskId === task.taskId);
            if (
              entry?.worker === task.worker &&
              'taskId' in watch.input &&
              task.taskId === watch.input.taskId
            ) {
              if (state.selected?.entry.taskId === task.taskId) {
                state.selected.task = task;
                state.selected.failed = false;
              } else state.cache[task.taskId] = task;
            }
          }
        } else if (
          outcome.status === 'failed' ||
          outcome.status === 'not_accepted' ||
          outcome.status === 'succeeded'
        ) {
          state.watchFailures.push(watch.key);
          if (watch.method === 'watchTasks') state.failed = true;
          else if ('taskId' in watch.input) {
            if (state.selected?.entry.taskId === watch.input.taskId) state.selected.failed = true;
            else state.cache[watch.input.taskId] = null;
          }
        }
      } else if (event.kind === 'action' && event.key === state.pending?.key) {
        const request = state.pending.request,
          result = event.event as ActionOutcome;
        state.pending = null;
        if (request.kind === 'resolve') {
          if (
            result.status === 'operation' &&
            result.reply.reference.contract.name === 'maka.workhub.Coordinator' &&
            result.reply.reference.method === 'resolve'
          ) {
            original(state, result.reply);
            state.coordinator = null;
          } else {
            state.restoring = true;
            state.failed = true;
            state.retry = { kind: 'history' };
          }
        } else if (result.status !== 'succeeded') {
          if (request.kind === 'task') {
            if (request.detail && state.selected?.entry.taskId === request.entry.taskId)
              state.selected.failed = true;
            else state.cache[request.entry.taskId] = null;
          } else {
            state.failed = true;
            state.retry = request;
          }
        } else if (request.kind === 'history') {
          const entries = result.value as CallerIntent[];
          if (!entries.length) state.restoring = false;
          else {
            state.historyAfter = entries[0].cursor;
            state.original = entries[0].reference;
            ask(state, actions, { kind: 'receipt', reference: entries[0].reference });
          }
        } else if (request.kind === 'receipt')
          original(state, result.value as OperationReply, request.reference);
        else if (request.kind === 'coordinator') {
          state.coordinator = result.value as WorkHubCoordinator;
          if (
            state.coordinator.kind === 'ready' &&
            state.coordinatorSelection?.id !== state.coordinator.session.id
          )
            state.coordinatorSelection = {
              request: ++state.serial,
              id: state.coordinator.session.id,
            };
        } else if (request.kind === 'tasks') {
          state.entries = result.value as WorkHubTasks;
          state.pageAfter = request.after;
          state.pageKey++;
          state.cache = {};
        } else {
          const task = result.value as WorkHubTask;
          if (task.taskId !== request.entry.taskId || task.worker !== request.entry.worker) {
            if (request.detail && state.selected) state.selected.failed = true;
            else state.cache[request.entry.taskId] = null;
          } else if (
            request.detail &&
            state.selected?.entry.taskId === request.entry.taskId &&
            sameValue(state.selected.cursor, request.cursor)
          )
            state.selected.task = task;
          else if (!request.detail) state.cache[request.entry.taskId] = task;
        }
      } else if (
        event.kind === 'component' &&
        event.child === 'workhub-directory-picker' &&
        state.directories
      ) {
        if (event.name === 'closed') state.directories = false;
        else if (
          event.name === 'selected' &&
          !state.pending &&
          !state.original &&
          !state.restoring
        ) {
          state.directories = false;
          ask(state, actions, {
            kind: 'resolve',
            workspace: event.event as AgentDirectorySelection,
          });
        }
      } else if (
        event.kind === 'component' &&
        event.child === 'worker-agent-screen' &&
        event.name === 'workhub'
      ) {
        state.route = 'coordinator';
        state.watchRevision++;
        state.watchFailures = [];
      } else if (event.kind === 'raw') {
        const key = event.key;
        if (key.startsWith('workhub-route-')) {
          const route = key.slice('workhub-route-'.length) as Route;
          if (['coordinator', 'tasks', 'sessions'].includes(route)) {
            if (route !== state.route) {
              state.watchRevision++;
              state.watchFailures = [];
            }
            if (route === 'tasks' && state.route !== route && !state.selected)
              state.refreshTasks = true;
            state.route = route;
          }
        } else if (
          state.route === 'tasks' &&
          !state.selected &&
          state.pending?.request.kind !== 'tasks' &&
          state.entries?.entries.some((_entry, at) => key === `task-open-${state.pageKey}-${at}`)
        ) {
          const at = state.entries.entries.findIndex(
            (_entry, at) => key === `task-open-${state.pageKey}-${at}`,
          );
          state.selected = {
            entry: state.entries.entries[at],
            task: null,
            cursor: null,
            previous: [],
            failed: false,
          };
          state.watchRevision++;
          state.watchFailures = [];
        } else if (!state.pending) {
          if (key === 'workhub-check-original' && state.original)
            ask(state, actions, { kind: 'receipt', reference: state.original });
          else if (key === 'workhub-retry-read' && state.retry) ask(state, actions, state.retry);
          else if (key === 'workhub-coordinator-refresh')
            ask(state, actions, { kind: 'coordinator' });
          else if (key === 'workhub-setup-start' && !state.original && !state.restoring) {
            if (state.coordinator?.kind === 'pending')
              ask(state, actions, { kind: 'resolve', workspace: state.coordinator.workspace });
            else if (
              state.coordinator?.kind === 'missing' ||
              state.coordinator?.kind === 'rejected'
            )
              state.directories = true;
          } else if (state.route === 'tasks') {
            if (key === 'task-back') {
              state.selected = null;
              state.watchRevision++;
              state.watchFailures = [];
            } else if (
              ['tasks-refresh', 'tasks-first', 'tasks-next', 'tasks-previous'].includes(key)
            ) {
              if (key === 'tasks-previous' && !state.previousPages.length)
                return frame(state, values, actions);
              if (key !== 'tasks-next' || state.entries?.after) {
                if (key === 'tasks-next') state.previousPages.push(state.pageAfter);
                if (key === 'tasks-first') state.previousPages = [];
                state.landing = key === 'tasks-previous' ? 'end' : 'start';
                ask(state, actions, {
                  kind: 'tasks',
                  after:
                    key === 'tasks-previous'
                      ? state.previousPages.pop()!
                      : key === 'tasks-first'
                        ? null
                        : key === 'tasks-next'
                          ? state.entries!.after!
                          : state.pageAfter,
                });
              }
            } else if (state.selected) {
              const selection = state.selected;
              if (key === 'task-check') {
                selection.failed = false;
                ask(state, actions, {
                  kind: 'task',
                  entry: selection.entry,
                  cursor: selection.cursor,
                  detail: true,
                });
              } else if (key === 'task-session') {
                const id = target(selection.task, worker);
                if (id) {
                  state.workerSelection = { request: ++state.serial, id };
                  state.route = 'sessions';
                }
              } else if (['answer-first', 'answer-next', 'answer-previous'].includes(key)) {
                const work = progress(selection.task),
                  next = work?.kind === 'delivered' ? work.answer?.next : null;
                if (
                  key === 'answer-first' ||
                  (key === 'answer-next' && next) ||
                  (key === 'answer-previous' && selection.previous.length)
                ) {
                  if (key === 'answer-next') selection.previous.push(selection.cursor);
                  if (key === 'answer-first') selection.previous = [];
                  state.landing = key === 'answer-previous' ? 'end' : 'start';
                  selection.cursor =
                    key === 'answer-previous'
                      ? selection.previous.pop()!
                      : key === 'answer-first'
                        ? null
                        : next!;
                  selection.task = null;
                  selection.failed = false;
                  state.watchRevision++;
                  state.watchFailures = [];
                }
              }
            }
          }
        }
      }
      if (state.landing && !state.pending && (!state.selected || state.selected.task)) {
        state.readingCommand = {
          sequence: (state.readingCommand?.sequence ?? 0) + 1,
          target: { kind: state.landing },
        };
        state.landing = null;
      }
      return frame(state, values, actions);
    },
  };
}
export function activate() {
  return {
    WorkHub: { kind: 'component', contract: workhubRootContract, component: ['component'] },
  };
}
