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

import { cleanup, Input, inputAction, Scroll, signal, Text, View } from 'nsh';
import { Button, Dialog } from 'nsh/base';
import { host } from './host';
import type { BrowserSession } from './browsing';
import { forkTitle, SessionRevisionConflict } from './sessionLifecycle';
import type { SessionAction } from './sessionLifecycle';
import { C } from './ui';

type Page = 'actions' | 'rename' | 'fork' | 'remove';
export function SessionActions(props: {
  session: BrowserSession;
  current(): boolean;
  close(): void;
  change(action: SessionAction): Promise<void>;
}) {
  const session = signal(props.session);
  const page = signal<Page>('actions');
  const title = signal('');
  const pending = signal(false);
  const error = signal('');
  let alive = true;
  cleanup(() => {
    alive = false;
  });
  const active = () => alive && props.current();
  const sameSession = (original: BrowserSession) =>
    original.id === session().id && original.revision === session().revision;
  const back = (from: Page) => {
    if (!active() || pending() || page() !== from) return;
    if (page() === 'actions') props.close();
    else {
      page.set('actions');
      error.set('');
    }
  };
  const edit = ({ to, original }: { to: Page; original: BrowserSession }) => {
    if (!active() || pending() || page() !== 'actions' || !sameSession(original)) return;
    error.set('');
    title.set(to === 'fork' ? forkTitle(original.title) : original.title);
    page.set(to);
  };
  const submit = async (action: SessionAction) => {
    if (!active() || pending() || !sameSession(action.session)) return;
    if (action.kind === 'rename' || action.kind === 'fork' || action.kind === 'remove') {
      if (page() !== action.kind) return;
    } else if (page() !== 'actions' || action.kind !== (session().archived ? 'restore' : 'archive'))
      return;
    if (
      (action.kind === 'rename' || action.kind === 'fork') &&
      (action.title !== title() || !action.title.trim() || [...action.title].length > 128)
    )
      return;
    pending.set(true);
    error.set('');
    try {
      await props.change(action);
      if (active()) props.close();
    } catch (reason) {
      if (!active()) return;
      error.set(reason instanceof Error ? reason.message : String(reason));
      if (reason instanceof SessionRevisionConflict) {
        // Keep the typed title. The refreshed snapshot supplies the next
        // explicitly requested action, never an automatic mutation retry.
        try {
          const observed = await host.query('agent', 'observe', { sessionId: action.session.id });
          if (active()) session.set(observed.session);
        } catch (refreshError) {
          if (active())
            error.set(
              `${error()} ${refreshError instanceof Error ? refreshError.message : String(refreshError)}`,
            );
        }
      }
    } finally {
      if (alive) pending.set(false);
    }
  };
  return (
    <Dialog
      animation="none"
      closeOnEscape={false}
      closeOnBackdropPress={false}
      closeOnSwipe={false}
      onCancel={() => back(page())}
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
          maxWidth: 560,
          maxHeight: '100%',
          padding: 16,
          gap: 8,
          backgroundColor: C.surface,
        }}
      >
        <View style={{ flexDirection: 'row', gap: 8 }}>
          <Text numberOfLines={2} style={{ flex: 1, color: C.accent, fontWeight: '700' }}>
            {page() === 'rename'
              ? 'Rename conversation'
              : page() === 'fork'
                ? 'Fork conversation'
                : page() === 'remove'
                  ? 'Delete conversation?'
                  : 'Conversation actions'}
          </Text>
          <Button
            disabled={pending()}
            accessibilityLabel="Back from conversation actions"
            onPress={inputAction(back, page())}
          >
            <Text style={{ color: C.muted }}>
              {page() === 'actions' ? 'Close · Esc' : 'Back · Esc'}
            </Text>
          </Button>
        </View>
        <Scroll style={{ flexShrink: 1 }}>
          <View style={{ gap: 8 }}>
            <Text selectable style={{ color: C.text }}>
              {session().title || 'Untitled conversation'}
            </Text>
            <Text numberOfLines={2} style={{ color: C.muted }}>
              {session().workspace.path}
            </Text>
            <If when={session().archived}>
              <Text style={{ color: C.warning }}>Archived conversation</Text>
            </If>
            <If when={error()}>
              <Text selectable style={{ color: C.danger }}>
                {error()}
              </Text>
            </If>
            <If when={pending()}>
              <Text style={{ color: C.muted }}>Confirming the original action…</Text>
            </If>
            <If when={page() === 'actions'}>
              <Button
                disabled={pending() || session().archived}
                accessibilityLabel="Rename conversation"
                onPress={inputAction(edit, { to: 'rename', original: session() })}
              >
                <Text style={{ color: C.text }}>Rename</Text>
              </Button>
              <Button
                disabled={pending()}
                accessibilityLabel="Fork conversation"
                onPress={inputAction(edit, { to: 'fork', original: session() })}
              >
                <Text style={{ color: C.text }}>Fork into a new conversation</Text>
              </Button>
              <Button
                disabled={pending()}
                accessibilityLabel={
                  session().archived ? 'Restore conversation' : 'Archive conversation'
                }
                onPress={inputAction(submit, {
                  kind: session().archived ? 'restore' : 'archive',
                  session: session(),
                })}
              >
                <Text style={{ color: C.text }}>{session().archived ? 'Restore' : 'Archive'}</Text>
              </Button>
              <Button
                disabled={pending()}
                accessibilityLabel="Delete conversation"
                onPress={inputAction(edit, { to: 'remove', original: session() })}
              >
                <Text style={{ color: C.danger }}>Delete…</Text>
              </Button>
            </If>
            <If when={page() === 'rename' || page() === 'fork'}>
              <Input
                ref={(node) => {
                  node.focus();
                  node.setSelection(0, title().length);
                }}
                autoFocus
                accessibilityLabel="Conversation title"
                value={title()}
                maxLength={256}
                readonly={pending()}
                onChange={(value) => title.set(value)}
                placeholder="Conversation title"
                style={{
                  height: 32,
                  color: C.text,
                  backgroundColor: C.background,
                  paddingHorizontal: 8,
                }}
              />
              <If when={page() === 'fork'}>
                <Text style={{ color: C.muted }}>
                  Copy the current history into an independent conversation. This draft stays with
                  its original conversation.
                </Text>
              </If>
              <Text style={{ color: [...title()].length > 128 ? C.danger : C.muted }}>
                {[...title()].length}/128 characters
              </Text>
              <Button
                disabled={pending() || !title().trim() || [...title()].length > 128}
                accessibilityLabel={
                  page() === 'fork' ? 'Create conversation fork' : 'Save conversation title'
                }
                onPress={inputAction(
                  submit,
                  page() === 'fork'
                    ? { kind: 'fork', session: session(), title: title(), turnId: null }
                    : { kind: 'rename', session: session(), title: title() },
                )}
              >
                <Text style={{ color: C.accent }}>
                  {page() === 'fork' ? 'Create fork' : 'Save title'}
                </Text>
              </Button>
            </If>
            <If when={page() === 'remove'}>
              <Text style={{ color: C.warning }}>
                Permanently delete this conversation and its history. Workspace files and
                independent forks are kept.
              </Text>
              <Button
                disabled={pending()}
                accessibilityLabel="Confirm delete conversation"
                onPress={inputAction(submit, { kind: 'remove', session: session() })}
              >
                <Text style={{ color: C.danger }}>Delete permanently</Text>
              </Button>
              <Button disabled={pending()} onPress={inputAction(back, 'remove')}>
                <Text style={{ color: C.text }}>Keep conversation</Text>
              </Button>
            </If>
          </View>
        </Scroll>
      </View>
    </Dialog>
  );
}
