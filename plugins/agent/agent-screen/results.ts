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

import { readLanguage } from '../../shared/language-preference.js';
import { comparePosition, type RowPosition } from '../../shared/history-position.js';
import { compactQuery, compactSettled } from './compact.js';
import { hostLogResult } from './host-logs.js';
import { diagnosticResult } from './diagnostics.js';
import type {
  Action,
  AgentRecapReceipt,
  AgentGoalResult,
  AgentDeliveryPage,
  InteractionRecord,
  AgentInteractionResult,
  AgentQueueMessageResult,
  AgentHistoryPage,
  AgentHistorySourcePage,
  AgentObservation,
  AgentCreated,
  AgentMarkReadResult,
  AgentDirectoryPage,
  AgentAttachmentUploadResult,
  AgentQueueMutationResult,
  CallerIntent,
  OperationReply,
  TextChunk,
} from '../../../packages/plugin-sdk/src/index.js';
import { goalChanged, goalFieldResult } from './goal.js';
import { planChanged, planModeChanged, planHistoryResult } from './plan.js';
import { graphChanged, graphReadResult } from './graph.js';
import type { WorkHubGraphChanged } from '../../../packages/plugin-sdk/src/index.js';
import type {
  AgentPlanResult,
  AgentPlanHistory,
  AgentMetadataResult,
} from '../../../packages/plugin-sdk/src/index.js';
import { sameValue } from '../../shared/value.js';

import { sameFile } from '../../shared/attachments.js';

import { type Request, type Mutation, type DisplayPart, type State } from './state.js';
import { utf8Bytes, appendLive, closeLive, rememberActivity } from './messages.js';
import {
  intent,
  mutationPending,
  catalog,
  observe,
  history,
  follow,
  refreshActivity,
  readingPrompt,
  promptAnchor,
  select,
  restoreBackend,
  initializeQueueEdit,
  readInteraction,
  respondQuestion,
  closeDialog,
} from './actions.js';

