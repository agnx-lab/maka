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

import { compactView, compactCopy } from './compact.js';
import type {
  Action,
  ComponentValues,
  Emission,
  Shortcut,
} from '../../../packages/plugin-sdk/src/index.js';
import { sandboxView } from './sandbox.js';
import {
  appearanceEvent,
  appearanceView,
  effectiveAppearance,
  effectiveLocale,
} from '../../shared/appearance-editor.js';
import { bookmarkView } from '../../shared/bookmark.js';
import type {
  QuestionEditorProps,
  QueueEditorProps,
  ServiceApprovalProps,
  Theme,
} from '../../shared/contracts.js';
import { resolveLocale } from '../../shared/locale.js';
import { copy, tuiText } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { compactLabel, output, type Node, type Presentation } from '../../shared/view.js';

import { hostLogsOpen, stopHostLogs } from './host-logs.js';
import { diagnosticsView } from './diagnostics.js';
import { checklistView } from './checklist-view.js';
import { goalView } from './goal.js';
import { planView } from './plan.js';
import { graphView } from './graph.js';
import { recapView } from './recap-view.js';
import { attachmentView } from '../../shared/attachments.js';

import {
  catalogPending,
  closeDialog,
  conversationRoute,
  intent,
  mutationPending,
  questionChild,
  rootCommands,
  select,
  uploadPastedFile,
} from './actions.js';
import { outputsView } from './outputs-view.js';
import { conversationView } from './conversation-view.js';
import { navigationView } from './navigation-view.js';
import { pagesView } from './pages-view.js';
import { navigationBookmark, navigationKey, type State } from './state.js';

