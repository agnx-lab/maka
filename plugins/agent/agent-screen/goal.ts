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
  AgentGoalChange,
  AgentGoalView,
  AgentGoalResult,
  ComponentInput,
  EditorCommand,
  EditorObservation,
  InputProps,
  TextareaProps,
  TextChunk,
  RecoveryRef,
  GoalControl,
} from '../../../packages/plugin-sdk/src/index.js';
import type { Theme } from '../../shared/contracts.js';
import { copy, goalActions, goalStatuses } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';
import { intent, mutationPending } from './actions.js';
import type { State } from './state.js';

const fields = ['objective', 'iterations', 'budget'] as const;
type Field = (typeof fields)[number];
type Draft = {
  observation: EditorObservation | null;
  command: EditorCommand | null;
  reset: string | null;
  sequence: number;
  ready: boolean;
};
export type GoalRequest =
  | { kind: 'goalChange'; epoch: number; sessionId: string }
  | {
      kind: 'goalField';
      epoch: number;
      attempt: number;
      field: Field;
      observation: EditorObservation;
    }
  | { kind: 'goalReceipt'; reference: RecoveryRef };
export interface GoalState {
  watch: { key: string; input: { sessionId: string } } | null;
  page: AgentGoalView | null;
  failed: boolean;
  fields: Record<Field, Draft>;
  capture: {
    attempt: number;
    start: boolean;
    observations: Record<Field, EditorObservation>;
    values: Partial<Record<Field, string>>;
  } | null;
  confirmation: AgentGoalChange | null;
}
export function newGoalState(): GoalState {
  const draft = (reset: string): Draft => ({
    observation: null,
    command: null,
    reset,
    sequence: 0,
    ready: false,
  });
  return {
    watch: null,
    page: null,
    failed: false,
    capture: null,
    confirmation: null,
    fields: { objective: draft(''), iterations: draft('10'), budget: draft('') },
  };
}
export function watchGoal(state: State) {
  if (!state.selected) return;
  state.goal.watch = { key: `goal-${++state.serial}`, input: { sessionId: state.selected.id } };
  state.goal.failed = false;
}
export function goalUpdate(state: State, page: AgentGoalView) {
  state.goal.page = page;
  const confirmation = state.goal.confirmation;
  if (
    confirmation?.kind === 'control' &&
    (confirmation.goalId !== page.current?.id || confirmation.revision !== page.current?.revision)
  ) {
    state.goal.confirmation = null;
    state.notice = 'goalChanged';
  }
}
export function goalTerminal(status: NonNullable<AgentGoalView['current']>['status']) {
  return [
    'achieved',
    'impossible',
    'cancelled',
    'cancellation_unknown',
    'max_iterations',
    'budget_limited',
    'budget_unknown',
  ].includes(status);
}
function canCreate(state: State) {
  const current = state.goal.page?.current;
  return (
    !!state.goal.page?.enabled && (!current || (goalTerminal(current.status) && !current.pending))
  );
}
function available(state: State) {
  return (
    !!state.selected && !state.selected.archived && !mutationPending(state) && !state.goal.capture
  );
}
function approval(state: State) {
  const session = state.selected!;
  return {
    target: {
      sessionId: session.id,
      workspace: { ...session.workspace },
      permissions: { ...session.permissions },
      boundaryRevision: session.boundaryRevision,
    },
    model: { ...state.model!.choice.selection },
    thinkingLevel: state.model!.thinkingLevel ?? null,
  };
}
function captureNext(state: State, actions: Action[]) {
  const capture = state.goal.capture;
  if (!capture) return;
  const field = fields.find((name) => capture.values[name] === undefined);
  if (field) {
    const observation = capture.observations[field],
      basis = observation.snapshot.text;
    intent(
      state,
      actions,
      { kind: 'goalField', epoch: state.epoch, attempt: capture.attempt, field, observation },
      'reader',
      'read',
      { start: 0, end: basis.bytes },
      {
        text: {
          child: 'goal-field-' + field,
          method: 'snapshot',
          input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
        },
      },
    );
    return;
  }
  state.goal.capture = null;
  const objective = capture.values.objective!.trim(),
    iterations = capture.values.iterations!.trim(),
    budget = capture.values.budget!.trim();
  if (
    !objective ||
    !/^[0-9]+$/.test(iterations) ||
    Number(iterations) < 1 ||
    Number(iterations) > 100 ||
    (budget &&
      (!/^[0-9]+$/.test(budget) || Number(budget) < 1 || Number(budget) > 1_000_000_000)) ||
    !state.model ||
    state.executor
  ) {
    state.notice = 'goalInvalid';
    return;
  }
  state.notice = null;
  state.goal.confirmation = {
    kind: 'create',
    sessionId: state.selected!.id,
    objective,
    maxIterations: Number(iterations),
    tokenBudget: budget ? Number(budget) : null,
    start: capture.start,
    approval: approval(state),
  };
}
export function goalFieldResult(
  state: State,
  actions: Action[],
  request: Extract<GoalRequest, { kind: 'goalField' }>,
  value: unknown,
) {
  const capture = state.goal.capture;
  if (request.epoch !== state.epoch || !capture || capture.attempt !== request.attempt) return;
  const chunk = value as TextChunk;
  if (
    !chunk ||
    !sameValue(chunk.metadata, request.observation.snapshot.text) ||
    !sameValue(
      state.goal.fields[request.field].observation?.snapshot.text,
      request.observation.snapshot.text,
    ) ||
    new TextEncoder().encode(chunk.text).length !== request.observation.snapshot.text.bytes
  ) {
    state.goal.capture = null;
    state.notice = 'goalInvalid';
    return;
  }
  capture.values[request.field] = chunk.text;
  captureNext(state, actions);
}
export function goalChanged(state: State, result: AgentGoalResult) {
  state.goal.confirmation = null;
  state.notice =
    result.kind === 'committed'
      ? null
      : result.kind === 'conflict'
        ? 'goalChanged'
        : 'goalRejected';
  watchGoal(state);
}
export function goalComponent(
  state: State,
  input: Extract<ComponentInput, { kind: 'component' }>,
): boolean {
  const name = fields.find((name) => input.child === 'goal-field-' + name);
  if (!name || input.name !== 'changed') return false;
  const field = state.goal.fields[name];
  const observed = input.event as EditorObservation;
  if (field.observation && field.observation.snapshot.text.owner !== observed.snapshot.text.owner) {
    field.command = null;
    field.ready = false;
    field.sequence = 0;
    field.reset = name === 'iterations' ? '10' : '';
  }
  field.observation = observed;
  const receipt = field.observation.receipt;
  if (
    field.command &&
    receipt?.owner === field.command.owner &&
    receipt.sequence === field.command.sequence
  ) {
    field.ready = receipt.outcome.status === 'applied';
    field.command = null;
    field.reset = null;
    if (!field.ready) state.notice = 'goalInvalid';
  } else if (field.reset !== null && !field.command) {
    field.command = {
      owner: field.observation.snapshot.text.owner,
      basedOn: field.observation.revision,
      sequence: ++field.sequence,
      edit: { kind: 'reset', text: field.reset },
    };
  }
  return true;
}
export function goalAction(state: State, actions: Action[], key: string): boolean {
  if (!key.startsWith('goal-') || key === 'goal-open') return false;
  if (key === 'goal-close') {
    if (state.dialog === 'goal') state.dialog = null;
    if (state.inspector === 'goal') state.inspector = null;
    state.goal.capture = null;
    state.goal.confirmation = null;
  } else if (key === 'goal-refresh') watchGoal(state);
  else if (key === 'goal-dismiss') state.goal.confirmation = null;
  else if (key === 'goal-check' && state.uncertain?.reference.method === 'changeGoal') {
    intent(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.uncertain.reference);
  } else if (available(state)) {
    const current = state.goal.page?.current;
    if (
      (key === 'goal-start' || key === 'goal-save') &&
      canCreate(state) &&
      state.model &&
      !state.executor
    ) {
      if (
        fields.some((name) => {
          const field = state.goal.fields[name];
          return (
            !field.ready ||
            !!field.command ||
            !field.observation ||
            field.observation.snapshot.marked ||
            field.observation.snapshot.text.bytes > (name === 'objective' ? 8192 : 10)
          );
        })
      ) {
        state.notice = 'goalInvalid';
        return true;
      }
      state.goal.capture = {
        attempt: ++state.serial,
        start: key === 'goal-start',
        observations: Object.fromEntries(
          fields.map((name) => [name, state.goal.fields[name].observation]),
        ) as Record<Field, EditorObservation>,
        values: {},
      };
      captureNext(state, actions);
    } else if (key === 'goal-confirm' && state.goal.confirmation) {
      const input = state.goal.confirmation;
      if (input.sessionId !== state.selected!.id) return true;
      state.goal.confirmation = null;
      intent(
        state,
        actions,
        { kind: 'goalChange', epoch: state.epoch, sessionId: state.selected!.id },
        'agent',
        'changeGoal',
        input,
      );
    } else if (current) {
      const controls: Record<string, GoalControl | undefined> = {
        'goal-resume': 'resume',
        'goal-pause': 'pause',
        'goal-cancel': 'cancel',
        'goal-complete': 'complete',
      };
      const action = controls[key];
      if (action) {
        state.notice = null;
        const input: AgentGoalChange = {
          kind: 'control',
          sessionId: state.selected!.id,
          goalId: current.id,
          revision: current.revision,
          action,
        };
        if (action === 'resume' && !current.pending) {
          if (!state.model || state.executor) {
            state.notice = 'goalChooseModel';
            return true;
          }
          input.approval = approval(state);
        }
        if (action === 'resume' || action === 'cancel' || action === 'complete')
          state.goal.confirmation = input;
        else
          intent(
            state,
            actions,
            { kind: 'goalChange', epoch: state.epoch, sessionId: state.selected!.id },
            'agent',
            'changeGoal',
            input,
          );
      }
    }
  }
  return true;
}
export function goalView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    goal = state.goal,
    current = goal.page?.current;
  const open = state.inspector === 'goal' && state.dialog === null;
  const editable = available(state) && !goal.confirmation;
  const form = canCreate(state);
  const field = (name: Field, label: string): N => {
    const common = {
      accessibilityLabel: label,
      tabIndex: fields.indexOf(name),
      autoFocus: name === 'objective',
      readOnly: !editable,
      disabled: false,
      placeholder: '',
      command: goal.fields[name].command,
    };
    return ui.box(
      'goal-input-' + name,
      [
        ui.text('goal-label-' + name, label, theme.muted),
        name === 'objective'
          ? presentation.slot(
              'goal-field-' + name,
              'editor',
              {
                ...common,
                appearance: theme.textarea,
                softWrap: true,
                terminalRows: { min: 3, max: 6 },
                submit: 'primary_enter',
                menu: {
                  appearance: theme.menu,
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
              { changed: 'changed' },
            )
          : presentation.slot(
              'goal-field-' + name,
              'input',
              {
                menu: editorMenu(theme, locale),
                ...common,
                appearance: theme.input,
              } satisfies InputProps,
              { changed: 'changed' },
            ),
      ],
      { gap: 0 },
    );
  };
  const confirmation = goal.confirmation;
  const title =
    confirmation?.kind === 'control'
      ? text[goalActions[confirmation.action]]
      : confirmation?.start
        ? text.goalStart
        : text.goalSave;
  return ui.box(
    'goal-panel',
    [
      ui.box(
        'goal-header',
        [
          ui.text('goal-title', text.goalTitle, theme.text, 550),
          ui.button('goal-close', text.closeModels),
        ],
        { row: true, gap: 1 },
      ),
      ...(state.notice &&
      (state.notice.startsWith('goal') || state.uncertain?.reference.method === 'changeGoal')
        ? [ui.text('goal-notice', text[state.notice], theme.warning)]
        : []),
      ...(goal.failed
        ? [
            ui.text('goal-error', text.goalUnavailable),
            ui.button('goal-refresh', text.checklistRetry),
          ]
        : !goal.page
          ? [ui.text('goal-loading', text.goalLoading)]
          : !goal.page.enabled
            ? [ui.text('goal-disabled', text.goalDisabled)]
            : []),
      ...(current
        ? [
            ui.text('goal-objective', current.objective),
            ui.text(
              'goal-state',
              `${text[goalStatuses[current.status]]} · ${current.iterations}/${current.maxIterations}`,
              theme.muted,
            ),
            ...(current.tokenBudget != null
              ? [
                  ui.text(
                    'goal-tokens',
                    `${text.goalTokens}: ${current.consumed.known}/${current.tokenBudget}${current.consumed.missing ? ' · ' + text.goalUnknownTokens : ''}`,
                  ),
                ]
              : []),
            ...(current.report?.note || current.note
              ? [ui.text('goal-note', current.report?.note || current.note)]
              : []),
            ...(current.pending ? [ui.text('goal-pending', text.goalPending, theme.muted)] : []),
            ui.box(
              'goal-controls',
              [
                ...(!goalTerminal(current.status)
                  ? [
                      ...(['armed', 'paused', 'waiting', 'blocked'].includes(current.status)
                        ? [ui.button('goal-resume', text.goalAction_resume, !editable)]
                        : []),
                      ...(current.status !== 'paused' && current.status !== 'armed'
                        ? [ui.button('goal-pause', text.goalAction_pause, !editable)]
                        : []),
                      ...(!current.pending
                        ? [ui.button('goal-complete', text.goalAction_complete, !editable)]
                        : []),
                      ui.button('goal-cancel', text.goalAction_cancel, !editable),
                    ]
                  : current.pending
                    ? [ui.button('goal-cancel', text.goalCheckCancel, !editable)]
                    : []),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ]
        : []),
      ...(form
        ? [
            ui.box(
              'goal-form',
              [
                field('objective', text.goalObjective),
                field('iterations', text.goalIterations),
                field('budget', text.goalTokens),
                ui.text('goal-budget-note', text.goalBudgetNote, theme.muted),
                ...(!state.model || state.executor
                  ? [ui.text('goal-model', text.goalChooseModel)]
                  : []),
                ui.box(
                  'goal-create',
                  [
                    ui.button(
                      'goal-save',
                      text.goalSave,
                      !editable || !state.model || !!state.executor,
                    ),
                    ui.button(
                      'goal-start',
                      text.goalStart,
                      !editable || !state.model || !!state.executor,
                    ),
                  ],
                  { row: true, gap: 1 },
                ),
              ],
              { visible: !confirmation, gap: 1 },
            ),
          ]
        : []),
      ...(confirmation
        ? [
            ui.text('goal-confirm-title', title, theme.text, 550),
            ...(confirmation.kind === 'create'
              ? [
                  ui.text('goal-confirm-objective', confirmation.objective),
                  ui.text(
                    'goal-confirm-limits',
                    `${text.goalIterations}: ${confirmation.maxIterations} · ${text.goalTokens}: ${confirmation.tokenBudget ?? text.goalNoBudget}`,
                  ),
                ]
              : []),
            ...(confirmation.kind === 'create' || confirmation.action === 'resume'
              ? [
                  ui.text('goal-approval', text.goalApproval),
                  ...(confirmation.approval
                    ? [
                        ui.text(
                          'goal-approved-workspace',
                          confirmation.approval.target.workspace.path,
                        ),
                        ui.text(
                          'goal-approved-access',
                          `${confirmation.approval.target.permissions.access === 'read_only' ? text.permissionReadOnly : text.permissionWorkspaceWrite} · ${confirmation.approval.target.permissions.requestPermissions ? text.permissionAsk : text.permissionNoAsk}`,
                        ),
                      ]
                    : []),
                  ui.text(
                    'goal-approved-model',
                    confirmation.kind === 'create'
                      ? confirmation.approval.model.modelId
                      : (confirmation.approval?.model.modelId ?? text.goalPending),
                  ),
                ]
              : [
                  ui.text(
                    'goal-control-note',
                    confirmation.action === 'cancel' ? text.goalCancelNote : text.goalCompleteNote,
                  ),
                ]),
            ui.box(
              'goal-confirmation',
              [
                ui.button('goal-confirm', text.goalConfirm, !available(state)),
                ui.button('goal-dismiss', text.cancel),
              ],
              { row: true, gap: 1 },
            ),
          ]
        : []),
      ...(state.uncertain?.reference.method === 'changeGoal'
        ? [
            ui.button(
              'goal-check',
              text.check,
              Object.values(state.pending).some(
                (request) => request.kind === 'receipt' || request.kind === 'goalReceipt',
              ),
            ),
          ]
        : []),
    ],
    { visible: open, gap: 1, scroll: 'manual', terminal: { grow: 1 } },
  );
}
