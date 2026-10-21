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

import { toolPresentation, webPresentation } from '../../../../plugins/agent/tool-details/presentation';
export const cardFamily = (key: string | null | undefined) => toolPresentation(key) ?? webPresentation(key);
export type ToolFamily = ReturnType<typeof cardFamily>;
export type ToolSection = { label: string; text: string; tone?: 'added' | 'removed' | 'warning' | 'error' };
export const record = (value: unknown): Record<string, unknown> | null =>
  value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;
const integer = (value: unknown): value is number => typeof value === 'number' && Number.isSafeInteger(value) && value >= 0;
const strings = (value: unknown): value is string[] => Array.isArray(value) && value.every((part) => typeof part === 'string');
export const processFamily = (kind: ToolFamily) => ['execute', 'processRead', 'stdin', 'stop'].includes(kind ?? '');

export function toolArguments(kind: ToolFamily, input: unknown): ToolSection[] | null {
  const args = record(input);
  if (!kind || !args) return null;
  if (processFamily(kind)) {
    const value = kind === 'execute' ? args.command : args.ref;
    return typeof value === 'string' ? [{ label: kind === 'execute' ? 'Command' : 'Process reference', text: value }] : null;
  }
  if (kind === 'fetch') return typeof args.url === 'string' ? [{ label: 'Requested URL', text: args.url }] : null;
  if (kind === 'webSearch' || kind === 'glob' || kind === 'grep') {
    const value = kind === 'webSearch' ? args.query : args.pattern;
    return typeof value === 'string' ? [{ label: kind === 'webSearch' ? 'Web query' : 'Search pattern', text: value }] : null;
  }
  const operation = record(args.operation);
  const path = kind === 'patch' ? operation?.path : args.path;
  if (typeof path !== 'string') return null;
  const sections: ToolSection[] = [{ label: 'File', text: path }];
  if (kind === 'write') {
    if (typeof args.content !== 'string') return null;
    sections.push({ label: 'Requested file contents', text: args.content, tone: 'added' });
  } else if (kind === 'edit') {
    if (typeof args.old_string !== 'string' || typeof args.new_string !== 'string') return null;
    sections.push({ label: 'Requested removal', text: args.old_string, tone: 'removed' },
      { label: 'Requested replacement', text: args.new_string, tone: 'added' });
  } else if (kind === 'patch') {
    if (!operation || typeof operation.type !== 'string') return null;
    sections.push({ label: 'Requested operation', text: operation.type });
    if (typeof operation.diff === 'string') sections.push({ label: 'Requested patch', text: operation.diff });
  }
  return sections;
}

