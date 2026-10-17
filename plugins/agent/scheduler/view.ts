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
  AgentScheduledDetail,
  AgentScheduledEffect,
  AgentScheduledSummary,
  Emission,
  InputProps,
  Schedule,
  TextareaProps,
} from '../../../packages/plugin-sdk/src/index.js';
import { type SchedulerProps } from '../../shared/contracts.js';
import { copy, type Copy } from './locale.js';
import { pageHeader } from '../../shared/page-shell.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';
import { anchor, date, detail, locked, timing } from './operations.js';
import { names, type FieldName, type State, type Timing } from './state.js';

export function status(task: AgentScheduledSummary | AgentScheduledDetail, text: Copy) {
  if (!task.approved) return text.scheduleApproval;
  if (task.pendingFire) return text.schedulePending;
  const value = 'status' in task ? task.status : task.task.status;
  return text[
    (
      {
        active: 'scheduleActive',
        paused: 'schedulePaused',
        completed: 'scheduleCompleted',
        expired: 'scheduleExpired',
      } as const
    )[value]
  ];
}

export function timingLabel(kind: Timing, text: Copy) {
  return kind === 'cron'
    ? 'Cron'
    : text[
        (
          {
            once: 'scheduleOnce',
            interval: 'scheduleInterval',
            daily: 'scheduleDaily',
            weekly: 'scheduleWeekly',
            monthly: 'scheduleMonthly',
          } as const
        )[kind]
      ];
}

export function effectLabel(kind: AgentScheduledEffect['kind'], text: Copy) {
  return text[
    kind === 'reminder'
      ? 'scheduleReminder'
      : kind === 'session'
        ? 'scheduleSession'
        : 'scheduleAgentRun'
  ];
}

export function scheduleSummary(schedule: Schedule, text: Copy) {
  return (
    timingLabel(timing(schedule), text) +
    (schedule.kind === 'interval'
      ? ` · ${schedule.everySeconds}s`
      : schedule.kind === 'cron'
        ? ` · ${schedule.expression}`
        : '') +
    ' · ' +
    date(anchor(schedule))
  );
}

