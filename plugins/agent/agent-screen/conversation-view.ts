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

import type { Action, RowsProps } from '../../../packages/plugin-sdk/src/index.js';
import type { Theme, TimelineProps } from '../../shared/contracts.js';
import { copy, tuiText } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { messageOriginCaption } from '../../shared/message-view.js';
import { type Node, type Presentation } from '../../shared/view.js';

import { attachmentFiles, attachmentSummary, pasteView } from '../../shared/attachments.js';

import { draftReady } from '../../shared/drafts.js';

import {
  canExecute,
  composerCommands,
  reviewTarget,
  selectedBackend,
  catalogPending,
  conversationRoute,
  executionPending,
  mutationPending,
} from './actions.js';
import { sandboxLabel } from './sandbox.js';
import { composerStatus } from './composer-status.js';
import { headerView } from './header-view.js';
import { messages, turnTimings, utf8Bytes, activeOutput } from './messages.js';
import { promptAnchor } from './actions.js';
import { readingKey, type State } from './state.js';

function readingProps(state: State, theme: Theme, locale: string): TimelineProps {
  const timelineMessages = messages(state);
  const prompt = state.visits.find((entry) => entry.id === state.selected?.id)?.prompt;
  return {
    theme,
    locale,
    messages: timelineMessages,
    activeOutput: activeOutput(state),
    leadingPrompt:
      state.window && prompt?.anchor === promptAnchor(state.window.page) ? prompt.value : null,
    timings: turnTimings(state, timelineMessages),
    copyRequest: !state.image ? state.copyRequest : null,
    browse: state.browse,
    markReadEnabled:
      !!state.selected?.hasUnread &&
      !state.selected.archived &&
      !mutationPending(state) &&
      conversationRoute(state) &&
      state.dialog === null &&
      state.inspector === null &&
      !state.source &&
      !state.image,
    search:
      state.searchOpen && state.selected
        ? {
            sessionId: state.selected.id,
            epoch:
              state.visits.find((visit) => visit.id === state.selected?.id)?.epoch ?? state.epoch,
            through: state.latestKnown,
          }
        : null,
    source: state.source
      ? {
          kind: state.source.field === 'body' ? 'message' : state.source.field,
          fields: state.source.fields,
          format: state.source.format ?? 'text',
          ready: state.source.ready,
          key: `${state.source.field}-${state.source.through}-${state.source.chunk.position.sourceSequence}-${state.source.chunk.position.itemIndex}-${state.source.chunk.offset}`,
          text: state.source.chunk.text,
          atStart: state.source.chunk.offset === 0,
          atEnd:
            state.source.chunk.offset + utf8Bytes(state.source.chunk.text) ===
            state.source.chunk.totalBytes,
          loading: state.source.loading,
          failed: state.source.failed,
          navigation: state.source.landing.sequence,
          landing: state.source.landing.edge,
        }
      : null,
    loading: state.loading,
    older: !!state.window?.page.before,
    newer: !!state.window?.page.after,
    latest:
      !!state.window &&
      (!state.window.latest ||
        state.latestKnown > state.window.page.through ||
        state.window.page.after !== null ||
        state.live.some(
          (fragment) =>
            fragment.closedAt !== null && fragment.closedAt <= state.window!.page.through,
        )),
    landing: state.landing,
    toolContext:
      state.selected && state.window && state.window.page.through > 0
        ? { sessionId: state.selected.id, through: state.window.page.through }
        : null,
    liveTools:
      state.window?.latest && state.run
        ? [
            ...new Set(
              state.window.page.chunks.flatMap((chunk) =>
                chunk.turnId === state.run!.turnId && chunk.tool ? [chunk.tool.toolUseId] : [],
              ),
            ),
          ]
        : [],
    waitingTools: (state.interactions?.entries ?? [])
      .filter(
        (entry) =>
          !(
            state.interaction?.record?.requestId === entry.requestId &&
            state.interaction.record.outcome
          ),
      )
      .flatMap((entry) => (entry.toolUseId === null ? [] : [entry.toolUseId])),
    live:
      !!state.window?.latest &&
      state.visits.find((visit) => visit.id === state.selected?.id)?.following !== false,
  };
}

