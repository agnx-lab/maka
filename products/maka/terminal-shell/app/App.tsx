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

import { cleanup, effect, inputAction, mount, signal, Text, View } from 'nsh';
import { Image } from 'nsh/image';
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
import { ImageViewer, type ImagePreview } from './ToolImage';
import { createPresentationSettings } from './presentation';
import logo from './assets/maka.png';

type Overlay = { instance: number } & (
  | { kind: PaletteKind; sessionId: string | null }
  | { kind: 'interaction'; sessionId: string; requestId: string }
  | { kind: 'image'; sessionId: string; image: ImagePreview }
);
type CommandContext = { instance: number; sessionId: string | null };
function App() {
  const presentation = createPresentationSettings();
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
  const showHomeLogo = () => windowSize().height >= 336;
  const homeRecentLimit = () => {
    // Header, padding, controls and gaps take 16 rows; the logo adds five.
    // Each recent entry takes two text rows and its following gap.
    const available = Math.floor(windowSize().height / 16) - (showHomeLogo() ? 21 : 16);
    return Math.max(0, Math.min(5, Math.floor(available / 3)));
  };
  let handle: ConversationHandle | null = null;
  let alive = true;
  let controlRead = 0;
  let overlayInstance = 0;
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
  const switchSession = async (id: string, expectedSessionId = selected()) => {
    if (!alive || selected() !== expectedSessionId || selected() === id || switching()) return;
    switching.set(true);
    try {
      if (handle && !(await handle.flushDraft())) return;
      if (alive) {
        overlay.set(null);
        selected.set(id);
      }
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
  const closeOverlay = (instance: number) => {
    if (!alive || overlay()?.instance !== instance) return false;
    overlay.set(null);
    return true;
  };
  const open = ({ kind, sessionId }: { kind: PaletteKind; sessionId: string | null }) => {
    if (!alive || selected() !== sessionId) return;
    if (kind === 'models' && sessionId === null) {
      status.set('Open or create a conversation before choosing its model.');
      return;
    }
    status.set('');
    overlay.set({ kind, sessionId, instance: ++overlayInstance });
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
      if (selected() === result.session.id) open({ kind: 'models', sessionId: result.session.id });
      await loadRecent();
    } catch (reason) {
      report(reason);
    }
  };
  const newConversation = (sessionId: string | null) => {
    if (!alive || selected() !== sessionId) return;
    if (sessionId) void create({ kind: 'session', sessionId });
    else open({ kind: 'workspaces', sessionId });
  };
  const stop = () => {
    if (handle) void handle.stop(handle.invocation()).catch(report);
  };
  const closeCommand = (context: CommandContext) =>
    alive &&
    selected() === context.sessionId &&
    (context.sessionId === null || handle?.sessionId === context.sessionId) &&
    closeOverlay(context.instance);
  const commandSessions = (context: CommandContext) => {
    if (closeCommand(context)) open({ kind: 'sessions', sessionId: context.sessionId });
  };
  const commandNew = (context: CommandContext) => {
    if (closeCommand(context)) newConversation(context.sessionId);
  };
  const commandModels = (context: CommandContext) => {
    if (closeCommand(context)) open({ kind: 'models', sessionId: context.sessionId });
  };
  const commandLatest = (context: CommandContext & { turnId: string | null }) => {
    if (closeCommand(context)) handle?.jumpLatest(context.turnId);
  };
  const commandHistory = (context: CommandContext) => {
    if (closeCommand(context)) handle?.focusHistory();
  };
  const commandActivity = (context: CommandContext & { turnId: string | null }) => {
    if (closeCommand(context)) handle?.focusActivity(context.turnId);
  };
  const commandStop = (context: CommandContext & { invocationId: string | null }) => {
    if (closeCommand(context)) void handle?.stop(context.invocationId).catch(report);
  };
  const commands = (context: CommandContext): PaletteEntry[] => [
    {
      id: 'sessions',
      title: 'Sessions',
      detail: 'Resume a conversation',
      hint: 'Ctrl+O',
      run: inputAction(commandSessions, context),
    },
    {
      id: 'new',
      title: 'New conversation',
      detail: 'A fresh start in your workspace',
      hint: 'Ctrl+N',
      run: inputAction(commandNew, context),
    },
    {
      id: 'models',
      title: 'Models',
      detail: 'Choose the model for this conversation',
      hint: 'Ctrl+G',
      run: inputAction(commandModels, context),
    },
    {
      id: 'latest',
      title: 'Jump to latest',
      detail: 'Return to the newest messages',
      run: inputAction(commandLatest, {
        instance: context.instance,
        sessionId: context.sessionId,
        turnId: handle?.sessionId === context.sessionId ? handle.activeTurn() : null,
      }),
    },
    {
      id: 'history-focus',
      title: 'Focus conversation',
      detail: 'Scroll the whole conversation',
      hint: 'Alt+↑',
      run: inputAction(commandHistory, context),
    },
    {
      id: 'activity-focus',
      title: 'Focus activity',
      detail: 'Read this turn’s independent activity panel',
      hint: 'Alt+↓',
      run: inputAction(commandActivity, {
        instance: context.instance,
        sessionId: context.sessionId,
        turnId: handle?.sessionId === context.sessionId ? handle.activityTurn() : null,
      }),
    },
    {
      id: 'stop',
      title: 'Stop',
      detail: 'Cancel the current run',
      hint: 'Esc',
      run: inputAction(commandStop, {
        instance: context.instance,
        sessionId: context.sessionId,
        invocationId: handle?.sessionId === context.sessionId ? handle.invocation() : null,
      }),
    },
  ];
  keymap.bind('mod+p', () => open({ kind: 'commands', sessionId: selected() }), { when: 'always' });
  keymap.bind('mod+o', () => open({ kind: 'sessions', sessionId: selected() }), { when: 'always' });
  keymap.bind('mod+g', () => open({ kind: 'models', sessionId: selected() }), { when: 'always' });
  keymap.bind('mod+n', () => newConversation(selected()), { when: 'always' });
  keymap.bind(
    'mod+l',
    () => {
      if (handle) handle.jumpLatest(handle.activeTurn());
    },
    { when: 'always' },
  );
  keymap.bind('alt+up', () => handle?.focusHistory(), { when: 'always' });
  keymap.bind(
    'alt+down',
    () => {
      if (handle) handle.focusActivity(handle.activityTurn());
    },
    { when: 'always' },
  );
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
            if (selected() === result.session.id && !result.session.backend)
              open({ kind: 'models', sessionId: result.session.id });
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
  const selectRecent = ({ sessionId }: { sessionId: string }) => {
    if (selected() === null) void switchSession(sessionId);
  };
  const openInteraction = ({ sessionId, requestId }: { sessionId: string; requestId: string }) => {
    if (!alive || selected() !== sessionId) return;
    overlay.set({ kind: 'interaction', sessionId, requestId, instance: ++overlayInstance });
  };
  const previewImage = (image: ImagePreview) => {
    if (!alive || selected() !== image.source.sessionId || overlay()) return;
    overlay.set({
      kind: 'image',
      sessionId: image.source.sessionId,
      image,
      instance: ++overlayInstance,
    });
  };
  void loadRecent().catch(report);
  void refreshControls().catch(report);
  return (
    <View style={{ flex: 1, backgroundColor: C.background }}>
      <If when={selected() === null}>
        <View style={{ flex: 1 }}>
          <Header
            title="Your terminal, amplified."
            commands={inputAction(open, { kind: 'commands', sessionId: null })}
          />
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
            <If when={showHomeLogo()}>
              <Image
                src={logo}
                resizeMode="contain"
                style={{ width: 64, height: 64, flexShrink: 0 }}
              />
            </If>
            <Text style={{ color: C.accent, fontWeight: '700' }}>Make something remarkable.</Text>
            <Text style={{ color: C.muted }}>A quiet place for ambitious work.</Text>
            <Button onPress={inputAction(newConversation, null)} style={{ paddingVertical: 16 }}>
              <Text style={{ color: C.text }}>+ New conversation · Ctrl+N</Text>
            </Button>
            <Each of={recent().slice(0, homeRecentLimit())} key={(session) => session.id}>
              {(session) => (
                <Button
                  disabled={switching()}
                  onPress={inputAction(selectRecent, { sessionId: session().id })}
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
            <Button onPress={inputAction(open, { kind: 'sessions', sessionId: null })}>
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
            presentation={presentation}
            control={control}
            commands={inputAction(open, { kind: 'commands', sessionId: id() })}
            models={inputAction(open, { kind: 'models', sessionId: id() })}
            interaction={openInteraction}
            previewImage={previewImage}
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
            <Button disabled={recovering()} onPress={inputAction(recoverControls, null)}>
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
                <Button onPress={inputAction(abandonControl, original().id)}>
                  <Text style={{ color: C.warning }}>
                    Abandon unconfirmed {original().method} · {original().id.slice(0, 8)}
                  </Text>
                </Button>
              )}
            </Each>
          </If>
        </View>
      </If>
      <Each of={overlay() ? [overlay()!] : []} key={(value: Overlay) => value.instance}>
        {(value) => {
          const current = value();
          const close = inputAction(closeOverlay, current.instance);
          if (current.kind === 'image') return <ImageViewer image={current.image} close={close} />;
          if (current.kind === 'interaction')
            return (
              <InteractionPanel
                sessionId={current.sessionId}
                requestId={current.requestId}
                control={control}
                close={close}
              />
            );
          return (
            <Palette
              kind={current.kind}
              sessionId={current.sessionId}
              instance={current.instance}
              commands={commands({ instance: current.instance, sessionId: current.sessionId })}
              close={closeOverlay}
              selectSession={switchSession}
              create={create}
              chooseModel={chooseModel}
            />
          );
        }}
      </Each>
    </View>
  );
}
mount(App);