// Only complete originals under the explicitly recorded display contract are
// decoded. A malformed or unfamiliar value keeps the literal original view.
export function toolResult(kind: ToolFamily, input: unknown): ToolSection[] | null {
  const result = record(input);
  if (!kind || !result) return null;
  if (result.kind === 'failed' && typeof result.message === 'string')
    return [{ label: 'Tool failed', text: result.message, tone: 'error' }];
  const value = record(result.value);
  if (result.kind !== 'completed' || !value) return null;
  if (processFamily(kind)) {
    if (!['terminal', 'shell_run'].includes(String(value.kind)) || typeof value.status !== 'string' ||
      !['starting', 'running', 'completed', 'failed', 'timed_out', 'cancelled', 'orphaned'].includes(value.status)) return null;
    const exit = typeof value.exitCode === 'number' && Number.isSafeInteger(value.exitCode) ? value.exitCode : null;
    const sections: ToolSection[] = [{ label: 'Process status', text: `${value.status}${exit === null ? '' : ` · Exit ${exit}`}`,
      tone: value.status === 'cancelled' ? 'warning' : ['failed', 'timed_out', 'orphaned'].includes(value.status) || (exit !== null && exit !== 0) ? 'error' : undefined }];
    if (typeof value.cwd === 'string') sections.push({ label: 'Working directory', text: value.cwd });
    if (typeof value.failureMessage === 'string') sections.push({ label: 'Process failure', text: value.failureMessage, tone: 'error' });
    const output = record(value.output);
    if (value.output !== undefined && value.output !== null && !output) return null;
    if (output?.mode === 'pipes') {
      if (typeof output.stdout !== 'string' || typeof output.stderr !== 'string') return null;
      sections.push({ label: 'stdout', text: output.stdout }, { label: 'stderr', text: output.stderr, tone: 'warning' });
      if (output.stdoutTruncated === true || output.stderrTruncated === true)
        sections.push({ label: 'Output notice', text: 'The recorded process output is truncated.', tone: 'warning' });
    } else if (output?.mode === 'pty') {
      if (typeof output.scrollback !== 'string' || typeof output.screen !== 'string') return null;
      sections.push({ label: 'Terminal scrollback', text: output.scrollback }, { label: 'Terminal screen', text: output.screen });
      if (output.truncated === true) sections.push({ label: 'Output notice', text: 'The recorded terminal output is truncated.', tone: 'warning' });
    } else if (output !== null) return null;
    if (output?.redacted === true) sections.push({ label: 'Privacy notice', text: 'The Host redacted parts of this recorded output.', tone: 'warning' });
    return sections;
  }
  if (kind === 'read') {
    if (typeof value.content !== 'string' || !integer(value.offset) || !integer(value.returnedLines) || !integer(value.totalLines)) return null;
    const offset = value.offset;
    const lines = value.content.split('\n');
    if (lines.at(-1) === '') lines.pop();
    return [{ label: `Read lines ${value.returnedLines ? value.offset + 1 : value.offset}–${value.offset + value.returnedLines} / ${value.totalLines}`,
      text: lines.map((line, index) => `${offset + index + 1} │ ${line}`).join('\n') },
    ...(value.next !== null && value.next !== undefined ? [{ label: 'Read notice', text: 'The tool read a range of this file; this card preserves that original range.', tone: 'warning' as const }] : [])];
  }
  if (kind === 'glob' || kind === 'grep') {
    const matches = kind === 'glob' ? value.files : value.matches;
    if (!strings(matches) || typeof value.complete !== 'boolean') return null;
    return [{ label: kind === 'glob' ? 'Matching files' : 'Matching lines', text: matches.length ? matches.join('\n') : 'No matches.' },
      ...(!value.complete ? [{ label: 'Search notice', text: 'The tool reported an incomplete search result.', tone: 'warning' as const }] : [])];
  }
  if (kind === 'write') {
    if (value.kind !== 'file_write' || typeof value.path !== 'string' || !integer(value.bytes)) return null;
    return [{ label: 'Recorded write', text: `${value.path} · ${value.bytes} bytes` },
      ...(typeof value.previousContent === 'string' ? [{ label: 'Recorded previous contents', text: value.previousContent, tone: 'removed' as const }] :
        [{ label: 'Previous contents', text: 'Not included in this original result.' }])];
  }
  if (kind === 'edit') {
    if (value.ok !== true || typeof value.path !== 'string' || !integer(value.replacements)) return null;
    return [{ label: 'Recorded edit', text: `${value.path} · ${value.replacements} replacements` },
      ...(typeof value.matchedVia === 'string' ? [{ label: 'Match method', text: value.matchedVia }] : [])];
  }
  if (kind === 'patch') return value.status === 'completed' ? [{ label: 'Recorded patch result', text: 'Operation completed. The patch shown above is the original request.' }] : null;
  if (kind === 'fetch') {
    if (typeof value.url !== 'string' || !(value.title === null || typeof value.title === 'string') ||
      !['markdown', 'text'].includes(String(value.format)) || typeof value.content !== 'string' || typeof value.truncated !== 'boolean') return null;
    return [{ label: typeof value.title === 'string' && value.title ? value.title : 'Web page', text: value.url },
      { label: 'Recorded page content', text: value.content },
      ...(value.truncated ? [{ label: 'Web notice', text: 'The extracted page content is truncated.', tone: 'warning' as const }] : [])];
  }
  if (kind === 'webSearch') {
    if (typeof value.query !== 'string' || !Array.isArray(value.rows) || typeof value.truncated !== 'boolean' || !integer(value.omittedResults)) return null;
    const sections: ToolSection[] = [];
    for (const candidate of value.rows) {
      const row = record(candidate);
      if (!row || typeof row.title !== 'string' || typeof row.url !== 'string' || typeof row.snippet !== 'string' || typeof row.truncated !== 'boolean') return null;
      sections.push({ label: row.title || 'Web result', text: `${row.url}\n${row.snippet}${row.truncated ? '\n[Snippet truncated by the Host]' : ''}` });
    }
    if (!sections.length) sections.push({ label: 'Web results', text: 'No results.' });
    if (value.truncated) sections.push({ label: 'Web notice', text: `The search result is truncated · ${value.omittedResults} omitted results.`, tone: 'warning' });
    return sections;
  }
  return null;
}
