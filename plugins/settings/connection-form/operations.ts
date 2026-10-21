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
  CatalogMutationResult,
  ConnectionCatalogEntryUpdate,
  Emission,
  OperationReply,
} from '../../../packages/plugin-sdk/src/index.js';
import { type ConnectionFormProps } from '../../shared/contracts.js';
import { resetScroll } from '../../shared/scroll.js';
import { sameValue } from '../../shared/value.js';
import {
  names,
  type FieldName,
  type Request,
  type SettingsValidation,
  type State,
} from './state.js';

export function request(
  state: State,
  actions: Action[],
  r: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  const key = 'connection-form-' + ++state.serial;
  if (r.kind === 'providers' || r.kind === 'provider') state.providersFailed = false;
  state.pending[key] = r;
  actions.push({ key, binding, member: 0, method, input, resources });
}

export function active(state: State) {
  return names.filter((name) => state.fields[name].enabled);
}

export function busy(state: State) {
  return (
    !!state.current ||
    Object.values(state.pending).some((r) => ['review', 'reviewCatalog'].includes(r.kind))
  );
}

export function fresh(state: State, props: ConnectionFormProps) {
  return sameValue(state.opened, props.connection);
}

export function sameConnection(state: State, props: ConnectionFormProps) {
  return state.opened?.target.connectionId === props.connection?.target.connectionId;
}

export function configuration(state: State): Record<string, unknown> {
  return (
    state.baseline?.target.configuration ??
    (isObject(state.provider?.descriptor.configurationDefaults)
      ? (state.provider!.descriptor.configurationDefaults as Record<string, unknown>)
      : {})
  );
}

export function initial(state: State, name: FieldName) {
  switch (name) {
    case 'name':
      return state.baseline?.name ?? state.provider?.descriptor.label ?? '';
    case 'slug':
      return state.defaultSlug;
    case 'endpoint':
      return typeof configuration(state).baseUrl === 'string'
        ? (configuration(state).baseUrl as string)
        : '';
    case 'configuration':
      return JSON.stringify(configuration(state));
    case 'models':
      return '';
    case 'overrides':
    case 'overlay':
      return 'null';
  }
}

export function prefill(state: State, name: FieldName, empty = false) {
  const field = state.fields[name];
  if (!field.observation || field.command || busy(state)) return;
  field.initialized = false;
  field.failed = false;
  field.command = {
    owner: field.observation.snapshot.text.owner,
    sequence: ++field.sequence,
    basedOn: field.observation.revision,
    edit: { kind: 'reset', text: empty ? '' : initial(state, name) },
  };
}

export function canSave(state: State, props: ConnectionFormProps) {
  return (
    fresh(state, props) &&
    !busy(state) &&
    !state.conflict &&
    !state.confirmation &&
    !state.historyOpen &&
    !!(state.baseline || state.provider) &&
    active(state).every((name) => {
      const f = state.fields[name];
      return (
        f.initialized &&
        !f.failed &&
        !f.command &&
        !!f.observation &&
        !f.observation.snapshot.marked
      );
    })
  );
}

export function canRemove(state: State, props: ConnectionFormProps) {
  return !!state.baseline && fresh(state, props) && !busy(state) && !state.conflict;
}

export function remove(state: State, props: ConnectionFormProps, actions: Action[]) {
  if (state.confirmation !== 'remove' || !canRemove(state, props)) return;
  const target = state.baseline!.target;
  state.confirmation = null;
  state.current = {
    kind: 'remove',
    attempt: ++state.serial,
    values: {},
    remaining: [],
    reference: null,
  };
  state.notice = null;
  request(state, actions, { kind: 'mutate' }, 'settings', 'remove', {
    expected: { connectionId: target.connectionId, revision: target.revision },
  });
}

export function save(
  state: State,
  props: ConnectionFormProps,
  actions: Action[],
  validation: SettingsValidation,
) {
  if (!canSave(state, props)) return;
  const fields = active(state);
  const attempt = ++state.serial;
  state.current = { kind: 'save', attempt, values: {}, remaining: fields, reference: null };
  state.notice = null;
  state.invalidField = null;
  for (const name of fields) {
    const captured = state.fields[name].observation!;
    request(
      state,
      actions,
      { kind: 'read', field: name, captured, attempt },
      'reader',
      'read',
      { start: 0, end: captured.snapshot.text.bytes },
      {
        text: {
          child: 'field-' + name,
          method: 'snapshot',
          input: {
            kind: 'at_revision',
            owner: captured.snapshot.text.owner,
            revision: captured.revision,
          },
        },
      },
    );
  }
  if (!fields.length) mutate(state, actions, validation);
}

export function isObject(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}

export function utf8Bytes(value: string) {
  let bytes = 0;
  for (const character of value) {
    const code = character.codePointAt(0)!;
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
  }
  return bytes;
}

