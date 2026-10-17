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

import type { HistorySource } from '../../packages/plugin-sdk/src/index.js';
import type { Message, Theme, ToolDisplayStatus } from './contracts.js';
import { copy, type Copy } from './locale-catalogs/message-view.js';
import { toolPresentation } from '../agent/tool-details/index.js';
import { compactLabel, type Node, type Presentation } from './view.js';

export function messageOriginCaption(origin: Message['origin'], locale: string): string | null {
  if (!origin) return null;
  const text = copy(locale);
  switch (origin.kind) {
    case 'workhub_task':
      return text.workhubTask;
    case 'workhub_graph':
      return text.workhubGraph;
  }
}

function historyModelCaption(model: Message['model'], text: Copy): string | null {
  if (!model) return null;
  switch (model.kind) {
    case 'native':
      return model.reportedModelId === null
        ? `${text.historyRequestedModel}: ${model.requestedModelId}`
        : `${text.historyModel}: ${model.reportedModelId}`;
    case 'imported':
      return `${text.historyImportedModel}: ${model.modelId}`;
  }
}

export function sourceFieldLabel(locale: string, field: HistorySource['field']): string {
  const text = copy(locale);
  return {
    body: text.findFieldBody,
    thinking: text.findFieldThinking,
    tool_name: text.findFieldTool,
    arguments: text.findFieldArguments,
    result: text.findFieldResult,
    receipt: text.findFieldReceipt,
    failure_message: text.findFieldFailure,
    error_class: text.findFieldError,
  }[field];
}

// Host-recorded milliseconds; live deltas never synthesize a message time.
function timeLabel(millis: number | null): string | null {
  if (millis === null) return null;
  const at = new Date(millis),
    now = new Date();
  if (!Number.isFinite(at.getTime())) return null;
  const pad = (value: number) => String(value).padStart(2, '0');
  const time = pad(at.getHours()) + ':' + pad(at.getMinutes());
  const date = pad(at.getMonth() + 1) + '-' + pad(at.getDate());
  return at.getFullYear() !== now.getFullYear()
    ? at.getFullYear() + '-' + date + ' ' + time
    : at.getMonth() !== now.getMonth() || at.getDate() !== now.getDate()
      ? date + ' ' + time
      : time;
}

export function toolLabel(
  message: {
    tool: HistorySource['tool'];
    live?: boolean;
    waiting?: boolean;
    detailStatus?: ToolDisplayStatus | null;
  },
  locale: string,
) {
  const text = copy(locale);
  const title = message.tool?.call?.title;
  const kind = toolPresentation(message.tool?.call?.presentation);
  const name = title
    ? (title.translations?.[locale] ??
      title.translations?.[locale.split('-')[0]!] ??
      title.fallback)
    : kind
      ? {
          read: text.toolReadAction,
          write: text.toolWriteAction,
          edit: text.toolEditAction,
          patch: text.toolPatchAction,
          glob: text.toolSearchAction,
          grep: text.toolSearchAction,
          execute: text.terminalCommand,
          processRead: text.toolProcessRead,
          stdin: text.toolProcessInput,
          stop: text.stop,
        }[kind]
      : message.tool?.call?.name;
  const result = message.tool?.result;
  const settlement = result?.settlement;
  const pending = message.live && message.tool?.call?.pending;
  const state =
    settlement === 'failed'
      ? 'failed'
      : settlement === 'rejected'
        ? 'rejected'
        : settlement === 'unknown'
          ? 'unknown'
          : result?.isError
            ? 'reportedError'
            : result
              ? (message.detailStatus ?? (kind ? 'unresolved' : 'returned'))
              : pending
                ? message.waiting
                  ? 'waiting'
                  : message.tool?.call?.dispatched
                    ? 'running'
                    : 'pending'
                : message.tool?.call?.dispatched
                  ? 'unsettled'
                  : 'missing';
  const states = {
    failed: [text.toolFailed, 'error', '!', '×'],
    rejected: [text.toolRejected, 'error', '!', '×'],
    unknown: [text.toolUnknown, 'warning', '?', '?'],
    reportedError: [text.toolReportedError, 'error', '!', '×'],
    returned: [text.toolReturned, 'success', '+', '•'],
    unresolved: [text.toolReturned, 'subtle', '.', '•'],
    succeeded: [text.completed, 'success', '+', '•'],
    cancelled: [text.toolProcessCancelled, 'warning', '!', '×'],
    waiting: [text.waiting, 'warning', '?', '◇'],
    running: [text.toolDispatched, 'accent', '*', '◌'],
    pending: [text.toolPending, 'accent', '.', '·'],
    unsettled: [text.toolDispatchedMissing, 'warning', '?', '?'],
    missing: [text.toolMissing, 'warning', '?', '?'],
  } as const;
  const [status, tone, ascii, symbol] = states[state];
  return { name, status, tone, ascii, symbol };
}

