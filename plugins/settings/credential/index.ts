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

import { tuiText, copy, type Copy } from './locale.js';
import type {
  Action,
  ActionOutcome,
  Component,
  ComponentInput,
  Emission,
  CallerIntent,
  RecoveryRef,
  OperationReply,
  CredentialStatus,
  CredentialVersionBasis,
  CredentialVaultQueryResult,
  ConnectionCredentialTarget,
  SettingsCredentialSaveResult,
  SettingsProvider,
  SettingsLoginPrompt,
  SettingsCredentialLoginResult,
  SettingsConnectionDetail,
  ConnectionVersionBasis,
  CredentialMutationResult,
  EditorObservation,
  EditorCommand,
} from '../../../packages/plugin-sdk/src/index.js';
import { credentialContract, type CredentialProps } from '../../shared/contracts.js';
import { pageHeader, withPageShell, type PageState } from '../../shared/page-shell.js';
import { scrollBoundary, scrollRows, visibleRows, resetScroll } from '../../shared/scroll.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Presentation, type Node } from '../../shared/view.js';

type Request =
  | { kind: 'status' }
  | { kind: 'setupTarget'; basis: ConnectionVersionBasis; retried: boolean }
  | { kind: 'provider' }
  | { kind: 'reviewConnection'; basis: ConnectionVersionBasis; retried: boolean }
  | { kind: 'reviewStatus'; target: ConnectionCredentialTarget }
  | { kind: 'mutate' }
  | { kind: 'receipt'; reference: RecoveryRef }
  | { kind: 'history'; after: number }
  | { kind: 'earlier'; reference: RecoveryRef; cursor: number };
interface Current {
  kind: 'save' | 'delete' | 'login' | 'discover';
  captured: EditorObservation | null;
  reference: RecoveryRef | null;
}
interface Earlier {
  cursor: number;
  reference: RecoveryRef;
  result: OperationReply | null;
}
interface State extends PageState {
  revision: number;
  serial: number;
  target: ConnectionCredentialTarget;
  openedTarget: ConnectionCredentialTarget;
  reviewed: { target: ConnectionCredentialTarget; status: CredentialStatus } | null;
  status: CredentialStatus | null;
  statusFailed: boolean;
  provider: SettingsProvider | null;
  providerFailed: boolean;
  loginPrompt: SettingsLoginPrompt | null;
  loginMessage: string;
  cancelling: boolean;
  observation: EditorObservation | null;
  command: EditorCommand | null;
  commandSequence: number;
  reviewPage: number;
  pending: Record<string, Request>;
  current: Current | null;
  finished: boolean;
  setup: 'idle' | 'loading' | 'ready' | 'failed' | 'conflict';
  advanced: boolean;
  notice: keyof Copy | null;
  confirmation: 'leave' | 'remove' | 'rebase' | null;
  historyOpen: boolean;
  historyFailed: boolean;
  earlier: Earlier[];
  historyAfter: number;
  historyNext: number | null;
}
function request(
  state: State,
  actions: Action[],
  request: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  const key = 'credential-' + ++state.serial;
  state.pending[key] = request;
  actions.push({ key, binding, member: 0, method, input, resources });
}
function locator(state: State) {
  return {
    scope: 'connection' as const,
    connectionId: state.target.connectionId,
    kind: 'provider' as const,
  };
}
function basis(status: CredentialStatus | null): CredentialVersionBasis | null {
  return status?.configured && status.credentialId !== null && status.revision !== null
    ? { locator: status.locator, credentialId: status.credentialId, revision: status.revision }
    : null;
}
function busy(state: State) {
  return (
    !!state.current ||
    !!state.command ||
    Object.values(state.pending).some((r) =>
      ['status', 'provider', 'reviewConnection', 'reviewStatus', 'setupTarget'].includes(r.kind),
    )
  );
}
function fresh(state: State, props: CredentialProps) {
  return sameValue(state.openedTarget, props.target) && !state.finished;
}
function canSave(state: State, props: CredentialProps) {
  return (
    !busy(state) &&
    !state.historyOpen &&
    !state.confirmation &&
    fresh(state, props) &&
    !!state.status &&
    !!state.provider?.descriptor.authentication.some(
      (method) => method.id === 'api-key' && !method.interactive,
    ) &&
    !!state.observation?.snapshot.text.bytes &&
    !state.observation.snapshot.marked
  );
}
function readStatus(state: State, actions: Action[]) {
  if (busy(state) || state.finished || state.confirmation) return;
  // An observation used by an in-flight mutation is never overwritten by a late query.
  state.status = null;
  state.statusFailed = false;
  request(state, actions, { kind: 'status' }, 'settings', 'credentialStatus', {
    locator: locator(state),
  });
}
function review(state: State, props: CredentialProps, actions: Action[]) {
  if (busy(state) || state.confirmation || props.target.connectionId !== state.target.connectionId)
    return;
  state.reviewed = null;
  const original = { connectionId: state.target.connectionId, revision: state.target.revision };
  request(
    state,
    actions,
    { kind: 'reviewConnection', basis: original, retried: false },
    'settings',
    'connection',
    original,
  );
}