function settled(state: State, actions: Action[], request: Mutation, value: unknown) {
  state.uncertain = null;
  if (request.kind === 'languageChange') {
    const result =
      value as import('../../../packages/plugin-sdk/src/index.js').SettingsPreferencesUpdated;
    state.notice = result.kind === 'committed' ? null : 'unavailable';
    readLanguage(state.language, actions);
  } else if (request.kind === 'restored') {
    readLanguage(state.language, actions);
    state.restoring = true;
    intent(state, actions, { kind: 'restorePage' }, 'history', 'page', {
      after: state.restoreAfter,
      limit: 1,
    });
    catalog(state);
  } else if (request.kind === 'compact') {
    compactSettled(state, actions, request.sessionId, value);
  } else if (request.kind === 'create') {
    const created = value as AgentCreated;
    if (created.kind === 'rejected') {
      if (state.notice !== 'unavailable') state.notice = 'createRejected';
      return;
    }
    // The live catalog may have selected this first conversation before its
    // original creation receipt arrived. Keep that observation and its owners.
    if (state.selected?.id !== created.session.id) select(state, actions, created.session);
    else if (created.session.revision >= state.selected.revision) state.selected = created.session;
    // Explicit creation reveals its selected row; background catalog changes
    // continue to retain the current directory location.
    if (
      state.catalogFilter !== 'all' ||
      !state.catalog?.sessions.some((session) => session.id === created.session.id)
    ) {
      state.catalogFilter = 'all';
      catalog(state, { kind: 'at', cursor: created.session.id });
    }
  } else if (request.kind === 'changeSession') {
    const result = value as import('../../../packages/plugin-sdk/src/index.js').AgentSessionChanged;
    if (result.kind === 'removed' || result.kind === 'changed') {
      if (state.sessionChange?.id === request.sessionId) closeDialog(state);
      const removedIndex =
        state.catalog?.sessions.findIndex((session) => session.id === request.sessionId) ?? 0;
      if (state.catalog)
        state.catalog.sessions = state.catalog.sessions.filter(
          (session) => session.id !== request.sessionId,
        );
      if (state.selected?.id === request.sessionId) {
        const next =
          state.catalog?.sessions[Math.max(0, removedIndex)] ??
          state.catalog?.sessions.at(-1) ??
          null;
        select(state, actions, next);
      }
      state.visits = state.visits.filter((visit) => visit.id !== request.sessionId);
      state.notice = null;
      catalog(state, { kind: 'start' });
    } else {
      state.notice =
        result.kind === 'rejected' && result.reason === 'busy'
          ? 'sessionChangeBusy'
          : 'sessionChangeFailed';
      catalog(state);
      if (state.selected?.id === request.sessionId) observe(state, actions);
    }
  } else if (request.kind === 'sandboxChange') {
    if (request.epoch === state.epoch && state.selected?.id === request.sessionId) {
      const result = value as AgentMetadataResult;
      if (result.kind === 'committed') {
        if (result.session.revision >= state.selected.revision) state.selected = result.session;
        if (state.dialog === 'sandbox') state.dialog = null;
        state.notice = null;
      } else {
        state.notice = 'sandboxSaveFailed';
        observe(state, actions);
      }
    }
  } else if (request.kind === 'selectBackend') {
    const result = value as AgentMetadataResult;
    if (state.selected?.id === request.sessionId) {
      if (result.kind === 'committed') {
        if (result.session.revision >= state.selected.revision) state.selected = result.session;
        state.notice = null;
      } else {
        state.notice = 'modelSaveFailed';
      }
    }
  } else if (request.kind === 'graphChange') {
    if (request.epoch === state.epoch && request.sessionId === state.selected?.id)
      graphChanged(state, value as WorkHubGraphChanged);
  } else if (request.kind === 'planChange' || request.kind === 'planMode') {
    if (request.epoch === state.epoch && request.sessionId === state.selected?.id) {
      if (request.kind === 'planChange') planChanged(state, value as AgentPlanResult);
      else planModeChanged(state, value as AgentMetadataResult);
    }
  } else if (request.kind === 'goalChange') {
    if (request.epoch === state.epoch && request.sessionId === state.selected?.id)
      goalChanged(state, value as AgentGoalResult);
  } else if (request.kind === 'recapGenerate') {
    if (request.epoch === state.epoch && request.sessionId === state.selected?.id)
      state.recap = {
        receipt: value as AgentRecapReceipt,
        loaded: true,
        loading: null,
        failed: false,
        confirmation: null,
      };
  } else if (request.kind === 'markRead') {
    const result = value as AgentMarkReadResult;
    if (result.kind === 'observed' && result.session.id === request.sessionId) {
      if (
        state.selected?.id === request.sessionId &&
        result.session.revision >= state.selected.revision
      )
        state.selected = result.session;
      if (state.catalog)
        state.catalog.sessions = state.catalog.sessions.map((session) =>
          session.id === request.sessionId && result.session.revision >= session.revision
            ? result.session
            : session,
        );
      state.notice = null;
    } else state.notice = 'unavailable';
  } else if (request.kind === 'queue') {
    const result = value as AgentQueueMutationResult;
    state.notice = result.kind === 'committed' ? null : 'queueChanged';
    if (
      result.kind === 'committed' &&
      request.saved &&
      state.queueEdit?.attempt === request.saved.attempt &&
      state.queueDraft?.snapshot.text.owner === request.saved.draft.snapshot.text.owner &&
      state.queueDraft.revision === request.saved.draft.revision
    )
      closeDialog(state);
  } else if (request.kind === 'respond') {
    const result = value as AgentInteractionResult;
    const question = state.interaction;
    if (question?.attempt === request.attempt) {
      if (
        result.kind === 'recorded' &&
        result.record.sessionId === request.sessionId &&
        result.record.requestId === request.requestId &&
        sameValue(result.record.request, question.record?.request)
      ) {
        question.record = result.record;
        question.notice = null;
      } else
        question.notice =
          result.kind === 'rejected' &&
          result.reason === 'invalid_answer' &&
          question.record?.request.kind === 'form'
            ? 'formInvalid'
            : 'interactionChanged';
    }
    if (state.selected?.id === request.sessionId) observe(state, actions);
  } else if (request.kind === 'uploadAttachment') {
    const result = value as AgentAttachmentUploadResult;
    if (result.kind === 'uploaded') {
      if (
        !state.attachments.files.some(
          (file) =>
            file.id === result.attachment.id &&
            file.sessionId === request.sessionId &&
            sameValue(file.dataSource, request.dataSource),
        )
      )
        state.attachments.files.push({ ...result.attachment, dataSource: request.dataSource });
      if (
        request.pasted &&
        state.attachments.paste &&
        sameFile(request.pasted, state.attachments.paste.observed.selection)
      ) {
        state.attachments.paste.command = { selection: request.pasted, action: 'next' };
      }
      if (!request.pasted && state.attachments.selection?.attempt === request.attempt) {
        if (state.dialog === 'attachments') state.dialog = null;
        state.attachments.selection = null;
      }
      state.notice = null;
    } else state.notice = 'attachmentFailed';
  } else {
    state.notice = 'stopping';
    if (state.selected?.id === request.sessionId) observe(state, actions);
  }
}

