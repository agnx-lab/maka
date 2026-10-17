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

import type { Message } from './contracts.js';
import { comparePosition, type RowPosition } from './history-position.js';
import { sameValue } from './value.js';

/** Merge only reciprocal Host references, retaining both native source bodies.
 * A counterpart outside this bounded window remains accessible by its reference.
 */
function key(position: RowPosition | null): string {
  return position ? `${position.sourceSequence}:${position.itemIndex}` : '';
}
export function toolCards(
  messages: readonly Message[],
): { message: Message; result: Message | null; depth: number }[] {
  const bySequence = new Map<string, Message[]>();
  for (const message of messages) {
    if (message.source !== null && message.tool) {
      const rows = bySequence.get(key(message.source)) ?? [];
      rows.push(message);
      bySequence.set(key(message.source), rows);
    }
  }
  const resultsByCall = new Map<string, Set<string>>();
  for (const message of messages) {
    if (message.role === 'tool_result' && message.source !== null && message.tool?.call) {
      const sequences = resultsByCall.get(key(message.tool.call.position)) ?? new Set<string>();
      sequences.add(key(message.source));
      resultsByCall.set(key(message.tool.call.position), sequences);
    }
  }
  const paired = new Map<Message, Message>();
  const consumed = new Set<Message>();
  for (const call of messages) {
    const metadata = call.tool;
    if (
      call.role !== 'tool_call' ||
      (resultsByCall.get(key(call.source!))?.size ?? 0) > 1 ||
      !metadata?.call ||
      !metadata.result ||
      !sameValue(metadata.call.position, call.source) ||
      bySequence.get(key(call.source))?.length !== 1
    )
      continue;
    const candidates = bySequence.get(key(metadata.result.position));
    if (candidates?.length !== 1) continue;
    const result = candidates[0]!;
    if (
      result.role !== 'tool_result' ||
      !result.tool?.result ||
      !result.tool?.call ||
      comparePosition(result.source!, call.source!) <= 0 ||
      result.tool?.toolUseId !== metadata.toolUseId ||
      !sameValue(result.tool.call?.position, call.source) ||
      !sameValue(result.tool.call, metadata.call) ||
      !sameValue(result.tool.result?.position, result.source) ||
      result.tool.result.isError !== metadata.result.isError ||
      result.tool.result.image !== metadata.result.image ||
      result.tool.result.settlement !== metadata.result.settlement
    )
      continue;
    paired.set(call, result);
    consumed.add(result);
  }
  const rows = messages
    .filter((message) => !consumed.has(message))
    .map((message) => ({
      message:
        message.tool?.call &&
        bySequence.get(key(message.source!))?.length === 1 &&
        (message.role === 'tool_call' ||
          (!bySequence.has(key(message.tool.call.position)) &&
            (resultsByCall.get(key(message.tool.call.position))?.size ?? 0) === 1))
          ? { ...message, id: message.tool.toolUseId }
          : message,
      result: paired.get(message) ?? null,
      depth: 0,
    }));
  // Only visible, unambiguous earlier calls establish indentation. A parent
  // outside this bounded window does not invent an empty ancestor row.
  const parents = new Map<string, { position: RowPosition; depth: number }>();
  for (const row of rows) {
    const tool = row.message.tool;
    if (!tool?.call || row.message.id !== tool.toolUseId) continue;
    const parent = tool.call.ancestorToolUseIds
      .map((id) => parents.get(id))
      .find((parent) => parent && comparePosition(parent.position, tool.call!.position) < 0);
    row.depth = parent ? parent.depth + 1 : 0;
    parents.set(tool.toolUseId, { position: tool.call.position, depth: row.depth });
  }
  return rows;
}
