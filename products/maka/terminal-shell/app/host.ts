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

import { host as nativeHost } from 'maka/host';
import type { ScopedHost } from 'maka/host';
import type {
  AgentFileKind,
  ClientDraft,
  ClientDraftEdit,
  ClientOperation,
  OperationReply,
  ServiceMethods,
} from '../../../../packages/plugin-sdk/src/index.js';

// Native checks each exact method contract before querying or subscribing. The Host
// validates their returned values; callers cannot choose an unrelated result T.
type Services = {
  agent: ServiceMethods['agent'];
  settings: ServiceMethods['settings'];
  authentication: ServiceMethods['settingsAuthentication'];
  setup: ServiceMethods['settingsSetup'];
  management: ServiceMethods['deploymentManager'];
};
type Endpoint = keyof Services;
type Method<E extends Endpoint, Mode extends string> = {
  [M in keyof Services[E]]: Services[E][M] extends { mode: Mode } ? M : never;
}[keyof Services[E]] &
  string;
type Input<E extends Endpoint, M extends string> = M extends keyof Services[E]
  ? Services[E][M] extends { input: infer I }
    ? I
    : never
  : never;
type Output<E extends Endpoint, M extends string> = M extends keyof Services[E]
  ? Services[E][M] extends { output: infer O }
    ? O
    : never
  : never;
type Subscription<T> = { next(): Promise<T | null>; close(): Promise<void> };
type FileSubscription = { readFile(): Promise<FileView>; close(): Promise<void> };
type StreamMethod<E extends Endpoint> = E extends 'agent'
  ? Exclude<Method<E, 'subscription'>, 'readFile'>
  : Method<E, 'subscription'>;

// This is the native collector's presentation, not agent.readFile's wire stream.
export type FileView =
  | {
      kind: 'text';
      rows: { offset: number; line: number; text: string }[];
      version: string;
      digest: string;
      bytes: number;
    }
  | {
      kind: 'image';
      uri: string;
      mimeType: string;
      version: string;
      digest: string;
      bytes: number;
    }
  | { kind: 'absent' }
  | { kind: 'changed' }
  | { kind: 'binary'; bytes: number }
  | { kind: 'too_large'; bytes: number; limit: number }
  | { kind: 'unsupported'; fileKind: AgentFileKind };

const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);
const count = (value: unknown): value is number =>
  typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
function fileView(value: unknown): FileView {
  if (!object(value)) throw new Error('Invalid native file presentation.');
  const captured = () =>
    typeof value.version === 'string' && typeof value.digest === 'string' && count(value.bytes);
  const valid =
    value.kind === 'absent' ||
    value.kind === 'changed' ||
    (value.kind === 'binary' && count(value.bytes)) ||
    (value.kind === 'too_large' && count(value.bytes) && count(value.limit)) ||
    (value.kind === 'unsupported' &&
      typeof value.fileKind === 'string' &&
      ['file', 'directory', 'symlink', 'other'].includes(value.fileKind)) ||
    (value.kind === 'image' &&
      captured() &&
      typeof value.uri === 'string' &&
      typeof value.mimeType === 'string') ||
    (value.kind === 'text' &&
      captured() &&
      Array.isArray(value.rows) &&
      value.rows.every(
        (row) =>
          object(row) && count(row.offset) && count(row.line) && typeof row.text === 'string',
      ));
  if (!valid) throw new Error('Invalid native file presentation.');
  return value as FileView;
}

type Attempt = { operationId: string; reply: unknown };
export type OriginalMutationEvent =
  | { kind: 'progress'; value: unknown }
  | { kind: 'not_dispatched' }
  | { kind: 'finished'; reply: OperationReply; cleanupError: string | null };
export type OriginalMutation = {
  readonly operationId: string;
  next(): Promise<OriginalMutationEvent | null>;
  cancel(): void;
};
function attempt(value: unknown): Attempt {
  if (!object(value) || typeof value.operationId !== 'string' || !('reply' in value))
    throw new Error('Invalid native operation observation.');
  return { operationId: value.operationId, reply: value.reply };
}

