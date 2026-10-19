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

import { comparePosition, type RowPosition } from './history-position.js';
import { scrollBoundary } from './scroll.js';
import type {
  Action,
  ActionOutcome,
  ComponentInput,
  EditorObservation,
  EditorTextRef,
  ReadingCommand,
  ReadingObservation,
  TextChunk,
  AgentHistoryMatch,
  AgentHistorySearchPage,
  AgentHistorySourcePage,
  HistorySource,
  SourcePosition,
} from '../../packages/plugin-sdk/src/index.js';
import type { TimelineProps } from './contracts.js';
import { copy } from './locale-catalogs/conversation-search.js';
import { toolCards } from './tool-cards.js';
import { toolPresentation } from '../agent/tool-details/index.js';
import { messageView, sourceFieldLabel } from './message-view.js';
import { compactLabel, type Node, type Presentation } from './view.js';

export interface ConversationSearch {
  context: NonNullable<TimelineProps['search']>;
  key: string;
  closing: boolean;
  serial: number;
  history: HistorySearch | null;
  observation: EditorObservation | null;
  basis: EditorTextRef | null;
  query: string;
  pending: { key: string; basis: EditorTextRef } | null;
  waiting: number | null;
  failed: boolean;
}
export interface SearchReading {
  revision: number;
  expanded: string[];
  command: ReadingCommand | null;
  observation: ReadingObservation | null;
  search: ConversationSearch | null;
}
function command(state: SearchReading, target: ReadingCommand['target']) {
  const sequence = (state.command?.sequence ?? 0) + 1;
  state.command = { sequence, target };
  return sequence;
}
function find(state: SearchReading, step: 'first' | 'next' | 'previous') {
  const search = state.search!;
  search.waiting = command(state, { kind: 'find', query: search.query, step });
}
export function syncSearch(state: SearchReading, context: TimelineProps['search']) {
  if (!context) {
    if (state.search && !state.search.closing) command(state, { kind: 'clear_find' });
    state.search = null;
  } else if (
    !state.search ||
    state.search.context.epoch !== context.epoch ||
    state.search.context.sessionId !== context.sessionId
  ) {
    state.search = {
      context,
      key: `find-query-${state.revision}`,
      closing: false,
      serial: 0,
      history: null,
      observation: null,
      basis: null,
      query: '',
      pending: null,
      waiting: null,
      failed: false,
    };
    find(state, 'first');
  }
}
function same(a: EditorTextRef | null, b: EditorTextRef | null) {
  return a?.owner === b?.owner && a?.revision === b?.revision && a?.bytes === b?.bytes;
}
export function searchReady(search: ConversationSearch) {
  return (
    !search.closing &&
    !search.pending &&
    !search.failed &&
    !!search.observation &&
    same(search.basis, search.observation.snapshot.text)
  );
}
function capture(state: SearchReading, actions: Action[]) {
  const search = state.search!,
    basis = search.observation?.snapshot.text;
  if (search.pending || !basis || same(basis, search.basis)) return;
  if (basis.bytes > 512) {
    search.failed = true;
    return;
  }
  const key = `${search.key}-${++search.serial}`;
  search.pending = { key, basis };
  actions.push({
    key,
    binding: 'reader',
    member: 0,
    method: 'read',
    input: { start: 0, end: basis.bytes },
    resources: {
      text: {
        child: search.key,
        method: 'snapshot',
        input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
      },
    },
  });
}
export function searchEvent(
  state: SearchReading,
  input: ComponentInput,
  props: TimelineProps,
  actions: Action[],
): 'closed' | 'handled' | null {
  const search = state.search;
  if (!search || search.closing) return null;
  if (input.kind === 'action' && input.key === search.pending?.key) {
    const request = search.pending;
    search.pending = null;
    const result = input.event as ActionOutcome;
    if (!same(request.basis, search.observation?.snapshot.text ?? null)) capture(state, actions);
    else if (result.status !== 'succeeded') search.failed = true;
    else {
      const chunk = result.value as TextChunk;
      const bytes = utf8Bytes(chunk.text);
      if (
        !same(chunk.metadata as EditorTextRef, request.basis) ||
        bytes !== request.basis.bytes ||
        bytes > 512
      )
        search.failed = true;
      else {
        search.basis = request.basis;
        search.failed = false;
        if (chunk.text !== search.query) {
          search.query = chunk.text;
          if (search.history) restartHistory(state, props, actions);
          else find(state, 'first');
        }
      }
    }
    return 'handled';
  }
  if (input.kind === 'component' && input.child === search.key) {
    if (input.name === 'changed') {
      search.observation = input.event as EditorObservation;
      search.failed = false;
      capture(state, actions);
    } else if (input.name === 'submitted' && searchReady(search)) {
      navigate(state, true, actions);
    }
    return 'handled';
  }
  if (historyEvent(state, input, props, actions)) return 'handled';
  if (input.kind === 'component' && input.child === 'reading' && input.name === 'observed') {
    const observation = state.observation,
      hit = observation?.find;
    if (
      hit &&
      hit.sequence === search.waiting &&
      observation?.receipt?.sequence === search.waiting &&
      observation.receipt.status === 'applied' &&
      searchReady(search) &&
      !search.history
    ) {
      search.waiting = null;
      if (hit.active) {
        const path = hit.active.path;
        const card = toolCards(props.messages).find(
          (card) =>
            (path.length === 1 &&
              [card.message, card.result].some(
                (message) => message && path[0] === 'text-' + message.contentKey,
              )) ||
            (path.length > 1 &&
              props.toolContext &&
              toolPresentation(card.message.tool?.call?.presentation) &&
              path[0] === 'tool-details-' + card.message.id),
        );
        if (card) {
          const id = card.message.id;
          if (!state.expanded.includes(id)) state.expanded.push(id);
          command(state, { kind: 'reveal_source', ...hit.active, find_sequence: hit.sequence });
        }
      }
    }
    return null;
  }
  if (input.kind === 'raw') {
    if (input.key === 'find-scope' || input.key === 'find-scope-label-input') {
      if (search.history) {
        search.history = null;
        find(state, 'first');
      } else {
        command(state, { kind: 'clear_find' });
        restartHistory(state, props, actions);
      }
      return 'handled';
    }
    if (input.key === 'find-close') {
      search.closing = true;
      search.waiting = null;
      command(state, { kind: 'clear_find' });
      return 'closed';
    }
    if (input.key === 'find-next' || input.key === 'find-previous') {
      if (searchReady(search)) navigate(state, input.key === 'find-next', actions);
      return 'handled';
    }
  }
  return null;
}