export function receipt(state: State, actions: Action[], request: Mutation, reply: OperationReply) {
  if (reply.receipt.kind === 'recorded' && reply.receipt.outcome.kind === 'returned') {
    const value = reply.receipt.outcome.value;
    const returned = value as AgentAttachmentUploadResult;
    if (
      request.kind === 'uploadAttachment' &&
      returned.kind === 'uploaded' &&
      returned.attachment.sessionId !== request.sessionId
    ) {
      state.uncertain = { reference: reply.reference, request };
      state.notice = 'unknown';
      return;
    }
    // Evicted entries still use their original journal; this only avoids repeat reads.
    state.settledOperations = [
      ...state.settledOperations.filter((id) => id !== reply.reference.operation),
      reply.reference.operation,
    ].slice(-32);
    state.recoveryDirty = true;
    settled(state, actions, request, value);
  } else {
    state.uncertain = { reference: reply.reference, request };
    state.notice = 'unknown';
    if (reply.reference.method === 'changeGraph')
      intent(
        state,
        actions,
        { kind: 'graphReceipt', reference: reply.reference },
        'workhub',
        'graphReceipt',
        { reference: reply.reference },
      );
    if (reply.reference.method === 'changePlan')
      intent(
        state,
        actions,
        { kind: 'planReceipt', reference: reply.reference },
        'agent',
        'planReceipt',
        { reference: reply.reference },
      );
    if (reply.reference.method === 'changeGoal')
      intent(
        state,
        actions,
        { kind: 'goalReceipt', reference: reply.reference },
        'agent',
        'goalReceipt',
        { reference: reply.reference },
      );
  }
}

export function queryFailed(state: State, request: Request) {
  if (request.kind === 'hostLogRead') {
    hostLogResult(state, [], request, null);
    return;
  }
  if (request.kind === 'diagnostics') {
    diagnosticResult(state, request, null);
    return;
  }
  if (
    request.kind === 'compactPrompt' ||
    request.kind === 'compactField' ||
    request.kind === 'compact'
  ) {
    if (state.compact?.sessionId === request.sessionId) state.compact.failed = true;
    return;
  }
  // A failed background checkpoint can be retried on the next confirmed result.
  if ((request.kind === 'restorePage' || request.kind === 'restoreReceipt') && !state.restoring) {
    state.restoreAfter = state.restoreThrough;
    return;
  }
  if (request.kind === 'graphChange' || request.kind === 'graphRead') {
    if (request.epoch === state.epoch) {
      state.graph.failed = true;
      if (request.kind === 'graphRead' && state.graph.loading === request.attempt)
        state.graph.loading = null;
    }
    return;
  }
  if (request.kind === 'graphReceipt') {
    state.notice = 'unknown';
    return;
  }
  if (
    request.kind === 'planChange' ||
    request.kind === 'planMode' ||
    request.kind === 'planHistory'
  ) {
    if (request.epoch === state.epoch) {
      state.notice = 'planUnavailable';
      if (request.kind === 'planHistory' && state.plan.history?.loading === request.attempt)
        state.plan.history.loading = null;
    }
    return;
  }
  if (request.kind === 'planReceipt') {
    state.notice = 'unknown';
    return;
  }

  if (request.kind === 'goalField' || request.kind === 'goalChange') {
    if (request.epoch === state.epoch) {
      state.goal.capture = null;
      state.notice = 'goalUnavailable';
    }
    return;
  }
  if (request.kind === 'goalReceipt') {
    state.notice = 'unknown';
    return;
  }
  if (request.kind === 'recapRead' || request.kind === 'recapGenerate') {
    if (
      request.epoch === state.epoch &&
      (request.kind === 'recapGenerate' || request.attempt === state.recap.loading)
    ) {
      state.recap.loading = null;
      state.recap.failed = true;
      state.recap.confirmation = null;
    }
    return;
  }
  if (
    request.kind === 'interaction' ||
    request.kind === 'questionRead' ||
    request.kind === 'respond'
  ) {
    if (state.interaction?.attempt === request.attempt)
      state.interaction.notice =
        request.kind === 'questionRead'
          ? 'questionInvalid'
          : request.kind === 'respond'
            ? 'interactionUnknown'
            : 'interactionUnavailable';
    if (state.questionSubmission?.attempt === request.attempt) state.questionSubmission = null;
    return;
  }
  if (request.kind === 'queueBody' || request.kind === 'queueRead') {
    if (state.queueEdit?.attempt === request.attempt) {
      if (request.kind === 'queueBody') state.queueEdit.phase = 'failed';
      state.notice = 'draftKept';
    }
    return;
  }
  if (request.kind === 'changeSession') {
    state.notice = 'sessionChangeFailed';
    return;
  }
  if (request.kind === 'sandboxChange') {
    if (request.epoch === state.epoch) state.notice = 'sandboxSaveFailed';
    return;
  }
  if (request.kind === 'selectBackend') {
    if (request.sessionId === state.selected?.id) state.notice = 'modelSaveFailed';
    return;
  }
  // Optional presentation work does not disable input or change execution facts.
  if (request.kind === 'history' && request.implicit) return;
  if (request.kind === 'source') {
    if (request.epoch === state.epoch && request.navigation === state.source?.navigation) {
      state.source.loading = false;
      state.source.failed = true;
    }
  } else if (request.kind === 'history' || request.kind === 'observe') {
    if (
      request.epoch === state.epoch &&
      (request.kind !== 'history' || request.navigation === state.navigation)
    ) {
      state.loading = false;
      state.notice = 'unavailable';
    }
  } else {
    if (request.kind === 'openSession') {
      if (sameValue(request.selection, state.externalSelection)) {
        state.loading = false;
        state.notice = 'unavailable';
      }
    } else if (request.kind === 'createDirectory') {
      if (state.notice !== 'unavailable') state.notice = 'createRejected';
    } else if (request.kind === 'restoreReceipt') {
      state.uncertain = { reference: request.reference, request: { kind: 'restored' } };
      state.restoring = false;
      state.notice = 'unknown';
    } else state.notice = request.kind === 'uploadAttachment' ? 'attachmentFailed' : 'unavailable';
  }
}

