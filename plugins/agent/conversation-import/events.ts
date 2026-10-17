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
  AgentDirectoryPage,
  AgentImportChange,
  AgentImportsPage,
  CallerIntent,
  ComponentInput,
  EditorObservation,
  Emission,
  OperationReply,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { type ImportsProps } from '../../shared/contracts.js';
import { scrollRows, resetScroll } from '../../shared/scroll.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';
import {
  capture,
  catalog,
  copies,
  focus,
  history,
  initialize,
  locked,
  mutate,
  notice,
  read,
  request,
  reset,
  saveSource,
  settle,
  sources,
  startForm,
} from './operations.js';
import { formatName, type Format, type State } from './state.js';
import { frame } from './view.js';

export function handleEvent<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  input: ComponentInput,
  props: ImportsProps,
) {
  state.revision++;
  const actions: Action[] = [],
    events: Emission[] = [];
  if (input.kind === 'component' && input.child === 'import-list' && input.name === 'input')
    input = input.event as ComponentInput;
  if (
    input.kind === 'component' &&
    input.child === 'import-list' &&
    input.name === 'observed' &&
    !locked(state, props)
  ) {
    const child = 'import-' + state.route;
    const count =
      state.route === 'catalog'
        ? state.entries.length
        : state.route === 'copies'
          ? state.copies.length
          : (state.preview?.items.length ?? 0);
    if (scrollRows(state, { ...input, child }, child, count))
      input = {
        kind: 'raw',
        key:
          state.route === 'catalog'
            ? 'catalog-more'
            : state.route === 'copies'
              ? 'copies-more'
              : 'preview-more',
        event: {},
      };
  }
  if (input.kind === 'component') {
    const name = (['name', 'database', 'search', 'title'] as const).find(
      (name) => input.kind === 'component' && input.child === 'import-' + name,
    );
    if (name && input.name === 'changed') {
      const field = state.fields[name];
      field.observation = input.event as EditorObservation;
      const receipt = field.observation.receipt;
      if (
        field.command &&
        receipt?.owner === field.command.owner &&
        receipt.sequence === field.command.sequence
      ) {
        field.ready = receipt.outcome.status === 'applied';
        field.command = null;
        field.reset = null;
        if (!field.ready) state.notice = 'importsUnavailable';
      } else initialize(field);
    } else if (name && input.name === 'submitted')
      input = {
        kind: 'raw',
        key: name === 'search' ? 'search' : name === 'title' ? 'review-copy' : 'save-source',
        event: {},
      } as ComponentInput;
  }
  if (
    input.kind === 'component' &&
    state.picker &&
    input.child === `import-${state.picker}-picker` &&
    props.open
  ) {
    if (input.name === 'closed') state.picker = null;
    else if (input.name === 'selected' && !locked(state, props)) {
      const purpose = state.picker;
      state.picker = null;
      request(
        state,
        actions,
        { kind: 'directory', purpose },
        'agent',
        purpose === 'source' ? 'importDirectories' : 'directories',
        { kind: 'children', selection: input.event, after: null },
      );
    }
  } else if (input.kind === 'action' && input.key === state.pending?.key) {
    const r = state.pending.request,
      result = input.event as ActionOutcome;
    state.pending = null;
    if (r.kind === 'history') {
      if (result.status !== 'succeeded') {
        state.historyFailed = true;
        state.notice = 'historyUnavailable';
      } else {
        const items = result.value as CallerIntent[];
        for (const item of items) {
          const recovery = item.recovery as {
            kind?: string;
            input?: AgentImportChange;
          } | null;
          if (
            item.reference.method === 'import' &&
            item.reference.contract.name === 'maka.agent.Agent' &&
            recovery?.kind === 'conversation-import/v1' &&
            recovery.input
          ) {
            state.original = item.reference;
            state.current = { input: recovery.input, reference: item.reference };
          }
        }
        const last = items.at(-1);
        if (last && last.cursor > r.after) history(state, actions, last.cursor);
        else if (last) {
          state.historyFailed = true;
          state.notice = 'historyUnavailable';
        } else {
          state.restoring = false;
          if (state.original)
            request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.original);
          else sources(state, actions);
        }
      }
    } else if (r.kind === 'mutate') {
      if (result.status === 'operation') settle(state, result.reply, actions, events);
      else if (result.status === 'not_accepted') {
        state.current = null;
        state.notice = 'importsUnavailable';
      } else state.notice = 'importsUnknown';
    } else if (r.kind === 'receipt') {
      if (result.status === 'succeeded')
        settle(state, result.value as OperationReply, actions, events);
      else state.notice = 'importsUnknown';
    } else if (r.kind === 'field') {
      const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
      if (
        !chunk ||
        !sameValue(chunk.metadata, r.observation.snapshot.text) ||
        new TextEncoder().encode(chunk.text).length !== r.observation.snapshot.text.bytes
      )
        state.notice = 'importsUnavailable';
      else if (r.field === 'name' && state.form) {
        state.capturedName = chunk.text;
        if (state.form.format === 'open_code') capture(state, actions, 'database');
        else saveSource(state, actions, null);
      } else if (r.field === 'database' && state.form) saveSource(state, actions, chunk.text);
      else if (r.field === 'search' && state.route === 'catalog') {
        if ([...chunk.text].length > 1024 || /[\u0000-\u001f\u007f-\u009f]/u.test(chunk.text))
          state.notice = 'importsInvalid';
        else {
          state.query = chunk.text;
          catalog(state, actions);
        }
      } else if (r.field === 'title' && state.preview && state.destination) {
        const title = chunk.text.trim();
        if (!title || [...title].length > 128 || /[\u0000-\u001f\u007f-\u009f]/u.test(title))
          state.notice = 'importsInvalid';
        else
          state.confirm = {
            kind: 'copy',
            title,
            selection: state.preview.selection,
            directory: state.destination.selection,
          };
      }
    } else if (result.status !== 'succeeded') state.notice = 'importsUnavailable';
    else if (r.kind === 'directory') {
      const page = result.value as AgentDirectoryPage;
      if (page.kind !== 'children') state.notice = 'importsUnavailable';
      else if (r.purpose === 'source' && state.form) state.form.directory = page.directory;
      else if (r.purpose === 'destination') state.destination = page.directory;
    } else {
      const value = result.value as AgentImportsPage;
      if (value.kind === 'sources' && r.kind === 'sources') {
        state.sources = value.sources;
        state.sourcesRevision = value.revision;
        focus(presentation, state, value.sources[0] ? 'source-' + value.sources[0].id : null);
      } else if (value.kind === 'catalog' && r.kind === 'catalog') {
        state.entries = r.after ? [...state.entries, ...value.entries] : value.entries;
        state.catalogNext = value.next;
        state.route = 'catalog';
        if (!r.after) focus(presentation, state, value.entries.length ? 'entry-0' : null);
      } else if (value.kind === 'preview' && r.kind === 'preview') {
        if (!state.preview || !sameValue(state.preview.selection, value.preview.selection)) {
          reset(
            state,
            'title',
            [...value.preview.title].slice(0, 128).join('') ||
              formatName(
                state.sources.find((source) => source.id === value.preview.selection.source.id)
                  ?.location.kind ?? 'codex',
              ),
          );
          state.destination = null;
        }
        if (!r.after) resetScroll(state, 'import-preview');
        state.preview =
          r.after && state.preview && sameValue(state.preview.selection, value.preview.selection)
            ? { ...value.preview, items: [...state.preview.items, ...value.preview.items] }
            : value.preview;
        state.route = 'preview';
      } else if (value.kind === 'copies' && r.kind === 'copies') {
        state.copies = r.after ? [...state.copies, ...value.sessions] : value.sessions;
        state.copiesNext = value.next;
        state.route = 'copies';
        if (!r.after)
          focus(presentation, state, value.sessions[0] ? 'copy-' + value.sessions[0].id : null);
      } else state.notice = notice(value.kind);
    }
  } else if (input.kind === 'raw' && props.open) {
    const key = input.key.startsWith('footer-') ? input.key.slice(7) : input.key;
    if (key === 'back' && !state.confirm && !state.picker) {
      if (state.current || state.restoring || state.route === 'sources')
        events.push({ name: 'closed', value: {} });
      else if (!state.pending) {
        if (state.route === 'preview') {
          state.route = 'catalog';
          state.preview = null;
        } else {
          state.route = 'sources';
          state.form = null;
          sources(state, actions);
        }
      }
    } else if (key === 'history-retry' && state.historyFailed && !state.pending)
      history(state, actions);
    else if (
      ['receipt', 'confirm-receipt'].includes(key) &&
      state.current?.reference &&
      !state.pending
    )
      request(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.current.reference);
    else if (key === 'confirm-cancel' && !state.pending && !state.current) state.confirm = null;
    else if (!locked(state, props) && !state.picker && (!state.confirm || key === 'confirm')) {
      if (key === 'confirm' && state.confirm) mutate(state, actions, state.confirm);
      else if (key === 'open-latest' && state.latest)
        events.push({ name: 'selected', value: { session: state.latest } });
      else if (state.route === 'sources') {
        if (key === 'add') startForm(state, null);
        else if (key === 'refresh') {
          state.notice = null;
          sources(state, actions);
        } else if (key === 'copies') {
          state.notice = null;
          copies(state, actions);
        } else
          for (const source of state.sources) {
            if (key === 'source-' + source.id) {
              state.selected = key;
              if ((input.event as { kind?: string }).kind !== 'submit') continue;
            }
            if (key === 'browse-' + source.id || key === 'source-' + source.id) {
              state.source = { id: source.id, revision: state.sourcesRevision };
              state.query = '';
              state.archived = false;
              state.notice = null;
              state.preview = null;
              reset(state, 'search', '');
              catalog(state, actions);
            } else if (key === 'edit-' + source.id) startForm(state, source);
            else if (key === 'remove-' + source.id) {
              state.notice = null;
              state.confirm = {
                kind: 'remove_source',
                revision: state.sourcesRevision,
                id: source.id,
              };
            }
          }
      } else if (state.route === 'form') {
        if (
          key.startsWith('format-') &&
          state.form &&
          ['codex', 'claude_code', 'open_code'].includes(key.slice(7))
        )
          state.form.format = key.slice(7) as Format;
        else if (key === 'source-directory') {
          state.notice = null;
          state.picker = 'source';
        } else if (key === 'save-source' && state.form?.directory) {
          state.capturedName = null;
          capture(state, actions, 'name');
        }
      } else if (state.route === 'catalog') {
        if (key === 'search') capture(state, actions, 'search');
        else if (key === 'archived') {
          state.archived = !state.archived;
          capture(state, actions, 'search');
        } else if (key === 'refresh-catalog') {
          state.notice = null;
          catalog(state, actions);
        } else if (key === 'catalog-more' && state.catalogNext)
          catalog(state, actions, state.catalogNext);
        else if (key.startsWith('entry-')) {
          const entry = state.entries[Number(key.slice(6))];
          if (entry) {
            state.selected = key;
            if ((input.event as { kind?: string }).kind === 'submit') {
              state.preview = null;
              state.notice = null;
              read(
                state,
                actions,
                { kind: 'preview' },
                {
                  kind: 'preview',
                  source: state.source,
                  entry: { sessionId: entry.id, path: entry.path },
                  digest: null,
                  after: 0,
                },
              );
            }
          }
        }
      } else if (state.route === 'preview') {
        if (key === 'destination-directory') {
          state.notice = null;
          state.picker = 'destination';
        } else if (key === 'review-copy' && state.destination) capture(state, actions, 'title');
        else if (state.preview && ['preview-more', 'preview-first'].includes(key)) {
          const selection = state.preview.selection;
          const after = key === 'preview-more' ? state.preview.next : 0;
          if (after !== null)
            read(
              state,
              actions,
              { kind: 'preview', after },
              {
                kind: 'preview',
                ...selection,
                digest: key === 'preview-first' ? null : selection.digest,
                after,
              },
            );
        }
      } else if (state.route === 'copies') {
        if (key === 'refresh-copies') copies(state, actions);
        else if (key === 'copies-more' && state.copiesNext)
          copies(state, actions, state.copiesNext);
        else {
          const session = state.copies.find((session) => key === 'copy-' + session.id);
          if (session) {
            state.selected = key;
            if ((input.event as { kind?: string }).kind === 'submit')
              events.push({ name: 'selected', value: { session } });
          }
        }
      }
    }
  }
  return frame(presentation, state, props, actions, events);
}
