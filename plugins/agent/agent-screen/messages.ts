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

import { comparePosition } from '../../shared/history-position.js';
import type { Message, TimelineProps } from '../../shared/contracts.js';

import { type DisplayPart, type State } from './state.js';

export function utf8Bytes(text: string): number {
  let bytes = 0;
  for (const character of text) {
    const point = character.codePointAt(0)!;
    bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
  }
  return bytes;
}
// Raw fences can advance while model text is still open. Only an actual step
// closure covers its live prefix for both atomic and item-accepting providers.

export function appendLive(
  state: State,
  sequence: number,
  part: DisplayPart,
  invocationId: string,
  text: string,
) {
  const visit = state.visits.find((entry) => entry.id === state.selected?.id);
  if (visit && state.run?.invocationId === invocationId) {
    const id = (part.textKind === 'thinking' ? 'thinking-' : '') + part.messageId;
    const previous = visit.activity?.turnId === state.run.turnId ? visit.activity : null;
    visit.activity = {
      through: previous?.through ?? 0,
      position: null,
      turnId: state.run.turnId,
      message: {
        id,
        contentKey: 'body-' + id,
        role: part.textKind === 'thinking' ? 'thinking' : 'assistant',
        text: ((previous?.message?.id === id ? previous.message.text : '') + text)
          .slice(-4096)
          .replace(/^[\uDC00-\uDFFF]/u, ''),
        timestamp: null,
        tool: null,
        continued: false,
        source: null,
        image: false,
      },
    };
  }
  const messageId = part.messageId;
  const field = part.textKind === 'thinking' ? 'thinking' : 'body';
  if (
    state.window &&
    sequence <= state.window.page.through &&
    state.window.page.chunks.some(
      (chunk) => chunk.messageId === messageId && (chunk.field ?? 'body') === field,
    )
  )
    return;
  const previous = state.live.at(-1);
  const adjacent =
    previous?.closedAt === null &&
    previous.messageId === messageId &&
    previous.invocationId === invocationId &&
    previous.stepId === part.stepId &&
    previous.field === field;
  const ids = new Set(state.live.map((fragment) => fragment.messageId));
  let remaining = 16000 - state.live.reduce((total, fragment) => total + fragment.text.length, 0);
  // Thinking shares the existing prefix budget but leaves room for an answer
  // before atomic providers publish either field. Never evict a displayed prefix.
  if (field === 'thinking') {
    const thinking = state.live.filter((fragment) => fragment.field === 'thinking');
    const thinkingIds = new Set(thinking.map((fragment) => fragment.messageId));
    if (
      (!adjacent && thinking.length >= 32) ||
      (!thinkingIds.has(messageId) && thinkingIds.size >= 8)
    )
      return;
    remaining = Math.min(
      remaining,
      8000 - thinking.reduce((total, fragment) => total + fragment.text.length, 0),
    );
  }
  // A truncated surrogate can leave one unused code unit; keep the prefix closed.
  if (
    remaining <= 1 ||
    (!adjacent && state.live.length >= 64) ||
    (!ids.has(messageId) && ids.size >= 16)
  )
    return;
  text = text.slice(0, remaining);
  const last = text.charCodeAt(text.length - 1);
  if (last >= 0xd800 && last <= 0xdbff) text = text.slice(0, -1);
  if (text && adjacent) {
    previous.text += text;
    previous.sequence = sequence;
  } else if (text)
    state.live.push({
      sequence,
      messageId,
      invocationId,
      stepId: part.stepId,
      text,
      closedAt: null,
      field,
    });
}

export function closeLive(state: State, sequence: number, invocationId: string, stepId?: string) {
  for (const fragment of state.live)
    if (
      fragment.closedAt === null &&
      fragment.invocationId === invocationId &&
      (stepId === undefined || fragment.stepId === stepId)
    )
      fragment.closedAt = sequence;
}

function chunkId(chunk: NonNullable<State['window']>['page']['chunks'][number]) {
  return (
    (chunk.field === 'thinking' ? 'thinking-' + chunk.messageId : chunk.messageId) +
    (chunk.offset ? '-offset-' + chunk.offset : '')
  );
}

