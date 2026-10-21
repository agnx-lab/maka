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
  AgentScheduleChange,
  AgentScheduledDetail,
  AgentScheduledEffect,
  AgentScheduledInput,
  AgentScheduleRead,
  AgentScheduleResult,
  EditorObservation,
  OperationReply,
  Schedule,
} from '../../../packages/plugin-sdk/src/index.js';
import { type SchedulerProps } from '../../shared/contracts.js';
import { sameValue } from '../../shared/value.js';
import {
  names,
  type Field,
  type FieldName,
  type Request,
  type State,
  type Timing,
} from './state.js';

export function initialize(value: Field) {
  if (value.reset !== null && value.observation && !value.command) {
    value.command = {
      owner: value.observation.snapshot.text.owner,
      basedOn: value.observation.revision,
      sequence: ++value.sequence,
      edit: { kind: 'reset', text: value.reset },
    };
  }
}

export function reset(state: State, name: FieldName, text: string) {
  const value = state.fields[name];
  value.reset = text;
  value.ready = false;
  initialize(value);
}

export function request(
  state: State,
  actions: Action[],
  r: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  if (state.pending) return;
  const key = 'schedule-' + ++state.serial;
  state.pending = { key, request: r };
  actions.push({ key, binding, member: 0, method, input, resources });
}

export function watch(state: State, input: AgentScheduleRead) {
  state.watch = { key: 'schedule-watch-' + ++state.serial, input };
  state.watchFailed = false;
}

export function list(state: State, cursor: string | null = state.listCursor) {
  state.listCursor = cursor;
  state.listLoading = true;
  state.route = 'list';
  state.detail = null;
  watch(state, {
    kind: 'list',
    cursor,
    expectedRevision: cursor ? (state.page?.revision ?? null) : null,
  });
}

export function moveList(state: State, direction: 'up' | 'down') {
  if (state.listLoading || !state.page) return;
  if (direction === 'up' && state.listPrevious.length) list(state, state.listPrevious.pop()!);
  else if (direction === 'down' && state.page.nextCursor) {
    state.listPrevious.push(state.listCursor);
    const rows = state.page.tasks;
    list(state, rows[Math.max(0, Math.floor(rows.length / 2) - 1)]?.id ?? state.page.nextCursor);
  }
}

export function sessions(
  state: State,
  actions: Action[],
  direction: 'start' | 'up' | 'down' = 'start',
) {
  if (state.pending) return;
  const page = state.sessions;
  if ((direction === 'up' && !page?.before) || (direction === 'down' && !page?.after)) return;
  const middle = page?.sessions[Math.floor(page.sessions.length / 2)];
  state.picker = true;
  request(state, actions, { kind: 'sessions' }, 'agent', 'list', {
    filter: 'all',
    position:
      direction === 'start'
        ? { kind: 'start' }
        : direction === 'down' && middle && page!.sessions.length > 1
          ? { kind: 'at', cursor: middle.id }
          : {
              kind: direction === 'up' ? 'before' : 'after',
              cursor: direction === 'up' ? (middle?.id ?? page!.before) : page!.after,
              revision: page!.revision,
            },
  });
}

export function detail(state: State, id: string) {
  state.route = 'detail';
  state.detail = null;
  watch(state, { kind: 'get', taskId: id });
}

export function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  state.historyFailed = false;
  if (!after) state.current = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}

export function locked(state: State, props: SchedulerProps) {
  return !props.open || props.busy || !!state.pending || !!state.current || state.restoring;
}

export function timing(schedule: Schedule): Timing {
  return schedule.kind === 'calendar' ? schedule.recurrence : schedule.kind;
}

export function anchor(schedule: Schedule) {
  return schedule.kind === 'once'
    ? schedule.runAt
    : schedule.kind === 'calendar'
      ? schedule.anchorAt
      : schedule.startAt;
}

export function date(at: number | undefined | null) {
  return at === null || at === undefined ? '—' : new Date(at).toISOString();
}

