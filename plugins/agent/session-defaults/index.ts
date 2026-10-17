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
  CallerIntent,
  Component,
  DeploymentRef,
  Emission,
  InstanceConfigure,
  InstanceSettingsPage,
  ManagedInstancesPage,
  OperationReply,
  RecoveryRef,
} from '../../../packages/plugin-sdk/src/index.js';
import { sessionDefaultsContract, type ProjectsProps } from '../../shared/contracts.js';
import { withPageShell, pageHeader, type PageState } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { copy, tuiText } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

type Request =
  | { kind: 'source' | 'settings' | 'mutate' | 'receipt' }
  | { kind: 'history'; after: number };
interface State extends PageState {
  revision: number;
  serial: number;
  source: DeploymentRef | null;
  saved: {
    access: 'read_only' | 'workspace_write' | 'danger_full_access';
    requestPermissions: boolean;
  } | null;
  pending: { key: string; request: Request } | null;
  current: { input: InstanceConfigure; reference: RecoveryRef | null } | null;
  restoring: boolean;
  notice: string | null;
}
function request(
  state: State,
  actions: Action[],
  r: Request,
  binding: string,
  method: string,
  input: unknown,
) {
  if (state.pending) return;
  const key = 'session-defaults-' + ++state.serial;
  state.pending = { key, request: r };
  actions.push({ key, binding, member: 0, method, input, resources: {} });
}
function read(state: State, actions: Action[]) {
  request(state, actions, { kind: 'source' }, 'management', 'instances', { kind: 'start' });
}
function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  if (!after) state.current = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}
function busy(state: State, props: ProjectsProps) {
  return !props.open || props.busy || !!state.pending || !!state.current || state.restoring;
}
function settle(state: State, reply: OperationReply, actions: Action[]) {
  if (
    !state.current ||
    (state.current.reference && !sameValue(state.current.reference, reply.reference))
  )
    return;
  state.current.reference = reply.reference;
  if (
    reply.receipt.kind !== 'recorded' ||
    reply.receipt.outcome.kind !== 'returned' ||
    reply.receipt.cleanup.kind !== 'confirmed'
  ) {
    state.notice = 'sessions-unknown';
    return;
  }
  const result = reply.receipt.outcome.value as { status: string };
  state.current = null;
  state.notice =
    result.status === 'committed'
      ? 'sessions-saved'
      : result.status === 'conflict'
        ? 'sessions-conflict'
        : 'sessions-rejected';
  read(state, actions);
}