interface HistorySearch {
  through: number;
  start: RowPosition | null;
  next: RowPosition | null;
  offset: number;
  back: { start: RowPosition | null; offset: number }[];
  matches: AgentHistoryMatch[];
  selected: number;
  reveal: ReadingCommand | null;
  selectLast: boolean;
  scanning: boolean;
  failed: boolean;
  pending: { key: string; after: RowPosition | null } | null;
  preview: {
    key: string;
    hit: AgentHistoryMatch;
    sections: { field: HistorySource['field']; source: HistorySource | null; failed: boolean }[];
    expanded: boolean;
    pending: { key: string; field: HistorySource['field']; position: SourcePosition } | null;
    command: ReadingCommand | null;
    highlight: { sequence: number; stage: 'land' | 'find'; reveal: boolean } | null;
  } | null;
}
function action(search: ConversationSearch, method: string, input: unknown): Action {
  return {
    key: `${search.key}-${++search.serial}`,
    binding: 'agent',
    member: 0,
    method,
    input,
    resources: {},
  };
}
function scan(search: ConversationSearch, actions: Action[], initial = false) {
  const history = search.history!;
  if (
    !search.query ||
    history.pending ||
    (history.next === null && !initial) ||
    history.matches.length >= 32 ||
    history.failed
  ) {
    history.scanning = false;
    return;
  }
  const after = history.next;
  const request = action(search, 'search', {
    sessionId: search.context.sessionId,
    through: history.through,
    after,
    query: search.query,
    caseSensitive: true,
    wholeWord: false,
    includeInternal: false,
  });
  history.scanning = true;
  history.pending = { key: request.key, after };
  actions.push(request);
}
function restartHistory(state: SearchReading, props: TimelineProps, actions: Action[]) {
  const search = state.search!;
  search.waiting = null;
  search.history = {
    through: props.search!.through,
    start: null,
    next: null,
    offset: 0,
    back: [],
    matches: [],
    selected: -1,
    // The result ReadingArea survives query restarts, so its command sequence must too.
    reveal: search.history?.reveal ?? null,
    selectLast: false,
    scanning: false,
    failed: false,
    pending: null,
    preview: null,
  };
  scan(search, actions, true);
}
function readPreview(
  search: ConversationSearch,
  actions: Action[],
  field: HistorySource['field'],
  position: SourcePosition,
) {
  const history = search.history!,
    preview = history.preview!;
  if (preview.pending) return;
  const request = action(search, 'source', {
    sessionId: search.context.sessionId,
    through: history.through,
    source: { kind: 'row', position: preview.hit.position },
    field,
    position,
  });
  preview.pending = { key: request.key, field, position };
  preview.sections.find((section) => section.field === field)!.failed = false;
  actions.push(request);
}
function pick(search: ConversationSearch, index: number, actions: Action[]) {
  const history = search.history!,
    hit = history.matches[index];
  if (!hit || history.selected === index) return;
  history.selected = index;
  history.reveal = {
    sequence: (history.reveal?.sequence ?? 0) + 1,
    target: { kind: 'reveal_child', key: `find-hit-${index}` },
  };
  const sequence = (history.preview?.command?.sequence ?? 0) + 1;
  history.preview = {
    key: `find-preview-reading-${hit.position.sourceSequence}-${hit.position.itemIndex}-${hit.field}`,
    hit,
    sections: [{ field: hit.field, source: null, failed: false }],
    expanded: true,
    pending: null,
    highlight: null,
    command: { sequence, target: { kind: 'start' } },
  };
  readPreview(search, actions, hit.field, { kind: 'start' });
}
function navigate(state: SearchReading, forward: boolean, actions: Action[]) {
  const search = state.search!,
    history = search.history;
  if (!history) {
    find(state, forward ? 'next' : 'previous');
    return;
  }
  const index = history.selected + (forward ? 1 : -1);
  if (index >= 0 && index < history.matches.length) {
    pick(search, index, actions);
    return;
  }
  if (history.scanning || history.failed) return;
  if (forward && history.next !== null) {
    history.back.push({ start: history.start, offset: history.offset });
    if (history.back.length > 64) history.back.shift();
    history.start = history.next;
    history.offset += history.matches.length;
    history.selectLast = false;
  } else if (!forward && history.back.length) {
    const previous = history.back.pop()!;
    history.start = previous.start;
    history.next = previous.start;
    history.offset = previous.offset;
    history.selectLast = true;
  } else return;
  history.matches = [];
  history.selected = -1;
  history.preview = null;
  scan(search, actions, history.start === null);
}
function utf8Bytes(text: string) {
  let bytes = 0;
  for (const character of text) {
    const point = character.codePointAt(0)!;
    bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
  }
  return bytes;
}
function historyEvent(
  state: SearchReading,
  input: ComponentInput,
  props: TimelineProps,
  actions: Action[],
) {
  const search = state.search!,
    history = search.history;
  if (!history) return false;
  if (input.kind === 'action' && input.key === history.pending?.key) {
    const request = history.pending;
    history.pending = null;
    const result = input.event as ActionOutcome;
    if (result.status !== 'succeeded') {
      history.failed = true;
      history.scanning = false;
      return true;
    }
    const page = result.value as AgentHistorySearchPage;
    if (page.through !== history.through) {
      history.failed = true;
      history.scanning = false;
      return true;
    }

    let after = request.after;
    if (
      page.matches.length > 16 ||
      page.matches.some((hit) => {
        const invalid =
          (after !== null && comparePosition(hit.position, after) <= 0) ||
          hit.position.sourceSequence > history.through;
        after = hit.position;
        return invalid;
      }) ||
      (page.nextAfter !== null &&
        ((request.after !== null && comparePosition(page.nextAfter, request.after) <= 0) ||
          (after !== null && comparePosition(page.nextAfter, after) < 0)))
    ) {
      history.failed = true;
      history.scanning = false;
      return true;
    }
    const remaining = 32 - history.matches.length;
    history.matches.push(...page.matches.slice(0, remaining));
    history.next =
      page.matches.length > remaining ? history.matches.at(-1)!.position : page.nextAfter;
    if (!history.selectLast && history.selected < 0 && history.matches.length)
      pick(search, 0, actions);
    scan(search, actions);
    if (!history.scanning && history.selectLast && history.matches.length) {
      history.selectLast = false;
      pick(search, history.matches.length - 1, actions);
    }
    return true;
  }
  const preview = history.preview;
  if (input.kind === 'action' && preview && input.key === preview.pending?.key) {
    const request = preview.pending;
    preview.pending = null;
    const section = preview.sections.find((section) => section.field === request.field)!;
    const result = input.event as ActionOutcome;
    const next = () => {
      const missing = preview.sections.find((section) => !section.source && !section.failed);
      if (missing) readPreview(search, actions, missing.field, { kind: 'start' });
    };
    const fail = () => {
      section.failed = true;
      next();
    };
    if (result.status !== 'succeeded') {
      fail();
      return true;
    }
    const page = result.value as AgentHistorySourcePage;
    if (page.through !== history.through) {
      fail();
      return true;
    }

    const source = page.source,
      end = source.offset + utf8Bytes(source.text),
      position = request.position,
      header = preview.sections.find((section) => section.source)?.source;
    if (
      comparePosition(source.position, preview.hit.position) !== 0 ||
      source.path != null ||
      source.field !== request.field ||
      !source.fields.includes(source.field) ||
      new Set(source.fields).size !== source.fields.length ||
      (header &&
        (source.role !== header.role ||
          source.timestamp !== header.timestamp ||
          JSON.stringify(source.fields) !== JSON.stringify(header.fields) ||
          JSON.stringify(source.tool) !== JSON.stringify(header.tool))) ||
      (!source.text && source.totalBytes > 0) ||
      end > source.totalBytes ||
      (section.source &&
        (source.totalBytes !== section.source.totalBytes ||
          source.format !== section.source.format)) ||
      (position.kind === 'start' && source.offset !== 0) ||
      (position.kind === 'tail' && end !== source.totalBytes) ||
      (position.kind === 'after' && source.offset !== position.offset) ||
      (position.kind === 'before' && end !== position.offset)
    ) {
      fail();
      return true;
    }
    const first = section.source === null;
    section.source = source;
    if (!header)
      preview.sections = source.fields.map((field) =>
        field === source.field ? section : { field, source: null, failed: false },
      );
    // Keep the original selected row bounded: one page per field, fetched serially.
    next();
    if (!first || !header) {
      preview.command = {
        sequence: (preview.command?.sequence ?? 0) + 1,
        target: first
          ? { kind: 'find', query: search.query, step: 'first' }
          : {
              kind: 'reveal_source',
              path: [sourceKey(source)],
              offset:
                position.kind === 'tail' || position.kind === 'before'
                  ? Math.max(0, utf8Bytes(source.text) - utf8Bytes([...source.text].at(-1) ?? ''))
                  : 0,
            },
      };
      preview.highlight = {
        sequence: preview.command.sequence,
        stage: first ? 'find' : 'land',
        reveal: first,
      };
    }
    return true;
  }
  if (
    input.kind === 'component' &&
    input.child === preview?.key &&
    input.name === 'observed' &&
    preview?.highlight
  ) {
    const observation = input.event as ReadingObservation;
    const highlight = preview.highlight;
    if (observation.receipt?.sequence === highlight.sequence) {
      preview.highlight = null;
      if (observation.receipt.status === 'applied') {
        if (highlight.stage === 'land') {
          preview.command = {
            sequence: highlight.sequence + 1,
            target: { kind: 'find', query: search.query, step: 'first' },
          };
          preview.highlight = {
            sequence: preview.command.sequence,
            stage: 'find',
            reveal: highlight.reveal,
          };
        } else if (
          highlight.reveal &&
          observation.find?.active &&
          observation.find.sequence === highlight.sequence
        ) {
          preview.command = {
            sequence: highlight.sequence + 1,
            target: {
              kind: 'reveal_source',
              ...observation.find.active,
              find_sequence: highlight.sequence,
            },
          };
        }
      }
    }
    return true;
  }
  const direction = preview && scrollBoundary(state, input, preview.key);
  if (direction && preview && !preview.pending) {
    const section = (direction === 'up' ? preview.sections : [...preview.sections].reverse()).find(
      (section) =>
        section.source &&
        !section.failed &&
        (direction === 'up'
          ? section.source.offset > 0
          : section.source.offset + utf8Bytes(section.source.text) < section.source.totalBytes),
    );
    if (section)
      input = {
        kind: 'raw',
        key: `find-source-${section.field}-${direction === 'up' ? 'previous' : 'next'}`,
        event: {},
      };
  }
  if (input.kind !== 'raw') return false;
  if (input.key === 'find-restart') {
    if (searchReady(search)) restartHistory(state, props, actions);
    return true;
  }
  if (input.key.startsWith('find-hit-')) {
    if (searchReady(search)) pick(search, Number(input.key.slice('find-hit-'.length)), actions);
    return true;
  }
  if (input.key === 'find-page-up' || input.key === 'find-page-down') {
    if (preview) preview.highlight = null;
    if (preview?.sections.some((section) => section.source))
      preview.command = {
        sequence: (preview.command?.sequence ?? 0) + 1,
        target: { kind: 'scroll_rows', rows: input.key === 'find-page-up' ? -8 : 8 },
      };
    return true;
  }
  if (preview && input.key === 'find-preview-toggle') {
    preview.expanded = !preview.expanded;
    preview.highlight = null;
    return true;
  }
  if (preview && !preview.pending) {
    for (const section of preview.sections) {
      const source = section.source,
        key = `find-source-${section.field}-`;
      const position: SourcePosition | null =
        input.key === key + 'retry' && section.failed
          ? { kind: 'start' }
          : source && input.key === key + 'first' && source.offset > 0
            ? { kind: 'start' }
            : source && input.key === key + 'previous' && source.offset > 0
              ? { kind: 'before', offset: source.offset }
              : source &&
                  input.key === key + 'next' &&
                  source.offset + utf8Bytes(source.text) < source.totalBytes
                ? { kind: 'after', offset: source.offset + utf8Bytes(source.text) }
                : source &&
                    input.key === key + 'last' &&
                    source.offset + utf8Bytes(source.text) < source.totalBytes
                  ? { kind: 'tail' }
                  : null;
      if (position) {
        preview.highlight = null;
        readPreview(search, actions, section.field, position);
        return true;
      }
    }
  }
  return false;
}

