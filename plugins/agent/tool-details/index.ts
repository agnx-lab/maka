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

import { scrollBoundary } from '../../shared/scroll.js';
import type {
  Action,
  ActionOutcome,
  AgentHistorySourcePage,
  Component,
  DiffProps,
  HistorySource,
  SourcePosition,
  ReadingCommand,
} from '../../../packages/plugin-sdk/src/index.js';
import {
  toolDetailsContract,
  type ToolDetailsProps,
  type ToolDetailsContext,
  type ToolDisplayStatus,
} from '../../shared/contracts.js';
import { copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';

/** These keys explicitly name display contracts. Names and JSON shapes do not. */
export function filePresentation(key: string | null | undefined) {
  switch (key) {
    case 'maka.workspace.Files@1/readText':
      return 'read';
    case 'maka.workspace.Files@1/write':
      return 'write';
    case 'maka.workspace.Files@1/edit':
      return 'edit';
    case 'maka.workspace.Files@1/applyPatch':
      return 'patch';
    default:
      return null;
  }
}
export function toolPresentation(key: string | null | undefined) {
  switch (key) {
    case 'maka.workspace.Files@1/glob':
      return 'glob';
    case 'maka.workspace.Files@1/grep':
      return 'grep';
    case 'maka.workspace.Processes@1/execute':
      return 'execute';
    case 'maka.workspace.Processes@1/read':
      return 'processRead';
    case 'maka.workspace.Processes@1/writeStdin':
      return 'stdin';
    case 'maka.workspace.Processes@1/stop':
      return 'stop';
    default:
      return filePresentation(key);
  }
}
function compactPart(id: string) {
  return [
    'path',
    'status',
    'resolvedPath',
    'command',
    'reference',
    'resultKind',
    'processStatus',
    'exitCode',
    'pattern',
    'searchComplete',
  ].includes(id);
}
type Part = {
  field: 'arguments' | 'result';
  path: string[] | null;
  format: 'text' | 'json' | null;
  page: HistorySource | null;
  filePath: string | null;
  failed: boolean;
  pending: { key: string; position: SourcePosition } | null;
  position: SourcePosition;
  reveal?: ReadingCommand;
};
type State = {
  revision: number;
  serial: number;
  context: ToolDetailsProps['context'];
  tool: ToolDetailsProps['tool'];
  parts: Record<string, Part>;
  published: ToolDetailsContext | null;
};
function bytes(text: string) {
  let size = 0;
  for (const point of text) {
    const value = point.codePointAt(0)!;
    size += value <= 0x7f ? 1 : value <= 0x7ff ? 2 : value <= 0xffff ? 3 : 4;
  }
  return size;
}
function lines(text: string) {
  return text ? text.split('\n').length - Number(text.endsWith('\n')) : 0;
}
function small(text: string) {
  return bytes(text) <= 8192 && lines(text) <= 256 && !text.includes('\0');
}
function sameIdentity(state: State, props: ToolDetailsProps) {
  return (
    state.context.sessionId === props.context.sessionId &&
    state.tool.toolUseId === props.tool.toolUseId &&
    sameValue(state.tool.call?.position, props.tool.call?.position) &&
    state.tool.call?.presentation === props.tool.call?.presentation &&
    sameValue(state.tool.result?.position, props.tool.result?.position)
  );
}
function add(
  state: State,
  id: string,
  field: Part['field'],
  path: Part['path'],
  format: Part['format'] = 'text',
) {
  state.parts[id] ??= {
    field,
    path,
    format,
    page: null,
    filePath: null,
    failed: false,
    pending: null,
    position: { kind: 'start' },
  };
}
function request(state: State, part: Part, position: SourcePosition, actions: Action[]) {
  const rowPosition =
    part.field === 'arguments' ? state.tool.call?.position : state.tool.result?.position;
  if (!rowPosition || part.pending) return;
  const key = 'tool-value-' + ++state.serial;
  part.pending = { key, position };
  part.position = position;
  part.failed = false;
  actions.push({
    key,
    binding: 'agent',
    member: 0,
    method: 'source',
    resources: {},
    input: {
      ...state.context,
      source: { kind: 'row', position: rowPosition },
      field: part.field,
      path: part.path,
      position,
    },
  });
}
function complete(part: Part) {
  const page = part.page;
  return !!page && page.offset === 0 && bytes(page.text) === page.totalBytes && !part.failed;
}
function scalar(state: State, id: string): string | number | boolean | null {
  const part = state.parts[id];
  if (!part || !complete(part)) return null;
  if (part.format === 'text') return part.page!.text;
  try {
    const value: unknown = JSON.parse(part.page!.text);
    if (typeof value === 'boolean') return value;
    return typeof value === 'number' &&
      Number.isSafeInteger(value) &&
      (id === 'exitCode' || value >= 0)
      ? value
      : null;
  } catch {
    return null;
  }
}
function accept(state: State, part: Part, value: AgentHistorySourcePage, position: SourcePosition) {
  if (value.through !== state.context.through) {
    part.failed = true;
    return;
  }
  const page = value.source,
    end = page.offset + bytes(page.text);
  const rowPosition =
    part.field === 'arguments' ? state.tool.call?.position : state.tool.result?.position;
  if (
    !sameValue(page.position, rowPosition) ||
    page.field !== part.field ||
    !sameValue(page.path ?? null, part.path) ||
    (part.format !== null && page.format !== part.format) ||
    page.tool?.toolUseId !== state.tool.toolUseId ||
    !sameValue(page.tool.call?.position, state.tool.call?.position) ||
    page.tool.call?.presentation !== state.tool.call?.presentation ||
    end > page.totalBytes ||
    (!page.text && page.totalBytes > 0) ||
    (part.page && part.page.totalBytes !== page.totalBytes) ||
    (position.kind === 'start' && page.offset !== 0) ||
    (position.kind === 'tail' && end !== page.totalBytes) ||
    (position.kind === 'after' && page.offset !== position.offset) ||
    (position.kind === 'before' && end !== position.offset)
  ) {
    part.failed = true;
    return;
  }
  part.page = page;
  part.filePath =
    page.offset === 0 &&
    end === page.totalBytes &&
    typeof value.filePath === 'string' &&
    bytes(value.filePath) <= 4096
      ? value.filePath
      : null;
}
export function toolDetails<N extends Node>(
  presentation: Presentation<N>,
): Component<State, ToolDetailsProps, N> {
  function frame(state: State, props: ToolDetailsProps, actions: Action[] = []) {
    if (!sameIdentity(state, props)) {
      state.context = { ...props.context };
      state.tool = props.tool;
      state.parts = {};
    }
    const kind = toolPresentation(state.tool.call?.presentation);
    const file = filePresentation(state.tool.call?.presentation);
    const search = kind === 'glob' || kind === 'grep';
    const process = !!kind && !file && !search;
    if (!kind && props.expanded) {
      if (state.tool.call) add(state, 'arguments', 'arguments', null, 'json');
      if (state.tool.result) add(state, 'result', 'result', null, null);
    }
    if (search && state.tool.call) {
      add(state, 'pattern', 'arguments', ['pattern']);
      if (props.expanded) add(state, 'searchArguments', 'arguments', [], 'json');
      if (state.tool.result) add(state, 'status', 'result', ['kind']);
      if (scalar(state, 'status') === 'completed') {
        add(state, 'searchComplete', 'result', ['value', 'complete'], 'json');
        if (props.expanded)
          add(state, 'matches', 'result', ['value', kind === 'glob' ? 'files' : 'matches'], 'json');
      } else if (props.expanded && scalar(state, 'status') === 'failed')
        add(state, 'failure', 'result', ['message']);
    }
    if (process && state.tool.call) {
      if (kind === 'execute') add(state, 'command', 'arguments', ['command']);
      else add(state, 'reference', 'arguments', ['ref']);
      if (state.tool.result) add(state, 'status', 'result', ['kind']);
      if (scalar(state, 'status') === 'completed') {
        add(state, 'resultKind', 'result', ['value', 'kind']);
        const resultKind = scalar(state, 'resultKind');
        if (resultKind === 'terminal' || resultKind === 'shell_run') {
          add(state, 'processStatus', 'result', ['value', 'status']);
          if (kind !== 'execute') add(state, 'command', 'result', ['value', 'cmd']);
          else if (resultKind === 'shell_run') add(state, 'reference', 'result', ['value', 'ref']);
          const status = scalar(state, 'processStatus');
          if (
            status === 'completed' ||
            status === 'failed' ||
            status === 'timed_out' ||
            status === 'cancelled'
          )
            add(state, 'exitCode', 'result', ['value', 'exitCode'], 'json');
          if (props.expanded) {
            add(state, 'cwd', 'result', ['value', 'cwd']);
            if (
              typeof status === 'string' &&
              !(
                kind === 'execute' &&
                resultKind === 'shell_run' &&
                (status === 'starting' || status === 'running')
              )
            )
              add(state, 'outputMode', 'result', ['value', 'output', 'mode']);
            if (status === 'failed' || status === 'orphaned')
              add(state, 'processFailure', 'result', ['value', 'failureMessage']);
            const mode = scalar(state, 'outputMode');
            if (mode === 'pipes') {
              for (const id of ['stdout', 'stderr']) {
                add(state, id, 'result', ['value', 'output', id]);
                add(
                  state,
                  id + 'Truncated',
                  'result',
                  ['value', 'output', id + 'Truncated'],
                  'json',
                );
              }
            } else if (mode === 'pty') {
              for (const id of ['screen', 'scrollback'])
                add(state, id, 'result', ['value', 'output', id]);
              add(state, 'truncated', 'result', ['value', 'output', 'truncated'], 'json');
            }
            if (mode === 'pipes' || mode === 'pty')
              add(state, 'redacted', 'result', ['value', 'output', 'redacted'], 'json');
            if (kind === 'stdin' || kind === 'stop')
              add(state, 'control', 'result', ['value', 'operation'], 'json');
          }
        }
      } else if (props.expanded && scalar(state, 'status') === 'failed')
        add(state, 'failure', 'result', ['message']);
    }
    if (file && state.tool.call) {
      add(state, 'path', 'arguments', kind === 'patch' ? ['operation', 'path'] : ['path']);
      if (state.tool.result) add(state, 'status', 'result', ['kind']);
      if (scalar(state, 'status') === 'completed' && (kind === 'write' || kind === 'edit'))
        add(state, 'resolvedPath', 'result', ['value', 'path']);
      if (props.expanded) {
        if (kind === 'edit') {
          add(state, 'before', 'arguments', ['old_string']);
          add(state, 'after', 'arguments', ['new_string']);
        } else if (kind === 'write') add(state, 'after', 'arguments', ['content']);
        else if (kind === 'patch') {
          add(state, 'operation', 'arguments', ['operation', 'type']);
          const operation = scalar(state, 'operation');
          if (operation === 'create_file' || operation === 'update_file')
            add(state, 'patch', 'arguments', ['operation', 'diff']);
        }
        if (scalar(state, 'status') === 'completed') {
          if (kind === 'read') {
            add(state, 'content', 'result', ['value', 'content']);
            for (const name of ['offset', 'returnedLines', 'totalLines'])
              add(state, name, 'result', ['value', name], 'json');
          } else if (kind === 'write') add(state, 'before', 'result', ['value', 'previousContent']);
        } else if (scalar(state, 'status') === 'failed')
          add(state, 'failure', 'result', ['message']);
      }
    }
    for (const [id, part] of Object.entries(state.parts))
      if (
        props.load !== false &&
        (props.expanded || compactPart(id)) &&
        !part.page &&
        !part.pending &&
        !part.failed
      )
        request(state, part, part.position, actions);
    const ui = presentation.paint(props.theme),
      text = copy(props.locale);
    const labels: Record<string, string> = {
      arguments: text.toolArguments,
      result: text.toolResult,
      path: text.toolFilePath,
      command: text.terminalCommandLabel,
      reference: text.toolProcessRef,
      cwd: text.toolProcessCwd,
      stdout: text.toolStdout,
      stderr: text.toolStderr,
      screen: text.toolScreen,
      scrollback: text.toolScrollback,
      processFailure: text.toolProcessFailure,
      control: text.toolProcessControl,
      before: kind === 'write' ? text.toolRecordedBefore : text.toolRequestedBefore,
      after: text.toolRequestedAfter,
      content: text.toolReadContent,
      patch: text.toolPatch,
      operation: text.toolPatchOperation,
      failure: text.toolBusinessFailure,
      searchArguments: text.toolArguments,
      matches: kind === 'glob' ? text.toolMatchingFiles : text.toolMatchingLines,
    };
    const before = scalar(state, 'before'),
      after = scalar(state, 'after');
    const comparison =
      typeof before === 'string' &&
      typeof after === 'string' &&
      small(before) &&
      small(after) &&
      bytes(before) + bytes(after) <= 8192 &&
      lines(before) + lines(after) <= 256;
    const path = scalar(state, 'path');
    const hint = typeof path === 'string' ? path.split(/[\\/]/).at(-1)?.split('.').at(-1) : null;
    const language = hint && bytes(hint) <= 128 ? hint : null;
    const operation = scalar(state, 'operation');
    const resolvedPath = state.parts.resolvedPath;
    const filePath =
      (resolvedPath && complete(resolvedPath) ? resolvedPath.filePath : null) ??
      state.parts.path?.filePath;
    const sections = Object.entries(state.parts)
      .filter(([id]) => id in labels && !(comparison && id === 'after'))
      .map(([id, part]) => {
        const page = part.page,
          end = page ? page.offset + bytes(page.text) : 0;
        const change =
          id === 'before'
            ? 'removed'
            : id === 'after'
              ? kind === 'edit'
                ? 'added'
                : 'content'
              : null;
        const source = page ? page.text : '';
        const content: DiffProps['content'] | null =
          comparison && id === 'before'
            ? { kind: 'comparison', before: before as string, after: after as string, language }
            : id === 'patch' &&
                complete(part) &&
                small(source) &&
                (operation === 'create_file' || operation === 'update_file')
              ? {
                  kind: 'patch',
                  source,
                  operation: operation === 'create_file' ? 'create' : 'update',
                  language,
                }
              : change
                ? {
                    kind: 'annotated',
                    source,
                    rows: [{ start: 0, end: bytes(source), kind: change, language }],
                  }
                : null;
        const body =
          content && true
            ? presentation.slot(
                comparison && id === 'before' ? 'changes-body' : id + '-body',
                'diff',
                {
                  content,
                  ascii: props.theme.ascii,
                  disabled: false,
                  focusOnPointer: props.focusOnPointer,
                  appearance: {
                    text: props.theme.markdown.code,
                    background: props.theme.markdown.codeBackground,
                    added: props.theme.markdown.dark ? 0x55cc88ff : 0x246c48ff,
                    removed: props.theme.markdown.dark ? 0xff6677ff : 0xbb2233ff,
                    selection: props.theme.markdown.selection,
                    terminal: props.theme.terminal,
                    dark: props.theme.markdown.dark,
                  },
                } satisfies DiffProps,
                {},
              )
            : presentation.slot(
                id + '-body',
                'source',
                {
                  source,
                  appearance: { ...props.theme.markdown.code, foreground: props.theme.muted },
                  selection: props.theme.markdown.selection,
                  terminal: props.theme.terminal
                    ? { ...props.theme.terminal, foreground: props.theme.terminal.muted }
                    : props.theme.terminal,
                  disabled: false,
                  focusOnPointer: props.focusOnPointer,
                },
                {},
              );
        return ui.box(
          id,
          [
            ui.text(
              id + '-label',
              comparison && id === 'before' ? `${labels.before} → ${labels.after}` : labels[id]!,
              props.theme.muted,
              550,
            ),
            ...(page
              ? [
                  page.offset > 0 || end < page.totalBytes
                    ? presentation.reading(
                        id + '-reading',
                        {
                          command: part.reveal ?? null,
                          content: ui.box(id + '-viewport', [body], {
                            scroll: 'manual',
                            terminal: { basis: 12 },
                          }),
                        },
                        { input: 'part-input', observed: 'part-observed' },
                      )
                    : body,
                ]
              : []),
            ...(part.pending ? [ui.text(id + '-loading', text.loading, props.theme.muted)] : []),
            ...(part.failed
              ? [
                  ui.text(
                    id + '-failed',
                    id === 'before' && kind === 'write'
                      ? text.toolOriginalUnavailable
                      : text.sourceFailed,
                    props.theme.muted,
                  ),
                ]
              : []),
            ...(page && (page.offset > 0 || end < page.totalBytes)
              ? [
                  ui.text(
                    id + '-range',
                    `${page.offset}–${end} / ${page.totalBytes} ${text.toolByteRange}`,
                    props.theme.muted,
                  ),
                ]
              : []),
            ui.box(
              id + '-controls',
              [
                ...(id === 'path' && filePath && !part.failed && complete(part)
                  ? [
                      ui.button(
                        'copy-file-path',
                        text.toolCopyPath,
                        false,
                        false,
                        { background: 0, align: 'start' },
                        filePath,
                      ),
                    ]
                  : []),
                ...(part.failed
                  ? [ui.button(id + '-retry', text.sourceRetry, !!part.pending)]
                  : []),
                ...(page && page.offset > 0
                  ? [ui.button(id + '-first', text.sourceFirst, !!part.pending)]
                  : []),
                ...(page && end < page.totalBytes
                  ? [ui.button(id + '-last', text.sourceLast, !!part.pending)]
                  : []),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ],
          { gap: 0 },
        );
      });
    const offset = scalar(state, 'offset'),
      count = scalar(state, 'returnedLines'),
      total = scalar(state, 'totalLines');
    if (typeof offset === 'number' && typeof count === 'number' && typeof total === 'number')
      sections.push(
        ui.text(
          'read-range',
          `${text.toolReadLines}: ${count ? offset + 1 : offset}–${offset + count} / ${total}`,
          props.theme.muted,
        ),
      );
    if (file && state.parts.status?.failed)
      sections.push(ui.button('status-retry', text.sourceRetry, !!state.parts.status.pending));
    const summary: N[] = [];
    if (search) {
      const pattern = state.parts.pattern;
      if (pattern?.page?.offset === 0 && !pattern.failed)
        summary.push(
          ui.text(
            'search-summary',
            `${text.toolSearchPattern} · ${pattern.page.text}${complete(pattern) ? '' : '…'}`,
            props.theme.muted,
            400,
            { truncate: true },
          ),
        );
      else if (pattern?.pending)
        summary.push(ui.text('search-summary-loading', text.loading, props.theme.muted));
      const notice =
        scalar(state, 'status') === 'failed'
          ? text.toolBusinessFailure
          : scalar(state, 'searchComplete') === false
            ? text.toolSearchIncomplete
            : null;
      if (notice) {
        summary.push(ui.text('search-summary-status', notice, props.theme.muted));
        sections.unshift(ui.text('search-status', notice, props.theme.muted));
      }
    }
    if (file) {
      const label =
        typeof path === 'string' && path.trim()
          ? compactLabel(path, 28)
          : state.parts.path?.failed
            ? text.sourceFailed
            : state.parts.path?.page
              ? text.partial
              : text.loading;
      summary.push(
        filePath && typeof path === 'string' && !state.parts.path?.failed
          ? ui.button(
              'copy-file-summary',
              `${props.theme.ascii ? '+' : '⧉'} ${label}`,
              false,
              false,
              { background: 0, align: 'start' },
              filePath,
              text.toolCopyPath,
            )
          : ui.text('file-summary', `${text.toolFilePath} · ${label}`, props.theme.muted),
      );
      if (scalar(state, 'status') === 'failed')
        summary.push(ui.text('file-summary-failed', text.toolBusinessFailure, props.theme.muted));
      for (const id of ['path', 'status']) {
        const part = state.parts[id];
        if (part?.failed)
          summary.push(
            ...(id === 'status'
              ? [ui.text(`summary-${id}-failed`, text.sourceFailed, props.theme.muted)]
              : []),
            ui.button(`summary-${id}-retry`, text.sourceRetry, !!part.pending),
          );
      }
    }
    if (process) {
      const titled = !!props.tool.call?.title;
      const commandPart = state.parts.command;
      const command =
        commandPart?.page?.offset === 0 && !commandPart.failed
          ? commandPart.page.text + (complete(commandPart) ? '' : '…')
          : scalar(state, 'reference');
      if (!titled && typeof command === 'string')
        summary.push(ui.text('process-summary', compactLabel(command, 72), props.theme.muted));
      if (
        !titled &&
        command == null &&
        (state.parts.command?.pending || state.parts.reference?.pending)
      )
        summary.push(ui.text('process-summary-loading', text.loading, props.theme.muted));
      const status = scalar(state, 'processStatus');
      const statusLabels: Record<string, string> = {
        starting: text.toolProcessStarting,
        running: text.toolProcessRunning,
        completed: text.completed,
        failed: text.toolProcessFailed,
        timed_out: text.toolProcessTimedOut,
        cancelled: text.toolProcessCancelled,
        orphaned: text.toolProcessOrphaned,
      };
      const exit = scalar(state, 'exitCode');
      const statusText =
        typeof status === 'string' && Object.hasOwn(statusLabels, status)
          ? `${statusLabels[status]}${typeof exit === 'number' ? ` · ${text.toolExitCode}: ${exit}` : ''}`
          : scalar(state, 'status') === 'failed'
            ? text.toolBusinessFailure
            : null;
      if (statusText) {
        const failed =
          ['failed', 'timed_out', 'orphaned'].includes(String(status)) ||
          (typeof exit === 'number' && exit !== 0) ||
          scalar(state, 'status') === 'failed';
        const color = failed ? props.theme.error : props.theme.muted;
        if (!titled || status !== 'completed' || exit !== 0)
          summary.push(ui.text('process-summary-status', statusText, color));
        sections.unshift(ui.text('process-status', statusText, color));
      }
      for (const id of ['stdoutTruncated', 'stderrTruncated', 'truncated', 'redacted']) {
        const part = state.parts[id];
        if (part && complete(part) && part.page!.text === 'true')
          sections.push(
            ui.text(
              id + '-notice',
              id === 'redacted' ? text.toolOutputRedacted : text.terminalTruncated,
              props.theme.muted,
            ),
          );
      }
    }
    if (process || search) {
      for (const [id, part] of Object.entries(state.parts)) {
        if (!part.failed) continue;
        const notice = (prefix: string) =>
          ui.box(
            prefix + id + '-unavailable',
            [
              ui.text(prefix + id + '-error', text.sourceFailed, props.theme.muted),
              ui.button(prefix + id + '-retry', text.sourceRetry, !!part.pending),
            ],
            { row: true, gap: 1 },
          );
        if (compactPart(id)) summary.push(notice('summary-'));
        if (!(id in labels)) sections.push(notice(''));
      }
    }
    const out = output(
      state,
      ui.box(
        'tool-details',
        [
          ui.box('tool-summary', summary, { visible: !props.expanded, gap: 0 }),
          ui.box('tool-expanded', sections, { visible: props.expanded, gap: 1 }),
        ],
        { gap: 0 },
      ),
    );
    out.actions = actions;
    let status: ToolDisplayStatus | null = null;
    if (scalar(state, 'status') === 'failed') status = 'failed';
    else if (scalar(state, 'status') === 'completed') {
      if (!process) status = 'succeeded';
      else {
        const processStatus = scalar(state, 'processStatus');
        const exit = scalar(state, 'exitCode');
        if (processStatus === 'cancelled') status = 'cancelled';
        else if (
          processStatus === 'failed' ||
          processStatus === 'timed_out' ||
          (typeof exit === 'number' && exit !== 0)
        )
          status = 'failed';
        else if (processStatus === 'completed' && exit === 0) status = 'succeeded';
        else if (processStatus === 'starting' || processStatus === 'running') status = 'running';
        else if (processStatus === 'orphaned') status = 'unknown';
      }
    }
    const context: ToolDetailsContext = {
      ...props.context,
      toolUseId: props.tool.toolUseId,
      presentation: props.tool.call?.presentation ?? null,
      call: props.tool.call?.position ?? null,
      result: props.tool.result?.position ?? null,
      path: filePath || null,
      status,
      expandable:
        !!kind ||
        Object.keys(state.parts).length === 0 ||
        Object.values(state.parts).some((part) => !complete(part) || !!part.page!.text.trim()),
    };
    if (!sameValue(state.published, context)) {
      state.published = context;
      out.events.push({ name: 'context', value: context });
    }
    return out;
  }
  return {
    mount(props) {
      return frame(
        {
          revision: 0,
          serial: 0,
          context: { ...props.context },
          tool: props.tool,
          parts: {},
          published: null,
        },
        props,
      );
    },
    update(state, props) {
      state.revision++;
      return frame(state, props);
    },
    event(state, input, props) {
      state.revision++;
      if (!sameIdentity(state, props)) return frame(state, props);
      const actions: Action[] = [];
      if (input.kind === 'component') {
        const id = Object.keys(state.parts).find(
          (id) => input.kind === 'component' && input.child === id + '-reading',
        );
        if (id) {
          const direction = scrollBoundary(state, input, id + '-reading');
          if (direction)
            input = {
              kind: 'raw',
              key: id + (direction === 'up' ? '-previous' : '-next'),
              event: {},
            };
          else if (input.name === 'input') input = input.event as typeof input;
        }
      }
      if (input.kind === 'action') {
        const entry = Object.entries(state.parts).find(
          ([, part]) => part.pending?.key === input.key,
        );
        const part = entry?.[1];
        if (part?.pending) {
          const position = part.pending.position;
          part.pending = null;
          const result = input.event as ActionOutcome;
          if (result.status !== 'succeeded') part.failed = true;
          else {
            const value = result.value as AgentHistorySourcePage;
            accept(state, part, value, position);
            if (part.page && !part.failed)
              part.reveal = {
                sequence: (part.reveal?.sequence ?? 0) + 1,
                target: {
                  kind: position.kind === 'before' || position.kind === 'tail' ? 'end' : 'start',
                },
              };
          }
        }
      } else if (input.kind === 'raw') {
        const key = input.key.startsWith('summary-') ? input.key.slice(8) : input.key;
        const split = key.lastIndexOf('-'),
          id = key.slice(0, split),
          event = key.slice(split + 1),
          part = state.parts[id];
        if (part && !part.pending && (props.expanded || (compactPart(id) && event === 'retry'))) {
          const page = part.page,
            end = page ? page.offset + bytes(page.text) : 0;
          if (event === 'retry' && part.failed) request(state, part, part.position, actions);
          else if (page && page.offset > 0 && event === 'first')
            request(state, part, { kind: 'start' }, actions);
          else if (page && page.offset > 0 && event === 'previous')
            request(state, part, { kind: 'before', offset: page.offset }, actions);
          else if (page && end < page.totalBytes && event === 'next')
            request(state, part, { kind: 'after', offset: end }, actions);
          else if (page && end < page.totalBytes && event === 'last')
            request(state, part, { kind: 'tail' }, actions);
        }
      }
      return frame(state, props, actions);
    },
  };
}
export function activate() {
  return {
    ToolDetails: { kind: 'component', contract: toolDetailsContract, component: ['component'] },
  };
}
