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

import { tuiText, copy } from './locale.js';
import type { Theme } from '../../shared/contracts.js';
import { type Presentation, type Node } from '../../shared/view.js';

import { type State } from './state.js';
import { catalogPending, mutationPending, rootCommands } from './actions.js';

export function navigationView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    busy = mutationPending(state);
  const catalogBusy = busy || catalogPending(state);
  const commands = rootCommands(state, locale);
  const createButton = ui.button(
    'create',
    '+  ' + tuiText(locale, 'sidebar-new-session'),
    !commands.some((command) => command.kind === 'create' && !command.disabled),
    true,
    { background: 0, align: 'start' },
  );
  const createEntry = ui.contextMenu('navigation-create-menu', text.new, createButton, [
    ui.button(
      'create-workspace',
      text.newWorkspace,
      !commands.some((command) => command.kind === 'create-workspace' && !command.disabled),
    ),
  ]);
  const navigationLink = (
    key: string,
    label: string,
    glyph: string,
    ascii: string,
    current = false,
  ) =>
    ui.button(key, `${theme.ascii ? ascii : glyph}  ${label}`, false, false, {
      background: current ? theme.selected : 0,
      align: 'start',
    });
  const navigationApps = !state.embedding.fixed
    ? [
        ui.box('navigation-apps-gap', [], { terminal: { basis: 1 } }),
        navigationLink('apps-open', text.appsTitle, '▦', '#', state.pages.at(-1) === 'apps'),
        ...(state.embedding.workhub ? [navigationLink('workhub-open', 'WorkHub', '◈', '*')] : []),
        ...(state.scheduledTasks
          ? [
              navigationLink(
                'scheduler-open',
                text.scheduleTitle +
                  (state.scheduleAlerts.pending ? ` (${state.scheduleAlerts.pending})` : ''),
                '⏲',
                'T',
                state.pages.at(-1) === 'scheduler',
              ),
            ]
          : []),
        navigationLink('recall-open', text.recallTitle, '⌕', '/', state.pages.at(-1) === 'recall'),
        navigationLink(
          'imports-open',
          text.importsTitle,
          '⇥',
          '>',
          state.pages.at(-1) === 'imports',
        ),
      ]
    : [];
  const navigation = ui.box(
    'navigation-panel',
    [
      ui.box(
        'navigation-heading',
        [
          ui.box('navigation-create-cell', [createEntry], { terminal: { grow: 1 } }),
          ui.button(
            'navigation-back',
            theme.ascii ? 'x' : '×',
            false,
            false,
            { background: 0, align: 'start' },
            undefined,
            text.hideSidebar,
          ),
        ],
        { row: true, gap: 0 },
      ),
      ui.box(
        'catalog-filters',
        [
          ...(['all', 'needs_you', 'archived'] as const).map((filter) => {
            const label =
              filter === 'archived'
                ? text.archivedSessions
                : filter === 'all'
                  ? tuiText(locale, 'sidebar-all')
                  : tuiText(locale, 'sidebar-needs-you') +
                    (state.catalog?.needsYouCount ? ` · ${state.catalog.needsYouCount}` : '');
            const selected = state.catalogFilter === filter;
            return ui.item(
              `catalog-filter-${filter}`,
              ui.text(`catalog-filter-label-${filter}`, label, theme.text),
              selected,
              catalogBusy,
            );
          }),
          ui.button(
            'catalog-search',
            `${theme.ascii ? '/' : '⌕'} ${tuiText(locale, 'sidebar-search')}`,
            false,
            false,
            { background: 0, align: 'start' },
            undefined,
            tuiText(locale, 'sidebar-search-sessions'),
          ),
        ],
        { row: true, focusGroup: true, gap: 2 },
      ),
      ui.box('catalog-gap', [], { terminal: { basis: 1 } }),
      ui.box(
        'catalog-body',
        [
          ...(!state.catalogFailed && !catalogPending(state) && state.catalog?.sessions.length === 0
            ? [
                ui.box(
                  'catalog-empty-inset',
                  [
                    ui.text(
                      'catalog-empty',
                      '   ' +
                        (state.catalogFilter === 'archived'
                          ? text.noArchivedSessions
                          : state.catalogFilter === 'needs_you'
                            ? text.noneNeedsYou
                            : tuiText(locale, 'sidebar-empty')),
                      theme.muted,
                    ),
                  ],
                  { padding: 0, terminal: { shrink: true } },
                ),
              ]
            : []),
          ...(state.catalogFailed
            ? [ui.text('catalog-error', text.sessionsUnavailable, theme.muted)]
            : []),
          ui.box(
            'catalog-controls',
            [
              ...(state.catalogFailed
                ? [ui.button('catalog-refresh', text.refreshSessions, catalogBusy)]
                : []),
            ],
            { row: true, terminal: { wrap: true } },
          ),
          presentation.slot(
            'navigation',
            'navigation',
            {
              theme,
              locale,
              sessions: state.catalog?.sessions ?? [],
              selected: state.selected?.id ?? null,
              busy,
              loading: catalogPending(state),
              older: !!state.catalog?.before,
              newer: !!state.catalog?.after,
            },
            { select: 'select', older: 'older', newer: 'newer' },
          ),
        ],
        { gap: 0, scroll: 'none', terminal: { grow: 1 } },
      ),
      ui.box(
        'navigation-footer',
        [
          ...navigationApps,
          ...(!state.embedding.fixed
            ? [
                navigationLink(
                  'projects-open',
                  text.projectsTitle,
                  '▦',
                  'P',
                  state.pages.at(-1) === 'projects',
                ),
              ]
            : []),
          navigationLink(
            'settings',
            tuiText(locale, 'route-settings'),
            '⛭',
            'S',
            state.pages.at(-1) === 'settings',
          ),
        ],
        { gap: 0 },
      ),
    ],
    { surface: false, gap: 0, padding: 1, terminal: { grow: 1 } },
  );

  return navigation;
}
