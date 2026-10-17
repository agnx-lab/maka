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

import type { RowPosition } from '../../shared/history-position.js';
import { type AppearanceEditor } from '../../shared/appearance-editor.js';
import { type SettingsCategory } from '../../shared/settings.js';
import type {
  ModelSelection,
  AgentExecutorSelection,
  AgentQueue,
  AgentPendingInteractions,
  InteractionRecord,
  AgentQueueMessage,
  AgentHistoryPage,
  AgentHistoryChunk,
  SourceField,
  SourceFormat,
  SourcePosition,
  HistoryPosition,
  AgentPart,
  AgentRun,
  AgentSessionMetrics,
  AgentSession,
  AgentDataSource,
  AgentSessionPage,
  AgentList,
  AgentScheduleAlerts,
  AgentChecklist,
  AgentRecapReceipt,
  FileCapture,
  RecoveryRef,
  EditorCommand,
  EditorObservation,
  TerminalPaneCommand,
  ReadingCommand,
} from '../../../packages/plugin-sdk/src/index.js';
import type {
  AgentScreenProps,
  InsightsProps,
  PageChrome,
  SelectedModel,
  ModelSetup,
  TimelineFileContext,
  MessageCopyObservation,
} from '../../shared/contracts.js';
import { type Bookmark } from '../../shared/bookmark.js';
import { type Copy } from './locale.js';

import { type AttachmentDrafts } from '../../shared/attachments.js';

import { type ComposerReadiness } from '../../shared/drafts.js';

import type { HostLogRead } from './host-logs.js';
import type { DiagnosticsState, DiagnosticsRequest } from './diagnostics.js';

import type { CompactState, CompactRequest } from './compact.js';

import type { GoalState, GoalRequest } from './goal.js';
import type { PlanState, PlanRequest } from './plan.js';
import type { GraphState, GraphRequest } from './graph.js';

type BackendPreference = import('../../../packages/plugin-sdk/src/index.js').AgentBackendPreference;

type Notice = keyof Copy | null;

export type Request =
  | HostLogRead
  | DiagnosticsRequest
  | CompactRequest
  | { kind: 'sandboxChange'; epoch: number; sessionId: string }
  | GoalRequest
  | PlanRequest
  | GraphRequest
  | { kind: 'activity'; epoch: number; through: number; turnId: string; position: HistoryPosition }
  | {
      kind: 'readingPrompt';
      epoch: number;
      anchor: string;
    }
  | { kind: 'observe'; epoch: number }
  | { kind: 'recapRead'; epoch: number; attempt: number }
  | { kind: 'recapGenerate'; epoch: number; sessionId: string }
  | {
      kind: 'history';
      epoch: number;
      through: number;
      navigation: number;
      latest: boolean;
      implicit: boolean;
      extend?: boolean;
      position: HistoryPosition;
    }
  | {
      kind: 'source';
      epoch: number;
      navigation: number;
      through: number;
      rowPosition: RowPosition;
      field: SourceField;
      position: SourcePosition;
    }
  | { kind: 'interaction'; attempt: number; sessionId: string; requestId: string }
  | { kind: 'questionRead'; attempt: number; index: number; draft: EditorObservation }
  | { kind: 'respond'; attempt: number; sessionId: string; requestId: string }
  | { kind: 'openSession'; selection: NonNullable<AgentScreenProps['session']> }
  | { kind: 'changeSession'; sessionId: string; action: 'archive' | 'restore' | 'remove' }
  | { kind: 'selectBackend'; sessionId: string; backend: BackendPreference }
  | { kind: 'create' }
  | { kind: 'createDirectory'; title: string }
  | {
      kind: 'uploadAttachment';
      sessionId: string;
      dataSource: AgentDataSource;
      attempt: number;
      pasted?: FileCapture;
    }
  | { kind: 'markRead'; sessionId: string }
  | { kind: 'stop'; sessionId: string }
  | { kind: 'queue'; sessionId: string; saved?: { attempt: number; draft: EditorObservation } }
  | { kind: 'queueBody'; attempt: number }
  | { kind: 'queueRead'; attempt: number; draft: EditorObservation }
  | { kind: 'receipt' }
  | { kind: 'restorePage' }
  | { kind: 'restoreReceipt'; reference: RecoveryRef; cursor: number };

