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

import { cleanup, signal } from 'nsh';
import { host } from './host';
import type {
  ClientOperation,
  CredentialStatus,
  CredentialVersionBasis,
} from '../../../../packages/plugin-sdk/src/generated/runtime';

const record = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);
export function settingsValue(reply: unknown): unknown {
  const receipt = record(reply) && record(reply.receipt) ? reply.receipt : null;
  const outcome = receipt?.kind === 'recorded' && record(receipt.outcome) ? receipt.outcome : null;
  if (outcome?.kind !== 'returned')
    throw new Error(
      'The result is unknown. Recover the original action before making another change.',
    );
  return outcome.value;
}

export const credentialBasis = (status: CredentialStatus | null): CredentialVersionBasis | null =>
  status?.configured && status.credentialId !== null && status.revision !== null
    ? { locator: status.locator, credentialId: status.credentialId, revision: status.revision }
    : null;
export function settingsRejection(value: any): Error {
  if (
    value?.kind === 'connection_stale' ||
    value?.kind === 'revision_conflict' ||
    value?.kind === 'superseded' ||
    value?.kind === 'credential_stale' ||
    value?.reason === 'credential_changed' ||
    value?.reason === 'connection_changed'
  )
    return new Error('These settings changed elsewhere. Reload before saving again.');
  if (value?.kind === 'connection_exists')
    return new Error('That connection ID is already in use. Choose another ID.');
  if (value?.kind === 'invalid_default_target')
    return new Error('Enable the connection and model before making it the default.');
  return new Error(
    `The Host rejected this action (${value?.reason ?? value?.kind ?? 'unknown result'}).`,
  );
}
// One presentation owner for the existing native journal. The Client's
// atomic reservation remains the write fence across pages and processes.
export function createSettingsOperations() {
  const busy = signal(false);
  const error = signal('');
  const notice = signal('');
  const pending = signal<ClientOperation[]>([]);
  const pendingKnown = signal(false);
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const report = (reason: unknown) => {
    if (alive) error.set(reason instanceof Error ? reason.message : String(reason));
  };
  const refreshPending = async () => {
    try {
      const rows = await host.pendingSettings();
      if (alive) {
        pending.set(rows);
        pendingKnown.set(true);
      }
    } catch (reason) {
      if (alive) pendingKnown.set(false);
      throw reason;
    }
  };
  const mutate = async (
    endpoint: 'settings' | 'setup' | 'authentication' | 'management',
    method: string,
    input: unknown,
    secret: string | null = null,
  ): Promise<any> => {
    return settingsValue((await host.settingsMutation(endpoint, method, input, secret)).reply);
  };
  const run = async (operation: () => Promise<void>) => {
    if (!alive || busy() || !pendingKnown() || pending().length) return;
    busy.set(true);
    error.set('');
    notice.set('');
    try {
      await operation();
    } catch (reason) {
      report(reason);
    } finally {
      await refreshPending().catch(report);
      if (alive) busy.set(false);
    }
  };
  const recover = (afterKnown: (original: ClientOperation, value: unknown) => void) => {
    if (!alive || busy()) return;
    busy.set(true);
    error.set('');
    void (async () => {
      try {
        for (const original of await host.pendingSettings()) {
          if (!original.reference) continue;
          const value = settingsValue(await host.recoverSetting(original.id));
          if (!alive) return;
          if (original.endpoint === 'management') {
            if (!record(value) || value.status !== 'committed') {
              if (record(value) && value.status === 'conflict')
                throw new Error('Host services changed elsewhere. Reload before saving again.');
              throw new Error('The Host rejected this service change. Reload and review it.');
            }
          } else {
            const outcome = record(value) && value.kind === 'applied' ? value.result : value;
            if (
              !record(outcome) ||
              !['committed', 'saved', 'ready'].some((kind) => kind === outcome.kind)
            )
              throw settingsRejection(outcome);
          }
          afterKnown(original, value);
        }
        if (alive) notice.set('Original settings action receipts checked.');
      } catch (reason) {
        report(reason);
      } finally {
        await refreshPending().catch(report);
        if (alive) busy.set(false);
      }
    })();
  };
  const abandon = (id: string) => {
    if (!alive || busy()) return;
    busy.set(true);
    error.set('');
    void (async () => {
      try {
        await host.abandonSetting(id);
      } catch (reason) {
        report(reason);
      } finally {
        await refreshPending().catch(report);
        if (alive) busy.set(false);
      }
    })();
  };
  void refreshPending().catch(report);
  return { busy, error, notice, pending, pendingKnown, report, mutate, run, recover, abandon };
}
export type SettingsOperations = ReturnType<typeof createSettingsOperations>;
