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

import { initialLanguageObservation, readLanguage } from '../../shared/language-preference.js';
import { initialAppearance } from '../../shared/appearance-editor.js';
import type { Action, Component } from '../../../packages/plugin-sdk/src/index.js';
import type { AgentScreenProps } from '../../shared/contracts.js';
import { bookmark } from '../../shared/bookmark.js';
import { type Presentation, type Node } from '../../shared/view.js';

import { newDiagnostics } from './diagnostics.js';
import { newGoalState } from './goal.js';
import { newPlanState } from './plan.js';
import { newGraphState } from './graph.js';
import { type State } from './state.js';
import { catalog } from './actions.js';
import { frame } from './frame.js';
import { handleEvent } from './events.js';

export function agentScreen<N extends Node>(
  presentation: Presentation<N>,
  terminals = false,
  place = '',
  skills = false,
  scheduledTasks = false,
  web = false,
  jev = false,
  graphs = false,
  hostLogs = false,
): Component<State, AgentScreenProps, N> {
  return {
    mount(props, values) {
      const state: State = {
        embedding: props,
        externalSelection: null,
        place,
        preferences: initialAppearance(),
        language: initialLanguageObservation(),
        recoveryCheckpoint: bookmark({ through: 0, scope: null }),
        historyScope: null,
        restoreStarted: false,
        restoreThrough: 0,
        recoveryDirty: false,
        settledOperations: [],
        bookmark: bookmark({ pages: [], category: 'appearance' }),
        revision: 0,
        attachments: { files: [], selection: null, paste: null },
        drafts: [],
        serial: 0,
        epoch: 0,
        catalog: null,
        catalogGeneration: 0,
        catalogWatch: null,
        catalogFilter: 'all',
        catalogFailed: false,
        selected: null,
        model: null,
        backendReady: false,
        backendPreference: null,
        executor: null,
        hasBackends: false,
        modelSetup: 'loading',
        modelOpenRequest: null,
        connectionSetupRequest: null,
        catalogEpoch: 0,
        modelRefreshing: false,
        pages: [],
        openedPages: [],
        pageChrome: {},
        pageBack: {},
        recallAnchor: null,
        scheduledTasks,
        web,
        jev,
        goal: newGoalState(),
        plan: newPlanState(),
        graph: newGraphState(graphs),
        recap: { receipt: null, loaded: false, loading: null, failed: false, confirmation: null },
        checklist: { watch: null, page: null, failed: false },
        scheduleAlerts: { pending: 0, reminders: [] },
        scheduleWatch: scheduledTasks ? 'schedule-alerts-initial' : null,
        scheduleRequested: null,
        settingsCategory: 'appearance',
        insightsScope: null,
        searchOpen: false,
        browse: null,
        sidebar: { command: null },
        terminals,
        skills,
        dialog: null,
        sessionChange: null,
        diagnostics: newDiagnostics(),
        hostLogsEnabled: hostLogs,
        inspector: null,
        image: null,
        window: null,
        visits: [],
        source: null,
        navigation: 0,
        landing: { sequence: 0, edge: 'end' },
        tailReceipt: null,
        compact: null,
        compacting: false,
        fileContext: null,
        copyRequest: null,
        messageCopy: null,
        followAttempt: null,
        parts: [],
        run: null,
        metrics: null,
        queue: null,
        queueEdit: null,
        queueDraft: null,
        interactions: null,
        interaction: null,
        questionSubmission: null,
        runThrough: 0,
        loading: false,
        live: [],
        latestKnown: 0,
        watch: null,
        pending: {},
        uncertain: null,
        restoring: true,
        restoreAfter: 0,
        notice: null,
      };
      const actions: Action[] = [];
      catalog(state);
      readLanguage(state.language, actions);

      return frame(presentation, state, values, actions);
    },
    update(state, props, values) {
      const actions: Action[] = [];
      if (props.languageRevision !== state.embedding.languageRevision)
        readLanguage(state.language, actions);
      if (state.embedding.fixed && state.embedding.session?.id !== props.session?.id) {
        state.bookmark = bookmark({ pages: [], category: 'appearance' });
        state.pages = [];
        state.settingsCategory = 'appearance';
      }
      return frame(
        presentation,
        { ...state, embedding: props, revision: state.revision + 1 },
        values,
        actions,
      );
    },
    event(previous, input, props, values) {
      return handleEvent(presentation, previous, input, props, values);
    },
  };
}
