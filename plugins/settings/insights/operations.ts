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
  OperationReply,
  Pricing,
  PricingEntry,
  PricingUpdate,
  SettingsPricingResult,
} from '../../../packages/plugin-sdk/src/index.js';
import { sameValue } from '../../shared/value.js';
import {
  fields,
  newFields,
  type Field,
  type FieldName,
  type Request,
  type State,
} from './state.js';

export function initialize(field: Field) {
  if (field.reset !== null && field.observation && !field.command)
    field.command = {
      owner: field.observation.snapshot.text.owner,
      sequence: ++field.sequence,
      basedOn: field.observation.revision,
      edit: { kind: 'reset', text: field.reset },
    };
}

export function request(
  state: State,
  actions: Action[],
  r: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  if (state.pending) return;
  const key = 'insights-' + ++state.serial;
  state.pending = { key, request: r };
  actions.push({ key, binding, member: 0, method, input, resources });
}

export function refresh(state: State, actions: Action[]) {
  const to = Date.now();
  state.range = {
    ...state.range,
    from: state.period === 'all' ? 0 : Math.max(0, to - Number(state.period) * 86_400_000),
    to,
    through: null,
  };
  state.report = null;
  state.activity = null;
  state.offsets = [0];
  state.detail = null;
  state.groupOffsets = [0];
  summary(state, actions);
}

export function summary(state: State, actions: Action[]) {
  request(state, actions, { kind: 'summary' }, 'settings', 'usageSummary', {
    range: state.range,
    group: state.group,
    offset: state.groupOffsets.at(-1)!,
  });
}

export function activity(state: State, actions: Action[]) {
  request(state, actions, { kind: 'activity' }, 'settings', 'usage', {
    range: state.range,
    selection: state.selection,
    offset: state.offsets.at(-1)!,
  });
}

export function prices(state: State, actions: Action[], offset = 0) {
  request(
    state,
    actions,
    { kind: 'prices' },
    'settings',
    'prices',
    offset && state.prices
      ? { kind: 'continue', revision: state.prices.revision, offset }
      : { kind: 'start' },
  );
}

export function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  if (!after) state.current = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', {
    after,
    limit: 16,
  });
}

export function mutate(state: State, actions: Action[], input: PricingUpdate) {
  state.current = { input, reference: null };
  state.notice = null;
  state.confirm = false;
  request(state, actions, { kind: 'mutate' }, 'settings', 'updatePrice', input);
  actions.at(-1)!.recovery = { kind: 'insights-price/v1', input };
}

export function settle(state: State, reply: OperationReply, actions: Action[]) {
  if (
    !state.current ||
    (state.current.reference && !sameValue(state.current.reference, reply.reference))
  )
    return;
  state.current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'insights-unknown';
    return;
  }
  const result = reply.receipt.outcome.value as SettingsPricingResult;
  state.current = null;
  if (result.kind === 'updated' && result.result.kind !== 'revision_conflict') {
    state.notice = 'insights-saved';
    state.editing = null;
    state.priceOffsets = [0];
    if (state.tab === 'pricing') prices(state, actions);
    else refresh(state, actions);
  } else {
    state.notice = result.kind === 'rejected' ? 'insights-invalid' : 'insights-conflict';
    if (!state.report && state.tab !== 'pricing') refresh(state, actions);
  }
}

export function edit(state: State, entry: PricingEntry | null) {
  if (!state.prices) return;
  state.editing = {
    key: ++state.serial,
    entry,
    revision: state.prices.revision,
  };
  state.fields = newFields();
  state.confirm = false;
  state.notice = null;
  const p = entry?.pricing;
  const values = {
    model: p?.modelKey ?? '',
    input: p?.inputUsdPer1M,
    output: p?.outputUsdPer1M,
    cacheRead: p?.cacheReadUsdPer1M,
    cacheWrite: p?.cacheWriteUsdPer1M,
  };
  for (const name of fields)
    state.fields[name].reset = values[name] === undefined ? '' : String(values[name]);
}

export function capture(state: State, actions: Action[]) {
  const form = state.capture!;
  const name = fields.find((name) => form.values[name] === undefined);
  if (name) {
    const captured = form.observations[name];
    request(
      state,
      actions,
      { kind: 'field', name, captured },
      'reader',
      'read',
      { start: 0, end: captured.snapshot.text.bytes },
      {
        text: {
          child: `insights-field-${state.editing!.key}-${name}`,
          method: 'snapshot',
          input: {
            kind: 'at_revision',
            owner: captured.snapshot.text.owner,
            revision: captured.snapshot.text.revision,
          },
        },
      },
    );
    return;
  }
  state.capture = null;
  try {
    if (
      !state.editing ||
      fields.some(
        (name) =>
          !sameValue(
            state.fields[name].observation?.snapshot.text,
            form.observations[name].snapshot.text,
          ),
      )
    )
      throw Error();
    const rate = (name: FieldName, optional = false) => {
      const text = form.values[name]!.trim();
      if (!text && optional) return undefined;
      if (!/^(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(text)) throw Error();
      const n = Number(text);
      if (!Number.isFinite(n) || n < 0) throw Error();
      return n;
    };
    const modelKey = form.values.model!;
    if (!modelKey || modelKey.trim() !== modelKey || /[\u0000-\u001f\u007f]/.test(modelKey))
      throw Error();
    const pricing: Pricing = {
      modelKey,
      inputUsdPer1M: rate('input')!,
      outputUsdPer1M: rate('output')!,
    };
    const read = rate('cacheRead', true),
      write = rate('cacheWrite', true);
    if (read !== undefined) pricing.cacheReadUsdPer1M = read;
    if (write !== undefined) pricing.cacheWriteUsdPer1M = write;
    mutate(state, actions, {
      expectedRevision: state.editing.revision,
      mutation: { kind: 'upsert', pricing },
    });
  } catch {
    state.notice = 'insights-invalid';
  }
}
