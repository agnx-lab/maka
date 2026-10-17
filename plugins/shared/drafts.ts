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
  AgentAttachment,
  AgentDataSource,
  AgentSession,
  CallerIntent,
  EditorCheckpointObservation,
  EditorRestoration,
  OperationReply,
} from '../../packages/plugin-sdk/src/index.js';
import type { AttachmentDrafts } from './attachments.js';
import { sameValue } from './value.js';

export interface ComposerReadiness {
  context: string;
  dataSource: AgentDataSource;
  ready: boolean;
}
export interface DraftCheckpoint {
  context: string;
  dataSource: AgentDataSource;
  observation: EditorCheckpointObservation;
  owners: string[];
}
type DraftStatus = 'checking' | 'unknown' | 'failed' | 'applying';
export interface DraftRecord {
  context: string;
  dataSource: AgentDataSource;
  observation: EditorCheckpointObservation;
  work: {
    after: number;
    scope: string | null;
    checkpoint?: { after: number; scope: string };
    done: boolean;
    queue: CallerIntent[];
    clearText: boolean;
    attachments: AgentAttachment[];
    valid: boolean;
    status: DraftStatus;
    restore: EditorRestoration | null;
  } | null;
}
export type DraftStep =
  | { kind: 'draftPage'; owner: string; after: number }
  | { kind: 'draftReceipt'; owner: string; original: CallerIntent };