export function json(text: string, nullable = false) {
  const value: unknown = JSON.parse(text);
  const pending: unknown[] = [value];
  while (pending.length) {
    const item = pending.pop();
    if (typeof item === 'number' && !Number.isFinite(item))
      throw new Error('finite number required');
    if (item && typeof item === 'object')
      for (const child of Object.values(item)) pending.push(child);
  }
  if ((!nullable || value !== null) && !isObject(value)) throw new Error('object required');
  return value as Record<string, unknown> | null;
}

export function mutate(state: State, actions: Action[], validation: SettingsValidation) {
  const current = state.current!;
  const values = current.values;
  try {
    const changes: ConnectionCatalogEntryUpdate = {};
    if (values.name !== undefined) {
      if (!values.name.length) throw new Error('name');
      if (!state.baseline || values.name !== state.baseline.name) changes.name = values.name;
    }
    let cfg: Record<string, unknown> | undefined;
    if (values.configuration !== undefined) cfg = json(values.configuration)!;
    else if (
      values.endpoint !== undefined &&
      (state.baseline || values.endpoint !== initial(state, 'endpoint'))
    ) {
      cfg = { ...configuration(state), baseUrl: values.endpoint };
    }
    if (cfg && (!state.baseline || !sameValue(cfg, state.baseline.target.configuration)))
      changes.configuration = cfg;
    if (!state.baseline || state.enabled !== state.baseline.enabled)
      changes.enabled = state.enabled;
    if (values.models !== undefined) {
      const ids = values.models
        .split(/\r?\n/u)
        .map((id) => id.trim())
        .filter(Boolean);
      if (new Set(ids).size !== ids.length) throw new Error('models');
      changes.enabledModelIds = ids;
    }
    if (values.overrides !== undefined)
      changes.modelOverrides = json(
        values.overrides,
        true,
      ) as ConnectionCatalogEntryUpdate['modelOverrides'];
    if (values.overlay !== undefined) changes.requestBodyOverlay = json(values.overlay, true);
    if (!state.baseline) {
      if (!values.slug || !/^[a-z0-9]+(?:[-_][a-z0-9]+)*$/.test(values.slug))
        throw new Error('slug');
      const input = {
        expectedCatalogRevision: state.catalogRevision,
        connection: {
          slug: values.slug,
          name: changes.name,
          provider: state.provider!.identity,
          configuration: changes.configuration ?? configuration(state),
          enabled: state.enabled,
          enabledModelIds: changes.enabledModelIds ?? [],
          ...(changes.modelOverrides ? { modelOverrides: changes.modelOverrides } : {}),
          ...(changes.requestBodyOverlay ? { requestBodyOverlay: changes.requestBodyOverlay } : {}),
        },
      };
      if (!validation.create(input)) throw new Error('invalid create input');
      request(state, actions, { kind: 'mutate' }, 'settings', 'create', input);
    } else if (!Object.keys(changes).length) {
      state.current = null;
      state.notice = 'noConnectionChanges';
    } else {
      const input = {
        expected: {
          connectionId: state.baseline.target.connectionId,
          revision: state.baseline.target.revision,
        },
        changes,
      };
      if (!validation.update(input)) throw new Error('invalid update input');
      request(state, actions, { kind: 'mutate' }, 'settings', 'update', input);
    }
    // Read results are immutable attempt data, never the writable editor state.
    current.values = {};
  } catch (error) {
    state.current = null;
    state.notice = 'invalidConnectionFields';
    const field = error instanceof Error ? (error.message as FieldName) : null;
    state.invalidField = field && names.includes(field) ? field : null;
    if (state.invalidField && !['name', 'endpoint'].includes(state.invalidField))
      state.advanced = true;
  }
}

export function settle(state: State, reply: OperationReply, events: Emission[]) {
  const current = state.current;
  if (!current || (current.reference && !sameValue(current.reference, reply.reference))) return;
  current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'connectionUnknown';
    return;
  }
  const value = reply.receipt.outcome.value as CatalogMutationResult;
  if (value.kind === 'committed') {
    if (
      current.kind === 'remove'
        ? value.connection !== undefined
        : !value.connection ||
          (state.baseline && value.connection.connectionId !== state.baseline.target.connectionId)
    ) {
      state.notice = 'connectionUnknown';
      return;
    }
    state.current = null;
    events.push(
      {
        name: 'changed',
        value: !state.baseline && value.connection ? { created: value.connection } : {},
      },
      { name: 'closed', value: {} },
    );
  } else {
    state.current = null;
    state.conflict = ['connection_stale', 'revision_conflict'].includes(value.kind);
    state.notice = state.conflict ? 'connectionConflict' : 'connectionRejected';
  }
}

export function history(state: State, actions: Action[], after: number) {
  if (Object.values(state.pending).some((r) => ['history', 'earlier'].includes(r.kind))) return;
  if (!after) resetScroll(state, 'connection-history-reading');
  state.historyFailed = false;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 4 });
}
