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

import { editorMenu } from '../../shared/editor-menu.js';
import type {
  Action,
  ActionOutcome,
  CallerIntent,
  Component,
  ComponentInput,
  ReadingCommand,
  Emission,
  OperationReply,
  RecoveryRef,
  ResourceUpdate,
  TerminalPage,
  TerminalTarget,
  TerminalLeaseUpdate,
  TerminalControlReceipt,
  TerminalStopped,
  TerminalReleased,
  TerminalViewObservation,
  TerminalInputAcknowledgement,
  TerminalViewProps,
  EditorObservation,
  InputProps,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { terminalPanelContract, type TerminalPanelProps } from '../../shared/contracts.js';
import { scrollBoundary, scrollRows, visibleRows, resetScroll } from '../../shared/scroll.js';
import { copy, type Copy } from './locale.js';
import { panelHeader } from '../../shared/page-shell.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';

type Result<T> = { kind: 'completed'; value: T } | { kind: 'failed'; message: string };
type Current = {
  kind: 'start' | 'stop' | 'control' | 'release';
  session: string;
  target: TerminalTarget | null;
  controller: string | null;
  sequence: number | null;
  acknowledgement: TerminalInputAcknowledgement | null;
  epoch: number;
  reference: RecoveryRef | null;
};
type Request =
  | { kind: 'mutate' }
  | { kind: 'receipt'; reference: RecoveryRef }
  | { kind: 'command'; epoch: number; draft: EditorObservation }
  | { kind: 'history'; after: number }
  | { kind: 'earlier'; cursor: number; reference: RecoveryRef };
interface State {
  revision: number;
  serial: number;
  epoch: number;
  session: string | null;
  open: boolean;
  page: TerminalPage | null;
  pageOffset: number | null;
  catalogCommand: ReadingCommand | null;
  catalogLanding: 'start' | 'end' | null;
  rowKeys: Record<string, string>;
  offset: number;
  previous: number[];
  catalog: string | null;
  watch: string | null;
  acquire: string | null;
  selected: string | null;
  resource: ResourceUpdate | null;
  lease: { id: string; next: number } | null;
  observation: TerminalViewObservation | null;
  acknowledgement: TerminalInputAcknowledgement | null;
  pending: Record<string, Request>;
  current: Current | null;
  notice: keyof Copy | null;
  confirmation: 'stop' | 'forget' | null;
  commandOpen: boolean;
  draft: EditorObservation | null;
  historyOpen: boolean;
  earlier: { cursor: number; reference: RecoveryRef; reply: OperationReply | null }[];
  historyNext: number | null;
}
// The service returns a byte-bounded page of at most 64 records. Leave native
// scene input space for controls; each next offset still queries the service.
const catalogRows = 16;
function request(
  state: State,
  actions: Action[],
  r: Request,
  binding: string,
  method: string,
  input: unknown,
  resources: Action['resources'] = {},
) {
  const key = `terminal-${++state.serial}`;
  state.pending[key] = r;
  actions.push({ key, binding, member: 0, method, input, resources });
}
function active(state: State, props: TerminalPanelProps) {
  return props.open && props.enabled && state.open && state.session === props.session?.id;
}
function writable(state: State, props: TerminalPanelProps) {
  return (
    active(state, props) &&
    !props.busy &&
    !props.session?.archived &&
    !state.current &&
    !state.historyOpen &&
    !Object.values(state.pending).some((r) => r.kind === 'command')
  );
}
function running(resource: ResourceUpdate | null) {
  return resource && ['starting', 'running'].includes(resource.result.status);
}
function target(state: State): TerminalTarget | null {
  return state.session && state.selected ? { sessionId: state.session, ref: state.selected } : null;
}
function disconnect(state: State) {
  state.acquire = null;
  state.lease = null;
  state.observation = null;
  state.acknowledgement = null;
}
function catalog(state: State) {
  state.catalog = `terminal-list-${++state.serial}`;
}
function select(state: State, reference: string | null) {
  disconnect(state);
  state.epoch++;
  state.selected = reference;
  state.resource = null;
  state.watch = reference ? `terminal-watch-${++state.serial}` : null;
  state.confirmation = null;
}
function reconcile(state: State, props: TerminalPanelProps) {
  const open = props.open && props.enabled && !!props.session;
  const session = props.session?.id ?? null;
  if (state.open !== open || state.session !== session) {
    select(state, null);
    state.session = session;
    state.open = open;
    state.page = null;
    state.rowKeys = {};
    state.offset = 0;
    state.previous = [];
    state.catalog = null;
    state.commandOpen = false;
    state.historyOpen = false;
    if (open) catalog(state);
  }
  if (!open || props.busy || props.session?.archived) disconnect(state);
}
function observe(state: State, resource: ResourceUpdate) {
  if (resource.sessionId !== state.session || resource.result.ref !== state.selected) return false;
  if (!state.resource || resource.result.revision >= state.resource.result.revision)
    state.resource = resource;
  if (!running(state.resource) || state.resource?.ownership.kind !== 'local') disconnect(state);
  return true;
}
function mutate(
  state: State,
  actions: Action[],
  kind: Current['kind'],
  input: unknown,
  acknowledgement: TerminalInputAcknowledgement | null = null,
) {
  if (state.current || !state.session) return;
  state.current = {
    kind,
    session: state.session,
    target: target(state),
    controller: state.lease?.id ?? null,
    sequence: kind === 'control' ? state.lease!.next : null,
    acknowledgement,
    epoch: state.epoch,
    reference: null,
  };
  request(state, actions, { kind: 'mutate' }, 'terminal', kind, input);
  state.notice = null;
}
function settle(state: State, props: TerminalPanelProps, reply: OperationReply) {
  const current = state.current;
  if (!current || (current.reference && !sameValue(current.reference, reply.reference))) return;
  if (
    reply.reference.contract.name !== 'maka.workspace.Terminal' ||
    reply.reference.method !== current.kind
  )
    return;
  current.reference = reply.reference;
  if (reply.receipt.kind !== 'recorded' || reply.receipt.outcome.kind !== 'returned') {
    state.notice = 'terminalUnknown';
    disconnect(state);
    return;
  }
  const result = reply.receipt.outcome.value as Result<
    ResourceUpdate | TerminalControlReceipt | TerminalStopped | TerminalReleased
  >;
  const same =
    current.epoch === state.epoch && current.session === state.session && active(state, props);
  if (result.kind === 'completed') {
    if (current.kind === 'start') {
      const resource = result.value as ResourceUpdate;
      if (resource.sessionId !== current.session || !resource.result?.ref) return;
      if (same) {
        select(state, resource.result.ref);
        state.commandOpen = false;
        observe(state, resource);
        catalog(state);
      }
    } else if (current.kind === 'control') {
      const receipt = result.value as TerminalControlReceipt;
      if (receipt.controllerId !== current.controller || receipt.sequence !== current.sequence)
        return;
      if (same && state.lease?.id === current.controller && current.acknowledgement) {
        state.lease.next = receipt.sequence + 1;
        state.acknowledgement = current.acknowledgement;
      }
    } else if (current.kind === 'stop') {
      const receipt = result.value as TerminalStopped;
      if (
        receipt.resource?.sessionId !== current.session ||
        receipt.resource.result.ref !== current.target?.ref
      )
        return;
      if (same) observe(state, receipt.resource);
    } else {
      const receipt = result.value as TerminalReleased;
      if (receipt.controllerId !== current.controller) return;
    }
    state.notice = reply.receipt.cleanup.kind === 'confirmed' ? null : 'terminalCleanupUnknown';
    if (reply.receipt.cleanup.kind !== 'confirmed') disconnect(state);
  } else if (result.kind === 'failed') {
    state.notice = 'terminalFailed';
    if (same && current.kind === 'control') disconnect(state);
  } else return;
  state.current = null;
}
function history(state: State, actions: Action[], after: number) {
  if (!after) resetScroll(state, 'terminal-history-reading');
  if (Object.values(state.pending).some((r) => r.kind === 'history' || r.kind === 'earlier'))
    return;
  request(state, actions, { kind: 'history', after }, 'history', 'page', { after, limit: 4 });
}
function runCommand(state: State, props: TerminalPanelProps, actions: Action[]) {
  const draft = state.draft;
  if (
    !writable(state, props) ||
    !state.commandOpen ||
    !draft ||
    draft.snapshot.marked ||
    !draft.snapshot.text.bytes ||
    draft.snapshot.text.bytes > 8192
  )
    return;
  request(
    state,
    actions,
    { kind: 'command', epoch: state.epoch, draft },
    'reader',
    'read',
    { start: 0, end: draft.snapshot.text.bytes },
    {
      text: {
        child: 'terminal-command',
        method: 'snapshot',
        input: {
          kind: 'at_revision',
          owner: draft.snapshot.text.owner,
          revision: draft.snapshot.text.revision,
        },
      },
    },
  );
}
function subscription(state: State, props: TerminalPanelProps, key: string, result: ActionOutcome) {
  if (!active(state, props)) return;
  const value = result.status === 'updated' ? (result.value as Result<unknown>) : null;
  if (key === state.catalog) {
    if (value?.kind === 'completed') {
      const received = value.value as TerminalPage;
      const page: TerminalPage = {
        ...received,
        resources: received.resources.slice(0, catalogRows),
        nextOffset:
          received.resources.length > catalogRows
            ? state.offset + catalogRows
            : received.nextOffset,
      };
      if (!page.resources.every((r) => r.sessionId === state.session)) return;
      // Resource references are opaque and can exceed native scene key limits.
      // Retain local row identity across catalogue updates, bounded by this page.
      state.rowKeys = Object.fromEntries(
        page.resources.map((r) => [
          r.result.ref,
          Object.hasOwn(state.rowKeys, r.result.ref)
            ? state.rowKeys[r.result.ref]
            : `task-${++state.serial}`,
        ]),
      );
      state.page = page;
      state.pageOffset = state.offset;
      if (state.catalogLanding) {
        state.catalogCommand = {
          sequence: (state.catalogCommand?.sequence ?? 0) + 1,
          target: { kind: state.catalogLanding },
        };
        state.catalogLanding = null;
      }
      if (!state.selected && page.resources.length) select(state, page.resources[0].result.ref);
    } else {
      state.catalog = null;
      state.notice = 'terminalFailed';
    }
  } else if (key === state.watch) {
    if (value?.kind === 'completed') observe(state, value.value as ResourceUpdate);
    else {
      state.watch = null;
      disconnect(state);
      state.notice = 'terminalFailed';
    }
  } else if (key === state.acquire) {
    if (value?.kind !== 'completed') {
      disconnect(state);
      state.notice = 'terminalFailed';
      return;
    }
    const lease = value.value as TerminalLeaseUpdate;
    if (!observe(state, lease.resource)) return;
    if (!state.acquire || !lease.held || lease.nextSequence == null || !running(state.resource)) {
      disconnect(state);
      return;
    }
    if (state.lease && state.lease.id !== lease.controllerId) {
      disconnect(state);
      return;
    }
    // Only the acquire's first successful update initializes this counter.
    // Later output notifications cannot rewind a locally settled Host receipt.
    state.lease ??= { id: lease.controllerId, next: lease.nextSequence };
  }
}
function frame<N extends Node>(
  p: Presentation<N>,
  state: State,
  props: TerminalPanelProps,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  const ui = p.paint(props.theme),
    text = copy(props.locale);
  const ready = writable(state, props),
    resource = state.resource,
    selected = target(state);
  const live = running(resource) && resource?.ownership.kind === 'local';
  const canonical = resource?.result.output;
  const copyText =
    canonical && !canonical.redacted
      ? canonical.mode === 'pty'
        ? canonical.screen
        : [canonical.stdout, canonical.stderr].filter(Boolean).join('\n')
      : '';
  const viewSource = `${state.session ?? ''}:${state.selected ?? ''}`;
  const screen = p.slot(
    'terminal-screen',
    'view',
    {
      source: viewSource || 'empty',
      accessibilityLabel: text.terminalTitle,
      output: resource?.result.output ?? null,
      controller: active(state, props) && state.lease ? state.lease.id : null,
      acknowledgement: state.acknowledgement,
      appearance: {
        font: { family: 'monospace', fallbacks: [], size: 13, lineHeight: 1.2, weight: 400 },
        foreground: props.theme.text,
      },
      background: props.theme.background,
      caret: props.theme.accent,
      rows: 12,
      tabIndex: 0,
    } satisfies TerminalViewProps,
    { observed: 'observed' },
  );
  const command = p.slot(
    'terminal-command',
    'input',
    {
      menu: editorMenu(props.theme, props.locale),
      appearance: props.theme.input,
      accessibilityLabel: text.terminalCommandLabel,
      placeholder: text.terminalCommandLabel,
      autoFocus: true,
      readOnly: !ready,
      disabled: !props.open || !state.commandOpen,
      tabIndex: 0,
      command: null,
    } satisfies InputProps,
    { changed: 'draft', submitted: 'run' },
  );
  const out = output(
    state,
    ui.box(
      'terminal-panel',
      [
        panelHeader(
          p,
          props,
          'terminal-heading',
          state.historyOpen ? text.terminalHistory : text.terminalTitle,
          'close',
          state.confirmation
            ? 'cancel'
            : state.historyOpen
              ? 'history-back'
              : state.commandOpen
                ? 'command-back'
                : undefined,
        ),
        ui.box(
          'terminal-toolbar',
          [
            ui.button('new', text.terminalNew, !ready),
            ui.button('command', text.terminalCommand, !ready),
            ui.button(
              'refresh',
              props.theme.ascii ? 'r' : '↻',
              !active(state, props),
              false,
              { background: 0 },
              undefined,
              text.refreshSessions,
            ),
            ui.button('history', text.terminalHistory, false, false, {
              background: 0,
              align: 'start',
            }),
          ],
          {
            visible: !state.historyOpen && !state.confirmation,
            row: true,
            gap: 1,
            terminal: { wrap: true },
          },
        ),
        ...(!props.enabled ? [ui.text('terminal-unavailable', text.terminalUnavailable)] : []),
        ...(state.notice
          ? [ui.text('terminal-notice', text[state.notice], props.theme.muted)]
          : []),
        ...(state.current && !Object.values(state.pending).some((r) => r.kind === 'mutate')
          ? [
              ui.text(
                'terminal-original',
                `${text.terminalOriginalSession} ${compactLabel(state.current.session, 48)}`,
                props.theme.muted,
              ),
              ui.box(
                'terminal-recovery',
                [
                  ui.button(
                    'check',
                    text.check,
                    !state.current.reference ||
                      Object.values(state.pending).some(
                        (r) => r.kind === 'receipt' || r.kind === 'mutate',
                      ),
                  ),
                  ui.button(
                    'forget',
                    text.terminalForget,
                    Object.values(state.pending).some((r) => r.kind === 'mutate'),
                  ),
                ],
                { row: true, gap: 1 },
              ),
            ]
          : []),
        ui.box(
          'terminal-confirmation',
          [
            ui.text(
              'terminal-confirm-text',
              state.confirmation === 'stop' ? text.terminalStopConfirm : text.terminalForgetConfirm,
            ),
            ui.button('confirm', state.confirmation === 'stop' ? text.stop : text.terminalForget),
          ],
          { visible: !!state.confirmation, gap: 1 },
        ),
        ui.box(
          'terminal-command-form',
          [
            command,
            ui.button('run', text.terminalRun, !ready || !state.draft?.snapshot.text.bytes),
          ],
          { visible: state.commandOpen && !state.historyOpen, gap: 1 },
        ),
        ui.box(
          'terminal-main',
          [
            p.reading(
              'terminal-catalog-reading',
              {
                command: state.catalogCommand,
                content: ui.box(
                  'terminal-catalog',
                  [
                    ...(state.page?.resources.map((r) =>
                      ui.button(
                        state.rowKeys[r.result.ref],
                        `${r.result.ref === state.selected ? '● ' : ''}${compactLabel(r.result.cmd || r.result.ref, 40)} · ${r.result.status}`,
                        !!state.confirmation,
                        false,
                        { align: 'start' },
                      ),
                    ) ?? [ui.text('terminal-loading', text.terminalLoading)]),
                    ...(state.page?.total === 0
                      ? [ui.text('terminal-empty', text.terminalEmpty)]
                      : []),
                  ],
                  {
                    scroll: 'manual',
                    gap: 1,
                    terminal: {
                      basis: Math.max(1, Math.min(3, state.page?.resources.length ?? 1)),
                    },
                  },
                ),
              },
              { observed: 'catalog-observed', input: 'catalog-input' },
            ),
            ...(resource
              ? [
                  ui.text(
                    'terminal-cwd',
                    compactLabel(resource.result.cwd, 60),
                    props.theme.muted,
                    400,
                    { truncate: false },
                  ),
                  ui.text(
                    'terminal-detail',
                    compactLabel(resource.result.cmd || resource.result.ref, 60),
                    props.theme.text,
                    400,
                    { truncate: false },
                  ),
                  ui.text(
                    'terminal-status',
                    `${resource.result.status}${resource.result.exitCode != null ? ` (${resource.result.exitCode})` : ''}`,
                  ),
                  ...(resource.ownership.kind !== 'local'
                    ? [ui.text('terminal-owner', text.terminalSourceOwned)]
                    : []),
                ]
              : [ui.text('terminal-select', text.terminalSelect)]),
            ui.box(
              'terminal-controls',
              [
                ui.button(
                  'connect',
                  state.lease
                    ? text.terminalControlled
                    : state.acquire
                      ? text.terminalConnecting
                      : text.terminalConnect,
                  !ready || !live || resource?.result.mode !== 'pty' || !!state.acquire,
                ),
                ui.button('release', text.terminalRelease, !state.acquire),
                ui.button(
                  'reconnect',
                  text.terminalReconnect,
                  !ready || !live || resource?.result.mode !== 'pty',
                ),
                ui.button('stop', text.stop, !ready || !live),
                ui.button('copy', text.copy, !copyText, false, {}, copyText || undefined),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
            ui.box('terminal-pty', [screen], {
              visible: resource?.result.mode === 'pty',
              gap: 0,
              terminal: { grow: 1, basis: 0 },
            }),
            p.reading(
              'terminal-pipes-reading',
              {
                content: ui.box(
                  'terminal-pipes',
                  resource?.result.output?.mode === 'pipes' && !resource.result.output.redacted
                    ? [
                        ui.text('terminal-stdout', resource.result.output.stdout),
                        ui.text('terminal-stderr', resource.result.output.stderr),
                      ]
                    : [],
                  {
                    visible: resource?.result.mode === 'pipes',
                    scroll: 'manual',
                    terminal: { basis: 8 },
                  },
                ),
              },
              {},
            ),
            ui.text(
              'terminal-help',
              state.lease ? text.terminalControlled + ' · ' + text.terminalHelp : text.terminalHelp,
              props.theme.muted,
            ),
            ...(resource?.result.output &&
            (resource.result.output.mode === 'pty'
              ? resource.result.output.truncated
              : resource.result.output.stdoutTruncated || resource.result.output.stderrTruncated)
              ? [ui.text('terminal-truncated', text.terminalTruncated, props.theme.muted)]
              : []),
            ...(state.observation?.error
              ? [
                  ui.text(
                    'terminal-input-error',
                    state.observation.error === 'input_full'
                      ? text.terminalInputFull
                      : text.terminalInputFailed,
                  ),
                ]
              : []),
          ],
          {
            visible: props.enabled && !state.historyOpen && !state.confirmation,
            gap: 1,
            terminal: { grow: 1 },
          },
        ),
        ...(state.historyOpen
          ? [
              p.reading(
                'terminal-history-reading',
                {
                  content: ui.box(
                    'terminal-history',
                    [
                      ...visibleRows(state, 'terminal-history-reading', state.earlier).map(
                        ({ entry }) =>
                          ui.box(`terminal-operation-${entry.cursor}`, [
                            ui.text(
                              `terminal-operation-state-${entry.cursor}`,
                              `${entry.reference.method} · ${
                                entry.reply?.receipt.kind === 'recorded' &&
                                entry.reply.receipt.outcome.kind === 'returned'
                                  ? (entry.reply.receipt.outcome.value as Result<unknown>).kind ===
                                    'failed'
                                    ? text.terminalFailed
                                    : entry.reply.receipt.cleanup.kind !== 'confirmed'
                                      ? text.terminalCleanupUnknown
                                      : text.terminalRecorded
                                  : text.terminalUnknown
                              }`,
                            ),
                            ui.button(
                              `earlier:${entry.cursor}`,
                              text.check,
                              Object.values(state.pending).some(
                                (r) => r.kind === 'earlier' && r.cursor === entry.cursor,
                              ),
                            ),
                          ]),
                      ),
                      ui.button(
                        'history-refresh',
                        props.theme.ascii ? 'r' : '↻',
                        false,
                        false,
                        { background: 0 },
                        undefined,
                        text.refreshDefaultOperations,
                      ),
                    ],
                    { gap: 1, scroll: 'anchor', terminal: { grow: 1 } },
                  ),
                },
                { input: 'history-input', observed: 'history-observed' },
              ),
            ]
          : []),
      ],
      {
        visible: props.open,
        gap: 1,
        terminal: { grow: 1 },
        shortcuts: [
          {
            key: 'escape',
            modifiers: [],
            input: state.confirmation
              ? 'cancel'
              : state.historyOpen
                ? 'history-back'
                : state.commandOpen
                  ? 'command-back'
                  : 'close',
            enabled: true,
          },
        ],
      },
    ),
    events,
  );
  out.actions = actions;
  const sub = (key: string, method: string, input: unknown) => ({
    key,
    binding: 'terminal',
    member: 0,
    method,
    input,
    resources: {},
  });
  out.subscriptions = active(state, props)
    ? [
        ...(state.catalog
          ? [sub(state.catalog, 'watchList', { sessionId: state.session, offset: state.offset })]
          : []),
        ...(state.watch && selected ? [sub(state.watch, 'watch', selected)] : []),
        ...(state.acquire && selected ? [sub(state.acquire, 'acquire', selected)] : []),
      ]
    : [];
  return out;
}

export function terminalPanel<N extends Node>(
  p: Presentation<N>,
): Component<State, TerminalPanelProps, N> {
  return {
    mount(props) {
      const state: State = {
        revision: 0,
        serial: 0,
        epoch: 0,
        session: null,
        open: false,
        page: null,
        pageOffset: null,
        catalogCommand: null,
        catalogLanding: null,
        rowKeys: {},
        offset: 0,
        previous: [],
        catalog: null,
        watch: null,
        acquire: null,
        selected: null,
        resource: null,
        lease: null,
        observation: null,
        acknowledgement: null,
        pending: {},
        current: null,
        notice: null,
        confirmation: null,
        commandOpen: false,
        draft: null,
        historyOpen: false,
        earlier: [],
        historyNext: null,
      };
      reconcile(state, props);
      return frame(p, state, props);
    },
    update(state, props) {
      state.revision++;
      reconcile(state, props);
      return frame(p, state, props);
    },
    event(state, input, props) {
      state.revision++;
      reconcile(state, props);
      if (
        input.kind === 'component' &&
        ['terminal-catalog-reading', 'terminal-history-reading'].includes(input.child) &&
        input.name === 'input'
      )
        input = input.event as ComponentInput;
      const direction = scrollBoundary(state, input, 'terminal-catalog-reading');
      if (direction && state.pageOffset === state.offset)
        input = { kind: 'raw', key: direction === 'up' ? 'previous' : 'next', event: {} };
      if (
        state.historyOpen &&
        scrollRows(state, input, 'terminal-history-reading', state.earlier.length)
      )
        input = { kind: 'raw', key: 'history-next', event: {} };
      const actions: Action[] = [],
        events: Emission[] = [];
      if (input.kind === 'action') {
        const result = input.event as ActionOutcome;
        if ([state.catalog, state.watch, state.acquire].includes(input.key))
          subscription(state, props, input.key, result);
        else {
          const r = state.pending[input.key];
          if (!r) return frame(p, state, props);
          delete state.pending[input.key];
          if (r.kind === 'mutate') {
            if (result.status === 'operation') settle(state, props, result.reply);
            else if (result.status === 'not_accepted') {
              state.current = null;
              disconnect(state);
              state.notice = 'terminalFailed';
            } else {
              disconnect(state);
              state.notice = 'terminalUnknown';
            }
          } else if (r.kind === 'receipt') {
            if (
              state.current?.reference &&
              sameValue(state.current.reference, r.reference) &&
              result.status === 'succeeded' &&
              sameValue((result.value as OperationReply).reference, r.reference)
            )
              settle(state, props, result.value as OperationReply);
            else state.notice = 'terminalUnknown';
          } else if (r.kind === 'command') {
            const chunk = result.status === 'succeeded' ? (result.value as TextChunk) : null;
            if (
              r.epoch === state.epoch &&
              writable(state, props) &&
              state.commandOpen &&
              chunk &&
              sameValue(chunk.metadata, r.draft.snapshot.text) &&
              new TextEncoder().encode(chunk.text).length === r.draft.snapshot.text.bytes
            )
              mutate(state, actions, 'start', { sessionId: state.session, command: chunk.text });
          } else if (r.kind === 'history') {
            if (!active(state, props) || !state.historyOpen) return frame(p, state, props);
            if (result.status === 'succeeded') {
              const entries = result.value as CallerIntent[];
              state.historyNext = entries.length === 4 ? entries[entries.length - 1].cursor : null;
              state.earlier = [
                ...(r.after ? state.earlier : []),
                ...entries
                  .filter((e) => e.reference.contract.name === 'maka.workspace.Terminal')
                  .map((e) => ({ cursor: e.cursor, reference: e.reference, reply: null })),
              ];
              for (const e of state.earlier.filter((e) => e.cursor > r.after && !e.reply))
                request(
                  state,
                  actions,
                  { kind: 'earlier', cursor: e.cursor, reference: e.reference },
                  'receipts',
                  'read',
                  e.reference,
                );
            } else state.notice = 'historyUnavailable';
          } else if (r.kind === 'earlier') {
            const e = state.earlier.find(
              (e) => e.cursor === r.cursor && sameValue(e.reference, r.reference),
            );
            if (
              e &&
              result.status === 'succeeded' &&
              sameValue((result.value as OperationReply).reference, r.reference)
            )
              e.reply = result.value as OperationReply;
          }
        }
      } else if (input.kind === 'component') {
        if (input.child === 'terminal-command') {
          if (input.name === 'changed') state.draft = input.event as EditorObservation;
          if (input.name === 'submitted') runCommand(state, props, actions);
        } else if (input.child === 'terminal-screen' && input.name === 'observed') {
          const o = input.event as TerminalViewObservation;
          if (
            state.lease &&
            o.source === `${state.session ?? ''}:${state.selected ?? ''}` &&
            o.controller === state.lease?.id &&
            active(state, props) &&
            (!state.observation ||
              o.owner !== state.observation.owner ||
              o.revision > state.observation.revision)
          ) {
            state.observation = o;
            const b = o.batch,
              ack = state.acknowledgement;
            if (
              b &&
              b.controller === state.lease.id &&
              writable(state, props) &&
              !(
                ack &&
                ack.owner === o.owner &&
                ack.controller === b.controller &&
                ack.sequence >= b.sequence
              )
            ) {
              mutate(
                state,
                actions,
                'control',
                {
                  controller: { target: target(state), controllerId: state.lease.id },
                  sequence: state.lease.next,
                  actions: b.actions,
                  size: b.size ?? null,
                },
                { owner: o.owner, controller: b.controller, sequence: b.sequence, accepted: true },
              );
            }
          }
        }
      } else if (input.kind === 'raw' && props.open) {
        const key = input.key;
        if (key === 'close') {
          disconnect(state);
          events.push({ name: 'closed', value: {} });
        } else if (key === 'command-back') state.commandOpen = false;
        else if (key === 'cancel') state.confirmation = null;
        else if (
          key === 'check' &&
          state.current?.reference &&
          !Object.values(state.pending).some((r) => r.kind === 'receipt' || r.kind === 'mutate')
        )
          request(
            state,
            actions,
            { kind: 'receipt', reference: state.current.reference },
            'receipts',
            'read',
            state.current.reference,
          );
        else if (
          key === 'forget' &&
          state.current &&
          !Object.values(state.pending).some((r) => r.kind === 'mutate')
        ) {
          disconnect(state);
          state.confirmation = 'forget';
        } else if (key === 'confirm' && state.confirmation === 'forget') {
          state.current = null;
          state.confirmation = null;
        } else if (key === 'history') {
          disconnect(state);
          state.historyOpen = true;
          history(state, actions, 0);
        } else if (key === 'history-back') state.historyOpen = false;
        else if (key === 'history-refresh' && state.historyOpen) history(state, actions, 0);
        else if (key === 'history-next' && state.historyOpen && state.historyNext !== null)
          history(state, actions, state.historyNext);
        else if (key.startsWith('earlier:') && state.historyOpen) {
          const e = state.earlier.find((e) => String(e.cursor) === key.slice(8));
          if (
            e &&
            !Object.values(state.pending).some((r) => r.kind === 'earlier' && r.cursor === e.cursor)
          )
            request(
              state,
              actions,
              { kind: 'earlier', cursor: e.cursor, reference: e.reference },
              'receipts',
              'read',
              e.reference,
            );
        } else if (key === 'release' && state.acquire) {
          if (!state.current && state.lease)
            mutate(state, actions, 'release', {
              target: target(state),
              controllerId: state.lease.id,
            });
          disconnect(state);
        } else if (active(state, props) && !state.confirmation && !state.historyOpen) {
          if (key === 'refresh') {
            catalog(state);
            if (state.selected) state.watch = `terminal-watch-${++state.serial}`;
          } else if (state.page?.resources.some((r) => state.rowKeys[r.result.ref] === key))
            select(
              state,
              state.page.resources.find((r) => state.rowKeys[r.result.ref] === key)!.result.ref,
            );
          else if (key === 'previous' && state.previous.length) {
            state.catalogLanding = 'end';
            state.offset = state.previous.pop()!;
            catalog(state);
          } else if (key === 'next' && state.page?.nextOffset != null) {
            state.catalogLanding = 'start';
            state.previous.push(state.offset);
            state.offset = state.page.nextOffset;
            catalog(state);
          } else if (key === 'new' && writable(state, props))
            mutate(state, actions, 'start', { sessionId: state.session });
          else if (key === 'command' && writable(state, props))
            state.commandOpen = !state.commandOpen;
          else if (key === 'run') runCommand(state, props, actions);
          else if (
            key === 'stop' &&
            writable(state, props) &&
            running(state.resource) &&
            state.resource?.ownership.kind === 'local'
          ) {
            disconnect(state);
            state.confirmation = 'stop';
          } else if (
            (key === 'connect' || key === 'reconnect') &&
            writable(state, props) &&
            running(state.resource) &&
            state.resource?.ownership.kind === 'local' &&
            state.resource.result.mode === 'pty'
          ) {
            disconnect(state);
            state.acquire = `terminal-acquire-${++state.serial}`;
          }
        } else if (
          key === 'confirm' &&
          state.confirmation === 'stop' &&
          writable(state, props) &&
          running(state.resource) &&
          target(state)
        ) {
          mutate(state, actions, 'stop', target(state));
          state.confirmation = null;
        }
      }
      return frame(p, state, props, actions, events);
    },
  };
}
export function activate() {
  return {
    TerminalPanel: { kind: 'component', contract: terminalPanelContract, component: ['component'] },
  };
}
