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

import { compactComponent } from './compact.js';
import type {
  Action,
  AgentDirectorySelection,
  AgentExecutorSelection,
  AgentProjectRef,
  AgentQueueAction,
  AgentSession,
  ComponentInput,
  ComponentValues,
  EditorObservation,
  Emission,
  FileObservation,
  InteractionAnswer,
  RowActivated,
  SourceField,
  SourcePosition,
} from '../../../packages/plugin-sdk/src/index.js';
import type {
  ComposerSent,
  ComposerSubmission,
  MessageCopyObservation,
  ModelSetup,
  PageChrome,
  QuestionDraft,
  SelectedModel,
  TimelineFileContext,
} from '../../shared/contracts.js';
import { copy } from './locale.js';
import { sameValue } from '../../shared/value.js';
import { type Node, type Presentation } from '../../shared/view.js';

import { attachmentChild, pastedFiles, type DraftFilePaste } from '../../shared/attachments.js';

import type { ComposerReadiness } from '../../shared/drafts.js';

import {
  sentDraft,
  openModelSetup,
  catalog,
  catalogPending,
  checkInteraction,
  closeDialog,
  controlSidebar,
  conversationRoute,
  executionPending,
  follow,
  refreshActivity,
  history,
  promptAnchor,
  initializeQueueEdit,
  intent,
  loadQueueEdit,
  mutationPending,
  observe,
  openUsage,
  questionChild,
  readHistorySource,
  respondInteraction,
  rootCommand,
  rootCommands,
  select,
  changeSession,
  saveBackend,
  submitQuestion,
} from './actions.js';
import { goalComponent } from './goal.js';
import { frame } from './frame.js';
import { utf8Bytes } from './messages.js';
import { type Page, type State } from './state.js';
import { readingKey } from './state.js';

