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
  AgentImportChange,
  AgentImportLocation,
  AgentImportResult,
  AgentImportSource,
  Emission,
  OperationReply,
} from '../../../packages/plugin-sdk/src/index.js';
import { type ImportsProps } from '../../shared/contracts.js';
import { type Copy } from './locale.js';
import { resetScroll } from '../../shared/scroll.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';
import { formatName, type Field, type FieldName, type Request, type State } from './state.js';

export function initialize(field: Field) {
  if (field.reset !== null && field.observation && !field.command)
    field.command = {
      owner: field.observation.snapshot.text.owner,
      basedOn: field.observation.revision,
      sequence: ++field.sequence,
      edit: { kind: 'reset', text: field.reset },
    };
}

export function reset(state: State, name: FieldName, value: string) {
  const field = state.fields[name];
  field.reset = value;
  field.ready = false;
  initialize(field);
}

export function request(
  state: State,
  actions: Action[],
  r: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  if (state.pending) return;
  const key = `imports-${++state.serial}`;
  state.pending = { key, request: r };
  actions.push({ key, binding, member: 0, method, input, resources });
}

export function read(
  state: State,
  actions: Action[],
  r: Extract<
    Request,
    { kind: 'sources' | 'catalog' | 'preview' | 'copies' | 'mutate' | 'receipt' }
  >,
  input: unknown,
) {
  request(state, actions, r, 'agent', 'imports', input);
}

export function sources(state: State, actions: Action[]) {
  read(state, actions, { kind: 'sources' }, { kind: 'sources' });
}

export function catalog(state: State, actions: Action[], cursor: string | null = null) {
  if (!cursor) resetScroll(state, 'import-catalog');
  if (state.source)
    read(
      state,
      actions,
      { kind: 'catalog', after: cursor },
      {
        kind: 'catalog',
        source: state.source,
        query: { text: state.query, includeArchived: state.archived, cursor },
      },
    );
}

export function copies(state: State, actions: Action[], after: string | null = null) {
  if (!after) resetScroll(state, 'import-copies');
  read(state, actions, { kind: 'copies', after }, { kind: 'copies', after });
}

export function history(state: State, actions: Action[], after = 0) {
  state.restoring = true;
  state.historyFailed = false;
  if (after === 0) state.original = null;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 16 });
}

export function locked(state: State, props: ImportsProps) {
  return !props.open || props.busy || !!state.pending || !!state.current || state.restoring;
}

export function capture(state: State, actions: Action[], name: FieldName) {
  const field = state.fields[name],
    observation = field.observation;
  if (!observation || observation.snapshot.marked || field.command || !field.ready) return;
  if (observation.snapshot.text.bytes > 4096) {
    state.notice = 'importsInvalid';
    return;
  }
  request(
    state,
    actions,
    { kind: 'field', field: name, observation },
    'reader',
    'read',
    { start: 0, end: observation.snapshot.text.bytes },
    {
      text: {
        child: `import-${name}`,
        method: 'snapshot',
        input: {
          kind: 'at_revision',
          owner: observation.snapshot.text.owner,
          revision: observation.snapshot.text.revision,
        },
      },
    },
  );
}

export function mutate(state: State, actions: Action[], input: AgentImportChange) {
  state.current = { input, reference: null };
  state.notice = null;
  request(state, actions, { kind: 'mutate' }, 'agent', 'import', input);
  actions.at(-1)!.recovery = { kind: 'conversation-import/v1', input };
}

export function notice(kind: string): keyof Copy {
  return kind === 'changed'
    ? 'importsChanged'
    : kind === 'invalid'
      ? 'importsInvalid'
      : kind === 'limit'
        ? 'importsLimit'
        : 'importsUnavailable';
}

export function settle(state: State, reply: OperationReply, actions: Action[], events: Emission[]) {
  if (
    !state.current ||
    (state.current.reference && !sameValue(reply.reference, state.current.reference))
  )
    return;
  state.current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'importsUnknown';
    return;
  }
  const result = reply.receipt.outcome.value as AgentImportResult;
  const input = state.current.input;
  state.current = null;
  state.confirm = null;
  if (result.kind === 'sources') {
    state.sources = result.sources;
    state.sourcesRevision = result.revision;
    state.form = null;
    state.route = 'sources';
    state.notice = input.kind === 'remove_source' ? 'importsRemoved' : 'importsSaved';
    state.selected = state.sources[0] ? 'source-' + state.sources[0].id : null;
    sources(state, actions);
  } else if (result.kind === 'imported') {
    state.latest = result.session;
    state.notice = 'importsDone';
    state.route = 'copies';
    events.push({ name: 'changed', value: {} });
    copies(state, actions);
  } else state.notice = notice(result.kind);
}

export function startForm(state: State, source: AgentImportSource | null) {
  state.form = {
    id: source?.id ?? null,
    format: source?.location.kind ?? 'codex',
    directory: source
      ? { name: source.name, path: source.path, selection: source.location.directory }
      : null,
  };
  state.route = 'form';
  state.notice = null;
  reset(state, 'name', source?.name ?? '');
  reset(
    state,
    'database',
    source?.location.kind === 'open_code' ? source.location.database : 'opencode.db',
  );
}

export function saveSource(state: State, actions: Action[], database: string | null) {
  if (!state.form?.directory || state.capturedName === null) return;
  const { format, directory, id } = state.form;
  const name = state.capturedName.trim() || formatName(format);
  if ([...name].length > 128 || /[\u0000-\u001f\u007f-\u009f]/u.test(name)) {
    state.notice = 'importsInvalid';
    return;
  }
  const location: AgentImportLocation =
    format === 'open_code'
      ? { kind: format, directory: directory.selection, database: database?.trim() ?? '' }
      : { kind: format, directory: directory.selection };
  mutate(state, actions, {
    kind: 'save_source',
    revision: state.sourcesRevision,
    id,
    name,
    location,
  });
}

export function focus<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  id: string | null,
) {
  state.selected = id;
  if (id)
    state.focus = {
      sequence: (state.focus?.sequence ?? 0) + 1,
      target: { kind: 'focus_item', key: id },
    };
}