export function instant(value: string) {
  // Explicit offsets keep folds unambiguous and never depend on the UI process timezone.
  const m =
    /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})(?::(\d{2})(?:\.(\d{1,3}))?)?\s*(Z|[+-]\d{2}:\d{2})$/.exec(
      value,
    );
  if (!m) throw Error('date');
  const year = Number(m[1]),
    month = Number(m[2]),
    day = Number(m[3]),
    hour = Number(m[4]),
    minute = Number(m[5]),
    second = Number(m[6] ?? 0);
  const ms = Number((m[7] ?? '').padEnd(3, '0'));
  const local = new Date(Date.UTC(year, month - 1, day, hour, minute, second, ms));
  if (
    year < 1970 ||
    local.getUTCFullYear() !== year ||
    local.getUTCMonth() !== month - 1 ||
    local.getUTCDate() !== day ||
    local.getUTCHours() !== hour ||
    local.getUTCMinutes() !== minute ||
    local.getUTCSeconds() !== second
  )
    throw Error('date');
  const zone = m[8]!,
    hours = zone === 'Z' ? 0 : Number(zone.slice(1, 3)),
    minutes = zone === 'Z' ? 0 : Number(zone.slice(4));
  if (hours > 23 || minutes > 59) throw Error('offset');
  const at = local.getTime() - (zone[0] === '-' ? -1 : 1) * (hours * 60 + minutes) * 60_000;
  if (at < 0) throw Error('date');
  return at;
}

export function integer(value: string, min: number, max: number) {
  if (!/^\d+$/.test(value)) throw Error('integer');
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < min || number > max) throw Error('integer');
  return number;
}

export function startForm(state: State, existing: AgentScheduledDetail | null) {
  const task = existing?.task,
    effect = task?.effect,
    selected = effect && effect.kind !== 'reminder' ? effect : null;
  state.form = {
    id: task?.id ?? null,
    revision: existing?.revision ?? 0,
    timing: task ? timing(task.schedule) : 'once',
    effect: effect?.kind ?? 'reminder',
    target: selected?.target ?? null,
    sourceTitle: null,
    misfire: existing?.misfire ?? 'skip',
    model: selected
      ? {
          choice: {
            selection: selected.model,
            connectionName: selected.model.connectionId,
            label: selected.model.modelId,
            thinkingLevels: selected.thinkingLevel ? [selected.thinkingLevel] : [],
            defaultThinkingLevel: null,
          },
          thinkingLevel: selected.thinkingLevel ?? null,
        }
      : null,
  };
  const initial: Record<FieldName, string> = {
    title: task?.title ?? '',
    body: task?.intent.body ?? '',
    at: date(task ? anchor(task.schedule) : Date.now() + 3600_000),
    seconds: task?.schedule.kind === 'interval' ? String(task.schedule.everySeconds) : '3600',
    cron: task?.schedule.kind === 'cron' ? task.schedule.expression : '0 9 * * *',
    timezone: existing?.timezone ?? 'UTC',
    max: task?.maxFires ? String(task.maxFires) : '',
    expiry: task?.expiresAt ? date(task.expiresAt) : '',
  };
  for (const name of names) reset(state, name, initial[name]);
  state.route = 'form';
  state.picker = false;
  state.confirm = null;
  state.notice = null;
  state.watch = null;
}

export function formInput(
  state: State,
  values: Partial<Record<FieldName, string>>,
): AgentScheduledInput {
  const form = state.form!,
    title = values.title!.trim(),
    body = values.body!.trim(),
    at = instant(values.at!.trim());
  if (!title || (!body && form.effect !== 'reminder')) throw Error('text');
  const schedule: Schedule =
    form.timing === 'once'
      ? { kind: 'once', runAt: at }
      : form.timing === 'interval'
        ? {
            kind: 'interval',
            startAt: at,
            everySeconds: integer(values.seconds!.trim(), 1, 4294967295),
          }
        : form.timing === 'cron'
          ? { kind: 'cron', startAt: at, expression: values.cron!.trim() }
          : { kind: 'calendar', anchorAt: at, recurrence: form.timing };
  const effect: AgentScheduledEffect =
    form.effect === 'reminder'
      ? { kind: 'reminder' }
      : {
          kind: form.effect,
          target: form.target!,
          model: form.model!.choice.selection,
          thinkingLevel: form.model!.thinkingLevel,
        };
  return {
    task: {
      title,
      intentBody: body,
      schedule,
      effect,
      maxFires: values.max!.trim() ? integer(values.max!.trim(), 1, 4294967295) : null,
      expiresAt: values.expiry!.trim() ? instant(values.expiry!.trim()) : null,
    },
    timezone: values.timezone!.trim(),
    misfire: form.misfire,
  };
}