function save(state: State, props: CredentialProps, actions: Action[]) {
  if (!canSave(state, props)) return;
  const captured = state.observation!;
  state.setup = 'idle';
  state.current = { kind: 'save', captured, reference: null };
  state.notice = null;
  request(
    state,
    actions,
    { kind: 'mutate' },
    'settings',
    'saveCredential',
    { expected: state.target, expectedCredential: basis(state.status) },
    {
      secret: {
        child: 'secret',
        method: 'snapshot',
        input: {
          kind: 'at_revision',
          owner: captured.snapshot.text.owner,
          revision: captured.revision,
        },
      },
    },
  );
}
function login(state: State, props: CredentialProps, method: string, actions: Action[]) {
  if (
    busy(state) ||
    !fresh(state, props) ||
    state.confirmation ||
    state.historyOpen ||
    !state.status ||
    !state.provider?.descriptor.authentication.some((m) => m.id === method && m.interactive)
  )
    return;
  state.setup = 'idle';
  state.current = { kind: 'login', captured: null, reference: null };
  state.loginPrompt = null;
  state.loginMessage = '';
  state.cancelling = false;
  state.notice = 'loginStarting';
  request(state, actions, { kind: 'mutate' }, 'authentication', 'loginCredential', {
    expected: state.target,
    expectedCredential: basis(state.status),
    method,
  });
}
function setup(state: State, actions: Action[]) {
  if (busy(state) || state.confirmation) return;
  state.setup = 'loading';
  state.notice = 'loadingConnectionModels';
  const basis = { connectionId: state.target.connectionId, revision: state.target.revision };
  request(
    state,
    actions,
    { kind: 'setupTarget', basis, retried: false },
    'settings',
    'connection',
    basis,
  );
}
function known(state: State, value: unknown, events: Emission[]) {
  const current = state.current!;
  if (current.kind === 'discover') {
    const result = value as
      | { kind: 'ready'; connection: ConnectionVersionBasis }
      | { kind: 'failed' | 'superseded' };
    if (
      result.kind === 'ready' &&
      'connection' in result &&
      result.connection.connectionId === state.target.connectionId
    ) {
      state.setup = 'ready';
      state.notice = 'connectionReady';
      state.finished = true;
      events.push({ name: 'changed', value: {} });
    } else if (result.kind === 'failed' || result.kind === 'superseded') {
      state.setup = result.kind === 'failed' ? 'failed' : 'conflict';
      state.notice = result.kind === 'failed' ? 'connectionModelsFailed' : 'keyConflict';
    } else return false;
  } else if (current.kind === 'save' || current.kind === 'login') {
    const result = value as SettingsCredentialLoginResult;
    if (result.kind === 'failed' && current.kind === 'login') {
      state.notice = {
        cancelled: 'loginCancelled',
        expired: 'loginExpired',
        transport: 'loginTransportFailed',
        authentication: 'loginFailed',
      }[result.reason] as keyof Copy;
      state.loginMessage = result.message;
      state.loginPrompt = null;
      state.current = null;
      state.cancelling = false;
      return true;
    }
    if (result.kind === 'saved') {
      if (
        result.connection.connectionId !== state.target.connectionId ||
        result.connection.slug !== state.target.slug ||
        !sameValue(result.connection.provider, state.target.provider)
      )
        return false;
      state.status = state.status ? { ...state.status, configured: true } : null;
      state.finished = true; // Saving changes connection revision; never guess its next basis.
      state.notice = current.kind === 'login' ? 'loginSaved' : 'keySaved';
      state.loginPrompt = null;
      const captured = current.captured;
      if (captured)
        state.command = {
          owner: captured.snapshot.text.owner,
          sequence: ++state.commandSequence,
          basedOn: captured.revision,
          edit: { kind: 'reset', text: '' },
        };
      events.push({ name: 'changed', value: {} });
    } else if (result.kind === 'rejected') {
      state.notice = ['connection_changed', 'credential_changed', 'attempt_conflict'].includes(
        result.reason,
      )
        ? 'keyConflict'
        : 'keyRejected';
      if (state.notice === 'keyConflict') state.finished = true;
    } else return false;
  } else {
    const result = value as CredentialMutationResult;
    if (result.kind === 'committed') {
      if (!sameValue(result.status.locator, locator(state))) return false;
      state.status = result.status;
      state.notice = 'keyRemoved';
      events.push({ name: 'changed', value: {} });
    } else if (result.kind === 'rejected') state.notice = 'keyRejected';
    else if (
      ['connection_not_found', 'connection_stale', 'credential_stale'].includes(result.kind)
    ) {
      state.finished = true;
      state.notice = 'keyConflict';
    } else return false;
  }
  state.current = null;
  state.loginPrompt = null;
  state.cancelling = false;
  return true;
}
function receipt(state: State, reply: OperationReply, events: Emission[]) {
  const current = state.current;
  if (!current) return;
  if (current.reference && !sameValue(current.reference, reply.reference)) {
    state.notice = 'keyUnknown';
    return;
  }
  current.reference = reply.reference;
  if (
    reply.receipt.kind !== 'recorded' ||
    reply.receipt.outcome.kind !== 'returned' ||
    !known(state, reply.receipt.outcome.value, events)
  )
    state.notice = 'keyUnknown';
}
function history(state: State, actions: Action[], after: number) {
  if (Object.values(state.pending).some((r) => r.kind === 'history' || r.kind === 'earlier'))
    return;
  if (!after) resetScroll(state, 'credential-history-reading');
  state.historyFailed = false;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 4 });
}
function earlierLabel(entry: Earlier, text: Copy): string {
  const receipt = entry.result?.receipt;
  if (receipt?.kind !== 'recorded' || receipt.outcome.kind !== 'returned')
    return text.historyUnknown;
  const result = receipt.outcome.value as SettingsCredentialSaveResult | CredentialMutationResult;
  if (
    ['saveCredential', 'loginCredential'].includes(entry.reference.method) &&
    result.kind === 'saved'
  )
    return text.historySaved + ' ' + result.connection.slug;
  if (
    entry.reference.method === 'deleteCredential' &&
    result.kind === 'committed' &&
    result.status.locator.scope === 'connection'
  )
    return text.historyRemoved + ' ' + result.status.locator.connectionId;
  return text.historyRejected;
}

