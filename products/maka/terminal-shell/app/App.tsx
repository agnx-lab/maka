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

import { batch, cleanup, computed, effect, Input, mount, signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { List } from 'nsh/list';
import { host } from 'maka/host';

type Position = { sourceSequence: number; itemIndex: number };
type Fragment = { offset: number; text: string };
type Subscription = { next(): Promise<any | null>; close(): Promise<void> };
type Row = {
  key: string;
  messageId: string;
  field: string;
  role: string;
  text: string;
  position?: Position;
  fragments?: Fragment[];
  tool?: any;
  invocationId?: string;
  stepId?: string;
  sequence?: number;
  closedAt?: number | null;
};

// These are the Desktop transcript's position/fragment rules, independent of
// either renderer. History cursors use byte offsets; JS never derives them
// from UTF-16 string lengths.
export function mergeChunks(rows: Row[], chunks: any[], older = false): Row[] {
  const merged = new Map(rows.map((row) => [row.key, row]));
  for (const chunk of chunks) {
    const key = `${chunk.position.sourceSequence}/${chunk.position.itemIndex}/${chunk.field}`;
    const previous = merged.get(key);
    const fresh = { offset: chunk.offset, text: chunk.text };
    const fragments = (
      older && previous
        ? [
            ...previous.fragments!,
            ...(previous.fragments!.some((part) => part.offset === chunk.offset) ? [] : [fresh]),
          ]
        : [...(previous?.fragments ?? []).filter((part) => part.offset !== chunk.offset), fresh]
    ).sort((a, b) => a.offset - b.offset);
    merged.set(key, {
      key,
      messageId: chunk.messageId,
      position: chunk.position,
      field: chunk.field,
      role: chunk.field === 'thinking' ? 'thinking' : chunk.role,
      tool: older && previous ? previous.tool : chunk.tool,
      fragments,
      text: fragments.map((part) => part.text).join(''),
    });
  }
  return [...merged.values()].sort(
    (a, b) =>
      a.position!.sourceSequence - b.position!.sourceSequence ||
      a.position!.itemIndex - b.position!.itemIndex ||
      Number(a.field === 'body') - Number(b.field === 'body'),
  );
}

export function acceptedSend(reply: any, sessionId: string): any | null {
  const receipt = reply?.receipt;
  if (receipt?.kind !== 'recorded' || receipt.outcome?.kind !== 'returned') return null;
  const result = receipt.outcome.value;
  if (result?.sessionId !== sessionId || typeof result.messageId !== 'string' || !result.messageId)
    return null;
  if (result.kind === 'started' && typeof result.invocationId === 'string' && result.invocationId)
    return result;
  if (result.kind === 'queued' && Number.isSafeInteger(result.revision) && result.revision >= 0)
    return result;
  return null;
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

function App() {
  const sessionId = host.sessionId;
  const rows = signal<Row[]>([]);
  const live = signal<Row[]>([]);
  const pendingMessages = signal<Row[]>([]);
  const title = signal(sessionId);
  const run = signal('Loading session…');
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
  const display = computed(() => [...rows(), ...pendingMessages(), ...live()]);
  let list: YNode | undefined;
  let alive = true;
  let subscription: Subscription | null = null;
  let olderFence = 0;
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
  const publishHistory = (chunks: any[], older = false) => {
    rows.set(mergeChunks(rows(), chunks, older));
    const canonical = covered();
    live.set(live().filter((row) => !canonical.has(`${row.messageId}/${row.field}`)));
    pendingMessages.set(
      pendingMessages().filter((row) => !canonical.has(`${row.messageId}/${row.field}`)),
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
    title.set(observed.session.title || sessionId);
    if (observed.through >= runThrough) {
      run.set(observed.run?.state ?? 'Idle');
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
          publishHistory(page.chunks);
          if (page.after === null) break;
          position = { kind: 'after', cursor: page.after };
        }
        live.set(live().filter((row) => row.closedAt === null || row.closedAt! > fence));
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
      if (Object.values(page.pending).some((value) => Array.isArray(value) && value.length)) {
        notice.set('This session needs an interaction. Resolve it in the existing client.');
      }
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
          closeLive(event.sequence, event.invocationId);
          parts = parts.filter((part) => part.invocationId !== event.invocationId);
          if (event.sequence > runThrough) {
            run.set(fact.failed ? 'Failed' : 'Idle');
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
    olderFence = observed.through;
    let position: any = { kind: 'tail' };
    for (;;) {
      const page = await history(olderFence, position);
      if (!alive) return;
      if (page.chunks.length === 0 && page.before !== null) {
        position = { kind: 'before', cursor: page.before };
        continue;
      }
      before.set(page.before);
      publishHistory(page.chunks);
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
    try {
      let position: any = { kind: 'before', cursor: before() };
      for (;;) {
        const page = await history(olderFence, position);
        if (!alive) return;
        before.set(page.before);
        if (page.chunks.length || page.before === null) {
          publishHistory(page.chunks, true);
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
        const saved = await host.saveDraft(draftRevision, text);
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
    if (backend?.kind !== 'model')
      throw new Error(
        'This session has no selected native model. Select one in the existing client.',
      );
    // Exactly the session's connection/model, with its current catalog revision.
    // A changing catalog gets one complete retry; a missing route never falls back.
    for (let attempt = 0; attempt < 2; attempt += 1) {
      let page = await host.query('settings', 'models', { kind: 'start', query: backend.modelId });
      if (!alive) return null;
      if (page.kind !== 'ready') continue;
      const revision = page.revision;
      for (;;) {
        const choice = page.choices.find(
          (choice: any) =>
            choice.selection.connectionId === backend.connectionId &&
            choice.selection.modelId === backend.modelId,
        );
        if (choice)
          return { model: choice.selection, thinkingLevel: backend.thinkingLevel ?? null };
        if (page.next === null)
          throw new Error(`The session's model is unavailable: ${backend.modelId}`);
        page = await host.query('settings', 'models', {
          kind: 'continue',
          revision,
          cursor: page.next,
        });
        if (!alive) return null;
        if (page.kind !== 'ready') break;
      }
    }
    throw new Error('The model catalog changed. Try sending again.');
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
    notice.set(accepted.kind === 'queued' ? 'Message queued.' : 'Message accepted.');
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
      const sent = await host.send(draftRevision, model);
      if (!alive) return;
      settleReply(sent.reply, text, epoch);
    } catch (reason) {
      report(reason);
    } finally {
      try {
        if (alive) {
          const operations = await host.pending();
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
      const originals = await host.pending();
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
        const reply = await host.recover(original.id);
        if (!alive) return;
        settleReply(reply);
      }
      const stored = await host.loadDraft();
      if (!alive) return;
      acceptRecoveredDraft(stored, epoch);
      const remaining = await host.pending();
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
      const original = await host.abandonUnprepared(operationId);
      if (!alive) return;
      if (original.abandoned) {
        notice.set(
          'Unconfirmed attempt abandoned. The draft is kept; press Enter explicitly to send a new attempt.',
        );
      } else if (original.reference) {
        const reply = await host.recover(operationId);
        if (!alive) return;
        settleReply(reply);
        const stored = await host.loadDraft();
        if (!alive) return;
        acceptRecoveredDraft(stored, epoch);
      }
      const operations = await host.pending();
      if (!alive) return;
      unresolved.set(operations);
      receiptsKnown.set(true);
    } catch (reason) {
      report(reason);
    } finally {
      if (alive) recovering.set(false);
    }
  };

  cleanup(() => {
    alive = false;
    const stream = subscription;
    subscription = null;
    if (stream !== null) void stream.close().catch(() => {});
  });
  effect(() => {
    void openTranscript().catch(async (reason) => {
      report(reason);
      const stream = subscription;
      subscription = null;
      if (stream !== null) await stream.close().catch(() => {});
    });
    void (async () => {
      const stored = await host.loadDraft();
      if (!alive) return;
      draftRevision = stored.revision;
      draft.set(stored.text);
      draftReady.set(true);
      const operations = await host.pending();
      if (!alive) return;
      unresolved.set(operations);
      receiptsKnown.set(true);
      if (operations.length) await recover();
    })().catch(report);
  });

  return (
    <View style={{ flex: 1, backgroundColor: '#111827', padding: 8, gap: 8 }}>
      <View style={{ flexDirection: 'row', justifyContent: 'space-between', gap: 8 }}>
        <Text style={{ color: '#f9fafb', fontSize: 16 }}>{title()}</Text>
        <Text style={{ color: '#93c5fd', fontSize: 16 }}>{run()}</Text>
      </View>
      <Text style={{ color: '#9ca3af', fontSize: 16 }}>
        Session: {sessionId} · plain text transcript
      </Text>
      <View
        onPress={() => {
          void loadOlder();
        }}
        style={{ padding: 8, backgroundColor: '#1f2937' }}
      >
        <Text style={{ color: '#93c5fd', fontSize: 16 }}>
          {loadingOlder()
            ? 'Loading earlier messages…'
            : before() === null
              ? 'Beginning of loaded history'
              : 'Load earlier messages'}
        </Text>
      </View>
      <List
        ref={list}
        each={display()}
        estimatedItemSize={64}
        maintainVisibleContentPosition={{ autoscrollToBottomThreshold: 32 }}
        style={{ flex: 1, minHeight: 0 }}
        contentContainerStyle={{ gap: 8 }}
      >
        {(item) => (
          <View
            style={{ padding: 8, backgroundColor: item().role === 'user' ? '#1e293b' : '#111827' }}
          >
            <Text style={{ color: '#93c5fd', fontSize: 16 }}>
              {item().role}
              {item().tool?.call?.name ? ` · ${item().tool.call.name}` : ''}
            </Text>
            <Text
              selectable
              style={{ color: item().role === 'thinking' ? '#9ca3af' : '#f9fafb', fontSize: 16 }}
            >
              {item().text ||
                (item().tool ? '[Tool details are available in the existing client]' : '')}
            </Text>
          </View>
        )}
      </List>
      <If when={error()}>
        <Text style={{ color: '#fca5a5', fontSize: 16 }}>{error()}</Text>
      </If>
      <If when={notice()}>
        <Text style={{ color: '#d1d5db', fontSize: 16 }}>{notice()}</Text>
      </If>
      <If when={draftReady() && (!receiptsKnown() || unresolved().length > 0)}>
        <View
          onPress={() => {
            void recover();
          }}
          style={{ padding: 8, backgroundColor: '#713f12' }}
        >
          <Text style={{ color: '#fef3c7', fontSize: 16 }}>
            {recovering() ? 'Reading original receipts…' : 'Recover original send receipt'}
          </Text>
        </View>
      </If>
      <Each of={unresolved()} key={(original) => original.id}>
        {(original) => {
          // Each owns this row by operation id; it is never a recycled List
          // cell whose accessor could retarget an already queued activation.
          const operationId = original().id;
          return (
            <If when={!original().reference}>
              <View
                onPress={() => {
                  void abandon(operationId);
                }}
                style={{ padding: 8, backgroundColor: '#713f12' }}
              >
                <Text style={{ color: '#fef3c7', fontSize: 16 }}>
                  撤销未提交尝试 · Abandon unconfirmed attempt {operationId.slice(0, 8)}
                </Text>
              </View>
            </If>
          );
        }}
      </Each>
      <Input
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
        placeholder={
          sendBlockedReason()
            ? 'Draft · Sending waits for the status shown below'
            : 'Message · Enter to send · Shift+Enter for a new line'
        }
        placeholderTextColor="#9ca3af"
        style={{
          minHeight: 48,
          maxHeight: 128,
          padding: 8,
          color: '#f9fafb',
          backgroundColor: '#1f2937',
          fontSize: 16,
        }}
      />
      <Text style={{ color: '#9ca3af', fontSize: 16 }}>
        {sendBlockedReason() ||
          'Enter sends · Shift+Enter newline · Ctrl+C copies selection · Ctrl+Q exits'}
      </Text>
    </View>
  );
}

mount(App);