export function querySucceeded(state: State, actions: Action[], request: Request, value: unknown) {
  switch (request.kind) {
    case 'hostLogRead':
      hostLogResult(
        state,
        actions,
        request,
        value as import('../../../packages/plugin-sdk/src/index.js').HostLogPage,
      );
      break;
    case 'diagnostics':
      diagnosticResult(
        state,
        request,
        value as import('../../../packages/plugin-sdk/src/index.js').AgentDiagnosticPage,
      );
      break;
    case 'compactPrompt':
    case 'compactField':
      compactQuery(state, actions, request, value);
      break;
    case 'goalField':
      goalFieldResult(state, actions, request, value);
      break;
    case 'graphRead':
      graphReadResult(state, request, value);
      break;
    case 'planHistory':
      planHistoryResult(state, request, value as AgentPlanHistory);
      break;
    case 'graphReceipt':
    case 'planReceipt':
    case 'goalReceipt': {
      const original = state.uncertain;
      if (value && original && sameValue(original.reference, request.reference))
        settled(state, actions, original.request, value);
      else state.notice = 'unknown';
      break;
    }
    case 'recapRead': {
      if (request.epoch === state.epoch && request.attempt === state.recap.loading)
        state.recap = {
          receipt: value as AgentRecapReceipt | null,
          loaded: true,
          loading: null,
          failed: false,
          confirmation: null,
        };
      break;
    }
    case 'openSession': {
      if (!sameValue(request.selection, state.externalSelection)) break;
      const observed = value as AgentObservation;
      if (observed.session.id !== request.selection.id) {
        state.loading = false;
        state.notice = 'unavailable';
        break;
      }
      select(state, actions, observed.session);
      break;
    }
    case 'interaction': {
      const question = state.interaction,
        record = value as InteractionRecord;
      if (!question || question.attempt !== request.attempt) break;
      if (
        record.sessionId !== request.sessionId ||
        record.requestId !== request.requestId ||
        !['question', 'form', 'service_approval', 'permissions', 'client_capability'].includes(
          record.request.kind,
        ) ||
        (question.record && !sameValue(question.record.request, record.request))
      ) {
        question.notice = 'interactionUnavailable';
        break;
      }
      if (!question.record?.outcome) question.record = record;
      question.notice = null;
      break;
    }
    case 'questionRead': {
      const submission = state.questionSubmission,
        question = state.interaction;
      if (
        !submission ||
        !question ||
        submission.attempt !== request.attempt ||
        question.attempt !== request.attempt
      )
        break;
      const chunk = value as TextChunk;
      if (
        !sameValue(chunk.metadata, request.draft.snapshot.text) ||
        utf8Bytes(chunk.text) !== request.draft.snapshot.text.bytes ||
        !chunk.text.trim()
      ) {
        state.questionSubmission = null;
        question.notice = 'questionInvalid';
        break;
      }
      submission.answers[request.index] = chunk.text;
      submission.waiting = submission.waiting.filter((index) => index !== request.index);
      respondQuestion(state, actions);
      break;
    }
    case 'createDirectory': {
      const page = value as AgentDirectoryPage;
      if (page.kind !== 'roots' || !page.roots.length) {
        state.notice = 'createRejected';
        break;
      }
      intent(state, actions, { kind: 'create' }, 'agent', 'create', {
        title: request.title,
        automaticTitle: true,
        labels: [],
        workspace: { kind: 'directory', selection: page.roots[0].selection },
      });
      break;
    }
    case 'observe': {
      if (request.epoch !== state.epoch) break;
      const observation = value as AgentObservation;
      if (observation.session.id !== state.selected?.id) {
        state.loading = false;
        state.notice = 'unavailable';
        break;
      }
      if (observation.session.revision >= state.selected!.revision) {
        state.selected = observation.session;
        restoreBackend(state, observation.session, observation.rememberedBackend);
      }
      if (
        observation.through >= state.runThrough &&
        observation.session.revision >= state.selected!.revision
      ) {
        state.run = observation.run ?? null;
        state.compacting = observation.compacting ?? false;
        state.metrics = observation.metrics ?? null;
        state.runThrough = observation.through;
        if (!state.run && state.notice === 'stopping') state.notice = null;
      }
      if (!state.watch)
        state.parts = observation.parts.map((part) => ({
          ...part,
          invocationId: observation.run?.invocationId ?? null,
        }));
      state.latestKnown = Math.max(state.latestKnown, observation.through);
      // Watch/run facts are independent of the fixed-fence reading window.
      if (!state.watch) {
        const after = Math.min(
          observation.through,
          ...state.parts.map((part) => part.startSequence - 1),
        );
        state.watch = {
          key: 'watch-' + state.epoch,
          input: { sessionId: state.selected!.id, after },
          last: after,
        };
      }
      if (
        !state.window &&
        !Object.values(state.pending).some(
          (pending) => pending.kind === 'history' && pending.epoch === state.epoch,
        )
      ) {
        const anchor = state.recallAnchor;
        state.recallAnchor = null;
        history(
          state,
          actions,
          anchor === null ? { kind: 'tail' } : { kind: 'row', position: anchor, field: 'body' },
          observation.through,
          anchor === null,
        );
      } else if (
        !Object.values(state.pending).some(
          (pending) =>
            pending.kind === 'history' &&
            pending.epoch === state.epoch &&
            pending.navigation === state.navigation,
        )
      )
        state.loading = false;
      follow(state, actions);
      refreshActivity(state, actions);
      break;
    }
    case 'source': {
      const source = state.source;
      if (
        request.epoch !== state.epoch ||
        !source ||
        request.navigation !== source.navigation ||
        request.field !== source.field
      )
        break;
      const page = value as AgentHistorySourcePage;
      if (page.through !== request.through) {
        queryFailed(state, request);
        break;
      }
      const next = page.source;
      const end = next.offset + utf8Bytes(next.text);
      if (
        comparePosition(next.position, request.rowPosition) !== 0 ||
        next.path != null ||
        next.field !== request.field ||
        !next.fields.includes(next.field) ||
        (!next.text && next.totalBytes > 0) ||
        end > next.totalBytes ||
        (source.ready &&
          (next.totalBytes !== source.chunk.totalBytes || next.format !== source.format)) ||
        (request.position.kind === 'after' && next.offset !== request.position.offset) ||
        (request.position.kind === 'before' && end !== request.position.offset) ||
        (request.position.kind === 'start' && next.offset !== 0) ||
        (request.position.kind === 'tail' && end !== next.totalBytes)
      ) {
        queryFailed(state, request);
        break;
      }
      source.chunk = {
        ...source.chunk,
        position: next.position,
        offset: next.offset,
        totalBytes: next.totalBytes,
        text: next.text,
      };
      source.fields = next.fields;
      source.format = next.format;
      source.ready = true;
      source.loading = false;
      source.failed = false;
      source.landing = {
        sequence: request.navigation,
        edge:
          request.position.kind === 'before' || request.position.kind === 'tail' ? 'end' : 'start',
      };
      break;
    }
    case 'readingPrompt': {
      if (
        request.epoch !== state.epoch ||
        !state.window ||
        request.anchor !== promptAnchor(state.window.page)
      )
        break;
      const visit = state.visits.find((entry) => entry.id === state.selected?.id);
      if (visit)
        visit.prompt = {
          anchor: request.anchor,
          value: value as { position: RowPosition; text: string } | null,
        };
      break;
    }
    case 'activity': {
      if (request.epoch !== state.epoch || request.turnId !== state.run?.turnId) break;
      const page = value as AgentHistoryPage;
      if (page.through !== request.through) break;
      if (!page.chunks.length && page.before) {
        const position = { kind: 'before' as const, cursor: page.before };
        intent(state, actions, { ...request, position }, 'agent', 'history', {
          sessionId: state.selected!.id,
          through: request.through,
          position,
          includeThinking: true,
          view: 'conversation',
        });
        break;
      }
      rememberActivity(state, page);
      const wanted =
        state.visits.find((entry) => entry.id === state.selected?.id)?.activityWanted ?? 0;
      if (wanted > page.through) refreshActivity(state, actions, wanted);
      break;
    }
    case 'history': {
      if (request.epoch !== state.epoch || request.navigation !== state.navigation) break;
      if (
        request.implicit &&
        state.visits.find((visit) => visit.id === state.selected?.id)?.following === false
      )
        break;
      const page = value as AgentHistoryPage;
      if (page.through !== request.through) {
        queryFailed(state, request);
        break;
      }
      // A bounded canonical page may contain only non-text state rows. The
      // latest visible result can be on the preceding page, including a failed
      // turn with no assistant reply. Keep the same fence while finding it.
      if (request.latest && !request.extend && !page.chunks.length && page.before) {
        history(
          state,
          actions,
          { kind: 'before', cursor: page.before },
          page.through,
          true,
          request.implicit,
        );
        break;
      }
      if (request.latest) rememberActivity(state, page);
      const old = state.window;
      if (request.extend && !page.chunks.length) {
        const position =
          request.position.kind === 'before' && page.before
            ? { kind: 'before' as const, cursor: page.before }
            : request.position.kind === 'after' && page.after
              ? { kind: 'after' as const, cursor: page.after }
              : null;
        if (position) {
          history(state, actions, position, page.through, request.latest, request.implicit, true);
          break;
        }
      }
      if (request.extend && old && (old.page.through === page.through || request.implicit)) {
        const before = request.position.kind === 'before';
        const adjacent = before
          ? [...page.chunks, ...old.page.chunks]
          : [...old.page.chunks, ...page.chunks];
        const chunks = [
          ...new Map(
            adjacent.map(
              (chunk) =>
                [
                  `${chunk.position.sourceSequence}-${chunk.position.itemIndex}-${chunk.field}-${chunk.offset}`,
                  chunk,
                ] as const,
            ),
          ).values(),
        ].map((chunk) => {
          // Text segments are immutable; the recorded tool pair and Turn
          // boundary can advance at a newer fence. Use the Host's exact row
          // references rather than inferring a result from text or tool names.
          const current = page.chunks.find(
            (next) =>
              next.tool?.toolUseId === chunk.tool?.toolUseId &&
              ((next.tool?.call &&
                comparePosition(next.tool.call.position, chunk.position) === 0) ||
                (next.tool?.result &&
                  comparePosition(next.tool.result.position, chunk.position) === 0)),
          );
          return current ? { ...chunk, tool: current.tool } : chunk;
        });
        // Tool rows may have no body; their metadata still occupies the window.
        const size = (chunk: (typeof chunks)[number]) => utf8Bytes(JSON.stringify(chunk)) + 1;
        let bytes = chunks.reduce((total, chunk) => total + size(chunk), 0);
        let first = before ? page.before : old.page.before;
        let last = before ? old.page.after : page.after;
        // Filling spare viewport space must never evict the visible tail.
        if (before && request.implicit && bytes > 131072) break;
        // Cursor-adjacent immutable text keeps its original reading owner when
        // the live fence advances. Retain a bounded window around that anchor.
        while (bytes > 131072) {
          const removed = before ? chunks.pop()! : chunks.shift()!;
          bytes -= size(removed);
          if (before)
            last = { position: removed.position, offset: removed.offset, field: removed.field };
          else {
            const start = chunks[0]!;
            first = { position: start.position, offset: start.offset, field: start.field };
          }
        }
        state.window = {
          page: { ...page, chunks, before: first, after: last },
          latest: request.latest || last === null,
        };
      } else {
        state.window = { page, latest: request.latest };
        state.landing = {
          sequence: state.landing.sequence + 1,
          edge: request.position.kind === 'before' || request.latest ? 'end' : 'start',
        };
      }
      readingPrompt(state, actions, state.window.page);
      state.loading = false;
      if (!request.implicit) state.notice = null;
      state.live = state.live.filter(
        (fragment) =>
          fragment.sequence > page.through ||
          ((fragment.closedAt === null ||
            fragment.closedAt > page.through ||
            page.after !== null) &&
            !page.chunks.some(
              (chunk) =>
                chunk.messageId === fragment.messageId &&
                (chunk.field ?? 'body') === fragment.field,
            )),
      );
      state.followAttempt = null;
      follow(state, actions);
      break;
    }
    case 'queueBody': {
      const edit = state.queueEdit;
      if (!edit || edit.attempt !== request.attempt) break;
      const body = value as AgentQueueMessageResult;
      if (body.kind !== 'ready' || body.revision !== edit.target.expectedRevision) {
        edit.phase = 'failed';
        state.notice = 'queueChanged';
        break;
      }
      edit.body = body.message;
      edit.hasAttachments = body.hasAttachments;
      edit.phase = 'initializing';
      initializeQueueEdit(state);
      break;
    }
    case 'queueRead': {
      const edit = state.queueEdit;
      if (!edit || edit.attempt !== request.attempt || state.dialog !== 'queue') break;
      const chunk = value as TextChunk,
        captured = request.draft.snapshot.text;
      if (!sameValue(chunk.metadata, captured) || utf8Bytes(chunk.text) !== captured.bytes) {
        state.notice = 'draftKept';
        break;
      }
      if (!chunk.text.trim() && !edit.hasAttachments) {
        state.notice = 'empty';
        break;
      }
      intent(
        state,
        actions,
        {
          kind: 'queue',
          sessionId: edit.target.sessionId,
          saved: { attempt: edit.attempt, draft: request.draft },
        },
        'agent',
        'editQueue',
        {
          sessionId: edit.target.sessionId,
          expectedRevision: edit.target.expectedRevision,
          edit: { kind: 'update', messageId: edit.target.messageId, message: chunk.text },
        },
      );
      break;
    }
    case 'restorePage': {
      state.restoreStarted = true;
      // New work may have entered after this background page was requested.
      // Resume later instead of treating its still-running journal as unknown.
      if (!state.restoring && mutationPending(state)) {
        state.recoveryDirty = true;
        break;
      }
      const page = value as CallerIntent[];
      if (!page.length) {
        // Persist only a completely checked prefix. Unknown receipts never
        // advance this checkpoint; a lost checkpoint only repeats safe reads.
        state.restoreThrough = state.restoreAfter;
        if (state.restoring) {
          readLanguage(state.language, actions);
          if (state.notice === 'unknown') state.notice = null;
        }
        state.restoring = false;
        break;
      }
      const original = page[0];
      // History is scoped to the original caller. A bookmark from a replaced
      // consumer must never skip that consumer's unchecked operations.
      state.historyScope = JSON.stringify([
        original.caller.stateDomain,
        original.caller.deployment,
        original.consumer,
      ]);
      const saved = state.recoveryCheckpoint.saved;
      if (saved?.scope === state.historyScope && saved.through >= original.cursor) {
        state.restoreAfter = saved.through;
        state.restoreThrough = saved.through;
        intent(state, actions, { kind: 'restorePage' }, 'history', 'page', {
          after: saved.through,
          limit: 1,
        });
        break;
      }
      state.restoreAfter = original.cursor;
      if (state.settledOperations.includes(original.reference.operation)) {
        intent(state, actions, { kind: 'restorePage' }, 'history', 'page', {
          after: original.cursor,
          limit: 1,
        });
        break;
      }
      intent(
        state,
        actions,
        { kind: 'restoreReceipt', reference: original.reference, cursor: original.cursor },
        'receipts',
        'read',
        original.reference,
      );
      break;
    }
    case 'restoreReceipt': {
      const reply = value as OperationReply;
      if (
        !state.restoring &&
        (!sameValue(request.reference, reply.reference) ||
          reply.receipt.kind !== 'recorded' ||
          reply.receipt.outcome.kind !== 'returned')
      ) {
        // Other mounts may still be running. A background checkpoint grants
        // no completion and must not change the foreground's admission state.
        state.restoreAfter = state.restoreThrough;
        break;
      }
      if (!sameValue(request.reference, reply.reference)) {
        state.uncertain = { reference: request.reference, request: { kind: 'restored' } };
        state.notice = 'unknown';
        state.restoring = false;
        break;
      }
      if (reply.receipt.kind === 'recorded' && reply.receipt.outcome.kind === 'returned')
        intent(state, actions, { kind: 'restorePage' }, 'history', 'page', {
          after: request.cursor,
          limit: 1,
        });
      else {
        state.uncertain = { reference: request.reference, request: { kind: 'restored' } };
        state.notice = 'unknown';
        state.restoring = false;
      }
      break;
    }
    case 'receipt': {
      const original = state.uncertain;
      const reply = value as OperationReply;
      if (!original || !sameValue(original.reference, reply.reference)) {
        state.notice = 'unknown';
        break;
      }
      receipt(state, actions, original.request, reply);
      break;
    }
    default:
      state.notice = 'unknown';
  }
}

