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
import { tuiText, copy } from './locale.js';
import { type Presentation, type Node } from '../../shared/view.js';

import { type State } from './state.js';
import { mutationPending, conversationRoute } from './actions.js';

export function headerView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
  title: N,
  models: N,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    busy = mutationPending(state);
  const currentPage = state.pages.at(-1);
  const header = ui.box(
    'header',
    [
      ui.box(
        'header-left',
        [
          ...(!state.embedding.fixed
            ? [
                ui.box(
                  'navigation-cell',
                  [
                    ui.button(
                      'navigation-toggle',
                      theme.ascii ? '=' : '≡',
                      false,
                      false,
                      { background: 0 },
                      undefined,
                      text.toggleSidebar + ' · Ctrl+B',
                    ),
                  ],
                  { terminal: { basis: 3 } },
                ),
              ]
            : []),
          ...(currentPage
            ? [
                ui.box(
                  'back-cell',
                  [
                    ui.button(
                      'page-back',
                      theme.ascii ? '<' : '‹',
                      currentPage !== 'settings' &&
                        state.pageChrome[currentPage]?.backEnabled !== true,
                      false,
                      { background: 0 },
                      undefined,
                      text.back,
                    ),
                  ],
                  { terminal: { basis: 3 } },
                ),
              ]
            : []),
        ],
        {
          visible: !state.embedding.fixed || !conversationRoute(state),
          row: true,
          gap: 0,
          terminal: { basis: (state.embedding.fixed ? 0 : 3) + (currentPage ? 3 : 0) },
        },
      ),
      ui.box(
        'header-task',
        [
          ...(!conversationRoute(state)
            ? [ui.box('header-title', [title], { terminal: { shrink: true } })]
            : []),
          ui.box('header-title-trailing', [], { terminal: { grow: 1 } }),
        ],
        { row: true, gap: 1, terminal: { grow: 1, shrink: true } },
      ),
      ui.box(
        'toolbar',
        [
          ...[
            ['diagnostics-open', text.diagnosticsTitleLabel, '!'],
            ['outputs-open', tuiText(locale, 'task-outputs'), theme.ascii ? '[o]' : '▧'],
            ['details-open', text.detailsOpen, theme.ascii ? '(i)' : 'ⓘ'],
            ['terminal-open', text.terminalTitle, '>_'],
            ['palette-open', text.paletteTitle, theme.ascii ? ':' : '⌘'],
          ]
            .filter(([key]) => key === 'palette-open' || conversationRoute(state))
            .map(([key, label, icon]) =>
              ui.box(
                `${key}-cell`,
                [
                  ui.button(
                    key!,
                    icon!,
                    key !== 'palette-open' &&
                      !state.selected &&
                      !(key === 'diagnostics-open' && state.hostLogsEnabled),
                    false,
                    { background: 0 },
                    undefined,
                    label + (key === 'palette-open' ? ' · Ctrl+K' : ''),
                  ),
                ],
                { terminal: { basis: 4, shrink: false } },
              ),
            ),
        ],
        { row: true, gap: 1, terminal: { shrink: true, wrap: true } },
      ),
      ...(!conversationRoute(state) || !state.selected
        ? [
            ...(state.notice && !['completed', 'failed', 'stopping'].includes(state.notice)
              ? [ui.text('page-notice', text[state.notice], theme.muted)]
              : []),
            ...(state.uncertain
              ? [
                  ui.button(
                    'check',
                    text.check,
                    Object.values(state.pending).some((request) => request.kind === 'receipt'),
                  ),
                ]
              : []),
            ...(state.notice === 'unavailable'
              ? [ui.button('reload', text.reloadView, state.loading || busy)]
              : []),
          ]
        : []),
    ],
    { row: true, gap: 0 },
  );

  return header;
}
