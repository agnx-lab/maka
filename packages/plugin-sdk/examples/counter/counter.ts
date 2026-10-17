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
  Component,
  UiExport,
  Contract,
  Output,
  Surface,
  TerminalNode,
} from '../../src/index.js';

export const contract = {
  name: 'example.App',
  version: 1,
  transport: 'local',
  interface: {
    kind: 'component',
    props: { type: 'object' },
    events: {},
    captures: {},
  },
} satisfies Contract;

export function activate(): Record<string, UiExport> {
  return { App: { contract, kind: 'component', component: ['app'] } };
}

export interface CounterState {
  count: number;
  revision: number;
}
export function createApp<N extends TerminalNode>(
  surface: Surface,
  scene: (state: CounterState) => N,
): Component<CounterState, unknown, N> {
  function frame(state: CounterState): Output<CounterState, N> {
    return {
      actions: [],
      events: [],
      frame: {
        revision: state.revision,
        state,
        surface,
        view: scene(state),
      },
    };
  }
  return {
    mount(props) {
      const count =
        typeof props === 'object' && props !== null && 'count' in props ? props.count : 0;
      if (typeof count !== 'number' || !Number.isSafeInteger(count))
        throw Error('Invalid initial count');
      return frame({ count, revision: 0 });
    },
    update(state) {
      return frame({ ...state, revision: state.revision + 1 });
    },
    event(state, input) {
      if (input.kind !== 'raw' || input.key !== 'inc') throw Error('Unexpected counter input');
      return frame({ count: state.count + 1, revision: state.revision + 1 });
    },
  };
}
