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
  SettingsAssistantPreferences,
} from '../../packages/plugin-sdk/src/index.js';

/** A mounted view's native observation, never a preference resolver or writer. */
export interface LanguageObservation {
  observed: SettingsAssistantPreferences | null;
  pending: string | null;
  sequence: number;
  refreshRequested: boolean;
}
export const initialLanguageObservation = (): LanguageObservation => ({
  observed: null,
  pending: null,
  sequence: 0,
  refreshRequested: false,
});
export function readLanguage(state: LanguageObservation, actions: Action[]) {
  if (state.pending) {
    state.refreshRequested = true;
    return;
  }
  state.pending = 'language-preference-' + ++state.sequence;
  actions.push({
    key: state.pending,
    binding: 'settings',
    member: 0,
    method: 'assistantPreferences',
    input: {},
    resources: {},
  });
}
export function observeLanguage(
  state: LanguageObservation,
  input: ComponentInput,
  actions: Action[],
): boolean {
  if (input.kind !== 'action' || input.key !== state.pending) return false;
  state.pending = null;
  const outcome = input.event as ActionOutcome;
  if (outcome.status === 'succeeded')
    state.observed = outcome.value as SettingsAssistantPreferences;
  if (state.refreshRequested) {
    state.refreshRequested = false;
    readLanguage(state, actions);
  }
  return true;
}