function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: CredentialProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const apiKey = !!state.provider?.descriptor.authentication.some(
    (method) => method.id === 'api-key' && !method.interactive,
  );
  const pendingStatus = Object.values(state.pending).some((r) => r.kind === 'status');
  const status =
    state.notice === 'loginSaved' || state.notice === 'keySaved'
      ? text.keyConfigured
      : state.statusFailed
        ? text.keyUnavailable
        : pendingStatus || !state.status
          ? text.keyLoading
          : state.status.configured
            ? text.keyConfigured
            : state.provider?.descriptor.anonymous
              ? tuiText(props.locale, 'anonymous-credential')
              : text.keyAbsent;
  const canReview =
    !busy(state) && !state.confirmation && props.target.connectionId === state.target.connectionId;
  const form = ui.box(
    'credential-form',
    [
      ...pageHeader(
        presentation,
        state,
        props,
        events,
        state.provider?.descriptor.anonymous ? text.connectionProvider : text.signInConnection,
      ),
      ui.text(
        'connection',
        compactLabel(state.target.provider.name + ' · ' + state.target.slug, 60),
        props.theme.muted,
      ),
      ui.text('status', status, props.theme.muted),
      ...(state.statusFailed ? [ui.button('refresh-status', text.keyRefresh, busy(state))] : []),
      ...(!sameValue(state.openedTarget, props.target)
        ? [ui.text('target-changed', text.connectionChanged, props.theme.muted)]
        : []),
      ui.box(
        'key-input',
        [
          presentation.slot(
            'secret',
            'secret',
            {
              appearance: props.theme.input,
              accessibilityLabel: text.credentials,
              readOnly: false,
              disabled: false,
              tabIndex: 0,
              autoFocus: true,
              placeholder: text.keyPlaceholder,
              command: state.command,
            },
            { changed: 'changed', submitted: 'submitted' },
          ),
        ],
        { visible: apiKey && !state.finished },
      ),
      ...(state.providerFailed
        ? [
            ui.text('provider-error', text.providersUnavailable),
            ui.button('refresh-provider', text.refreshConnections, busy(state)),
          ]
        : []),
      ...(state.loginPrompt
        ? [
            ui.box(
              'login-prompt',
              [
                ui.text('login-help', text.loginInstructions),
                ui.box(
                  'login-link-row',
                  [
                    ui.text('login-address', state.loginPrompt.url),
                    ui.button(
                      'login-url',
                      text.copyLoginLink,
                      false,
                      false,
                      {},
                      state.loginPrompt.url,
                    ),
                  ],
                  { row: true, gap: 1, terminal: { wrap: true } },
                ),
                ui.box(
                  'login-code-row',
                  [
                    ui.text('login-device-code', state.loginPrompt.userCode),
                    ui.button(
                      'login-code',
                      text.copyLoginCode,
                      false,
                      true,
                      {},
                      state.loginPrompt.userCode,
                    ),
                  ],
                  { row: true, gap: 1, terminal: { wrap: true } },
                ),
              ],
              { gap: 1 },
            ),
          ]
        : []),
      ui.box(
        'actions',
        [
          ...(apiKey && !state.finished
            ? [ui.button('save', text.saveKey, !canSave(state, props), true)]
            : []),
          ...(state.setup === 'ready' ? [ui.button('done', text.setupDone, false, true)] : []),
          ...(state.setup === 'failed' ||
          ((state.status?.configured || state.provider?.descriptor.anonymous) &&
            state.setup === 'idle')
            ? [ui.button('setup-models', text.loadConnectionModels, busy(state), true)]
            : []),
          ...(state.provider?.descriptor.authentication
            .filter(
              (method) => method.interactive && !state.finished && state.current?.kind !== 'login',
            )
            .map((method) =>
              ui.button(
                'login-' + method.id,
                method.label,
                busy(state) || !fresh(state, props) || !state.status || !!state.confirmation,
                true,
              ),
            ) ?? []),
          ...(state.current?.kind === 'login' &&
          Object.values(state.pending).some((r) => r.kind === 'mutate')
            ? [
                ui.button(
                  'login-cancel',
                  state.cancelling ? text.loginCancelling : text.cancel,
                  state.cancelling,
                ),
              ]
            : []),
          ...(state.advanced && basis(state.status)
            ? [
                ui.button(
                  'remove',
                  text.removeKey,
                  busy(state) || !fresh(state, props) || !!state.confirmation,
                ),
              ]
            : []),
          ...(state.current?.reference
            ? [
                ui.button(
                  'check',
                  text.check,
                  Object.values(state.pending).some((r) => r.kind === 'receipt'),
                ),
              ]
            : []),
          ...(state.setup === 'conflict' ||
          (state.finished && state.setup === 'idle') ||
          !sameValue(state.openedTarget, props.target)
            ? [ui.button('review', text.reviewConnection, !canReview)]
            : []),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
      ...(state.notice ? [ui.text('notice', text[state.notice], props.theme.muted)] : []),
      ...(state.loginMessage
        ? [ui.text('login-error-detail', state.loginMessage, props.theme.muted)]
        : []),
      ...(state.confirmation && state.confirmation !== 'rebase'
        ? [
            ui.text(
              'confirmation',
              state.confirmation === 'leave'
                ? state.current
                  ? text.leaveUnknown
                  : text.discardKey
                : state.confirmation === 'remove'
                  ? text.confirmRemoveKey
                  : text.reviewReady,
              props.theme.text,
            ),
            ui.box(
              'confirmation-actions',
              [
                ui.button(
                  'confirm',
                  state.confirmation === 'leave'
                    ? state.current
                      ? text.leave
                      : text.discard
                    : state.confirmation === 'remove'
                      ? text.confirmRemove
                      : text.useConnection,
                ),
                ui.button(
                  'cancel',
                  state.confirmation === 'leave' ? text.keepEditing : text.cancel,
                ),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ]
        : []),
      ...(!state.current
        ? [ui.button('advanced', state.advanced ? text.basicSettings : text.advancedSettings)]
        : []),
      ...(state.advanced
        ? [
            ui.button('history', text.earlierOperations, false, false, {
              background: 0,
              align: 'start',
            }),
          ]
        : []),
    ],
    {
      visible: !state.historyOpen && state.confirmation !== 'rebase',
      scroll: 'anchor',
      terminal: { grow: 1 },
      gap: 1,
    },
  );
  // History has its own bounded pane. The hidden native input stays mounted;
  // reading another operation cannot adopt its basis or clear this draft.
  const earlier = state.historyOpen
    ? presentation.reading(
        'credential-history-reading',
        {
          content: ui.box(
            'earlier-operations',
            [
              ui.button('history-back', text.back, false, false, { background: 0, align: 'start' }),
              ui.text('history-heading', text.earlierOperations, props.theme.text, 550),
              ui.text('history-help', text.earlierHelp, props.theme.muted),
              ui.box(
                'history-actions',
                [
                  ui.button(
                    'history-refresh',
                    props.theme.ascii ? 'r' : '↻',
                    Object.values(state.pending).some(
                      (r) => r.kind === 'history' || r.kind === 'earlier',
                    ),
                    false,
                    { background: 0 },
                    undefined,
                    text.keyRefresh,
                  ),
                ],
                { row: true, gap: 1, terminal: { wrap: true } },
              ),
              ...(state.historyFailed
                ? [ui.text('history-error', text.historyUnavailable, props.theme.muted)]
                : []),
              ...visibleRows(state, 'credential-history-reading', state.earlier).flatMap(
                ({ entry, at }) => [
                  ui.text(
                    'earlier-label-' + at,
                    compactLabel(earlierLabel(entry, text), 60),
                    props.theme.muted,
                  ),
                  ui.button(
                    'earlier-' + at,
                    text.check,
                    Object.values(state.pending).some(
                      (r) => r.kind === 'earlier' && r.cursor === entry.cursor,
                    ),
                  ),
                ],
              ),
            ],
            { visible: state.historyOpen, scroll: 'anchor', terminal: { grow: 1 }, gap: 1 },
          ),
        },
        { input: 'history-input', observed: 'history-observed' },
      )
    : ui.box('history-hidden', [], { visible: false });

  const reviewed = state.reviewed?.target;
  const details = reviewed
    ? [
        ...(typeof reviewed.configuration.baseUrl === 'string'
          ? [reviewed.configuration.baseUrl]
          : []),
        JSON.stringify(reviewed.provider),
        reviewed.slug,
        JSON.stringify(reviewed.configuration),
      ].flatMap((value) => {
        const characters = [...value];
        const rows: string[] = [];
        for (let at = 0; at < characters.length; at += 36)
          rows.push(characters.slice(at, at + 36).join(''));
        return rows;
      })
    : [];
  const pageSize = 8;
  const pages = Math.max(1, Math.ceil(details.length / pageSize));
  const page = Math.min(state.reviewPage, pages - 1);
  const reviewPane = ui.box(
    'review-target',
    [
      ui.text('review-heading', text.reviewConnection, props.theme.text, 550),
      ui.text('review-question', text.reviewReady, props.theme.text),
      ui.box(
        'review-actions',
        [
          ui.button('review-confirm', text.useConnection, !state.reviewed || busy(state)),
          ui.button('review-cancel', text.cancel),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
      presentation.reading(
        'credential-review-reading',
        {
          content: ui.box(
            'review-lines',
            details
              .slice(page * pageSize, (page + 1) * pageSize)
              .map((line, at) => ui.text('target-line-' + at, line, props.theme.text)),
            { scroll: 'manual', terminal: { grow: 1, basis: 0 } },
          ),
        },
        { observed: 'review-observed' },
      ),
    ],
    { visible: state.confirmation === 'rebase', gap: 1 },
  );
  const view = ui.box(
    'credential',
    [
      ui.dialog(
        'credential-auxiliary',
        state.historyOpen ? text.earlierOperations : text.reviewConnection,
        form,
        ui.box('credential-auxiliary-content', [earlier, reviewPane], { gap: 0 }),
        state.historyOpen || state.confirmation === 'rebase',
        { width: 72, height: 24 },
      ),
    ],
    {
      shortcuts: [
        {
          key: 'escape',
          modifiers: [],
          input: state.confirmation ? 'cancel' : state.historyOpen ? 'history-back' : 'back',
          enabled: true,
        },
      ],
      gap: 0,
      padding: props.shell ? 0 : 1,
      scroll: 'none',
      terminal: { grow: 1 },
    },
  );
  const out = output(state, view, events);
  out.actions = actions;
  return out;
}

export function credential<N extends Node>(
  presentation: Presentation<N>,
): Component<State, CredentialProps, N> {
  return withPageShell<State, CredentialProps, N>(
    {
      mount(props) {
        const state: State = {
          revision: 0,
          serial: 0,
          target: props.target,
          openedTarget: props.target,
          reviewed: null,
          status: null,
          statusFailed: false,
          provider: null,
          providerFailed: false,
          loginPrompt: null,
          loginMessage: '',
          cancelling: false,
          observation: null,
          command: null,
          commandSequence: 0,
          reviewPage: 0,
          pending: {},
          current: null,
          finished: false,
          setup: 'idle',
          advanced: false,
          notice: null,
          confirmation: null,
          historyOpen: false,
          historyFailed: false,
          earlier: [],
          historyAfter: 0,
          historyNext: null,
        };
        const actions: Action[] = [];
        readStatus(state, actions);
        request(
          state,
          actions,
          { kind: 'provider' },
          'authentication',
          'provider',
          state.target.provider,
        );
        return frame(presentation, state, props, actions);
      },
      update(state, props) {
        state.revision++;
        return frame(presentation, state, props);
      },
      event(state, input, props) {
        const reviewScroll = scrollBoundary(state, input, 'credential-review-reading');
        if (reviewScroll)
          input = {
            kind: 'raw',
            key: reviewScroll === 'up' ? 'review-previous' : 'review-next',
            event: {},
          };
        if (
          input.kind === 'component' &&
          input.child === 'credential-history-reading' &&
          input.name === 'input'
        )
          input = input.event as ComponentInput;
        if (
          state.historyOpen &&
          scrollRows(state, input, 'credential-history-reading', state.earlier.length)
        )
          input = { kind: 'raw', key: 'history-more', event: {} };
        if (input.kind === 'raw' && input.key === 'credential-auxiliary')
          input = { ...input, key: state.historyOpen ? 'history-back' : 'review-cancel' };
        state.revision++;
        const actions: Action[] = [],
          events: Emission[] = [],
          cancel: string[] = [];
        if (input.kind === 'action') {
          const r = state.pending[input.key];
          if (!r) return frame(presentation, state, props);
          const result = input.event as ActionOutcome;
          if (
            r.kind === 'mutate' &&
            state.current?.kind === 'login' &&
            result.status === 'updated'
          ) {
            state.loginPrompt = result.value as SettingsLoginPrompt;
            state.notice = 'loginWaiting';
            return frame(presentation, state, props);
          }
          delete state.pending[input.key];
          if (r.kind === 'mutate') {
            if (result.status === 'operation') receipt(state, result.reply, events);
            else if (result.status === 'not_accepted') {
              const discovering = state.current?.kind === 'discover';
              state.current = null;
              if (discovering) state.setup = 'failed';
              state.notice = discovering ? 'connectionModelsFailed' : 'keyRejected';
            } else state.notice = 'keyUnknown';
          } else if (r.kind === 'receipt') {
            if (
              result.status === 'succeeded' &&
              sameValue((result.value as OperationReply).reference, r.reference)
            )
              receipt(state, result.value as OperationReply, events);
            else state.notice = 'keyUnknown';
          } else if (r.kind === 'setupTarget') {
            const detail =
              result.status === 'succeeded' ? (result.value as SettingsConnectionDetail) : null;
            if (
              detail?.kind === 'stale' &&
              detail.actual?.connectionId === state.target.connectionId &&
              !r.retried
            ) {
              request(
                state,
                actions,
                { kind: 'setupTarget', basis: detail.actual, retried: true },
                'settings',
                'connection',
                detail.actual,
              );
            } else if (
              detail?.kind === 'ready' &&
              detail.target.connectionId === r.basis.connectionId &&
              detail.target.revision === r.basis.revision &&
              sameValue(detail.target.provider, state.target.provider) &&
              sameValue(detail.target.configuration, state.target.configuration)
            ) {
              state.target = detail.target;
              state.current = { kind: 'discover', captured: null, reference: null };
              request(state, actions, { kind: 'mutate' }, 'setup', 'discoverModels', detail.target);
            } else {
              state.setup = detail ? 'conflict' : 'failed';
              state.notice = detail ? 'keyConflict' : 'connectionModelsFailed';
            }
          } else if (r.kind === 'reviewConnection') {
            const detail =
              result.status === 'succeeded' ? (result.value as SettingsConnectionDetail) : null;
            if (
              detail?.kind === 'stale' &&
              detail.actual &&
              detail.actual.connectionId === state.target.connectionId &&
              !r.retried
            )
              request(
                state,
                actions,
                { kind: 'reviewConnection', basis: detail.actual, retried: true },
                'settings',
                'connection',
                detail.actual,
              );
            else if (
              detail?.kind === 'ready' &&
              detail.target.connectionId === r.basis.connectionId &&
              detail.target.revision === r.basis.revision
            )
              request(
                state,
                actions,
                { kind: 'reviewStatus', target: detail.target },
                'settings',
                'credentialStatus',
                { locator: locator(state) },
              );
            else state.notice = 'keyConflict';
          } else if (r.kind === 'reviewStatus') {
            const value =
              result.status === 'succeeded' ? (result.value as CredentialVaultQueryResult) : null;
            if (value?.kind === 'status' && sameValue(value.status.locator, locator(state))) {
              state.reviewed = { target: r.target, status: value.status };
              state.reviewPage = 0;
              state.confirmation = 'rebase';
            } else state.notice = 'keyUnavailable';
          } else if (r.kind === 'provider') {
            const provider =
              result.status === 'succeeded' ? (result.value as SettingsProvider) : null;
            if (provider && sameValue(provider.identity, state.target.provider)) {
              state.provider = provider;
              state.providerFailed = false;
            } else state.providerFailed = true;
          } else if (r.kind === 'status') {
            const value =
              result.status === 'succeeded' ? (result.value as CredentialVaultQueryResult) : null;
            if (value?.kind === 'status' && sameValue(value.status.locator, locator(state)))
              state.status = value.status;
            else state.statusFailed = true;
          } else if (r.kind === 'history') {
            if (result.status !== 'succeeded') state.historyFailed = true;
            else {
              const page = result.value as CallerIntent[];
              state.historyAfter = r.after;
              state.historyNext = page.at(-1)?.cursor ?? null;
              state.earlier = [
                ...(r.after ? state.earlier : []),
                ...page
                  .filter(
                    (entry) =>
                      ['maka.settings.Settings', 'maka.settings.Authentication'].includes(
                        entry.reference.contract.name,
                      ) &&
                      ['saveCredential', 'deleteCredential', 'loginCredential'].includes(
                        entry.reference.method,
                      ),
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
            // Earlier operation facts never become current intent/target/status or reset commands.
          }
        } else if (input.kind === 'component' && input.child === 'secret') {
          if (input.name === 'changed') {
            state.observation = input.event as EditorObservation;
            const r = state.observation.receipt;
            if (
              state.command &&
              r?.owner === state.command.owner &&
              r.sequence === state.command.sequence
            ) {
              if (r.outcome.status === 'conflict') state.notice = 'draftKept';
              state.command = null;
            }
          } else if (input.name === 'submitted') save(state, props, actions);
        } else if (input.kind === 'raw') {
          if (
            input.key === 'login-cancel' &&
            state.current?.kind === 'login' &&
            !state.cancelling
          ) {
            cancel.push(
              ...Object.entries(state.pending)
                .filter(([, r]) => r.kind === 'mutate')
                .map(([key]) => key),
            );
            state.cancelling = true;
            state.notice = 'loginCancelling';
          } else if (input.key.startsWith('login-'))
            login(state, props, input.key.slice(6), actions);
          else if (input.key === 'refresh-provider' && !busy(state))
            request(
              state,
              actions,
              { kind: 'provider' },
              'authentication',
              'provider',
              state.target.provider,
            );
          else if (input.key === 'setup-models') setup(state, actions);
          else if (input.key === 'done' && state.setup === 'ready')
            events.push({ name: 'completed', value: {} });
          else if (input.key === 'advanced') state.advanced = !state.advanced;
          else if (input.key === 'save') save(state, props, actions);
          else if (input.key === 'refresh-status') readStatus(state, actions);
          else if (input.key === 'review') review(state, props, actions);
          else if (input.key === 'review-next' && state.confirmation === 'rebase')
            state.reviewPage++;
          else if (input.key === 'review-previous' && state.confirmation === 'rebase')
            state.reviewPage = Math.max(0, state.reviewPage - 1);
          else if (input.key === 'back') {
            if (
              state.observation?.snapshot.text.bytes ||
              state.observation?.snapshot.marked ||
              state.command ||
              state.current
            )
              state.confirmation = 'leave';
            else events.push({ name: 'closed', value: {} });
          } else if (input.key === 'cancel' || input.key === 'review-cancel') {
            state.confirmation = null;
            state.reviewed = null;
          } else if (
            input.key === 'remove' &&
            !busy(state) &&
            fresh(state, props) &&
            basis(state.status)
          )
            state.confirmation = 'remove';
          else if (input.key === 'review-confirm' && state.confirmation === 'rebase') {
            if (
              state.reviewed &&
              !busy(state) &&
              props.target.connectionId === state.target.connectionId
            ) {
              state.target = state.reviewed.target;
              state.openedTarget = props.target;
              state.status = state.reviewed.status;
              state.finished = false;
              state.setup = 'idle';
              state.notice = null;
            }
            state.reviewed = null;
            state.confirmation = null;
          } else if (input.key === 'confirm' && state.confirmation === 'leave')
            events.push({ name: 'closed', value: {} });
          else if (input.key === 'confirm' && state.confirmation === 'remove') {
            state.confirmation = null;
            const expected = basis(state.status);
            if (expected && !busy(state) && fresh(state, props)) {
              state.current = { kind: 'delete', captured: null, reference: null };
              state.notice = null;
              request(state, actions, { kind: 'mutate' }, 'settings', 'deleteCredential', {
                expected,
                expectedConnection: state.target,
              });
            }
          } else if (
            input.key === 'check' &&
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
          else if (input.key === 'history') {
            state.historyOpen = !state.historyOpen;
            if (state.historyOpen) history(state, actions, state.historyAfter);
          } else if (input.key === 'history-back') state.historyOpen = false;
          else if (state.historyOpen && input.key === 'history-refresh')
            history(state, actions, state.historyAfter);
          else if (state.historyOpen && input.key === 'history-more' && state.historyNext !== null)
            history(state, actions, state.historyNext);
          else if (state.historyOpen && /^earlier-(0|[1-9][0-9]*)$/.test(input.key)) {
            const entry = state.earlier[Number(input.key.slice(8))];
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
        if (
          state.setup === 'idle' &&
          (['loginSaved', 'keySaved'].includes(state.notice ?? '') ||
            state.provider?.descriptor.anonymous)
        )
          setup(state, actions);
        if (state.setup === 'failed' && state.finished && !busy(state)) {
          state.finished = false;
          readStatus(state, actions);
        }
        const out = frame(presentation, state, props, actions, events);
        out.cancel = cancel;
        return out;
      },
    },
    () => 'back',
  );
}
export function activate() {
  return {
    Credential: { kind: 'component', contract: credentialContract, component: ['component'] },
  };
}
