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
import { copy, tuiText } from './locale.js';
import { panelHeader } from '../../shared/page-shell.js';
import { toolLabel } from '../../shared/message-view.js';
import { toolCards } from '../../shared/tool-cards.js';
import { compactLabel, type Node, type Presentation } from '../../shared/view.js';
import { toolPresentation } from '../tool-details/index.js';
import { messages } from './messages.js';
import type { State } from './state.js';

export function outputsView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale);
  const current = state.visits.find((visit) => visit.id === state.selected?.id);
  const readers = state.visits
    .filter((visit) => visit.outputFocus)
    .map((visit) => {
      const active = visit === current;
      const window = active ? state.window : visit.window;
      const displayed = messages(active ? state : { ...state, window, live: [] });
      const rows = toolCards(displayed).filter(
        ({ message }) =>
          (message.role === 'assistant' && message.source !== null) || !!message.tool,
      );
      return ui.box(
        'output-pane-' + visit.key,
        [
          presentation.reading(
            'output-reading-' + visit.key,
            {
              command: visit.outputFocus,
              content: ui.box(
                'output-records',
                [
                  ...(!rows.length
                    ? [ui.text('output-empty', tuiText(locale, 'task-outputs-empty'), theme.muted)]
                    : []),
                  ...rows.map(({ message }) => {
                    const tool = message.tool;
                    const label = tool
                      ? toolLabel(
                          {
                            tool,
                            live: active && !!state.run && !!tool.call?.pending,
                            waiting:
                              active &&
                              !!state.interactions?.entries.some(
                                (entry) => entry.toolUseId === tool.toolUseId,
                              ),
                          },
                          locale,
                        )
                      : null;
                    const expanded = visit.output === message.id;
                    return ui.box(
                      'output-' + message.id,
                      [
                        ...(tool
                          ? [
                              ui.box(
                                'output-tool-heading-' + message.id,
                                [
                                  ui.text(
                                    'output-tool-marker-' + message.id,
                                    theme.ascii ? '*' : '•',
                                    theme[label!.tone],
                                    400,
                                    {},
                                    label!.status,
                                  ),
                                  ui.item!(
                                    'output-item-' + message.id,
                                    ui.text(
                                      'output-label-' + message.id,
                                      `${expanded ? '−' : '+'} ${label?.name ?? text.toolOutput}`,
                                      theme.text,
                                      550,
                                    ),
                                    expanded,
                                    !active,
                                    { submit: true },
                                  ),
                                ],
                                { row: true, gap: 1 },
                              ),
                            ]
                          : [
                              ui.item!(
                                'output-item-' + message.id,
                                ui.text(
                                  'output-reply-' + message.id,
                                  compactLabel(message.text, 180),
                                  theme.text,
                                ),
                                false,
                                !active,
                                { submit: true },
                              ),
                            ]),
                        ...(tool &&
                        toolPresentation(tool.call?.presentation) &&
                        window &&
                        window.page.through > 0
                          ? [
                              presentation.slot(
                                'output-details-' + visit.key + '-' + message.id,
                                'tools',
                                {
                                  theme,
                                  locale,
                                  context: { sessionId: visit.id, through: window.page.through },
                                  tool,
                                  expanded,
                                  focusOnPointer: true,
                                },
                                {},
                              ),
                            ]
                          : tool
                            ? [
                                ui.text(
                                  'output-summary-' + message.id,
                                  compactLabel(message.text, 120),
                                  theme.muted,
                                ),
                              ]
                            : []),
                        ...(message.source !== null
                          ? [
                              ui.button(
                                (tool?.result ? 'output-result-' : 'output-source-') +
                                  message.source,
                                text.readSource,
                                !active || state.loading,
                                false,
                                { background: 0 },
                              ),
                            ]
                          : []),
                      ],
                      { gap: 0 },
                    );
                  }),
                ],
                { gap: 1, focusGroup: true, scroll: 'anchor', terminal: { grow: 1 } },
              ),
            },
            { input: 'input' },
          ),
        ],
        { visible: active, gap: 0, terminal: { grow: 1 } },
      );
    });
  return ui.box(
    'outputs-panel',
    [
      panelHeader(
        presentation,
        { theme, locale },
        'outputs-heading',
        tuiText(locale, 'task-outputs'),
        'inspector-close',
      ),
      ui.text('outputs-explainer', tuiText(locale, 'task-evidence-hint'), theme.muted),
      ...readers,
      ui.box(
        'outputs-navigation',
        [
          ...(state.window?.page.before
            ? [ui.button('outputs-older', text.older, state.loading)]
            : []),
          ...(state.window?.page.after
            ? [ui.button('outputs-newer', text.newer, state.loading)]
            : []),
          ...(state.window &&
          (!state.window.latest || state.latestKnown > state.window.page.through)
            ? [ui.button('outputs-latest', text.latest, state.loading)]
            : []),
        ],
        { row: true, terminal: { wrap: true } },
      ),
    ],
    { visible: state.inspector === 'outputs' && !state.dialog, gap: 1, terminal: { grow: 1 } },
  );
}
