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
  AgentGlobalInstructionsView,
  AgentGlobalInstructionsUpdated,
  CallerIntent,
  ComponentInput,
  EditorCommand,
  EditorObservation,
  OperationReply,
  RecoveryRef,
  TextareaProps,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import type { ProjectsProps } from '../../shared/contracts.js';
import { editorMenu } from '../../shared/editor-menu.js';
import { sameValue } from '../../shared/value.js';
import type { Node, Presentation } from '../../shared/view.js';
import { tuiText, copy } from './locale.js';

type Request =
  | { kind: 'history'; after: number }
  | { kind: 'read'; reset: boolean }
  | { kind: 'capture'; observed: EditorObservation }
  | { kind: 'save' | 'receipt' };
export interface GlobalInstructionsState {
  serial: number;
  saved: Extract<AgentGlobalInstructionsView, { kind: 'available' }> | null;
  pending: { key: string; request: Request } | null;
  current: { reference: RecoveryRef | null } | null;
  restoring: boolean;
  observation: EditorObservation | null;
  command: EditorCommand | null;
  reset: string | null;
  sequence: number;
  ready: boolean;
  conflict: boolean;
  notice: string | null;
}
export function globalInstructionsMount(): GlobalInstructionsState {
  return {
    serial: 0,
    saved: null,
    pending: null,
    current: null,
    restoring: true,
    observation: null,
    command: null,
    reset: null,
    sequence: 0,
    ready: false,
    conflict: false,
    notice: null,
  };
}
function request(
  state: GlobalInstructionsState,
  actions: Action[],
  request: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  if (state.pending) return;
  const key = 'global-instructions-' + ++state.serial;
  state.pending = { key, request };
  actions.push({ key, binding, member: 0, method, input, resources });
}
function read(state: GlobalInstructionsState, actions: Action[], reset: boolean) {
  request(state, actions, { kind: 'read', reset }, 'agent', 'globalInstructions', {});
}
function history(state: GlobalInstructionsState, actions: Action[], after = 0) {
  state.restoring = true;
  if (!after) state.current = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}
