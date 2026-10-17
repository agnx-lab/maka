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
  AgentHistorySourcePage,
  ComponentInput,
  MarkdownTextResult,
  SourceField,
  SourceFormat,
} from '../../packages/plugin-sdk/src/index.js';
import type { Message, TimelineProps, MessageCopyObservation } from './contracts.js';
import { comparePosition, type RowPosition } from './history-position.js';
import { sameValue } from './value.js';
import { copy as localized } from './locale-catalogs/message-copy.js';

import { toolLabel } from './message-view.js';
function utf8Bytes(text: string) {
  let bytes = 0;
  for (const character of text) {
    const point = character.codePointAt(0)!;
    bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
  }
  return bytes;
}

// Native copy accepts 64 KiB of UTF-8. Escaped control characters also need
// a bounded JSON delivery through Timeline, Root and the physical copy widget.
function fits(text: string) {
  return utf8Bytes(text) <= 65536 && utf8Bytes(JSON.stringify(text)) <= 131074;
}
export interface MessageCopy {
  basis: {
    request: number;
    sessionId: string;
    through: number;
    landing: number;
    position: RowPosition | null;
    field: SourceField;
    markdown: boolean;
    tool: Message['tool'];
    locale: string;
    heading: string | null;
  };
  key: string;
  serial: number;
  pending: { key: string; kind: 'source' | 'format' } | null;
  preparing: number;
  arguments: boolean;
  total: number | null;
  format: SourceFormat | null;
  text: string;
  raw: string | null;
  rawFormat: SourceFormat | null;
  rendered: string | null;
  status: MessageCopyObservation['status'];
}
interface State {
  revision: number;
  copy: MessageCopy | null;
}
function action(copy: MessageCopy, actions: Action[], kind: 'source' | 'format') {
  const key = copy.key + '-' + ++copy.serial;
  copy.pending = { key, kind };
  const { basis } = copy;
  const position = copy.arguments ? basis.tool!.call!.position : basis.position!;
  const field = copy.arguments ? 'arguments' : basis.field;
  actions.push({
    key,
    binding: kind === 'source' ? 'agent' : 'formatter',
    member: 0,
    method: kind === 'source' ? 'source' : 'plain',
    resources: {},
    input:
      kind === 'source'
        ? {
            sessionId: basis.sessionId,
            through: basis.through,
            source: { kind: 'row', position },
            field,
            position: copy.text
              ? { kind: 'after', offset: utf8Bytes(copy.text) }
              : { kind: 'start' },
          }
        : { source: copy.raw! },
  });
}
export function syncMessageCopy(
  state: State,
  props: TimelineProps,
  message: Message | null,
  actions: Action[],
) {
  if (
    !props.copyRequest ||
    !props.toolContext ||
    props.source ||
    props.search ||
    props.loading ||
    !message
  ) {
    state.copy = null;
    return;
  }
  const result = message.tool?.result;
  const tool = message.role === 'tool_call' || message.role === 'tool_result';
  const field: SourceField = tool
    ? result || message.role === 'tool_result'
      ? 'result'
      : 'arguments'
    : message.role === 'thinking'
      ? 'thinking'
      : 'body';
  const position = tool
    ? (result?.position ??
      (message.role === 'tool_result'
        ? message.source
        : (message.tool?.call?.position ?? message.source)))
    : message.source;
  const text = localized(props.locale);
  const label = toolLabel(
    {
      ...message,
      waiting: !!message.tool && props.waitingTools.includes(message.tool.toolUseId),
      live: !!message.tool && props.liveTools.includes(message.tool.toolUseId),
    },
    props.locale,
  );
  const heading = tool
    ? [
        (message.role === 'tool_call' ? text.toolActivity : text.toolOutput) +
          (label.name ? ' · ' + label.name : ''),
        label.status ?? (message.tool?.result?.isError ? text.toolReportedError : null),
      ]
        .filter(Boolean)
        .join('\n')
    : null;
  const basis = {
    request: props.copyRequest,
    ...props.toolContext,
    landing: props.landing.sequence,
    position,
    field,
    markdown: message.role === 'assistant' || message.role === 'thinking',
    tool: tool ? message.tool : null,
    locale: props.locale,
    heading,
  };
  if (state.copy && sameValue(state.copy.basis, basis)) return;
  state.copy = {
    basis,
    key: `message-copy-${props.copyRequest}-${state.revision}`,
    serial: 0,
    pending: null,
    preparing: 0,
    arguments: false,
    total: null,
    format: null,
    text: '',
    raw: null,
    rawFormat: null,
    rendered: null,
    status: position === null ? 'streaming' : 'loading',
  };
  if (position !== null) action(state.copy, actions, 'source');
}
export function messageCopyEvent(state: State, input: ComponentInput, actions: Action[]): boolean {
  if (input.kind !== 'action' || !state.copy?.pending || state.copy.pending.key !== input.key)
    return false;
  const copy = (state.copy = { ...state.copy });
  const pending = copy.pending!;
  copy.pending = null;
  const result = input.event as ActionOutcome;
  if (result.status !== 'succeeded') {
    copy.status = 'unavailable';
    return true;
  }
  if (pending.kind === 'format') {
    const value = result.value as MarkdownTextResult;
    if (value.kind === 'too_large' || (value.kind === 'ready' && !fits(value.text)))
      copy.status = 'too_large';
    else if (value.kind === 'ready') {
      copy.rendered = value.text || null;
      copy.status = value.text ? 'ready' : 'empty';
    } else copy.status = 'unavailable';
    return true;
  }
  const page = result.value as AgentHistorySourcePage;
  if (page.through !== copy.basis.through) {
    copy.status = 'unavailable';
    return true;
  }
  if (page.kind === 'preparing') {
    if (++copy.preparing <= 8) action(copy, actions, 'source');
    else copy.status = 'unavailable';
    return true;
  }
  const source = page.source,
    offset = utf8Bytes(copy.text),
    end = source.offset + utf8Bytes(source.text);
  if (
    comparePosition(
      source.position,
      (copy.arguments ? copy.basis.tool!.call!.position : copy.basis.position)!,
    ) !== 0 ||
    source.field !== (copy.arguments ? 'arguments' : copy.basis.field) ||
    (copy.basis.tool && !sameValue(source.tool, copy.basis.tool)) ||
    source.path != null ||
    !source.fields.includes(source.field) ||
    source.offset !== offset ||
    end > source.totalBytes ||
    (!source.text && source.totalBytes > 0) ||
    (copy.total !== null && (source.totalBytes !== copy.total || source.format !== copy.format))
  ) {
    copy.status = 'unavailable';
    return true;
  }
  if (source.totalBytes > 65536) {
    copy.status = 'too_large';
    copy.text = '';
    return true;
  }
  copy.total = source.totalBytes;
  copy.format = source.format;
  copy.text += source.text;
  if (end < source.totalBytes) {
    action(copy, actions, 'source');
    return true;
  }
  if (!fits(copy.text)) {
    copy.status = 'too_large';
    copy.text = '';
    return true;
  }
  const body = copy.text;
  copy.text = '';
  if (!copy.arguments) {
    copy.raw = body || null;
    copy.rawFormat = source.format;
  }
  if (copy.arguments) finishTool(copy, body);
  else if (copy.basis.heading !== null) {
    if (copy.basis.field === 'result' && copy.basis.tool?.call) {
      copy.arguments = true;
      copy.total = null;
      copy.format = null;
      copy.preparing = 0;
      action(copy, actions, 'source');
    } else finishTool(copy, copy.basis.field === 'arguments' ? body : null);
  } else if (copy.raw === null) copy.status = 'empty';
  else if (copy.basis.markdown && source.format === 'text') action(copy, actions, 'format');
  else {
    copy.rendered = copy.raw;
    copy.status = 'ready';
  }
  return true;
}
// Preserve JSON tokens (including integers beyond JS precision) while decoding
// string values for reading. Raw source remains a separate exact-byte choice.
function readable(source: string, format: SourceFormat | null): string {
  if (format !== 'json') return source;
  try {
    JSON.parse(source);
  } catch {
    return source;
  }
  const tokens = source.match(
    /"(?:\\.|[^"\\])*"|true|false|null|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|[{}\[\],:]/g,
  )!;
  let result = '',
    depth = 0,
    budget = 0;
  function append(text: string) {
    budget += utf8Bytes(text);
    if (budget > 65536) throw new Error('copy budget');
    result += text;
  }
  const newline = () => append('\n' + '  '.repeat(depth));
  for (const [index, token] of tokens.entries()) {
    if (token === '{' || token === '[') {
      append(token);
      depth++;
      if (tokens[index + 1] !== (token === '{' ? '}' : ']')) newline();
    } else if (token === '}' || token === ']') {
      depth--;
      if (tokens[index - 1] !== (token === '}' ? '{' : '[')) newline();
      append(token);
    } else if (token === ',') {
      append(token);
      newline();
    } else if (token === ':') append(': ');
    else
      append(
        token.startsWith('"') && tokens[index + 1] !== ':' ? (JSON.parse(token) as string) : token,
      );
  }
  return result;
}
function finishTool(copy: MessageCopy, args: string | null) {
  const text = localized(copy.basis.locale);
  try {
    const sections = [copy.basis.heading!];
    if (args !== null) sections.push(text.toolArguments + '\n' + readable(args, copy.format));
    if (copy.basis.field === 'result')
      sections.push(text.toolResult + '\n' + readable(copy.raw ?? '', copy.rawFormat));
    const rendered = sections.join('\n\n');
    if (!fits(rendered)) throw new Error('copy budget');
    copy.rendered = rendered;
    copy.status = 'ready';
  } catch {
    copy.status = 'too_large';
  }
}
export function messageCopyObservation(state: State): MessageCopyObservation | null {
  const copy = state.copy;
  return copy
    ? {
        request: copy.basis.request,
        sessionId: copy.basis.sessionId,
        through: copy.basis.through,
        landing: copy.basis.landing,
        field: copy.basis.field,
        status: copy.status,
        source: copy.raw,
        message: copy.rendered,
      }
    : null;
}
