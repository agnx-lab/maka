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
  AgentPlanChange,
  AgentPlanView,
  AgentPlanResult,
  AgentPlanSnapshot,
  AgentPlanHistory,
  AgentMetadataResult,
  RecoveryRef,
} from '../../../packages/plugin-sdk/src/index.js';
import type { State } from './state.js';
import type { Theme } from '../../shared/contracts.js';
import { copy, planProposals, planExecutions, planSteps, planActions } from './locale.js';
import { type Node, type Presentation } from '../../shared/view.js';
import { intent, mutationPending } from './actions.js';

export type PlanRequest =
  | { kind: 'planChange' | 'planMode'; epoch: number; sessionId: string }
  | { kind: 'planReceipt'; reference: RecoveryRef }
  | { kind: 'planHistory'; epoch: number; attempt: number };
export interface PlanState {
  watch: { key: string; input: { sessionId: string } } | null;
  page: AgentPlanView | null;
  failed: boolean;
  confirmation: { input: AgentPlanChange; snapshot: AgentPlanSnapshot } | null;
  history: {
    page: AgentPlanHistory | null;
    after: number;
    back: number[];
    selected: number | null;
    loading: number | null;
  } | null;
}
export function newPlanState(): PlanState {
  return { watch: null, page: null, failed: false, confirmation: null, history: null };
}
export function watchPlan(state: State) {
  if (!state.selected) return;
  state.plan.watch = { key: `plan-${++state.serial}`, input: { sessionId: state.selected.id } };
  state.plan.failed = false;
}
export function planUpdate(state: State, page: AgentPlanView) {
  state.plan.page = page;
  if (state.plan.confirmation && state.plan.confirmation.input.revision !== page.current.revision) {
    state.plan.confirmation = null;
    state.notice = 'planChanged';
  }
}
export function planChanged(state: State, result: AgentPlanResult) {
  state.plan.confirmation = null;
  state.notice =
    result.kind === 'committed'
      ? null
      : result.kind === 'conflict'
        ? 'planChanged'
        : 'planRejected';
  watchPlan(state);
}
export function planModeChanged(state: State, result: AgentMetadataResult) {
  if (result.kind === 'committed' && result.session.id === state.selected?.id) {
    if (result.session.revision >= state.selected.revision) state.selected = result.session;
    state.notice = null;
  } else
    state.notice =
      result.kind === 'rejected' && result.reason === 'busy' ? 'planModeBusy' : 'planChanged';
}
export function planHistoryResult(
  state: State,
  request: Extract<PlanRequest, { kind: 'planHistory' }>,
  value: AgentPlanHistory,
) {
  const history = state.plan.history;
  if (request.epoch !== state.epoch || !history || request.attempt !== history.loading) return;
  history.page = value;
  history.loading = null;
  history.selected = value.snapshots[0]?.revision ?? null;
}
function loadHistory(state: State, actions: Action[]) {
  const history = state.plan.history;
  if (!history || !state.selected || history.loading !== null) return;
  const attempt = ++state.serial;
  history.loading = attempt;
  intent(
    state,
    actions,
    { kind: 'planHistory', epoch: state.epoch, attempt },
    'agent',
    'planHistory',
    {
      sessionId: state.selected.id,
      throughRevision: history.page?.throughRevision ?? state.plan.page?.current.revision ?? 0,
      after: history.after,
    },
  );
}
function available(state: State) {
  return (
    !!state.selected &&
    !state.selected.archived &&
    !!state.plan.page?.enabled &&
    !mutationPending(state)
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
export function planAction(state: State, actions: Action[], key: string): boolean {
  if (!key.startsWith('plan-') || key === 'plan-open') return false;
  const plan = state.plan,
    current = plan.page?.current;
  if (key === 'plan-close') {
    if (state.dialog === 'plan') state.dialog = null;
    if (state.inspector === 'plan') state.inspector = null;
    plan.confirmation = null;
  } else if (key === 'plan-refresh') watchPlan(state);
  else if (key === 'plan-dismiss') plan.confirmation = null;
  else if (key === 'plan-current') plan.history = null;
  else if (key === 'plan-history' && current) {
    plan.confirmation = null;
    plan.history = { page: null, after: 0, back: [], selected: null, loading: null };
    loadHistory(state, actions);
  } else if (
    key === 'plan-history-next' &&
    plan.history?.page?.nextAfter != null &&
    plan.history.loading === null
  ) {
    plan.history.back.push(plan.history.after);
    plan.history.after = plan.history.page.nextAfter;
    loadHistory(state, actions);
  } else if (
    key === 'plan-history-back' &&
    plan.history?.back.length &&
    plan.history.loading === null
  ) {
    plan.history.after = plan.history.back.pop()!;
    loadHistory(state, actions);
  } else if (key.startsWith('plan-revision-') && plan.history?.page) {
    const revision = Number(key.slice('plan-revision-'.length));
    if (plan.history.page.snapshots.some((snapshot) => snapshot.revision === revision))
      plan.history.selected = revision;
  } else if (key === 'plan-check' && state.uncertain?.reference.method === 'changePlan') {
    intent(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.uncertain.reference);
  } else if (available(state) && current && !plan.history) {
    if (key === 'plan-mode-plan' || key === 'plan-mode-agent') {
      const session = state.selected!;
      plan.confirmation = null;
      intent(
        state,
        actions,
        { kind: 'planMode', epoch: state.epoch, sessionId: session.id },
        'agent',
        'metadata',
        {
          sessionId: session.id,
          expectedRevision: session.revision,
          title: session.title,
          labels: [...session.labels],
          collaborationMode: key === 'plan-mode-plan' ? 'plan' : 'agent',
        },
      );
    } else if (key === 'plan-confirm' && plan.confirmation) {
      const input = plan.confirmation.input;
      if (input.sessionId !== state.selected!.id || input.revision !== current.revision) {
        plan.confirmation = null;
        state.notice = 'planChanged';
        return true;
      }
      plan.confirmation = null;
      intent(
        state,
        actions,
        { kind: 'planChange', epoch: state.epoch, sessionId: state.selected!.id },
        'agent',
        'changePlan',
        input,
      );
    } else {
      let action: AgentPlanChange['action'] | null = null;
      const proposal = current.proposal,
        execution = current.execution;
      if (
        key === 'plan-approve' &&
        proposal?.status === 'pending_approval' &&
        state.model &&
        !state.executor
      )
        action = {
          kind: 'approve',
          proposalId: proposal.id,
          proposalRevision: proposal.revision,
          approval: approval(state),
        };
      else if (
        key === 'plan-resume' &&
        execution?.phase === 'interrupted' &&
        state.model &&
        !state.executor
      )
        action = { kind: 'resume', executionId: execution.id, approval: approval(state) };
      else if (
        (key === 'plan-revise' || key === 'plan-abandon') &&
        proposal &&
        ['pending_approval', 'revision_requested'].includes(proposal.status)
      )
        action = { kind: key === 'plan-revise' ? 'revise' : 'abandon', proposalId: proposal.id };
      else if (
        key === 'plan-cancel' &&
        execution &&
        ['awaiting_admission', 'active', 'interrupted'].includes(execution.phase)
      )
        action = {
          kind: 'cancel',
          executionId: execution.id,
          reason: 'User cancelled the reviewed Plan',
        };
      if (action) {
        state.notice = null;
        plan.confirmation = {
          input: { sessionId: state.selected!.id, revision: current.revision, action },
          snapshot: JSON.parse(JSON.stringify(current)) as AgentPlanSnapshot,
        };
      }
    }
  }
  return true;
}

export function planView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    plan = state.plan;
  const open = state.inspector === 'plan' && state.dialog === null;
  const historical = plan.history?.page?.snapshots.find(
    (snapshot) => snapshot.revision === plan.history?.selected,
  );
  const snapshot =
    plan.confirmation?.snapshot ?? historical ?? (plan.history ? null : plan.page?.current);
  const proposal = snapshot?.proposal,
    execution = snapshot?.execution;
  const showProposal =
    !!proposal &&
    (!execution || proposal.id !== execution.proposalId || proposal.status !== 'approved');
  const artifact = showProposal ? proposal!.artifact : (execution?.artifact ?? proposal?.artifact);
  const active = execution && ['awaiting_admission', 'active'].includes(execution.phase);
  const confirmation = plan.confirmation,
    action = confirmation?.input.action;
  const grant =
    action && (action.kind === 'approve' || action.kind === 'resume') ? action.approval : null;
  const editable = available(state) && !plan.history && !confirmation;
  return ui.box(
    'plan-panel',
    [
      ui.box(
        'plan-header',
        [
          ui.text('plan-title', text.planTitle, theme.text, 550),
          ui.button('plan-close', text.closeModels),
        ],
        { row: true, gap: 1 },
      ),
      ...(state.notice &&
      (state.notice.startsWith('plan') || state.uncertain?.reference.method === 'changePlan')
        ? [ui.text('plan-notice', text[state.notice], theme.warning)]
        : []),
      ...(plan.failed
        ? [
            ui.text('plan-error', text.planUnavailable),
            ui.button('plan-refresh', text.checklistRetry),
          ]
        : !plan.page
          ? [ui.text('plan-loading', text.planLoading)]
          : !plan.page.enabled
            ? [ui.text('plan-disabled', text.planDisabled)]
            : []),
      ...(!plan.history && !confirmation && plan.page?.enabled
        ? [
            ui.text(
              'plan-mode',
              state.selected?.collaborationMode === 'plan' ? text.planPlanning : text.planAgent,
              theme.muted,
            ),
            ui.text('plan-mode-note', text.planModeNote, theme.muted),
            ui.button(
              state.selected?.collaborationMode === 'plan' ? 'plan-mode-agent' : 'plan-mode-plan',
              state.selected?.collaborationMode === 'plan'
                ? text.planSwitchAgent
                : text.planSwitchPlanning,
              !editable || state.selected?.activity !== 'idle' || !!state.executor,
            ),
          ]
        : []),
      ...(plan.history
        ? [
            ui.text('plan-history-title', text.planHistory),
            ui.button('plan-current', text.planCurrent),
            ...(plan.history.loading !== null
              ? [ui.text('plan-history-loading', text.planLoading)]
              : []),
            ui.box(
              'plan-revisions',
              (plan.history.page?.snapshots ?? []).map((snapshot) =>
                ui.button(
                  `plan-revision-${snapshot.revision}`,
                  `${text.planRevision} ${snapshot.revision}`,
                  false,
                  snapshot.revision === plan.history?.selected,
                ),
              ),
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
            ui.box(
              'plan-history-navigation',
              [
                ui.button(
                  'plan-history-back',
                  text.planHistoryBack,
                  !plan.history.back.length || plan.history.loading !== null,
                ),
                ui.button(
                  'plan-history-next',
                  text.planHistoryNext,
                  plan.history.page?.nextAfter == null || plan.history.loading !== null,
                ),
              ],
              { row: true, gap: 1 },
            ),
          ]
        : snapshot && snapshot.revision > 0 && !confirmation
          ? [ui.button('plan-history', text.planHistory)]
          : []),
      ...(snapshot && snapshot.revision > 0
        ? [ui.text('plan-revision', `${text.planRevision} ${snapshot.revision}`, theme.muted)]
        : []),
      ...(showProposal
        ? [ui.text('plan-proposal-state', text[planProposals[proposal!.status]], theme.muted)]
        : execution
          ? [
              ui.text('plan-execution-state', text[planExecutions[execution.phase]], theme.muted),
              ...(execution.reason
                ? [ui.text('plan-reason', execution.reason, theme.warning)]
                : []),
              ...(execution.cancellation && active
                ? [ui.text('plan-cancelling', text.planCancelling, theme.warning)]
                : []),
            ]
          : []),
      ...(artifact
        ? [
            ui.text('plan-artifact-title', artifact.title, theme.text, 550),
            ...(artifact.overview ? [ui.text('plan-overview', artifact.overview)] : []),
            ...artifact.steps.map((step, index) =>
              ui.box(
                `plan-step-${step.id}`,
                [
                  ui.text(
                    `plan-step-title-${step.id}`,
                    `${index + 1}. ${step.title}${!showProposal && execution ? ' · ' + text[planSteps[execution.steps.find((progress) => progress.id === step.id)?.status ?? 'pending']] : ''}`,
                    theme.text,
                    550,
                  ),
                  ui.text(`plan-step-description-${step.id}`, step.description),
                  ...((step.files ?? []).length
                    ? [
                        ui.text(
                          `plan-step-files-${step.id}`,
                          (step.files ?? []).join(', '),
                          theme.muted,
                        ),
                      ]
                    : []),
                  ...(!showProposal &&
                  execution?.steps.find((progress) => progress.id === step.id)?.note
                    ? [
                        ui.text(
                          `plan-step-note-${step.id}`,
                          execution.steps.find((progress) => progress.id === step.id)!.note!,
                          theme.muted,
                        ),
                      ]
                    : []),
                ],
                { gap: 0 },
              ),
            ),
            ...((artifact.risks ?? []).length
              ? [
                  ui.text('plan-risks-title', text.planRisks, theme.text, 550),
                  ...(artifact.risks ?? []).map((risk, index) =>
                    ui.text(`plan-risk-${index}`, risk),
                  ),
                ]
              : []),
          ]
        : plan.page?.enabled && !plan.history
          ? [ui.text('plan-empty', text.planEmpty)]
          : []),
      ...(confirmation && action
        ? [
            ui.text('plan-confirm-title', text[planActions[action.kind]], theme.text, 550),
            ...(grant
              ? [
                  ui.text('plan-approval', text.planApproval),
                  ui.text('plan-approved-model', grant.model.modelId),
                  ui.text('plan-approved-workspace', grant.target.workspace.path),
                  ui.text(
                    'plan-approved-access',
                    `${grant.target.permissions.access === 'read_only' ? text.permissionReadOnly : text.permissionWorkspaceWrite} · ${grant.target.permissions.requestPermissions ? text.permissionAsk : text.permissionNoAsk}`,
                  ),
                ]
              : [
                  ui.text(
                    'plan-confirm-note',
                    action.kind === 'cancel' ? text.planCancelNote : text.planDecisionNote,
                  ),
                ]),
            ui.box(
              'plan-confirmation',
              [
                ui.button('plan-confirm', text.planConfirm, !available(state)),
                ui.button('plan-dismiss', text.cancel),
              ],
              { row: true, gap: 1 },
            ),
          ]
        : !plan.history
          ? [
              ...(!state.model || state.executor
                ? [ui.text('plan-model', text.planChooseModel, theme.muted)]
                : []),
              ui.box(
                'plan-controls',
                [
                  ...(proposal &&
                  ['pending_approval', 'revision_requested'].includes(proposal.status)
                    ? [
                        ...(proposal.status === 'pending_approval'
                          ? [
                              ui.button(
                                'plan-approve',
                                text.planAction_approve,
                                !editable || !state.model || !!state.executor,
                              ),
                            ]
                          : []),
                        ui.button('plan-revise', text.planAction_revise, !editable),
                        ui.button('plan-abandon', text.planAction_abandon, !editable),
                      ]
                    : []),
                  ...(execution?.phase === 'interrupted'
                    ? [
                        ui.button(
                          'plan-resume',
                          text.planAction_resume,
                          !editable || !state.model || !!state.executor,
                        ),
                      ]
                    : []),
                  ...(active || execution?.phase === 'interrupted'
                    ? [
                        ui.button(
                          'plan-cancel',
                          text.planAction_cancel,
                          !editable || !!execution?.cancellation,
                        ),
                      ]
                    : []),
                ],
                { row: true, gap: 1, terminal: { wrap: true } },
              ),
            ]
          : []),
      ...(state.uncertain?.reference.method === 'changePlan'
        ? [
            ui.button(
              'plan-check',
              text.check,
              Object.values(state.pending).some(
                (request) => request.kind === 'receipt' || request.kind === 'planReceipt',
              ),
            ),
          ]
        : []),
    ],
    { visible: open, gap: 1, scroll: 'manual', terminal: { grow: 1 } },
  );
}
