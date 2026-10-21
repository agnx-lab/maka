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

import { batch, cleanup, computed, effect, inputAction, signal, Text, untracked, View } from 'nsh';
import type { YNode } from 'nsh';
import { Button } from 'nsh/base';
import { List } from 'nsh/list';
import { AnimatedValue } from 'nsh/animated';
import {
  host,
  loadDraft,
  saveDraft,
  sendDraft,
  pendingSends,
  recoverSend,
  abandonSend,
  type ScopedHost,
} from './host';
import { C, Header } from './ui';
import { acceptedSend, historyMetadata, mergeChunks } from './history';
import type { Row, Position, Subscription } from './history';
import type { ComposerSession, SendAction } from './Composer';
import { sameRoute } from './modelSelection';
import type { BackendIdentity, ModelDetails } from './modelSelection';
import { MessageRow, type MessageViewState } from './MessageRow';
import { FinalActions } from './FinalActions';
import type { SessionAction } from './sessionLifecycle';
import { TurnPanel, TurnSummary, ActivityStatus, turnOutcome } from './TurnPanel';
import type { GroupViewState, LiveActivity, TurnViewState } from './TurnPanel';
import { projectProcessGroups, stableRowId } from './processGroups';
import type { ProcessEntry, ProcessGroup, TurnPresentation } from './processGroups';
import type { PresentationSettings } from './presentation';
import { PresentationControls } from './PresentationControls';
import { ReturnToLatest, TurnNavigation } from './TurnNavigation';
import type { TurnNavigationItem } from './TurnNavigation';
import type { ImagePreview } from './ImageThumbnail';
import { latestToolActivity, toolActivitySummary, updateToolActivity } from './toolActivity';
import type { ToolActivity } from './toolActivity';
import type {
  AgentHistoryAttachment,
  AgentHistoryLandmark,
} from '../../../../packages/plugin-sdk/src/index.js';
import {
  createAttachmentDraft,
  type AttachmentEdit,
  type DraftAttachment,
} from './attachmentDraft';

export type Control = (method: string, input: any) => Promise<any>;
export type ConversationHandle = ComposerSession & {
  activeTurn(): string | null;
  activityTurn(): string | null;
  flushDraft(): Promise<boolean>;
  focus(): void;
  jumpLatest(turnId: string | null): void;
  refresh(): Promise<any>;
  focusHistory(): void;
  focusActivity(turnId: string | null): void;
};
type ConversationProps = {
  client: ScopedHost;
  presentation: PresentationSettings;
  control: Control;
  fork(action: Extract<SessionAction, { kind: 'fork' }>): Promise<void>;
  commands(): void;
  browse(): void;
  files(): void;
  filesOpener(node: YNode): void;
  interaction(target: { sessionId: string; requestId: string }): void;
  previewImage(image: ImagePreview): void;
  attach(handle: ConversationHandle): void;
  detach(handle: ConversationHandle): void;
  focusComposer(): void;
};
type LivePart = {
  invocationId: string | null;
  stepId: string;
  partId: string;
  messageId: string;
  field: 'thinking' | 'body';
  startSequence: number;
};
function comparePosition(left: Position, right: Position): number {
  return left.sourceSequence - right.sourceSequence || left.itemIndex - right.itemIndex;
}
function receiptMessage(reply: any): string {
  const receipt = reply?.receipt;
  if (receipt?.kind === 'recorded' && receipt.outcome?.kind === 'returned') {
    const value = receipt.outcome.value;
    return value?.kind === 'rejected'
      ? `Send rejected: ${value.reason}`
      : 'Host returned an unexpected send result.';
  }
  return 'The original send has no confirmed result. Recover its receipt before sending again.';
}

