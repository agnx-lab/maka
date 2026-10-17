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

import type { Component, Emission } from '../../packages/plugin-sdk/src/index.js';
import type { PageChrome, PageShellProps, Theme } from './contracts.js';
import { copy } from './locale-catalogs/page-shell.js';
import { sameValue } from './value.js';
import { type Node, type Presentation } from './view.js';

export interface PageState {
  pageBack?: number;
  pageChrome?: PageChrome;
}
type Props = PageShellProps & { theme: Theme; locale: string; open?: boolean };

// The page owns what Back does. The ordinary shell owns where its heading is drawn.
export function pageHeader<N extends Node>(
  presentation: Presentation<N>,
  state: PageState,
  props: Props,
  events: Emission[],
  title: string,
  back = 'back',
  disabled = false,
): N[] {
  if (props.shell) {
    const chrome = { title, backEnabled: !disabled };
    if (!sameValue(state.pageChrome, chrome)) {
      state.pageChrome = chrome;
      events.push({ name: 'chrome', value: chrome });
    }
    return [];
  }
  const ui = presentation.paint(props.theme);
  return [
    ui.box(
      'page-heading',
      [
        ui.button(back, copy(props.locale).back, disabled, false, {
          background: 0,
          align: 'start',
        }),
        ui.text('page-title', title, props.theme.text, 600),
      ],
      { row: true, gap: 1 },
    ),
  ];
}

export function withPageShell<S extends PageState, P extends Props, N>(
  component: Component<S, P, N>,
  back: (state: S, props: P) => string | null,
): Component<S, P, N> {
  return {
    ...component,
    mount(props, values) {
      const result = component.mount(props, values);
      result.frame.state.pageBack = props.shell?.back ?? 0;
      return result;
    },
    update(state, props, values) {
      const sequence = props.shell?.back ?? 0;
      const requested = sequence > (state.pageBack ?? 0);
      state.pageBack = sequence;
      const key = requested && props.open !== false ? back(state, props) : null;
      return key
        ? component.event(state, { kind: 'raw', key, event: {} }, props, values)
        : component.update(state, props, values);
    },
  };
}

// Inspector navigation stays outside the scrolling content and action rows.
export function panelHeader<N extends Node>(
  presentation: Presentation<N>,
  props: { theme: Theme; locale: string },
  key: string,
  title: string,
  close: string,
  back?: string,
): N {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  return ui.box(
    key,
    [
      ...(back
        ? [
            ui.box(
              key + '-back',
              [
                ui.button(
                  back,
                  props.theme.ascii ? '<' : '‹',
                  false,
                  false,
                  { background: 0 },
                  undefined,
                  text.back,
                ),
              ],
              { terminal: { basis: 3, shrink: false } },
            ),
          ]
        : []),
      ui.box(
        key + '-title-cell',
        [ui.text(key + '-title', title, props.theme.text, 650, { truncate: true })],
        { terminal: { grow: 1, shrink: true } },
      ),
      ui.box(
        key + '-close',
        [
          ui.button(
            close,
            props.theme.ascii ? 'x' : '×',
            false,
            false,
            { background: 0 },
            undefined,
            text.closeModels,
          ),
        ],
        { terminal: { basis: 3, shrink: false } },
      ),
    ],
    { row: true, gap: 1, terminal: { basis: 1, shrink: false } },
  );
}