async function query<E extends Endpoint, M extends Method<E, 'query'>>(
  endpoint: E,
  method: M,
  input: Input<E, M>,
): Promise<Output<E, M>> {
  return nativeHost.query(endpoint, method, input) as Promise<Output<E, M>>;
}
function subscribe(
  endpoint: 'agent',
  method: 'readFile',
  input: Input<'agent', 'readFile'>,
): Promise<FileSubscription>;
function subscribe<E extends Endpoint, M extends StreamMethod<E>>(
  endpoint: E,
  method: M,
  input: Input<E, M>,
): Promise<Subscription<Output<E, M>>>;
async function subscribe(
  endpoint: Endpoint,
  method: string,
  input: unknown,
): Promise<FileSubscription | Subscription<unknown>> {
  const opened = await nativeHost.subscribe(endpoint, method, input);
  if (endpoint === 'agent' && method === 'readFile') {
    return {
      readFile: async () => fileView(await opened.readFile()),
      close: () => opened.close(),
    };
  }
  // Keep the original native subscription and its cleanup, without a handle table.
  return opened;
}

// Local journal methods serialize these actual Rust DTOs. Their receipt business
// values remain unknown and continue through the application's existing guards.
export const host = {
  query,
  subscribe,
  get initialSession() {
    return nativeHost.initialSession;
  },
  session(sessionId: string) {
    return nativeHost.session(sessionId);
  },
  pathLabel(path: unknown) {
    return nativeHost.pathLabel(path);
  },
  async control(method: string, input: unknown) {
    return attempt(await nativeHost.control(method, input));
  },
  async settingsMutation(endpoint: string, method: string, input: unknown, secret?: string | null) {
    return attempt(await nativeHost.settingsMutation(endpoint, method, input, secret));
  },
  beginSetting<E extends Exclude<Endpoint, 'agent'>, M extends Method<E, 'mutation'>>(
    endpoint: E,
    method: M,
    input: Input<E, M>,
    secret?: string | null,
  ): Promise<OriginalMutation> {
    return nativeHost.beginSetting(endpoint, method, input, secret) as Promise<OriginalMutation>;
  },
  pendingSettings: () => nativeHost.pendingSettings() as Promise<ClientOperation[]>,
  recoverSetting: (id: string) => nativeHost.recoverSetting(id) as Promise<OperationReply>,
  abandonSetting: (id: string) => nativeHost.abandonSetting(id) as Promise<ClientOperation>,
  pendingControls: () => nativeHost.pendingControls() as Promise<ClientOperation[]>,
  recoverControl: (id: string) => nativeHost.recoverControl(id) as Promise<OperationReply>,
  abandonControl: (id: string) => nativeHost.abandonControl(id) as Promise<ClientOperation>,
};

// Session methods take the original native Instance. In particular, the same
// instance still goes directly into AttachmentPicker.client.
export type { ScopedHost };
// The schema also accepts old inputs without attachments. These native methods
// serialize Draft without skipping that field, including [] for an empty draft.
type SavedDraft = ClientDraft & Required<Pick<ClientDraft, 'attachments'>>;
export const loadDraft = (scope: ScopedHost) => scope.loadDraft() as Promise<SavedDraft>;
export const saveDraft = (scope: ScopedHost, revision: number, edit: ClientDraftEdit) =>
  scope.saveDraft(revision, edit) as Promise<SavedDraft>;
export async function sendDraft(scope: ScopedHost, revision: number, selection: unknown) {
  return attempt(await scope.send(revision, selection));
}
export const pendingSends = (scope: ScopedHost) => scope.pending() as Promise<ClientOperation[]>;
export const recoverSend = (scope: ScopedHost, id: string) =>
  scope.recover(id) as Promise<OperationReply>;
export const abandonSend = (scope: ScopedHost, id: string) =>
  scope.abandonUnprepared(id) as Promise<ClientOperation>;
export const findUpload = (scope: ScopedHost, id: string) =>
  scope.findUpload(id) as Promise<ClientOperation | null>;
export const recoverUpload = (scope: ScopedHost, id: string) =>
  scope.recoverUpload(id) as Promise<ClientOperation>;
export const abandonUpload = (scope: ScopedHost, id: string) =>
  scope.abandonUpload(id) as Promise<ClientOperation>;
export const watchUploads = (scope: ScopedHost) =>
  scope.watchUploads() as { next(): Promise<ClientOperation[] | null>; close(): void };