export function delivery(state: State, page: AgentDeliveryPage, actions: Action[]) {
  if (page.kind === 'interactions') {
    if (!state.interactions || page.pending.revision > state.interactions.revision) {
      const changed =
        !state.interactions || !sameValue(state.interactions.entries, page.pending.entries);
      state.interactions = page.pending;
      if (changed) observe(state, actions);
      const question = state.interaction;
      if (
        question?.record &&
        !question.record.outcome &&
        question.sessionId === state.selected?.id &&
        !page.pending.entries.some((entry) => entry.requestId === question.requestId)
      )
        readInteraction(state, actions);
    }
    return;
  }
  if (page.kind === 'queue') {
    if (!state.queue || page.queue.revision > state.queue.revision) state.queue = page.queue;
    return;
  }
  const watch = state.watch;
  if (!watch) return;
  state.latestKnown = Math.max(state.latestKnown, page.through);
  let canonicalChanged = false;
  for (const event of page.events) {
    if (event.sequence <= watch.last) continue;
    const fact = event.fact;
    if (
      [
        'opened',
        'executor_started',
        'transcript_changed',
        'part_finished',
        'step_ended',
        'ended',
      ].includes(fact.kind)
    )
      canonicalChanged = true;
    if (fact.kind === 'opened') {
      if (event.sequence > state.runThrough) {
        state.run = { invocationId: event.invocationId, turnId: event.turnId, state: 'running' };
        state.runThrough = event.sequence;
      }
    } else if (fact.kind === 'part_started' || fact.kind === 'executor_started') {
      const started: DisplayPart[] =
        fact.kind === 'executor_started'
          ? (['text', 'thinking'] as const).map((textKind) => ({
              startSequence: event.sequence,
              messageId: fact.messageId,
              stepId: event.invocationId,
              partId: textKind,
              textKind,
              invocationId: event.invocationId,
            }))
          : [
              {
                startSequence: event.sequence,
                messageId: fact.messageId,
                stepId: fact.stepId,
                partId: fact.partId,
                textKind: fact.textKind,
                invocationId: event.invocationId,
              },
            ];
      state.parts = state.parts.filter(
        (part) =>
          !started.some(
            (next) =>
              part.invocationId === next.invocationId &&
              part.stepId === next.stepId &&
              part.partId === next.partId,
          ),
      );
      if (state.parts.length + started.length > 128) {
        state.watch = null;
        state.notice = 'unavailable';
        return;
      }
      state.parts.push(...started);
    } else if (fact.kind === 'delta') {
      const part = state.parts.find(
        (part) =>
          part.invocationId === event.invocationId &&
          part.stepId === fact.stepId &&
          part.partId === fact.partId,
      );
      if (part) {
        appendLive(state, event.sequence, part, event.invocationId, fact.text);
      }
    } else if (fact.kind === 'part_finished') {
      state.parts = state.parts.filter(
        (part) =>
          part.invocationId !== event.invocationId ||
          part.stepId !== fact.stepId ||
          part.partId !== fact.partId,
      );
    } else if (fact.kind === 'step_ended') {
      closeLive(state, event.sequence, event.invocationId, fact.stepId);
      state.parts = state.parts.filter(
        (part) => part.invocationId !== event.invocationId || part.stepId !== fact.stepId,
      );
    } else if (fact.kind === 'ended') {
      closeLive(state, event.sequence, event.invocationId);
      state.parts = state.parts.filter((part) => part.invocationId !== event.invocationId);
      if (event.sequence > state.runThrough && state.run?.invocationId === event.invocationId) {
        state.run = null;
        state.runThrough = event.sequence;
        state.notice = fact.failed ? 'failed' : 'completed';
      }
    }
  }
  if (
    page.events.some(
      (event) =>
        event.sequence > watch.last && ['opened', 'step_ended', 'ended'].includes(event.fact.kind),
    )
  )
    observe(state, actions);
  watch.last = Math.max(watch.last, page.nextAfter);
  if (canonicalChanged) {
    follow(state, actions);
    refreshActivity(state, actions);
  }
}
