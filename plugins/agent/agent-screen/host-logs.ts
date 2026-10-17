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
  HostLogRequest,
  HostLogPage,
} from '../../../packages/plugin-sdk/src/index.js';
import type { Theme } from '../../shared/contracts.js';
import { type Node, type Presentation } from '../../shared/view.js';
import { copy } from './locale.js';
import { intent, conversationRoute } from './actions.js';
import type { State } from './state.js';

const WINDOW_BYTES = 64 * 1024;
export interface HostLogsState {
  generation: number;
  cursor: string | null;
  bytes: number[];
  status: 'unread' | 'missing' | 'data';
  omitted: boolean;
  reset: Extract<HostLogPage, { kind: 'data' }>['reset'];
  loading: number | null;
  failed: boolean;
  watch: { key: string; input: HostLogRequest; generation: number } | null;
}
export type HostLogRead = {
  kind: 'hostLogRead';
  generation: number;
  attempt: number;
  position: HostLogRequest['position'];
};
export function newHostLogs(): HostLogsState {
  return {
    generation: 0,
    cursor: null,
    bytes: [],
    status: 'unread',
    omitted: false,
    reset: null,
    loading: null,
    failed: false,
    watch: null,
  };
}
export function hostLogsOpen(state: State): boolean {
  return (
    conversationRoute(state) &&
    state.hostLogsEnabled &&
    state.diagnostics.source === 'host' &&
    (state.inspector === 'diagnostics' || state.dialog === 'diagnostics')
  );
}
export function stopHostLogs(state: State) {
  const host = state.diagnostics.host;
  host.generation++;
  host.watch = null;
  host.loading = null;
}
export function readHostLogs(
  state: State,
  actions: Action[],
  position: HostLogRequest['position'] = { kind: 'tail' },
) {
  if (!state.hostLogsEnabled) return;
  if (position.kind === 'tail') stopHostLogs(state);
  const host = state.diagnostics.host,
    attempt = ++state.serial;
  host.loading = attempt;
  host.failed = false;
  intent(
    state,
    actions,
    { kind: 'hostLogRead', generation: host.generation, attempt, position },
    'hostLogs',
    'read',
    { position } satisfies HostLogRequest,
  );
}
function accept(host: HostLogsState, page: HostLogPage, tail: boolean): boolean {
  if (page.kind === 'missing') {
    if (tail) {
      host.bytes = [];
      host.cursor = null;
      host.omitted = false;
      host.reset = null;
    }
    host.status = 'missing';
    return true;
  }
  let bytes: number[];
  try {
    bytes = Array.from(Uint8Array.from(atob(page.bytes), (character) => character.charCodeAt(0)));
  } catch {
    return false;
  }
  if (!tail && !page.reset && page.cursor === host.cursor) {
    host.status = 'data';
    return true;
  }
  if (tail || page.reset) {
    host.bytes = [];
    host.omitted = false;
    host.reset = page.reset;
  }
  host.bytes.push(...bytes);
  host.omitted ||= page.truncated;
  if (host.bytes.length > WINDOW_BYTES) {
    host.bytes = host.bytes.slice(-WINDOW_BYTES);
    host.omitted = true;
  }
  // Invalid UTF-8 expands to three-byte replacement characters. Bound the
  // rendered SourceText as well as its raw bytes, retaining a contiguous suffix.
  const encoder = new TextEncoder();
  let renderedBytes = encoder.encode(logText(host.bytes)).length;
  while (renderedBytes > WINDOW_BYTES) {
    host.bytes = host.bytes.slice(Math.ceil((renderedBytes - WINDOW_BYTES) / 3));
    host.omitted = true;
    renderedBytes = encoder.encode(logText(host.bytes)).length;
  }
  if (host.omitted || page.reset) {
    // A bounded tail can begin inside one UTF-8 character. Keep byte cursors opaque.
    let drop = 0;
    while (drop < 3 && drop < host.bytes.length && (host.bytes[drop]! & 0xc0) === 0x80) drop++;
    if (drop) host.bytes = host.bytes.slice(drop);
  }
  host.cursor = page.cursor;
  host.status = 'data';
  return true;
}
export function hostLogResult(
  state: State,
  actions: Action[],
  request: HostLogRead,
  page: HostLogPage | null,
) {
  const host = state.diagnostics.host;
  if (request.generation !== host.generation || request.attempt !== host.loading) return;
  host.loading = null;
  host.failed = !page || !accept(host, page, request.position.kind === 'tail');
  if (page?.kind === 'data' && page.hasMore && !host.failed && hostLogsOpen(state))
    readHostLogs(state, actions, { kind: 'after', cursor: page.cursor });
}
export function hostLogUpdate(state: State, page: HostLogPage | null) {
  const host = state.diagnostics.host;
  if (!hostLogsOpen(state) || !host.watch || host.watch.generation !== host.generation) return;
  if (!page || !accept(host, page, false)) {
    stopHostLogs(state);
    host.failed = true;
  } else host.failed = false;
}
export function hostLogEvent(state: State, actions: Action[], key: string): boolean {
  if (!key.startsWith('host-log-')) return false;
  if (!hostLogsOpen(state)) return true;
  const host = state.diagnostics.host;
  if (key === 'host-log-refresh') readHostLogs(state, actions);
  else if (key === 'host-log-stop') stopHostLogs(state);
  else if (key === 'host-log-follow' && host.loading === null && !host.watch) {
    host.failed = false;
    host.watch = {
      key: 'host-log-watch-' + ++state.serial,
      input: { position: host.cursor ? { kind: 'after', cursor: host.cursor } : { kind: 'tail' } },
      generation: host.generation,
    };
  }
  return true;
}
function logText(bytes: number[]): string {
  return new TextDecoder().decode(new Uint8Array(bytes), { stream: true });
}
export function hostLogsView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
): N {
  const ui = presentation.paint(theme),
    text = copy(locale),
    host = state.diagnostics.host;
  const contents = logText(host.bytes);
  return ui.box(
    'host-logs',
    [
      ui.text('host-log-scope', text.hostLogsLabel, theme.text, 550),
      ...(host.failed
        ? [ui.text('host-log-error', text.recoveryRefreshFailed, theme.warning)]
        : []),
      ...(host.omitted ? [ui.text('host-log-omitted', text.hostLogsOmitted, theme.muted)] : []),
      ...(host.reset
        ? [
            ui.text(
              'host-log-reset',
              host.reset === 'replaced' ? text.hostLogsReplaced : text.hostLogsTruncated,
              theme.warning,
            ),
          ]
        : []),
      ...(host.status === 'missing'
        ? [ui.text('host-log-missing', text.hostLogsMissing, theme.muted)]
        : []),
      ...(host.status === 'unread' && host.loading !== null
        ? [ui.text('host-log-loading', text.loading, theme.muted)]
        : []),
      presentation.reading(
        'host-log-reading',
        {
          command: null,
          content: ui.box(
            'host-log-content',
            [
              presentation.slot(
                'host-log-text',
                'source',
                {
                  source: contents,
                  appearance: theme.markdown.code,
                  selection: theme.markdown.selection,
                  terminal: theme.terminal,
                  disabled: false,
                  focusOnPointer: true,
                },
                {},
              ),
            ],
            { scroll: host.watch ? 'follow_end' : 'manual', terminal: { grow: 1 } },
          ),
        },
        {},
      ),
      ui.box(
        'host-log-controls',
        [
          ui.button('host-log-refresh', text.check, host.loading !== null),
          host.watch
            ? ui.button('host-log-stop', text.stop)
            : ui.button('host-log-follow', text.hostLogsFollow, host.loading !== null),
        ],
        { row: true, gap: 1 },
      ),
    ],
    { gap: 1, terminal: { grow: 1 } },
  );
}
