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
  AgentList,
  AgentSession,
  AgentSessionPage,
  Component,
  ComponentInput,
  EditorObservation,
  EditorTextRef,
  Emission,
  ReadingCommand,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { paletteContract, type PaletteCommand, type PaletteProps } from '../../shared/contracts.js';
import { scrollBoundary } from '../../shared/scroll.js';
import { copy } from './locale.js';
import { compactLabel, output, type Presentation, type Node } from '../../shared/view.js';

type Choice =
  | { kind: 'command'; command: PaletteCommand }
  | { kind: 'session'; session: AgentSession };
type Request =
  | { kind: 'page'; key: string; query: string }
  | { kind: 'text'; key: string; basis: EditorTextRef; choice: Choice | null };
interface State {
  revision: number;
  serial: number;
  open: boolean;
  observation: EditorObservation | null;
  basis: EditorTextRef | null;
  query: string;
  queryError: 'long' | 'unavailable' | null;
  pending: Request | null;
  submitted: EditorTextRef | null;
  source: AgentSessionPage | null;
  scanOffset: number;
  matches: AgentSession[];
  failed: boolean;
  batch: number;
  matchLimit: number;
  active: number;
  offset: number;
  reveal: ReadingCommand | null;
}
function sameText(a: EditorTextRef | null, b: EditorTextRef | null) {
  return a?.owner === b?.owner && a?.revision === b?.revision;
}
function dirty(state: State) {
  return !!state.observation && !sameText(state.observation.snapshot.text, state.basis);
}
function interactive(state: State, props: PaletteProps) {
  return state.open && props.open && !props.busy;
}
function matches(query: string, value: string) {
  const lower = value.toLowerCase();
  return query
    .toLowerCase()
    .split(/\s+/u)
    .every((term) => lower.includes(term));
}
function rows(state: State, props: PaletteProps) {
  return [
    ...props.commands.flatMap((command) =>
      matches(state.query, command.label + ' ' + command.id + ' ' + command.hint)
        ? [{ key: `command-${command.id}`, choice: { kind: 'command', command } as Choice }]
        : [],
    ),
    ...state.matches.map((session, at) => ({
      key: `session-${state.batch}-${at}`,
      choice: { kind: 'session', session } as Choice,
    })),
  ];
}
function enabled(state: State, props: PaletteProps, choice: Choice) {
  return (
    interactive(state, props) &&
    !!state.observation &&
    !dirty(state) &&
    !state.queryError &&
    state.pending?.kind !== 'text' &&
    (choice.kind === 'session'
      ? !props.selectionDisabled
      : props.commands.some((command) => command.id === choice.command.id && !command.disabled))
  );
}
function reveal(state: State, props: PaletteProps) {
  const row = rows(state, props)[state.active];
  if (row)
    state.reveal = {
      sequence: (state.reveal?.sequence ?? 0) + 1,
      target: { kind: 'reveal_child', key: row.key },
    };
}
function reset(state: State, props: PaletteProps) {
  state.source = null;
  state.scanOffset = 0;
  state.matches = [];
  state.failed = false;
  state.batch++;
  state.matchLimit = 32;
  state.active = state.offset = 0;
  reveal(state, props);
}
function capture(state: State, choice: Choice | null = null): Action[] {
  const basis = state.observation?.snapshot.text;
  if (!basis || state.pending?.kind === 'text') return [];
  if (basis.bytes > 1024) {
    state.submitted = null;
    state.queryError = 'long';
    return [];
  }
  // A user choice supersedes the directory read. Its original accepted job
  // still settles; a late reply cannot become the pending choice's result.
  const key = `palette-${++state.serial}`;
  state.pending = { kind: 'text', key, basis, choice };
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
          child: 'palette-query',
          method: 'snapshot',
          input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
        },
      },
    },
  ];
}
function page(state: State, position: AgentList['position']): Action[] {
  const key = `palette-${++state.serial}`;
  state.pending = { kind: 'page', key, query: state.query };
  return [
    {
      key,
      binding: 'agent',
      member: 0,
      method: 'list',
      input: { filter: 'all', position },
      resources: {},
    },
  ];
}
function resume(state: State, props: PaletteProps): Action[] {
  if (
    !interactive(state, props) ||
    state.queryError ||
    !sameText(state.submitted, state.observation?.snapshot.text ?? null)
  )
    state.submitted = null;
  if (!interactive(state, props) || state.pending || state.queryError) return [];
  if (dirty(state)) return capture(state);
  const active = rows(state, props)[state.active]?.key;
  while (
    state.query &&
    !state.failed &&
    state.source &&
    state.scanOffset < state.source.sessions.length &&
    state.matches.length < state.matchLimit
  ) {
    const session = state.source.sessions[state.scanOffset++];
    if (
      matches(
        state.query,
        [session.id, session.title, session.workspace.path, ...session.labels].join(' '),
      )
    )
      state.matches.push(session);
  }
  if (rows(state, props)[state.active]?.key !== active) reveal(state, props);
  if (state.submitted) {
    const row = rows(state, props)[state.active];
    if (row) {
      state.submitted = null;
      // Clipboard admission belongs to the physical input, not a later reply.
      if (
        enabled(state, props, row.choice) &&
        !(row.choice.kind === 'command' && row.choice.command.copyText)
      )
        return capture(state, row.choice);
    }
  }
  if (!state.query || state.failed || state.matches.length >= state.matchLimit) {
    state.submitted = null;
    return [];
  }
  if (!state.source) return page(state, { kind: 'start' });
  if (state.matches.length < state.matchLimit && state.source.after)
    return page(state, {
      kind: 'after',
      revision: state.source.revision,
      cursor: state.source.after,
    });
  state.submitted = null;
  return [];
}
function hasMore(state: State) {
  return (
    !!state.source && (state.scanOffset < state.source.sessions.length || !!state.source.after)
  );
}
function frame<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  props: PaletteProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = presentation.paint(props.theme),
    text = copy(props.locale);
  const all = rows(state, props);
  const buttons = all.slice(state.offset, state.offset + 32).map(({ key, choice }, index) => {
    const active = index + state.offset === state.active;
    const label =
      choice.kind === 'command'
        ? choice.command.label + (choice.command.hint ? `  ${choice.command.hint}` : '')
        : `${text.paletteSession} · ${choice.session.title}`;
    const button = ui.button(
      key,
      (active ? (props.theme.ascii ? '> ' : '› ') : '  ') + compactLabel(label, 68),
      !enabled(state, props, choice),
      false,
      { align: 'start', ...{ background: active ? props.theme.selected : 0 } },
      choice.kind === 'command' ? choice.command.copyText : undefined,
    );

    return button;
  });
  const available =
    interactive(state, props) &&
    !dirty(state) &&
    !state.queryError &&
    state.pending?.kind !== 'text';
  const selected = all[state.active]?.choice;
  const submitCopy =
    selected?.kind === 'command' &&
    selected.command.copyText &&
    enabled(state, props, selected) &&
    state.observation
      ? { basis: state.observation.snapshot.text, text: selected.command.copyText }
      : null;
  const view = ui.box(
    'palette-content',
    [
      presentation.slot(
        'palette-query',
        'input',
        {
          appearance: props.theme.input,
          accessibilityLabel: text.paletteFilter,
          placeholder: text.paletteFilter,
          autoFocus: true,
          readOnly: false,
          disabled: !interactive(state, props),
          tabIndex: 1,
          submitCopy,
        },
        { changed: 'query', submitted: 'choose' },
      ),
      ...(state.queryError
        ? [
            ui.text(
              'query-error',
              state.queryError === 'long' ? text.searchTooLong : text.paletteSearchFailed,
              props.theme.muted,
            ),
          ]
        : []),
      ...(state.failed
        ? [ui.text('directory-error', text.paletteSearchFailed, props.theme.muted)]
        : []),
      ...(state.pending?.kind === 'page'
        ? [ui.text('directory-loading', text.paletteSearching, props.theme.muted)]
        : []),
      ...(!all.length && !state.pending && !state.queryError && !state.failed && !dirty(state)
        ? [ui.text('palette-empty', text.paletteEmpty, props.theme.muted)]
        : []),
      presentation.reading(
        'palette-rows',
        {
          content: ui.box('palette-viewport', buttons, {
            scroll: 'anchor',
            gap: 0,
            terminal: { grow: 1, basis: 0 },
          }),
          command: state.reveal,
        },
        { input: 'row-input', observed: 'row-viewport' },
      ),
      ui.box(
        'palette-controls',
        [
          ...((state.query || state.queryError === 'unavailable') && state.queryError !== 'long'
            ? [
                ui.button(
                  'refresh',
                  props.theme.ascii ? 'r' : '↻',
                  !!state.pending || !interactive(state, props),
                  false,
                  { background: 0 },
                  undefined,
                  text.paletteRefresh,
                ),
              ]
            : []),
          ui.button('close', text.paletteClose, false, false, { background: 0, align: 'start' }),
        ],
        { row: true, gap: 1, terminal: { wrap: true } },
      ),
    ],
    {
      visible: state.open && props.open,
      gap: 0,
      terminal: { grow: 1 },
      shortcuts: [
        { key: 'down', modifiers: [], input: 'down', enabled: available },
        { key: 'up', modifiers: [], input: 'up', enabled: available },
        { key: 'home', modifiers: [], input: 'first', enabled: available },
        { key: 'end', modifiers: [], input: 'last', enabled: available },
        { key: 'k', modifiers: ['primary'], input: 'close', enabled: true },
        { key: 'pageup', modifiers: [], input: 'page-up', enabled: available },
        { key: 'pagedown', modifiers: [], input: 'page-down', enabled: available },
        { key: 'escape', modifiers: [], input: 'close', enabled: true },
      ],
    },
  );
  const out = output(state, view, events);
  out.actions = actions;
  return out;
}
function close(state: State) {
  state.submitted = null;
  if (state.pending?.kind === 'text') state.pending.choice = null;
}
export function palette<N extends Node>(
  presentation: Presentation<N>,
): Component<State, PaletteProps, N> {
  return {
    mount(props) {
      const state: State = {
        revision: 0,
        serial: 0,
        open: props.open,
        observation: null,
        basis: null,
        query: '',
        queryError: null,
        pending: null,
        submitted: null,
        source: null,
        scanOffset: 0,
        matches: [],
        failed: false,
        batch: 0,
        matchLimit: 32,
        active: 0,
        offset: 0,
        reveal: null,
      };
      reveal(state, props);
      return frame(presentation, state, props);
    },
    update(previous, props) {
      const state = { ...previous, revision: previous.revision + 1 };
      if (!props.open) {
        close(state);
        state.open = false;
      } else if (!state.open) {
        state.open = true;
        state.active = state.offset = 0;
        reveal(state, props);
      }
      state.active = Math.min(state.active, Math.max(0, rows(state, props).length - 1));
      state.offset = Math.floor(state.active / 32) * 32;
      const row = rows(state, props)[state.active];
      if (
        row &&
        state.reveal?.target.kind === 'reveal_child' &&
        state.reveal.target.key !== row.key
      )
        reveal(state, props);
      return frame(presentation, state, props, resume(state, props));
    },
    event(state, input, props) {
      state.revision++;
      const actions: Action[] = [],
        events: Emission[] = [];
      const choose = (choice: Choice) => {
        if (choice.kind === 'command' && choice.command.copyText) {
          // The visible action owns physical clipboard admission. Enter also
          // checks the original native query revision before emitting submit.
          close(state);
          events.push({ name: 'invoked', value: { id: choice.command.id } });
        } else actions.push(...capture(state, choice));
      };
      if (input.kind === 'component' && input.child === 'palette-rows' && input.name === 'input')
        input = input.event as ComponentInput;
      const direction = scrollBoundary(state, input, 'palette-rows');
      if (direction && !state.pending)
        input = {
          kind: 'raw',
          key:
            direction === 'up'
              ? 'previous'
              : state.offset + 32 < rows(state, props).length
                ? 'next'
                : 'more',
          event: {},
        };
      if (input.kind === 'action' && input.key === state.pending?.key) {
        const request = state.pending;
        state.pending = null;
        const result = input.event as ActionOutcome;
        if (request.kind === 'text') {
          if (!sameText(request.basis, state.observation?.snapshot.text ?? null))
            actions.push(...resume(state, props));
          else if (result.status !== 'succeeded') {
            state.submitted = null;
            state.queryError = 'unavailable';
          } else {
            const query = (result.value as TextChunk).text.trim();
            if ([...query].length > 256) {
              state.submitted = null;
              state.queryError = 'long';
            } else if (request.choice) {
              if (enabled(state, props, request.choice) && query === state.query) {
                close(state);
                events.push(
                  request.choice.kind === 'command'
                    ? { name: 'invoked', value: { id: request.choice.command.id } }
                    : { name: 'selected', value: request.choice.session },
                );
              } else actions.push(...resume(state, props));
            } else {
              const changed = query !== state.query;
              state.query = query;
              state.basis = request.basis;
              state.queryError = null;
              if (changed) reset(state, props);
              actions.push(...resume(state, props));
            }
          }
        } else if (request.query !== state.query || dirty(state))
          actions.push(...resume(state, props));
        else if (result.status !== 'succeeded') {
          state.submitted = null;
          state.failed = true;
        } else {
          const next = result.value as AgentSessionPage;
          // Never merge a replacement catalog with the current search batch.
          if (state.source && state.source.revision !== next.revision) {
            state.submitted = null;
            state.failed = true;
          } else {
            state.source = next;
            state.scanOffset = 0;
            actions.push(...resume(state, props));
          }
        }
      } else if (input.kind === 'component' && input.child === 'palette-query') {
        if (input.name === 'changed') {
          state.observation = input.event as EditorObservation;
          state.queryError = null;
          if (!state.observation.snapshot.text.bytes) {
            const changed = state.query !== '';
            state.query = '';
            state.basis = state.observation.snapshot.text;
            if (changed) reset(state, props);
          }
          actions.push(...resume(state, props));
        } else if (input.name === 'submitted') {
          const row = rows(state, props)[state.active];
          if (row && enabled(state, props, row.choice)) choose(row.choice);
          else if (interactive(state, props) && !state.queryError) {
            state.submitted = state.observation?.snapshot.text ?? null;
            actions.push(...resume(state, props));
          }
        }
      } else if (input.kind === 'raw') {
        if (input.key === 'close') {
          close(state);
          events.push({ name: 'closed', value: {} });
        } else if (interactive(state, props)) {
          const all = rows(state, props);
          const row = all.find((row) => row.key === input.key);
          if (row && enabled(state, props, row.choice)) choose(row.choice);
          else if (!dirty(state) && !state.queryError && state.pending?.kind !== 'text') {
            if (input.key === 'first' || input.key === 'last') {
              state.active = input.key === 'first' ? 0 : Math.max(0, all.length - 1);
              state.offset = Math.floor(state.active / 32) * 32;
              reveal(state, props);
            } else if (
              ['up', 'down', 'page-up', 'page-down', 'previous', 'next'].includes(input.key)
            ) {
              const delta =
                input.key === 'up'
                  ? -1
                  : input.key === 'down'
                    ? 1
                    : ['page-up', 'previous'].includes(input.key)
                      ? -32
                      : 32;
              state.active = Math.max(0, Math.min(all.length - 1, state.active + delta));
              state.offset = Math.floor(state.active / 32) * 32;
              reveal(state, props);
            } else if (input.key === 'more' && !state.pending && hasMore(state)) {
              state.matchLimit += 32;
              actions.push(...resume(state, props));
            }
          }
          if (input.key === 'refresh' && !state.pending) {
            if (state.queryError === 'unavailable') state.queryError = null;
            reset(state, props);
            actions.push(...resume(state, props));
          }
        }
      }
      return frame(presentation, state, props, actions, events);
    },
  };
}
export function activate() {
  return {
    CommandPalette: { kind: 'component', contract: paletteContract, component: ['component'] },
  };
}