function initialize(state: GlobalInstructionsState) {
  if (state.reset !== null && state.observation && !state.command)
    state.command = {
      owner: state.observation.snapshot.text.owner,
      sequence: ++state.sequence,
      basedOn: state.observation.revision,
      edit: { kind: 'reset', text: state.reset },
    };
}
function busy(state: GlobalInstructionsState, props: ProjectsProps) {
  return !props.open || props.busy || !!state.pending || !!state.current || state.restoring;
}
function settle(state: GlobalInstructionsState, reply: OperationReply, actions: Action[]) {
  if (
    !state.current ||
    (state.current.reference && !sameValue(state.current.reference, reply.reference))
  )
    return;
  state.current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'assistant-unknown';
    return;
  }
  const result = reply.receipt.outcome.value as AgentGlobalInstructionsUpdated;
  state.current = null;
  state.notice =
    result.kind === 'saved'
      ? 'assistant-global-saved'
      : result.kind === 'changed'
        ? 'assistant-global-conflict'
        : 'assistant-unavailable';
  if (result.kind === 'changed') state.conflict = true;
  if (result.kind === 'saved' || result.kind === 'changed')
    read(state, actions, result.kind === 'saved');
  else if (!state.saved) read(state, actions, true);
}
export function globalInstructionsEvent(
  state: GlobalInstructionsState,
  input: ComponentInput | null,
  props: ProjectsProps,
  actions: Action[],
) {
  if (!input) {
    history(state, actions);
    return;
  }
  if (
    input.kind === 'component' &&
    input.child === 'assistant-global' &&
    input.name === 'changed'
  ) {
    const observation = input.event as EditorObservation;
    if (
      state.observation &&
      state.observation.snapshot.text.owner !== observation.snapshot.text.owner
    ) {
      state.command = null;
      state.ready = false;
      state.reset = state.saved?.content ?? null;
    }
    state.observation = observation;
    const receipt = observation.receipt;
    if (
      state.command &&
      receipt?.owner === state.command.owner &&
      receipt.sequence === state.command.sequence
    ) {
      state.ready = receipt.outcome.status === 'applied';
      state.command = null;
      state.reset = null;
      if (!state.ready) state.notice = 'assistant-unavailable';
    } else initialize(state);
  } else if (input.kind === 'action' && input.key === state.pending?.key) {
    const r = state.pending.request,
      result = input.event as ActionOutcome;
    state.pending = null;
    if (r.kind === 'history') {
      if (result.status !== 'succeeded') {
        state.notice = 'assistant-unavailable';
        return;
      }
      const items = result.value as CallerIntent[];
      for (const item of items) {
        if (
          item.reference.contract.name === 'maka.agent.Agent' &&
          item.reference.method === 'updateGlobalInstructions'
        )
          state.current = { reference: item.reference };
      }
      const last = items.at(-1);
      if (last && last.cursor > r.after) history(state, actions, last.cursor);
      else if (last) state.notice = 'assistant-unavailable';
      else {
        state.restoring = false;
        if (state.current?.reference)
          request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.current.reference);
        else read(state, actions, true);
      }
    } else if (r.kind === 'read') {
      const view =
        result.status === 'succeeded' ? (result.value as AgentGlobalInstructionsView) : null;
      if (view?.kind !== 'available' || new TextEncoder().encode(view.content).length > 65536) {
        state.saved = null;
        state.ready = false;
        state.notice = 'assistant-global-unavailable';
        return;
      }
      state.saved = view;
      if (r.reset) {
        state.ready = false;
        state.reset = view.content;
        state.conflict = false;
        initialize(state);
      }
    } else if (r.kind === 'capture') {
      const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
      if (
        !chunk ||
        !state.saved ||
        !sameValue(chunk.metadata, r.observed.snapshot.text) ||
        !sameValue(state.observation?.snapshot.text, r.observed.snapshot.text) ||
        new TextEncoder().encode(chunk.text).length !== r.observed.snapshot.text.bytes ||
        r.observed.snapshot.text.bytes > 65536
      ) {
        state.notice = 'assistant-invalid';
        return;
      }
      const update = {
        source: state.saved.source,
        revision: state.saved.revision,
        content: chunk.text,
      };
      state.current = { reference: null };
      state.notice = null;
      request(state, actions, { kind: 'save' }, 'agent', 'updateGlobalInstructions', update);
    } else if (r.kind === 'save') {
      if (result.status === 'operation') settle(state, result.reply, actions);
      else if (result.status === 'not_accepted') {
        state.current = null;
        state.notice = 'assistant-unavailable';
      } else state.notice = 'assistant-unknown';
    } else if (r.kind === 'receipt') {
      if (result.status === 'succeeded') settle(state, result.value as OperationReply, actions);
      else state.notice = 'assistant-unknown';
    }
  } else if (input.kind === 'raw' && props.open) {
    if (
      input.key === 'global-history' &&
      !state.pending &&
      (state.restoring || (state.current && !state.current.reference))
    )
      history(state, actions);
    else if (input.key === 'global-receipt' && !state.pending && state.current?.reference)
      request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.current.reference);
    else if (!busy(state, props)) {
      if (input.key === 'global-reload') {
        state.notice = null;
        read(state, actions, true);
      } else if (
        input.key === 'global-save' &&
        state.saved &&
        state.ready &&
        !state.command &&
        !state.conflict
      ) {
        const observed = state.observation;
        if (!observed || observed.snapshot.marked) return;
        if (observed.snapshot.text.bytes > 65536) {
          state.notice = 'assistant-global-limit';
          return;
        }
        request(
          state,
          actions,
          { kind: 'capture', observed },
          'reader',
          'read',
          { start: 0, end: observed.snapshot.text.bytes },
          {
            text: {
              child: 'assistant-global',
              method: 'snapshot',
              input: {
                kind: 'at_revision',
                owner: observed.snapshot.text.owner,
                revision: observed.snapshot.text.revision,
              },
            },
          },
        );
      }
    }
  }
}
export function globalInstructionsView<N extends Node>(
  state: GlobalInstructionsState,
  props: ProjectsProps,
  p: Presentation<N>,
): N {
  const ui = p.paint(props.theme),
    t = (key: string) => tuiText(props.locale, key),
    text = copy(props.locale);
  return ui.box(
    'global-instructions',
    [
      ui.text('global-label', t('assistant-global'), props.theme.muted),
      ...(state.saved ? [ui.text('global-path', state.saved.path, props.theme.muted)] : []),
      p.slot(
        'assistant-global',
        'editor',
        {
          appearance: props.theme.textarea,
          menu: editorMenu(props.theme, props.locale),
          accessibilityLabel: t('assistant-global'),
          placeholder: '',
          autoFocus: false,
          tabIndex: 2,
          readOnly: false,
          disabled: busy(state, props) || !state.ready || !!state.command,
          command: state.command,
          softWrap: true,
          terminalRows: { min: 4, max: 24, resizable: true },
        } satisfies TextareaProps,
        { changed: 'changed' },
      ),
      ...(state.notice ? [ui.text('global-notice', t(state.notice), props.theme.muted)] : []),
      ui.box(
        'global-controls',
        [
          ui.button(
            'global-save',
            t('assistant-global-save'),
            busy(state, props) || !state.saved || !state.ready || !!state.command || state.conflict,
          ),
          ui.button('global-reload', t('assistant-global-reload'), busy(state, props)),
        ],
        { row: false, gap: 1, wide: { min_width: 54, direction: 'row' } },
      ),
      ...(state.current
        ? [
            ui.button(
              'global-receipt',
              text.executionReceipt,
              !!state.pending || !state.current.reference,
            ),
          ]
        : []),
      ...((state.restoring || (state.current && !state.current.reference)) && !state.pending
        ? [ui.button('global-history', text.refreshDefaultOperations)]
        : []),
    ],
    { gap: 0 },
  );
}