export type Mutation =
  | Extract<
      Request,
      {
        kind:
          | 'changeSession'
          | 'selectBackend'
          | 'sandboxChange'
          | 'create'
          | 'stop'
          | 'queue'
          | 'respond'
          | 'markRead'
          | 'uploadAttachment'
          | 'compact'
          | 'recapGenerate'
          | 'goalChange'
          | 'planChange'
          | 'planMode'
          | 'graphChange';
      }
    >
  | { kind: 'restored' };

export interface DisplayPart extends AgentPart {
  invocationId: string | null;
}

interface Uncertain {
  reference: RecoveryRef;
  request: Mutation;
}

interface QueueEdit {
  hasAttachments: boolean;
  attempt: number;
  target: AgentQueueMessage;
  phase: 'loading' | 'initializing' | 'ready' | 'failed';
  body: string | null;
  command: EditorCommand | null;
}

export type Page =
  | 'settings'
  | 'connections'
  | 'deployment'
  | 'projects'
  | 'imports'
  | 'apps'
  | 'recall'
  | 'scheduler';

export type Navigation = { pages: State['pages']; category: SettingsCategory };

export function navigationBookmark(state: State): Navigation {
  return { pages: state.pages, category: state.settingsCategory };
}

export function navigationKey(state: State): string {
  return (
    'navigation/' +
    (state.embedding.fixed ? (state.embedding.session?.id ?? 'pending') : 'agent-screen')
  );
}

// Bounded visited workspaces keep their original native reading owners. These
// are presentation snapshots only; run, queue and permission facts are refreshed.
export interface Visit {
  submissions?: (import('../../shared/contracts.js').ComposerSubmission & {
    attachmentNames: string[];
  })[];
  detached?: boolean;
  activityWanted?: number;
  prompt?: { anchor: string; value: { position: RowPosition; text: string } | null };
  activity?: {
    through: number;
    position: RowPosition | null;
    turnId: string;
    message: import('../../shared/contracts.js').Message | null;
  };
  id: string;
  dataSource: AgentDataSource;
  key: string;
  window: State['window'];
  live: State['live'];
  following: boolean;
  source: State['source'];
  image: State['image'];
  landing: State['landing'];
  searchOpen: boolean;
  browse: number | null;
  inspector: State['inspector'];
  output: string | null;
  outputFocus: ReadingCommand | null;
  session: AgentSession;
  epoch: number;
}

export function readingKey(state: State): string {
  return state.visits.find((visit) => visit.id === state.selected?.id)?.key ?? 'timeline-empty';
}

