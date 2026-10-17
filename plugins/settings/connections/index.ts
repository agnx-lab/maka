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
  Action,
  ActionOutcome,
  Component,
  ComponentInput,
  ConnectionTarget,
  ReadingCommand,
  ReadingObservation,
  Emission,
  SettingsConnectionsPage,
  SettingsConnectionSummary,
  SettingsConnectionDetail,
  ConnectionVersionBasis,
} from '../../../packages/plugin-sdk/src/index.js';
import { connectionsContract, type ConnectionsProps } from '../../shared/contracts.js';
import { searchState, searchEvent, matches, type Search } from '../search.js';

import { pageHeader, withPageShell, type PageState } from '../../shared/page-shell.js';
import { copy, tuiText } from './locale.js';
import { output, type Presentation, type Node } from '../../shared/view.js';
interface State extends PageState {
  revision: number;
  setupRequest: number | null;
  setupRequested: boolean;
  serial: number;
  pending: {
    key: string;
    kind: 'page' | 'detail';
    basis: ConnectionVersionBasis | null;
    append: boolean;
  } | null;
  search: Search;
  created?: ConnectionVersionBasis;
  detailRoute: 'credential' | 'form';
  removeRequested: boolean;
  route: 'catalog' | 'credential' | 'form';
  detail: Extract<SettingsConnectionDetail, { kind: 'ready' }> | null;
  connections: SettingsConnectionSummary[];
  catalogRevision: number | null;
  next: number | null;
  selected: string | null;
  windowStart: number;
  scrollRevision: number;
  focus: ReadingCommand | null;
  defaultTarget: ConnectionTarget | null;
  failed: boolean;
  mount: number;
}
function request(
  state: State,
  kind: 'page' | 'detail',
  input: unknown,
  basis: ConnectionVersionBasis | null = null,
  append = false,
): Action[] {
  if (state.pending && !(kind === 'detail' && state.pending.kind === 'page')) return [];
  const key = 'connections-' + ++state.serial;
  state.pending = { key, kind, basis, append };
  state.failed = false;
  return [
    {
      key,
      binding: 'settings',
      member: 0,
      method: kind === 'page' ? 'connections' : 'connection',
      input,
      resources: {},
    },
  ];
}
function page(state: State, more = false) {
  return request(
    state,
    'page',
    more && state.catalogRevision !== null && state.next !== null
      ? { kind: 'continue', revision: state.catalogRevision, offset: state.next }
      : { kind: 'start' },
    null,
    more,
  );
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: ConnectionsProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const matched = state.connections
    .map((connection, at) => ({ connection, at }))
    .filter(({ connection }) =>
      matches(state.search, connection.name, connection.slug, connection.provider.name),
    );
  // Keep overlapping rows mounted when extending the native scroll window.
  const rows = matched
    .slice(state.windowStart, state.windowStart + 64)
    .flatMap(({ connection, at }) => {
      const id = connection.basis.connectionId;
      const selected = state.selected === id;
      const actions = [
        ui.button('open-' + id, text.signInConnection, state.pending?.kind === 'detail'),
        ui.button('edit-' + id, text.editConnection, state.pending?.kind === 'detail'),
        ui.button('remove-' + id, text.removeConnection, state.pending?.kind === 'detail'),
      ];
      {
        const identity =
          connection.slug === connection.provider.name
            ? connection.slug
            : `${connection.slug} · ${connection.provider.name}`;
        const detail =
          state.defaultTarget?.connectionId === id
            ? `${tuiText(props.locale, 'connection-default')}: ${state.defaultTarget.modelId} · ${identity}`
            : `${tuiText(props.locale, 'connection-models', { count: String(connection.enabledModelCount) })} · ${identity}`;
        const item = ui.item(
          'select-' + id,
          ui.box(
            'summary-' + id,
            [
              ui.text(
                'name-' + id,
                `${selected ? (props.theme.ascii ? '>' : '›') : ' '} ${connection.name}${connection.enabled ? '' : ' · ' + text.connectionDisabled}`,
                props.theme.text,
                400,
                { truncate: true },
              ),
              ui.text('detail-' + id, '  ' + detail, props.theme.muted, 400, { truncate: true }),
            ],
            { gap: 0 },
          ),
          selected,
          state.pending?.kind === 'detail',
          { followFocus: true, submit: true },
        );
        return connection.editable
          ? ui.contextMenu!(
              'connection-' + id,
              tuiText(props.locale, 'connection-actions'),
              item,
              actions,
            )
          : item;
      }
    });
  const catalog = ui.box(
    'connections',
    [
      ...(state.route === 'catalog'
        ? pageHeader(presentation, state, props, events, text.settings, 'back', false)
        : []),
      ui.box(
        'connection-query-box',
        [
          presentation.slot(
            'connection-query',
            'input',
            {
              appearance: props.theme.input,
              accessibilityLabel: text.searchConnections,
              placeholder: text.searchConnections,
              autoFocus: true,
              tabIndex: 1,
              readOnly: false,
              disabled: state.route !== 'catalog',
            },
            { changed: 'connection-query' },
          ),
        ],
        { gap: 0 },
      ),
      ...(state.search.error ? [ui.text('search-error', text[state.search.error])] : []),
      ...(!rows.length && state.search.query && !state.pending
        ? [ui.text('no-matches', text.noSearchResults)]
        : []),
      ...(state.pending ? [ui.text('loading', text.connecting, props.theme.muted)] : []),
      ...(state.failed ? [ui.text('failed', text.connectionsUnavailable, props.theme.muted)] : []),
      ...(!state.pending && !state.failed && !state.connections.length
        ? [ui.text('empty', text.noConnections, props.theme.muted)]
        : []),
      presentation.reading(
        'connection-list',
        {
          command: state.focus,
          content: ui.box('connection-rows', rows, {
            focusGroup: true,
            scroll: 'anchor',
            gap: 1,
            terminal: { grow: 1, basis: 0 },
          }),
        },
        { input: 'connection-input', observed: 'connection-viewport' },
      ),
      ui.box(
        'page-actions',
        [
          ...(state.selected &&
          state.connections.some((c) => c.basis.connectionId === state.selected && c.editable)
            ? [
                ui.button(
                  'open-' + state.selected,
                  text.signInConnection,
                  state.pending?.kind === 'detail',
                ),
                ui.button(
                  'edit-' + state.selected,
                  text.editConnection,
                  state.pending?.kind === 'detail',
                ),
              ]
            : []),
          ui.button(
            'create',
            text.createConnection,
            !!state.pending || state.catalogRevision === null,
            true,
          ),

          ui.button(
            'refresh',
            props.theme.ascii ? 'r' : '↻',
            !!state.pending,
            false,
            { background: 0 },
            undefined,
            text.refreshConnections,
          ),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    {
      visible: state.route === 'catalog',
      autoFocus: true,
      gap: 1,
      padding: props.shell ? 0 : 1,
      terminal: { grow: 1 },
      shortcuts: [
        { key: 'escape', modifiers: [], input: 'back', enabled: state.route === 'catalog' },
      ],
    },
  );
  const view = ui.box(
    'connections-page',
    [
      catalog,
      ...(state.route === 'form'
        ? [
            presentation.slot(
              'form-' + state.mount,
              'form',
              {
                theme: props.theme,
                locale: props.locale,
                shell: props.shell,
                catalogRevision: state.catalogRevision!,
                connection: state.detail,
                removeRequested: state.removeRequested,
                existingSlugs: state.connections.map((c) => c.slug),
              },
              { closed: 'closed', changed: 'changed', chrome: 'chrome' },
            ),
          ]
        : state.route === 'credential' && state.detail
          ? [
              presentation.slot(
                'credential-' + state.mount,
                'credential',
                {
                  theme: props.theme,
                  locale: props.locale,
                  shell: props.shell,
                  target: state.detail.target,
                },
                { closed: 'closed', changed: 'changed', chrome: 'chrome', completed: 'completed' },
              ),
            ]
          : []),
    ],
    { gap: 0, terminal: { grow: 1 } },
  );
  const out = output(state, view, events);
  out.actions = actions;
  return out;
}
export function connections<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ConnectionsProps, N> {
  return withPageShell<State, ConnectionsProps, N>(
    {
      mount(props) {
        const state: State = {
          revision: 0,
          setupRequest: props.setupRequest ?? null,
          setupRequested: props.setupRequest != null,
          serial: 0,
          pending: null,
          search: searchState(),
          detailRoute: 'credential',
          removeRequested: false,
          route: 'catalog',
          detail: null,
          connections: [],
          catalogRevision: null,
          next: null,
          selected: null,
          windowStart: 0,
          scrollRevision: 0,
          focus: null,
          defaultTarget: null,
          failed: false,
          mount: 0,
        };
        return frame(presentation, state, props, page(state));
      },
      update(state, props) {
        state.revision++;
        if (props.setupRequest != null && props.setupRequest !== state.setupRequest) {
          state.setupRequest = props.setupRequest;
          state.setupRequested = true;
          state.route = 'catalog';
          state.detail = null;
          return frame(presentation, state, props, page(state));
        }
        return frame(presentation, state, props);
      },
      event(state, input, props) {
        state.revision++;
        if (
          input.kind === 'component' &&
          input.child === 'connection-list' &&
          input.name === 'input'
        )
          input = input.event as ComponentInput;
        const actions: Action[] = [],
          events: Emission[] = [];
        if (searchEvent(state.search, input, 'connection-query', actions)) {
          state.windowStart = 0;
          const first = state.connections.find((c) =>
            matches(state.search, c.name, c.slug, c.provider.name),
          );
          state.selected = first?.basis.connectionId ?? null;
          if (state.selected)
            state.focus = {
              sequence: (state.focus?.sequence ?? 0) + 1,
              target: { kind: 'reveal_child', key: 'select-' + state.selected },
            };
        } else if (
          input.kind === 'component' &&
          input.child === 'connection-list' &&
          input.name === 'observed'
        ) {
          const observed = input.event as ReadingObservation;
          if (
            observed.scroll &&
            observed.scroll.readingRevision > state.scrollRevision &&
            observed.viewport?.readingRevision === observed.readingRevision &&
            observed.pendingCommand == null
          ) {
            state.scrollRevision = observed.scroll.readingRevision;
            const count = state.connections.filter((c) =>
              matches(state.search, c.name, c.slug, c.provider.name),
            ).length;
            if (observed.scroll.direction === 'up' && observed.viewport.atStart)
              state.windowStart = Math.max(0, state.windowStart - 32);
            else if (
              observed.scroll.direction === 'down' &&
              observed.viewport.atEnd &&
              state.windowStart + 64 < count
            )
              state.windowStart += 32;
          }
        } else if (input.kind === 'action' && input.key === state.pending?.key) {
          const pending = state.pending;
          state.pending = null;
          const result = input.event as ActionOutcome;
          if (result.status !== 'succeeded') state.failed = true;
          else if (pending.kind === 'page') {
            const resultPage = result.value as SettingsConnectionsPage;
            if (resultPage.kind === 'changed') {
              state.connections = [];
              state.next = null;
              state.catalogRevision = null;
              actions.push(...page(state));
            } else {
              if (!pending.append) state.windowStart = 0;
              state.connections = pending.append
                ? [...state.connections, ...resultPage.connections]
                : resultPage.connections;
              state.catalogRevision = resultPage.revision;
              state.next = resultPage.next;
              state.defaultTarget = resultPage.defaultTarget;
              const visible = state.connections.filter((connection) =>
                matches(state.search, connection.name, connection.slug, connection.provider.name),
              );
              if (!visible.some((connection) => connection.basis.connectionId === state.selected))
                state.selected = visible[0]?.basis.connectionId ?? null;
              if (state.next !== null) actions.push(...page(state, true));
              else if (state.setupRequested) {
                state.setupRequested = false;
                if (!state.connections.length) {
                  state.route = 'form';
                  state.mount++;
                }
              }
              if (state.selected && !pending.append)
                state.focus = {
                  sequence: (state.focus?.sequence ?? 0) + 1,
                  target: { kind: 'reveal_child', key: 'select-' + state.selected },
                };
            }
          } else {
            const detail = result.value as SettingsConnectionDetail;
            if (
              detail.kind === 'ready' &&
              detail.target.connectionId === pending.basis?.connectionId &&
              detail.target.revision === pending.basis.revision
            ) {
              state.detail = detail;
              state.route = state.detailRoute;
              state.mount++;
            } else {
              state.failed = true;
              actions.push(...page(state));
            }
          }
        } else if (
          input.kind === 'component' &&
          (input.child === 'credential-' + state.mount || input.child === 'form-' + state.mount) &&
          state.route !== 'catalog'
        ) {
          if (input.name === 'chrome') {
            state.pageChrome = input.event as NonNullable<State['pageChrome']>;
            events.push({ name: 'chrome', value: input.event });
          } else if (input.name === 'changed') {
            state.created = (input.event as { created?: ConnectionVersionBasis }).created;
            events.push({ name: 'changed', value: {} });
          } else if (input.name === 'completed') {
            state.detail = null;
            state.route = 'catalog';
            events.push({ name: 'completed', value: {} });
            actions.push(...page(state));
          } else if (input.name === 'closed') {
            state.detail = null;
            state.route = 'catalog';
            if (state.created) {
              state.detailRoute = 'credential';
              actions.push(...request(state, 'detail', state.created, state.created));
              state.created = undefined;
            } else if (props.setupRequest != null && input.child.startsWith('form-'))
              events.push({ name: 'closed', value: {} });
            else actions.push(...page(state));
          }
        } else if (input.kind === 'raw' && state.route === 'catalog') {
          if (input.key === 'back') events.push({ name: 'closed', value: {} });
          else if (input.key === 'refresh') actions.push(...page(state));
          else if (input.key === 'create' && !state.pending && state.catalogRevision !== null) {
            state.detail = null;
            state.removeRequested = false;
            state.route = 'form';
            state.mount++;
          } else if (state.pending?.kind !== 'detail' && input.key.startsWith('select-')) {
            const connection = state.connections.find(
              (connection) => connection.basis.connectionId === input.key.slice(7),
            );
            if (
              connection &&
              matches(state.search, connection.name, connection.slug, connection.provider.name)
            ) {
              state.selected = connection.basis.connectionId;
              const matched = state.connections.filter((c) =>
                matches(state.search, c.name, c.slug, c.provider.name),
              );
              const at = matched.indexOf(connection);
              if (at < state.windowStart + 3 && state.windowStart > 0)
                state.windowStart = Math.max(0, state.windowStart - 32);
              else if (at >= state.windowStart + 61 && state.windowStart + 64 < matched.length)
                state.windowStart += 32;
              if ((input.event as { kind?: string }).kind === 'submit' && connection.editable) {
                state.detailRoute = 'credential';
                actions.push(...request(state, 'detail', connection.basis, connection.basis));
              }
            }
          } else if (state.pending?.kind !== 'detail' && /^(open|edit|remove)-/.test(input.key)) {
            const action = input.key.slice(0, input.key.indexOf('-'));
            const id = input.key.slice(action.length + 1);
            const connection = state.connections.find((row) => row.basis.connectionId === id);
            if (connection?.editable) {
              state.removeRequested = action === 'remove';
              state.detailRoute = action === 'open' ? 'credential' : 'form';
              actions.push(...request(state, 'detail', connection.basis, connection.basis));
            }
          }
        }
        return frame(presentation, state, props, actions, events);
      },
    },
    (state) => (state.route === 'catalog' ? 'back' : null),
  );
}
export function activate() {
  return {
    Connections: { kind: 'component', contract: connectionsContract, component: ['component'] },
  };
}
