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

/** Application authoring types. Runtime admission remains in Rust. */
export type * from './generated/runtime.js';
export { contracts } from './contracts.js';
export { configurations } from './configurations.js';
export { ui } from './ui.js';
export type {
  TerminalNode,
  TerminalKind,
  ComponentDeclaration,
  ReadingDeclaration,
  TerminalProps,
} from './ui.js';

import type {
  ComponentInput,
  ComponentOutput,
  CapturePlan,
  ValueBindings,
} from './generated/runtime.js';
import type {
  HostRequest,
  HostObservation,
  OperationReply,
  ResourceMetadata,
} from './generated/runtime.js';

/** State is private to this component; external props and events still need their contract. */
export type Output<State = unknown, View = unknown> = Omit<ComponentOutput, 'frame'> & {
  frame: Omit<ComponentOutput['frame'], 'state' | 'view'> & { state: State; view: View };
};

/** Detached snapshots of this activation's captured immutable value imports. */
export type ComponentValues = { readonly [K in keyof ValueBindings]: Readonly<ValueBindings[K]> };

/** Synchronous declarative callbacks; no promises or native ownership in a frame. */
export interface Component<State = unknown, Props = unknown, View = unknown> {
  mount(props: Props, values: ComponentValues): Output<State, View>;
  update(state: State, props: Props, values: ComponentValues): Output<State, View>;
  event(
    state: State,
    input: ComponentInput,
    props: Props,
    values: ComponentValues,
  ): Output<State, View>;
  capture?(
    state: State,
    props: Props,
    method: string,
    input: unknown,
    values: ComponentValues,
  ): CapturePlan;
}

/** Provided by the actual invocation. Retaining it does not retain request resources. */
export interface RequestContext {
  invoke(request: Extract<HostRequest, { operation: 'emit' }>): Promise<null>;
  invoke(request: Extract<HostRequest, { operation: 'mutate' }>): Promise<OperationReply>;
  invoke(request: Extract<HostRequest, { operation: 'recover' }>): Promise<OperationReply>;
  invoke(request: Extract<HostRequest, { operation: 'metadata' }>): Promise<ResourceMetadata>;
  invoke(request: Extract<HostRequest, { operation: 'read' }>): Promise<number[]>;
  invoke(request: HostRequest): Promise<unknown>;
}

export type HostService = Record<
  string,
  (input: unknown, observation: HostObservation, context: RequestContext) => unknown
>;

export { nativeImplementations } from './native.js';
