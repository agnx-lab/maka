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

import type { Component, Emission } from '../../../packages/plugin-sdk/src/index.js';
import {
  deploymentContract,
  type DeploymentProps,
  type PageChrome,
} from '../../shared/contracts.js';
import {
  bookmark,
  bookmarkReady,
  bookmarkView,
  observeBookmark,
  retryBookmark,
  type Bookmark,
} from '../../shared/bookmark.js';
import { copy } from './locale.js';
import { pageHeader, withPageShell, type PageState } from '../../shared/page-shell.js';
import { output, type Presentation, type Node } from '../../shared/view.js';

type Place = 'interface' | 'services';
interface State extends PageState {
  revision: number;
  backs: Partial<Record<Place, number>>;
  chrome: Partial<Record<Place, PageChrome>>;
  bookmark: Bookmark<Place>;
  selected: Place;
  opened: Place[];
}

export function deploymentSettings<N extends Node>(
  presentation: Presentation<N>,
): Component<State, DeploymentProps, N> {
  function frame(state: State, props: DeploymentProps, events: Emission[] = []) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale);
    if (props.shell) {
      const chrome = state.chrome[state.selected];
      pageHeader(
        presentation,
        state,
        props,
        events,
        chrome?.title ?? text.deploymentTitle,
        'deployment-settings-back',
        chrome?.backEnabled !== true,
      );
    }
    return output(
      state,
      ui.box(
        'deployment-settings',
        [
          bookmarkView(
            presentation,
            state.bookmark,
            'deployment-place-checkpoint',
            (props.bookmarkKey ?? 'deployment') + '/place',
            state.selected,
          ),
          ...(state.bookmark.observed?.state.kind === 'failed'
            ? [
                ui.box(
                  'deployment-place-error',
                  [
                    ui.text(
                      'deployment-place-message',
                      state.bookmark.command &&
                        state.bookmark.command.sequence > state.bookmark.observed.sequence
                        ? text.restoringView
                        : text.viewSaveFailed,
                      props.theme.warning,
                    ),
                    ...(state.bookmark.command &&
                    state.bookmark.command.sequence > state.bookmark.observed.sequence
                      ? []
                      : [ui.button('deployment-place-retry', text.reloadView)]),
                  ],
                  { row: true, gap: 1, terminal: { wrap: true } },
                ),
              ]
            : []),
          ui.box(
            'deployment-targets',
            [
              ui.button(
                'deployment-interface',
                text.deploymentInterface,
                false,
                state.selected === 'interface',
              ),
              ui.button(
                'deployment-services',
                text.deploymentServices,
                false,
                state.selected === 'services',
              ),
            ],
            { row: true, gap: 1, terminal: { wrap: true } },
          ),
          ui.text(
            'deployment-target-help',
            state.selected === 'interface'
              ? text.deploymentInterfaceHelp
              : text.deploymentServicesHelp,
            props.theme.muted,
          ),
          ...state.opened.map((place) =>
            ui.box(
              'deployment-target-' + place,
              [
                presentation.slot(
                  'deployment-editor-' + place,
                  place,
                  {
                    ...props,
                    ...(props.shell ? { shell: { back: state.backs[place] ?? 0 } } : {}),
                    bookmarkKey: (props.bookmarkKey ?? 'deployment') + '/' + place,
                    navigationReady:
                      props.navigationReady !== false &&
                      bookmarkReady(state.bookmark, state.selected),
                  },
                  { closed: 'closed', chrome: 'chrome' },
                ),
              ],
              { visible: state.selected === place, terminal: { grow: 1, basis: 0 } },
            ),
          ),
        ],
        {
          gap: 0,
          terminal: { grow: 1 },
          shortcuts: [
            { key: 'escape', modifiers: [], input: 'deployment-settings-back', enabled: true },
          ],
        },
      ),
      events,
    );
  }
  return withPageShell<State, DeploymentProps, N>(
    {
      mount(props) {
        return frame(
          {
            revision: 0,
            backs: {},
            chrome: {},
            selected: 'interface',
            opened: ['interface'],
            bookmark: bookmark<Place>('interface'),
          },
          props,
        );
      },
      update(state, props) {
        state.revision++;
        return frame(state, props);
      },
      event(state, input, props) {
        state.revision++;
        const events: Emission[] = [];
        if (input.kind === 'raw' && input.key === 'deployment-place-retry')
          retryBookmark(state.bookmark);
        const restored = observeBookmark(
          state.bookmark,
          input,
          'deployment-place-checkpoint',
          state.selected,
          (value) => (value === 'interface' || value === 'services' ? value : null),
        );
        if (restored) {
          state.selected = restored;
          if (!state.opened.includes(restored)) state.opened.push(restored);
        }
        if (input.kind === 'raw') {
          if (input.key === 'deployment-settings-back') {
            if (!props.shell) events.push({ name: 'closed', value: {} });
            else if (state.chrome[state.selected]?.backEnabled)
              state.backs[state.selected] = (state.backs[state.selected] ?? 0) + 1;
          }
          const place =
            input.key === 'deployment-interface'
              ? 'interface'
              : input.key === 'deployment-services'
                ? 'services'
                : null;
          if (place) {
            state.selected = place;
            // A hidden editor keeps its native draft, original operation and receipt.
            if (!state.opened.includes(place)) state.opened.push(place);
          }
        } else if (
          input.kind === 'component' &&
          input.name === 'chrome' &&
          state.opened.some((place) => input.child === 'deployment-editor-' + place)
        ) {
          const place = input.child.slice('deployment-editor-'.length) as Place;
          state.chrome[place] = input.event as PageChrome;
        } else if (
          input.kind === 'component' &&
          input.name === 'closed' &&
          input.child === 'deployment-editor-' + state.selected
        ) {
          events.push({ name: 'closed', value: {} });
        }
        return frame(state, props, events);
      },
    },
    () => 'deployment-settings-back',
  );
}
export function activate() {
  return {
    Deployment: { kind: 'component', contract: deploymentContract, component: ['component'] },
  };
}