export function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: SchedulerProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  if (
    props.open &&
    props.requested &&
    props.requested.sequence > state.requested &&
    !state.restoring &&
    !state.current &&
    !state.pending &&
    !state.confirm
  ) {
    state.requested = props.requested.sequence;
    state.form = null;
    state.picker = false;
    detail(state, props.requested.taskId);
  }
  const ui = presentation.paint(props.theme),
    text = copy(props.locale),
    disabled = locked(state, props),
    editing = disabled || !!state.confirm || state.picker;
  const choose = (
    key: string,
    label: string,
    options: { key: string; label: string }[],
    current: string,
  ) => ui.choice(key, label, options, current, editing);
  const input = (name: FieldName, label: string, visible = true) => {
    const value = state.fields[name],
      id = 'schedule-field-' + name;
    const common = {
      accessibilityLabel: label,
      tabIndex: names.indexOf(name),
      autoFocus: name === 'title',
      readOnly: editing,
      disabled: false,
      placeholder: '',
      command: value.command,
    } satisfies Omit<InputProps, 'appearance'>;
    const field =
      name === 'body'
        ? presentation.slot(
            id,
            'editor',
            {
              ...common,
              appearance: { ...props.theme.textarea, borderWidth: 0 },
              softWrap: true,
              terminalRows: { min: 3, max: 6 },
              submit: 'primary_enter',
              menu: {
                appearance: props.theme.menu,
                labels: {
                  title: text.edit,
                  undo: text.undo,
                  redo: text.redo,
                  cut: text.cut,
                  copy: text.copy,
                  paste: text.paste,
                  selectAll: text.selectAll,
                },
              },
            } satisfies TextareaProps,
            { changed: 'field', submitted: 'review' },
          )
        : presentation.slot(
            id,
            'input',
            {
              menu: editorMenu(props.theme, props.locale),
              ...common,
              appearance: { ...props.theme.input, borderWidth: 0 },
            } satisfies InputProps,
            { changed: 'field', submitted: 'review' },
          );
    return ui.box(
      'schedule-input-' + name,
      [
        ui.box(
          'schedule-label-' + name,
          [ui.text('schedule-label-text-' + name, label, props.theme.muted)],
          { gap: 0 },
        ),
        ui.box('schedule-editor-' + name, [field], { gap: 0 }),
      ],
      {
        visible,
        gap: 0,
        wide: {
          min_width: 56,
          direction: 'row',
          children: {
            ['schedule-label-' + name]: { basis: 24 },
            ['schedule-editor-' + name]: { grow: 1, basis: 0 },
          },
        },
      },
    );
  };
  const form = state.form;
  const formView = ui.box(
    'schedule-form',
    [
      input('title', text.scheduleName),
      input('body', text.scheduleBody),
      choose(
        'effect',
        text.scheduleEffect,
        (['reminder', 'session', 'agent_run'] as const).map((key) => ({
          key,
          label: effectLabel(key, text),
        })),
        form?.effect ?? 'reminder',
      ),
      ui.box(
        'schedule-target',
        [
          ui.button('source', form?.sourceTitle ?? text.scheduleSource, editing),
          ...(form?.target
            ? [
                ui.text(
                  'schedule-workspace',
                  compactLabel(form.target.workspace.path, 400),
                  props.theme.muted,
                ),
              ]
            : []),
          ui.text('schedule-source-hint', text.scheduleSourceHint, props.theme.muted),
          presentation.slot(
            'schedule-models',
            'models',
            {
              theme: props.theme,
              locale: props.locale,
              selection: form?.model ?? null,
              catalogEpoch: props.catalogEpoch,
              openRequest: null,
              busy: editing || !form || form.effect === 'reminder',
            },
            { selected: 'model', connections: 'connections' },
          ),
        ],
        { visible: !!form && form.effect !== 'reminder', gap: 1 },
      ),
      choose(
        'timing',
        text.scheduleTiming,
        (['once', 'interval', 'daily', 'weekly', 'monthly', 'cron'] as const).map((key) => ({
          key,
          label: timingLabel(key, text),
        })),
        form?.timing ?? 'once',
      ),
      input('at', text.scheduleAt),
      ui.text('schedule-time-hint', text.scheduleTimeHint, props.theme.muted),
      input('seconds', text.scheduleSeconds, form?.timing === 'interval'),
      input('cron', text.scheduleCron, form?.timing === 'cron'),
      input('timezone', text.scheduleZone),
      choose(
        'misfire',
        text.scheduleMissed,
        [
          { key: 'skip', label: text.scheduleSkip },
          { key: 'latest', label: text.scheduleLatest },
        ],
        form?.misfire ?? 'skip',
      ),
      input('max', text.scheduleMax),
      input('expiry', text.scheduleExpiry),
    ],
    {
      visible: state.route === 'form' && !state.picker,
      gap: 1,
      scroll: 'manual',
      terminal: { grow: 1, basis: 0 },
    },
  );
  const controls = (task: AgentScheduledSummary, prefix: string) => [
    ui.button(prefix + 'open-' + task.id, text.scheduleHistory, disabled),
    ...(task.status === 'active' || task.status === 'paused'
      ? [
          ui.button(prefix + 'edit-' + task.id, text.edit, disabled || !!task.pendingFire),
          ui.button(
            prefix + 'state-' + task.id,
            !task.approved
              ? text.scheduleEnable
              : task.status === 'paused'
                ? text.scheduleResume
                : text.schedulePause,
            disabled,
          ),
          ...(task.status === 'active'
            ? [
                ui.button(
                  prefix + 'run-' + task.id,
                  text.scheduleRun,
                  disabled || !!task.pendingFire || !task.approved,
                ),
              ]
            : []),
        ]
      : []),
    ui.button(
      prefix + 'delete-' + task.id,
      text.scheduleDelete,
      disabled || (!!task.pendingFire && !task.reminder),
    ),
  ];
  const content: N[] = [];
  if (state.picker) {
    for (const session of state.sessions?.sessions ?? [])
      content.push(
        ui.item(
          'source-' + session.id,
          ui.text('source-label-' + session.id, compactLabel(session.title, 120)),
          state.focused === 'source-' + session.id,
          !props.open || props.busy || session.archived,
          { followFocus: true, submit: true },
        ),
      );
  } else if (state.route === 'list') {
    content.push(
      ui.item(
        'new',
        ui.box(
          'schedule-new-row',
          [
            ui.box(
              'schedule-new-label',
              [ui.text('schedule-new-text', text.scheduleNew, props.theme.text)],
              { terminal: { grow: 1 } },
            ),
            ui.text('schedule-new-arrow', '›', props.theme.muted),
          ],
          { row: true, gap: 1 },
        ),
        false,
        disabled,
      ),
    );
    for (const task of state.page?.tasks ?? []) {
      const row = ui.box(
        'schedule-row-' + task.id,
        [
          ui.text('schedule-title-' + task.id, compactLabel(task.title, 120)),
          ui.text(
            'schedule-summary-' + task.id,
            `${status(task, text)} · ${text.scheduleNext}: ${date(task.nextFireAt)}`,
            task.pendingFire ? props.theme.warning : props.theme.muted,
          ),
        ],
        { gap: 0 },
      );
      const item = ui.item('task-' + task.id, row, state.focused === 'task-' + task.id, disabled, {
        followFocus: true,
        submit: true,
      });
      content.push(
        ui.contextMenu(
          'schedule-menu-' + task.id,
          compactLabel(task.title, 120),
          item,
          controls(task, 'context-'),
        ),
      );
    }
    if (state.page && !state.page.tasks.length)
      content.push(ui.text('schedule-empty', text.scheduleEmpty, props.theme.muted));
  } else if (state.detail) {
    const d = state.detail,
      task = d.task,
      effect = task.effect;
    content.push(
      ui.text('schedule-detail-title', task.title, props.theme.text, 600),
      ui.text(
        'schedule-status',
        `${status(d, text)} · ${text.scheduleCount}: ${task.fireCount}${task.maxFires ? '/' + task.maxFires : ''}`,
        props.theme.muted,
      ),
      ui.text('schedule-next', `${text.scheduleNext}: ${date(task.nextFireAt)}`),
      ui.text(
        'schedule-timing',
        `${scheduleSummary(task.schedule, text)} · ${d.timezone}`,
        props.theme.muted,
      ),
      ui.text('schedule-detail-effect', effectLabel(effect.kind, text)),
      ...(effect.kind !== 'reminder'
        ? [
            ui.text('schedule-detail-path', compactLabel(effect.target.workspace.path, 400)),
            ui.text('schedule-detail-model', effect.model.modelId),
          ]
        : []),
      ui.text('schedule-instructions', task.intent.body),
    );
    if (d.pendingFire && effect.kind === 'reminder')
      content.push(
        ui.box(
          'schedule-reminder-actions',
          [
            ui.button('acknowledge', text.scheduleDone, disabled, true),
            ui.button('snooze', text.scheduleSnooze, disabled),
          ],
          { row: true, gap: 1, terminal: { wrap: true } },
        ),
      );
    const summary: AgentScheduledSummary = {
      ...task,
      revision: d.revision,
      approved: d.approved,
      pendingFire: d.pendingFire,
      reminder: effect.kind === 'reminder',
    };
    content.push(
      ui.box(
        'schedule-detail-actions',
        controls(summary, 'detail-').filter((_, index) => index !== 0),
        { row: true, terminal: { wrap: true }, gap: 1 },
      ),
      ui.text('schedule-history', text.scheduleHistory, props.theme.text, 600),
    );
    if (!task.runs.length)
      content.push(ui.text('schedule-history-empty', text.scheduleNoHistory, props.theme.muted));
    for (const [index, run] of task.runs.entries())
      content.push(
        ui.box(
          'schedule-run-' + run.id,
          [
            ui.text(
              'schedule-run-status-' + run.id,
              `${date(run.at)} · ${run.outcome === 'ok' ? (run.sessionId ? text.scheduleAdmitted : text.scheduleAcknowledged) : run.outcome === 'blocked' ? text.scheduleBlocked : text.scheduleFailed}`,
            ),
            ...(run.outcome !== 'ok'
              ? [
                  ui.text(
                    'schedule-run-error-' + run.id,
                    run.message === 'scheduler_authorization_changed'
                      ? text.scheduleTargetChanged
                      : text.scheduleUnavailable,
                    props.theme.error,
                  ),
                ]
              : []),
            ...(run.sessionId
              ? [ui.button('conversation-' + index, text.recallOpen, disabled)]
              : []),
          ],
          { gap: 0 },
        ),
      );
    if (task.runs.length) content.push(ui.button('clear-history', text.scheduleClear, disabled));
  }
  const body = ui.box(
    'schedule-page',
    [
      ...pageHeader(
        presentation,
        state,
        props,
        events,
        state.picker
          ? text.scheduleSource
          : state.route === 'form'
            ? state.form?.id
              ? text.edit
              : text.scheduleNew
            : text.scheduleTitle,
        'back',
        false,
      ),
      ...(state.notice
        ? [ui.text('schedule-notice', text[state.notice], props.theme.warning)]
        : []),
      ...(state.pending || state.restoring
        ? [ui.text('schedule-loading', text.terminalLoading, props.theme.muted)]
        : []),
      ...(state.current
        ? [ui.button('receipt', text.executionReceipt, !!state.pending || !state.current.reference)]
        : []),
      ...(state.historyFailed
        ? [ui.button('history-retry', text.sourceRetry, !!state.pending)]
        : []),
      ...(state.watchFailed ? [ui.button('refresh', text.directoryRefresh, disabled)] : []),
      formView,
      ui.box(
        'schedule-reading-container',
        [
          presentation.reading(
            'schedule-reading',
            {
              content: ui.box('schedule-content', content, {
                gap: 1,
                scroll: 'anchor',
                focusGroup: state.picker || state.route === 'list',
                terminal: { grow: 1, basis: 0 },
              }),
            },
            { input: 'schedule-input', observed: 'schedule-viewport' },
          ),
        ],
        { visible: state.route !== 'form' || state.picker, terminal: { grow: 1, basis: 0 } },
      ),
      ui.box(
        'schedule-footer',
        state.picker
          ? [ui.button('sessions-refresh', text.directoryRefresh, disabled)]
          : state.route === 'form'
            ? [ui.button('review', text.scheduleReview, editing, true)]
            : state.route === 'list'
              ? [ui.button('refresh', text.directoryRefresh, disabled)]
              : [ui.button('refresh', text.directoryRefresh, disabled)],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    {
      visible: props.open,
      autoFocus: true,
      gap: 1,
      shortcuts: [{ key: 'escape', modifiers: [], input: 'back', enabled: true }],
      terminal: { grow: 1 },
    },
  );
  const confirmation = state.confirm;
  const reviewed =
    confirmation?.kind === 'create' || confirmation?.kind === 'update' ? confirmation.input : null;
  const effect = reviewed?.task.effect ?? state.confirmTask?.task.effect;
  const consent =
    !!confirmation &&
    ['create', 'update', 'approve', 'resume', 'trigger_now'].includes(confirmation.kind);
  const reviewView = ui.box(
    'schedule-confirm',
    [
      ui.text(
        'schedule-confirm-title',
        reviewed?.task.title ?? state.confirmTask?.task.title ?? '',
        props.theme.text,
        600,
      ),
      ...(reviewed
        ? [
            ui.text(
              'schedule-confirm-time',
              `${scheduleSummary(reviewed.task.schedule, text)} · ${reviewed.timezone}`,
            ),
            ui.text('schedule-confirm-body', reviewed.task.intentBody),
          ]
        : []),
      ...(consent && effect
        ? [
            ui.text('schedule-confirm-effect', effectLabel(effect.kind, text)),
            ...(effect.kind !== 'reminder'
              ? [
                  ui.text('schedule-confirm-path', compactLabel(effect.target.workspace.path, 400)),
                  ui.text('schedule-confirm-model', effect.model.modelId),
                  ui.text(
                    'schedule-confirm-permissions',
                    effect.target.permissions.access === 'read_only'
                      ? text.scheduleReadOnly
                      : text.scheduleWorkspaceWrite,
                  ),
                  ...(effect.target.permissions.requestPermissions
                    ? [ui.text('schedule-confirm-additional', text.scheduleAdditional)]
                    : []),
                ]
              : []),
            ui.text(
              'schedule-consent',
              effect.kind === 'reminder' ? text.scheduleReminderConsent : text.scheduleConsent,
              props.theme.muted,
            ),
          ]
        : [
            ui.text(
              'schedule-confirm-description',
              confirmation?.kind === 'delete' ? text.scheduleDeleteHelp : text.scheduleClearHelp,
            ),
          ]),
      ...(state.notice
        ? [ui.text('schedule-confirm-notice', text[state.notice], props.theme.warning)]
        : []),
      ...(state.current
        ? [
            ui.button(
              'confirm-receipt',
              text.executionReceipt,
              !!state.pending || !state.current.reference,
            ),
          ]
        : []),
      ui.box(
        'schedule-confirm-actions',
        [
          ui.button('confirm-cancel', text.cancel, !!state.current || !!state.pending),
          ui.button(
            'confirm',
            confirmation?.kind === 'delete'
              ? text.scheduleDelete
              : confirmation?.kind === 'clear_history'
                ? text.scheduleClear
                : confirmation?.kind === 'trigger_now'
                  ? text.scheduleRun
                  : confirmation?.kind === 'update'
                    ? text.scheduleSave
                    : text.scheduleEnable,
            disabled,
            true,
          ),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    { gap: 1, scroll: 'manual' },
  );
  const view = ui.dialog('schedule-review', text.scheduleReview, body, reviewView, !!confirmation, {
    width: 72,
    height: 24,
  });
  const out = output(state, view, events);
  out.actions = actions;
  out.subscriptions =
    state.watch && props.open && !state.restoring
      ? [{ ...state.watch, binding: 'agent', member: 0, method: 'watchSchedules', resources: {} }]
      : [];
  return out;
}
