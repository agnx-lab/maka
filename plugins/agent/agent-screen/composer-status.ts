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

import type { Theme } from '../../shared/contracts.js';
import { copy, goalStatuses, tuiText } from './locale.js';
import { compactLabel, type Node, type Presentation } from '../../shared/view.js';
import { mutationPending, reviewTarget, rootCommands } from './actions.js';
import { goalTerminal } from './goal.js';
import type { State } from './state.js';

// Actions that need a reply stay next to the editor; settled outcomes belong to the transcript.
export function composerStatus<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale);
  const review = reviewTarget(state);
  const pendingPlan = state.plan.page?.current.proposal?.status === 'pending_approval';
  const question = review && ['question', 'form'].includes(review.kind);
  const attention = presentation.paint({
    ...theme,
    accent: question ? theme.accent : theme.warning,
    text: question ? theme.accent : theme.warning,
  });
  const controls: N[] = [];
  if (state.checklist.page?.enabled && state.checklist.page.total > 0)
    controls.push(
      ui.button(
        'checklist-open',
        `${text.checklistTitle} ${state.checklist.page.completed}/${state.checklist.page.total}`,
        !!state.dialog,
        false,
        { background: 0 },
        undefined,
        state.checklist.page.current ? compactLabel(state.checklist.page.current.content, 160) : '',
      ),
    );
  const goal = state.goal.page?.current;
  if (goal && !goalTerminal(goal.status) && state.inspector !== 'goal' && state.dialog !== 'goal')
    controls.push(
      ui.button(
        'goal-open',
        `${text.goalTitle} · ${text[goalStatuses[goal.status]]}`,
        !!state.dialog,
        false,
        { background: 0 },
        undefined,
        compactLabel(goal.objective, 160),
      ),
    );
  if (
    !pendingPlan &&
    (state.selected?.collaborationMode === 'plan' ||
      state.plan.page?.current.execution?.phase === 'active') &&
    state.inspector !== 'plan' &&
    state.dialog !== 'plan'
  )
    controls.push(
      ui.button(
        'plan-open',
        state.plan.page?.current.execution?.phase === 'active'
          ? text.planExecution_active
          : text.planPlanning,
        !!state.dialog,
        false,
        { background: 0 },
      ),
    );
  if (state.graph.enabled && state.inspector !== 'graph' && state.dialog !== 'graph')
    controls.push(
      ui.button('graph-open', text.graphTitle, !!state.dialog, false, { background: 0 }),
    );
  const notice =
    state.notice && !['completed', 'failed', 'stopping'].includes(state.notice)
      ? text[state.notice]
      : '';
  const open = state.dialog === 'approval' || state.dialog === 'question';
  return ui.box(
    'composer-status',
    [
      ...(controls.length
        ? [
            ui.box('conversation-controls', controls, {
              row: true,
              gap: 1,
              terminal: { wrap: true },
            }),
          ]
        : []),
      ...(!open && (review || pendingPlan)
        ? [
            ui.box(
              'composer-attention',
              [
                ui.box('attention-leading', [], { terminal: { grow: 1 } }),
                attention.button(
                  review ? 'interaction-open' : 'plan-open',
                  `${theme.ascii ? '!' : '△'} ${tuiText(locale, question ? 'chat-reply-needed' : 'chat-approval-needed')}${review ? ' · Ctrl+G' : ''}`,
                  review
                    ? !rootCommands(state, locale).some(
                        (command) => command.kind === 'interaction-open' && !command.disabled,
                      )
                    : !!state.dialog,
                  false,
                  { background: 0 },
                ),
                ui.box('attention-trailing', [], { terminal: { grow: 1 } }),
              ],
              { row: true, gap: 0 },
            ),
          ]
        : []),
      ...(state.uncertain
        ? [
            ui.button(
              'check',
              text.check,
              Object.values(state.pending).some((request) => request.kind === 'receipt'),
              false,
              { background: 0 },
            ),
          ]
        : notice
          ? [ui.text('composer-notice', notice, theme.muted, 400, { align: 'center' })]
          : []),
      ...(state.notice === 'unavailable'
        ? [
            ui.button('reload', text.reloadView, state.loading || mutationPending(state), false, {
              background: 0,
            }),
          ]
        : []),
    ],
    { gap: 0, visible: !!state.selected && !state.source && !state.image },
  );
}