export function messageView<N extends Node>(
  presentation: Presentation<N>,

  theme: Theme,
  locale: string,
  message: {
    browse?: { key: string; current: boolean };
    traceDepth?: number;
    detailStatus?: ToolDisplayStatus | null;
    separate?: boolean;
    separateBefore?: boolean;
    id: string;
    role: HistorySource['role'] | 'thinking';
    origin?: Message['origin'];
    model?: Message['model'];
    timestamp: number | null;
    text: string;
    tool: HistorySource['tool'];
    continued: boolean;
    expanded: boolean;
    structured?: boolean;
    waiting?: boolean;
    live?: boolean;
    toggle: string | null;
    body: N;
    actions: N[];
    sourceActions: N[];
  },
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    tool = message.role === 'tool_call' || message.role === 'tool_result',
    expanded = message.expanded,
    body = message.body;
  const modelCaption =
    message.role === 'assistant' || message.role === 'thinking'
      ? historyModelCaption(message.model, text)
      : null;
  const roleCaption =
    message.role === 'thinking'
      ? text.findFieldThinking
      : message.role === 'user'
        ? (messageOriginCaption(message.origin, locale) ?? text.you)
        : message.role === 'assistant'
          ? text.assistant
          : message.role === 'tool_call'
            ? text.toolActivity
            : message.role === 'failure'
              ? text.findFieldFailure
              : text.toolOutput;
  const caption = modelCaption ? roleCaption + ' · ' + modelCaption : roleCaption;
  const { name, status, tone, ascii, symbol } = tool
    ? toolLabel(message, locale)
    : { name: null, status: null, tone: 'subtle' as const, ascii: '*', symbol: '•' };
  const result = message.tool?.result,
    settlement = result?.settlement;
  const time =
    message.role === 'user' || message.role === 'assistant' ? timeLabel(message.timestamp) : null;
  {
    const user = message.role === 'user';
    const thinking = message.role === 'thinking';
    const failed =
      message.role === 'failure' ||
      result?.isError ||
      settlement === 'failed' ||
      settlement === 'rejected';
    const marker = tool
      ? theme.ascii
        ? ascii
        : tone === 'success'
          ? '·'
          : symbol
      : user
        ? theme.ascii
          ? '>'
          : '❯'
        : thinking || failed
          ? theme.ascii
            ? '*'
            : '•'
          : ' ';
    const markerColor = tool
      ? theme[tone]
      : failed
        ? theme.error
        : user || message.live
          ? theme.accent
          : theme.subtle;
    const row = (key: string, children: N[], band = false) =>
      ui.box(key, children, {
        row: true,
        gap: 0,
        surface: band,
        terminal: { grow: 0 },
        ...(time && key === 'message-body-' + message.id
          ? {
              wide: {
                min_width: 88,
                direction: 'row' as const,
                children: { ['time-' + message.id]: { basis: Math.min(time.length + 2, 24) } },
              },
            }
          : {}),
      });
    const toolControl = {
      background: 0,
      align: 'start' as const,
      text: { ...theme.markdown.body, foreground: theme.muted },
    };
    const gutter = (rail = false) =>
      ui.box(
        'gutter-' + message.id + (rail ? '-rail' : ''),
        [
          ...(tool
            ? [
                ui.box('indent-' + message.id + (rail ? '-rail' : ''), [], {
                  terminal: { basis: 2 + 2 * (message.traceDepth ?? 0) },
                }),
              ]
            : []),
          ...(rail
            ? expanded && ui.rule
              ? [ui.rule('rail-' + message.id, 'column')]
              : []
            : [
                ui.text(
                  'marker-' + message.id,
                  marker,
                  markerColor,
                  400,
                  {},
                  tool ? (status ?? '') : '',
                ),
              ]),
        ],
        {
          row: true,
          gap: 0,
          terminal: { basis: tool ? 4 + 2 * (message.traceDepth ?? 0) : 2 },
          surface: user,
        },
      );
    const heading = tool
      ? row('tool-heading-' + message.id, [
          gutter(),
          message.toggle
            ? ui.button(
                message.toggle,
                `${expanded ? (theme.ascii ? 'v' : '⌄') : theme.ascii ? '>' : '›'} ${compactLabel(name ?? caption, 72)}`,
                false,
                false,
                toolControl,
                undefined,
                expanded ? text.collapseTool : text.expandTool,
              )
            : ui.text('role-' + message.id, name ?? caption, theme.muted),
        ])
      : row(
          'message-body-' + message.id,
          [
            message.browse && ui.item
              ? ui.item(message.browse.key, gutter(), message.browse.current, false, {
                  followFocus: true,
                  submit: true,
                  submitSpace: true,
                })
              : gutter(),
            ui.box(
              'body-' + message.id,
              [
                ...((user && message.origin) || modelCaption
                  ? [ui.text('role-' + message.id, caption, theme.muted)]
                  : []),
                body,
              ],
              {
                gap: 0,
                surface: user,
                terminal: { grow: 1, shrink: true },
              },
            ),
            ...(time
              ? [
                  ui.box(
                    'time-' + message.id,
                    [ui.text('timestamp-' + message.id, ' ' + time, theme.subtle)],
                    { surface: user, terminal: { basis: 0 } },
                  ),
                ]
              : []),
          ],
          user,
        );
    const actions = tool
      ? message.actions
      : [
          ...(!message.continued && message.text
            ? [
                ui.button(
                  'copy-raw-' + message.id,
                  user ? text.copyMessage : text.copySource,
                  false,
                  false,
                  {},
                  message.text,
                ),
              ]
            : []),
          ...message.sourceActions,
        ];
    const hasMenu = actions.length > 0;
    const card = ui.box(
      (hasMenu || message.separateBefore ? 'message-card-' : 'message-') + message.id,
      [
        ...(user && !theme.terminal?.terminal
          ? [ui.box('band-top-' + message.id, [], { surface: true, terminal: { basis: 1 } })]
          : []),
        tool && message.browse && ui.item
          ? ui.item(message.browse.key, heading, message.browse.current, false, {
              followFocus: true,
              submit: true,
              submitSpace: true,
            })
          : heading,
        ...(tool
          ? [
              ui.box(
                'details-' + message.id,
                [
                  row('tool-detail-row-' + message.id, [
                    gutter(true),
                    ui.box(
                      'tool-panel-' + message.id,
                      [
                        body,
                        ui.box('tool-actions-' + message.id, [], {
                          visible: expanded,
                          row: true,
                          gap: 1,
                          terminal: { wrap: true },
                        }),
                        ...(expanded && message.toggle
                          ? [
                              ui.button(
                                'collapse-' + message.id,
                                `${theme.ascii ? '^' : '↑'} ${text.collapseTool}`,
                                false,
                                false,
                                toolControl,
                              ),
                            ]
                          : []),
                      ],
                      { gap: 0, terminal: { grow: 1, shrink: true } },
                    ),
                  ]),
                ],
                { gap: 0, visible: expanded || !!message.structured },
              ),
            ]
          : []),
        ...(message.continued
          ? [ui.text('continued-' + message.id, '  ' + text.partial, theme.subtle)]
          : []),
        ...(user && !theme.terminal?.terminal
          ? [ui.box('band-bottom-' + message.id, [], { surface: true, terminal: { basis: 1 } })]
          : []),
        ...(message.separate
          ? [ui.box('message-space-' + message.id, [], { terminal: { basis: 1 } })]
          : []),
      ],
      { gap: 0, surface: user },
    );
    const content = hasMenu
      ? ui.contextMenu!(
          (message.separateBefore ? 'message-menu-' : 'message-') + message.id,
          caption,
          card,
          actions,
          text.copy,
        )
      : card;
    return message.separateBefore
      ? ui.box(
          'message-' + message.id,
          [ui.box('message-space-before-' + message.id, [], { terminal: { basis: 1 } }), content],
          { gap: 0 },
        )
      : content;
  }
}
