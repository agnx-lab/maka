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

import type { Action, Component } from '../../../packages/plugin-sdk/src/index.js';
import { connectionFormContract, type ConnectionFormProps } from '../../shared/contracts.js';
import { withPageShell } from '../../shared/page-shell.js';
import { type Node, type Presentation } from '../../shared/view.js';
import { handleEvent } from './events.js';
import { searchState } from '../search.js';
import { request } from './operations.js';
import { names, type SettingsValidation, type State } from './state.js';
import { frame } from './view.js';

export function connectionForm<N extends Node>(
  presentation: Presentation<N>,
  validation: SettingsValidation,
): Component<State, ConnectionFormProps, N> {
  return withPageShell<State, ConnectionFormProps, N>(
    {
      mount(props) {
        const state: State = {
          revision: 0,
          serial: 0,
          opened: props.connection,
          baseline: props.connection,
          catalogRevision: props.catalogRevision,
          provider: null,
          providers: [],
          search: searchState(),
          providerActive: 0,
          providerGroup: null,
          providerFocus: null,
          providersFailed: false,
          fields: Object.fromEntries(
            names.map((name) => [
              name,
              {
                enabled: false,
                observation: null,
                command: null,
                sequence: 0,
                initialized: false,
                initialText: null,
                failed: false,
              },
            ]),
          ) as State['fields'],
          enabled: props.connection?.enabled ?? true,
          enabledEdited: false,
          defaultSlug: '',
          advanced: false,
          pending: {},
          current: null,
          notice: null,
          invalidField: null,
          conflict: false,
          reviewed: null,
          reviewedCatalog: null,
          reviewPage: 0,
          confirmation: props.removeRequested && props.connection ? 'remove' : null,
          historyOpen: false,
          earlier: [],
          historyNext: null,
          historyFailed: false,
        };
        const actions: Action[] = [];
        if (!props.connection)
          request(state, actions, { kind: 'providers', offset: 0 }, 'settings', 'providers', {
            offset: 0,
          });
        return frame(presentation, state, props, actions);
      },
      update(state, props) {
        state.revision++;
        return frame(presentation, state, props);
      },
      event(state, input, props) {
        return handleEvent(presentation, state, input, props, validation);
      },
    },
    () => 'form-back',
  );
}

export function activate() {
  return {
    ConnectionForm: {
      kind: 'component',
      contract: connectionFormContract,
      component: ['component'],
    },
  };
}
export type { SettingsValidation } from './state.js';