/** The deployment source owns new-session defaults; there is no second policy store. */
export function sessionDefaults<N extends Node>(
  presentation: Presentation<N>,
  instance: string[],
): Component<State, ProjectsProps, N> {
  function frame(
    state: State,
    props: ProjectsProps,
    actions: Action[] = [],
    events: Emission[] = [],
  ) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale),
      t = (key: string) => tuiText(props.locale, key),
      disabled = busy(state, props) || !state.saved || !state.source;
    const result = output(
      state,
      ui.box(
        'session-defaults',
        [
          ...pageHeader(presentation, state, props, events, t('settings-sessions')),
          ui.box(
            'session-defaults-content',
            [
              ui.text('sessions-help', t('sessions-defaults-help'), props.theme.muted),
              ui.text(
                'sessions-access-label',
                t('sessions-access') +
                  (state.saved ? ': ' + t('sessions-' + state.saved.access) : ''),
                props.theme.text,
              ),
              ...(['read_only', 'workspace_write', 'danger_full_access'] as const).map((access) =>
                ui.button(
                  'access-' + access,
                  t('sessions-' + access),
                  disabled,
                  state.saved?.access === access,
                ),
              ),
              ui.button(
                'requests',
                t('sessions-requests') +
                  ': ' +
                  (state.saved?.requestPermissions ? text.settingOn : text.settingOff),
                disabled,
              ),
              ...(state.notice && !state.pending
                ? [ui.text('sessions-notice', t(state.notice), props.theme.muted)]
                : []),
              ...(state.current
                ? [
                    ui.button(
                      'receipt',
                      text.executionReceipt,
                      !!state.pending || !state.current.reference,
                    ),
                  ]
                : []),
              ...((state.restoring || (state.current && !state.current.reference)) && !state.pending
                ? [ui.button('history', text.refreshDefaultOperations)]
                : []),
              ui.button('reload', t('extensions-refresh'), busy(state, props)),
            ],
            { gap: 1, scroll: 'manual', terminal: { grow: 1 } },
          ),
        ],
        {
          visible: props.open,
          gap: 0,
          terminal: { grow: 1 },
          shortcuts: [{ key: 'escape', modifiers: [], input: 'back', enabled: true }],
        },
      ),
      events,
    );
    result.actions = actions;
    return result;
  }
  return withPageShell<State, ProjectsProps, N>(
    {
      mount(props) {
        const state: State = {
          revision: 0,
          serial: 0,
          source: null,
          saved: null,
          pending: null,
          current: null,
          restoring: true,
          notice: null,
        };
        const actions: Action[] = [];
        history(state, actions);
        return frame(state, props, actions);
      },
      update(state, props) {
        state.revision++;
        return frame(state, props);
      },
      event(state, input, props) {
        state.revision++;
        const actions: Action[] = [],
          events: Emission[] = [];
        if (input.kind === 'action' && input.key === state.pending?.key) {
          const r = state.pending.request,
            result = input.event as ActionOutcome;
          state.pending = null;
          if (r.kind === 'history') {
            if (result.status !== 'succeeded') state.notice = 'sessions-unavailable';
            else {
              const items = result.value as CallerIntent[];
              for (const item of items) {
                const recovery = item.recovery as {
                  kind?: string;
                  input?: InstanceConfigure;
                } | null;
                if (
                  item.reference.method === 'configure' &&
                  recovery?.kind === 'session-defaults/v1' &&
                  recovery.input &&
                  sameValue(recovery.input.instance, instance)
                )
                  state.current = { input: recovery.input, reference: item.reference };
              }
              const last = items.at(-1);
              if (last && last.cursor > r.after) history(state, actions, last.cursor);
              else if (last) state.notice = 'sessions-unavailable';
              else {
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
                else read(state, actions);
              }
            }
          } else if (r.kind === 'source') {
            if (result.status !== 'succeeded') {
              state.source = null;
              state.notice = 'sessions-unavailable';
            } else {
              state.source = (result.value as ManagedInstancesPage).source;
              request(state, actions, { kind: 'settings' }, 'management', 'readSettings', {
                source: state.source,
                instance,
                path: ['permissions'],
                after: null,
              });
            }
          } else if (r.kind === 'settings') {
            const page =
              result.status === 'succeeded' ? (result.value as InstanceSettingsPage) : null;
            const access = page?.fields.find((field) => field.key === 'access'),
              requests = page?.fields.find((field) => field.key === 'requestPermissions');
            if (
              page &&
              sameValue(page.source, state.source) &&
              sameValue(page.instance, instance) &&
              sameValue(page.path, ['permissions']) &&
              access?.editable &&
              requests?.editable &&
              access.value?.kind === 'text' &&
              ['read_only', 'workspace_write', 'danger_full_access'].includes(access.value.text) &&
              requests.value?.kind === 'boolean'
            ) {
              state.saved = {
                access: access.value.text as 'read_only' | 'workspace_write' | 'danger_full_access',
                requestPermissions: requests.value.value,
              };
            } else {
              state.saved = null;
              state.notice = 'sessions-unavailable';
            }
          } else if (r.kind === 'mutate') {
            if (result.status === 'operation') settle(state, result.reply, actions);
            else if (result.status === 'not_accepted') {
              state.current = null;
              state.notice = 'sessions-rejected';
            } else state.notice = 'sessions-unknown';
          } else if (r.kind === 'receipt') {
            if (result.status === 'succeeded')
              settle(state, result.value as OperationReply, actions);
            else state.notice = 'sessions-unknown';
          }
        } else if (input.kind === 'raw' && props.open) {
          if (input.key === 'back') events.push({ name: 'closed', value: {} });
          else if (
            input.key === 'history' &&
            !state.pending &&
            (state.restoring || (state.current && !state.current.reference))
          )
            history(state, actions);
          else if (input.key === 'receipt' && !state.pending && state.current?.reference)
            request(
              state,
              actions,
              { kind: 'receipt' },
              'receipts',
              'read',
              state.current.reference,
            );
          else if (!busy(state, props)) {
            if (input.key === 'reload') read(state, actions);
            else if (state.saved && state.source) {
              const access = input.key.slice('access-'.length);
              const change: InstanceConfigure['changes'][number] | null =
                input.key === 'requests'
                  ? {
                      path: ['permissions', 'requestPermissions'],
                      value: { kind: 'boolean', value: !state.saved.requestPermissions },
                    }
                  : input.key.startsWith('access-') &&
                      ['read_only', 'workspace_write', 'danger_full_access'].includes(access) &&
                      access !== state.saved.access
                    ? { path: ['permissions', 'access'], value: { kind: 'text', text: access } }
                    : null;
              if (change) {
                const value: InstanceConfigure = {
                  expectedRevision: state.source.revision,
                  instance,
                  changes: [change],
                };
                state.current = { input: value, reference: null };
                state.notice = null;
                request(state, actions, { kind: 'mutate' }, 'management', 'configure', value);
                actions.at(-1)!.recovery = { kind: 'session-defaults/v1', input: value };
              }
            }
          }
        }
        return frame(state, props, actions, events);
      },
    },
    () => 'back',
  );
}
export function activate() {
  return {
    SessionDefaults: {
      kind: 'component',
      contract: sessionDefaultsContract,
      component: ['component'],
    },
  };
}
