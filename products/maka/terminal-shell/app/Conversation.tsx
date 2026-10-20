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

import { batch, cleanup, computed, effect, Input, signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { List } from 'nsh/list';
import { AnimatedValue } from 'nsh/animated';
import { windowSize } from 'nsh/device';
import { host } from 'maka/host';
import { C, Header } from './ui';
import { acceptedSend, historyMetadata, mergeChunks, messageKey } from './history';
import type { Row, Position, Subscription } from './history';
import { Checklist } from './Checklist';
import { MessageRow } from './MessageRow';
import { TurnPanel } from './TurnPanel';
import type { TranscriptItem, TurnItem, TurnViewState } from './TurnPanel';

export type Control = (method: string, input: any) => Promise<any>;
export type ConversationHandle = {
  flushDraft(): Promise<boolean>;
  focus(): void;
  jumpLatest(): void;
  stop(): Promise<void>;
  refresh(): Promise<any>;
  focusHistory(): void;
  focusActivity(): void;
};
type ConversationProps = {
  client: any;
  control: Control;
  models(): void;
  commands(): void;
  interaction(sessionId: string, requestId: string): void;
  attach(handle: ConversationHandle): void;
  detach(handle: ConversationHandle): void;
};
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
  const pendingMessages = signal<Row[]>([]);
  const title = signal(sessionId);
  const run = signal('Loading session…');
  const invocation = signal<string | null>(null);
  const stoppingRun = signal<string | null>(null);
  const stopping = () => invocation() !== null && stoppingRun() === invocation();
  const workspace = signal('');
  const permissions = signal('');
  const runTurn = signal<string | null>(null);
  const backend = signal<any>(null);
  const interactions = signal<any>({ revision: -1, entries: [] });
  let editor: YNode | undefined;
  const error = signal('');
  const ready = signal(false);
  const before = signal<any>(null);
  const loadingOlder = signal(false);
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
  const metrics = signal<any>(null);
  const turnStates = new Map<string, TurnViewState>();
  const turnState = (id: string) => {
    let state = turnStates.get(id);
    if (!state) {
      state = {
        expanded: signal<{ open: boolean; completed: boolean } | null>(null),
        offset: new AnimatedValue(0),
        rows: new Map(),
        visited: false,
        lease: 0,
      };
      turnStates.set(id, state);
    }
    return state;
  };
  const display = computed<TranscriptItem[]>(() => {
    const pairedRows: Row[] = [];
    const paired = new Map<string, number>();
    for (const row of [...rows(), ...pendingMessages(), ...live()]) {
      const call = row.tool?.call;
      if (!call) {
        pairedRows.push(row);
        continue;
      }
      const key = `tool/${row.turnId}/${row.tool.toolUseId}/${call.position.sourceSequence}/${call.position.itemIndex}`;
      const index = paired.get(key);
      if (index === undefined) {
        paired.set(key, pairedRows.length);
        pairedRows.push({ ...row, key });
      } else if (row.tool.result) pairedRows[index] = { ...row, key };
    }
    const result: TranscriptItem[] = [];
    const turns = new Map<string, TurnItem>();
    for (const row of pairedRows) {
      if (
        row.role === 'user' ||
        !row.turnId ||
        row.role === 'failure' ||
        row.role === 'compaction'
      ) {
        if (reading() === 'chat' || row.role === 'failure')
          result.push({ kind: 'message', key: messageKey(row), row });
        continue;
      }
      let turn = turns.get(row.turnId);
      if (!turn) {
        turn = {
          kind: 'turn',
          key: `turn/${row.turnId}`,
          turnId: row.turnId,
          rows: [],
          timing: row.timing,
          active: invocation() !== null && runTurn() === row.turnId,
          completed: false,
        };
        turns.set(row.turnId, turn);
        result.push(turn);
      }
      if (row.timing) turn.timing = row.timing;
      if (row.finalOutput) turn.completed = true;
      if (row.finalOutput && row.field === 'body' && row.role === 'assistant') {
        if (reading() === 'chat') result.push({ kind: 'message', key: row.key, row });
      } else turn.rows.push(row);
    }
    return result.filter(
      (item) => item.kind !== 'turn' || item.rows.length > 0 || item.active || item.completed,
    );
  });
  const displayByKey = computed(() => new Map(display().map((item) => [item.key, item])));
  const displayKeys = computed(() => display().map((item) => item.key));
  let list: YNode | undefined;
  let alive = true;
  let subscription: Subscription | null = null;
  let refreshFrom: { position: Position; field: string } | null = null;
  let latestKnown = 0;
  let watchLast = 0;
  let runThrough = 0;
  let parts: any[] = [];
  let followTask: Promise<void> | null = null;
  let followAgain = false;
  let draftRevision = 0;
  let editEpoch = 0;
  let pendingDraft: string | null = null;
  let saveTask: Promise<void> | null = null;

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
          ? { ...row, ...historyMetadata(chunk, through) }
          : row;
      }),
    );
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
  const observe = async () => {
    const observed = await host.query('agent', 'observe', { sessionId });
    if (!alive) return observed;
    if (observed.session.id !== sessionId) throw new Error('Host observed a different session.');
    latestKnown = Math.max(latestKnown, observed.through);
    title.set(observed.session.title || 'Untitled conversation');
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
          for (;;) {
            const page = await history(metadataFence, cursor);
            if (!alive) return;
            publishMetadata(page.chunks, page.through);
            if (page.after === null) break;
            cursor = { kind: 'after', cursor: page.after };
          }
        }
      } while (alive && followAgain);
    })()
      .catch(report)
      .finally(() => {
        followTask = null;
        if (alive && followAgain) void follow();
      });
    return followTask;
  };

  const openParts = (started: any[]) => {
    const same = (a: any, b: any) =>
      a.invocationId === b.invocationId && a.stepId === b.stepId && a.partId === b.partId;
    parts = [...parts.filter((part) => !started.some((other) => same(part, other))), ...started];
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
      const field = (kind: string) => (kind === 'thinking' ? 'thinking' : 'body');
      switch (fact.kind) {
        case 'opened':
          if (event.sequence > runThrough) {
            run.set('running');
            invocation.set(event.invocationId);
            runTurn.set(event.turnId);
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
            },
          ]);
          break;
        case 'delta': {
          const part = parts.find(
            (part) =>
              part.invocationId === event.invocationId &&
              part.stepId === fact.stepId &&
              part.partId === fact.partId,
          );
          if (!part || covered().has(`${part.messageId}/${part.field}`)) break;
          const key = `live/${part.messageId}/${part.field}`;
          const previous = live().find((row) => row.key === key);
          const next = {
            ...part,
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
          parts = parts.filter(
            (part) =>
              part.invocationId !== event.invocationId ||
              part.stepId !== fact.stepId ||
              part.partId !== fact.partId,
          );
          changed = true;
          break;
        case 'step_ended':
          closeLive(event.sequence, event.invocationId, fact.stepId);
          parts = parts.filter(
            (part) => part.invocationId !== event.invocationId || part.stepId !== fact.stepId,
          );
          changed = true;
          break;
        case 'ended':
          requestMetadata(rows().find((row) => row.turnId === event.turnId));
          closeLive(event.sequence, event.invocationId);
          parts = parts.filter((part) => part.invocationId !== event.invocationId);
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
        case 'transcript_changed':
          changed = true;
          break;
      }
    }
    watchLast = Math.max(watchLast, page.nextAfter);
    if (changed) void follow();
  };

  const openTranscript = async () => {
    const observed = await observe();
    if (!alive) return;
    parts = observed.parts.map((part: any) => ({
      ...part,
      invocationId: observed.run?.invocationId ?? null,
      field: part.textKind === 'thinking' ? 'thinking' : 'body',
    }));
    const after = Math.min(
      observed.through,
      ...observed.parts.map((part: any) => part.startSequence - 1),
    );
    const stream = await host.subscribe('agent', 'watch', { sessionId, after });
    if (!alive) {
      await stream.close();
      return;
    }
    subscription = stream;
    watchLast = after;
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
    // This is the sole next() loop; watching from the earliest open part also
    // recovers deltas emitted before observe, without a history/watch gap.
    for (;;) {
      const page = await stream.next();
      if (!alive) return;
      if (page === null) throw new Error('Live updates ended. Reopen the client to reconnect.');
      batch(() => deliver(page));
    }
  };

  const loadOlder = async () => {
    if (!alive || !ready() || loadingOlder() || before() === null) return;
    loadingOlder.set(true);
    const fence = latestKnown;
    try {
      let position: any = { kind: 'before', cursor: before() };
      for (;;) {
        const page = await history(fence, position);
        if (!alive) return;
        before.set(page.before);
        if (page.chunks.length || page.before === null) {
          publishHistory(page.chunks, page.through, true);
          if (page.through < latestKnown) {
            requestMetadata(page.chunks[0]);
            void follow();
          }
          break;
        }
        position = { kind: 'before', cursor: page.before };
      }
    } catch (reason) {
      report(reason);
    } finally {
      if (alive) loadingOlder.set(false);
    }
  };

  // A slow disk retains one in-flight snapshot and one replaceable latest
  // edit, rather than one complete draft and Promise closure per keystroke.
  const startSave = () => {
    if (saveTask || pendingDraft === null || !alive || savingFailed()) return;
    saveTask = (async () => {
      while (alive && pendingDraft !== null) {
        const text = pendingDraft;
        pendingDraft = null;
        const saved = await client.saveDraft(draftRevision, text);
        if (!alive) return;
        draftRevision = saved.revision;
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
    pendingDraft = text;
    startSave();
  };
  const acceptRecoveredDraft = (stored: { revision: number; text: string }, epoch: number) => {
    if (
      editEpoch === epoch &&
      draftRevision === stored.revision &&
      !savingFailed() &&
      pendingDraft === null &&
      saveTask === null
    ) {
      draft.set(stored.text);
    }
  };

  const selection = async () => {
    const observed = await observe();
    if (!alive) return null;
    const backend = observed.session.backend ?? observed.rememberedBackend;
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

  const settleReply = (reply: any, text?: string, epoch?: number) => {
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
          text,
        },
      ]);
    }
    if (text !== undefined && editEpoch === epoch && draft() === text) draft.set('');
    notice.set(
      accepted.kind === 'queued'
        ? 'Message queued.'
        : text === undefined
          ? 'Message accepted.'
          : '',
    );
    list?.scrollToEnd({ animated: false });
  };

  const sendBlockedReason = () => {
    if (!ready()) return 'Loading session history… Your draft is kept.';
    if (!draftReady()) return 'Loading the saved draft…';
    if (savingFailed()) return 'The draft could not be saved. Sending is unavailable.';
    if (sending()) return 'Confirming send… New typing is kept.';
    if (recovering()) return 'Reading original receipts… New typing is kept.';
    if (!receiptsKnown() || unresolved().length)
      return 'Sending waits for the original receipt; it will not be replayed.';
    return '';
  };
  const send = async (text: string) => {
    if (!alive || !text.trim()) return;
    if (sendBlockedReason()) {
      notice.set(
        'This Enter did not send a message. Your draft is kept; check the status below before trying again.',
      );
      return;
    }
    if (draft() !== text) changeDraft(text);
    const epoch = editEpoch;
    sending.set(true);
    error.set('');
    try {
      const model = await selection();
      if (!alive || model === null) return;
      await flushDraft();
      if (!alive) return;
      if (savingFailed()) throw new Error('The draft could not be saved; it has not been sent.');
      if (editEpoch !== epoch)
        throw new Error('The draft changed before confirmation. Press Enter again to send it.');
      // The native journal consumes only this exact revision. New edits keep
      // saving while confirmation waits and never inherit this send attempt.
      receiptsKnown.set(false);
      const sent = await client.send(draftRevision, model);
      if (!alive) return;
      settleReply(sent.reply, text, epoch);
    } catch (reason) {
      report(reason);
    } finally {
      try {
        if (alive) {
          const operations = await client.pending();
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
      const originals = await client.pending();
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
        const reply = await client.recover(original.id);
        if (!alive) return;
        settleReply(reply);
      }
      const stored = await client.loadDraft();
      if (!alive) return;
      acceptRecoveredDraft(stored, epoch);
      const remaining = await client.pending();
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
      const original = await client.abandonUnprepared(operationId);
      if (!alive) return;
      if (original.abandoned) {
        notice.set(
          'Unconfirmed attempt abandoned. The draft is kept; press Enter explicitly to send a new attempt.',
        );
      } else if (original.reference) {
        const reply = await client.recover(operationId);
        if (!alive) return;
        settleReply(reply);
        const stored = await client.loadDraft();
        if (!alive) return;
        acceptRecoveredDraft(stored, epoch);
      }
      const operations = await client.pending();
      if (!alive) return;
      unresolved.set(operations);
      receiptsKnown.set(true);
    } catch (reason) {
      report(reason);
    } finally {
      if (alive) recovering.set(false);
    }
  };

  const stop = async () => {
    const invocationId = invocation();
    if (!alive || !invocationId || stopping()) return;
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
  const ownHandle: ConversationHandle = {
    async flushDraft() {
      if (!draftReady()) {
        notice.set('Wait for the saved draft before switching.');
        return false;
      }
      await flushDraft();
      return !savingFailed();
    },
    focus() {
      editor?.focus();
    },
    focusHistory() {
      list?.focus();
    },
    focusActivity() {
      const index = display().findLastIndex((item) => item.kind === 'turn');
      const turn = display()[index];
      if (turn?.kind !== 'turn') return;
      list?.scrollToIndex({ index, viewPosition: 0 });
      const state = turnState(turn.turnId);
      state.requestFocus = true;
      state.expanded.set({ open: true, completed: turn.completed });
      if (state.list) {
        state.requestFocus = false;
        state.list.focus();
      }
    },
    jumpLatest() {
      if (runTurn()) turnStates.get(runTurn()!)?.list?.scrollToEnd({ animated: false });
      list?.scrollToEnd({ animated: false });
      editor?.focus();
    },
    stop,
    refresh: observe,
  };
  props.attach(ownHandle);

  cleanup(() => {
    alive = false;
    props.detach(ownHandle);
    const stream = subscription;
    subscription = null;
    if (stream !== null) void stream.close().catch(() => {});
  });
  effect(() => {
    void host
      .query('agent', 'pendingInteractions', { sessionId })
      .then((pending: any) => {
        if (alive && pending.revision >= interactions().revision) interactions.set(pending);
      })
      .catch(report);
    void openTranscript().catch(async (reason) => {
      report(reason);
      const stream = subscription;
      subscription = null;
      if (stream !== null) await stream.close().catch(() => {});
    });
    void (async () => {
      const stored = await client.loadDraft();
      if (!alive) return;
      draftRevision = stored.revision;
      draft.set(stored.text);
      draftReady.set(true);
      const operations = await client.pending();
      if (!alive) return;
      unresolved.set(operations);
      receiptsKnown.set(true);
      if (operations.length) await recover();
    })().catch(report);
  });

  return (
    <View style={{ flex: 1, backgroundColor: C.background }}>
      <Header
        title={title()}
        workspace={workspace()}
        status={stopping() ? 'Stopping…' : run()}
        active={invocation() !== null}
        commands={props.commands}
      />
      <View
        style={{
          flexDirection: 'row',
          paddingHorizontal: 16,
          gap: 24,
          borderBottomWidth: 16,
          borderBottomColor: C.line,
        }}
      >
        <View onPress={() => reading.set('chat')}>
          <Text
            style={{
              color: reading() === 'chat' ? C.accent : C.muted,
              fontWeight: reading() === 'chat' ? '700' : '400',
            }}
          >
            Chat
          </Text>
        </View>
        <View onPress={() => reading.set('trajectory')}>
          <Text
            style={{
              color: reading() === 'trajectory' ? C.accent : C.muted,
              fontWeight: reading() === 'trajectory' ? '700' : '400',
            }}
          >
            Trajectory
          </Text>
        </View>
      </View>
      <View
        style={{
          flex: 1,
          minHeight: 0,
          width: '100%',
          maxWidth: 800,
          alignSelf: 'center',
          paddingHorizontal: 16,
        }}
      >
        <View
          style={{ flex: 1, minHeight: 0 }}
          onLayout={(frame) => readingSize.set({ width: frame.width, height: frame.height })}
        >
          <List
            ref={list}
            each={displayKeys()}
            estimatedItemSize={96}
            maintainVisibleContentPosition={{ autoscrollToBottomThreshold: 32 }}
            contentContainerStyle={{ paddingRight: 8 }}
            style={{ flex: 1, minHeight: 0 }}
            onScroll={(event) => {
              if (event.y <= 32) void loadOlder();
            }}
            ListEmptyComponent={
              <View style={{ paddingVertical: 32, gap: 16 }}>
                <Text style={{ color: C.text, fontWeight: '700' }}>
                  {ready() ? 'What shall we build?' : 'Loading session history…'}
                </Text>
                <Text style={{ color: C.muted }}>
                  {ready()
                    ? 'Bring a question, a hard bug, or your next big idea.'
                    : 'Your saved draft will be ready in a moment.'}
                </Text>
              </View>
            }
          >
            {(key) => (
              <Each of={[key()]} key={(id: string) => id}>
                {(ownedKey) => {
                  const initial = displayByKey().get(ownedKey());
                  if (!initial) return <View />;
                  const entry = () => displayByKey().get(ownedKey()) ?? initial;
                  return (
                    <If
                      when={entry().kind === 'turn'}
                      else={
                        <MessageRow
                          item={() => (entry() as any).row}
                          sessionId={sessionId}
                          through={() => latestKnown}
                        />
                      }
                    >
                      <TurnPanel
                        turn={() => entry() as TurnItem}
                        state={turnState((entry() as TurnItem).turnId)}
                        sessionId={sessionId}
                        through={() => latestKnown}
                        reveal={() =>
                          list?.scrollToIndex({
                            index: displayKeys().indexOf(ownedKey()),
                            viewPosition: 0,
                          })
                        }
                        trajectory={() => reading() === 'trajectory'}
                        viewport={readingSize}
                      />
                    </If>
                  );
                }}
              </Each>
            )}
          </List>
        </View>
        <If when={interactions().entries.length > 0}>
          <View
            onPress={() => props.interaction(sessionId, interactions().entries[0].requestId)}
            style={{ padding: 8, backgroundColor: C.surface }}
          >
            <Text style={{ color: C.warning }}>
              ! Your input is needed · Open request ({interactions().entries.length})
            </Text>
          </View>
        </If>
        <If when={error()}>
          <Text selectable style={{ color: C.danger }}>
            {error()}
          </Text>
        </If>
        <If when={notice()}>
          <Text style={{ color: C.muted }}>{notice()}</Text>
        </If>
        <If when={draftReady() && (!receiptsKnown() || unresolved().length > 0)}>
          <View
            onPress={() => {
              void recover();
            }}
            style={{ paddingVertical: 16 }}
          >
            <Text style={{ color: C.warning }}>
              {recovering() ? 'Reading original receipts…' : 'Recover original send receipt'}
            </Text>
          </View>
        </If>
        <Each of={unresolved()} key={(original) => original.id}>
          {(original) => {
            const operationId = original().id;
            return (
              <If when={!original().reference}>
                <View
                  onPress={() => {
                    void abandon(operationId);
                  }}
                  style={{ paddingVertical: 16 }}
                >
                  <Text style={{ color: C.warning }}>
                    Abandon unconfirmed send · {operationId.slice(0, 8)}
                  </Text>
                </View>
              </If>
            );
          }}
        </Each>

        <Checklist sessionId={sessionId} />
        <View
          style={{
            backgroundColor: C.surface,
            borderRadius: 16,
            paddingHorizontal: 16,
            paddingVertical: 16,
            gap: 16,
          }}
        >
          <Input
            ref={editor}
            testID="message"
            autoFocus
            multiline
            submitBehavior="submit"
            value={draft()}
            editable={draftReady() && !savingFailed()}
            onChangeText={changeDraft}
            onSubmitEditing={(event) => {
              void send(event.text);
            }}
            placeholder="Message or run a task…"
            placeholderTextColor={C.muted}
            style={{ minHeight: 16, maxHeight: 80, color: C.text, fontSize: 16, lineHeight: 16 }}
          />
          <View style={{ flexDirection: 'row', alignItems: 'center', gap: 16 }}>
            <View onPress={props.commands}>
              <Text style={{ color: C.muted }}>+</Text>
            </View>
            <Text style={{ color: C.muted }}>{permissions()}</Text>
            <View onPress={props.models} style={{ flex: 1, alignItems: 'flex-end' }}>
              <Text
                numberOfLines={1}
                style={{ color: backend()?.kind === 'model' ? C.muted : C.warning }}
              >
                {backend()?.kind === 'model' ? backend().modelId : 'Choose a model'} · Ctrl+G
              </Text>
            </View>
            <If when={invocation() !== null}>
              <View
                onPress={() => {
                  void stop();
                }}
                style={{ backgroundColor: C.accent, paddingHorizontal: 8, borderRadius: 8 }}
              >
                <Text style={{ color: C.background }}>{stopping() ? '…' : '■'}</Text>
              </View>
            </If>
          </View>
        </View>
        <Text style={{ color: sendBlockedReason() ? C.warning : C.muted, paddingVertical: 0 }}>
          {sendBlockedReason() ||
            (windowSize().width < 720
              ? 'Enter sends · Shift+Enter newline · Ctrl+P commands'
              : 'Enter sends · Shift+Enter newline · Ctrl+P commands · Ctrl+Q exits')}
        </Text>
        <If when={metricText(metrics())}>
          <Text numberOfLines={1} style={{ color: C.muted }}>
            {metricText(metrics())}
          </Text>
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
