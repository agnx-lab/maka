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
  CallerIntent,
  Component,
  EditorCommand,
  EditorObservation,
  Emission,
  InputProps,
  OperationReply,
  RecoveryRef,
  SettingsNetworkSnapshot,
  SettingsNetworkUpdate,
  SettingsNetworkUpdated,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { networkSettingsContract, type ProjectsProps } from '../../shared/contracts.js';
import { withPageShell, pageHeader, type PageState } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { copy, tuiText } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

const names = ['host', 'port', 'username', 'bypass'] as const;
type Name = (typeof names)[number];
interface Field {
  observation: EditorObservation | null;
  command: EditorCommand | null;
  reset: string | null;
  sequence: number;
  ready: boolean;
}
type Request =
  | { kind: 'snapshot' | 'mutate' | 'receipt' }
  | { kind: 'history'; after: number }
  | { kind: 'field'; name: Name; captured: EditorObservation };
interface State extends PageState {
  revision: number;
  serial: number;
  saved: SettingsNetworkSnapshot | null;
  draft: SettingsNetworkSnapshot['proxy'] | null;
  credential: SettingsNetworkUpdate['credential'];
  fields: Record<Name | 'secret', Field>;
  pending: { key: string; request: Request } | null;
  current: {
    input: SettingsNetworkUpdate;
    reference: RecoveryRef | null;
    secret: EditorObservation | null;
  } | null;
  capture: {
    observations: Record<Name, EditorObservation>;
    values: Partial<Record<Name, string>>;
    secret: EditorObservation | null;
  } | null;
  restoring: boolean;
  notice: string | null;
}
const field = (): Field => ({
  observation: null,
  command: null,
  reset: null,
  sequence: 0,
  ready: false,
});
function initialize(field: Field) {
  if (field.reset !== null && field.observation && !field.command)
    field.command = {
      owner: field.observation.snapshot.text.owner,
      sequence: ++field.sequence,
      basedOn: field.observation.revision,
      edit: { kind: 'reset', text: field.reset },
    };
}
function value(state: State, name: Name | 'secret') {
  if (!state.draft || name === 'secret') return '';
  return name === 'bypass' ? state.draft.bypassList.join(', ') : String(state.draft[name]);
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
  if (state.pending) return;
  const key = 'network-settings-' + ++state.serial;
  state.pending = { key, request: r };
  actions.push({ key, binding, member: 0, method, input, resources });
}
function read(state: State, actions: Action[]) {
  request(state, actions, { kind: 'snapshot' }, 'settings', 'network', {});
}
function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  if (!after) state.current = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}
function busy(state: State, props: ProjectsProps) {
  return (
    !props.open ||
    props.busy ||
    !!state.pending ||
    !!state.current ||
    !!state.capture ||
    state.restoring
  );
}
function resource(observed: EditorObservation, name: string) {
  return {
    child: 'network-' + name,
    method: 'snapshot',
    input: {
      kind: 'at_revision',
      owner: observed.snapshot.text.owner,
      revision: observed.snapshot.text.revision,
    },
  };
}
function capture(state: State, actions: Action[], name: Name) {
  const observed = state.capture!.observations[name];
  request(
    state,
    actions,
    { kind: 'field', name, captured: observed },
    'reader',
    'read',
    { start: 0, end: observed.snapshot.text.bytes },
    { text: resource(observed, name) },
  );
}
function settle(state: State, reply: OperationReply, actions: Action[]) {
  const current = state.current;
  if (!current || (current.reference && !sameValue(current.reference, reply.reference))) return;
  current.reference = reply.reference;
  if (
    reply.receipt.kind !== 'recorded' ||
    reply.receipt.outcome.kind !== 'returned' ||
    reply.receipt.cleanup.kind !== 'confirmed'
  ) {
    state.notice = 'network-unknown';
    return;
  }
  const result = reply.receipt.outcome.value as SettingsNetworkUpdated;
  const committed = result.kind === 'applied' && result.result.kind === 'committed';
  if (
    committed &&
    current.secret &&
    state.fields.secret.observation?.snapshot.text.owner === current.secret.snapshot.text.owner
  ) {
    state.fields.secret.command = {
      owner: current.secret.snapshot.text.owner,
      sequence: ++state.fields.secret.sequence,
      basedOn: current.secret.revision,
      edit: { kind: 'reset', text: '' },
    };
  }
  state.current = null;
  state.notice = committed
    ? 'network-saved'
    : result.kind === 'rejected'
      ? 'network-invalid'
      : 'network-conflict';
  // Historical results report the operation; current values always come from the owner.
  if (committed || !state.saved) read(state, actions);
}
export function networkSettings<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ProjectsProps, N> {
  function frame(
    state: State,
    props: ProjectsProps,
    actions: Action[] = [],
    events: Emission[] = [],
  ) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale),
      t = (key: string) => tuiText(props.locale, key);
    const disabled = busy(state, props) || !state.saved;
    const fields = names.map((name, index) =>
      ui.box(
        'network-field-' + name,
        [
          ui.text('network-label-' + name, t('network-' + name), props.theme.muted),
          presentation.slot(
            'network-' + name,
            'input',
            {
              menu: editorMenu(props.theme, props.locale),
              accessibilityLabel: t('network-' + name),
              placeholder: '',
              autoFocus: name === 'host',
              tabIndex: index + 1,
              readOnly: false,
              disabled: disabled || !state.fields[name].ready || !!state.fields[name].command,
              command: state.fields[name].command,
              appearance: {
                ...props.theme.input,
                borderWidth: 0,
                padding: { top: 0, right: 0, bottom: 0, left: 0 },
              },
            } satisfies InputProps,
            { changed: 'changed' },
          ),
        ],
        { gap: 0 },
      ),
    );
    const result = output(
      state,
      ui.box(
        'network-settings',
        [
          ...pageHeader(presentation, state, props, events, t('settings-host')),
          ui.box(
            'network-settings-content',
            [
              ui.toggle(
                'enabled',
                t('network-enabled'),
                state.draft?.enabled ?? false,
                disabled,
                t('network-help'),
              ),
              ui.box(
                'network-protocols',
                (['http', 'https', 'socks5'] as const).map((protocol) =>
                  ui.button(
                    'protocol-' + protocol,
                    protocol.toUpperCase(),
                    disabled,
                    state.draft?.protocol === protocol,
                  ),
                ),
                { row: true, gap: 1 },
              ),
              ...fields,
              ui.toggle(
                'auth',
                t('network-auth'),
                state.draft?.authEnabled ?? false,
                disabled,
                t('network-auth-help'),
              ),
              ui.text(
                'network-credential-status',
                t(
                  state.saved?.credential.configured
                    ? 'network-secret-present'
                    : 'network-secret-absent',
                ),
                props.theme.muted,
              ),
              ui.box(
                'network-credential-actions',
                (['keep', 'replace', 'delete'] as const).map((action) =>
                  ui.button(
                    'credential-' + action,
                    t('network-' + action),
                    disabled || (!state.draft?.authEnabled && action !== 'delete'),
                    state.credential === action,
                  ),
                ),
                { gap: 0 },
              ),
              presentation.slot(
                'network-secret',
                'secret',
                {
                  menu: editorMenu(props.theme, props.locale),
                  accessibilityLabel: t('network-password'),
                  placeholder: t('network-password'),
                  autoFocus: false,
                  tabIndex: 5,
                  readOnly: false,
                  disabled:
                    disabled || state.credential !== 'replace' || !!state.fields.secret.command,
                  command: state.fields.secret.command,
                  appearance: {
                    ...props.theme.input,
                    borderWidth: 0,
                    padding: { top: 0, right: 0, bottom: 0, left: 0 },
                  },
                } satisfies InputProps,
                { changed: 'changed' },
              ),
              ui.text('network-delete-help', t('network-delete-help'), props.theme.muted),
              ...(state.notice && !state.pending
                ? [ui.text('network-notice', t(state.notice), props.theme.muted)]
                : []),
              ui.button(
                'save',
                t('network-save'),
                disabled ||
                  names.some((name) => !state.fields[name].ready || !!state.fields[name].command),
              ),
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
              ui.button('reload', t('network-reload'), busy(state, props)),
              ...(state.saved
                ? [
                    ui.text('network-root-id', 'Root: ' + state.saved.rootId, props.theme.muted),
                    ui.text('network-root-path', state.saved.rootPath, props.theme.muted),
                    ui.text(
                      'network-auto-bypass',
                      t('network-auto-bypass') +
                        ': ' +
                        state.saved.proxy.autoBypassDomains.join(', '),
                      props.theme.muted,
                    ),
                  ]
                : []),
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
          saved: null,
          draft: null,
          credential: 'keep',
          fields: {
            host: field(),
            port: field(),
            username: field(),
            bypass: field(),
            secret: field(),
          },
          pending: null,
          current: null,
          capture: null,
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
        if (input.kind === 'component' && input.name === 'changed') {
          const name = [...names, 'secret' as const].find(
            (name) => input.kind === 'component' && input.child === 'network-' + name,
          );
          if (name) {
            const field = state.fields[name],
              observed = input.event as EditorObservation;
            if (
              field.observation &&
              field.observation.snapshot.text.owner !== observed.snapshot.text.owner
            ) {
              field.command = null;
              field.ready = false;
              field.reset = value(state, name);
            }
            field.observation = observed;
            if (
              field.command &&
              observed.receipt?.owner === field.command.owner &&
              observed.receipt.sequence === field.command.sequence
            ) {
              field.ready = observed.receipt.outcome.status === 'applied';
              field.command = null;
              field.reset = null;
              if (!field.ready) state.notice = 'network-unavailable';
            } else initialize(field);
          }
        } else if (input.kind === 'action' && input.key === state.pending?.key) {
          const r = state.pending.request,
            result = input.event as ActionOutcome;
          state.pending = null;
          if (r.kind === 'history') {
            if (result.status !== 'succeeded') state.notice = 'network-unavailable';
            else {
              const items = result.value as CallerIntent[];
              for (const item of items) {
                const recovery = item.recovery as {
                  kind?: string;
                  input?: SettingsNetworkUpdate;
                } | null;
                if (
                  item.reference.method === 'updateNetwork' &&
                  item.reference.contract.name === 'maka.settings.Settings' &&
                  recovery?.kind === 'network-settings/v1' &&
                  recovery.input
                )
                  state.current = {
                    input: recovery.input,
                    reference: item.reference,
                    secret: null,
                  };
              }
              const last = items.at(-1);
              if (last && last.cursor > r.after) history(state, actions, last.cursor);
              else if (last) state.notice = 'network-unavailable';
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
          } else if (r.kind === 'snapshot') {
            if (result.status !== 'succeeded') state.notice = 'network-unavailable';
            else {
              state.saved = result.value as SettingsNetworkSnapshot;
              state.draft = { ...state.saved.proxy };
              state.credential = state.draft.authEnabled ? 'keep' : 'delete';
              for (const name of names) {
                state.fields[name].ready = false;
                state.fields[name].reset = value(state, name);
                initialize(state.fields[name]);
              }
            }
          } else if (r.kind === 'mutate') {
            if (result.status === 'operation') settle(state, result.reply, actions);
            else if (result.status === 'not_accepted') {
              state.current = null;
              state.notice = 'network-invalid';
            } else state.notice = 'network-unknown';
          } else if (r.kind === 'receipt') {
            if (result.status === 'succeeded')
              settle(state, result.value as OperationReply, actions);
            else state.notice = 'network-unknown';
          } else if (r.kind === 'field') {
            const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
            if (
              !state.capture ||
              !chunk ||
              !sameValue(chunk.metadata, r.captured.snapshot.text) ||
              !sameValue(state.fields[r.name].observation?.snapshot.text, r.captured.snapshot.text)
            ) {
              state.capture = null;
              state.notice = 'network-invalid';
            } else {
              state.capture.values[r.name] = chunk.text;
              const next = names[names.indexOf(r.name) + 1];
              if (next) capture(state, actions, next);
              else {
                const captured = state.capture;
                state.capture = null;
                const values = captured.values,
                  port = Number(values.port);
                if (
                  !state.saved ||
                  !state.draft ||
                  !/^\d+$/.test(values.port!) ||
                  !Number.isInteger(port) ||
                  port < 1 ||
                  port > 65535 ||
                  names.some(
                    (name) =>
                      !sameValue(
                        state.fields[name].observation?.snapshot.text,
                        captured.observations[name].snapshot.text,
                      ),
                  )
                )
                  state.notice = 'network-invalid';
                else {
                  const update: SettingsNetworkUpdate = {
                    expectedPolicyRevision: state.saved.revision,
                    expectedCredential: state.saved.credential.configured
                      ? {
                          locator: state.saved.credential.locator,
                          credentialId: state.saved.credential.credentialId!,
                          revision: state.saved.credential.revision!,
                        }
                      : null,
                    networkProxy: {
                      ...state.draft,
                      host: values.host!,
                      port,
                      username: values.username!,
                      bypassList: [
                        ...new Set(
                          values
                            .bypass!.split(',')
                            .map((s) => s.trim())
                            .filter(Boolean),
                        ),
                      ],
                    },
                    credential: state.credential,
                  };
                  state.current = { input: update, reference: null, secret: captured.secret };
                  state.notice = null;
                  request(
                    state,
                    actions,
                    { kind: 'mutate' },
                    'settings',
                    'updateNetwork',
                    update,
                    captured.secret ? { secret: resource(captured.secret, 'secret') } : {},
                  );
                  actions.at(-1)!.recovery = { kind: 'network-settings/v1', input: update };
                }
              }
            }
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
            else if (state.draft) {
              if (input.key === 'enabled') state.draft.enabled = !state.draft.enabled;
              else if (input.key === 'auth') {
                state.draft.authEnabled = !state.draft.authEnabled;
                state.credential = state.draft.authEnabled ? 'keep' : 'delete';
              } else if (input.key.startsWith('protocol-')) {
                const p = input.key.slice(9);
                if (p === 'http' || p === 'https' || p === 'socks5') state.draft.protocol = p;
              } else if (input.key.startsWith('credential-')) {
                const c = input.key.slice(11);
                if (
                  (c === 'keep' || c === 'replace' || c === 'delete') &&
                  (state.draft.authEnabled || c === 'delete')
                )
                  state.credential = c;
              } else if (input.key === 'save' && state.saved) {
                const secret =
                  state.credential === 'replace' ? state.fields.secret.observation : null;
                if (
                  names.some(
                    (name) =>
                      !state.fields[name].ready ||
                      state.fields[name].command ||
                      !state.fields[name].observation ||
                      state.fields[name].observation!.snapshot.marked,
                  ) ||
                  (state.credential === 'replace' &&
                    (!secret ||
                      secret.snapshot.marked ||
                      !secret.snapshot.text.bytes ||
                      state.fields.secret.command))
                )
                  state.notice = 'network-invalid';
                else {
                  state.capture = {
                    observations: Object.fromEntries(
                      names.map((name) => [name, state.fields[name].observation!]),
                    ) as Record<Name, EditorObservation>,
                    values: {},
                    secret,
                  };
                  capture(state, actions, names[0]);
                }
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
    NetworkSettings: {
      kind: 'component',
      contract: networkSettingsContract,
      component: ['component'],
    },
  };
}