export function conversationView<N extends Node>(
  presentation: Presentation<N>,
  state: State,
  theme: Theme,
  locale: string,
  navigation: N,
  actions: Action[],
) {
  const currentPage = state.pages.at(-1);
  const text = copy(locale),
    ui = presentation.paint(theme),
    busy = mutationPending(state);
  const catalogBusy = busy || catalogPending(state);
  const timelineProps = readingProps(state, theme, locale);
  const visit = state.visits.find((entry) => entry.key === readingKey(state));
  // Keep bounded native readers, without serializing a second copy of every
  // message and theme in component state. Evict oldest background content first.
  while (
    state.visits.length > 1 &&
    utf8Bytes(JSON.stringify(state.visits.filter((entry) => entry !== visit))) > 160000
  )
    state.visits.splice(0, 1);
  const timelineEvents = {
    older: 'older',
    newer: 'newer',
    fill: 'fill',
    prompt: 'prompt',
    latest: 'latest',
    markRead: 'mark-read',
    source: 'source',
    toolSource: 'tool-source',
    fileContext: 'fileContext',
    messageCopy: 'messageCopy',
    viewImage: 'view-image',
    sourceFirst: 'source-first',
    sourcePrevious: 'source-previous',
    sourceNext: 'source-next',
    sourceLast: 'source-last',
    sourceBack: 'source-back',
    sourceRetry: 'source-retry',
    sourceField: 'source-field',
    tailReady: 'tail-ready',
    detached: 'detached',
    searchClosed: 'search-closed',
    browseClosed: 'browse-closed',
  };
  const timeline = ui.box(
    'task-readers',
    [
      ...state.visits.map((entry) => {
        const props =
          entry === visit
            ? timelineProps
            : readingProps(
                {
                  ...state,
                  selected: entry.session,
                  window: entry.window,
                  source: entry.source,
                  image: entry.image,
                  landing: entry.landing,
                  searchOpen: entry.searchOpen,
                  browse: entry.browse,
                  run: null,
                  live: entry.live,
                  interactions: null,
                  interaction: null,
                  loading: false,
                  latestKnown: entry.window?.page.through ?? 0,
                  copyRequest: null,
                },
                theme,
                locale,
              );
        props.active = entry === visit;
        if (entry !== visit) props.markReadEnabled = false;
        return ui.box(
          'task-reader-' + entry.key,
          [presentation.slot(entry.key, 'timeline', props, timelineEvents)],
          { visible: entry === visit, terminal: { grow: 1 } },
        );
      }),
      ...(!visit
        ? [presentation.slot('timeline-empty', 'timeline', timelineProps, timelineEvents)]
        : []),
    ],
    { terminal: { grow: 1 } },
  );
  const imageViewer = presentation.slot(
    'image-viewer',
    'imageViewer',
    {
      theme,
      locale,
      image: state.image,
      active: !!state.image && conversationRoute(state),
    },
    { closed: 'image-closed' },
  );
  const attached = attachmentFiles(
    state.attachments,
    state.selected?.id,
    state.selected?.dataSource,
  );
  const attachmentsDisabled =
    !state.selected ||
    !draftReady(state.drafts, state.selected) ||
    busy ||
    state.loading ||
    !!state.attachments.paste;
  const attachmentsButton = ui.button(
    'attachments-open',
    `${text.attachmentTitle}${attached.length ? ` (${attached.length})` : ''}`,
    attachmentsDisabled,
  );
  const composer = presentation.slot(
    'composer',
    'composer',
    {
      approvalPending:
        state.plan.page?.current.proposal?.status === 'pending_approval' ||
        ['service_approval', 'permissions', 'client_capability'].includes(
          reviewTarget(state)?.kind ?? '',
        ),
      commands: composerCommands(state, locale),
      skillContext: JSON.stringify([
        state.selected?.workspace,
        state.selected?.tools,
        state.selected?.collaborationMode,
        state.selected?.boundaryRevision,
      ]),
      backend: canExecute(state) ? selectedBackend(state) : null,
      context: state.selected?.id ?? null,
      dataSource: state.selected?.dataSource ?? null,
      attachments: state.attachments.files,
      theme,
      locale,
      busy: !!state.attachments.paste,
      filePaste:
        state.selected &&
        !state.selected.archived &&
        conversationRoute(state) &&
        !state.dialog &&
        !state.source &&
        !state.image &&
        (!state.attachments.paste ||
          (state.attachments.paste.observed.context === state.selected.id &&
            sameValue(state.attachments.paste.observed.dataSource, state.selected.dataSource)))
          ? {
              context: state.selected.id,
              allowFiles: true,
            }
          : null,
      fileCommand: state.attachments.paste?.command ?? null,
      running: !!state.run,
      sendLabel: canExecute(state)
        ? state.run
          ? tuiText(locale, 'task-queue')
          : text.send
        : state.modelSetup === 'connect'
          ? text.configureModels
          : text.chooseModel,
      enabled:
        !!state.selected &&
        !state.selected.archived &&
        conversationRoute(state) &&
        !state.dialog &&
        !state.source &&
        !state.image,
      autoFocus: !state.searchOpen && state.browse === null,
    },
    {
      command: 'command',
      sent: 'sent',
      submission: 'submission',
      setup: 'setup',
      readiness: 'readiness',
      restored: 'restored',
      stop: 'stop',
      attachments: 'attachments',
      files: 'files',
    },
  );
  const queued = (state.queue?.entries ?? []).filter((entry) => entry.kind !== 'in_flight');
  const queueProps: RowsProps = {
    terminal: theme.terminal,
    entry: queued.at(-1)?.messageId ?? null,
    revision: state.queue?.revision ?? 0,
    rows: queued.map((entry): RowsProps['rows'][number] => {
      const lane = queued.filter((row) => row.kind === entry.kind);
      const index = lane.findIndex((row) => row.messageId === entry.messageId);
      const enabled = !busy && !executionPending(state);
      const up = {
        key: 'up',
        label: theme.ascii ? '^' : '↑',
        enabled: enabled && index > 0,
      };
      const down = {
        key: 'down',
        label: theme.ascii ? 'v' : '↓',
        enabled: enabled && index + 1 < lane.length,
      };
      const edit = { key: 'edit', label: theme.ascii ? 'e' : '✎', enabled };
      const retract = {
        key: 'retract',
        label: theme.ascii ? 'x' : '×',
        enabled,
      };
      const origin = messageOriginCaption(entry.origin, locale);
      return {
        key: entry.messageId,
        text: `${origin ? origin + ' · ' : ''}${tuiText(locale, entry.kind === 'steering' ? 'task-steering' : 'task-followup')} · ${entry.preview}`,
        actions:
          entry.kind === 'followup'
            ? [
                {
                  key: 'promote',
                  label: theme.ascii ? '^' : '↗',
                  enabled: enabled && !!state.run,
                },
                up,
                down,
                edit,
                retract,
              ]
            : [up, down, edit, retract],
      };
    }),
    shortcuts: [
      { key: 'enter', modifiers: [], input: 'edit', enabled: true },
      { key: 'e', modifiers: [], input: 'edit', enabled: true },
      { key: 's', modifiers: [], input: 'promote', enabled: true },
      { key: 'delete', modifiers: [], input: 'retract', enabled: true },
      { key: 'x', modifiers: [], input: 'retract', enabled: true },
      { key: 'up', modifiers: ['alt'], input: 'up', enabled: true },
      { key: 'down', modifiers: ['alt'], input: 'down', enabled: true },
    ],
    maxRows: 3,
    compactBelow: { height: 12, maxRows: 1 },
    tabIndex: 2,
    foreground: theme.text,
    muted: theme.muted,
    selected: theme.selected,
    hovered: theme.surface,
    scrollbar: {
      thumb: theme.ascii ? '#' : '┃',
      track: theme.ascii ? '|' : '│',
      thumbColor: theme.text,
      trackColor: theme.border,
    },
  };
  const title = ui.text(
    'title',
    (currentPage && state.pageChrome[currentPage]?.title) ||
      (state.pages.at(-1) === 'scheduler'
        ? text.scheduleTitle
        : state.pages.at(-1) === 'recall'
          ? text.recallTitle
          : state.pages.at(-1) === 'apps'
            ? text.appsTitle
            : state.pages.at(-1) === 'imports'
              ? text.importsTitle
              : state.pages.at(-1) === 'projects'
                ? text.projectsTitle
                : state.pages.at(-1) === 'settings'
                  ? tuiText(locale, 'route-settings')
                  : state.pages.at(-1) === 'connections'
                    ? text.settings
                    : state.pages.at(-1) === 'deployment'
                      ? text.deploymentTitle
                      : (state.selected?.title ?? tuiText(locale, 'route-agent-screen'))),
    theme.text,
    650,
    { align: 'start', truncate: true },
  );
  const models = presentation.slot(
    'models',
    'models',
    {
      theme,
      locale,
      selection: state.model,
      selectionKey: String(state.epoch),
      preferredModel:
        state.backendReady && state.backendPreference?.kind === 'model'
          ? {
              connectionId: state.backendPreference.connectionId,
              modelId: state.backendPreference.modelId,
              thinkingLevel: state.backendPreference.thinkingLevel ?? null,
            }
          : null,
      suggestDefault: state.backendReady && !state.backendPreference,
      executor: state.executor,
      catalogEpoch: state.catalogEpoch,
      openRequest: state.modelOpenRequest,
      busy: busy || !state.backendReady || executionPending(state) || (!!state.run && !state.queue),
    },
    {
      selected: 'model',
      resolved: 'model-resolved',
      setup: 'model-setup',
      connections: 'model-connections',
      executor: 'executor',
      backends: 'backends',
    },
  );

  const header = headerView(presentation, state, theme, locale, title, models);
  const metrics =
    state.metrics &&
    !state.executor &&
    state.model &&
    state.metrics.modelId === state.model.choice.selection.modelId &&
    state.metrics.connectionId === state.model.choice.selection.connectionId
      ? state.metrics
      : null;
  const footer = presentation.slot(
    'footer',
    'footer',
    {
      sessionId: state.selected?.id ?? null,
      dataSource: state.selected?.dataSource ?? null,
      running: !!state.run,
      compacting: state.compacting,
      metrics,
      theme,
      locale,
    },
    {},
  );
  const sandbox = ui.button(
    'sandbox-open',
    state.selected ? sandboxLabel(state.selected.permissions.access, locale) : text.sandboxTitle,
    busy || !state.selected || !!state.selected.archived,
    false,
    { background: 0 },
  );
  const welcome =
    !!state.selected &&
    !state.selected.archived &&
    !state.loading &&
    !!state.window &&
    !state.window.page.chunks.length &&
    !state.window.page.before &&
    !state.window.page.after &&
    !state.live.length &&
    !state.visits.find((entry) => entry.id === state.selected?.id)?.submissions?.length &&
    !state.run &&
    !state.source &&
    !state.image &&
    !state.searchOpen;
  const main = ui.box(
    'main',
    [
      ui.box(
        'home',
        [
          ui.box('home-top', [], { terminal: { grow: 1 } }),
          ui.box(
            'home-center',
            [
              ui.box('home-left', [], { terminal: { grow: 1 } }),
              ui.box(
                'home-content',
                [
                  ui.text('home-title', 'Maka', theme.text, 650, { align: 'center' }),
                  ...(state.place
                    ? [
                        ui.text('home-place', state.place, theme.subtle, 400, {
                          align: 'center',
                          truncate: true,
                        }),
                      ]
                    : []),
                  ui.box('home-gap', [], { terminal: { basis: 1 } }),
                  ...(!canExecute(state)
                    ? [
                        ui.text(
                          'home-setup',
                          state.hasBackends
                            ? text.executionBackend
                            : tuiText(
                                locale,
                                state.modelSetup === 'loading'
                                  ? 'connections-loading'
                                  : state.modelSetup === 'unavailable'
                                    ? 'home-setup-failed'
                                    : state.modelSetup === 'connect'
                                      ? 'home-needs-connection'
                                      : 'home-needs-default',
                              ),
                          state.modelSetup === 'unavailable' ? theme.warning : theme.subtle,
                          400,
                          { align: 'center' },
                        ),
                      ]
                    : []),
                  ui.button(
                    'home-new',
                    '+ ' + tuiText(locale, 'sidebar-new-session') + '   Ctrl+N',
                    catalogBusy,
                    true,
                    { background: 0 },
                  ),
                  ui.button('home-projects', text.projectsTitle, false, false, { background: 0 }),
                  ui.button('home-imports', text.importsTitle, false, false, { background: 0 }),
                ],
                { gap: 0, terminal: { basis: 52, shrink: true } },
              ),
              ui.box('home-right', [], { terminal: { grow: 1 } }),
            ],
            { row: true, gap: 0 },
          ),
          ui.box('home-bottom', [], { terminal: { grow: 1 } }),
        ],
        {
          gap: 0,
          visible: !state.selected && !state.embedding.fixed,
          viewport: true,
          terminal: { grow: 1 },
        },
      ),
      ...(state.selected?.archived && attached.length && !state.source && !state.image
        ? [ui.box('archived-attachments', [attachmentsButton], { row: true })]
        : []),
      ui.box(
        'session-body',
        [
          ui.box(
            'conversation',
            [
              ui.box(
                'task-introduction',
                [
                  ui.text('task-prompt', tuiText(locale, 'task-empty'), theme.text, 650),
                  ui.text('task-prompt-help', tuiText(locale, 'task-empty-help'), theme.muted),
                ],
                { visible: welcome, gap: 1 },
              ),
              ui.box('retained-timeline', [timeline], {
                visible: !state.image && !welcome,
                terminal: { grow: 1 },
              }),
              imageViewer,
            ],
            { terminal: { basis: 1, grow: 1 } },
          ),
          ...(state.attachments.paste
            ? [pasteView(presentation, theme, locale, state.attachments.paste, busy)]
            : []),
          ui.box(
            'retained-composer',
            [
              presentation.slot('queue', 'rows', queueProps, {
                activated: 'queue-action',
              }),
              ...(queueProps.rows.length && ui.focusButton
                ? [
                    ui.box(
                      'queue-status',
                      [
                        ui.box('queue-status-space', [], {
                          terminal: { basis: 0, grow: 1 },
                        }),
                        ui.focusButton(
                          'queue-focus',
                          `${theme.ascii ? '=' : '≡'} ${queueProps.rows.length}`,
                          'queue',
                        ),
                      ],
                      { row: true, gap: 0 },
                    ),
                  ]
                : []),
              ...(attached.length
                ? [attachmentSummary(presentation, theme, locale, attached, attachmentsDisabled)]
                : []),
              composerStatus(presentation, state, theme, locale),
              ui.box('composer-navigation', [composer], {
                gap: 0,
                terminal: { shrink: true },
                focusShortcuts: [
                  {
                    key: 'up',
                    modifiers: [],
                    child: 'queue',
                    enabled: queueProps.rows.length > 0,
                  },
                ],
              }),
            ],
            {
              gap: 0,
              visible: !state.source && !state.image && !state.selected?.archived,
              terminal: { shrink: true },
            },
          ),
        ],
        {
          gap: 0,
          visible: !!state.selected,
          viewport: true,
          terminal: { grow: 1 },
        },
      ),
      ui.box(
        'session-metadata',
        [
          ui.box(
            'session-controls',
            [ui.box('model-metadata', [models], { terminal: { shrink: true } }), sandbox],
            { row: true, gap: 0, terminal: { wrap: true, shrink: true } },
          ),
          ui.box('session-metrics', [footer], { terminal: { grow: 1, shrink: true } }),
        ],
        {
          gap: 0,
          wide: {
            min_width: 76,
            direction: 'row',
            children: {
              'session-controls': {
                grow: 0,
                basis: null,
                basis_fraction: null,
                shrink: true,
                wrap: false,
              },
              'session-metrics': {
                grow: 1,
                basis: null,
                basis_fraction: null,
                shrink: true,
                wrap: false,
              },
            },
          },
        },
      ),
    ],
    { visible: conversationRoute(state), gap: 0, terminal: { grow: 1 } },
  );
  return { main, header, attached };
}