export function Conversation(props: ConversationProps) {
  const client = props.client;
  const sessionId = client.sessionId;
  const rows = signal<Row[]>([]);
  const live = signal<Row[]>([]);
  const toolActivity = signal<ToolActivity[]>([]);
  const parts = signal<LivePart[]>([]);
  const pendingMessages = signal<(Row & { placement: 'queued' | 'transcript' })[]>([]);
  const title = signal(sessionId);
  const archived = signal(false);
  const run = signal('Loading session…');
  const invocation = signal<string | null>(null);
  const stoppingRun = signal<string | null>(null);
  const stopping = () => invocation() !== null && stoppingRun() === invocation();
  const workspace = signal('');
  const permissions = signal('');
  const runTurn = signal<string | null>(null);
  const backend = signal<any>(null);
  const interactions = signal<any>({ revision: -1, entries: [] });
  const error = signal('');
  const ready = signal(false);
  const transcriptStatus = signal<'loading' | 'live' | 'closing' | 'failed'>('loading');
  const before = signal<any>(null);
  const landmarks = signal<{ through: number; items: AgentHistoryLandmark[] }>({
    through: 0,
    items: [],
  });
  const awayFromLatest = signal(false);
  const visibleTurn = signal<string | null>(null);
  const navigatingTurn = signal<string | null>(null);
  const attachmentMetadataFailed = signal(false);
  const attachmentMetadataReads = new Set<string>();
  const draft = signal('');
  const draftReady = signal(false);
  const savingFailed = signal(false);
  const sending = signal(false);
  const unresolved = signal<any[]>([]);
  const receiptsKnown = signal(false);
  const recovering = signal(false);
  const notice = signal('');
  const reading = signal<'chat' | 'trajectory'>('chat');
  const readingSize = signal({ width: 640, height: 192 });
  const innerReader = signal<string | null>(null);
  const metrics = signal<any>(null);
  const turnStates = new Map<string, TurnViewState>();
  const groupStates = new Map<string, GroupViewState>();
  const rowStates = new Map<string, MessageViewState>();
  const turnState = (id: string) => {
    let state = turnStates.get(id);
    if (!state) {
      state = { expanded: signal<{ open: boolean; answerKey: string | undefined } | null>(null) };
      turnStates.set(id, state);
    }
    return state;
  };
  const groupState = (key: string) => {
    let state = groupStates.get(key);
    if (!state) {
      state = {
        open: signal(false),
        following: signal(false),
        offset: new AnimatedValue(0),
        measured: new Map(),
        lease: 0,
      };
      groupStates.set(key, state);
    }
    return state;
  };
  const rowState = (id: string) => {
    let state = rowStates.get(id);
    if (!state) {
      state = { expanded: signal(false), sourceMode: signal(false) };
      rowStates.set(id, state);
    }
    return state;
  };
  const activity = computed<LiveActivity>(() => {
    if (run() === 'waiting_for_user') return { kind: 'waiting', detail: '' };
    let latest: { part: LivePart; row: Row; sequence: number } | undefined;
    for (const part of parts()) {
      if (part.invocationId !== invocation()) continue;
      const matches = (row: Row) => row.messageId === part.messageId && row.field === part.field;
      const row = live().find(matches) ?? rows().find(matches);
      if (!row || row.closedAt != null) continue;
      const sequence = Math.max(part.startSequence, row.sequence ?? 0);
      if (!latest || sequence > latest.sequence) latest = { part, row, sequence };
    }
    if (!latest) return { kind: 'working', detail: '' };
    if (latest.part.field === 'body')
      return { kind: 'response', detail: '', rowId: stableRowId(latest.row) };
    const paragraph = latest.row.text
      .split(/\r?\n[\t ]*\r?\n/)
      .findLast((paragraph) => paragraph.trim() !== '');
    return {
      kind: 'thinking',
      rowId: stableRowId(latest.row),
      detail: (paragraph ?? '').replaceAll('**', '').replace(/\s+/g, ' ').trim(),
    };
  });
  const reasoningPreview = (id: string) =>
    props.presentation.policy().settledReasoningPreview ||
    parts().some(
      (part) =>
        part.invocationId === invocation() &&
        part.field === 'thinking' &&
        `${part.messageId}/${part.field}` === id,
    );
  let previousGroups: ProcessEntry[] = [];
  const projection = computed(() => {
    // Canonical position and live PartStarted sequence are source order. A send
    // echo has no source position yet and stays at the local tail until admitted.
    const order = (row: Row) =>
      row.position?.sourceSequence ?? row.startSequence ?? row.sequence ?? 0;
    const ordered = [...rows(), ...live()].sort((left, right) => {
      return (
        order(left) - order(right) ||
        (left.position?.itemIndex ?? 0) - (right.position?.itemIndex ?? 0) ||
        Number(left.field === 'body') - Number(right.field === 'body')
      );
    });
    const next = projectProcessGroups({
      rows: [...ordered, ...pendingMessages().filter((row) => row.placement === 'transcript')],
      toolActivity: toolActivity(),
      activeTurnId: invocation() === null ? null : runTurn(),
      previous: previousGroups,
    });
    previousGroups = next.entries;
    return next;
  });
  const latestExchangeTurns = computed(() => {
    const latest = new Set<string>();
    // This is logical exchange membership, never whether the reader is at
    // the viewport bottom. Queued receipt echoes do not start an exchange.
    if (pendingMessages().some((row) => row.placement === 'transcript')) return latest;
    for (const entry of projection().entries) {
      if (entry.kind === 'process') latest.add(entry.turnId);
      else {
        const row = projection().rowsById.get(entry.rowId);
        if (row?.role === 'user') latest.clear();
        else if (row?.turnId) latest.add(row.turnId);
      }
    }
    return latest;
  });
  const latestExchange = (turnId: string) => latestExchangeTurns().has(turnId);
  const canFoldTurn = (turn: TurnPresentation) =>
    turnOutcome(turn) === 'completed' &&
    !turn.active &&
    !turn.interleavedInput &&
    props.presentation.policy().foldCompletedTurns;
  const turnExpanded = (turn: TurnPresentation) => {
    if (!canFoldTurn(turn)) return true;
    const stored = turnState(turn.turnId).expanded();
    if (stored && stored.answerKey === turn.answerKey) return stored.open;
    return props.presentation.collapseTiming() === 'next-input' && latestExchange(turn.turnId);
  };
  const grouped = (turn: TurnPresentation) => {
    const policy = props.presentation.policy();
    return (
      policy.stepGrouping === 'collapsed' ||
      (policy.stepGrouping === 'history' &&
        !turn.active &&
        !(policy.collapseTiming === 'next-input' && latestExchange(turn.turnId)))
    );
  };
  type DisplayEntry =
    | { kind: 'summary'; key: string; turnId: string }
    | { kind: 'process'; key: string; group: ProcessGroup }
    | { kind: 'row'; key: string; rowId: string }
    | { kind: 'activity'; key: string; group: ProcessGroup };
  const display = computed<DisplayEntry[]>(() => {
    const result: DisplayEntry[] = [];
    const summaries = new Set<string>();
    for (const entry of projection().entries) {
      const row = entry.kind === 'message' ? projection().rowsById.get(entry.rowId) : undefined;
      const turnId = entry.kind === 'process' ? entry.turnId : row?.turnId;
      const turn = turnId ? projection().turns.get(turnId) : undefined;
      if (
        turn &&
        (entry.kind === 'process' || row?.role === 'assistant') &&
        !summaries.has(turn.turnId) &&
        turnOutcome(turn) !== null
      ) {
        summaries.add(turn.turnId);
        result.push({ kind: 'summary', key: `turn/${turn.turnId}`, turnId: turn.turnId });
      }
      if (entry.kind === 'process') {
        if (!turn || !turnExpanded(turn)) continue;
        if (grouped(turn)) result.push({ kind: 'process', key: entry.key, group: entry });
        else {
          // Flat members are native outer List seats, preserving virtualization.
          for (const rowId of entry.rowIds) result.push({ kind: 'row', key: rowId, rowId });
          if (entry.members.some((member) => member.kind === 'activity'))
            result.push({ kind: 'activity', key: `activity/${entry.key}`, group: entry });
        }
      } else if (row) {
        if (entry.processTurnId && turn && !turnExpanded(turn)) continue;
        if (
          reading() === 'trajectory' &&
          (row.role === 'user' ||
            row.finalOutput ||
            row.role === 'compaction' ||
            (!row.turnId && row.role !== 'failure'))
        )
          continue;
        result.push({ kind: 'row', key: entry.key, rowId: entry.rowId });
      }
    }
    for (const row of pendingMessages()) {
      if (row.placement === 'queued' && reading() === 'chat')
        result.push({ kind: 'row', key: stableRowId(row), rowId: stableRowId(row) });
    }
    return result;
  });
  let previouslyVisible = new Set<string>();
  effect(() => {
    const visible = new Set<string>();
    for (const turn of projection().turns.values()) {
      if (turnExpanded(turn)) visible.add(turn.turnId);
      else if (previouslyVisible.has(turn.turnId)) {
        for (const entry of projection().entries) {
          if (entry.kind === 'process' && entry.turnId === turn.turnId)
            groupStates.get(entry.key)?.open.set(false);
        }
      }
    }
    previouslyVisible = visible;
  });
  const displayByKey = computed(() => new Map(display().map((item) => [item.key, item])));
  const displayKeys = computed(() => display().map((item) => item.key));
  const navigationItems = computed<TurnNavigationItem[]>(() => {
    const items = new Map<string, TurnNavigationItem>();
    for (const item of landmarks().items) {
      items.set(item.turnId, {
        turnId: item.turnId,
        label: item.label,
        anchor: {
          kind: 'landmark',
          through: landmarks().through,
          position: item.position,
          lastPosition: item.lastPosition,
        },
      });
    }
    for (const row of rows()) {
      if (!row.turnId || !row.position) continue;
      const previous = items.get(row.turnId);
      const label =
        row.role === 'user'
          ? Array.from(row.text.replace(/\s+/g, ' ').trim()).slice(0, 96).join('')
          : '';
      if (!previous)
        items.set(row.turnId, {
          turnId: row.turnId,
          label,
          anchor: { kind: 'loaded', position: row.position },
        });
      else if (!previous.label && label) items.set(row.turnId, { ...previous, label });
    }
    return [...items.values()].sort((left, right) =>
      comparePosition(left.anchor.position, right.anchor.position),
    );
  });
  const entryTurn = (key: string) => {
    const entry = displayByKey().get(key);
    if (entry?.kind === 'summary') return entry.turnId;
    if (entry?.kind === 'process' || entry?.kind === 'activity') return entry.group.turnId;
    return entry?.kind === 'row' ? (projection().rowsById.get(entry.rowId)?.turnId ?? null) : null;
  };
  let list: YNode | undefined;
  let alive = true;
  let subscription: Subscription | null = null;
  let transcriptTask: Promise<void> | null = null;
  let wakeReconnect: (() => void) | null = null;
  let readFailure: { stream: Subscription; reason: unknown } | null = null;
  let refreshFrom: { position: Position; field: string } | null = null;
  let latestKnown = 0;
  let watchLast = 0;
  let runThrough = 0;
  let followTask: Promise<void> | null = null;
  let followAgain = false;
  let olderTask: Promise<boolean> | null = null;
  let landmarksReadingThrough = -1;
  let turnJump: TurnNavigationItem | null = null;
  let draftRevision = 0;
  let editEpoch = 0;
  let pendingDraft: (AttachmentEdit & { text: string }) | null = null;
  let saveTask: Promise<void> | null = null;
  const attachments = createAttachmentDraft({
    sessionId,
    client,
    focus: props.focusComposer,
    changed() {
      if (!alive || !draftReady()) return;
      editEpoch += 1;
      pendingDraft = { text: draft(), ...attachments.snapshot() };
      startSave();
    },
  });

  const report = (reason: unknown) => {
    if (alive) error.set(reason instanceof Error ? reason.message : String(reason));
  };
  const covered = () => new Set(rows().map((row) => `${row.messageId}/${row.field}`));
  const publishHistory = (chunks: any[], through: number, older = false) => {
    batch(() => {
      rows.set(mergeChunks(rows(), chunks, older, through));
      const canonical = covered();
      live.set(live().filter((row) => !canonical.has(`${row.messageId}/${row.field}`)));
      pendingMessages.set(
        pendingMessages().filter((row) => !canonical.has(`${row.messageId}/${row.field}`)),
      );
    });
    hydrateAttachmentMetadata();
  };
  const requestMetadata = (row: { position?: Position; field: string } | undefined) => {
    if (!row?.position) return;
    const next = { position: row.position, field: row.field };
    const order = (a: typeof next, b: typeof next) =>
      a.position.sourceSequence - b.position.sourceSequence ||
      a.position.itemIndex - b.position.itemIndex ||
      Number(a.field === 'body') - Number(b.field === 'body');
    if (refreshFrom === null || order(next, refreshFrom) < 0) refreshFrom = next;
    followAgain = true;
  };
  const publishMetadata = (chunks: any[], through: number) => {
    const metadata = new Map(
      chunks.map((chunk) => [
        `${chunk.position.sourceSequence}/${chunk.position.itemIndex}/${chunk.field}`,
        chunk,
      ]),
    );
    rows.set(
      rows().map((row) => {
        const chunk = metadata.get(row.key);
        return chunk && through >= (row.metadataThrough ?? 0)
          ? { ...row, ...historyMetadata(chunk, through, row) }
          : row;
      }),
    );
    if (!rows().some((row) => row.role === 'user' && row.attachments === undefined))
      attachmentMetadataFailed.set(false);
    hydrateAttachmentMetadata();
  };
  const history = async (through: number, position: any) => {
    const page = await host.query('agent', 'history', {
      sessionId,
      through,
      position,
      includeThinking: true,
      view: 'conversation',
    });
    if (page.through !== through) throw new Error('History changed its requested fence.');
    return page;
  };
  const hydrateAttachmentMetadata = () => {
    for (const row of rows()) {
      if (
        row.role !== 'user' ||
        row.field !== 'body' ||
        !row.position ||
        row.attachments !== undefined
      )
        continue;
      const through = row.metadataThrough!;
      const key = `${row.key}/${through}`;
      if (attachmentMetadataReads.has(key)) continue;
      attachmentMetadataReads.add(key);
      const position = { ...row.position };
      // A tail fragment can omit the first-fragment metadata. Read just that
      // row's first page; its text never changes the loaded window or anchor.
      void history(through, { kind: 'row', position, field: 'body' })
        .then((page) => {
          if (!alive) return;
          const current = rows().find((candidate) => candidate.key === row.key);
          if (
            !current ||
            current.messageId !== row.messageId ||
            current.metadataThrough !== through
          )
            return;
          const first = page.chunks.find(
            (chunk: any) =>
              chunk.position.sourceSequence === position.sourceSequence &&
              chunk.position.itemIndex === position.itemIndex &&
              chunk.field === 'body',
          );
          if (
            !first ||
            first.offset !== 0 ||
            first.messageId !== row.messageId ||
            first.role !== 'user'
          )
            throw new Error('The original attachment metadata could not be verified.');
          publishMetadata([first], through);
        })
        .catch((reason) => {
          if (
            alive &&
            rows().some(
              (current) =>
                current.key === row.key &&
                current.messageId === row.messageId &&
                current.metadataThrough === through &&
                current.attachments === undefined,
            )
          ) {
            attachmentMetadataFailed.set(true);
            report(reason);
          }
        })
        .finally(() => attachmentMetadataReads.delete(key));
    }
  };
  const retryAttachmentMetadata = () => {
    if (!alive) return;
    attachmentMetadataFailed.set(false);
    hydrateAttachmentMetadata();
  };
  const readLandmarks = (through: number) => {
    if (through <= landmarksReadingThrough) return;
    landmarksReadingThrough = through;
    void host
      .query('agent', 'historyLandmarks', { sessionId, through })
      .then((page) => {
        if (page.through !== through)
          throw new Error('Turn landmarks changed their requested fence.');
        if (alive && page.through >= landmarks().through)
          landmarks.set({ through: page.through, items: page.landmarks });
      })
      .catch((reason) => {
        if (!alive || through < landmarks().through) return;
        if (landmarksReadingThrough === through) landmarksReadingThrough = -1;
        report(reason);
      });
  };
  const observe = async () => {
    const observed = await host.query('agent', 'observe', { sessionId });
    if (!alive) return observed;
    if (observed.session.id !== sessionId) throw new Error('Host observed a different session.');
    latestKnown = Math.max(latestKnown, observed.through);
    readLandmarks(observed.through);
    title.set(observed.session.title || 'Untitled conversation');
    archived.set(observed.session.archived);
    workspace.set(observed.session.workspace.path);
    permissions.set(
      (
        {
          read_only: 'Read only',
          workspace_write: 'Workspace write',
          danger_full_access: 'Full access',
        } as any
      )[observed.session.permissions.access],
    );
    backend.set(observed.session.backend ?? observed.rememberedBackend ?? null);
    metrics.set(observed.metrics ?? null);
    if (observed.through >= runThrough) {
      run.set(observed.run?.state ?? 'Idle');
      invocation.set(observed.run?.invocationId ?? null);
      runTurn.set(observed.run?.turnId ?? null);
      if (!observed.run) stoppingRun.set(null);
      runThrough = observed.through;
    }
    return observed;
  };

  // Facts may request another pass while the single canonical-history reader
  // is awaiting a page. The newest boundary fragment is re-read and replaced.
  const follow = (): Promise<void> => {
    followAgain = true;
    if (followTask) return followTask;
    const stream = subscription;
    followTask = (async () => {
      do {
        followAgain = false;
        const fence = latestKnown;
        const last = rows().at(-1);
        let position: any = last
          ? {
              kind: 'after',
              cursor: {
                position: last.position,
                field: last.field,
                offset: last.fragments!.at(-1)!.offset,
              },
            }
          : { kind: 'tail' };
        for (;;) {
          const page = await history(fence, position);
          if (!alive) return;
          publishHistory(page.chunks, page.through);
          if (page.after === null) break;
          position = { kind: 'after', cursor: page.after };
        }
        live.set(live().filter((row) => row.closedAt === null || row.closedAt! > fence));
        const refresh = refreshFrom;
        refreshFrom = null;
        if (refresh !== null) {
          const metadataFence = latestKnown;
          // Completion changes metadata on earlier multipart output as well.
          // Never merge these newly budgeted fragments into already loaded text.
          let cursor: any = { kind: 'row', position: refresh.position, field: refresh.field };
          try {
            for (;;) {
              const page = await history(metadataFence, cursor);
              if (!alive) return;
              publishMetadata(page.chunks, page.through);
              if (page.after === null) break;
              cursor = { kind: 'after', cursor: page.after };
            }
          } catch (reason) {
            requestMetadata(refresh);
            throw reason;
          }
        }
      } while (alive && followAgain);
    })()
      .catch((reason) => {
        followAgain = false;
        if (alive && stream !== null && subscription === stream) {
          readFailure = { stream, reason };
          transcriptStatus.set('closing');
          void stream.close().catch(report);
        }
        throw reason;
      })
      .finally(() => {
        followTask = null;
        if (alive && followAgain) void follow().catch(report);
      });
    return followTask;
  };

  const openParts = (started: LivePart[]) => {
    const same = (a: LivePart, b: LivePart) =>
      a.invocationId === b.invocationId && a.stepId === b.stepId && a.partId === b.partId;
    parts.set([
      ...parts().filter((part) => !started.some((other) => same(part, other))),
      ...started,
    ]);
  };
  const closeLive = (sequence: number, invocationId: string, stepId?: string) => {
    live.set(
      live().map((row) =>
        row.invocationId === invocationId && (stepId === undefined || row.stepId === stepId)
          ? { ...row, closedAt: sequence }
          : row,
      ),
    );
  };
  const deliver = (page: any) => {
    if (page.kind === 'interactions') {
      if (page.pending.revision >= interactions().revision) interactions.set(page.pending);
      return;
    }
    if (page.kind !== 'events') return;
    latestKnown = Math.max(latestKnown, page.through);
    let changed = false;
    for (const event of page.events) {
      if (event.sequence <= watchLast) continue;
      const fact = event.fact;
      const nextTools = updateToolActivity(toolActivity(), event);
      if (nextTools !== toolActivity()) toolActivity.set(nextTools);
      const field = (kind: string) => (kind === 'thinking' ? 'thinking' : 'body');
      switch (fact.kind) {
        case 'opened':
          if (event.sequence > runThrough) {
            run.set('running');
            invocation.set(event.invocationId);
            runTurn.set(event.turnId);
            requestMetadata(rows().find((row) => row.turnId === event.turnId));
            void observe().catch(report);
            runThrough = event.sequence;
          }
          changed = true;
          break;
        case 'executor_started':
          openParts(
            ['text', 'thinking'].map((textKind) => ({
              invocationId: event.invocationId,
              stepId: event.invocationId,
              partId: textKind,
              messageId: fact.messageId,
              field: field(textKind),
              startSequence: event.sequence,
            })),
          );
          changed = true;
          break;
        case 'part_started':
          openParts([
            {
              invocationId: event.invocationId,
              stepId: fact.stepId,
              partId: fact.partId,
              messageId: fact.messageId,
              field: field(fact.textKind),
              startSequence: event.sequence,
            },
          ]);
          break;
        case 'delta': {
          const part = parts().find(
            (part) =>
              part.invocationId === event.invocationId &&
              part.stepId === fact.stepId &&
              part.partId === fact.partId,
          );
          if (!part || covered().has(`${part.messageId}/${part.field}`)) break;
          const key = `live/${part.messageId}/${part.field}`;
          const previous = live().find((row) => row.key === key);
          if (previous?.sequence !== undefined && event.sequence <= previous.sequence) break;
          const next = {
            ...part,
            invocationId: part.invocationId ?? undefined,
            key,
            role: part.field === 'thinking' ? 'thinking' : 'assistant',
            turnId: event.turnId,
            text: (previous?.text ?? '') + fact.text,
            sequence: event.sequence,
            closedAt: null,
          };
          live.set(
            previous ? live().map((row) => (row.key === key ? next : row)) : [...live(), next],
          );
          break;
        }
        case 'part_finished':
          parts.set(
            parts().filter(
              (part) =>
                part.invocationId !== event.invocationId ||
                part.stepId !== fact.stepId ||
                part.partId !== fact.partId,
            ),
          );
          changed = true;
          break;
        case 'step_ended':
          closeLive(event.sequence, event.invocationId, fact.stepId);
          parts.set(
            parts().filter(
              (part) => part.invocationId !== event.invocationId || part.stepId !== fact.stepId,
            ),
          );
          changed = true;
          break;
        case 'ended':
          requestMetadata(rows().find((row) => row.turnId === event.turnId));
          closeLive(event.sequence, event.invocationId);
          parts.set(parts().filter((part) => part.invocationId !== event.invocationId));
          if (event.sequence > runThrough) {
            run.set(fact.failed ? 'Failed' : 'Idle');
            invocation.set(null);
            runTurn.set(null);
            stoppingRun.set(null);
            void observe().catch(report);
            runThrough = event.sequence;
          }
          changed = true;
          break;
        case 'tool_input_ready':
        case 'tool_dispatched':
        case 'tool_rejected':
        case 'tool_settled':
        case 'provider_tool_result':
        case 'executor_tool_start':
        case 'executor_tool_result':
        case 'transcript_changed':
          changed = true;
          break;
      }
    }
    watchLast = Math.max(watchLast, page.nextAfter);
    if (changed) void follow().catch(report);
  };

  const openTranscript = async () => {
    if (followTask) await followTask.catch(() => {});
    if (!alive) return;
    // A turn may have ended while its watch was offline. Earlier accepted
    // fragments need the same completion metadata refresh as an ended event.
    const previousTurn = runTurn();
    if (previousTurn !== null) requestMetadata(rows().find((row) => row.turnId === previousTurn));
    const observed = await observe();
    if (!alive) return;
    parts.set(
      observed.parts.map((part: any) => ({
        ...part,
        invocationId: observed.run?.invocationId ?? null,
        field: part.textKind === 'thinking' ? 'thinking' : 'body',
      })),
    );
    const after = Math.min(
      observed.through,
      ...observed.parts.map((part: any) => part.startSequence - 1),
      ...observed.toolSeeds.map((tool: any) => tool.startSequence - 1),
    );
    const stream = await host.subscribe('agent', 'watch', { sessionId, after });
    if (!alive) {
      await stream.close();
      return;
    }
    subscription = stream;
    watchLast = after;
    let failure: { reason: unknown } | null = null;
    try {
      if (ready()) {
        // Fill the gap from the existing canonical tail, retaining loaded
        // history and the native reader anchor instead of jumping to the end.
        await follow();
      } else {
        let position: any = { kind: 'tail' };
        for (;;) {
          const page = await history(observed.through, position);
          if (!alive) return;
          if (page.chunks.length === 0 && page.before !== null) {
            position = { kind: 'before', cursor: page.before };
            continue;
          }
          before.set(page.before);
          publishHistory(page.chunks, page.through);
          ready.set(true);
          list?.scrollToEnd({ animated: false });
          break;
        }
      }
      if (!alive) return;
      transcriptStatus.set('live');
      error.set('');
      // Subscribe before reading history so facts cannot fall between fences.
      // The snapshot is sufficient for readiness; no new event is required.
      for (;;) {
        const page = await stream.next();
        if (!alive) return;
        if (page === null) throw new Error('Live updates ended.');
        batch(() => deliver(page));
      }
    } catch (reason) {
      failure = { reason: readFailure?.stream === stream ? readFailure.reason : reason };
      throw failure.reason;
    } finally {
      if (alive) transcriptStatus.set('closing');
      try {
        await stream.close();
      } catch (reason) {
        throw new Error(
          `${failure ? String(failure.reason) : 'Observation stopped'}; original observation cleanup remains unconfirmed: ${String(reason)}`,
        );
      } finally {
        // This replaces only the read-only JS observer. Native Client retains
        // unconfirmed Request custody, within its existing limit, until the
        // original session close acknowledges cleanup. No mutation is replayed.
        if (subscription === stream) subscription = null;
        if (readFailure?.stream === stream) readFailure = null;
      }
    }
  };
  const startTranscript = () => {
    if (!alive || transcriptTask !== null) return;
    transcriptStatus.set('loading');
    error.set('');
    const task: Promise<void> = (async () => {
      for (;;) {
        if (!alive) return;
        transcriptStatus.set('loading');
        try {
          await openTranscript();
          return;
        } catch (reason) {
          if (!alive) return;
          toolActivity.set([]);
          report(reason);
          // Match the desktop catalog's quiet two-second reconnect pause.
          // Only observations repeat; original mutations remain in the journal.
          transcriptStatus.set('failed');
          await new Promise<void>((resolve) => {
            const done = () => {
              wakeReconnect = null;
              resolve();
            };
            const timer = setTimeout(done, 2000);
            wakeReconnect = () => {
              clearTimeout(timer);
              done();
            };
          });
          if (!alive) return;
        }
      }
    })().finally(() => {
      if (transcriptTask !== task) return;
      transcriptTask = null;
      if (alive) transcriptStatus.set('failed');
    });
    transcriptTask = task;
  };
  const retryTranscript = (owner: string) => {
    if (!alive || owner !== sessionId || transcriptStatus() !== 'failed') return;
    // Retry wakes the sole observer task; it never starts a parallel loop.
    if (wakeReconnect) wakeReconnect();
    else if (transcriptTask === null) startTranscript();
  };

  // Automatic paging and an explicit Turn jump await the same native-history read.
  const loadOlder = (): Promise<boolean> => {
    if (olderTask) return olderTask;
    if (!alive || !ready() || before() === null) return Promise.resolve(false);
    const fence = latestKnown;
    olderTask = (async () => {
      let position: any = { kind: 'before', cursor: before() };
      for (;;) {
        const page = await history(fence, position);
        if (!alive) return false;
        before.set(page.before);
        if (page.chunks.length || page.before === null) {
          publishHistory(page.chunks, page.through, true);
          if (page.through < latestKnown) {
            requestMetadata(page.chunks[0]);
            void follow().catch(report);
          }
          return true;
        }
        position = { kind: 'before', cursor: page.before };
      }
    })()
      .catch((reason) => {
        report(reason);
        return false;
      })
      .finally(() => {
        olderTask = null;
      });
    return olderTask;
  };
  const cancelTurnNavigation = () => {
    turnJump = null;
    navigatingTurn.set(null);
  };
  const navigateTurn = async (item: TurnNavigationItem) => {
    if (!alive || !ready()) return;
    turnJump = item;
    navigatingTurn.set(item.turnId);
    innerReader.set(null);
    try {
      if (item.anchor.kind === 'landmark') {
        const target = item.anchor;
        // The raw Turn opening may itself be omitted from conversation history.
        // Load past that boundary, then choose a real display seat in its bounds.
        while (
          alive &&
          turnJump === item &&
          before() !== null &&
          (comparePosition(before().position, target.position) > 0 ||
            (comparePosition(before().position, target.position) === 0 && before().offset > 0))
        ) {
          const previous = before();
          if (!(await loadOlder())) return;
          if (
            before() !== null &&
            comparePosition(before().position, previous.position) === 0 &&
            before().field === previous.field &&
            before().offset === previous.offset
          ) {
            notice.set('Older history did not advance. This turn could not be reached.');
            return;
          }
        }
      }
      if (!alive || turnJump !== item) return;
      const targetRows = new Set(
        rows()
          .filter(
            (row) =>
              row.turnId === item.turnId &&
              (item.anchor.kind === 'loaded' ||
                (row.position &&
                  comparePosition(row.position, item.anchor.position) >= 0 &&
                  comparePosition(row.position, item.anchor.lastPosition) <= 0)),
          )
          .map(stableRowId),
      );
      const index =
        targetRows.size === 0
          ? -1
          : display().findIndex((entry) =>
              entry.kind === 'summary'
                ? entry.turnId === item.turnId
                : entry.kind === 'row'
                  ? targetRows.has(entry.rowId)
                  : entry.group.turnId === item.turnId &&
                    entry.group.rowIds.some((id) => targetRows.has(id)),
            );
      if (index < 0) {
        notice.set('This turn has no visible messages in the current reading mode.');
        return;
      }
      awayFromLatest.set(true);
      list?.scrollToIndex({ index, viewPosition: 0 });
      list?.focus();
    } finally {
      if (turnJump === item) cancelTurnNavigation();
    }
  };

  // A slow disk retains one in-flight snapshot and one replaceable latest
  // edit, rather than one complete draft and Promise closure per keystroke.
  const startSave = () => {
    if (saveTask || pendingDraft === null || !alive || savingFailed()) return;
    saveTask = (async () => {
      while (alive && pendingDraft !== null) {
        const edit = pendingDraft;
        pendingDraft = null;
        const saved = await saveDraft(client, draftRevision, edit);
        if (!alive) return;
        draftRevision = saved.revision;
        attachments.saved(edit);
      }
    })()
      .catch((reason) => {
        if (alive) savingFailed.set(true);
        report(reason);
      })
      .finally(() => {
        saveTask = null;
        if (alive && pendingDraft !== null) startSave();
      });
  };
  const flushDraft = async () => {
    while (alive && !savingFailed() && (saveTask || pendingDraft !== null)) {
      startSave();
      await saveTask;
    }
  };
  const changeDraft = (text: string) => {
    if (!alive || !draftReady()) return;
    editEpoch += 1;
    draft.set(text);
    pendingDraft = { text, ...attachments.snapshot() };
    startSave();
  };
  const acceptRecoveredDraft = (
    stored: { revision: number; text: string; attachments: DraftAttachment[] },
    epoch: number,
  ) => {
    if (
      editEpoch === epoch &&
      draftRevision === stored.revision &&
      !savingFailed() &&
      pendingDraft === null &&
      saveTask === null
    ) {
      draft.set(stored.text);
      attachments.restore(stored.attachments);
    }
  };

  const backendIdentity = (value: any): BackendIdentity =>
    value?.kind === 'model'
      ? {
          connectionId: value.connectionId,
          modelId: value.modelId,
          thinkingLevel: value.thinkingLevel ?? null,
        }
      : null;
  const sameBackend = (left: BackendIdentity, right: BackendIdentity) =>
    left === null || right === null
      ? left === right
      : left.connectionId === right.connectionId &&
        left.modelId === right.modelId &&
        left.thinkingLevel === right.thinkingLevel;
  // Metadata follows the route's value, not observe()'s newly allocated backend
  // objects or live stream state. The durable backend remains the only choice.
  const modelRoute = computed(() => {
    const value = backendIdentity(backend());
    return value === null ? null : JSON.stringify([value.connectionId, value.modelId]);
  });
  const modelProjection = signal<{ route: string | null; details: ModelDetails }>({
    route: null,
    details: { kind: 'unavailable', message: 'Choose a model.' },
  });
  let modelGeneration = 0;
  let modelRead: { route: string; generation: number; task: Promise<void> } | null = null;
  const modelDetails = (): ModelDetails => {
    const value = modelProjection();
    return value.route === modelRoute() ? value.details : { kind: 'loading' };
  };
  const refreshModel = (): Promise<void> => {
    if (!alive) return Promise.resolve();
    const route = modelRoute();
    const captured = backendIdentity(backend());
    if (route === null || captured === null) {
      modelGeneration += 1;
      modelRead = null;
      modelProjection.set({
        route: null,
        details: { kind: 'unavailable', message: 'Choose a model.' },
      });
      return Promise.resolve();
    }
    if (modelRead?.route === route) return modelRead.task;
    const generation = ++modelGeneration;
    modelProjection.set({ route, details: { kind: 'loading' } });
    const current = () => alive && generation === modelGeneration && modelRoute() === route;
    const task = Promise.resolve().then(async () => {
      try {
        if (!current()) return;
        const result = await host.query('settings', 'resolveModel', {
          connectionId: captured.connectionId,
          modelId: captured.modelId,
        });
        if (!current()) return;
        modelProjection.set({
          route,
          details:
            result.kind === 'ready' && sameRoute(captured, result.choice.selection)
              ? { kind: 'ready', choice: result.choice }
              : { kind: 'unavailable', message: `Model unavailable: ${captured.modelId}.` },
        });
      } catch (reason) {
        if (current())
          modelProjection.set({
            route,
            details: { kind: 'unavailable', message: String(reason) },
          });
      } finally {
        if (modelRead?.generation === generation) modelRead = null;
      }
    });
    modelRead = { route, generation, task };
    return task;
  };
  effect(() => {
    modelRoute();
    untracked(() => void refreshModel());
  });
  const selection = async (expectedBackend?: BackendIdentity) => {
    const observed = await observe();
    if (!alive) return null;
    const backend = observed.session.backend ?? observed.rememberedBackend;
    if (expectedBackend !== undefined && !sameBackend(backendIdentity(backend), expectedBackend))
      throw new Error('The model changed before confirmation. Send again to confirm it.');
    if (backend?.kind !== 'model') throw new Error('Choose a model with Ctrl+G before sending.');
    const resolved = await host.query('settings', 'resolveModel', {
      connectionId: backend.connectionId,
      modelId: backend.modelId,
    });
    if (!alive) return null;
    if (resolved.kind !== 'ready')
      throw new Error(`Model unavailable: ${backend.modelId}. Press Ctrl+G to choose another.`);
    return { model: resolved.choice.selection, thinkingLevel: backend.thinkingLevel ?? null };
  };

  const settleReply = (
    reply: any,
    text?: string,
    epoch?: number,
    files: DraftAttachment[] = [],
  ) => {
    const accepted = acceptedSend(reply, sessionId);
    if (!accepted) {
      notice.set(receiptMessage(reply));
      return;
    }
    if (text !== undefined && !covered().has(`${accepted.messageId}/body`)) {
      pendingMessages.set([
        ...pendingMessages().filter((row) => row.messageId !== accepted.messageId),
        {
          key: `pending/${accepted.messageId}`,
          messageId: accepted.messageId,
          field: 'body',
          role: 'user',
          placement: accepted.kind === 'queued' ? 'queued' : 'transcript',
          text,
          attachments: files.map(
            (file): AgentHistoryAttachment => ({
              name: file.name,
              mimeType: file.mimeType,
              bytes: file.bytes,
              kind: file.mimeType.startsWith('image/') ? 'image' : 'other',
            }),
          ),
        },
      ]);
    }
    if (text !== undefined && editEpoch === epoch && draft() === text) draft.set('');
    if (text !== undefined) attachments.sent(files.map((file) => file.id));
    notice.set(
      accepted.kind === 'queued'
        ? 'Message queued.'
        : text === undefined
          ? 'Message accepted.'
          : '',
    );
    cancelTurnNavigation();
    awayFromLatest.set(false);
    innerReader.set(null);
    list?.scrollToEnd({ animated: false });
  };

  const sendBlockedReason = () => {
    if (transcriptStatus() === 'closing')
      return 'Closing disconnected updates… Your draft is kept.';
    if (transcriptStatus() === 'failed')
      return ready()
        ? 'Updates disconnected. Reconnecting automatically; your draft is kept.'
        : 'History unavailable. Retrying automatically; your draft is kept.';
    if (ready() && transcriptStatus() !== 'live')
      return 'Reconnecting updates… Your draft is kept.';
    if (!ready()) return 'Loading session history… Your draft is kept.';
    if (archived())
      return 'This conversation is archived. Restore it in Sessions to send; your draft is kept.';
    if (!draftReady()) return 'Loading the saved draft…';
    if (savingFailed()) return 'The draft could not be saved. Sending is unavailable.';
    if (attachments.blocked()) return attachments.blocked();
    if (sending()) return 'Confirming send… New typing is kept.';
    if (recovering()) return 'Reading original receipts… New typing is kept.';
    if (!receiptsKnown() || unresolved().length)
      return 'Sending waits for the original receipt; it will not be replayed.';
    return '';
  };
  const send = async (text: string, expectedBackend?: BackendIdentity) => {
    if (!alive || (!text.trim() && attachments.files().length === 0)) return;
    if (sendBlockedReason()) {
      notice.set(
        'This message was not sent. Your draft is kept; check the status below before trying again.',
      );
      return;
    }
    cancelTurnNavigation();
    if (draft() !== text) changeDraft(text);
    const epoch = editEpoch;
    const files = [...attachments.files()];
    sending.set(true);
    error.set('');
    try {
      const model = await selection(expectedBackend);
      if (!alive || model === null) return;
      await flushDraft();
      if (!alive) return;
      if (savingFailed()) throw new Error('The draft could not be saved; it has not been sent.');
      if (editEpoch !== epoch)
        throw new Error('The draft changed before confirmation. Send again to confirm it.');
      if (
        expectedBackend !== undefined &&
        !sameBackend(backendIdentity(backend()), expectedBackend)
      )
        throw new Error('The model changed before confirmation. Send again to confirm it.');
      // The native journal consumes only this exact revision. New edits keep
      // saving while confirmation waits and never inherit this send attempt.
      receiptsKnown.set(false);
      const sent = await sendDraft(client, draftRevision, model);
      if (!alive) return;
      settleReply(sent.reply, text, epoch, files);
    } catch (reason) {
      report(reason);
    } finally {
      try {
        if (alive) {
          const operations = await pendingSends(client);
          if (alive) {
            unresolved.set(operations);
            receiptsKnown.set(true);
          }
        }
      } catch (reason) {
        report(reason);
      }
      if (alive) sending.set(false);
    }
  };

  const recover = async () => {
    if (!alive || recovering() || sending()) return;
    recovering.set(true);
    const epoch = editEpoch;
    await flushDraft();
    if (!alive) return;
    try {
      const originals = await pendingSends(client);
      if (!alive) return;
      unresolved.set(originals);
      receiptsKnown.set(true);
      for (const original of originals) {
        if (!original.reference) {
          notice.set(
            'An attempt was not confirmed. Explicitly abandon it below before starting a new send.',
          );
          continue;
        }
        const reply = await recoverSend(client, original.id);
        if (!alive) return;
        settleReply(reply);
      }
      const stored = await loadDraft(client);
      if (!alive) return;
      acceptRecoveredDraft(stored, epoch);
      const remaining = await pendingSends(client);
      if (!alive) return;
      unresolved.set(remaining);
    } catch (reason) {
      report(reason);
    } finally {
      if (alive) recovering.set(false);
    }
  };

  const abandon = async (operationId: string) => {
    if (!alive || recovering() || sending()) return;
    recovering.set(true);
    const epoch = editEpoch;
    try {
      const original = await abandonSend(client, operationId);
      if (!alive) return;
      if (original.abandoned) {
        notice.set(
          'Unconfirmed attempt abandoned. The draft is kept; press Enter explicitly to send a new attempt.',
        );
      } else if (original.reference) {
        const reply = await recoverSend(client, operationId);
        if (!alive) return;
        settleReply(reply);
        const stored = await loadDraft(client);
        if (!alive) return;
        acceptRecoveredDraft(stored, epoch);
      }
      const operations = await pendingSends(client);
      if (!alive) return;
      unresolved.set(operations);
      receiptsKnown.set(true);
    } catch (reason) {
      report(reason);
    } finally {
      if (alive) recovering.set(false);
    }
  };

  const stop = async (invocationId: string | null) => {
    if (!alive || !invocationId || invocation() !== invocationId || stopping()) return;
    stoppingRun.set(invocationId);
    try {
      const result = await props.control('stop', { sessionId, invocationId });
      if (!alive) return;
      if (!result.requested) await observe();
      // A requested cancellation is not an ended run. The watch owns that fact.
    } catch (reason) {
      if (alive && stoppingRun() === invocationId) stoppingRun.set(null);
      report(reason);
    }
  };
  const chooseReading = ({ owner, mode }: { owner: string; mode: 'chat' | 'trajectory' }) => {
    if (alive && owner === sessionId) {
      cancelTurnNavigation();
      reading.set(mode);
    }
  };
  const sendRendered = ({
    owner,
    text,
    backend: expectedBackend,
    attachments: files,
  }: SendAction) => {
    if (
      !alive ||
      owner !== sessionId ||
      draft() !== text ||
      !attachments.same(files) ||
      !sameBackend(backendIdentity(backend()), expectedBackend)
    )
      return;
    void send(text, expectedBackend);
  };
  const ownHandle: ConversationHandle = {
    sessionId,
    draft,
    attachments,
    previewImage: props.previewImage,
    editable: () => draftReady() && !savingFailed(),
    blockedReason: sendBlockedReason,
    backend: () => backendIdentity(backend()),
    modelDetails,
    refreshModel,
    permissions,
    workspace,
    empty: () =>
      ready() &&
      invocation() === null &&
      rows().length === 0 &&
      live().length === 0 &&
      pendingMessages().length === 0 &&
      toolActivity().length === 0,
    error,
    notice,
    recoverable: () => !sending() && draftReady() && (!receiptsKnown() || unresolved().length > 0),
    recovering: () => recovering() || sending(),
    unconfirmed: () =>
      unresolved()
        .filter((original) => !original.reference)
        .map((original) => original.id),
    stopping,
    metrics: () => metricText(metrics()),
    changeDraft,
    submit(event) {
      void send(event.text);
    },
    sendRendered,
    recover,
    abandon,
    invocation,
    activeTurn: runTurn,
    activityTurn: () => {
      const group = projection().entries.findLast((entry) => entry.kind === 'process');
      return group?.kind === 'process' ? group.turnId : null;
    },
    async flushDraft() {
      if (!draftReady()) {
        notice.set('Wait for the saved draft before switching.');
        return false;
      }
      await flushDraft();
      return !savingFailed();
    },
    focus() {
      props.focusComposer();
    },
    focusHistory() {
      cancelTurnNavigation();
      list?.focus();
    },
    focusActivity(turnId) {
      cancelTurnNavigation();
      const group = projection().entries.findLast(
        (entry) => entry.kind === 'process' && entry.turnId === turnId,
      );
      if (group?.kind !== 'process') {
        const index = display().findLastIndex(
          (entry) =>
            entry.kind === 'row' && projection().rowsById.get(entry.rowId)?.turnId === turnId,
        );
        if (index >= 0) {
          list?.scrollToIndex({ index, viewPosition: 0 });
          list?.focus();
        }
        return;
      }
      const turn = projection().turns.get(group.turnId);
      if (!turn) return;
      turnState(turn.turnId).expanded.set({ open: true, answerKey: turn.answerKey });
      const state = groupState(group.key);
      if (!state.open()) {
        innerReader.set(group.closed ? group.key : null);
        state.opening = group.closed ? 'top' : 'bottom';
        state.following.set(!group.closed);
      }
      state.open.set(true);
      const target = grouped(turn) ? group.key : group.rowIds.at(-1);
      const index = displayKeys().indexOf(target ?? '');
      if (index >= 0) list?.scrollToIndex({ index, viewPosition: 0 });
      if (grouped(turn)) {
        state.requestFocus = true;
        if (state.list) {
          state.requestFocus = false;
          state.list.focus();
        } else list?.focus();
      } else list?.focus();
    },
    jumpLatest(turnId) {
      cancelTurnNavigation();
      awayFromLatest.set(false);
      innerReader.set(null);
      const group = projection().entries.findLast(
        (entry) => entry.kind === 'process' && entry.turnId === turnId,
      );
      if (group?.kind === 'process')
        groupStates.get(group.key)?.list?.scrollToEnd({ animated: false });
      list?.scrollToEnd({ animated: false });
      props.focusComposer();
    },
    stop,
    refresh: observe,
  };
  props.attach(ownHandle);

  cleanup(() => {
    alive = false;
    turnJump = null;
    wakeReconnect?.();
    modelGeneration += 1;
    modelRead = null;
    toolActivity.set([]);
    props.detach(ownHandle);
    const stream = subscription;
    subscription = null;
    if (stream !== null) void stream.close().catch(() => {});
  });
  effect(() =>
    untracked(() => {
      void host
        .query('agent', 'pendingInteractions', { sessionId })
        .then((pending: any) => {
          if (alive && pending.revision >= interactions().revision) interactions.set(pending);
        })
        .catch(report);
      startTranscript();
      void (async () => {
        const stored = await loadDraft(client);
        if (!alive) return;
        draftRevision = stored.revision;
        draft.set(stored.text);
        attachments.restore(stored.attachments);
        draftReady.set(true);
        void attachments.start();
        const operations = await pendingSends(client);
        if (!alive) return;
        unresolved.set(operations);
        receiptsKnown.set(true);
        if (operations.length) await recover();
      })().catch(report);
    }),
  );

  return (
    <View style={{ flex: ownHandle.empty() ? 0 : 1, minHeight: 0, backgroundColor: C.background }}>
      <If when={attachmentMetadataFailed()}>
        <Button onPress={retryAttachmentMetadata}>
          <Text style={{ color: C.accent }}>Retry attachment metadata</Text>
        </Button>
      </If>
      <Header
        title={title()}
        workspace={workspace()}
        status={stopping() ? 'Stopping…' : run()}
        active={invocation() !== null}
        commands={props.commands}
        browse={props.browse}
        files={props.files}
        filesOpener={props.filesOpener}
      />
      <If when={transcriptStatus() === 'failed'}>
        <Button
          accessibilityLabel={ready() ? 'Reconnect updates' : 'Retry loading conversation'}
          onPress={inputAction(retryTranscript, sessionId)}
          style={{
            alignSelf: 'flex-start',
            paddingHorizontal: 16,
            backgroundColor: C.raised,
          }}
        >
          <Text style={{ color: C.accent }}>
            {ready() ? 'Reconnect updates' : 'Retry loading conversation'}
          </Text>
        </Button>
      </If>
      <View
        style={{
          display: ownHandle.empty() ? 'none' : 'flex',
          flexDirection: 'row',
          flexWrap: 'wrap',
          paddingHorizontal: 16,
          gap: 24,
          borderBottomWidth: 16,
          borderBottomColor: C.surface,
        }}
      >
        <Button
          role="tab"
          selected={reading() === 'chat'}
          onPress={inputAction(chooseReading, { owner: sessionId, mode: 'chat' })}
          hoverStyle={{ backgroundColor: C.surface }}
          focusStyle={{ backgroundColor: C.raised }}
          activeStyle={{ backgroundColor: C.line }}
        >
          <Text
            style={{
              color: reading() === 'chat' ? C.accent : C.muted,
              fontWeight: reading() === 'chat' ? '700' : '400',
            }}
          >
            Chat
          </Text>
        </Button>
        <Button
          role="tab"
          selected={reading() === 'trajectory'}
          onPress={inputAction(chooseReading, { owner: sessionId, mode: 'trajectory' })}
          hoverStyle={{ backgroundColor: C.surface }}
          focusStyle={{ backgroundColor: C.raised }}
          activeStyle={{ backgroundColor: C.line }}
        >
          <Text
            style={{
              color: reading() === 'trajectory' ? C.accent : C.muted,
              fontWeight: reading() === 'trajectory' ? '700' : '400',
            }}
          >
            Trajectory
          </Text>
        </Button>
        <PresentationControls settings={props.presentation} />
      </View>
      <View
        style={{
          display: ownHandle.empty() ? 'none' : 'flex',
          flex: 1,
          minHeight: 0,
          width: '100%',
          maxWidth: 800,
          alignSelf: 'center',
          paddingHorizontal: 16,
        }}
      >
        <View style={{ flex: 1, minHeight: 0, flexDirection: 'row' }}>
          <List
            ref={list}
            onLayout={(frame) => readingSize.set({ width: frame.width, height: frame.height })}
            each={displayKeys()}
            onScrollBeginDrag={cancelTurnNavigation}
            onViewableItemsChanged={(event) =>
              visibleTurn.set(entryTurn(event.viewableItems[0]?.item ?? ''))
            }
            estimatedItemSize={96}
            maintainVisibleContentPosition={
              innerReader() === null ? { autoscrollToBottomThreshold: 32 } : {}
            }
            contentStyle={{ paddingRight: 8 }}
            style={{ flex: 1, minHeight: 0 }}
            onScroll={(event) => {
              awayFromLatest.set(event.y < Math.max(0, event.contentHeight - event.height) - 32);
              const reader = innerReader();
              if (
                reader !== null &&
                !groupStates.get(reader)?.list &&
                event.y >= Math.max(0, event.contentHeight - event.height) - 1
              )
                innerReader.set(null);
              if (event.y <= 32) void loadOlder();
            }}
            empty={
              <Text style={{ color: C.muted, paddingVertical: 32 }}>Loading session history…</Text>
            }
          >
            {(key) => (
              <Each of={[key()]} key={(id: string) => id}>
                {(ownedKey) => {
                  const initial = displayByKey().get(ownedKey());
                  if (!initial) return <View />;
                  const entry = () => displayByKey().get(ownedKey()) ?? initial;
                  const reveal = () =>
                    list?.scrollToIndex({
                      index: displayKeys().indexOf(ownedKey()),
                      viewPosition: 0,
                    });
                  if (initial.kind === 'summary') {
                    const first = projection().turns.get(initial.turnId);
                    if (!first) return <View />;
                    const turn = () => projection().turns.get(initial.turnId) ?? first;
                    return (
                      <TurnSummary
                        turn={turn}
                        expanded={() => turnExpanded(turn())}
                        canFold={() => canFoldTurn(turn())}
                        toggle={(open) => {
                          turnState(initial.turnId).expanded.set({
                            open,
                            answerKey: turn().answerKey,
                          });
                          if (open) reveal();
                        }}
                      />
                    );
                  }
                  if (initial.kind === 'process' || initial.kind === 'activity') {
                    const group = () => {
                      const latest = entry();
                      return latest.kind === 'process' || latest.kind === 'activity'
                        ? latest.group
                        : initial.group;
                    };
                    const first = projection().turns.get(initial.group.turnId);
                    if (!first) return <View />;
                    const turn = () => projection().turns.get(initial.group.turnId) ?? first;
                    const tool = () =>
                      latestToolActivity(
                        toolActivity().filter((item) => group().toolIds.includes(item.toolUseId)),
                        invocation(),
                        group().turnId,
                      );
                    if (initial.kind === 'activity')
                      return (
                        <ActivityStatus
                          title={() => {
                            const current = tool();
                            return current
                              ? toolActivitySummary(
                                  props.presentation.policy().liveProcessDetail
                                    ? current
                                    : { ...current, title: undefined, detail: '' },
                                )
                              : 'Working…';
                          }}
                          active={() => turn().active}
                        />
                      );
                    return (
                      <TurnPanel
                        group={group}
                        turn={turn}
                        toolActivity={tool}
                        state={groupState(initial.group.key)}
                        policy={props.presentation.policy}
                        row={(id) => projection().rowsById.get(id)}
                        rowState={rowState}
                        reasoningPreview={reasoningPreview}
                        sessionId={sessionId}
                        through={() => latestKnown}
                        previewImage={props.previewImage}
                        reveal={reveal}
                        activity={activity}
                        viewport={readingSize}
                        reading={(away) => {
                          if (away) innerReader.set(initial.group.key);
                          else if (innerReader() === initial.group.key) innerReader.set(null);
                        }}
                      />
                    );
                  }
                  const first =
                    projection().rowsById.get(initial.rowId) ??
                    pendingMessages().find((row) => stableRowId(row) === initial.rowId);
                  if (!first) return <View />;
                  const row = () =>
                    projection().rowsById.get(initial.rowId) ??
                    pendingMessages().find((row) => stableRowId(row) === initial.rowId) ??
                    first;
                  return (
                    <View>
                      <If
                        when={
                          activity().kind === 'response' &&
                          activity().rowId === initial.rowId &&
                          !latestToolActivity(toolActivity(), invocation(), row().turnId ?? '')
                        }
                      >
                        <ActivityStatus title={() => 'Writing response'} active={() => true} />
                      </If>
                      <MessageRow
                        item={row}
                        ownerKey={() => initial.rowId}
                        viewState={rowState(initial.rowId)}
                        reasoningPreview={() => reasoningPreview(initial.rowId)}
                        sessionId={sessionId}
                        through={() => latestKnown}
                        previewImage={props.previewImage}
                        onExpand={reveal}
                        finalActions={(row, ownerKey) => (
                          <FinalActions
                            row={row}
                            ownerKey={ownerKey}
                            sessionId={sessionId}
                            through={() => latestKnown}
                            fork={props.fork}
                          />
                        )}
                      />
                    </View>
                  );
                }}
              </Each>
            )}
          </List>
          <If when={ready() && navigationItems().length > 1}>
            <TurnNavigation
              items={navigationItems}
              active={visibleTurn}
              busy={navigatingTurn}
              navigate={(item) => {
                void navigateTurn(item).catch(report);
              }}
              cancel={cancelTurnNavigation}
            />
          </If>
          <ReturnToLatest
            visible={awayFromLatest() || innerReader() !== null}
            activate={() => ownHandle.jumpLatest(runTurn())}
          />
        </View>
        <If when={interactions().entries.length > 0}>
          <Button
            onPress={inputAction(props.interaction, {
              sessionId,
              requestId: interactions().entries[0].requestId,
            })}
            style={{ padding: 8, backgroundColor: C.surface }}
          >
            <Text style={{ color: C.warning }}>
              ! Your input is needed · Open request ({interactions().entries.length})
            </Text>
          </Button>
        </If>
      </View>
    </View>
  );
}
function metricText(value: any): string {
  if (!value) return '';
  const compact = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n));
  const parts: string[] = [];
  if (value.outputTokensPerSecond != null)
    parts.push(`${value.outputTokensPerSecond.toFixed(1)} tok/s (last request)`);
  if (value.contextTokens != null) {
    const context = `${value.approximate ? '≈' : ''}${compact(value.contextTokens)}`;
    const total = value.contextWindow != null ? ` / ${compact(value.contextWindow)}` : '';
    const percent =
      value.contextWindow > 0
        ? ` · ${Math.round((value.contextTokens / value.contextWindow) * 100)}%`
        : '';
    parts.push(
      `Context${value.contextLastKnown ? ' (last known)' : ''} ${context}${total}${percent}`,
    );
  }
  return parts.join(' · ');
}
