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
  Component,
  ComponentValues,
  SettingsModelChoice,
} from '../../../packages/plugin-sdk/src/index.js';
import { studioContract, type Theme } from '../../shared/contracts.js';
import { resolveLocale } from '../../shared/locale.js';
import { copy } from './locale.js';
import { output, type Presentation, type Node } from '../../shared/view.js';

type Route = 'overview' | 'connections' | 'models' | 'deployment';
interface State {
  revision: number;
  route: Route;
  catalogEpoch: number;
  selection: SettingsModelChoice | null;
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  locale: string,
  state: State,
  values: ComponentValues,
) {
  locale = resolveLocale(locale, values.locale?.[0]);

  const theme = values.appearance![0] as unknown as Theme;
  const text = copy(locale);
  const ui = presentation.paint(theme);
  const labels = {
    overview: text.studioOverview,
    connections: text.settings,
    models: text.models,
    deployment: text.deploymentTitle,
  };
  const page = (route: Route, nodes: ReturnType<typeof ui.box>[]) =>
    ui.box('page-' + route, nodes, {
      visible: state.route === route,
      scroll: route === 'overview' ? 'manual' : 'none',
      terminal: { grow: 1 },
    });
  const out = output(
    state,
    ui.box(
      'studio',
      [
        ui.box(
          'heading',
          [
            ui.text('title', text.studioTitle, theme.text, 600),
            ui.box('introduction', [ui.text('intro', text.studioIntro, theme.muted)], {
              visible: state.route === 'overview',
              gap: 0,
            }),
          ],
          { gap: 0 },
        ),
        ui.box(
          'navigation',
          (Object.keys(labels) as Route[]).map((route) =>
            ui.button('route-' + route, labels[route], false, state.route === route),
          ),
          { row: true, gap: 1, terminal: { wrap: true } },
        ),
        page('overview', [
          ...(['connections', 'models', 'deployment'] as const).map((route) =>
            ui.box(
              'overview-' + route,
              [
                ui.button(
                  'open-' + route,
                  route === 'connections' ? text.studioConnections : labels[route],
                ),
                ui.text(
                  'help-' + route,
                  route === 'connections'
                    ? text.studioConnectionsHelp
                    : route === 'models'
                      ? text.studioModelsHelp
                      : text.studioPluginsHelp,
                  theme.muted,
                ),
              ],
              { padding: 1, surface: true },
            ),
          ),
        ]),
        page('connections', [
          presentation.slot(
            'connections',
            'connections',
            { theme, locale },
            { closed: 'closed', changed: 'connections-changed' },
          ),
        ]),
        page('models', [
          ui.text('models-title', text.models, theme.text, 600),
          ui.text('models-help', text.studioModelsHelp, theme.muted),
          presentation.slot(
            'models',
            'models',
            {
              theme,
              locale,
              catalogEpoch: state.catalogEpoch,
              openRequest: null,
              selection: state.selection,
              busy: false,
            },
            { selected: 'model-selected', connections: 'model-connections' },
          ),
        ]),
        page('deployment', [
          presentation.slot('deployment', 'deployment', { theme, locale }, { closed: 'closed' }),
        ]),
      ],
      { padding: 1, background: theme.background, gap: 1, terminal: { grow: 1 } },
    ),
  );

  out.frame.surface = { kind: 'terminal', binding: 'surface' };
  return out;
}
export function studio<N extends Node>(
  presentation: Presentation<N>,
  locale: string,
): Component<State, Record<string, never>, N> {
  return {
    mount(_props, values) {
      return frame(
        presentation,
        locale,
        { revision: 0, route: 'overview', catalogEpoch: 0, selection: null },
        values,
      );
    },
    update(state, _props, values) {
      return frame(presentation, locale, { ...state, revision: state.revision + 1 }, values);
    },
    event(previous, event, _props, values) {
      const state = { ...previous, revision: previous.revision + 1 };
      if (event.kind === 'raw') {
        for (const route of ['overview', 'connections', 'models', 'deployment'] as const)
          if (event.key === 'route-' + route || event.key === 'open-' + route) state.route = route;
      } else if (event.kind === 'component') {
        if (
          (event.child === 'connections' || event.child === 'deployment') &&
          event.name === 'closed'
        )
          state.route = 'overview';
        if (event.child === 'connections' && event.name === 'changed') state.catalogEpoch++;
        if (event.child === 'models' && event.name === 'selected')
          state.selection = event.event as SettingsModelChoice;
        if (event.child === 'models' && event.name === 'connections') state.route = 'connections';
      }
      return frame(presentation, locale, state, values);
    },
  };
}
export function activate() {
  return { Workbench: { kind: 'component', contract: studioContract, component: ['component'] } };
}
