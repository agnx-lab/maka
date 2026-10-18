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

import { observeLanguage, readLanguage } from '../../shared/language-preference.js';
import type {
  Action,
  ActionOutcome,
  AgentDeliveryPage,
  AgentScheduleAlerts,
  AgentSessionPage,
  AgentChecklist,
  AgentGoalView,
  AgentPlanView,
  ComponentInput,
  ComponentValues,
  Emission,
} from '../../../packages/plugin-sdk/src/index.js';
import { appearanceEvent } from '../../shared/appearance-editor.js';
import { hostLogUpdate } from './host-logs.js';
import { diagnosticChange } from './diagnostics.js';
import { observeBookmark, retryBookmark } from '../../shared/bookmark.js';
import type { AgentScreenProps, Theme } from '../../shared/contracts.js';
import { settingsCategory } from '../../shared/settings.js';

import { type Node, type Presentation } from '../../shared/view.js';

import { goalUpdate } from './goal.js';
import { planUpdate } from './plan.js';
import { graphUpdate } from './graph.js';
import type { WorkHubGraphView } from '../../../packages/plugin-sdk/src/index.js';
import { messages } from './messages.js';
import { toolCards } from '../../shared/tool-cards.js';
import { follow, select, intent } from './actions.js';
import { handleComponent } from './component-events.js';
import { frame } from './frame.js';
import { handleRaw } from './raw-events.js';
import { delivery, queryFailed, querySucceeded, receipt } from './results.js';
import {
  readingKey,
  navigationBookmark,
  navigationKey,
  type Navigation,
  type State,
} from './state.js';