export function handleComponent<N extends Node>(
  presentation: Presentation<N>,
  configuredLocale: string,
  state: State,
  input: Extract<ComponentInput, { kind: 'component' }>,
  values: ComponentValues,
  actions: Action[],
  events: Emission[],
  locale: string,
) {
  if (input.child === 'composer' && input.name === 'command') {
    const command = input.event as Pick<ComposerSent, 'context' | 'dataSource'> & { id: string };
    if (
      command.context === state.selected?.id &&
      sameValue(command.dataSource, state.selected.dataSource)
    ) {
      const available = rootCommands(state, locale).find(
        (item) => item.id === command.id && !item.disabled,
      );
      if (available) rootCommand(state, actions, locale, available.kind);
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (compactComponent(state, input))
    return frame(presentation, configuredLocale, state, values, actions);
  if (goalComponent(state, input))
    return frame(presentation, configuredLocale, state, values, actions);
  if (input.name === 'chrome' && state.openedPages.includes(input.child as Page)) {
    state.pageChrome[input.child as Page] = input.event as PageChrome;
    return frame(presentation, configuredLocale, state, values, actions);
  }

  if (input.child === 'composer' && input.name === 'readiness') {
    state.drafts = input.event as ComposerReadiness[];
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'composer' && input.name === 'restored') {
    const files = (input.event as { files: State['attachments']['files'] }).files;
    for (const file of files) {
      if (
        !state.attachments.files.some(
          (current) =>
            current.id === file.id &&
            current.sessionId === file.sessionId &&
            sameValue(current.dataSource, file.dataSource),
        )
      )
        state.attachments.files.push(file);
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'composer' && input.name === 'files' && true) {
    pastedFiles(state.attachments, input.event as DraftFilePaste);
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === attachmentChild(state.attachments) && input.name === 'changed') {
    if (state.dialog === 'attachments' && state.attachments.selection && !mutationPending(state))
      state.attachments.selection.file = input.event as FileObservation;
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === `permission-approval-${state.interaction?.attempt}`) {
    if (state.dialog === 'approval') {
      if (input.name === 'closed') closeDialog(state);
      else if (input.name === 'check') checkInteraction(state, actions);
      else if (input.name === 'submit') {
        const event = input.event as {
          requestId: string;
          decision: 'deny' | 'allow';
          scope: 'once' | 'turn' | 'session';
        };
        const request = state.interaction?.record?.request;
        if (event.requestId === state.interaction?.requestId && request?.kind === 'permissions')
          respondInteraction(state, actions, {
            kind: 'permissions',
            decision:
              event.decision === 'deny'
                ? { decision: 'deny' }
                : {
                    decision: 'allow',
                    scope: event.scope,
                    permissions: request.request.permissions,
                  },
          });
      }
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === `service-approval-${state.interaction?.attempt}`) {
    if (state.dialog === 'approval') {
      if (input.name === 'closed') closeDialog(state);
      else if (input.name === 'check') checkInteraction(state, actions);
      else if (input.name === 'submit') {
        const event = input.event as { requestId: string; decision: 'allow' | 'deny' };
        if (event.requestId === state.interaction?.requestId)
          respondInteraction(state, actions, {
            kind:
              state.interaction?.record?.request.kind === 'client_capability'
                ? 'client_capability'
                : 'service_approval',
            decision: event.decision,
          });
      }
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === questionChild(state)) {
    if (state.dialog === 'question') {
      if (input.name === 'closed') closeDialog(state);
      else if (input.name === 'check') checkInteraction(state, actions);
      else if (input.name === 'submit' && state.interaction?.record?.request.kind === 'form') {
        const event = input.event as { requestId: string; answer: InteractionAnswer };
        if (event.requestId === state.interaction.requestId && event.answer.kind === 'form')
          respondInteraction(state, actions, event.answer);
      } else if (input.name === 'submit')
        submitQuestion(
          state,
          actions,
          input.event as { requestId: string; answers: QuestionDraft[] },
        );
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'queue-editor') {
    const edit = state.queueEdit;
    if (input.name === 'changed') {
      const draft = input.event as EditorObservation;
      state.queueDraft = draft;
      if (
        edit?.command &&
        draft.receipt?.owner === edit.command.owner &&
        draft.receipt.sequence === edit.command.sequence
      ) {
        edit.phase = draft.receipt.outcome.status === 'applied' ? 'ready' : 'failed';
        edit.command = null;
        if (edit.phase === 'failed') state.notice = 'draftKept';
      }
      initializeQueueEdit(state);
    } else if (state.dialog === 'queue' && edit) {
      if (input.name === 'closed') closeDialog(state);
      else if (
        input.name === 'check' &&
        state.uncertain &&
        !Object.values(state.pending).some((request) => request.kind === 'receipt')
      )
        intent(state, actions, { kind: 'receipt' }, 'receipts', 'read', state.uncertain.reference);
      else if (!mutationPending(state)) {
        if (
          input.name === 'reload' &&
          state.selected?.id === edit.target.sessionId &&
          state.queue
        ) {
          loadQueueEdit(state, actions, {
            ...edit.target,
            expectedRevision: state.queue.revision,
          });
        } else if (input.name === 'save' && edit.phase === 'ready' && !edit.command) {
          const draft = input.event as EditorObservation;
          if (!sameValue(draft, state.queueDraft) || draft.snapshot.marked)
            state.notice = 'draftKept';
          else
            intent(
              state,
              actions,
              { kind: 'queueRead', attempt: edit.attempt, draft },
              'reader',
              'read',
              { start: 0, end: draft.snapshot.text.bytes },
              {
                text: {
                  child: 'queue-editor',
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
      }
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'skill-library') {
    if (input.name === 'closed' && state.dialog === 'skills') state.dialog = null;
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'help') {
    if (input.name === 'closed' && state.dialog === 'help') state.dialog = null;
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'terminal-panel') {
    if (input.name === 'closed') {
      if (state.dialog === 'terminal') state.dialog = null;
      if (state.inspector === 'terminal') state.inspector = null;
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'session-details') {
    if (input.name === 'closed') {
      if (state.dialog === 'details') state.dialog = null;
      if (state.inspector === 'details') state.inspector = null;
    } else if (input.name === 'changed') {
      if (state.selected?.id === (input.event as { sessionId: string }).sessionId)
        observe(state, actions);
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (
    [
      'assistant-preferences',
      'session-defaults',
      'network-settings',
      'insights',
      'web-settings',
      'jev-settings',
      'subagent-settings',
    ].includes(input.child) &&
    input.name === 'closed' &&
    state.pages.at(-1) === 'settings'
  )
    state.pages.pop();
  if (input.child === 'apps') {
    if (input.name === 'closed' && state.pages.at(-1) === 'apps') state.pages.pop();
    if (input.name === 'manage' && state.pages.at(-1) === 'apps') state.pages.push('deployment');
  }
  if (input.child === 'scheduler') {
    if (input.name === 'closed' && state.pages.at(-1) === 'scheduler') state.pages.pop();
    else if (input.name === 'connections' && state.pages.at(-1) === 'scheduler')
      state.pages.push('connections');
    else if (
      input.name === 'selected' &&
      state.pages.at(-1) === 'scheduler' &&
      !state.embedding.fixed &&
      !mutationPending(state) &&
      !catalogPending(state)
    ) {
      select(state, actions, input.event as AgentSession);
      state.pages = [];
      catalog(state);
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'recall') {
    if (input.name === 'closed' && state.pages.at(-1) === 'recall') state.pages.pop();
    else if (
      input.name === 'selected' &&
      state.pages.at(-1) === 'recall' &&
      !state.embedding.fixed &&
      !mutationPending(state) &&
      !catalogPending(state)
    ) {
      const selected = input.event as { session: AgentSession; anchor: number | null };
      select(state, actions, selected.session);
      state.recallAnchor = selected.anchor;
      state.pages = [];
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'imports') {
    if (input.name === 'closed' && state.pages.at(-1) === 'imports') state.pages.pop();
    else if (input.name === 'changed') catalog(state);
    else if (
      input.name === 'selected' &&
      state.pages.at(-1) === 'imports' &&
      !state.embedding.fixed &&
      !mutationPending(state) &&
      !catalogPending(state)
    ) {
      select(state, actions, (input.event as { session: AgentSession }).session);
      state.pages = [];
      catalog(state);
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'projects') {
    if (input.name === 'closed' && state.pages.at(-1) === 'projects') state.pages.pop();
    else if (input.name === 'changed') {
      catalog(state);
      if (state.selected) observe(state, actions);
    } else if (
      input.name === 'selected' &&
      state.pages.at(-1) === 'projects' &&
      !state.embedding.fixed &&
      !mutationPending(state) &&
      !catalogPending(state)
    ) {
      intent(state, actions, { kind: 'create' }, 'agent', 'create', {
        title: copy(locale).new,
        automaticTitle: true,
        labels: [],
        workspace: {
          kind: 'project',
          ...(input.event as {
            project: AgentProjectRef;
            selection: AgentDirectorySelection;
          }),
        },
      });
      state.pages = [];
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'directory-picker') {
    if (state.dialog === 'directories' && conversationRoute(state)) {
      if (input.name === 'closed') state.dialog = null;
      else if (
        input.name === 'selected' &&
        !state.embedding.fixed &&
        !mutationPending(state) &&
        !catalogPending(state)
      ) {
        intent(state, actions, { kind: 'create' }, 'agent', 'create', {
          title: copy(locale).new,
          automaticTitle: true,
          labels: [],
          workspace: { kind: 'directory', selection: input.event as AgentDirectorySelection },
        });
        state.dialog = null;
      }
    }
    return frame(presentation, configuredLocale, state, values, actions);
  }
  if (input.child === 'command-palette') {
    if (state.dialog === 'palette') {
      if (input.name === 'closed') state.dialog = null;
      else if (input.name === 'invoked') {
        const command = rootCommands(state, locale).find(
          (command) => command.id === (input.event as { id: string }).id && !command.disabled,
        );
        if (
          command &&
          rootCommand(state, actions, locale, command.kind) &&
          state.dialog === 'palette'
        ) {
          state.dialog = null;
          if (command.kind === 'workhub-open') events.push({ name: 'workhub', value: {} });
        }
      } else if (
        input.name === 'selected' &&
        !state.embedding.fixed &&
        !mutationPending(state) &&
        !catalogPending(state)
      ) {
        select(state, actions, input.event as AgentSession);
        state.pages = [];
        state.dialog = null;
      }
    }
    return frame(presentation, configuredLocale, state, values, actions, events);
  }

  if (input.child === 'queue' && input.name === 'activated') {
    const event = input.event as RowActivated;
    const queue = state.queue;
    const row = queue?.entries.find((row) => row.messageId === event.row);
    if (
      conversationRoute(state) &&
      !state.dialog &&
      !mutationPending(state) &&
      !executionPending(state) &&
      state.selected &&
      queue &&
      event.revision === queue.revision &&
      row &&
      row.kind !== 'in_flight'
    ) {
      let edit: AgentQueueAction | null = null;
      if (event.action === 'edit')
        loadQueueEdit(state, actions, {
          sessionId: state.selected.id,
          messageId: row.messageId,
          expectedRevision: event.revision,
        });
      else if (event.action === 'retract') edit = { kind: 'retract', messageId: row.messageId };
      else if (event.action === 'promote' && row.kind === 'followup' && state.run)
        edit = { kind: 'promote', messageId: row.messageId };
      else if (event.action === 'up' || event.action === 'down') {
        const ids = queue.entries
          .filter((entry) => entry.kind === row.kind)
          .map((entry) => entry.messageId);
        const index = ids.indexOf(row.messageId),
          next = index + (event.action === 'up' ? -1 : 1);
        if (next >= 0 && next < ids.length) {
          [ids[index], ids[next]] = [ids[next], ids[index]];
          edit = { kind: 'reorder', messageIds: [ids[0], ...ids.slice(1)] };
        }
      }
      if (edit)
        intent(
          state,
          actions,
          { kind: 'queue', sessionId: state.selected.id },
          'agent',
          'editQueue',
          { sessionId: state.selected.id, expectedRevision: event.revision, edit },
        );
    }
  } else if (input.child === readingKey(state) && input.name === 'messageCopy') {
    const copied = input.event as MessageCopyObservation | null;
    if (
      !copied ||
      (state.dialog === 'palette' &&
        copied.request === state.copyRequest &&
        copied.sessionId === state.selected?.id &&
        copied.through === state.window?.page.through &&
        copied.landing === state.landing.sequence)
    )
      state.messageCopy = copied;
  } else if (input.child === readingKey(state) && input.name === 'fileContext') {
    const context = input.event as TimelineFileContext | null;
    if (
      !context ||
      (context.sessionId === state.selected?.id &&
        context.through === state.window?.page.through &&
        context.landing === state.landing.sequence)
    )
      state.fileContext = context;
  } else if (input.child === readingKey(state) && input.name === 'browseClosed') {
    if ((input.event as { request: number }).request === state.browse) state.browse = null;
  } else if (input.child === readingKey(state) && input.name === 'searchClosed') {
    state.searchOpen = false;
  } else if (input.child === readingKey(state) && input.name === 'detached') {
    const event = input.event as { landing: number; value: boolean };
    const visit = state.visits.find((entry) => entry.id === state.selected?.id);
    if (visit && event.landing === state.landing.sequence) {
      visit.detached = event.value;
      refreshActivity(state, actions);
    }
  } else if (input.child === readingKey(state) && input.name === 'tailReady') {
    const receipt = input.event as NonNullable<State['tailReceipt']>;
    if (receipt.landing === state.landing.sequence) {
      state.tailReceipt = receipt;
      follow(state, actions);
    }
  } else if (input.child === 'connections' && input.name === 'completed') {
    if (state.connectionSetupRequest !== null && state.pages.at(-1) === 'connections') {
      state.pages.pop();
      state.connectionSetupRequest = null;
      state.modelOpenRequest = ++state.serial;
    }
  } else if (
    input.name === 'closed' &&
    input.child === 'settings-models' &&
    state.pages.at(-1) === 'settings'
  ) {
    state.pages.pop();
  } else if (input.name === 'closed' && input.child === state.pages.at(-1)) {
    state.pages.pop();
    if (input.child === 'connections') state.connectionSetupRequest = null;
  } else if (
    (input.child === 'connections' || input.child === 'settings-models') &&
    input.name === 'changed'
  ) {
    state.catalogEpoch++;
    state.modelSetup = 'loading';
    state.modelRefreshing = !!state.model;
  } else if (input.child === 'composer' && input.name === 'submission') {
    const submission = input.event as ComposerSubmission;
    const visit = state.visits.find(
      (entry) =>
        entry.id === submission.context && sameValue(entry.dataSource, submission.dataSource),
    );
    if (visit) {
      const entries = (visit.submissions ??= []);
      const index = entries.findIndex((entry) => entry.key === submission.key);
      if (submission.status === 'removed') {
        if (index >= 0) entries.splice(index, 1);
      } else if (index >= 0) Object.assign(entries[index]!, submission);
      else if (submission.status === 'pending') {
        entries.push({
          ...submission,
          attachmentNames: state.attachments.files
            .filter(
              (file) =>
                file.sessionId === submission.context &&
                sameValue(file.dataSource, submission.dataSource) &&
                submission.attachments.includes(file.id),
            )
            .map((file) => file.name),
        });
        if (state.selected?.id === visit.id) {
          state.searchOpen = false;
          state.browse = null;
          visit.following = true;
          state.landing = { sequence: state.landing.sequence + 1, edge: 'end' };
        }
      }
    }
  } else if (input.child === 'composer' && input.name === 'sent') {
    sentDraft(state, actions, input.event as ComposerSent);
  } else if (input.child === 'composer' && input.name === 'setup') {
    if ((input.event as { context: string }).context === state.selected?.id) openModelSetup(state);
  } else if (input.child === 'composer' && input.name === 'stop') {
    rootCommand(state, actions, locale, 'stop');
  } else if (input.child === 'models' && input.name === 'backends') {
    state.hasBackends = (input.event as { available: boolean }).available;
  } else if (
    input.child === 'models' &&
    input.name === 'executor' &&
    !mutationPending(state) &&
    !executionPending(state)
  ) {
    state.executor = input.event as AgentExecutorSelection | null;
    saveBackend(state, actions);
  } else if (input.child === 'models' && input.name === 'resolved') {
    const resolved = input.event as { selectionKey: string; selection: SelectedModel | null };
    if (resolved.selectionKey === String(state.epoch) && !state.executor) {
      state.model = resolved.selection;
      state.modelRefreshing = false;
    }
  } else if (input.child === 'models' && input.name === 'setup') {
    const setup = input.event as ModelSetup;
    if (setup.catalogEpoch === state.catalogEpoch) state.modelSetup = setup.kind;
  } else if (
    input.child === 'models' &&
    input.name === 'connections' &&
    conversationRoute(state) &&
    !state.dialog
  ) {
    state.connectionSetupRequest = ++state.serial;
    state.pages = ['connections'];
  } else if (
    input.child === 'models' &&
    input.name === 'selected' &&
    !state.executor &&
    !mutationPending(state) &&
    !executionPending(state)
  ) {
    state.model = input.event as SelectedModel;
    state.modelRefreshing = false;
    saveBackend(state, actions);
  } else if (input.child === 'navigation' && (input.name === 'older' || input.name === 'newer')) {
    const page = state.catalog;
    if (page && !mutationPending(state) && !catalogPending(state)) {
      // Keep half of the original rows in the next bounded window. Native
      // reading preserves their screen position and focus across the reply.
      const middle = page.sessions[Math.floor(page.sessions.length / 2)];
      if (middle && input.name === 'newer' && page.after)
        catalog(
          state,
          page.sessions.length > 1
            ? { kind: 'at', cursor: middle.id }
            : { kind: 'after', revision: page.revision, cursor: page.after },
        );
      else if (middle && input.name === 'older' && page.before)
        catalog(state, { kind: 'before', revision: page.revision, cursor: middle.id });
    }
  } else if (
    input.child === 'navigation' &&
    !state.embedding.fixed &&
    input.name === 'select' &&
    state.dialog === null &&
    !mutationPending(state)
  ) {
    const { sessionId, details, usage, action } = input.event as {
      sessionId: string;
      details?: boolean;
      usage?: boolean;
      action?: 'archive' | 'remove' | 'stop';
    };
    const session = state.catalog?.sessions.find((item) => item.id === sessionId);
    if (session && action === 'remove') {
      if (session.activity !== 'idle') state.notice = 'sessionChangeBusy';
      else {
        state.sessionChange = session;
        state.dialog = 'session-change';
        state.notice = null;
      }
    } else if (session && action === 'archive')
      changeSession(state, actions, session, session.archived ? 'restore' : 'archive');
    else if (session && action === 'stop')
      intent(state, actions, { kind: 'stop', sessionId }, 'agent', 'stop', { sessionId });
    else if (session && usage && true) openUsage(state, session);
    else if (session) {
      state.pages = [];
      if (state.selected?.id !== session.id) select(state, actions, session);
      if (details && !executionPending(state)) {
        state.inspector = 'details';
      }
      controlSidebar(state, 'dismiss_transient');
    }
  } else if (input.child === 'image-viewer' && input.name === 'closed') {
    state.image = null;
    state.followAttempt = null;
    follow(state, actions);
  } else if (
    input.child === readingKey(state) &&
    input.name === 'viewImage' &&
    state.selected &&
    state.window &&
    !state.loading
  ) {
    const sequence = (input.event as { sequence: number }).sequence;
    const chunk = state.window.page.chunks.find(
      (c) => c.sequence === sequence && c.tool?.result?.image,
    );
    if (chunk)
      state.image = {
        sessionId: state.selected.id,
        through: state.window.page.through,
        sequence: chunk.tool!.result!.sequence,
      };
  } else if (input.child === readingKey(state) && state.source) {
    const source = state.source;
    if (input.name === 'sourceBack') state.source = null;
    else if (!source.loading && input.name === 'sourceField') {
      const field = (input.event as { field: SourceField }).field;
      if (field !== source.field && source.fields.includes(field)) {
        source.field = field;
        source.ready = false;
        source.format = null;
        source.chunk = { ...source.chunk, offset: 0, totalBytes: 0, text: '' };
        readHistorySource(state, actions, { kind: 'start' });
      }
    } else if (!source.loading) {
      const chunk = source.chunk;
      const end = chunk.offset + utf8Bytes(chunk.text);
      const position: SourcePosition | null =
        input.name === 'sourceRetry' && source.failed
          ? source.position
          : input.name === 'sourceFirst' && chunk.offset > 0
            ? { kind: 'start' }
            : input.name === 'sourceLast' && end < chunk.totalBytes
              ? { kind: 'tail' }
              : input.name === 'sourcePrevious' && chunk.offset > 0
                ? { kind: 'before', offset: chunk.offset }
                : input.name === 'sourceNext' && end < chunk.totalBytes
                  ? { kind: 'after', offset: end }
                  : null;
      if (position) readHistorySource(state, actions, position);
    }
  } else if (input.child === readingKey(state) && !state.loading && state.window) {
    const page = state.window.page;
    if (
      input.name === 'markRead' &&
      state.selected?.hasUnread &&
      !state.selected.archived &&
      !mutationPending(state) &&
      conversationRoute(state) &&
      state.dialog === null &&
      state.inspector === null &&
      !state.source &&
      !state.image &&
      !state.searchOpen
    ) {
      const event = input.event as {
        sessionId: string;
        sequence: number;
        field: 'body' | 'thinking';
      };
      const chunk =
        event.sessionId === state.selected.id &&
        page.chunks.find(
          (chunk) =>
            chunk.sequence === event.sequence &&
            (chunk.field ?? 'body') === event.field &&
            (chunk.role === 'user' || chunk.role === 'assistant'),
        );
      if (chunk)
        intent(
          state,
          actions,
          { kind: 'markRead', sessionId: state.selected.id },
          'agent',
          'markRead',
          {
            sessionId: state.selected.id,
            messageId: chunk.messageId,
          },
        );
    } else if (input.name === 'prompt') {
      const sequence = (input.event as { sequence: number }).sequence;
      const visit = state.visits.find((entry) => entry.id === state.selected?.id);
      const prompt = visit?.prompt;
      if (
        page.chunks.some((chunk) => chunk.sequence === sequence && chunk.role === 'user') ||
        (prompt?.anchor === promptAnchor(page) && prompt.value?.source === sequence)
      ) {
        if (visit) visit.following = false;
        history(
          state,
          actions,
          { kind: 'after', cursor: { sequence, offset: 0, field: 'body' } },
          page.through,
          false,
        );
      }
    } else if (input.name === 'source') {
      const chunk = page.chunks.find(
        (chunk) =>
          chunk.sequence === (input.event as { sequence: number }).sequence &&
          (chunk.field ?? 'body') === ((input.event as { field?: string }).field ?? 'body'),
      );
      if (chunk)
        state.source = {
          through: page.through,
          chunk: {
            sequence: chunk.sequence,
            offset: chunk.offset,
            totalBytes: chunk.totalBytes,
            text: chunk.text,
          },
          navigation: ++state.navigation,
          landing: { sequence: state.navigation, edge: 'start' },
          loading: false,
          failed: false,
          field: chunk.field ?? 'body',
          fields: [chunk.field ?? 'body'],
          format: 'text',
          ready: true,
          position: { kind: 'after', offset: chunk.offset },
        };
    } else if (input.name === 'toolSource') {
      const event = input.event as { sequence: number; field: 'arguments' | 'result' };
      const chunk = page.chunks.find((chunk) => chunk.sequence === event.sequence);
      const reference =
        event.field === 'arguments'
          ? chunk?.tool?.call
          : event.field === 'result'
            ? chunk?.tool?.result
            : null;
      if (reference) {
        state.source = {
          through: page.through,
          chunk: { sequence: reference.sequence, offset: 0, totalBytes: 0, text: '' },
          navigation: state.navigation,
          landing: { sequence: state.navigation, edge: 'start' },
          loading: false,
          failed: false,
          field: event.field,
          fields: [],
          format: null,
          ready: false,
          position: { kind: 'start' },
        };
        readHistorySource(state, actions, { kind: 'start' });
      }
    } else if (
      input.name === 'fill' &&
      page.before &&
      state.window?.latest &&
      state.visits.find((visit) => visit.id === state.selected?.id)?.following !== false &&
      !Object.values(state.pending).some(
        (request) => request.kind === 'history' && request.epoch === state.epoch,
      )
    ) {
      history(
        state,
        actions,
        { kind: 'before', cursor: page.before },
        page.through,
        true,
        true,
        true,
      );
    } else if (input.name === 'older' && page.before) {
      // A short page is both at its start and end. Asking for older content
      // must suspend following even while that native viewport still fits.
      const visit = state.visits.find((entry) => entry.id === state.selected?.id);
      if (visit) visit.following = false;
      if (
        !Object.values(state.pending).some(
          (request) => request.kind === 'history' && request.epoch === state.epoch,
        )
      )
        history(
          state,
          actions,
          { kind: 'before', cursor: page.before },
          page.through,
          false,
          false,
          true,
        );
    } else if (
      input.name === 'newer' &&
      page.after &&
      !Object.values(state.pending).some(
        (request) => request.kind === 'history' && request.epoch === state.epoch,
      )
    )
      history(
        state,
        actions,
        { kind: 'after', cursor: page.after },
        page.through,
        false,
        false,
        true,
      );
    else if (input.name === 'latest')
      history(state, actions, { kind: 'tail' }, state.latestKnown, true);
  }

  return frame(presentation, configuredLocale, state, values, actions, events);
}
