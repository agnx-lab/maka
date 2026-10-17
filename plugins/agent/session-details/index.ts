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
  AgentMetadata,
  AgentMetadataResult,
  AgentObservation,
  AgentSession,
  AgentWorkspacePermissions,
  AgentPermissionChange,
  CallerIntent,
  Component,
  ComponentInput,
  EditorCommand,
  EditorObservation,
  EditorTextRef,
  Emission,
  OperationReply,
  InputProps,
  TextareaProps,
  RecoveryRef,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { sessionDetailsContract, type SessionDetailsProps } from '../../shared/contracts.js';
import { scrollRows, resetScroll } from '../../shared/scroll.js';
import { copy, type Copy } from './locale.js';
import { panelHeader } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Presentation, type Node } from '../../shared/view.js';

const names = ['title', 'labels'] as const;
type FieldName = (typeof names)[number];
interface Field {
  baseline: EditorTextRef | null;
  observation: EditorObservation | null;
  command: EditorCommand | null;
  sequence: number;
  phase: 'initial' | 'ready' | 'failed';
}
type Request =
  | { kind: 'read'; field: FieldName; attempt: number; captured: EditorObservation }
  | { kind: 'mutate' }
  | { kind: 'receipt'; reference: RecoveryRef }
  | { kind: 'review'; sessionId: string }
  | { kind: 'history'; after: number }
  | { kind: 'earlier'; cursor: number; reference: RecoveryRef };