function sourceKey(source: HistorySource) {
  return `find-preview-text-${source.position.sourceSequence}-${source.position.itemIndex}-${source.field}-${source.offset}`;
}
export function historyView<N extends Node>(
  presentation: Presentation<N>,
  props: TimelineProps,
  search: ConversationSearch,
): N {
  const history = search.history!,
    ui = presentation.paint(props.theme),
    text = copy(props.locale),
    preview = history.preview;
  const header = preview?.sections.find((section) => section.source)?.source;
  const sections =
    preview?.sections.map((section) => {
      const source = section.source,
        key = `find-source-${section.field}-`,
        pending = !!preview.pending;
      const markdown =
        source?.role === 'assistant' &&
        source.format === 'text' &&
        (source.field === 'body' || source.field === 'thinking') &&
        source.offset === 0 &&
        utf8Bytes(source.text) === source.totalBytes;
      return ui.box(
        'find-section-' + section.field,
        [
          ui.text(
            'find-field-' + section.field,
            sourceFieldLabel(props.locale, section.field),
            props.theme.muted,
          ),
          ...(source
            ? [
                presentation.slot(
                  sourceKey(source),
                  markdown ? 'markdown' : 'source',
                  markdown
                    ? {
                        source: source.text,
                        appearance: props.theme.markdown,
                        focusOnPointer: false,
                        selectable: true,
                        disabled: false,
                      }
                    : {
                        source: source.text,
                        focusOnPointer: false,
                        appearance:
                          source.format === 'text'
                            ? props.theme.markdown.body
                            : props.theme.markdown.code,
                        selection: props.theme.markdown.selection,
                        terminal: props.theme.terminal,
                        disabled: false,
                      },
                  {},
                ),
              ]
            : [
                ui.text(
                  'find-loading-' + section.field,
                  section.failed ? text.sourceFailed : text.loading,
                  props.theme.muted,
                ),
              ]),
          ui.box(
            'find-controls-' + section.field,
            [
              ...(source && source.offset > 0
                ? [ui.button(key + 'first', text.sourceFirst, pending)]
                : []),
              ...(source && source.offset + utf8Bytes(source.text) < source.totalBytes
                ? [ui.button(key + 'last', text.sourceLast, pending)]
                : []),
              ...(section.failed ? [ui.button(key + 'retry', text.sourceRetry, pending)] : []),
            ],
            { row: true, gap: 1, terminal: { wrap: true } },
          ),
        ],
        { gap: 0 },
      );
    }) ?? [];
  const rowBody = ui.box('find-row-fields', sections, { gap: 0 });
  const row =
    header && preview
      ? messageView(presentation, props.theme, props.locale, {
          id: 'find-preview-row',
          role: header.role,
          origin: header.origin,
          model: header.model,
          timestamp: header.timestamp,
          tool: header.tool,
          text:
            preview.sections.find((section) => section.field === 'body')?.source?.text ??
            preview.hit.preview,
          continued: preview.sections.some(
            (section) =>
              !!section.source &&
              (section.source.offset > 0 ||
                utf8Bytes(section.source.text) < section.source.totalBytes),
          ),
          expanded: preview.expanded,
          toggle: 'find-preview-toggle',
          body: rowBody,
          actions: [],
          sourceActions: [],
        })
      : rowBody;
  const rows = history.matches.map((hit, index) => {
    const label = `${index === history.selected ? '› ' : ''}${history.offset + index + 1}. ${compactLabel(hit.preview, 96)}`;
    return ui.button(`find-hit-${index}`, label, !searchReady(search), false, {
      align: 'start',
      ...(index === history.selected ? { background: props.theme.selected } : {}),
    });
  });
  return ui.box(
    'find-history',
    [
      ui.box(
        'find-results',
        [
          presentation.reading(
            'find-results-reading',
            {
              content: ui.box(
                'find-result-list',
                rows.length
                  ? rows
                  : [
                      ui.text(
                        'find-empty',
                        history.scanning ? text.loading : text.findNoResults,
                        props.theme.muted,
                      ),
                    ],
                {
                  scroll: 'manual',
                  wheel: { up: 'find-previous', down: 'find-next' },
                  gap: 0,
                  terminal: { grow: 1, basis: 0 },
                },
              ),
              command: history.reveal,
            },
            { input: 'find-result-input' },
          ),
        ],
        {
          terminal: {
            basis_fraction: {
              numerator: 1,
              denominator: 3,
              min: 1,
              max: Math.max(1, Math.min(6, rows.length)),
            },
          },
        },
      ),
      ui.box('find-divider', [], { background: props.theme.border, terminal: { basis: 1 } }),
      ui.box(
        'find-preview-pane',
        [
          ...(history.failed ? [ui.button('find-restart', text.findFailed)] : []),
          ...(preview
            ? [
                ui.text(
                  'find-preview-label',
                  sourceFieldLabel(props.locale, preview.hit.field),
                  props.theme.muted,
                ),
                ui.text(
                  'find-preview-hit',
                  compactLabel(preview.hit.preview, 160),
                  props.theme.muted,
                ),
                ui.box(
                  'find-preview',
                  [
                    presentation.reading(
                      preview.key,
                      {
                        content: ui.box('find-preview-document', [row], {
                          scroll: 'anchor',
                          gap: 0,
                          terminal: { grow: 1, basis: 0 },
                        }),
                        command: preview.command,
                      },
                      { input: 'find-preview-input', observed: 'find-preview-observed' },
                    ),
                  ],
                  { terminal: { grow: 1, basis: 0 } },
                ),
              ]
            : []),
        ],
        { gap: 0, terminal: { grow: 1, basis: 0 } },
      ),
    ],
    {
      gap: 0,
      terminal: { grow: 1, basis: 0 },
      wide: {
        min_width: 100,
        direction: 'row',
        children: { 'find-results': { basis_fraction: { numerator: 34, denominator: 100 } } },
      },
    },
  );
}
export function searchCount(state: SearchReading) {
  const search = state.search!;
  if (search.pending) return '…';
  const history = search.history;
  if (history)
    return `${history.selected < 0 ? 0 : history.offset + history.selected + 1}/${history.offset + history.matches.length}${history.scanning || history.next !== null ? '+' : ''}`;
  const find = state.observation?.find;
  return `${find?.index ?? 0}/${find?.count ?? 0}${find?.limited ? '+' : ''}`;
}