export function captureNext(state: State, actions: Action[]) {
  const capture = state.capture!,
    name = names.find((name) => capture.values[name] === undefined);
  if (!name) {
    state.capture = null;
    try {
      const input = formInput(state, capture.values),
        form = state.form!;
      state.confirm = form.id
        ? { kind: 'update', taskId: form.id, revision: form.revision, input }
        : { kind: 'create', input };
    } catch {
      state.notice = 'scheduleInvalid';
    }
    return;
  }
  const observation = capture.observations[name],
    basis = observation.snapshot.text;
  request(
    state,
    actions,
    { kind: 'field', field: name, observation },
    'reader',
    'read',
    { start: 0, end: basis.bytes },
    {
      text: {
        child: 'schedule-field-' + name,
        method: 'snapshot',
        input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
      },
    },
  );
}

export function review(state: State, actions: Action[]) {
  if (!state.form) return;
  if (state.form.effect !== 'reminder' && (!state.form.target || !state.form.model)) {
    state.notice = 'scheduleChooseTarget';
    return;
  }
  if (
    names.some((name) => {
      const field = state.fields[name];
      return (
        !field.ready ||
        !!field.command ||
        !field.observation ||
        field.observation.snapshot.marked ||
        field.observation.snapshot.text.bytes > (name === 'body' ? 32000 : 512)
      );
    })
  ) {
    state.notice = 'scheduleInvalid';
    return;
  }
  state.capture = {
    values: {},
    observations: Object.fromEntries(
      names.map((name) => [name, state.fields[name].observation]),
    ) as Record<FieldName, EditorObservation>,
  };
  state.notice = null;
  captureNext(state, actions);
}

export function mutate(state: State, actions: Action[], input: AgentScheduleChange) {
  state.current = { input, reference: null };
  state.notice = null;
  request(state, actions, { kind: 'mutate' }, 'agent', 'schedule', input);
  actions.at(-1)!.recovery = { kind: 'scheduled-task/v1', input };
}

export function confirmChange(state: State, change: AgentScheduleChange) {
  state.confirm = change;
  state.confirmTask = state.detail;
}

export function inspectAction(
  state: State,
  kind: NonNullable<State['nextAction']>['kind'],
  taskId: string,
  revision: number,
) {
  state.nextAction = { kind, taskId, revision };
  detail(state, taskId);
}

export function result(state: State, value: AgentScheduleResult) {
  const operation = state.current?.input;
  state.current = null;
  state.confirm = null;
  state.notice =
    value.kind === 'committed'
      ? 'scheduleSaved'
      : value.kind === 'conflict'
        ? 'scheduleChanged'
        : value.reason.includes('authorization') ||
            value.reason.includes('model') ||
            value.reason.includes('session')
          ? 'scheduleTargetChanged'
          : value.reason.includes('pending')
            ? 'schedulePending'
            : 'scheduleInvalid';
  if (value.kind === 'committed') {
    if (operation?.kind === 'delete' || state.route === 'list') list(state);
    else detail(state, value.taskId);
    state.form = null;
  } else if (state.route === 'detail' && state.detail)
    watch(state, { kind: 'get', taskId: state.detail.task.id });
}

export function settle(state: State, reply: OperationReply, actions: Action[]) {
  if (
    !state.current ||
    (state.current.reference && !sameValue(reply.reference, state.current.reference))
  )
    return;
  state.current.reference = reply.reference;
  if (reply.receipt.kind === 'recorded' && reply.receipt.outcome.kind === 'returned')
    result(state, reply.receipt.outcome.value as AgentScheduleResult);
  else {
    state.notice = 'scheduleUnknown';
    request(state, actions, { kind: 'business' }, 'agent', 'schedules', {
      kind: 'receipt',
      reference: reply.reference,
    });
  }
}
