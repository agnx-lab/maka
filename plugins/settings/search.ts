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
  ComponentInput,
  EditorObservation,
  EditorTextRef,
  TextChunk,
  SettingsProviderSummary,
} from '../../packages/plugin-sdk/src/index.js';
import { sameValue } from '../shared/value.js';

export interface Search {
  query: string;
  observation: EditorObservation | null;
  pending: { key: string; basis: EditorTextRef } | null;
  serial: number;
  error: 'searchUnavailable' | null;
}
export function searchState(): Search {
  return { query: '', observation: null, pending: null, serial: 0, error: null };
}
export function searchEvent(
  state: Search,
  input: ComponentInput,
  child: string,
  actions: Action[],
): boolean {
  if (input.kind === 'component' && input.child === child && input.name === 'changed') {
    const previous = state.observation;
    state.observation = input.event as EditorObservation;
    if (
      sameValue(previous?.snapshot.text, state.observation.snapshot.text) &&
      sameValue(previous?.snapshot.marked, state.observation.snapshot.marked)
    )
      return false;
    state.error = null;
    if (!state.observation.snapshot.text.bytes) state.query = '';
  } else if (input.kind === 'action' && input.key === state.pending?.key) {
    const pending = state.pending;
    state.pending = null;
    if (sameValue(pending.basis, state.observation?.snapshot.text)) {
      const result = input.event as ActionOutcome;
      const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
      if (!chunk || !sameValue(chunk.metadata, pending.basis)) state.error = 'searchUnavailable';
      else state.query = chunk.text.trim().toLocaleLowerCase();
      return true;
    }
  } else return false;
  const observation = state.observation;
  if (!state.pending && observation && !observation.snapshot.marked) {
    const basis = observation.snapshot.text;
    if (basis.bytes) {
      const key = child + '-' + ++state.serial;
      state.pending = { key, basis };
      actions.push({
        key,
        binding: 'reader',
        member: 0,
        method: 'read',
        input: { start: 0, end: basis.bytes },
        resources: {
          text: {
            child,
            method: 'snapshot',
            input: { kind: 'at_revision', owner: basis.owner, revision: basis.revision },
          },
        },
      });
    }
  }
  return true;
}
export function matches(search: Search, ...values: string[]) {
  return search.query
    .split(/\s+/u)
    .every((part) => values.join(' ').toLocaleLowerCase().includes(part));
}

export function matchesProvider(search: Search, provider: SettingsProviderSummary) {
  return matches(
    search,
    provider.label,
    provider.identity.name,
    ...provider.authentication.map((method) =>
      method.interactive ? 'oauth ' + method.label : 'api key',
    ),
  );
}

export function providerGroup(provider: SettingsProviderSummary): { key: string; label: string } {
  const name = provider.identity.name.toLowerCase();
  if (['openai-compatible', 'openai-responses-compatible', 'anthropic-compatible'].includes(name))
    return { key: 'custom', label: 'provider-custom' };
  const families: [string, string][] = [
    ['alibaba', 'Alibaba'],
    ['minimax', 'MiniMax'],
    ['tencent', 'Tencent'],
    ['volcengine', 'Volcengine'],
    ['xiaomi', 'Xiaomi'],
    ['stepfun', 'StepFun'],
    ['zai', 'Z.AI'],
    ['moonshot', 'Moonshot / Kimi'],
    ['kimi', 'Moonshot / Kimi'],
    ['ollama', 'Ollama'],
    ['opencode', 'OpenCode'],
  ];
  const family = families.find(([prefix]) => name === prefix || name.startsWith(prefix + '-'));
  return family
    ? { key: family[1], label: family[1] }
    : { key: provider.label, label: provider.label };
}
