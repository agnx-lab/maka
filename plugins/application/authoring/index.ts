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
  RecoveryRef,
  OperationReply,
  UiAuthoringRequest,
} from '../../../packages/plugin-sdk/src/index.js';
import { ui } from '../../../packages/plugin-sdk/src/index.js';
import { output, type Node, type Presentation } from '../../shared/view.js';
interface State {
  revision: number;
  serial: number;
  scanning: boolean;
  after: number;
  latest: CallerIntent | null;
  pending: { key: string; stage: string } | null;
  requestId: string | null;
  halted: boolean;
  wake: boolean;
}
const mutations = new Set([
  'installSource',
  'installUrl',
  'replaceSource',
  'editComposition',
  'configure',
]);
function value(outcome: ActionOutcome): unknown {
  if (outcome.status === 'succeeded') return outcome.value;
  if (
    outcome.status === 'operation' &&
    outcome.reply.receipt.kind === 'recorded' &&
    outcome.reply.receipt.outcome.kind === 'returned'
  )
    return outcome.reply.receipt.outcome.value;
  return undefined;
}
export function worker<N extends Node>(
  presentation: Presentation<N>,
): Component<State, Record<string, never>, N> {
  const action = (
    state: State,
    stage: string,
    binding: string,
    method: string,
    input: unknown,
    mutation = false,
    recovery: unknown = null,
  ): Action => {
    const key = `authoring-${++state.serial}`;
    state.pending = { key, stage };
    return {
      key,
      binding,
      member: 0,
      method,
      input,
      resources: {},
      ...(mutation ? { recovery } : {}),
    };
  };
  const claim = (state: State) => {
    state.wake = false;
    return action(state, 'claim', 'requests', 'claim', {}, true, { uiAuthoring: 'claim' });
  };
  const recover = (
    state: State,
    stage: string,
    binding: string,
    reference: RecoveryRef,
  ): Action => {
    const key = `authoring-${++state.serial}`;
    state.pending = { key, stage: `recover-${stage}` };
    return { key, binding, member: 0, method: 'read', input: reference, resources: {} };
  };
  const frame = (state: State, actions: Action[] = []) => {
    const view = ui.terminal.stack({
      key: 'authoring-worker',
      children: [],
      direction: 'column',
      gap: 0,
      padding: 0,
      background: 0,
    });
    const out = output(state, view as N);
    out.actions = actions;
    out.subscriptions =
      state.scanning || state.halted
        ? []
        : [
            {
              key: 'authoring-watch',
              binding: 'requests',
              member: 0,
              method: 'watch',
              input: {},
              resources: {},
            },
          ];
    return out;
  };
  const complete = (state: State, outcome: ActionOutcome) =>
    action(state, 'complete', 'requests', 'complete', { id: state.requestId!, outcome }, true, {
      uiAuthoring: 'complete',
      id: state.requestId!,
    });
  const execute = (state: State, request: UiAuthoringRequest): Action => {
    const { method, input } = request.command;
    return action(state, 'apply', 'management', method, input, mutations.has(method), {
      uiAuthoring: 'apply',
      id: request.id,
    });
  };
  return {
    mount() {
      const state: State = {
        revision: 0,
        serial: 0,
        scanning: true,
        after: 0,
        latest: null,
        pending: null,
        requestId: null,
        halted: false,
        wake: false,
      };
      return frame(state, [action(state, 'history', 'history', 'page', { after: 0, limit: 32 })]);
    },
    update(state) {
      return frame({ ...state, revision: state.revision + 1 });
    },
    event(state, input) {
      state = { ...state, revision: state.revision + 1 };
      if (input.kind !== 'action') return frame(state);
      if (input.key === 'authoring-watch') {
        if ((input.event as ActionOutcome).status !== 'updated') {
          state.halted = true;
          return frame(state);
        }
        state.wake = true;
        return frame(
          state,
          !state.pending && !state.scanning && !state.halted ? [claim(state)] : [],
        );
      }
      if (input.key !== state.pending?.key) return frame(state);
      let stage = state.pending.stage;
      state.pending = null;
      let outcome = input.event as ActionOutcome;
      if (stage.startsWith('recover-')) {
        stage = stage.slice(8);
        if (outcome.status === 'succeeded')
          outcome = { status: 'operation', reply: outcome.value as OperationReply, caller: null };
      }
      const result = value(outcome);
      if (stage === 'history') {
        if (!Array.isArray(result)) {
          state.halted = true;
          return frame(state);
        }
        const page = result as CallerIntent[];
        for (const intent of page)
          if ((intent.recovery as { uiAuthoring?: string })?.uiAuthoring) state.latest = intent;
        if (page.length) {
          state.after = page.at(-1)!.cursor;
          return frame(state, [
            action(state, 'history', 'history', 'page', { after: state.after, limit: 32 }),
          ]);
        }
        state.scanning = false;
        const saved = state.latest?.recovery as { uiAuthoring?: string; id?: string } | undefined;
        if (saved?.uiAuthoring === 'apply' && saved.id) {
          state.requestId = saved.id;
          return frame(state, [
            recover(state, 'apply', 'managerReceipts', state.latest!.reference),
          ]);
        }
        if (saved?.uiAuthoring === 'complete')
          return frame(state, [recover(state, 'complete', 'receipts', state.latest!.reference)]);
        if (saved?.uiAuthoring === 'claim')
          return frame(state, [recover(state, 'claim', 'receipts', state.latest!.reference)]);
        return frame(state, [claim(state)]);
      }
      if (stage === 'claim') {
        if (!result || typeof result !== 'object' || !('request' in result)) {
          state.halted = true;
          return frame(state);
        }
        const request = (result as { request: UiAuthoringRequest | null }).request;
        state.requestId = request?.id ?? null;
        return frame(state, request ? [execute(state, request)] : state.wake ? [claim(state)] : []);
      }
      if (stage === 'apply') return frame(state, [complete(state, outcome)]);
      if (stage === 'complete') {
        state.requestId = null;
        if (result !== true) {
          state.halted = true;
          return frame(state);
        }
        return frame(state, [claim(state)]);
      }
      return frame(state);
    },
  };
}
