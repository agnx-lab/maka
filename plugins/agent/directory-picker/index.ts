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
  AgentDirectories,
  AgentDirectorySelection,
  AgentDirectoryPage,
  Component,
  ComponentInput,
  Emission,
  EditorObservation,
  EditorTextRef,
  TextChunk,
  ReadingCommand,
} from '../../../packages/plugin-sdk/src/index.js';
import { directoryPickerContract, type DirectoryPickerProps } from '../../shared/contracts.js';
import { scrollRows, visibleRows, resetScroll, showRow } from '../../shared/scroll.js';
import { copy } from './locale.js';
import { compactLabel, output, type Presentation, type Node } from '../../shared/view.js';

type Navigation = 'roots' | 'child' | 'parent' | 'page';
interface State {
  purpose: 'workspace' | 'import';
  revision: number;
  serial: number;
  open: boolean;
  page:
    | (
        | Extract<AgentDirectoryPage, { kind: 'roots' }>
        | (Omit<Extract<AgentDirectoryPage, { kind: 'children' }>, 'children'> & {
            children: Extract<AgentDirectoryPage, { kind: 'children' }>['children'][number][];
          })
      )
    | null;
  pageKey: string;
  observation: EditorObservation | null;
  basis: EditorTextRef | null;
  query: string;
  queryError: 'long' | 'unavailable' | null;
  reading: { key: string; basis: EditorTextRef; choice: string | null } | null;
  ancestors: AgentDirectorySelection[];
  pending: { key: string; navigation: Navigation; append: boolean } | null;
  failed: boolean;
  active: number;
  reveal: ReadingCommand | null;
}
function interactive(state: State, props: DirectoryPickerProps) {
  return state.open && props.open && !props.busy && !state.pending && !state.failed;
}
function sameText(a: EditorTextRef | null, b: EditorTextRef | null) {
  return a?.owner === b?.owner && a?.revision === b?.revision;
}
function dirty(state: State) {
  return !!state.observation && !sameText(state.observation.snapshot.text, state.basis);
}
function available(state: State, props: DirectoryPickerProps) {
  return interactive(state, props) && !dirty(state) && !state.reading && !state.queryError;
}
function rows(state: State) {
  const all = state.page?.kind === 'roots' ? state.page.roots : (state.page?.children ?? []);
  return all.flatMap((directory, at) =>
    directory.name.toLowerCase().includes(state.query.toLowerCase())
      ? [{ key: `${state.pageKey}-${at}`, directory }]
      : [],
  );
}
function capture(
  state: State,
  props: DirectoryPickerProps,
  choice: string | null = null,
): Action[] {
  const basis = state.observation?.snapshot.text;
  if (!interactive(state, props) || state.reading || !basis) return [];
  if (basis.bytes > 1024) {
    state.queryError = 'long';
    return [];
  }
  const key = `directory-${++state.serial}`;
  state.reading = { key, basis, choice };
  state.queryError = null;
  return [
    {
      key,
      binding: 'reader',
      member: 0,
      method: 'read',
      input: { start: 0, end: basis.bytes },
      resources: {
        text: {
          child: 'directory-query',
          method: 'snapshot',
          input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
        },
      },
    },
  ];
}
function resume(state: State, props: DirectoryPickerProps): Action[] {
  if (dirty(state) && !state.queryError) return capture(state, props);
  if (available(state, props) && state.query && state.page?.kind === 'children' && state.page.after)
    return children(state, state.page.directory.selection, 'page', state.page.after);
  return [];
}
function choose(
  state: State,
  props: DirectoryPickerProps,
  key: string,
  events: Emission[],
): Action[] {
  if (!available(state, props)) return [];
  if (key === `${state.pageKey}-choose` && state.page?.kind === 'children')
    events.push({ name: 'selected', value: state.page.directory.selection });
  else {
    const row = rows(state).find((row) => row.key === key);
    if (row) return children(state, row.directory.selection, 'child');
  }
  return [];
}
function reveal(state: State) {
  showRow(state, 'directory-rows', state.active);
  if (rows(state)[state.active])
    state.reveal = {
      sequence: (state.reveal?.sequence ?? 0) + 1,
      target: { kind: 'reveal_child', key: rows(state)[state.active].key },
    };
}
function query(state: State, input: AgentDirectories, navigation: Navigation): Action[] {
  const key = `directory-${++state.serial}`;
  state.pending = { key, navigation, append: input.kind === 'children' && input.after != null };
  state.failed = false;
  return [
    {
      key,
      binding: 'agent',
      member: 0,
      method: state.purpose === 'import' ? 'importDirectories' : 'directories',
      input,
      resources: {},
    },
  ];
}
function roots(state: State) {
  return query(state, { kind: 'roots' }, 'roots');
}
function children(
  state: State,
  selection: AgentDirectorySelection,
  navigation: Navigation = 'page',
  after: string | null = null,
) {
  return query(state, { kind: 'children', selection, after }, navigation);
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: DirectoryPickerProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const enabled = available(state, props);
  const page = state.page;
  const current = page?.kind === 'children' ? page.directory : null;
  const buttons = visibleRows(state, 'directory-rows', rows(state)).map(
    ({ entry: { key, directory }, at }) => {
      const active = at === state.active;
      const label =
        (active ? (props.theme.ascii ? '> ' : '› ') : '  ') + compactLabel(directory.name, 64);
      const button = ui.button(key, label, !enabled, false, {
        align: 'start',
        ...{ background: active ? props.theme.selected : 0 },
      });

      return button;
    },
  );
  const view = ui.box(
    'directory-picker',
    [
      ui.text('directory-help', text.directoryHelp, props.theme.muted),
      ui.box(
        'directory-query-scope',
        [
          presentation.slot(
            'directory-query',
            'input',
            {
              appearance: props.theme.input,
              accessibilityLabel: text.directoryFilter,
              placeholder: text.directoryFilter,
              autoFocus: true,
              readOnly: false,
              disabled: !state.open || !props.open || props.busy || !!state.pending,
              tabIndex: 1,
            },
            { changed: 'query', submitted: 'open' },
          ),
        ],
        {
          gap: 0,
          shortcuts: [
            { key: 'down', modifiers: [], input: 'down', enabled: enabled },
            { key: 'up', modifiers: [], input: 'up', enabled: enabled },
            { key: 'home', modifiers: [], input: 'first', enabled: enabled },
            { key: 'end', modifiers: [], input: 'last', enabled: enabled },
          ],
        },
      ),
      ...(state.queryError
        ? [
            ui.text(
              'directory-query-error',
              state.queryError === 'long' ? text.searchTooLong : text.directoryFailed,
              props.theme.muted,
            ),
          ]
        : []),
      ...(current
        ? [ui.text('directory-path', compactLabel(current.path, 66), props.theme.muted)]
        : []),
      ...(state.pending
        ? [ui.text('directory-loading', text.directoryLoading, props.theme.muted)]
        : []),
      ...(state.failed
        ? [ui.text('directory-failed', text.directoryFailed, props.theme.muted)]
        : []),
      ...(!state.pending && !state.failed && page && !buttons.length
        ? [ui.text('directory-empty', text.directoryEmpty, props.theme.muted)]
        : []),
      presentation.reading(
        'directory-rows',
        {
          content: ui.box('directory-viewport', buttons, {
            scroll: 'anchor',
            gap: 0,
            terminal: { grow: 1, basis: 0 },
          }),
          command: state.reveal,
        },
        { input: 'row-input', observed: 'row-viewport' },
      ),
      ui.box(
        'directory-controls',
        [
          ...(current
            ? [ui.button(`${state.pageKey}-parent`, text.directoryParent, !enabled)]
            : []),
          ui.button(
            'refresh',
            props.theme.ascii ? 'r' : '↻',
            !state.open || !props.open || props.busy || !!state.pending,
            false,
            { background: 0 },
            undefined,
            text.directoryRefresh,
          ),
          ...(current
            ? [
                ui.button(
                  `${state.pageKey}-choose`,
                  props.chooseLabel ?? text.directoryChoose,
                  !enabled,
                ),
              ]
            : []),
          ui.button('close', text.cancel, false, false, { background: 0, align: 'start' }),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    {
      visible: state.open && props.open,
      gap: 0,
      terminal: { grow: 1 },
      shortcuts: [{ key: 'escape', modifiers: [], input: 'close', enabled: true }],
    },
  );
  const out = output(state, view, events);
  out.actions = actions;
  return out;
}
export function directoryPicker<N extends Node>(
  presentation: Presentation<N>,
): Component<State, DirectoryPickerProps, N> {
  return {
    mount(props) {
      const state: State = {
        revision: 0,
        purpose: props.purpose ?? 'workspace',
        serial: 0,
        open: props.open,
        page: null,
        pageKey: '',
        observation: null,
        basis: null,
        query: '',
        queryError: null,
        reading: null,
        ancestors: [],
        pending: null,
        failed: false,
        active: 0,
        reveal: null,
      };
      return frame(presentation, state, props, props.open ? roots(state) : []);
    },
    update(previous, props) {
      const state = { ...previous, revision: previous.revision + 1 };
      let actions: Action[] = [];
      if (state.purpose !== (props.purpose ?? 'workspace')) {
        state.purpose = props.purpose ?? 'workspace';
        state.open = false;
        state.pending = null;
        state.reading = null;
      }
      if (!props.open) {
        state.open = false;
        state.pending = null;
        state.reading = null;
      } else if (!state.open) {
        state.open = true;
        state.page = null;
        state.ancestors = [];
        state.active = 0;
        actions = roots(state);
      }
      actions.push(...resume(state, props));
      return frame(presentation, state, props, actions);
    },
    event(state, input, props) {
      state.revision++;
      let actions: Action[] = [];
      const events: Emission[] = [];
      if (input.kind === 'component' && input.child === 'directory-rows' && input.name === 'input')
        input = input.event as ComponentInput;
      if (scrollRows(state, input, 'directory-rows', rows(state).length))
        input = { kind: 'raw', key: `${state.pageKey}-more`, event: {} };
      if (input.kind === 'component' && input.child === 'directory-query') {
        if (input.name === 'changed') {
          state.observation = input.event as EditorObservation;
          state.queryError = null;
          if (!state.observation.snapshot.text.bytes) {
            state.query = '';
            state.basis = state.observation.snapshot.text;
            state.active = 0;
            reveal(state);
          }
          actions.push(...resume(state, props));
        } else if (input.name === 'submitted' && available(state, props)) {
          const row = rows(state)[state.active];
          if (row) actions.push(...capture(state, props, row.key));
        }
      } else if (input.kind === 'action' && input.key === state.reading?.key) {
        const request = state.reading;
        state.reading = null;
        const result = input.event as ActionOutcome;
        if (!sameText(request.basis, state.observation?.snapshot.text ?? null))
          actions.push(...resume(state, props));
        else if (result.status !== 'succeeded') state.queryError = 'unavailable';
        else {
          const query = (result.value as TextChunk).text.trim();
          if ([...query].length > 256) state.queryError = 'long';
          else {
            const changed = state.query !== query;
            state.query = query;
            state.basis = request.basis;
            state.queryError = null;
            if (changed) {
              state.active = 0;
              reveal(state);
            }
            if (request.choice && !changed)
              actions.push(...choose(state, props, request.choice, events));
            actions.push(...resume(state, props));
          }
        }
      } else if (input.kind === 'action' && input.key === state.pending?.key) {
        const request = state.pending;
        state.pending = null;
        const result = input.event as ActionOutcome;
        if (state.open && props.open) {
          if (result.status !== 'succeeded') state.failed = true;
          else {
            if (request.navigation === 'roots') state.ancestors = [];
            else if (request.navigation === 'parent') state.ancestors.pop();
            else if (request.navigation === 'child' && state.page?.kind === 'children')
              state.ancestors.push(state.page.directory.selection);
            const page = result.value as AgentDirectoryPage;
            if (request.append && state.page?.kind === 'children' && page.kind === 'children') {
              state.page = { ...page, children: [...state.page.children, ...page.children] };
            } else {
              state.page = page;
              state.pageKey = request.key;
              state.active = 0;
              resetScroll(state, 'directory-rows');
              reveal(state);
            }
            actions.push(...resume(state, props));
          }
        }
      } else if (input.kind === 'raw') {
        if (input.key === 'close' && state.open && props.open) {
          state.pending = null;
          state.reading = null;
          events.push({ name: 'closed', value: {} });
        } else if (
          input.key === 'refresh' &&
          state.open &&
          props.open &&
          !props.busy &&
          !state.pending &&
          !state.reading
        ) {
          if (state.queryError === 'unavailable') {
            state.queryError = null;
            actions = capture(state, props);
          } else
            actions =
              state.page?.kind === 'children'
                ? children(state, state.page.directory.selection)
                : roots(state);
        } else if (available(state, props)) {
          const all = rows(state),
            page = state.page;
          if (input.key === `${state.pageKey}-choose`) actions = capture(state, props, input.key);
          else if (input.key === `${state.pageKey}-parent` && page?.kind === 'children') {
            const parent = state.ancestors.at(-1);
            actions = parent ? children(state, parent, 'parent') : roots(state);
          } else if (
            input.key === `${state.pageKey}-more` &&
            page?.kind === 'children' &&
            page.after
          )
            actions = children(state, page.directory.selection, 'page', page.after);
          else if (['down', 'up', 'first', 'last'].includes(input.key)) {
            state.active =
              input.key === 'first'
                ? 0
                : input.key === 'last'
                  ? Math.max(0, all.length - 1)
                  : Math.max(
                      0,
                      Math.min(all.length - 1, state.active + (input.key === 'down' ? 1 : -1)),
                    );
            reveal(state);
          } else {
            if (all.some((row) => row.key === input.key))
              actions = capture(state, props, input.key);
          }
        }
      }
      return frame(presentation, state, props, actions, events);
    },
  };
}
export function activate() {
  return {
    DirectoryPicker: {
      kind: 'component',
      contract: directoryPickerContract,
      component: ['component'],
    },
  };
}
