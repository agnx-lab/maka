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
  WebKey,
  WebSet,
  WebSnapshot,
  WebUpdate,
} from '../../../packages/plugin-sdk/src/index.js';
import { webSettingsContract, type ProjectsProps } from '../../shared/contracts.js';
import { withPageShell, pageHeader, type PageState } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { copy, tuiText } from './locale.js';
import { output, type Node, type Presentation } from '../../shared/view.js';

type Mutation =
  | { method: 'save'; input: WebSet }
  | { method: 'saveKey' | 'deleteKey' | 'checkKey'; input: WebKey };
type Request = { kind: 'snapshot' | 'mutate' | 'receipt' } | { kind: 'history'; after: number };
interface State extends PageState {
  revision: number;
  serial: number;
  snapshot: WebSnapshot | null;
  observation: EditorObservation | null;
  command: EditorCommand | null;
  sequence: number;
  pending: { key: string; request: Request } | null;
  current: {
    mutation: Mutation;
    reference: RecoveryRef | null;
    captured: EditorObservation | null;
  } | null;
  restoring: boolean;
  confirming: boolean;
  notice: string | null;
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
  if (state.pending) return;
  const key = 'web-settings-' + ++state.serial;
  state.pending = { key, request };
  actions.push({ key, binding, member: 0, method, input, resources });
}
function read(state: State, actions: Action[]) {
  request(state, actions, { kind: 'snapshot' }, 'web', 'snapshot', {});
}
function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  if (!after) state.current = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}
function busy(state: State, props: ProjectsProps) {
  return !props.open || props.busy || !!state.pending || !!state.current || state.restoring;
}
function mutate(state: State, actions: Action[], mutation: Mutation) {
  const captured = mutation.method === 'saveKey' ? state.observation : null;
  state.current = { mutation, reference: null, captured };
  state.notice = null;
  request(
    state,
    actions,
    { kind: 'mutate' },
    'web',
    mutation.method,
    mutation.input,
    captured
      ? {
          secret: {
            child: 'web-secret',
            method: 'snapshot',
            input: {
              kind: 'at_revision',
              owner: captured.snapshot.text.owner,
              revision: captured.revision,
            },
          },
        }
      : {},
  );
  actions.at(-1)!.recovery = { kind: 'web-settings/v1', ...mutation };
}
function settle(state: State, reply: OperationReply, actions: Action[]) {
  const current = state.current;
  if (!current || (current.reference && !sameValue(current.reference, reply.reference))) return;
  current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'web-unknown';
    return;
  }
  const result = reply.receipt.outcome.value as WebUpdate;
  if (!['updated', 'changed', 'invalid_key', 'check_unavailable'].includes(result.kind)) {
    state.notice = 'web-unknown';
    return;
  }
  if (
    result.kind === 'updated' &&
    current.captured &&
    state.observation?.snapshot.text.owner === current.captured.snapshot.text.owner
  ) {
    state.command = {
      owner: current.captured.snapshot.text.owner,
      sequence: ++state.sequence,
      basedOn: current.captured.revision,
      edit: { kind: 'reset', text: '' },
    };
  }
  state.current = null;
  state.notice =
    result.kind === 'updated'
      ? current.mutation.method === 'checkKey'
        ? 'web-checked'
        : 'web-saved'
      : result.kind === 'invalid_key'
        ? 'web-key-invalid'
        : result.kind === 'check_unavailable'
          ? 'web-check-unavailable'
          : 'web-conflict';
  // A recovered receipt may be older than a change by another caller.
  read(state, actions);
}
export function webSettings<N extends Node>(
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
    const disabled = busy(state, props) || state.confirming,
      snapshot = state.snapshot;
    const canSave =
      !!state.observation?.snapshot.text.bytes &&
      state.observation.snapshot.text.bytes <= 4096 &&
      !state.observation.snapshot.marked &&
      !state.command;
    const content = ui.box(
      'web-content',
      [
        ui.toggle(
          'enabled',
          t('web-enabled'),
          snapshot?.settings.enabled ?? false,
          disabled || !snapshot,
          t('web-enabled-help'),
        ),
        ui.text('web-source-label', t('web-source'), props.theme.muted),
        ...(['model', 'tavily'] as const).map((source) =>
          ui.button(
            'source-' + source,
            t('web-source-' + source),
            disabled || !snapshot,
            snapshot?.settings.source === source,
            { background: 0, align: 'start' },
          ),
        ),
        ui.text(
          'web-source-help',
          t(snapshot?.settings.source === 'tavily' ? 'web-tavily-help' : 'web-model-help'),
          props.theme.muted,
        ),
        ui.text(
          'web-key-status',
          t(snapshot?.credential.configured ? 'web-key-configured' : 'web-key-missing'),
          props.theme.muted,
        ),
        presentation.slot(
          'web-secret',
          'secret',
          {
            menu: editorMenu(props.theme, props.locale),
            appearance: {
              ...props.theme.input,
              borderWidth: 0,
              padding: { top: 0, right: 0, bottom: 0, left: 0 },
            },
            accessibilityLabel: t('web-key'),
            placeholder: t('web-key'),
            autoFocus: false,
            tabIndex: 0,
            readOnly: false,
            disabled: disabled || !!state.command,
            command: state.command,
          } satisfies InputProps,
          { changed: 'changed' },
        ),
        ui.button('save-key', t('web-save-key'), disabled || !snapshot || !canSave),
        ui.button('check-key', t('web-check-key'), disabled || !snapshot?.credential.configured),
        ...(snapshot?.credential.check
          ? [
              ui.text(
                'web-check-result',
                t('web-check-' + snapshot.credential.check.outcome),
                props.theme.muted,
              ),
            ]
          : []),
        ui.button('remove-key', t('web-remove-key'), disabled || !snapshot?.credential.configured),
        ...(state.notice ? [ui.text('web-notice', t(state.notice), props.theme.muted)] : []),
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
        ui.button('reload', t('web-reload'), disabled),
      ],
      { gap: 1, scroll: 'manual', terminal: { grow: 1 } },
    );
    const confirmation = ui.box(
      'web-confirmation',
      [
        ui.text('web-remove-question', t('web-remove-question'), props.theme.text),
        ui.button('confirm-remove', t('web-remove-key'), busy(state, props)),
        ui.button('cancel', text.cancel),
      ],
      {
        gap: 1,
        shortcuts: [{ key: 'escape', modifiers: [], input: 'cancel', enabled: state.confirming }],
      },
    );
    const result = output(
      state,
      ui.box(
        'web-settings-root',
        [
          ...pageHeader(presentation, state, props, events, t('settings-web')),
          ui.dialog('web-removal', t('web-remove-key'), content, confirmation, state.confirming, {
            width: 52,
            height: 10,
          }),
        ],
        {
          visible: props.open,
          gap: 1,
          terminal: { grow: 1 },
          shortcuts: [
            {
              key: 'escape',
              modifiers: [],
              input: state.confirming ? 'cancel' : 'back',
              enabled: true,
            },
          ],
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
          snapshot: null,
          observation: null,
          command: null,
          sequence: 0,
          pending: null,
          current: null,
          restoring: true,
          confirming: false,
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
        if (
          input.kind === 'component' &&
          input.child === 'web-secret' &&
          input.name === 'changed'
        ) {
          const observation = input.event as EditorObservation;
          if (
            state.observation &&
            state.observation.snapshot.text.owner !== observation.snapshot.text.owner
          )
            state.command = null;
          state.observation = observation;
          if (
            state.command &&
            observation.receipt?.owner === state.command.owner &&
            observation.receipt.sequence === state.command.sequence
          ) {
            state.command = null;
          }
        } else if (input.kind === 'action' && input.key === state.pending?.key) {
          const r = state.pending.request,
            result = input.event as ActionOutcome;
          state.pending = null;
          if (r.kind === 'history') {
            if (result.status !== 'succeeded') state.notice = 'web-unavailable';
            else {
              const items = result.value as CallerIntent[];
              for (const item of items) {
                const recovery = item.recovery as (Mutation & { kind?: string }) | null;
                if (
                  item.reference.contract.name === 'maka.workspace.Web' &&
                  recovery?.kind === 'web-settings/v1' &&
                  recovery.input &&
                  ['save', 'saveKey', 'deleteKey', 'checkKey'].includes(recovery.method) &&
                  recovery.method === item.reference.method
                )
                  state.current = { mutation: recovery, reference: item.reference, captured: null };
              }
              const last = items.at(-1);
              if (last && last.cursor > r.after) history(state, actions, last.cursor);
              else if (last) state.notice = 'web-unavailable';
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
            if (result.status === 'succeeded') state.snapshot = result.value as WebSnapshot;
            else state.notice = 'web-unavailable';
          } else if (r.kind === 'mutate') {
            if (result.status === 'operation') settle(state, result.reply, actions);
            else if (result.status === 'not_accepted') {
              state.current = null;
              state.notice = 'web-unavailable';
            } else state.notice = 'web-unknown';
          } else if (r.kind === 'receipt') {
            if (result.status === 'succeeded')
              settle(state, result.value as OperationReply, actions);
            else state.notice = 'web-unknown';
          }
        } else if (input.kind === 'raw' && props.open) {
          if (input.key === 'back') {
            if (state.confirming) state.confirming = false;
            else events.push({ name: 'closed', value: {} });
          } else if (input.key === 'cancel') state.confirming = false;
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
            if (input.key === 'confirm-remove' && state.confirming && state.snapshot) {
              state.confirming = false;
              mutate(state, actions, {
                method: 'deleteKey',
                input: { revision: state.snapshot.credential.revision },
              });
            } else if (!state.confirming) {
              if (input.key === 'reload') read(state, actions);
              else if (state.snapshot) {
                const snapshot = state.snapshot;
                if (input.key === 'enabled')
                  mutate(state, actions, {
                    method: 'save',
                    input: {
                      revision: snapshot.revision,
                      settings: { ...snapshot.settings, enabled: !snapshot.settings.enabled },
                    },
                  });
                else if (input.key === 'source-model' || input.key === 'source-tavily') {
                  const source = input.key === 'source-model' ? 'model' : 'tavily';
                  if (source !== snapshot.settings.source)
                    mutate(state, actions, {
                      method: 'save',
                      input: {
                        revision: snapshot.revision,
                        settings: { ...snapshot.settings, source },
                      },
                    });
                } else if (input.key === 'check-key' && snapshot.credential.configured)
                  mutate(state, actions, {
                    method: 'checkKey',
                    input: { revision: snapshot.credential.revision },
                  });
                else if (input.key === 'remove-key' && snapshot.credential.configured)
                  state.confirming = true;
                else if (
                  input.key === 'save-key' &&
                  !state.command &&
                  state.observation?.snapshot.text.bytes &&
                  state.observation.snapshot.text.bytes <= 4096 &&
                  !state.observation.snapshot.marked
                )
                  mutate(state, actions, {
                    method: 'saveKey',
                    input: { revision: snapshot.credential.revision },
                  });
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
    WebSettings: { kind: 'component', contract: webSettingsContract, component: ['component'] },
  };
}