interface State {
  revision: number;
  serial: number;
  target: AgentSession | null;
  fields: Record<FieldName, Field>;
  permissions: AgentWorkspacePermissions | null;
  revokeGrants: boolean;
  pending: Record<string, Request>;
  current: {
    target: AgentSession;
    permissions: AgentPermissionChange | undefined;
    attempt: number;
    captured: Record<FieldName, EditorObservation>;
    values: Partial<Record<FieldName, string>>;
    reference: RecoveryRef | null;
  } | null;
  conflict: boolean;
  reviewed: AgentSession | null;
  discard: boolean;
  notice: keyof Copy | null;
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
  const key = `session-details-${++state.serial}`;
  state.pending[key] = r;
  actions.push({ key, binding, member: 0, method, input, resources });
}
function active(state: State, props: SessionDetailsProps) {
  return props.open && !props.busy;
}
function busy(state: State) {
  return (
    !!state.current ||
    names.some((name) => !!state.fields[name].command) ||
    Object.values(state.pending).some((r) => r.kind === 'review')
  );
}
function initial(state: State, name: FieldName) {
  return name === 'title' ? state.target!.title : state.target!.labels.join('\n');
}
function initialize(state: State) {
  if (!state.target) return;
  for (const name of names) {
    const field = state.fields[name];
    if (field.phase === 'initial' && !field.command && field.observation)
      field.command = {
        owner: field.observation.snapshot.text.owner,
        sequence: ++field.sequence,
        basedOn: field.observation.revision,
        edit: { kind: 'reset', text: initial(state, name) },
      };
  }
}
function canSave(state: State, props: SessionDetailsProps) {
  return (
    active(state, props) &&
    !busy(state) &&
    !state.conflict &&
    !state.reviewed &&
    !state.historyOpen &&
    !!state.target &&
    !state.target.archived &&
    !props.session?.archived &&
    state.target.id === props.session?.id &&
    names.every((name) => {
      const f = state.fields[name];
      return (
        f.phase === 'ready' &&
        !f.command &&
        !!f.observation &&
        !f.observation.snapshot.marked &&
        f.observation.snapshot.text.bytes <= (name === 'title' ? 512 : 4158)
      );
    })
  );
}
function save(state: State, props: SessionDetailsProps, actions: Action[]) {
  if (!canSave(state, props)) return;
  const captured = {
    title: state.fields.title.observation!,
    labels: state.fields.labels.observation!,
  };
  const attempt = ++state.serial;
  state.current = {
    target: state.target!,
    attempt,
    captured,
    values: {},
    reference: null,
    permissions:
      state.permissions &&
      (state.revokeGrants || !sameValue(state.permissions, state.target!.permissions))
        ? { settings: { ...state.permissions }, revokeGrants: state.revokeGrants }
        : undefined,
  };
  state.notice = null;
  for (const name of names) {
    const observation = captured[name];
    request(
      state,
      actions,
      { kind: 'read', field: name, attempt, captured: observation },
      'reader',
      'read',
      { start: 0, end: observation.snapshot.text.bytes },
      {
        text: {
          child: `details-${name}`,
          method: 'snapshot',
          input: {
            kind: 'at_revision',
            owner: observation.snapshot.text.owner,
            revision: observation.snapshot.text.revision,
          },
        },
      },
    );
  }
}
function mutate(state: State, props: SessionDetailsProps, actions: Action[]) {
  const current = state.current;
  if (!current || current.values.title === undefined || current.values.labels === undefined) return;
  if (
    !active(state, props) ||
    props.session?.id !== current.target.id ||
    props.session.archived ||
    current.target.archived
  ) {
    state.current = null;
    state.notice = 'detailsDraftKept';
    return;
  }
  const title = current.values.title;
  const labels = current.values.labels === '' ? [] : current.values.labels.split(/\r?\n/u);
  const input: AgentMetadata = {
    sessionId: current.target.id,
    expectedRevision: current.target.revision,
    title,
    labels,
    ...(current.permissions ? { permissions: current.permissions } : {}),
  };
  request(state, actions, { kind: 'mutate' }, 'agent', 'metadata', input);
}
function settle(state: State, reply: OperationReply, events: Emission[]) {
  const current = state.current;
  if (!current || (current.reference && !sameValue(current.reference, reply.reference))) return;
  current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'detailsUnknown';
    return;
  }
  const value = reply.receipt.outcome.value as AgentMetadataResult;
  if (value.kind === 'committed') {
    if (
      value.session.id !== current.target.id ||
      !sameValue(value.session.workspace, current.target.workspace) ||
      value.session.revision < current.target.revision
    ) {
      state.notice = 'detailsUnknown';
      return;
    }
    state.target = value.session;
    state.permissions = { ...value.session.permissions };
    state.revokeGrants = false;
    state.conflict = false;
    state.notice = 'detailsSaved';
    for (const name of names)
      state.fields[name].baseline = state.current!.captured[name].snapshot.text;
    events.push({ name: 'changed', value: { sessionId: value.session.id } });
  } else if (value.kind === 'revision_conflict') {
    state.conflict = true;
    state.notice = 'detailsConflict';
  } else
    state.notice =
      value.reason === 'title_invalid' || value.reason === 'labels_invalid'
        ? 'detailsInvalid'
        : 'detailsRejected';
  state.current = null;
}
function history(state: State, actions: Action[], after: number) {
  if (Object.values(state.pending).some((r) => r.kind === 'history' || r.kind === 'earlier'))
    return;
  if (!after) resetScroll(state, 'details-history-reading');
  state.historyFailed = false;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 4 });
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: SessionDetailsProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const locked = !active(state, props) || busy(state) || !!state.reviewed || state.historyOpen;
  const policyLocked =
    locked ||
    !state.target ||
    state.target.archived ||
    !!props.session?.archived ||
    state.target.id !== props.session?.id ||
    state.conflict;
  const choice = (key: string, label: string, selected: boolean) =>
    ui.item(key, ui.text(key + '-label', label), selected, policyLocked);
  const form = ui.box(
    'details-form',
    [
      ui.box(
        'details-fields',
        [
          ...(state.target
            ? [
                ui.text(
                  'details-workspace',
                  compactLabel(state.target.workspace.path, 66),
                  props.theme.muted,
                ),
              ]
            : []),
          ...names.map((name) => {
            const field = state.fields[name];
            const fieldProps = {
              accessibilityLabel: name === 'title' ? text.detailsName : text.detailsLabels,
              placeholder: name === 'title' ? text.detailsName : text.detailsLabelsHelp,
              autoFocus: name === 'title',
              readOnly:
                locked ||
                !!state.target?.archived ||
                !!props.session?.archived ||
                field.phase !== 'ready',
              disabled: !props.open,
              tabIndex: name === 'title' ? 1 : 2,
              command: field.command,
            };
            return ui.box(
              `details-field-${name}`,
              [
                ui.text(
                  `details-label-${name}`,
                  name === 'title' ? text.detailsName : text.detailsLabels,
                  props.theme.muted,
                ),
                ui.box(
                  `details-editor-${name}`,
                  [
                    presentation.slot(
                      `details-${name}`,
                      name === 'title' ? 'input' : 'editor',
                      name === 'title'
                        ? ({
                            menu: editorMenu(props.theme, props.locale),
                            ...fieldProps,
                            appearance: props.theme.input,
                          } satisfies InputProps)
                        : ({
                            menu: editorMenu(props.theme, props.locale),
                            ...fieldProps,
                            appearance: props.theme.textarea,
                            softWrap: true,
                          } satisfies TextareaProps),
                      { changed: name, submitted: 'save' },
                    ),
                  ],
                  { gap: 0, terminal: name === 'labels' ? { basis: 5 } : {} },
                ),
                ...(field.phase === 'failed'
                  ? [ui.button(`retry-${name}`, text.detailsRetry, locked)]
                  : []),
              ],
              { gap: 0 },
            );
          }),
          ui.text('details-help', text.detailsLabelsHelp, props.theme.muted),
          ui.text('details-permissions', text.permissionsTitle, props.theme.muted),
          ui.box(
            'details-access',
            [
              choice(
                'access-read_only',
                text.permissionReadOnly,
                state.permissions?.access === 'read_only',
              ),
              choice(
                'access-danger_full_access',
                text.permissionFullAccess,
                state.permissions?.access === 'danger_full_access',
              ),
              choice(
                'access-workspace_write',
                text.permissionWorkspaceWrite,
                state.permissions?.access === 'workspace_write',
              ),
            ],
            { row: true, gap: 1, focusGroup: true, terminal: { wrap: true } },
          ),
          ui.box(
            'details-permission-prompts',
            [
              choice(
                'permissions-ask',
                text.permissionAsk,
                state.permissions?.requestPermissions === true,
              ),
              choice(
                'permissions-no-ask',
                text.permissionNoAsk,
                state.permissions?.requestPermissions === false,
              ),
            ],
            { row: true, gap: 1, focusGroup: true, terminal: { wrap: true } },
          ),
          choice('permissions-revoke', text.permissionRevoke, state.revokeGrants),
          ui.text('details-permissions-help', text.permissionPolicyHelp, props.theme.muted),
        ],
        { gap: 1, scroll: 'anchor', terminal: { shrink: true } },
      ),
      ...(state.target?.archived || props.session?.archived
        ? [ui.text('details-archived', text.archivedConversation, props.theme.muted)]
        : []),
      ...(state.target?.id !== props.session?.id
        ? [ui.text('details-target-changed', text.detailsTargetChanged, props.theme.muted)]
        : []),
      ...(state.notice ? [ui.text('details-notice', text[state.notice], props.theme.muted)] : []),
      ui.box(
        'details-actions',
        [
          ui.button('save', text.detailsSave, !canSave(state, props), true),
          ui.button('review', text.detailsReview, locked || !state.target),
          ...(state.current?.reference
            ? [
                ui.button(
                  'check',
                  text.check,
                  Object.values(state.pending).some(
                    (r) => r.kind === 'mutate' || r.kind === 'receipt',
                  ),
                ),
              ]
            : []),
          ui.button('history', text.detailsHistory, !!state.reviewed, false, {
            background: 0,
            align: 'start',
          }),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    { gap: 1, visible: !state.reviewed && !state.historyOpen, terminal: { grow: 1 } },
  );
  const reviewed = state.reviewed;
  const review = ui.box(
    'details-review',
    [
      ui.text(
        'details-review-help',
        state.discard ? text.detailsReloadHelp : text.detailsRebaseHelp,
        props.theme.muted,
      ),
      presentation.reading(
        'details-review-content',
        {
          content: ui.box(
            'details-review-lines',
            [
              ui.text('review-title', reviewed?.title ?? ''),
              ...(reviewed?.labels ?? []).map((label, at) =>
                ui.text(`review-label-${at}`, label, props.theme.muted),
              ),
              ui.text('review-workspace', reviewed?.workspace.path ?? '', props.theme.muted),
              ...(reviewed
                ? [
                    ui.text(
                      'review-permissions',
                      reviewed.permissions.access === 'read_only'
                        ? text.permissionReadOnly
                        : reviewed.permissions.access === 'danger_full_access'
                          ? text.permissionFullAccess
                          : text.permissionWorkspaceWrite,
                    ),
                    ui.text(
                      'review-permission-prompts',
                      reviewed.permissions.requestPermissions
                        ? text.permissionAsk
                        : text.permissionNoAsk,
                    ),
                  ]
                : []),
            ],
            { scroll: 'manual', gap: 0, terminal: { grow: 1, basis: 0 } },
          ),
          command: null,
        },
        { input: 'review-input', observed: 'review-observed' },
      ),
      ui.box(
        'details-review-actions',
        [
          ui.button(
            'use-version',
            state.discard ? text.detailsReload : text.detailsUseVersion,
            !active(state, props) ||
              busy(state) ||
              (!state.discard && reviewed?.id !== state.target?.id),
          ),
          ...(!state.discard
            ? [ui.button('reload', text.detailsReload, !active(state, props) || busy(state))]
            : []),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    { visible: !!reviewed && !state.historyOpen, gap: 1, terminal: { grow: 1 } },
  );
  const earlier = state.historyOpen
    ? presentation.reading(
        'details-history-reading',
        {
          content: ui.box(
            'details-earlier',
            [
              ui.text('details-history-help', text.detailsHistoryHelp, props.theme.muted),
              ...(state.historyFailed
                ? [ui.text('details-history-failed', text.historyUnavailable, props.theme.muted)]
                : []),
              ...state.earlier.map((entry) => {
                const receipt = entry.result?.receipt;
                const value =
                  receipt?.kind === 'recorded' && receipt.outcome.kind === 'returned'
                    ? (receipt.outcome.value as AgentMetadataResult)
                    : null;
                return ui.box(
                  `details-operation-${entry.cursor}`,
                  [
                    ui.text(
                      `details-result-${entry.cursor}`,
                      value?.kind === 'committed'
                        ? text.detailsSaved + ' ' + compactLabel(value.session.title, 42)
                        : value?.kind === 'revision_conflict'
                          ? text.detailsConflict
                          : value?.kind === 'rejected'
                            ? text.detailsRejected
                            : text.historyUnknown,
                      props.theme.muted,
                    ),
                    ui.button(
                      `earlier-${entry.cursor}`,
                      text.check,
                      Object.values(state.pending).some(
                        (r) => r.kind === 'earlier' && r.cursor === entry.cursor,
                      ),
                    ),
                  ],
                  { row: true, gap: 1, terminal: { wrap: true } },
                );
              }),
              ui.box(
                'details-history-actions',
                [
                  ui.button(
                    'history-refresh',
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
      'session-details',
      [
        panelHeader(
          presentation,
          props,
          'details-header',
          state.historyOpen
            ? text.detailsHistory
            : reviewed
              ? text.detailsReview
              : text.detailsTitle,
          'close',
          state.historyOpen ? 'history-back' : reviewed ? 'cancel-review' : undefined,
        ),
        form,
        review,
        earlier,
      ],
      {
        visible: props.open,
        gap: 0,
        terminal: { grow: 1 },
        shortcuts: [
          {
            key: 'escape',
            modifiers: [],
            input: state.historyOpen ? 'history-back' : reviewed ? 'cancel-review' : 'close',
            enabled: true,
          },
        ],
      },
    ),
    events,
  );
  out.actions = actions;
  return out;
}
export function sessionDetails<N extends Node>(
  presentation: Presentation<N>,
): Component<State, SessionDetailsProps, N> {
  return {
    mount(props) {
      const field = (): Field => ({
        observation: null,
        command: null,
        sequence: 0,
        phase: 'initial',
        baseline: null,
      });
      const state: State = {
        revision: 0,
        serial: 0,
        target: props.open ? props.session : null,
        fields: { title: field(), labels: field() },
        permissions: props.open && props.session ? { ...props.session.permissions } : null,
        revokeGrants: false,
        pending: {},
        current: null,
        conflict: false,
        reviewed: null,
        discard: false,
        notice: null,
        historyOpen: false,
        earlier: [],
        historyNext: null,
        historyFailed: false,
      };
      return frame(presentation, state, props);
    },
    update(state, props) {
      state.revision++;
      if (
        props.open &&
        props.session &&
        (!state.target ||
          ((state.target.id !== props.session.id ||
            state.target.revision !== props.session.revision) &&
            !busy(state) &&
            !state.reviewed &&
            !state.revokeGrants &&
            sameValue(state.permissions, state.target.permissions) &&
            names.every(
              (name) =>
                !state.fields[name].observation ||
                sameValue(
                  state.fields[name].baseline,
                  state.fields[name].observation!.snapshot.text,
                ),
            )))
      ) {
        for (const name of names) state.fields[name].phase = 'initial';
        state.conflict = false;
        state.notice = null;
        state.target = props.session;
        state.permissions = { ...props.session.permissions };
      }
      if (props.open) initialize(state);
      return frame(presentation, state, props);
    },
    event(state, input, props) {
      if (
        input.kind === 'component' &&
        input.child === 'details-history-reading' &&
        input.name === 'input'
      )
        input = input.event as ComponentInput;
      if (
        state.historyOpen &&
        scrollRows(state, input, 'details-history-reading', state.earlier.length)
      )
        input = { kind: 'raw', key: 'history-more', event: {} };
      state.revision++;
      const actions: Action[] = [],
        events: Emission[] = [];
      if (
        input.kind === 'component' &&
        input.child.startsWith('details-') &&
        names.includes(input.child.slice(8) as FieldName)
      ) {
        const name = input.child.slice(8) as FieldName,
          field = state.fields[name];
        if (input.name === 'changed') {
          field.observation = input.event as EditorObservation;
          const receipt = field.observation.receipt;
          if (
            field.command &&
            receipt?.owner === field.command.owner &&
            receipt.sequence === field.command.sequence
          ) {
            field.phase = receipt.outcome.status === 'applied' ? 'ready' : 'failed';
            if (field.phase === 'ready') field.baseline = field.observation.snapshot.text;
            field.command = null;
          }
          if (active(state, props)) initialize(state);
        } else if (input.name === 'submitted') save(state, props, actions);
      } else if (input.kind === 'action') {
        const r = state.pending[input.key];
        if (!r) return frame(presentation, state, props);
        delete state.pending[input.key];
        const result = input.event as ActionOutcome;
        if (r.kind === 'read' && state.current?.attempt === r.attempt) {
          const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
          if (
            !chunk ||
            !sameValue(chunk.metadata, r.captured.snapshot.text) ||
            new TextEncoder().encode(chunk.text).length !== r.captured.snapshot.text.bytes
          ) {
            state.current = null;
            state.notice = 'detailsDraftKept';
          } else {
            state.current.values[r.field] = chunk.text;
            mutate(state, props, actions);
          }
        } else if (r.kind === 'mutate') {
          if (result.status === 'operation') settle(state, result.reply, events);
          else if (result.status === 'not_accepted') {
            state.current = null;
            state.notice = 'detailsRejected';
          } else state.notice = 'detailsUnknown';
        } else if (r.kind === 'receipt') {
          if (
            result.status === 'succeeded' &&
            sameValue((result.value as OperationReply).reference, r.reference)
          )
            settle(state, result.value as OperationReply, events);
          else state.notice = 'detailsUnknown';
        } else if (r.kind === 'review') {
          const value = result.status === 'succeeded' ? (result.value as AgentObservation) : null;
          if (value?.session.id === r.sessionId) {
            state.reviewed = value.session;
            state.discard = false;
          } else state.notice = 'detailsReviewFailed';
        } else if (r.kind === 'history') {
          if (result.status !== 'succeeded') state.historyFailed = true;
          else {
            const page = result.value as CallerIntent[];
            state.historyNext = page.at(-1)?.cursor ?? null;
            state.earlier = [
              ...(r.after ? state.earlier : []),
              ...page
                .filter(
                  (entry) =>
                    entry.reference.method === 'metadata' &&
                    entry.reference.contract.name === 'maka.agent.Agent',
                )
                .map((entry) => ({
                  cursor: entry.cursor,
                  reference: entry.reference,
                  result: null,
                })),
            ];
          }
        } else if (r.kind === 'earlier') {
          const entry = state.earlier.find(
            (entry) => entry.cursor === r.cursor && sameValue(entry.reference, r.reference),
          );
          if (
            entry &&
            result.status === 'succeeded' &&
            sameValue((result.value as OperationReply).reference, r.reference)
          )
            entry.result = result.value as OperationReply;
        }
      } else if (input.kind === 'raw') {
        if (input.key === 'close' && props.open) events.push({ name: 'closed', value: {} });
        else if (
          [
            'access-read_only',
            'access-workspace_write',
            'access-danger_full_access',
            'permissions-ask',
            'permissions-no-ask',
            'permissions-revoke',
          ].includes(input.key)
        ) {
          if (
            active(state, props) &&
            !busy(state) &&
            !state.reviewed &&
            !state.historyOpen &&
            !state.conflict &&
            state.target &&
            !state.target.archived &&
            !props.session?.archived &&
            state.target.id === props.session?.id &&
            state.permissions
          ) {
            if (input.key === 'permissions-revoke') state.revokeGrants = !state.revokeGrants;
            else if (input.key.startsWith('access-'))
              state.permissions.access =
                input.key === 'access-read_only'
                  ? 'read_only'
                  : input.key === 'access-danger_full_access'
                    ? 'danger_full_access'
                    : 'workspace_write';
            else state.permissions.requestPermissions = input.key === 'permissions-ask';
          }
        } else if (input.key === 'save') save(state, props, actions);
        else if (
          input.key === 'check' &&
          state.current?.reference &&
          !Object.values(state.pending).some((r) => r.kind === 'receipt' || r.kind === 'mutate')
        )
          request(
            state,
            actions,
            { kind: 'receipt', reference: state.current.reference },
            'receipts',
            'read',
            state.current.reference,
          );
        else if (input.key === 'review' && active(state, props) && !busy(state) && state.target)
          request(
            state,
            actions,
            { kind: 'review', sessionId: props.session?.id ?? state.target.id },
            'agent',
            'observe',
            { sessionId: props.session?.id ?? state.target.id },
          );
        else if (
          input.key === 'use-version' &&
          active(state, props) &&
          !busy(state) &&
          state.reviewed &&
          (state.discard || state.reviewed.id === state.target?.id)
        ) {
          state.target = state.reviewed;
          state.reviewed = null;
          state.conflict = false;
          state.notice = null;
          if (state.discard) {
            state.permissions = { ...state.target.permissions };
            state.revokeGrants = false;
            for (const name of names) {
              state.fields[name].phase = 'initial';
            }
            initialize(state);
          }
          state.discard = false;
        } else if (input.key === 'reload' && active(state, props) && state.reviewed)
          state.discard = true;
        else if (input.key === 'cancel-review') {
          state.reviewed = null;
          state.discard = false;
        } else if (input.key.startsWith('retry-') && active(state, props) && !busy(state)) {
          const name = input.key.slice(6) as FieldName;
          if (names.includes(name) && state.fields[name].phase === 'failed') {
            state.fields[name].phase = 'initial';
            initialize(state);
          }
        } else if (input.key === 'history' && active(state, props)) {
          state.historyOpen = true;
          history(state, actions, 0);
        } else if (input.key === 'history-back') state.historyOpen = false;
        else if (input.key === 'history-refresh' && state.historyOpen) history(state, actions, 0);
        else if (input.key === 'history-more' && state.historyOpen && state.historyNext !== null)
          history(state, actions, state.historyNext);
        else if (input.key.startsWith('earlier-') && state.historyOpen) {
          const entry = state.earlier.find((entry) => input.key === `earlier-${entry.cursor}`);
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
        }
      }
      return frame(presentation, state, props, actions, events);
    },
  };
}
export function activate() {
  return {
    SessionDetails: {
      kind: 'component',
      contract: sessionDetailsContract,
      component: ['component'],
    },
  };
}
