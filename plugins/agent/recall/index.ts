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

import { editorMenu } from '../../shared/editor-menu.js';
import type {
  Action,
  ActionOutcome,
  AgentRecallPage,
  AgentSession,
  AgentSessionPage,
  Component,
  ComponentInput,
  EditorObservation,
  Emission,
  InputProps,
  ReadingCommand,
  ReadingObservation,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { recallContract, type RecallProps } from '../../shared/contracts.js';
import { pageHeader, withPageShell, type PageState } from '../../shared/page-shell.js';
import { copy, type Copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';

const fields = ['query', 'since', 'until'] as const;
type Field = (typeof fields)[number];
interface State extends PageState {
  revision: number;
  serial: number;
  observations: Partial<Record<Field, EditorObservation>>;
  capture: { observations: Record<Field, EditorObservation>; values: string[] } | null;
  pending: { key: string; kind: 'field' | 'search' | 'catalog' } | null;
  page: AgentRecallPage | null;
  session: AgentSession | null;
  picker: boolean;
  catalog: AgentSessionPage | null;
  notice: keyof Copy | null;
  focus: ReadingCommand | null;
  scrollRevision: number;
}
function day(value: string, end: boolean) {
  if (!value) return null;
  if (!/^[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(value) || Number(value.slice(0, 4)) < 1970)
    throw Error('date');
  const date = new Date(value + 'T00:00:00.000Z');
  if (!Number.isFinite(date.getTime()) || date.toISOString().slice(0, 10) !== value)
    throw Error('date');
  return date.getTime() + (end ? 86_400_000 - 1 : 0);
}
function request(
  state: State,
  actions: Action[],
  kind: NonNullable<State['pending']>['kind'],
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  const key = `recall-${++state.serial}`;
  state.pending = { key, kind };
  actions.push({
    key,
    binding: kind === 'field' ? 'reader' : 'agent',
    member: 0,
    method,
    input,
    resources,
  });
}
function capture(state: State, actions: Action[]) {
  const form = state.capture!;
  if (form.values.length === fields.length) {
    state.capture = null;
    try {
      const [query, start, end] = form.values.map((value) => value.trim());
      const terms = query!.split(/\s+/u).filter(Boolean),
        since = day(start!, false),
        until = day(end!, true);
      if (
        !terms.length ||
        terms.length > 8 ||
        terms.some((term) => [...term].length > 500) ||
        (since !== null && until !== null && since > until)
      )
        throw Error('query');
      request(state, actions, 'search', 'recall', {
        terms,
        question: null,
        limit: 20,
        session_id: state.session?.id ?? null,
        since,
        until,
      });
    } catch {
      state.notice = 'recallInvalid';
    }
    return;
  }
  const name = fields[form.values.length]!,
    observation = form.observations[name],
    basis = observation.snapshot.text;
  request(
    state,
    actions,
    'field',
    'read',
    { start: 0, end: basis.bytes },
    {
      text: {
        child: `recall-${name}`,
        method: 'snapshot',
        input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
      },
    },
  );
}
function find(state: State, actions: Action[]) {
  if (state.pending) return;
  if (
    fields.some(
      (name) =>
        !state.observations[name] ||
        state.observations[name]!.snapshot.marked ||
        state.observations[name]!.snapshot.text.bytes > (name === 'query' ? 512 : 10),
    )
  ) {
    state.notice = 'recallInvalid';
    return;
  }
  state.notice = null;
  state.capture = {
    observations: { ...state.observations } as Record<Field, EditorObservation>,
    values: [],
  };
  capture(state, actions);
}
function catalog(
  state: State,
  actions: Action[],
  direction: 'start' | 'before' | 'after' = 'start',
) {
  const page = state.catalog,
    cursor = direction === 'before' ? page?.before : page?.after;
  if (direction !== 'start' && !cursor) return;
  request(state, actions, 'catalog', 'list', {
    filter: 'all',
    position:
      direction === 'start'
        ? { kind: 'start' }
        : direction === 'after' && page!.sessions.length > 1
          ? { kind: 'at', cursor: page!.sessions[Math.floor(page!.sessions.length / 2)]!.id }
          : {
              kind: direction,
              cursor:
                direction === 'before'
                  ? (page!.sessions[Math.floor(page!.sessions.length / 2)]?.id ?? cursor)
                  : cursor,
              revision: page!.revision,
            },
  });
}
export function recall<N extends Node>(
  presentation: Presentation<N>,
): Component<State, RecallProps, N> {
  function frame(
    state: State,
    props: RecallProps,
    actions: Action[] = [],
    events: Emission[] = [],
  ) {
    const ui = presentation.paint(props.theme),
      text = copy(props.locale),
      disabled = !props.open || props.busy || !!state.pending;
    const rows: N[] = [];
    if (state.picker) {
      rows.push(ui.button('all', text.recallAll, disabled));
      for (const session of state.catalog?.sessions ?? [])
        rows.push(
          ui.item(
            'session-' + session.id,
            ui.text('label-' + session.id, compactLabel(session.title, 120)),
            state.session?.id === session.id,
            !props.open || props.busy,
            { followFocus: true, submit: true },
          ),
        );
    } else {
      for (const [index, session] of (state.page?.titles ?? []).entries())
        rows.push(
          ui.button('title-' + index, compactLabel(session.title, 120), disabled, false, {
            align: 'start',
          }),
        );
      for (const [index, passage] of (state.page?.passages ?? []).entries())
        rows.push(
          ui.box(
            'passage-' + index,
            [
              ui.button(
                'open-' + index,
                compactLabel(passage.session.title, 120),
                disabled,
                false,
                { align: 'start' },
              ),
              ui.text('excerpt-' + index, passage.excerpt, props.theme.muted),
            ],
            { gap: 0 },
          ),
        );
      if (state.page && !rows.length)
        rows.push(ui.text('recall-empty', text.recallEmpty, props.theme.muted));
      if (state.page && !state.page.complete)
        rows.push(ui.text('recall-partial', text.recallPartial, props.theme.muted));
    }
    const field = (name: Field, label: string) =>
      ui.box(
        'recall-field-' + name,
        [
          ...(name === 'query' ? [] : [ui.text('recall-label-' + name, label, props.theme.muted)]),
          presentation.slot(
            'recall-' + name,
            'input',
            {
              menu: editorMenu(props.theme, props.locale),
              appearance: props.theme.input,
              accessibilityLabel: label,
              placeholder: name === 'query' ? text.recallFind : 'YYYY-MM-DD',
              autoFocus: name === 'query',
              tabIndex: fields.indexOf(name),
              readOnly: false,
              disabled: disabled || state.picker,
            } satisfies InputProps,
            { changed: 'changed', submitted: 'find' },
          ),
        ],
        { gap: 0, terminal: { grow: 1 } },
      );
    const form = ui.box(
      'recall-form',
      [
        ui.text('recall-intro', text.recallIntro, props.theme.muted),
        ui.box(
          'recall-search',
          [field('query', text.recallFind), ui.button('find', text.recallFind, disabled, true)],
          { row: true, gap: 1 },
        ),
        ui.box(
          'recall-dates',
          [field('since', text.recallSince), field('until', text.recallUntil)],
          { gap: 1, wide: { min_width: 45, direction: 'row' } },
        ),
        ui.text('recall-date-hint', text.recallDateHint, props.theme.muted),
        ui.box(
          'recall-controls',
          [
            ui.button(
              'scope',
              state.session ? compactLabel(state.session.title, 40) : text.recallAll,
              disabled,
            ),
          ],
          { row: true, gap: 1, terminal: { wrap: true } },
        ),
      ],
      { visible: !state.picker, gap: 1 },
    );
    const content = ui.box('recall-content', rows, {
      scroll: 'anchor',
      gap: 1,
      focusGroup: state.picker,
      terminal: { grow: 1, basis: 0 },
    });
    const view = ui.box(
      'recall-page',
      [
        ...pageHeader(
          presentation,
          state,
          props,
          events,
          state.picker ? text.recallChoose : text.recallTitle,
          'back',
          false,
        ),
        ...(state.notice ? [ui.text('recall-notice', text[state.notice], props.theme.error)] : []),
        ...(state.pending ? [ui.text('recall-loading', text.loading, props.theme.muted)] : []),
        form,
        presentation.reading(
          'recall-reading',
          { content, command: state.focus },
          { input: 'recall-input', observed: 'recall-viewport' },
        ),
        ...(state.picker
          ? [
              ui.box('recall-paging', [ui.button('refresh', text.directoryRefresh, disabled)], {
                row: true,
                gap: 1,
              }),
            ]
          : []),
      ],
      {
        visible: props.open,
        padding: props.shell ? 0 : 1,
        gap: 1,
        autoFocus: true,
        shortcuts: [{ key: 'escape', modifiers: [], input: 'back', enabled: true }],
        terminal: { grow: 1 },
      },
    );
    const result = output(state, view, events);
    result.actions = actions;
    return result;
  }
  return withPageShell<State, RecallProps, N>(
    {
      mount(props) {
        return frame(
          {
            revision: 0,
            serial: 0,
            observations: {},
            capture: null,
            pending: null,
            page: null,
            session: null,
            picker: false,
            catalog: null,
            notice: null,
            focus: null,
            scrollRevision: 0,
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
        const actions: Action[] = [],
          events: Emission[] = [];
        if (
          input.kind === 'component' &&
          input.child === 'recall-reading' &&
          input.name === 'input'
        )
          input = input.event as ComponentInput;
        if (
          input.kind === 'component' &&
          input.child === 'recall-reading' &&
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
            if (state.picker && props.open && !props.busy && !state.pending) {
              if (observed.scroll.direction === 'up' && observed.viewport.atStart)
                catalog(state, actions, 'before');
              else if (observed.scroll.direction === 'down' && observed.viewport.atEnd)
                catalog(state, actions, 'after');
            }
          }
        } else if (input.kind === 'action' && input.key === state.pending?.key) {
          const pending = state.pending,
            result = input.event as ActionOutcome;
          state.pending = null;
          if (result.status !== 'succeeded') {
            state.capture = null;
            state.notice = 'recallUnavailable';
          } else if (pending.kind === 'search') {
            state.page = result.value as AgentRecallPage;
            state.focus = {
              sequence: (state.focus?.sequence ?? 0) + 1,
              target: {
                kind: 'reveal_child',
                key: state.page.titles.length ? 'title-0' : 'passage-0',
              },
            };
          } else if (pending.kind === 'catalog') state.catalog = result.value as AgentSessionPage;
          else if (state.capture) {
            const form = state.capture,
              name = fields[form.values.length]!,
              basis = form.observations[name].snapshot.text,
              chunk = result.value as TextChunk;
            if (
              !sameValue(chunk.metadata, basis) ||
              !sameValue(state.observations[name]?.snapshot.text, basis) ||
              new TextEncoder().encode(chunk.text).length !== basis.bytes
            ) {
              state.capture = null;
              state.notice = 'recallUnavailable';
            } else {
              form.values.push(chunk.text);
              capture(state, actions);
            }
          }
        } else if (input.kind === 'component') {
          const name = fields.find(
            (name) => input.kind === 'component' && input.child === 'recall-' + name,
          );
          if (name && input.name === 'changed')
            state.observations[name] = input.event as EditorObservation;
          else if (name && input.name === 'submitted' && props.open && !props.busy && !state.picker)
            find(state, actions);
        } else if (input.kind === 'raw' && props.open) {
          if (input.key === 'back') {
            if (state.picker) state.picker = false;
            else events.push({ name: 'closed', value: {} });
          } else if (!props.busy && !state.pending) {
            if (input.key === 'find' && !state.picker) find(state, actions);
            else if (input.key === 'scope') {
              state.picker = true;
              state.notice = null;
              catalog(state, actions);
            } else if (state.picker && input.key === 'refresh') catalog(state, actions);
            else if (state.picker && input.key === 'all') {
              state.session = null;
              state.picker = false;
            } else if (state.picker && input.key.startsWith('session-')) {
              const session = state.catalog?.sessions.find(
                (session) => input.kind === 'raw' && 'session-' + session.id === input.key,
              );
              if (session && true && (input.event as { kind?: string }).kind === 'activate') {
                const at = state.catalog!.sessions.indexOf(session);
                if (at >= state.catalog!.sessions.length - 3) catalog(state, actions, 'after');
                else if (at < 3) catalog(state, actions, 'before');
              } else if (session) {
                state.session = session;
                state.picker = false;
              }
            } else if (!state.picker && input.key.startsWith('title-')) {
              const session = state.page?.titles[Number(input.key.slice(6))];
              if (session) events.push({ name: 'selected', value: { session, anchor: null } });
            } else if (!state.picker && input.key.startsWith('open-')) {
              const passage = state.page?.passages[Number(input.key.slice(5))];
              if (passage)
                events.push({
                  name: 'selected',
                  value: { session: passage.session, anchor: passage.anchorSequence },
                });
            }
          }
        }
        return frame(state, props, actions, events);
      },
    },
    () => 'back',
  );
}
export function activate() {
  return { Recall: { kind: 'component', contract: recallContract, component: ['component'] } };
}
