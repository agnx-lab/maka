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
import type { YNode } from 'nsh';
import { Button, Dialog } from 'nsh/base';
import { host } from 'maka/host';
import { keymap } from 'nsh/keymap';
import { C, Header } from './ui';
import { Composer } from './Composer';
import { Conversation } from './Conversation';
import type { Control, ConversationHandle } from './Conversation';
import { Palette } from './Palette';
import type { PaletteEntry } from './Palette';
import { WorkspacePicker } from './WorkspacePicker';
import { ModelSelect } from './ModelSelect';
import type { ModelBackend } from './modelSelection';
import { InteractionPanel } from './InteractionPanel';
import type { ImagePreview } from './ToolImage';
import { createResources } from './resources';
import { createPresentationSettings } from './presentation';
import { createBrowsing } from './browsing';
import type { BrowserWorkspace } from './browsing';
import { Workbench, canDockBrowser } from './Workbench';
import { WorkspaceBrowser, type WorkspaceBrowserProps } from './WorkspaceBrowser';

type Overlay = { instance: number } & (
  | { kind: 'commands' | 'workspaces' | 'models' | 'sessions'; sessionId: string | null }
  | { kind: 'interaction'; sessionId: string; requestId: string }
);
type CommandContext = { instance: number; sessionId: string | null };
function App() {
  const presentation = createPresentationSettings();
  const browsing = createBrowsing();
  const selected = signal<string | null>(host.initialSession);
  const overlay = signal<Overlay | null>(null);
  const resources = createResources({ selected, blocked: () => overlay() !== null });
  const sidebarExpanded = signal(true);
  const browserFocus = signal(false);
  const error = signal('');
  const status = signal('');
  const switching = signal(false);
  const busy = signal(false);
  const controls = signal<any[]>([]);
  const controlsKnown = signal(false);
  const recovering = signal(false);
  const handle = signal<ConversationHandle | null>(null);
  let editor: YNode | undefined;
  const focusComposer = () => editor?.focus();
  let browserRoot: YNode | undefined;
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
  const switchSession = async (id: string, expectedSessionId = selected()) => {
    if (!alive || selected() !== expectedSessionId || selected() === id || switching()) return;
    switching.set(true);
    try {
      if (handle() && !(await handle()!.flushDraft())) return;
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
    if (overlay()?.kind === 'sessions') browserFocus.set(false);
    overlay.set(null);
    return true;
  };
  const open = ({
    kind,
    sessionId,
  }: {
    kind: 'commands' | 'workspaces' | 'models' | 'sessions';
    sessionId: string | null;
  }) => {
    if (!alive || selected() !== sessionId) return;
    if (kind === 'sessions' && canDockBrowser()) {
      resources.hide();
      status.set('');
      overlay.set(null);
      sidebarExpanded.set(true);
      browserFocus.set(true);
      return;
    }
    if (kind === 'models' && sessionId === null) {
      status.set('Open or create a conversation before choosing its model.');
      return;
    }
    status.set('');
    browserFocus.set(kind === 'sessions');
    overlay.set({ kind, sessionId, instance: ++overlayInstance });
  };
  const chooseModel = async (
    owner: ConversationHandle,
    instance: number,
    backend: ModelBackend,
  ) => {
    if (
      !alive ||
      selected() !== owner.sessionId ||
      handle() !== owner ||
      overlay()?.instance !== instance
    )
      throw new Error('This model selection is no longer open.');
    const result = await control('selectBackend', {
      sessionId: owner.sessionId,
      backend: { kind: 'model', ...backend },
    });
    if (result.kind !== 'committed')
      throw new Error(`Model selection rejected: ${result.reason ?? result.kind}`);
    await owner.refresh();
  };
  const create = async (workspace: BrowserWorkspace, expectedSessionId = selected()) => {
    if (!alive || selected() !== expectedSessionId) return;
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
      await switchSession(result.session.id, expectedSessionId);
      if (selected() === result.session.id) open({ kind: 'models', sessionId: result.session.id });
      await browsing.refresh();
    } catch (reason) {
      report(reason);
    }
  };
  const newConversation = (sessionId: string | null) => {
    if (!alive || selected() !== sessionId) return;
    if (sessionId) void create({ kind: 'session', sessionId }, sessionId);
    else open({ kind: 'workspaces', sessionId });
  };
  const stop = () => {
    if (!overlay() && handle()) void handle()!.stop(handle()!.invocation()).catch(report);
  };
  const closeCommand = (context: CommandContext) =>
    alive &&
    selected() === context.sessionId &&
    (context.sessionId === null || handle()?.sessionId === context.sessionId) &&
    closeOverlay(context.instance);
  const commandSessions = (context: CommandContext) => {
    if (closeCommand(context)) open({ kind: 'sessions', sessionId: context.sessionId });
  };
  const commandNew = (context: CommandContext) => {
    if (closeCommand(context)) newConversation(context.sessionId);
  };
  const commandWorkspace = (context: CommandContext) => {
    if (closeCommand(context)) open({ kind: 'workspaces', sessionId: context.sessionId });
  };
  const commandModels = (context: CommandContext) => {
    if (closeCommand(context)) open({ kind: 'models', sessionId: context.sessionId });
  };
  const commandFiles = (context: CommandContext) => {
    if (closeCommand(context) && context.sessionId)
      resources.openFiles({ sessionId: context.sessionId });
  };
  const commandLatest = (context: CommandContext & { turnId: string | null }) => {
    if (closeCommand(context)) handle()?.jumpLatest(context.turnId);
  };
  const commandHistory = (context: CommandContext) => {
    if (closeCommand(context)) handle()?.focusHistory();
  };
  const commandActivity = (context: CommandContext & { turnId: string | null }) => {
    if (closeCommand(context)) handle()?.focusActivity(context.turnId);
  };
  const commandStop = (context: CommandContext & { invocationId: string | null }) => {
    if (closeCommand(context)) void handle()?.stop(context.invocationId).catch(report);
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
      id: 'workspace',
      title: 'Choose workspace',
      detail: 'Start a conversation in another folder',
      run: inputAction(commandWorkspace, context),
    },
    {
      id: 'models',
      title: 'Models',
      detail: 'Choose the model for this conversation',
      hint: 'Ctrl+G',
      run: inputAction(commandModels, context),
    },
    ...(context.sessionId
      ? [
          {
            id: 'files',
            title: 'Files',
            detail: 'Browse workspace files and current changes',
            run: inputAction(commandFiles, context),
          },
        ]
      : []),
    {
      id: 'latest',
      title: 'Jump to latest',
      detail: 'Return to the newest messages',
      run: inputAction(commandLatest, {
        instance: context.instance,
        sessionId: context.sessionId,
        turnId: handle()?.sessionId === context.sessionId ? handle()!.activeTurn() : null,
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
        turnId: handle()?.sessionId === context.sessionId ? handle()!.activityTurn() : null,
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
        invocationId: handle()?.sessionId === context.sessionId ? handle()!.invocation() : null,
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
      if (handle()) handle()!.jumpLatest(handle()!.activeTurn());
    },
    { when: 'always' },
  );
  keymap.bind('alt+up', () => handle()?.focusHistory(), { when: 'always' });
  keymap.bind(
    'alt+down',
    () => {
      if (handle()) handle()!.focusActivity(handle()!.activityTurn());
    },
    { when: 'always' },
  );
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
      await handle()?.refresh();
      await browsing.refresh();
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
  const openInteraction = ({ sessionId, requestId }: { sessionId: string; requestId: string }) => {
    if (!alive || selected() !== sessionId) return;
    browserFocus.set(false);
    overlay.set({ kind: 'interaction', sessionId, requestId, instance: ++overlayInstance });
  };
  const previewImage = (image: ImagePreview) => {
    if (!alive || selected() !== image.source.sessionId || overlay()) return;
    browserFocus.set(false);
    resources.openImage(image);
  };
  const closeSidebar = () => {
    if (!alive || overlay()) return;
    sidebarExpanded.set(false);
    browserFocus.set(false);
    handle()?.focus();
  };
  const selectBrowsed = async (id: string, origin: string | null) => {
    const current = overlay();
    const instance = current?.kind === 'sessions' ? current.instance : null;
    await switchSession(id, origin);
    if (!alive || selected() !== id) return;
    if (instance !== null) closeOverlay(instance);
    browserFocus.set(false);
    handle()?.focus();
  };
  const browserProps: WorkspaceBrowserProps = {
    browsing,
    selected,
    switching: () => switching() || busy(),
    focusRequest: browserFocus,
    focusHandled: () => browserFocus.set(false),
    attach: (node) => {
      // Retain the identity across conditional unmount. Resize reads the
      // native focus path from before that presentation change.
      browserRoot = node;
    },
    close: closeSidebar,
    selectSession: selectBrowsed,
    create,
    newConversation,
  };
  let wasDocked = canDockBrowser();
  effect(() => {
    const docked = canDockBrowser();
    const current = overlay();
    if (docked && current?.kind === 'sessions') {
      closeOverlay(current.instance);
      sidebarExpanded.set(true);
      browserFocus.set(true);
    } else if (
      !docked &&
      wasDocked &&
      current === null &&
      browserRoot &&
      keymap.focusedWithin(browserRoot)
    ) {
      open({ kind: 'sessions', sessionId: selected() });
    }
    wasDocked = docked;
  });
  void refreshControls().catch(report);
  return (
    <View style={{ flex: 1, backgroundColor: C.background }}>
      <Workbench
        expanded={() => sidebarExpanded() && overlay()?.kind !== 'sessions'}
        browser={browserProps}
        stop={stop}
        resources={resources}
      >
        <If when={selected() === null}>
          <View>
            <Header
              title="Your terminal, amplified."
              commands={inputAction(open, { kind: 'commands', sessionId: null })}
              browse={inputAction(open, { kind: 'sessions', sessionId: null })}
            />
          </View>
        </If>
        <Each of={selected() ? [selected()!] : []} key={(id: string) => id}>
          {(id) => (
            <Conversation
              client={host.session(id())}
              presentation={presentation}
              control={control}
              commands={inputAction(open, { kind: 'commands', sessionId: id() })}
              browse={inputAction(open, { kind: 'sessions', sessionId: id() })}
              files={inputAction(resources.openFiles, { sessionId: id() })}
              filesOpener={(node) => resources.attachOpener(id(), node)}
              interaction={openInteraction}
              previewImage={previewImage}
              focusComposer={focusComposer}
              attach={(next) => {
                handle.set(next);
              }}
              detach={(previous) => {
                if (handle() === previous) handle.set(null);
              }}
            />
          )}
        </Each>
        <Composer
          session={handle}
          attach={(node) => {
            editor = node;
          }}
          commands={({ sessionId }) => open({ kind: 'commands', sessionId })}
          models={({ sessionId }) => open({ kind: 'models', sessionId })}
          chooseWorkspace={inputAction(open, { kind: 'workspaces', sessionId: null })}
        />
      </Workbench>
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
          if (current.kind === 'interaction')
            return (
              <InteractionPanel
                sessionId={current.sessionId}
                requestId={current.requestId}
                control={control}
                close={close}
              />
            );
          if (current.kind === 'sessions')
            return (
              <Dialog
                animation="none"
                onOpenChange={() => closeOverlay(current.instance)}
                style={{
                  flex: 1,
                  justifyContent: 'center',
                  alignItems: 'center',
                  padding: 16,
                  backgroundColor: '#00000099',
                }}
              >
                <View
                  style={{
                    width: '100%',
                    maxWidth: 640,
                    height: '100%',
                    maxHeight: 640,
                    backgroundColor: C.surface,
                    padding: 8,
                  }}
                >
                  <WorkspaceBrowser
                    {...browserProps}
                    docked={false}
                    close={() => closeOverlay(current.instance)}
                  />
                </View>
              </Dialog>
            );
          if (current.kind === 'models')
            return (
              <Each<ConversationHandle>
                of={handle()?.sessionId === current.sessionId ? [handle()!] : []}
                key={(owner: ConversationHandle) => owner.sessionId}
              >
                {(item) => {
                  const owner = item();
                  return (
                    <ModelSelect
                      owner={owner}
                      instance={current.instance}
                      current={() =>
                        overlay()?.instance === current.instance &&
                        selected() === owner.sessionId &&
                        handle() === owner
                      }
                      close={closeOverlay}
                      select={(backend) => chooseModel(owner, current.instance, backend)}
                    />
                  );
                }}
              </Each>
            );
          if (current.kind === 'workspaces')
            return (
              <WorkspacePicker
                instance={current.instance}
                current={() =>
                  overlay()?.instance === current.instance && selected() === current.sessionId
                }
                close={closeOverlay}
                create={(workspace) => create(workspace, current.sessionId)}
              />
            );
          return (
            <Palette
              instance={current.instance}
              commands={commands({ instance: current.instance, sessionId: current.sessionId })}
              close={closeOverlay}
            />
          );
        }}
      </Each>
    </View>
  );
}
mount(App);