function historyMessage(chunk: NonNullable<State['window']>['page']['chunks'][number]): Message {
  return {
    id: chunkId(chunk),
    ...(chunk.compaction ? { compaction: chunk.compaction } : {}),
    ...(chunk.origin ? { origin: chunk.origin } : {}),
    ...(chunk.model ? { model: chunk.model } : {}),
    contentKey: 'body-' + chunkId(chunk),
    role: chunk.field === 'thinking' ? 'thinking' : chunk.role,
    timestamp: chunk.timestamp,
    text: chunk.text,
    tool: chunk.tool,
    continued: chunk.offset !== 0 || utf8Bytes(chunk.text) !== chunk.totalBytes,
    source: chunk.role === 'compaction' ? null : chunk.position,
    image: chunk.tool?.result?.image ?? false,
  };
}

export function messages(state: State): Message[] {
  const visit = state.visits.find((entry) => entry.id === state.selected?.id);
  const chunks = state.window?.page.chunks ?? [];
  if (visit?.submissions)
    visit.submissions = visit.submissions.filter(
      (entry) => !entry.messageId || !chunks.some((chunk) => chunk.messageId === entry.messageId),
    );
  const submissions = visit?.submissions ?? [];
  const canonical = chunks.map(historyMessage);
  const previews: Message[] = submissions.map((entry) => ({
    id: 'submission-' + entry.key,
    contentKey: 'submission-' + entry.key,
    role: 'user',
    timestamp: null,
    text: [entry.text, ...entry.attachmentNames]
      .filter(Boolean)
      .join('\n')
      .slice(0, 24000)
      .replace(/[\uD800-\uDBFF]$/u, ''),
    tool: null,
    continued: false,
    source: null,
    image: false,
    pending: entry.status !== 'accepted',
  }));
  if (!state.window?.latest) return [...canonical, ...previews];
  const live = new Map<string, Message>();
  for (const fragment of state.live) {
    if (
      fragment.sequence <= state.window.page.through &&
      state.window.page.chunks.some(
        (chunk) =>
          chunk.messageId === fragment.messageId && (chunk.field ?? 'body') === fragment.field,
      )
    )
      continue;
    const key =
      fragment.field === 'thinking' ? 'thinking-' + fragment.messageId : fragment.messageId;
    let message = live.get(key);
    if (!message) {
      message = {
        id: key,
        contentKey: 'body-' + key,
        role: fragment.field === 'thinking' ? 'thinking' : 'assistant',
        timestamp: null,
        text: '',
        tool: null,
        continued: false,
        source: null,
        image: false,
      };
      live.set(key, message);
    }
    message.text += fragment.text;
  }
  return [...canonical, ...previews, ...live.values()];
}

export function turnTimings(state: State, displayed: Message[]): TimelineProps['timings'] {
  const turns = new Map<string, TimelineProps['timings'][number]>();
  for (const chunk of state.window?.page.chunks ?? []) {
    if (!chunk.timing) continue;
    if (!chunk.timing.finished && (!state.window?.latest || state.run?.turnId !== chunk.turnId))
      continue;
    turns.set(chunk.turnId, {
      ...chunk.timing,
      turnId: chunk.turnId,
      afterMessageId: chunkId(chunk),
    });
  }
  if (state.window?.latest && state.run) {
    const timing = turns.get(state.run.turnId);
    if (timing && !timing.finished) {
      for (const fragment of state.live) {
        const id =
          fragment.field === 'thinking' ? 'thinking-' + fragment.messageId : fragment.messageId;
        if (
          fragment.invocationId === state.run.invocationId &&
          displayed.some((message) => message.id === id)
        )
          timing.afterMessageId = id;
      }
    }
  }
  return [...turns.values()];
}

// A single current output is sufficient; retain no second transcript window.
export function rememberActivity(state: State, page: NonNullable<State['window']>['page']) {
  const visit = state.visits.find((entry) => entry.id === state.selected?.id);
  const turnId = state.run?.turnId;
  if (!visit || !turnId) return;
  const previous = visit.activity?.turnId === turnId ? visit.activity : null;
  const chunk = page.chunks
    .filter((entry) => entry.turnId === turnId && entry.role !== 'user')
    .at(-1);
  const position = chunk?.position ?? null;
  const message = chunk ? historyMessage(chunk) : null;
  visit.activity = {
    through: Math.max(page.through, previous?.through ?? 0),
    position:
      previous?.position && (!position || comparePosition(previous.position, position) > 0)
        ? previous.position
        : position,
    turnId,
    message:
      previous?.position && (!position || comparePosition(previous.position, position) > 0)
        ? previous.message
        : message,
  };
}

export function activeOutput(state: State): Message | null {
  if (!state.run) return null;
  const activity = state.visits.find((entry) => entry.id === state.selected?.id)?.activity;
  return activity?.turnId === state.run.turnId ? activity.message : null;
}
