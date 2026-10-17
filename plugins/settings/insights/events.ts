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
  ComponentInput,
  EditorObservation,
  Emission,
  OperationReply,
  PricingPage,
  PricingUpdate,
  SettingsUsagePage,
  SettingsUsageSummary,
  TextChunk,
  UsageSelection,
} from '../../../packages/plugin-sdk/src/index.js';
import { observeBookmark, retryBookmark } from '../../shared/bookmark.js';
import { type InsightsProps } from '../../shared/contracts.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';
import {
  activity,
  capture,
  edit,
  history,
  initialize,
  mutate,
  prices,
  refresh,
  request,
  settle,
  summary,
} from './operations.js';
import { fields, savedView, type FieldName, type State, type Tab } from './state.js';
import { frame } from './view.js';

export function handleEvent<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  input: ComponentInput,
  props: InsightsProps,
) {
  state.revision++;
  const actions: Action[] = [],
    events: Emission[] = [];
  if (input.kind === 'component' && input.child === 'insights-bookmark') {
    const restored = observeBookmark(
      state.bookmark,
      input,
      'insights-bookmark',
      state.savedView,
      savedView,
    );
    if (restored) state.savedView = restored;
  } else if (
    input.kind === 'component' &&
    input.child === 'insights-search-input' &&
    input.name === 'changed'
  ) {
    const field = state.search,
      observed = input.event as EditorObservation;
    if (
      field.observation &&
      field.observation.snapshot.text.owner !== observed.snapshot.text.owner
    ) {
      field.command = null;
      field.ready = false;
      field.reset = state.selection.search ?? '';
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
    } else initialize(field);
  } else if (input.kind === 'component' && state.editing) {
    const name = fields.find(
      (name) => input.child === `insights-field-${state.editing!.key}-${name}`,
    );
    if (name && input.name === 'changed') {
      const field = state.fields[name],
        observed = input.event as EditorObservation;
      if (
        field.observation &&
        field.observation.snapshot.text.owner !== observed.snapshot.text.owner
      ) {
        field.command = null;
        field.ready = false;
        state.notice = 'insights-editor-changed';
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
        if (!field.ready) state.notice = 'insights-unavailable';
      } else initialize(field);
    }
  } else if (input.kind === 'action' && input.key === state.pending?.key) {
    const r = state.pending.request,
      result = input.event as ActionOutcome;
    state.pending = null;
    if (r.kind === 'mutate') {
      if (result.status === 'operation') settle(state, result.reply, actions);
      else if (result.status === 'not_accepted') {
        state.current = null;
        state.notice = 'insights-unavailable';
      } else state.notice = 'insights-unknown';
    } else if (r.kind === 'receipt') {
      if (result.status === 'succeeded') settle(state, result.value as OperationReply, actions);
      else state.notice = 'insights-unknown';
    } else if (result.status !== 'succeeded') {
      state.capture = null;
      state.notice = 'insights-unavailable';
    } else if (r.kind === 'history') {
      const items = result.value as CallerIntent[];
      for (const item of items) {
        const recovery = item.recovery as {
          kind?: string;
          input?: PricingUpdate;
        } | null;
        if (
          item.reference.method === 'updatePrice' &&
          item.reference.contract.name === 'maka.settings.Settings' &&
          recovery?.kind === 'insights-price/v1' &&
          recovery.input
        )
          state.current = {
            input: recovery.input,
            reference: item.reference,
          };
      }
      const last = items.at(-1);
      if (last && last.cursor > r.after) history(state, actions, last.cursor);
      else if (last) state.notice = 'insights-unavailable';
      else {
        state.restoring = false;
        if (state.current?.reference)
          request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.current.reference);
        else refresh(state, actions);
      }
    } else if (r.kind === 'search') {
      const chunk = result.value as TextChunk;
      if (
        !sameValue(chunk.metadata, r.captured.snapshot.text) ||
        !sameValue(state.search.observation?.snapshot.text, r.captured.snapshot.text) ||
        new TextEncoder().encode(chunk.text).length > 1024 ||
        /[\u0000-\u001f\u007f]/.test(chunk.text)
      )
        state.notice = 'insights-search-invalid';
      else {
        state.selection.search = chunk.text;
        state.offsets = [0];
        state.detail = null;
        activity(state, actions);
      }
    } else if (r.kind === 'summary') {
      state.report = result.value as SettingsUsageSummary;
      state.range.through = state.report.through;
      if (state.tab === 'activity') activity(state, actions);
      else if (state.tab === 'pricing') prices(state, actions);
    } else if (r.kind === 'activity') state.activity = result.value as SettingsUsagePage;
    else if (r.kind === 'prices') {
      const page = result.value as PricingPage;
      if (page.kind === 'page') state.prices = page;
      else {
        state.prices = null;
        state.notice = 'insights-conflict';
        state.priceOffsets = [0];
      }
    } else if (r.kind === 'field') {
      const chunk = result.value as TextChunk;
      if (
        !state.capture ||
        !sameValue(chunk.metadata, r.captured.snapshot.text) ||
        !sameValue(state.fields[r.name].observation?.snapshot.text, r.captured.snapshot.text)
      ) {
        state.capture = null;
        state.notice = 'insights-invalid';
      } else {
        state.capture.values[r.name] = chunk.text;
        capture(state, actions);
      }
    }
  } else if (input.kind === 'raw' && props.open) {
    let key = input.key,
      value = (input.event as { key?: string }).key;
    if (key.includes(':')) [key, value] = key.split(':') as [string, string];
    if (key === 'back') events.push({ name: 'closed', value: {} });
    else if (key === 'receipt' && !state.pending && state.current?.reference)
      request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.current.reference);
    else if (key === 'view-retry') retryBookmark(state.bookmark);
    else if (
      key === 'history' &&
      !state.pending &&
      (state.restoring || (state.current && !state.current.reference))
    )
      history(state, actions);
    else if (
      !props.busy &&
      !state.pending &&
      !state.current &&
      !state.restoring &&
      !state.capture
    ) {
      if (key.startsWith('tab-') && ['overview', 'activity', 'pricing'].includes(key.slice(4))) {
        state.tab = key.slice(4) as Tab;
        if (state.tab === 'pricing' && !state.prices) prices(state, actions);
        else if (state.tab === 'activity' && !state.activity) activity(state, actions);
      } else if (
        key === 'view-save' &&
        true &&
        state.bookmark.observed?.state.kind === 'ready' &&
        !state.bookmark.command
      ) {
        state.savedView = {
          tab: state.tab,
          period: state.period,
          group: state.group,
          selection: { ...state.selection },
        };
      } else if (key === 'view-load' && state.bookmark.saved && !state.bookmark.command) {
        const view = state.bookmark.saved;
        state.tab = view.tab;
        state.period = view.period;
        state.group = view.group;
        state.selection = { ...view.selection };
        state.search.command = null;
        state.search.reset = view.selection.search ?? '';
        state.search.ready = false;
        initialize(state.search);
        refresh(state, actions);
      } else if (key === 'scope-toggle' && state.scope?.sessionId) {
        state.range.sessionId = state.range.sessionId ? null : state.scope.sessionId;
        refresh(state, actions);
      } else if (
        key === 'search-apply' &&
        state.search.ready &&
        !state.search.command &&
        state.search.observation &&
        !state.search.observation.snapshot.marked &&
        state.search.observation.snapshot.text.bytes <= 1024
      ) {
        const captured = state.search.observation;
        request(
          state,
          actions,
          { kind: 'search', captured },
          'reader',
          'read',
          { start: 0, end: captured.snapshot.text.bytes },
          {
            text: {
              child: 'insights-search-input',
              method: 'snapshot',
              input: {
                kind: 'at_revision',
                owner: captured.snapshot.text.owner,
                revision: captured.snapshot.text.revision,
              },
            },
          },
        );
      } else if (key === 'search-apply') state.notice = 'insights-search-invalid';
      else if (key === 'refresh') refresh(state, actions);
      else if (key === 'period' && value && ['1', '7', '30', 'all'].includes(value)) {
        state.period = value;
        refresh(state, actions);
      } else if (key === 'group' && value && ['byProvider', 'byModel', 'byTool'].includes(value)) {
        state.group = value as State['group'];
        state.groupOffsets = [0];
        summary(state, actions);
      } else if (key === 'group-prev' && state.groupOffsets.length > 1) {
        state.groupOffsets.pop();
        summary(state, actions);
      } else if (key === 'group-next' && state.report?.nextOffset != null) {
        state.groupOffsets.push(state.report.nextOffset);
        summary(state, actions);
      } else if (key === 'kind' && value && ['all', 'model', 'tool'].includes(value)) {
        state.selection.kind = value === 'all' ? null : (value as 'model' | 'tool');
        state.offsets = [0];
        state.detail = null;
        activity(state, actions);
      } else if (
        key === 'status' &&
        value &&
        ['all', 'success', 'error', 'aborted', 'unknown', 'rejected'].includes(value)
      ) {
        state.selection.status = value === 'all' ? null : (value as UsageSelection['status']);
        state.offsets = [0];
        state.detail = null;
        activity(state, actions);
      } else if (key === 'activity-next' && state.activity?.nextOffset != null) {
        state.offsets.push(state.activity.nextOffset);
        activity(state, actions);
      } else if (key === 'activity-prev' && state.offsets.length > 1) {
        state.offsets.pop();
        activity(state, actions);
      } else if (key.startsWith('attempt-'))
        state.detail = state.activity?.attempts[Number(key.slice(8))] ?? null;
      else if (key === 'detail-close') state.detail = null;
      else if (key === 'price-refresh') {
        state.priceOffsets = [0];
        prices(state, actions);
      } else if (key === 'prices-next' && state.prices?.nextOffset != null) {
        state.priceOffsets.push(state.prices.nextOffset);
        prices(state, actions, state.prices.nextOffset);
      } else if (key === 'prices-prev' && state.priceOffsets.length > 1) {
        state.priceOffsets.pop();
        prices(state, actions, state.priceOffsets.at(-1)!);
      } else if (key === 'price-new') edit(state, null);
      else if (/^price-\d+$/.test(key)) {
        const entry = state.prices?.entries[Number(key.slice(6))];
        if (entry) edit(state, entry);
      } else if (key === 'price-close') {
        state.editing = null;
        state.confirm = false;
      } else if (key === 'price-remove' && state.editing?.entry?.source === 'custom')
        state.confirm = true;
      else if (
        key === 'price-confirm' &&
        state.confirm &&
        state.editing?.entry?.source === 'custom'
      )
        mutate(state, actions, {
          expectedRevision: state.editing.revision,
          mutation: {
            kind: 'delete',
            modelKey: state.editing.entry.pricing.modelKey,
          },
        });
      else if (key === 'price-save' && state.editing) {
        if (
          fields.some(
            (name) =>
              !state.fields[name].ready ||
              state.fields[name].command ||
              !state.fields[name].observation ||
              state.fields[name].observation!.snapshot.marked ||
              state.fields[name].observation!.snapshot.text.bytes > (name === 'model' ? 512 : 64),
          )
        )
          state.notice = 'insights-invalid';
        else {
          state.capture = {
            observations: Object.fromEntries(
              fields.map((name) => [name, state.fields[name].observation!]),
            ) as Record<FieldName, EditorObservation>,
            values: {},
          };
          capture(state, actions);
        }
      }
    }
  }
  return frame(presentation, state, props, actions, events);
}