function object(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === 'object' && !Array.isArray(value);
}
function text(value: unknown, limit: number): value is string {
  return typeof value === 'string' && value.length > 0 && [...value].length <= limit;
}
function attachments(value: unknown, session: string): AgentAttachment[] | null {
  if (
    !object(value) ||
    Object.keys(value).some(
      (key) => !['attachments', 'historyAfter', 'historyScope'].includes(key),
    ) ||
    (value.historyScope !== null &&
      (typeof value.historyScope !== 'string' || value.historyScope.length > 4096)) ||
    !Number.isSafeInteger(value.historyAfter) ||
    (value.historyAfter as number) < 0 ||
    !Array.isArray(value.attachments)
  )
    return null;
  const files: AgentAttachment[] = [];
  for (const file of value.attachments) {
    if (
      !object(file) ||
      Object.keys(file).some(
        (key) => !['sessionId', 'id', 'name', 'mimeType', 'bytes'].includes(key),
      ) ||
      file.sessionId !== session ||
      !text(file.id, 128) ||
      !text(file.name, 512) ||
      !text(file.mimeType, 256) ||
      !Number.isSafeInteger(file.bytes) ||
      (file.bytes as number) < 0 ||
      (file.bytes as number) > 52428800 ||
      files.some((previous) => previous.id === file.id)
    )
      return null;
    files.push(file as unknown as AgentAttachment);
  }
  return files;
}
function correlation(original: CallerIntent, draft: DraftRecord) {
  const reference = original.reference;
  if (
    (reference.method !== 'send' && reference.method !== 'execute') ||
    reference.source.stateDomain !== draft.dataSource.stateDomain ||
    reference.source.deployment !== draft.dataSource.deployment ||
    !sameValue(reference.namespace, draft.dataSource.namespace)
  )
    return null;
  const value = original.recovery;
  if (
    !object(value) ||
    value.format !== 'maka.agent.draft-send@2' ||
    value.sessionId !== draft.context
  )
    return null;
  if (
    !object(value.editor) ||
    !text(value.editor.owner, 128) ||
    !Number.isSafeInteger(value.editor.revision) ||
    (value.editor.revision as number) < 0 ||
    !Array.isArray(value.attachments) ||
    !value.attachments.every((id) => text(id, 128)) ||
    new Set(value.attachments).size !== value.attachments.length
  )
    throw new Error('Invalid draft send correlation');
  return {
    owner: value.editor.owner,
    revision: value.editor.revision as number,
    attachments: value.attachments as string[],
  };
}
export function observeDraft(records: DraftRecord[], event: DraftCheckpoint): DraftRecord[] {
  const retained = records.filter((record) => event.owners.includes(record.observation.owner));
  let record = retained.find((record) => record.observation.owner === event.observation.owner);
  if (
    record &&
    (record.context !== event.context || !sameValue(record.dataSource, event.dataSource))
  )
    return retained;
  if (!record) {
    if (retained.length >= 32) return retained;
    record = {
      context: event.context,
      dataSource: event.dataSource,
      observation: event.observation,
      work: null,
    };
    retained.push(record);
  }
  if (
    record.observation.state.kind === 'restore' &&
    event.observation.state.kind === 'restore' &&
    record.observation.state.version !== event.observation.state.version
  )
    record.work = null;
  record.observation =
    event.observation.state.kind === 'restore'
      ? { ...event.observation, state: { ...event.observation.state, metadata: null } }
      : event.observation;
  if (event.observation.state.kind === 'ready') record.work = null;
  if (event.observation.state.kind === 'restore' && !record.work) {
    const files = attachments(event.observation.state.metadata, event.context);
    record.work = {
      after: 0,
      scope: null,
      ...(files &&
      object(event.observation.state.metadata) &&
      typeof event.observation.state.metadata.historyScope === 'string'
        ? {
            checkpoint: {
              after: event.observation.state.metadata.historyAfter as number,
              scope: event.observation.state.metadata.historyScope,
            },
          }
        : {}),
      done: false,
      queue: [],
      clearText: false,
      attachments: files ?? [],
      valid: files !== null,
      status: files ? 'checking' : 'failed',
      restore: null,
    };
  }
  return retained;
}
export function draftReady(
  records: ComposerReadiness[],
  session: Pick<AgentSession, 'id' | 'dataSource'> | null,
) {
  return (
    !session ||
    records.some(
      (record) =>
        record.ready &&
        record.context === session.id &&
        sameValue(record.dataSource, session.dataSource),
    )
  );
}
export function nextDraftStep(
  records: DraftRecord[],
  retained: AttachmentDrafts,
): DraftStep | null {
  for (const record of records) {
    const work = record.work;
    if (!work || work.status !== 'checking' || record.observation.state.kind !== 'restore')
      continue;
    if (work.queue.length)
      return { kind: 'draftReceipt', owner: record.observation.owner, original: work.queue[0] };
    if (!work.done)
      return { kind: 'draftPage', owner: record.observation.owner, after: work.after };
    const missing = work.attachments.filter(
      (file) =>
        !retained.files.some(
          (current) =>
            current.id === file.id &&
            current.sessionId === record.context &&
            sameValue(current.dataSource, record.dataSource),
        ),
    );
    retained.files.push(...missing.map((file) => ({ ...file, dataSource: record.dataSource })));
    work.restore = {
      owner: record.observation.owner,
      version: record.observation.state.version,
      clearText: work.clearText,
    };
    work.status = 'applying';
  }
  return null;
}
export function draftPage(
  records: DraftRecord[],
  step: Extract<DraftStep, { kind: 'draftPage' }>,
  page: CallerIntent[],
) {
  const record = records.find((record) => record.observation.owner === step.owner);
  if (
    !record?.work ||
    record.work.after !== step.after ||
    record.observation.state.kind !== 'restore'
  )
    return;
  const work = record.work;
  try {
    if (page.length > 1) throw new Error('Draft history page exceeds its requested bound');
    if (page[0]) {
      const first = page[0];
      work.scope = JSON.stringify([
        first.caller.stateDomain,
        first.caller.deployment,
        first.consumer,
      ]);
      const checkpoint = work.checkpoint;
      delete work.checkpoint;
      if (checkpoint?.scope === work.scope && checkpoint.after >= first.cursor) {
        work.after = checkpoint.after;
        return;
      }
    }
    for (const entry of page) {
      if (!Number.isSafeInteger(entry.cursor) || entry.cursor <= work.after)
        throw new Error('Invalid caller history cursor');
      work.after = entry.cursor;
      const sent = correlation(entry, record);
      if (
        sent &&
        ((sent.owner === record.observation.state.snapshot.text.owner &&
          sent.revision >= record.observation.state.snapshot.text.revision) ||
          work.attachments.some((file) => sent.attachments.includes(file.id)))
      )
        work.queue.push(entry);
    }
    work.done = page.length === 0;
  } catch {
    work.status = 'failed';
  }
}
export function draftReceipt(
  records: DraftRecord[],
  step: Extract<DraftStep, { kind: 'draftReceipt' }>,
  reply: OperationReply,
) {
  const record = records.find((record) => record.observation.owner === step.owner);
  if (
    !record?.work ||
    record.observation.state.kind !== 'restore' ||
    record.work.queue[0]?.cursor !== step.original.cursor
  )
    return;
  const work = record.work;
  if (
    !sameValue(reply.reference, step.original.reference) ||
    reply.receipt.kind !== 'recorded' ||
    reply.receipt.outcome.kind !== 'returned'
  ) {
    work.status = 'unknown';
    return;
  }
  const result = reply.receipt.outcome.value;
  if (
    object(result) &&
    result.kind === 'rejected' &&
    result.sessionId === record.context &&
    result.reason === 'skills_unavailable'
  ) {
    // A known rejection consumed neither the submitted text nor its attachments.
    work.queue.shift();
    return;
  }
  if (
    !object(result) ||
    !(
      (result.kind === 'started' && text(result.invocationId, 128)) ||
      (result.kind === 'queued' &&
        text(result.messageId, 128) &&
        Number.isSafeInteger(result.revision) &&
        (result.revision as number) >= 0)
    ) ||
    result.sessionId !== record.context
  ) {
    work.status = 'unknown';
    return;
  }
  try {
    const sent = correlation(step.original, record);
    if (!sent) throw new Error('Missing original draft correlation');
    if (
      sent.owner === record.observation.state.snapshot.text.owner &&
      sent.revision >= record.observation.state.snapshot.text.revision
    )
      work.clearText = true;
    work.attachments = work.attachments.filter((file) => !sent.attachments.includes(file.id));
    work.queue.shift();
  } catch {
    work.status = 'failed';
  }
}
export function draftFailed(records: DraftRecord[], step: DraftStep) {
  const record = records.find((record) => record.observation.owner === step.owner);
  if (record?.work) record.work.status = 'failed';
}
export function retryDraft(records: DraftRecord[], owner: string) {
  const record = records.find((record) => record.observation.owner === owner);
  if (!record?.work || record.observation.state.kind !== 'restore') return;
  if (record.work.status === 'unknown') record.work.status = 'checking';
  else if (record.work.status === 'failed') {
    if (record.work.valid)
      record.work = {
        after: 0,
        scope: null,
        done: false,
        queue: [],
        clearText: record.work.clearText,
        attachments: record.work.attachments,
        valid: true,
        status: 'checking',
        restore: null,
      };
  }
}
