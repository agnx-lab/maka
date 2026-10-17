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

import { settingsView } from '../../shared/settings.js';
import type { ComponentValues } from '../../../packages/plugin-sdk/src/index.js';
import type { Theme } from '../../shared/contracts.js';
import { bookmarkReady } from '../../shared/bookmark.js';
import { copy } from './locale.js';
import { type Presentation, type Node } from '../../shared/view.js';

import { type Page, navigationBookmark, navigationKey, type State } from './state.js';
import { catalogPending, mutationPending } from './actions.js';

export function pagesView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
  values: ComponentValues,
  main: N,
): N {
  const currentPage = state.pages.at(-1);
  const ui = presentation.paint(theme),
    text = copy(locale),
    busy = mutationPending(state);
  const pageShell = (page: Page) => ({ shell: { back: state.pageBack[page] ?? 0 } });
  const pageContent = ui.box(
    'page-content',
    [
      main,
      ...(state.openedPages.includes('settings')
        ? [
            ui.box(
              'retained-settings',
              [
                settingsView(
                  presentation,
                  state.preferences,
                  theme,
                  locale,
                  state.settingsCategory,
                  currentPage === 'settings',
                  state.insightsScope,
                  state.web,
                  state.jev,
                  state.embedding.workhub === true,
                ),
              ],
              { visible: currentPage === 'settings', terminal: { grow: 1 } },
            ),
          ]
        : []),
      ...(state.openedPages.includes('deployment')
        ? [
            ui.box(
              'retained-deployment',
              [
                presentation.slot(
                  'deployment',
                  'deployment',
                  {
                    theme,
                    locale,
                    ...pageShell('deployment'),
                    bookmarkKey: navigationKey(state) + '/deployment',
                    navigationReady: bookmarkReady(state.bookmark, navigationBookmark(state)),
                  },
                  { chrome: 'page-chrome', closed: 'deployment-closed' },
                ),
              ],
              { visible: currentPage === 'deployment', gap: 0, terminal: { grow: 1 } },
            ),
          ]
        : []),
      ...(state.openedPages.includes('apps')
        ? [
            ui.box(
              'retained-apps',
              [
                presentation.slot(
                  'apps',
                  'apps',
                  {
                    theme,
                    locale,
                    ...pageShell('apps'),
                    open: state.pages.at(-1) === 'apps',
                    busy: mutationPending(state) || catalogPending(state),
                  },
                  { chrome: 'page-chrome', closed: 'apps-closed', manage: 'apps-manage' },
                ),
              ],
              { visible: currentPage === 'apps', gap: 0, terminal: { grow: 1 } },
            ),
          ]
        : []),
      ...(state.openedPages.includes('scheduler')
        ? [
            ui.box(
              'retained-scheduler',
              [
                presentation.slot(
                  'scheduler',
                  'scheduler',
                  {
                    theme,
                    locale,
                    ...pageShell('scheduler'),
                    open: currentPage === 'scheduler',
                    busy: mutationPending(state) || catalogPending(state),
                    catalogEpoch: state.catalogEpoch,
                    requested: state.scheduleRequested,
                  },
                  {
                    chrome: 'page-chrome',
                    closed: 'scheduler-closed',
                    selected: 'scheduler-selected',
                    connections: 'scheduler-connections',
                  },
                ),
              ],
              { visible: currentPage === 'scheduler', gap: 0, terminal: { grow: 1 } },
            ),
          ]
        : []),
      ...(state.openedPages.includes('recall')
        ? [
            ui.box(
              'retained-recall',
              [
                presentation.slot(
                  'recall',
                  'recall',
                  {
                    theme,
                    locale,
                    ...pageShell('recall'),
                    open: state.pages.at(-1) === 'recall',
                    busy: mutationPending(state) || catalogPending(state),
                  },
                  { chrome: 'page-chrome', closed: 'recall-closed', selected: 'recall-selected' },
                ),
              ],
              { visible: currentPage === 'recall', gap: 0, terminal: { grow: 1 } },
            ),
          ]
        : []),
      ...(state.openedPages.includes('imports')
        ? [
            ui.box(
              'retained-imports',
              [
                presentation.slot(
                  'imports',
                  'imports',
                  {
                    theme,
                    locale,
                    ...pageShell('imports'),
                    open: state.pages.at(-1) === 'imports',
                    busy: mutationPending(state) || catalogPending(state),
                  },
                  {
                    chrome: 'page-chrome',
                    closed: 'imports-closed',
                    selected: 'import-selected',
                    changed: 'imports-changed',
                  },
                ),
              ],
              { visible: currentPage === 'imports', gap: 0, terminal: { grow: 1 } },
            ),
          ]
        : []),
      ...(state.openedPages.includes('projects')
        ? [
            ui.box(
              'retained-projects',
              [
                presentation.slot(
                  'projects',
                  'projects',
                  {
                    theme,
                    locale,
                    ...pageShell('projects'),
                    open: state.pages.at(-1) === 'projects',
                    busy: mutationPending(state) || catalogPending(state),
                  },
                  {
                    chrome: 'page-chrome',
                    closed: 'projects-closed',
                    selected: 'project-selected',
                    changed: 'projects-changed',
                  },
                ),
              ],
              { visible: currentPage === 'projects', gap: 0, terminal: { grow: 1 } },
            ),
          ]
        : []),
      ...(state.openedPages.includes('connections')
        ? [
            ui.box(
              'retained-connections',
              [
                presentation.slot(
                  'connections',
                  'connections',
                  {
                    theme,
                    locale,
                    ...pageShell('connections'),
                    setupRequest: state.connectionSetupRequest,
                  },
                  {
                    chrome: 'page-chrome',
                    closed: 'settings-closed',
                    changed: 'settings-changed',
                    completed: 'setup-completed',
                  },
                ),
              ],
              { visible: currentPage === 'connections', gap: 0, terminal: { grow: 1 } },
            ),
          ]
        : []),
    ],
    { gap: 0, terminal: { grow: 1 } },
  );

  return pageContent;
}