export interface State {
  visits: Visit[];
  bookmark: Bookmark<Navigation>;
  recoveryCheckpoint: Bookmark<{ through: number; scope: string | null }>;
  historyScope: string | null;
  restoreStarted: boolean;
  restoreThrough: number;
  recoveryDirty: boolean;
  settledOperations: string[];
  embedding: AgentScreenProps;
  externalSelection: AgentScreenProps['session'] | null;
  preferences: AppearanceEditor;
  drafts: ComposerReadiness[];
  attachments: AttachmentDrafts;
  revision: number;
  serial: number;
  epoch: number;
  catalog: AgentSessionPage | null;
  catalogGeneration: number;
  catalogWatch: { key: string; input: AgentList; loading: boolean } | null;
  catalogFilter: AgentList['filter'];
  catalogFailed: boolean;
  selected: AgentSession | null;
  model: SelectedModel | null;
  backendReady: boolean;
  backendPreference: BackendPreference | null;
  executor: AgentExecutorSelection | null;
  hasBackends: boolean;
  modelSetup: ModelSetup['kind'];
  modelOpenRequest: number | null;
  connectionSetupRequest: number | null;
  catalogEpoch: number;
  modelRefreshing: boolean;
  searchOpen: boolean;
  browse: number | null;
  pages: Page[];
  openedPages: Page[];
  pageChrome: Partial<Record<Page, PageChrome>>;
  pageBack: Partial<Record<Page, number>>;
  recallAnchor: RowPosition | null;
  scheduledTasks: boolean;
  web: boolean;
  jev: boolean;
  goal: GoalState;
  plan: PlanState;
  graph: GraphState;
  recap: {
    receipt: AgentRecapReceipt | null;
    loaded: boolean;
    loading: number | null;
    failed: boolean;
    confirmation: ModelSelection | null;
  };
  checklist: {
    watch: { key: string; input: { sessionId: string; offset: number } } | null;
    page: AgentChecklist | null;
    failed: boolean;
  };
  scheduleAlerts: AgentScheduleAlerts;
  scheduleWatch: string | null;
  scheduleRequested: { sequence: number; taskId: string } | null;
  settingsCategory: SettingsCategory;
  insightsScope: InsightsProps['scope'];
  sidebar: { open: boolean } | { command: TerminalPaneCommand | null };
  diagnostics: DiagnosticsState;
  hostLogsEnabled: boolean;
  inspector:
    | 'diagnostics'
    | 'details'
    | 'terminal'
    | 'outputs'
    | 'checklist'
    | 'recap'
    | 'goal'
    | 'plan'
    | 'graph'
    | null;
  terminals: boolean;
  skills: boolean;
  place: string;
  sessionChange: AgentSession | null;
  compact: CompactState | null;
  compacting: boolean;
  dialog:
    | 'compact'
    | 'sandbox'
    | 'session-change'
    | 'attachments'
    | 'terminal'
    | 'skills'
    | 'palette'
    | 'directories'
    | 'checklist'
    | 'recap'
    | 'goal'
    | 'plan'
    | 'graph'
    | 'diagnostics'
    | 'details'
    | 'help'
    | 'queue'
    | 'question'
    | 'approval'
    | null;
  window: {
    page: Extract<AgentHistoryPage, { kind: 'ready' }>;
    latest: boolean;
  } | null;
  navigation: number;
  image: { sessionId: string; through: number; position: RowPosition } | null;
  source: {
    through: number;
    chunk: Pick<AgentHistoryChunk, 'position' | 'offset' | 'totalBytes' | 'text'>;
    navigation: number;
    landing: { sequence: number; edge: 'start' | 'end' };
    loading: boolean;
    failed: boolean;
    field: SourceField;
    fields: SourceField[];
    format: SourceFormat | null;
    ready: boolean;
    position: SourcePosition;
  } | null;
  landing: { sequence: number; edge: 'start' | 'end' };
  tailReceipt: { landing: number; readingRevision: number } | null;
  fileContext: TimelineFileContext | null;
  copyRequest: number | null;
  messageCopy: MessageCopyObservation | null;
  followAttempt: {
    through: number;
    landing: number;
    readingRevision: number;
  } | null;
  parts: DisplayPart[];
  run: AgentRun | null;
  metrics: AgentSessionMetrics | null;
  queue: AgentQueue | null;
  queueEdit: QueueEdit | null;
  queueDraft: EditorObservation | null;
  interactions: AgentPendingInteractions | null;
  interaction: {
    attempt: number;
    sessionId: string;
    requestId: string;
    record: InteractionRecord | null;
    notice: Notice;
  } | null;
  questionSubmission: {
    attempt: number;
    answers: (string | null)[];
    waiting: number[];
  } | null;
  runThrough: number;
  loading: boolean;
  live: {
    sequence: number;
    messageId: string;
    invocationId: string;
    stepId: string;
    text: string;
    closedAt: number | null;
    field: 'body' | 'thinking';
  }[];
  latestKnown: number;
  watch: {
    key: string;
    input: { sessionId: string; after: number };
    last: number;
  } | null;
  pending: Record<string, Request>;
  uncertain: Uncertain | null;
  restoring: boolean;
  restoreAfter: number;
  notice: Notice;
}

export type RootCommand =
  | 'diagnostics-open'
  | 'browse'
  | 'outputs-open'
  | 'copy-file-path'
  | 'copy-message'
  | 'copy-source'
  | 'search-open'
  | 'help-open'
  | 'interaction-open'
  | 'create'
  | 'appearance-open'
  | 'model-setup'
  | 'terminal-open'
  | 'compact-open'
  | 'skills-open'
  | 'checklist-open'
  | 'recap-open'
  | 'goal-open'
  | 'plan-open'
  | 'graph-open'
  | 'details-open'
  | 'usage-open'
  | 'create-workspace'
  | 'navigation-toggle'
  | 'settings'
  | 'projects-open'
  | 'imports-open'
  | 'recall-open'
  | 'scheduler-open'
  | 'apps-open'
  | 'workhub-open'
  | 'deployment-open'
  | 'reload'
  | 'stop'
  | 'check';
