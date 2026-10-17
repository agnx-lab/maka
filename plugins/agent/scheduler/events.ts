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
  AgentObservation,
  AgentScheduleChange,
  AgentSchedules,
  AgentSessionPage,
  CallerIntent,
  ComponentInput,
  EditorObservation,
  Emission,
  OperationReply,
  TextChunk,
  ReadingObservation,
} from '../../../packages/plugin-sdk/src/index.js';
import { type SchedulerProps, type SelectedModel } from '../../shared/contracts.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';
import {
  captureNext,
  confirmChange,
  detail,
  history,
  initialize,
  inspectAction,
  list,
  moveList,
  sessions,
  locked,
  mutate,
  request,
  result,
  review,
  settle,
  startForm,
  watch,
} from './operations.js';
import { names, type Form, type State, type Timing } from './state.js';
import { frame } from './view.js';

export function handleEvent<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  input: ComponentInput,
  props: SchedulerProps,
) {
  state.revision++;
  const actions: Action[] = [],
    events: Emission[] = [];
  if (input.kind === 'component' && input.child === 'schedule-reading' && input.name === 'input')
    input = input.event as ComponentInput;
  if (
    input.kind === 'component' &&
    input.child === 'schedule-reading' &&
    input.name === 'observed'
  ) {
    const observed = input.event as ReadingObservation;
    if (
      observed.scroll &&
      observed.scroll.readingRevision > state.scrollRevision &&
      observed.viewport?.readingRevision === observed.readingRevision &&
      observed.pendingCommand == null
    ) {
      state.scrollRevision = observed.scroll.readingRevision;
      if (
        !locked(state, props) &&
        (observed.scroll.direction === 'up' ? observed.viewport.atStart : observed.viewport.atEnd)
      ) {
        if (state.picker) sessions(state, actions, observed.scroll.direction);
        else if (state.route === 'list') moveList(state, observed.scroll.direction);
      }
    }
  } else if (input.kind === 'action' && input.key === state.watch?.key) {
    const outcome = input.event as ActionOutcome;
    if (outcome.status === 'updated') {
      state.watchFailed = false;
      const page = outcome.value as AgentSchedules;
      if (page.kind === 'page') {
        state.page = page;
        state.listLoading = false;
      } else if (page.kind === 'task') {
        state.detail = page.detail ?? null;
        const next = state.nextAction;
        state.nextAction = null;
        if (!page.detail) {
          list(state);
          state.notice = 'scheduleChanged';
        } else if (next) {
          if (next.taskId !== page.detail.task.id || next.revision !== page.detail.revision)
            state.notice = 'scheduleChanged';
          else if (next.kind === 'edit') startForm(state, page.detail);
          else
            confirmChange(state, {
              kind: next.kind,
              taskId: next.taskId,
              revision: next.revision,
            });
        }
      } else if (page.kind === 'changed') {
        if (state.page) state.page.revision = page.revision;
        list(state);
        state.notice = 'scheduleChanged';
      }
    } else if (outcome.status === 'failed' || outcome.status === 'not_accepted') {
      state.watch = null;
      state.watchFailed = true;
      state.notice = 'scheduleUnavailable';
    }
  } else if (input.kind === 'action' && input.key === state.pending?.key) {
    const r = state.pending.request,
      outcome = input.event as ActionOutcome;
    state.pending = null;
    if (r.kind === 'history') {
      if (outcome.status !== 'succeeded') {
        state.historyFailed = true;
        state.notice = 'historyUnavailable';
      } else {
        const items = outcome.value as CallerIntent[];
        for (const item of items) {
          const saved = item.recovery as {
            kind?: string;
            input?: AgentScheduleChange;
          } | null;
          if (
            item.reference.method === 'schedule' &&
            item.reference.contract.name === 'maka.agent.Agent' &&
            saved?.kind === 'scheduled-task/v1' &&
            saved.input
          )
            state.current = { input: saved.input, reference: item.reference };
        }
        const last = items.at(-1);
        if (last && last.cursor > r.after) history(state, actions, last.cursor);
        else if (last) {
          state.historyFailed = true;
          state.notice = 'historyUnavailable';
        } else {
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
        }
      }
    } else if (r.kind === 'mutate') {
      if (outcome.status === 'operation') settle(state, outcome.reply, actions);
      else if (outcome.status === 'not_accepted') {
        state.current = null;
        state.notice = 'scheduleUnavailable';
      } else state.notice = 'scheduleUnknown';
    } else if (r.kind === 'receipt') {
      if (outcome.status === 'succeeded') settle(state, outcome.value as OperationReply, actions);
      else state.notice = 'scheduleUnknown';
    } else if (r.kind === 'business') {
      const page = outcome.status === 'succeeded' ? (outcome.value as AgentSchedules) : null;
      if (page?.kind === 'receipt' && page.result) result(state, page.result);
      else state.notice = 'scheduleUnknown';
    } else if (r.kind === 'field') {
      const chunk = outcome.status === 'succeeded' ? (outcome.value as TextChunk) : null;
      if (
        !chunk ||
        !state.capture ||
        !sameValue(chunk.metadata, r.observation.snapshot.text) ||
        !sameValue(state.fields[r.field].observation?.snapshot.text, r.observation.snapshot.text) ||
        new TextEncoder().encode(chunk.text).length !== r.observation.snapshot.text.bytes
      ) {
        state.capture = null;
        state.notice = 'scheduleInvalid';
      } else {
        state.capture.values[r.field] = chunk.text;
        captureNext(state, actions);
      }
    } else if (r.kind === 'sessions') {
      if (outcome.status === 'succeeded') state.sessions = outcome.value as AgentSessionPage;
      else state.notice = 'scheduleUnavailable';
    } else if (r.kind === 'session') {
      if (outcome.status === 'succeeded')
        events.push({ name: 'selected', value: (outcome.value as AgentObservation).session });
      else state.notice = 'scheduleUnavailable';
    }
  } else if (input.kind === 'component') {
    const name = names.find(
      (name) => input.kind === 'component' && input.child === 'schedule-field-' + name,
    );
    if (name && input.name === 'changed') {
      const value = state.fields[name];
      value.observation = input.event as EditorObservation;
      const receipt = value.observation.receipt;
      if (
        value.command &&
        receipt?.owner === value.command.owner &&
        receipt.sequence === value.command.sequence
      ) {
        value.ready = receipt.outcome.status === 'applied';
        value.command = null;
        value.reset = null;
        if (!value.ready) state.notice = 'scheduleUnavailable';
      } else initialize(value);
    } else if (
      name &&
      input.name === 'submitted' &&
      !locked(state, props) &&
      !state.confirm &&
      !state.picker
    )
      review(state, actions);
    else if (
      input.child === 'schedule-models' &&
      input.name === 'selected' &&
      state.form &&
      !state.current &&
      !state.confirm
    )
      state.form.model = input.event as SelectedModel;
    else if (input.child === 'schedule-models' && input.name === 'connections' && props.open)
      events.push({ name: 'connections', value: {} });
  } else if (input.kind === 'raw' && props.open) {
    let key = input.key;
    if (key === 'back' || key === 'confirm-cancel') {
      if (state.confirm) {
        if (!state.current && !state.pending) state.confirm = null;
      } else if (state.picker) state.picker = false;
      else if (state.route === 'form') {
        if (!state.current && !state.pending) {
          const id = state.form?.id;
          state.form = null;
          if (id) detail(state, id);
          else list(state);
        }
      } else if (state.route === 'detail') list(state);
      else events.push({ name: 'closed', value: {} });
    } else if (
      (key === 'receipt' || key === 'confirm-receipt') &&
      !state.pending &&
      state.current?.reference
    )
      request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.current.reference);
    else if (key === 'history-retry' && !state.pending) history(state, actions);
    else if (!locked(state, props)) {
      if (key === 'confirm' && state.confirm) mutate(state, actions, state.confirm);
      else if (!state.confirm) {
        let selected = (input.event as { key?: string }).key;
        if (key.includes(':')) {
          const pieces = key.split(':');
          key = pieces[0]!;
          selected = pieces[1];
        }
        if (state.form && !state.picker && ['effect', 'timing', 'misfire'].includes(key)) {
          if (key === 'effect' && ['reminder', 'session', 'agent_run'].includes(selected ?? ''))
            state.form.effect = selected as Form['effect'];
          if (
            key === 'timing' &&
            ['once', 'interval', 'daily', 'weekly', 'monthly', 'cron'].includes(selected ?? '')
          )
            state.form.timing = selected as Timing;
          if (key === 'misfire' && ['skip', 'latest'].includes(selected ?? ''))
            state.form.misfire = selected as Form['misfire'];
        } else if (key === 'source' || key === 'sessions-refresh') {
          sessions(state, actions);
        } else if (state.picker && key.startsWith('source-')) {
          const session = state.sessions?.sessions.find((s) => key === 'source-' + s.id);
          if (session && true && (input.event as { kind?: string }).kind === 'activate') {
            state.focused = key;
            const at = state.sessions!.sessions.indexOf(session);
            if (at >= state.sessions!.sessions.length - 3) sessions(state, actions, 'down');
            else if (at < 3) sessions(state, actions, 'up');
          } else if (session && !session.archived && state.form) {
            state.form.target = {
              sessionId: session.id,
              workspace: session.workspace,
              permissions: session.permissions,
              boundaryRevision: session.boundaryRevision,
            };
            state.form.sourceTitle = session.title;
            state.picker = false;
          }
        } else if (key === 'new') startForm(state, null);
        else if (key === 'review') review(state, actions);
        else if (key === 'refresh') {
          state.notice = null;
          if (state.route === 'detail' && state.detail)
            watch(state, { kind: 'get', taskId: state.detail.task.id });
          else list(state);
        } else if (key.startsWith('task-')) {
          const task = state.page?.tasks.find((t) => key === 'task-' + t.id);
          if (task && true && (input.event as { kind?: string }).kind === 'activate') {
            state.focused = key;
            const at = state.page!.tasks.indexOf(task);
            if (at >= state.page!.tasks.length - 3) moveList(state, 'down');
            else if (at < 3) moveList(state, 'up');
          } else if (task) detail(state, task.id);
        } else if (key === 'acknowledge' && state.detail?.pendingFire)
          mutate(state, actions, {
            kind: 'acknowledge',
            taskId: state.detail.task.id,
            revision: state.detail.revision,
            fireId: state.detail.pendingFire,
          });
        else if (key === 'snooze' && state.detail)
          mutate(state, actions, {
            kind: 'snooze',
            taskId: state.detail.task.id,
            revision: state.detail.revision,
            delayMs: 300000,
          });
        else if (key === 'clear-history' && state.detail)
          confirmChange(state, {
            kind: 'clear_history',
            taskId: state.detail.task.id,
            revision: state.detail.revision,
          });
        else if (key.startsWith('conversation-') && state.detail) {
          const run = state.detail.task.runs[Number(key.slice(13))];
          if (run?.sessionId)
            request(state, actions, { kind: 'session' }, 'agent', 'observe', {
              sessionId: run.sessionId,
            });
        } else {
          key = key.replace(/^(?:context|row|detail)-/, '');
          const kind = key.slice(0, key.indexOf('-')),
            id = key.slice(key.indexOf('-') + 1);
          const d = state.detail;
          const task =
            d?.task.id === id
              ? {
                  ...d.task,
                  revision: d.revision,
                  approved: d.approved,
                  pendingFire: d.pendingFire,
                  reminder: d.task.effect.kind === 'reminder',
                }
              : state.page?.tasks.find((task) => task.id === id);
          if (task && kind === 'open') detail(state, id);
          else if (task && kind === 'edit') {
            if (d?.task.id === id) startForm(state, d);
            else inspectAction(state, 'edit', id, task.revision);
          } else if (task && kind === 'state') {
            const change: AgentScheduleChange = {
              kind: !task.approved ? 'approve' : task.status === 'paused' ? 'resume' : 'pause',
              taskId: id,
              revision: task.revision,
            };
            if (change.kind === 'pause') mutate(state, actions, change);
            else if (d?.task.id === id) confirmChange(state, change);
            else inspectAction(state, !task.approved ? 'approve' : 'resume', id, task.revision);
          } else if (task && kind === 'run') {
            if (d?.task.id === id)
              confirmChange(state, {
                kind: 'trigger_now',
                taskId: id,
                revision: task.revision,
              });
            else inspectAction(state, 'trigger_now', id, task.revision);
          } else if (task && kind === 'delete') {
            if (d?.task.id === id)
              confirmChange(state, { kind: 'delete', taskId: id, revision: task.revision });
            else inspectAction(state, 'delete', id, task.revision);
          }
        }
      }
    }
  }
  return frame(presentation, state, props, actions, events);
}
