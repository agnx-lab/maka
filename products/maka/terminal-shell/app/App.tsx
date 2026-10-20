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

import { cleanup, effect, mount, signal, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import { host } from 'maka/host';
import { keymap } from 'nsh/keymap';
import { windowSize } from 'nsh/device';
import { C, Header } from './ui';
import { Conversation } from './Conversation';
import type { Control, ConversationHandle } from './Conversation';
import { Palette } from './Palette';
import type { PaletteEntry, PaletteKind } from './Palette';
import { InteractionPanel } from './InteractionPanel';

type Overlay =
  | { kind: PaletteKind; sessionId: string | null }
  | { kind: 'interaction'; sessionId: string; requestId: string };
function App() {
  const selected = signal<string | null>(host.initialSession);
  const overlay = signal<Overlay | null>(null);
  const recent = signal<any[]>([]);
  const error = signal('');
  const status = signal('');
  const switching = signal(false);
  const busy = signal(false);
  const controls = signal<any[]>([]);
  const controlsKnown = signal(false);
  const recovering = signal(false);
  let handle: ConversationHandle | null = null;
  let alive = true;
  let controlRead = 0;
  cleanup(() => {
    alive = false;
  });
  const report = (reason: unknown) => {
    if (alive) error.set(reason instanceof Error ? reason.message : String(reason));
  };
  const refreshControls = async () => {
    const reading = ++controlRead;
    const originals = await host.pendingControls();
    if (alive && reading === controlRead) {
      controls.set(originals);
      controlsKnown.set(true);
    }
  };
  const loadRecent = async () => {
    const page = await host.query('agent', 'list', { filter: 'all', position: { kind: 'start' } });
    if (alive)
      recent.set(
        page.sessions
          .filter((session: any) => !session.archived)
          .sort((a: any, b: any) => b.updatedAt - a.updatedAt),
      );
  };
  const switchSession = async (id: string) => {
    if (!alive || selected() === id || switching()) return;
    switching.set(true);
    try {
      if (handle && !(await handle.flushDraft())) return;
      if (alive) selected.set(id);
    } catch (reason) {
      report(reason);
    } finally {
      if (alive) switching.set(false);
    }
  };
  const returned = (reply: any) => {
    const receipt = reply?.receipt;
    if (receipt?.kind !== 'recorded' || receipt.outcome?.kind !== 'returned')
      throw new Error('Confirmation is still unknown. Recover the original action below.');
    const value = receipt.outcome.value;
    return value;
  };
  const control: Control = async (method, input) => {
    if (method !== 'stop' && !controlsKnown())
      throw new Error('Checking earlier actions. Try again in a moment.');
    const originals: any[] = method === 'stop' ? await host.pendingControls() : controls();
    const pending = originals.find(
      (original) =>
        original.method === method &&
        original.sessionId === (input.sessionId ?? null) &&
        (method !== 'stop' || original.correlation.invocationId === input.invocationId) &&
        (method !== 'respond' || original.correlation.requestId === input.requestId),
    );
    if (pending)
      throw new Error(
        'This action has an unresolved original. Recover it below before trying again.',
      );
    if (method !== 'stop' && busy())
      throw new Error('An action is being confirmed. Try again in a moment.');
    if (method !== 'stop') busy.set(true);
    try {
      return returned((await host.control(method, input)).reply);
    } finally {
      await refreshControls().catch((reason) => {
        controlsKnown.set(false);
        report(reason);
      });
      if (method !== 'stop' && alive) busy.set(false);
    }
  };
  const open = (kind: PaletteKind) => {
    if (kind === 'models' && !selected()) {
      status.set('Open or create a conversation before choosing its model.');
      return;
    }
    status.set('');
    overlay.set({ kind, sessionId: selected() });
  };
  const chooseModel = async (sessionId: string, choice: any) => {
    try {
      error.set('');
      const result = await control('selectBackend', {
        sessionId,
        backend: {
          kind: 'model',
          connectionId: choice.selection.connectionId,
          modelId: choice.selection.modelId,
          thinkingLevel: choice.defaultThinkingLevel ?? null,
        },
      });
      if (!alive) return;
      if (result.kind !== 'committed')
        throw new Error(`Model selection rejected: ${result.reason ?? result.kind}`);
      if (selected() === sessionId) {
        await handle?.refresh();
        handle?.focus();
      }
      status.set('');
    } catch (reason) {
      report(reason);
    }
  };
  const create = async (workspace: any) => {
    try {
      error.set('');
      const result = await control('create', {
        title: 'New conversation',
        automaticTitle: true,
        labels: [],
        workspace,
      });
      if (!alive) return;
      if (result.kind !== 'created')
        throw new Error(`Conversation creation rejected: ${result.reason ?? result.kind}`);
      await switchSession(result.session.id);
      if (selected() === result.session.id) open('models');
      await loadRecent();
    } catch (reason) {
      report(reason);
    }
  };
  const newConversation = () => {
    const sessionId = selected();
    if (sessionId) void create({ kind: 'session', sessionId });
    else open('workspaces');
  };
  const stop = () => {
    void handle?.stop().catch(report);
  };
  const commands = (): PaletteEntry[] => [
    {
      id: 'sessions',
      title: 'Sessions',
      detail: 'Resume a conversation',
      hint: 'Ctrl+O',
      run: () => open('sessions'),
    },
    {
      id: 'new',
      title: 'New conversation',
      detail: 'A fresh start in your workspace',
      hint: 'Ctrl+N',
      run: newConversation,
    },
    {
      id: 'models',
      title: 'Models',
      detail: 'Choose the model for this conversation',
      hint: 'Ctrl+G',
      run: () => open('models'),
    },
    {
      id: 'latest',
      title: 'Jump to latest',
      detail: 'Return to the newest messages',
      run: () => handle?.jumpLatest(),
    },
    {
      id: 'history-focus',
      title: 'Focus conversation',
      detail: 'Scroll the whole conversation',
      hint: 'Alt+↑',
      run: () => handle?.focusHistory(),
    },
    {
      id: 'activity-focus',
      title: 'Focus activity',
      detail: 'Read this turn’s independent activity panel',
      hint: 'Alt+↓',
      run: () => handle?.focusActivity(),
    },
    { id: 'stop', title: 'Stop', detail: 'Cancel the current run', hint: 'Esc', run: stop },
  ];
  keymap.bind('mod+p', () => open('commands'), { when: 'always' });
  keymap.bind('mod+o', () => open('sessions'), { when: 'always' });
  keymap.bind('mod+g', () => open('models'), { when: 'always' });
  keymap.bind('mod+n', newConversation, { when: 'always' });
  keymap.bind('mod+l', () => handle?.jumpLatest(), { when: 'always' });
  keymap.bind('alt+up', () => handle?.focusHistory(), { when: 'always' });
  keymap.bind('alt+down', () => handle?.focusActivity(), { when: 'always' });
  effect(() => {
    if (!overlay()) keymap.bind('escape', stop, { when: 'always' });
  });
  const recoverControls = async () => {
    if (recovering()) return;
    recovering.set(true);
    error.set('');
    try {
      const originals = await host.pendingControls();
      for (const original of originals) {
        if (!original.reference) continue;
        try {
          const reply = await host.recoverControl(original.id);
          if (!alive) return;
          const result = returned(reply);
          if (original.method === 'create' && result.kind === 'created') {
            await switchSession(result.session.id);
            if (selected() === result.session.id && !result.session.backend) open('models');
          }
        } catch (reason) {
          report(reason);
        }
      }
      await handle?.refresh();
      await loadRecent();
      status.set('Original actions checked.');
    } catch (reason) {
      report(reason);
    } finally {
      await refreshControls().catch(report);
      if (alive) recovering.set(false);
    }
  };
  const abandonControl = async (id: string) => {
    try {
      await host.abandonControl(id);
      await refreshControls();
    } catch (reason) {
      report(reason);
    }
  };
  void loadRecent().catch(report);
  void refreshControls().catch(report);
  return (
    <View style={{ flex: 1, backgroundColor: C.background }}>
      <If when={selected() === null}>
        <View style={{ flex: 1 }}>
          <Header title="Your terminal, amplified." commands={() => open('commands')} />
          <View
            style={{
              flex: 1,
              width: '100%',
              maxWidth: 800,
              alignSelf: 'center',
              padding: 32,
              gap: 16,
              justifyContent: 'center',
            }}
          >
            <Text style={{ color: C.accent, fontWeight: '700' }}>Make something remarkable.</Text>
            <Text style={{ color: C.muted }}>A quiet place for ambitious work.</Text>
            <Button onPress={newConversation} style={{ paddingVertical: 16 }}>
              <Text style={{ color: C.text }}>+ New conversation · Ctrl+N</Text>
            </Button>
            <Each
              of={recent().slice(
                0,
                Math.max(1, Math.min(5, Math.floor(windowSize().height / 16) - 18)),
              )}
              key={(session) => session.id}
            >
              {(session) => (
                <Button
                  disabled={switching()}
                  onPress={() => {
                    void switchSession(session().id);
                  }}
                  style={{ paddingVertical: 0 }}
                >
                  <Text numberOfLines={1} style={{ color: C.text }}>
                    {session().title || 'Untitled conversation'}
                  </Text>
                  <Text numberOfLines={1} style={{ color: C.muted }}>
                    {session().workspace.path}
                  </Text>
                </Button>
              )}
            </Each>
            <Button onPress={() => open('sessions')}>
              <Text style={{ color: C.muted }}>All conversations · Ctrl+O</Text>
            </Button>
            <Text style={{ color: C.muted }}>Ctrl+P commands · Ctrl+Q exits</Text>
          </View>
        </View>
      </If>
      <Each of={selected() ? [selected()!] : []} key={(id: string) => id}>
        {(id) => (
          <Conversation
            client={host.session(id())}
            control={control}
            commands={() => open('commands')}
            models={() => open('models')}
            interaction={(sessionId, requestId) =>
              overlay.set({ kind: 'interaction', sessionId, requestId })
            }
            attach={(next) => {
              handle = next;
            }}
            detach={(previous) => {
              if (handle === previous) handle = null;
            }}
          />
        )}
      </Each>
      <If
        when={error() || status() || switching() || busy() || !controlsKnown() || controls().length}
      >
        <View style={{ backgroundColor: C.surface, paddingHorizontal: 16 }}>
          <If when={error()}>
            <Text selectable style={{ color: C.danger }}>
              {error()}
            </Text>
          </If>
          <If when={switching() || busy()}>
            <Text style={{ color: C.muted }}>
              {switching() ? 'Saving draft before switching…' : 'Confirming action…'}
            </Text>
          </If>
          <If when={status()}>
            <Text style={{ color: C.muted }}>{status()}</Text>
          </If>
          <If when={!controlsKnown() || controls().length > 0}>
            <Button
              disabled={recovering()}
              onPress={() => {
                void recoverControls();
              }}
            >
              <Text style={{ color: C.warning }}>
                {recovering()
                  ? 'Reading original action receipts…'
                  : controlsKnown()
                    ? `Recover original actions (${controls().length})`
                    : 'Recheck original actions'}
              </Text>
            </Button>
            <Each
              of={controls().filter((original) => !original.reference)}
              key={(original) => original.id}
            >
              {(original) => (
                <Button
                  onPress={() => {
                    void abandonControl(original().id);
                  }}
                >
                  <Text style={{ color: C.warning }}>
                    Abandon unconfirmed {original().method} · {original().id.slice(0, 8)}
                  </Text>
                </Button>
              )}
            </Each>
          </If>
        </View>
      </If>
      <Each
        of={overlay() ? [overlay()!] : []}
        key={(value: Overlay) =>
          `${value.kind}/${value.sessionId}/${'requestId' in value ? value.requestId : ''}`
        }
      >
        {(value) => (
          <If
            when={value().kind !== 'interaction'}
            else={
              <InteractionPanel
                sessionId={value().sessionId!}
                requestId={(value() as any).requestId}
                control={control}
                close={() => overlay.set(null)}
              />
            }
          >
            <Palette
              kind={value().kind as PaletteKind}
              sessionId={value().sessionId}
              commands={commands()}
              close={() => overlay.set(null)}
              selectSession={switchSession}
              create={create}
              chooseModel={chooseModel}
            />
          </If>
        )}
      </Each>
    </View>
  );
}
mount(App);