export function frame<N extends Node>(
  presentation: Presentation<N>,
  locale: string,
  state: State,
  values: ComponentValues,
  actions: Action[] = [],
  events: Emission[] = [],
) {
  if (
    !hostLogsOpen(state) &&
    (state.diagnostics.host.watch || state.diagnostics.host.loading !== null)
  )
    stopHostLogs(state);

  const currentPage = state.pages.at(-1);
  if (currentPage && !state.openedPages.includes(currentPage)) state.openedPages.push(currentPage);
  const requested = state.embedding.session;
  if (requested && !sameValue(requested, state.externalSelection) && !mutationPending(state)) {
    state.externalSelection = requested;
    select(state, actions, null);
    state.loading = true;
    closeDialog(state);
    intent(state, actions, { kind: 'openSession', selection: requested }, 'agent', 'observe', {
      sessionId: requested.id,
    });
  }
  if (state.dialog !== 'palette') {
    state.copyRequest = null;
    state.messageCopy = null;
  }
  const recovering = Object.values(state.pending).some(
    (request) => request.kind === 'restorePage' || request.kind === 'restoreReceipt',
  );
  if (
    state.restoring &&
    !state.restoreStarted &&
    !recovering &&
    (state.recoveryCheckpoint.restored ||
      (state.recoveryCheckpoint.observed?.state.kind === 'failed' &&
        state.recoveryCheckpoint.readRetries >= 2))
  ) {
    state.restoreStarted = true;
    intent(state, actions, { kind: 'restorePage' }, 'history', 'page', { after: 0, limit: 1 });
  }
  if (
    state.recoveryCheckpoint.restored &&
    state.recoveryDirty &&
    !mutationPending(state) &&
    !recovering
  ) {
    state.recoveryDirty = false;
    intent(state, actions, { kind: 'restorePage' }, 'history', 'page', {
      after: state.restoreThrough,
      limit: 1,
    });
  }
  uploadPastedFile(state, actions);
  if (!conversationRoute(state) || state.source || state.image || !state.selected) {
    state.searchOpen = false;
    state.browse = null;
  }
  const baseTheme = values.appearance?.[0] as Theme;
  if (state.preferences.open && !state.preferences.draft)
    appearanceEvent(
      state.preferences,
      { kind: 'raw', key: 'appearance-open', event: {} },
      baseTheme,
      locale,
      actions,
    );
  locale = effectiveLocale(state.preferences, locale, values.locale?.[0]);
  const theme = effectiveAppearance(state.preferences, baseTheme);
  const text = copy(locale),
    ui = presentation.paint(theme),
    busy = mutationPending(state);
  const catalogBusy = busy || catalogPending(state);
  const commands = rootCommands(state, locale);
  const navigation = navigationView(presentation, state, theme, locale);
  const { main, header, attached } = conversationView(
    presentation,
    state,
    theme,
    locale,
    navigation,
    actions,
  );
  const pageContent = pagesView(presentation, state, theme, locale, values, main);
  const inlineInteraction = state.dialog === 'approval' || state.dialog === 'question';
  const interactionSlots = [
    ...(state.interaction?.record?.request.kind === 'service_approval' ||
    state.interaction?.record?.request.kind === 'permissions' ||
    state.interaction?.record?.request.kind === 'client_capability'
      ? [
          presentation.slot(
            `${state.interaction.record.request.kind === 'permissions' ? 'permission' : 'service'}-approval-${state.interaction.attempt}`,
            state.interaction.record.request.kind === 'permissions'
              ? 'permissionApproval'
              : 'serviceApproval',
            {
              theme,
              locale,
              open: state.dialog === 'approval',
              record: state.interaction.record,
              busy:
                mutationPending(state) ||
                state.selected?.id !== state.interaction.sessionId ||
                !!state.selected?.archived,
              checking: Object.values(state.pending).some(
                (r) => r.kind === 'interaction' || r.kind === 'receipt' || r.kind === 'respond',
              ),
              status: state.uncertain
                ? text.interactionUnknown
                : state.interaction.notice
                  ? text[state.interaction.notice]
                  : state.selected?.id !== state.interaction.sessionId
                    ? text.interactionChanged
                    : '',
            } satisfies ServiceApprovalProps,
            { closed: 'closed', submit: 'submit', check: 'check' },
          ),
        ]
      : state.interaction?.record
        ? [
            presentation.slot(
              questionChild(state),
              state.interaction.record.request.kind === 'form' ? 'formEditor' : 'questionEditor',
              {
                theme,
                locale,
                open: state.dialog === 'question',
                record: state.interaction.record,
                busy:
                  mutationPending(state) ||
                  state.selected?.id !== state.interaction.sessionId ||
                  !!state.selected?.archived,
                checking: Object.values(state.pending).some(
                  (r) => r.kind === 'interaction' || r.kind === 'receipt' || r.kind === 'respond',
                ),
                status: state.uncertain
                  ? text.interactionUnknown
                  : state.interaction.notice
                    ? text[state.interaction.notice]
                    : state.selected?.id !== state.interaction.sessionId
                      ? text.interactionChanged
                      : '',
              } satisfies QuestionEditorProps,
              { closed: 'closed', submit: 'submit', check: 'check' },
            ),
          ]
        : [
            ui.box(
              'question-loading',
              [
                ui.button('question-close', text.closeModels),
                ui.text(
                  'question-loading-text',
                  state.interaction?.notice ? text[state.interaction.notice] : text.questionLoading,
                ),
                ui.button(
                  'question-check',
                  text.check,
                  Object.values(state.pending).some((r) => r.kind === 'interaction'),
                ),
              ],
              {
                visible: state.dialog === 'question' || state.dialog === 'approval',
                terminal: { grow: 1 },
              },
            ),
          ]),
  ];
  const inspectorOpen =
    conversationRoute(state) &&
    ((!!state.selected && (state.inspector !== null || inlineInteraction)) ||
      (hostLogsOpen(state) && state.inspector === 'diagnostics'));
  const inspectorSlots = [
    diagnosticsView(presentation, state, theme, locale),
    graphView(presentation, state, theme, locale),
    planView(presentation, state, theme, locale),
    goalView(presentation, state, theme, locale),
    checklistView(presentation, state, theme, locale),
    recapView(presentation, state, theme, locale),
    outputsView(presentation, state, theme, locale),
    presentation.slot(
      'terminal-panel',
      'terminals',
      {
        theme,
        locale,
        open: inspectorOpen && !inlineInteraction && state.inspector === 'terminal',
        enabled: state.terminals,
        busy: mutationPending(state),
        session: state.selected,
      },
      { closed: 'closed' },
    ),
    presentation.slot(
      'session-details',
      'details',
      {
        theme,
        locale,
        open: inspectorOpen && !inlineInteraction && state.inspector === 'details',
        session: state.selected,
        busy: mutationPending(state),
      },
      { changed: 'changed', closed: 'closed' },
    ),
  ];
  const workspace = ui.box(
    'workspace',
    [
      ui.box('workspace-main', [pageContent], {
        gap: 0,
        terminal: inspectorOpen ? { basis: 0, shrink: true } : { grow: 1 },
      }),
      ui.box('workspace-inspector', [...inspectorSlots, ...interactionSlots], {
        visible: inspectorOpen,
        gap: 0,
        padding: 1,
        surface: true,
        terminal: { grow: 1 },
      }),
    ],
    {
      gap: 0,
      terminal: { grow: 1 },
      wide: {
        min_width: 100,
        direction: 'row',
        children: {
          'workspace-main': { grow: 1, basis: 0 },
          'workspace-inspector': { basis: 44, shrink: true },
        },
      },
    },
  );
  const conversation = ui.box(
    'agent-screen',
    state.embedding.fixed
      ? [workspace]
      : [
          ui.adaptivePane!(
            'navigation-pane',
            text.conversations,
            ui.box(
              'page',
              [
                ui.box('page-inset-start', [], { terminal: { basis: 1 } }),
                workspace,
                ui.box('page-inset-end', [], { terminal: { basis: 1 } }),
              ],
              { row: true, gap: 0, terminal: { grow: 1 } },
            ),
            ui.box('navigation-content', [navigation], {
              gap: 0,
              terminal: { grow: 1 },
            }),
            'command' in state.sidebar ? state.sidebar.command : null,
          ),
        ],
    { row: false, gap: 0, terminal: { grow: 1 } },
  );
  const background = ui.box(
    'product',
    [
      header,
      ...(state.bookmark.observed?.state.kind === 'failed'
        ? [
            ui.box(
              'navigation-save-error',
              [
                ui.text(
                  'navigation-save-message',
                  state.bookmark.command &&
                    state.bookmark.command.sequence > state.bookmark.observed.sequence
                    ? copy(locale).restoringView
                    : copy(locale).viewSaveFailed,
                  theme.warning,
                ),
                ...(state.bookmark.command &&
                state.bookmark.command.sequence > state.bookmark.observed.sequence
                  ? []
                  : [ui.button('navigation-checkpoint-retry', copy(locale).reloadView)]),
              ],
              { row: true, gap: 1, terminal: { wrap: true } },
            ),
          ]
        : []),
      ...(!state.embedding.fixed && state.scheduleAlerts.reminders.length
        ? [
            ui.button(
              'schedule-reminder',
              `${text.scheduleReminder}: ${compactLabel(state.scheduleAlerts.reminders[0]!.title, 100)}`,
              !!state.dialog,
              false,
              { background: theme.surface, align: 'start' },
            ),
          ]
        : []),
      conversation,
      bookmarkView(
        presentation,
        state.recoveryCheckpoint,
        'recovery-checkpoint',
        'operations/agent-screen',
        state.restoring || recovering
          ? (state.recoveryCheckpoint.saved ?? { through: 0, scope: null })
          : { through: state.restoreThrough, scope: state.historyScope },
      ),
      ...(!state.embedding.fixed || state.embedding.session
        ? [
            bookmarkView(
              presentation,
              state.bookmark,
              navigationKey(state),
              navigationKey(state),
              navigationBookmark(state),
            ),
          ]
        : []),
    ],
    {
      gap: 0,
      background: theme.background,
      terminal: { grow: 1 },
      shortcuts: [
        ...(inlineInteraction
          ? [{ key: 'escape', modifiers: [], input: 'question-close', enabled: true } as Shortcut]
          : []),
        ...(inspectorOpen && !state.dialog && !state.preferences.open
          ? [{ key: 'escape', modifiers: [], input: 'inspector-close', enabled: true } as Shortcut]
          : []),
        ...(state.pages.at(-1) === 'settings' && state.dialog === null && !state.preferences.open
          ? ([{ key: 'escape', modifiers: [], input: 'page-back', enabled: true }] as Shortcut[])
          : []),
        {
          key: 'f',
          modifiers: ['primary'],
          input: 'search-open',
          enabled: commands.some((command) => command.kind === 'search-open' && !command.disabled),
        },
        { key: 'f1', modifiers: [], input: 'help-open', enabled: true },
        {
          key: 'f4',
          modifiers: [],
          input: 'outputs-open',
          enabled: commands.some((command) => command.kind === 'outputs-open' && !command.disabled),
        },
        {
          key: 'g',
          modifiers: ['primary'],
          input: 'interaction-open',
          enabled: commands.some(
            (command) => command.kind === 'interaction-open' && !command.disabled,
          ),
        },
        {
          key: 'k',
          modifiers: ['primary'],
          input: 'palette-open',
          enabled: true,
        },
        {
          key: 'b',
          modifiers: ['primary'],
          input: 'navigation-toggle',
          enabled: !state.embedding.fixed,
        },
        {
          key: 'n',
          modifiers: ['primary'],
          input: 'create',
          enabled: !state.embedding.fixed && !mutationPending(state) && !catalogPending(state),
        },
      ],
    },
  );
  const out = output(
    state,
    ui.dialog(
      'agent-screen-dialog',
      state.dialog === 'compact'
        ? compactCopy(locale).title
        : state.dialog === 'sandbox'
          ? text.sandboxTitle
          : state.dialog === 'session-change'
            ? text.deleteSessionTitle
            : state.dialog === 'attachments'
              ? state.attachments.selection?.browse
                ? tuiText(locale, 'attachments-local-files')
                : text.attachmentTitle
              : state.dialog === 'skills'
                ? text.skillsTitle
                : state.dialog === 'terminal'
                  ? text.terminalTitle
                  : state.dialog === 'question'
                    ? state.interaction?.record?.request.kind === 'form'
                      ? text.formTitle
                      : text.questionsTitle
                    : state.dialog === 'approval'
                      ? state.interaction?.record?.request.kind === 'permissions'
                        ? text.permissionApprovalTitle
                        : state.interaction?.record?.request.kind === 'client_capability'
                          ? text.computerApprovalTitle
                          : text.approvalTitle
                      : state.dialog === 'directories'
                        ? text.directoryTitle
                        : state.dialog === 'graph'
                          ? text.graphTitle
                          : state.dialog === 'plan'
                            ? text.planTitle
                            : state.dialog === 'goal'
                              ? text.goalTitle
                              : state.dialog === 'recap'
                                ? text.recapTitle
                                : state.dialog === 'checklist'
                                  ? text.checklistTitle
                                  : state.dialog === 'details'
                                    ? text.detailsTitle
                                    : state.dialog === 'diagnostics'
                                      ? text.diagnosticsTitleLabel
                                      : state.dialog === 'queue'
                                        ? text.queueEdit
                                        : state.dialog === 'help'
                                          ? text.helpTitle
                                          : text.paletteTitle,
      background,
      ui.box(
        'dialog-content',
        [
          ...(state.dialog === 'compact' ? [compactView(presentation, state, theme, locale)] : []),
          ...(state.dialog === 'sandbox' ? [sandboxView(presentation, state, theme, locale)] : []),
          ...(state.dialog === 'session-change' && state.sessionChange
            ? [
                ui.text('session-change-name', state.sessionChange.title, theme.text, 600),
                ui.text('session-change-help', text.deleteSessionHelp, theme.muted),
                ...(state.notice
                  ? [ui.text('session-change-notice', text[state.notice], theme.warning)]
                  : []),
                ui.box(
                  'session-change-buttons',
                  [
                    ui.button('session-change-cancel', text.cancel),
                    ui.button(
                      'session-change-confirm',
                      text.deleteAction,
                      busy || !!state.uncertain,
                      false,
                    ),
                  ],
                  { row: true, gap: 2 },
                ),
              ]
            : []),
          ...(state.dialog === 'attachments' && state.attachments.selection
            ? [
                attachmentView(
                  presentation,
                  theme,
                  locale,
                  state.attachments,
                  busy,
                  state.selected?.id === state.attachments.selection.sessionId &&
                    sameValue(state.selected.dataSource, state.attachments.selection.dataSource) &&
                    !state.selected.archived,
                  state.notice
                    ? text[state.notice]
                    : state.selected?.archived
                      ? text.archivedConversation
                      : null,
                  !!state.uncertain,
                  Object.values(state.pending).some((request) => request.kind === 'receipt'),
                ),
              ]
            : []),
          presentation.slot(
            'queue-editor',
            'queueEditor',
            {
              theme,
              locale,
              open: state.dialog === 'queue',
              ready: state.queueEdit?.phase === 'ready',
              hasAttachments: state.queueEdit?.hasAttachments ?? false,
              busy: mutationPending(state),
              uncertain: !!state.uncertain,
              checking: Object.values(state.pending).some((request) => request.kind === 'receipt'),
              command: state.queueEdit?.command ?? null,
              status: state.notice
                ? text[state.notice]
                : state.queueEdit?.phase === 'loading'
                  ? text.queueLoading
                  : '',
            } satisfies QueueEditorProps,
            {
              changed: 'changed',
              save: 'save',
              closed: 'closed',
              reload: 'reload',
              check: 'check',
            },
          ),
          ...(state.skills
            ? [
                presentation.slot(
                  'skill-library',
                  'skills',
                  {
                    theme,
                    locale,
                    open: state.dialog === 'skills',
                    workspace: state.selected?.workspace ?? null,
                  },
                  { closed: 'closed' },
                ),
              ]
            : []),
          presentation.slot(
            'help',
            'help',
            { theme, locale, open: state.dialog === 'help' },
            { closed: 'closed' },
          ),
          presentation.slot(
            'directory-picker',
            'directories',
            {
              theme,
              locale,
              open: state.dialog === 'directories',
              busy: mutationPending(state) || catalogPending(state),
            },
            { selected: 'selected', closed: 'closed' },
          ),
          presentation.slot(
            'command-palette',
            'palette',
            {
              theme,
              locale,
              open: state.dialog === 'palette',
              busy: false,
              selectionDisabled:
                !!state.embedding.fixed || mutationPending(state) || catalogPending(state),
              commands: commands.map(({ kind: _kind, ...command }) => command),
            },
            { invoked: 'invoked', selected: 'selected', closed: 'closed' },
          ),
        ],
        { gap: 0, terminal: { grow: state.dialog === 'queue' ? 0 : 1 } },
      ),
      state.dialog !== null && !inlineInteraction,
      state.dialog === 'compact'
        ? { width: 88, height: 26 }
        : state.dialog === 'sandbox'
          ? { width: 56, height: 16 }
          : state.dialog === 'attachments'
            ? {
                width: state.attachments.selection?.browse ? 80 : 64,
                height: state.attachments.selection?.browse
                  ? 28
                  : Math.min(20, attached.length * 3 + 7),
              }
            : {},
    ),
  );
  out.frame.view = appearanceView(
    presentation,
    state.preferences,
    theme,
    locale,
    out.frame.view,
    resolveLocale('auto', values.locale?.[0]),
  );
  if (!state.embedding.embedded) out.frame.surface = { kind: 'terminal', binding: 'surface' };
  if (!state.embedding.embedded)
    out.frame.view = presentation.paint(theme).box(
      'agent-screen-with-authoring',
      [
        out.frame.view,
        presentation
          .paint(theme)
          .box('authoring-background', [presentation.slot('authoring', 'authoring', {}, {})], {
            visible: false,
          }),
      ],
      { terminal: { grow: 1 } },
    );
  out.actions = actions;
  out.events = events;
  out.subscriptions = [
    ...(hostLogsOpen(state) && state.diagnostics.host.watch
      ? [
          {
            key: state.diagnostics.host.watch.key,
            input: state.diagnostics.host.watch.input,
            binding: 'hostLogs',
            member: 0,
            method: 'watch',
            resources: {},
          },
        ]
      : []),
    ...(state.diagnostics.watch
      ? [
          {
            ...state.diagnostics.watch,
            binding: 'agent',
            member: 0,
            method: 'watchDiagnostics',
            resources: {},
          },
        ]
      : []),
    ...(state.graph.watch
      ? [
          {
            ...state.graph.watch,
            binding: 'workhub',
            member: 0,
            method: 'watchGraph',
            resources: {},
          },
        ]
      : []),
    ...(state.plan.watch
      ? [{ ...state.plan.watch, binding: 'agent', member: 0, method: 'watchPlan', resources: {} }]
      : []),
    ...(state.goal.watch
      ? [{ ...state.goal.watch, binding: 'agent', member: 0, method: 'watchGoal', resources: {} }]
      : []),
    ...(state.checklist.watch
      ? [
          {
            ...state.checklist.watch,
            binding: 'agent',
            member: 0,
            method: 'watchChecklist',
            resources: {},
          },
        ]
      : []),
    ...(state.scheduleWatch && !state.embedding.fixed
      ? [
          {
            key: state.scheduleWatch,
            binding: 'agent',
            member: 0,
            method: 'scheduleAlerts',
            input: {},
            resources: {},
          },
        ]
      : []),
    ...(state.catalogWatch
      ? [
          {
            key: state.catalogWatch.key,
            binding: 'agent',
            member: 0,
            method: 'catalog',
            input: state.catalogWatch.input,
            resources: {},
          },
        ]
      : []),
    ...(state.watch
      ? [
          {
            key: state.watch.key,
            binding: 'agent',
            member: 0,
            method: 'watch',
            input: state.watch.input,
            resources: {},
          },
        ]
      : []),
  ];
  return out;
}