export function handleEvent<N extends Node>(
  presentation: Presentation<N>,
  previous: State,
  input: ComponentInput,
  props: AgentScreenProps,
  values: ComponentValues,
) {
  // Callbacks receive detached committed state. Mutations here belong only to
  // the candidate frame and publish with its intents in one UI transaction.
  const state = previous;
  state.embedding = props;
  {
    observeBookmark(
      state.recoveryCheckpoint,
      input,
      'recovery-checkpoint',
      { through: state.restoreThrough, scope: state.historyScope },
      (value) => {
        const saved = value as { through?: number; scope?: string | null } | null;
        return saved &&
          Number.isSafeInteger(saved.through) &&
          saved.through! >= 0 &&
          (saved.scope === null || (typeof saved.scope === 'string' && saved.scope.length <= 4096))
          ? { through: saved.through!, scope: saved.scope }
          : null;
      },
    );
    if (input.kind === 'raw' && input.key === 'navigation-checkpoint-retry')
      retryBookmark(state.bookmark);
    const restored = observeBookmark(
      state.bookmark,
      input,
      navigationKey(state),
      navigationBookmark(state),
      (value) => {
        const item = value as Navigation | null;
        return item &&
          Array.isArray(item.pages) &&
          item.pages.length <= 3 &&
          item.pages.every((page) =>
            [
              'settings',
              'connections',
              'deployment',
              'projects',
              'imports',
              'apps',
              'recall',
              'scheduler',
            ].includes(page),
          ) &&
          settingsCategory(item.category) !== null
          ? { ...item, category: settingsCategory(item.category)! }
          : null;
      },
    );
    if (restored) {
      state.pages = restored.pages;
      state.settingsCategory = restored.category;
    }
  }
  if (input.kind === 'raw' && input.key === 'outputs-latest')
    input = { ...input, key: 'task-latest' };
  if (
    input.kind === 'component' &&
    input.child === 'output-reading-' + readingKey(state) &&
    input.name === 'input'
  )
    input = input.event as ComponentInput;
  if (input.kind === 'raw' && state.inspector === 'outputs' && !state.dialog) {
    if (
      input.key.startsWith('output-item-') &&
      (input.event as { kind?: string }).kind === 'submit'
    ) {
      const visit = state.visits.find((entry) => entry.id === state.selected?.id);
      const id = input.key.slice('output-item-'.length);
      const row = toolCards(messages(state)).find(({ message }) => message.id === id)?.message;
      if (visit && row?.tool) visit.output = visit.output === id ? null : id;
      else if (row?.source != null && !state.loading) {
        state.inspector = null;
        input = {
          kind: 'component',
          key: input.key,
          child: readingKey(state),
          name: 'source',
          event: { position: row.source, field: 'body' },
        };
      }
    } else if (input.key.startsWith('output-result-') || input.key.startsWith('output-source-')) {
      const result = input.key.startsWith('output-result-');
      const id = input.key.slice(result ? 'output-result-'.length : 'output-source-'.length);
      const row = toolCards(messages(state)).find(
        ({ message }) => message.contentKey === id,
      )?.message;
      if (row?.source != null && !state.loading) {
        state.inspector = null;
        input = {
          kind: 'component',
          key: input.key,
          child: readingKey(state),
          name: result ? 'toolSource' : 'source',
          event: { position: row.source, field: result ? 'result' : 'body' },
        };
      }
    } else if (input.key === 'outputs-older' || input.key === 'outputs-newer') {
      input = {
        kind: 'component',
        key: input.key,
        child: readingKey(state),
        name: input.key === 'outputs-older' ? 'older' : 'newer',
        event: {},
      };
    }
  }
  if (input.kind === 'component' && input.child === 'attachment-list' && input.name === 'input')
    input = input.event as ComponentInput;
  if (input.kind === 'component' && input.child === 'composer' && input.name === 'attachments')
    input = { kind: 'raw', key: 'attachments-open', event: {} };
  if (input.kind === 'raw' && input.key === 'attachments-summary')
    input = { ...input, key: 'attachments-open' };
  if (input.kind === 'raw' && input.key === 'attachment-browser-close')
    input = { ...input, key: 'attachment-close' };
  state.revision++;
  const actions: Action[] = [];
  const events: Emission[] = [];
  const previousLanguage = state.language.observed;
  if (observeLanguage(state.language, input, actions)) {
    if ((input.event as ActionOutcome).status !== 'succeeded') {
      state.language.observed = null;
      state.notice = 'unavailable';
    } else if (
      previousLanguage?.language !== state.language.observed?.language ||
      previousLanguage?.resolvedLanguage !== state.language.observed?.resolvedLanguage
    )
      events.push({ name: 'languageChanged', value: {} });
    return frame(presentation, state, values, actions, events);
  }
  if (
    (input.kind === 'raw' &&
      ['settings', 'settings-retry', 'check', 'settings-category-appearance'].includes(
        input.key,
      )) ||
    (input.kind === 'component' &&
      input.child === 'assistant-preferences' &&
      input.name === 'changed')
  )
    readLanguage(state.language, actions);
  const locale = state.language.observed?.resolvedLanguage ?? 'en';
  if (appearanceEvent(state.preferences, input, values.appearance?.[0] as Theme, locale, actions))
    return frame(presentation, state, values, actions);
  if (input.kind === 'action') {
    const outcome = input.event as ActionOutcome;
    if (state.diagnostics.host.watch?.key === input.key) {
      if (outcome.status === 'updated')
        hostLogUpdate(
          state,
          outcome.value as import('../../../packages/plugin-sdk/src/index.js').HostLogPage,
        );
      else if (outcome.status === 'failed' || outcome.status === 'not_accepted')
        hostLogUpdate(state, null);
    } else if (state.diagnostics.watch?.key === input.key) {
      if (outcome.status === 'updated')
        diagnosticChange(
          state,
          actions,
          outcome.value as import('../../../packages/plugin-sdk/src/index.js').AgentDiagnosticChange,
        );
      else if (outcome.status === 'failed' || outcome.status === 'not_accepted') {
        state.diagnostics.watch = null;
        state.diagnostics.failed = true;
      }
    } else if (state.graph.watch?.key === input.key) {
      if (outcome.status === 'updated') graphUpdate(state, outcome.value as WorkHubGraphView);
      else {
        state.graph.watch = null;
        state.graph.failed = true;
      }
    } else if (state.plan.watch?.key === input.key) {
      if (outcome.status === 'updated') planUpdate(state, outcome.value as AgentPlanView);
      else if (outcome.status === 'failed' || outcome.status === 'not_accepted') {
        state.plan.watch = null;
        state.plan.failed = true;
      }
    } else if (state.goal.watch?.key === input.key) {
      if (outcome.status === 'updated') goalUpdate(state, outcome.value as AgentGoalView);
      else if (outcome.status === 'failed' || outcome.status === 'not_accepted') {
        state.goal.watch = null;
        state.goal.failed = true;
      }
    } else if (state.checklist.watch?.key === input.key) {
      if (outcome.status === 'updated') state.checklist.page = outcome.value as AgentChecklist;
      else if (outcome.status === 'failed' || outcome.status === 'not_accepted') {
        state.checklist = { watch: null, page: null, failed: true };
      }
    } else if (state.scheduleWatch === input.key) {
      if (outcome.status === 'updated') state.scheduleAlerts = outcome.value as AgentScheduleAlerts;
      else if (outcome.status === 'failed' || outcome.status === 'not_accepted')
        state.scheduleWatch = null;
    } else if (state.catalogWatch?.key === input.key) {
      if (outcome.status === 'updated') {
        state.catalogWatch.loading = false;
        state.catalogFailed = false;
        state.catalog = outcome.value as AgentSessionPage;
        if (
          !state.selected &&
          !state.embedding.session &&
          !state.embedding.fixed &&
          state.catalog.sessions.length
        )
          select(state, actions, state.catalog.sessions[0]);
        else if (state.selected) {
          const selected = state.catalog.sessions.find(
            (session) => session.id === state.selected!.id,
          );
          if (selected && selected.revision >= state.selected.revision) state.selected = selected;
        }
      } else if (outcome.status === 'failed' || outcome.status === 'not_accepted') {
        state.catalogWatch = null;
        state.catalogFailed = true;
      }
    } else if (state.watch?.key === input.key) {
      if (outcome.status === 'updated')
        delivery(state, outcome.value as AgentDeliveryPage, actions);
      else if (outcome.status === 'failed' || outcome.status === 'not_accepted') {
        state.watch = null;
        state.notice = 'unavailable';
      }
    } else {
      const request = state.pending[input.key];
      if (request) {
        delete state.pending[input.key];
        if (
          outcome.status === 'operation' &&
          (request.kind === 'languageChange' ||
            request.kind === 'changeSession' ||
            request.kind === 'selectBackend' ||
            request.kind === 'sandboxChange' ||
            request.kind === 'create' ||
            request.kind === 'stop' ||
            request.kind === 'queue' ||
            request.kind === 'respond' ||
            request.kind === 'markRead' ||
            request.kind === 'uploadAttachment' ||
            request.kind === 'compact' ||
            request.kind === 'recapGenerate' ||
            request.kind === 'goalChange' ||
            request.kind === 'graphChange' ||
            request.kind === 'planChange' ||
            request.kind === 'planMode')
        )
          receipt(state, actions, request, outcome.reply);
        else if (request.kind === 'languageChange') {
          state.notice = outcome.status === 'not_accepted' ? 'unavailable' : 'unknown';
          if (outcome.status !== 'not_accepted') {
            state.restoring = true;
            state.restoreAfter = state.restoreThrough;
            intent(state, actions, { kind: 'restorePage' }, 'history', 'page', {
              after: state.restoreAfter,
              limit: 1,
            });
          }
        } else if (outcome.status === 'succeeded')
          querySucceeded(state, actions, request, outcome.value);
        else queryFailed(state, request);
        // A newer original paint/intent may have arrived while this read
        // was pending. The last attempted basis prevents self-retry.
        if (request.kind === 'history' && outcome.status === 'succeeded') follow(state, actions);
      }
    }
  } else if (input.kind === 'component') {
    return handleComponent(presentation, state, input, values, actions, events, locale);
  } else if (input.kind === 'raw') {
    return handleRaw(presentation, state, input, values, actions, events, locale);
  }
  return frame(presentation, state, values, actions, events);
}
