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

import {
  batch,
  cleanup,
  computed,
  effect,
  Input,
  inputAction,
  signal,
  Text,
  untracked,
  View,
} from 'nsh';
import type { YNode } from 'nsh';
import { Button } from 'nsh/base';
import { keymap } from 'nsh/keymap';
import { List } from 'nsh/list';
import { C } from './ui';
import type { Browsing, BrowserScope, BrowserWorkspace } from './browsing';

export type WorkspaceBrowserProps = {
  browsing: Browsing;
  selected(): string | null;
  switching(): boolean;
  focusRequest(): boolean;
  focusHandled(): void;
  attach(node: YNode): void;
  close(): void;
  selectSession(id: string, expectedSessionId: string | null): Promise<void>;
  create(workspace: BrowserWorkspace, expectedSessionId: string | null): Promise<void>;
  newConversation(expectedSessionId: string | null): void;
  docked?: boolean;
  resize?(columns: number): void;
};

export function WorkspaceBrowser(props: WorkspaceBrowserProps) {
  const browser = props.browsing;
  const choosingWorkspace = signal(false);
  const selectedRow = signal<string | null>(null);
  const navigationNotice = signal('');
  const viewportHeight = signal(160);
  let search: YNode | undefined;
  let list: YNode | undefined;
  let removeSearchKeys = () => {};
  let removeEscape = () => {};
  let alive = true;
  cleanup(() => {
    alive = false;
    removeSearchKeys();
    removeEscape();
  });
  const scopeLabel = () => {
    const scope = browser.scope();
    if (scope.kind === 'all') return 'All workspaces';
    if (scope.kind === 'project')
      return (
        browser.projects().find((project) => project.reference.id === scope.projectId)?.name ??
        'Project'
      );
    return browser.sessions()[0]?.workspace.path.split('/').filter(Boolean).at(-1) ?? 'Workspace';
  };
  const scopes = computed(() => [
    { id: 'all', title: 'All workspaces', detail: '', scope: { kind: 'all' } as BrowserScope },
    ...(props.selected()
      ? [
          {
            id: 'workspace',
            title: 'Current workspace',
            detail: '',
            scope: { kind: 'workspace', sessionId: props.selected()! } as BrowserScope,
          },
        ]
      : []),
    ...browser
      .projects()
      .filter((project) => !project.archived)
      .map((project) => ({
        id: project.reference.id,
        title: project.name,
        detail: project.path ?? 'Unavailable',
        scope: { kind: 'project', projectId: project.reference.id } as BrowserScope,
      })),
  ]);
  const sessionsById = computed(
    () => new Map(browser.sessions().map((session) => [session.id, session])),
  );
  const scopesById = computed(() => new Map(scopes().map((scope) => [scope.id, scope])));
  const rowIds = computed(() =>
    choosingWorkspace()
      ? scopes().map((scope) => scope.id)
      : browser.sessions().map((session) => session.id),
  );
  effect(() => {
    if (selectedRow() === null && rowIds().length && !browser.loading())
      selectedRow.set(rowIds()[0]);
  });
  const index = () => rowIds().indexOf(selectedRow() ?? '');
  const currentRow = () => (index() < 0 ? undefined : selectedRow());
  const select = async ({ id, origin }: { id: string; origin: string | null }) => {
    if (!alive || props.selected() !== origin || props.switching()) return;
    navigationNotice.set('');
    await props.selectSession(id, origin);
    if (alive && id !== origin && props.selected() === origin)
      navigationNotice.set(
        'The draft is not ready to save. Return to the conversation to check its status.',
      );
  };
  const chooseScope = (scope: BrowserScope) => {
    if (!alive) return;
    batch(() => {
      browser.setScope(scope);
      choosingWorkspace.set(false);
      selectedRow.set(null);
    });
    list?.scrollToIndex({ index: 0, animated: false });
    search?.focus();
  };
  const activate = () => {
    const id = currentRow();
    if (!id) return;
    if (choosingWorkspace()) {
      const scope = scopes().find((scope) => scope.id === id);
      if (scope) chooseScope(scope.scope);
    } else select({ id, origin: props.selected() });
  };
  const move = (delta: number) => {
    const next =
      index() < 0
        ? delta > 0
          ? 0
          : rowIds().length - 1
        : Math.max(0, Math.min(rowIds().length - 1, index() + delta));
    selectedRow.set(rowIds()[next] ?? null);
    list?.scrollToIndex({ index: next, animated: false, viewPosition: delta > 0 ? 1 : 0 });
    if (!choosingWorkspace() && next >= rowIds().length - 3) void browser.loadMore();
  };
  const escape = () => {
    if (choosingWorkspace()) {
      choosingWorkspace.set(false);
      search?.focus();
    } else props.close();
  };
  const focusSearch = () => {
    if (!props.focusRequest() || !search) return;
    search.focus();
    search.setSelection(0, browser.titleQuery().length);
    props.focusHandled();
  };
  effect(() => {
    if (props.focusRequest()) untracked(focusSearch);
  });
  effect(() => {
    const rows = browser.sessions().length;
    if (
      !choosingWorkspace() &&
      !browser.loading() &&
      !browser.error() &&
      rows < Math.ceil(viewportHeight() / 32)
    )
      void browser.loadMore();
  });
  const createWorkspace = ({
    workspace,
    origin,
  }: {
    workspace: BrowserWorkspace;
    origin: string | null;
  }) => {
    if (alive && props.selected() === origin && !props.switching())
      void props.create(workspace, origin);
  };
  const toggleScopes = () => {
    batch(() => {
      choosingWorkspace.set(!choosingWorkspace());
      selectedRow.set(null);
    });
    search?.focus();
  };
  const refresh = () => {
    void browser.refresh();
  };
  return (
    <View
      ref={(node) => {
        props.attach(node);
        removeEscape();
        removeEscape = keymap.bind('escape', escape, { when: 'always', within: node });
      }}
      style={{ flex: 1, minHeight: 0, backgroundColor: C.surface, padding: 8, gap: 16 }}
    >
      <View style={{ flexDirection: 'row', justifyContent: 'space-between', gap: 8 }}>
        <Text style={{ color: C.accent, fontWeight: '700' }}>Sessions</Text>
        <Button onPress={inputAction(props.close, null)} accessibilityLabel="Close session browser">
          <Text style={{ color: C.muted }}>{props.docked ? 'Hide' : 'Close · Esc'}</Text>
        </Button>
      </View>
      <Button
        accessibilityLabel="Choose workspace scope"
        onPress={inputAction(toggleScopes, null)}
        style={{ backgroundColor: C.raised, paddingHorizontal: 8 }}
      >
        <Text numberOfLines={1} style={{ color: C.text }}>
          {scopeLabel()} ▾
        </Text>
      </Button>
      <Input
        ref={(node) => {
          search = node;
          removeSearchKeys();
          const unbind = [
            keymap.bind('up', () => move(-1), { when: 'always', repeat: true, within: node }),
            keymap.bind('down', () => move(1), { when: 'always', repeat: true, within: node }),
            keymap.bind('enter', activate, { when: 'always', within: node }),
          ];
          removeSearchKeys = () => {
            for (const remove of unbind) remove();
          };
          focusSearch();
        }}
        testID="session-search"
        accessibilityLabel="Search session titles"
        autoFocus={!props.docked}
        maxLength={256}
        value={choosingWorkspace() ? '' : browser.titleQuery()}
        readonly={choosingWorkspace()}
        onChange={(text) => {
          batch(() => {
            selectedRow.set(null);
            browser.setTitleQuery(text);
          });
          list?.scrollToIndex({ index: 0, animated: false });
        }}
        placeholder={choosingWorkspace() ? 'Choose a workspace below' : 'Search sessions…'}
        style={{
          color: C.text,
          placeholderColor: C.muted,
          backgroundColor: C.background,
          paddingHorizontal: 8,
          height: 32,
        }}
      />
      <List
        ref={(node) => {
          list = node;
        }}
        each={rowIds()}
        estimatedItemSize={32}
        onLayout={(frame) => viewportHeight.set(frame.height)}
        onEndReached={() => {
          if (!choosingWorkspace()) void browser.loadMore();
        }}
        style={{ flex: 1, minHeight: 32 }}
        accessibilityLabel={choosingWorkspace() ? 'Workspace scopes' : 'Conversations'}
      >
        {(id) => (
          <Each of={[id()]} key={(key: string) => key}>
            {(key) => {
              if (choosingWorkspace()) {
                const initial = scopesById().get(key());
                if (!initial) return <View />;
                const scope = () => scopesById().get(key()) ?? initial;
                const project = () =>
                  browser.projects().find((project) => project.reference.id === key());
                return (
                  <View style={{ flexDirection: 'row', height: 32 }}>
                    <Button
                      disabled={!scopesById().has(key())}
                      onPress={inputAction(chooseScope, scope().scope)}
                      style={{
                        flex: 1,
                        paddingHorizontal: 8,
                        backgroundColor: currentRow() === key() ? C.line : C.surface,
                      }}
                    >
                      <Text numberOfLines={1} style={{ color: C.text }}>
                        {scope()?.title}
                      </Text>
                      <Text numberOfLines={1} style={{ color: C.muted }}>
                        {scope()?.detail}
                      </Text>
                    </Button>
                    <If when={project()}>
                      <Button
                        accessibilityLabel={`New conversation in ${project()?.name}`}
                        disabled={
                          props.switching() || !project()?.available || !project()?.selection
                        }
                        onPress={
                          project()?.selection
                            ? inputAction(createWorkspace, {
                                workspace: {
                                  kind: 'project',
                                  project: project()!.reference,
                                  selection: project()!.selection!,
                                },
                                origin: props.selected(),
                              })
                            : undefined
                        }
                      >
                        <Text style={{ color: C.accent }}> + </Text>
                      </Button>
                    </If>
                  </View>
                );
              }
              const session = () => sessionsById().get(key());
              return (
                <Button
                  accessibilityLabel={`Open conversation ${session()?.title || 'Untitled conversation'}`}
                  disabled={props.switching() || !session()}
                  selected={props.selected() === key()}
                  onPress={inputAction(select, { id: key(), origin: props.selected() })}
                  style={{
                    height: 32,
                    paddingHorizontal: 8,
                    backgroundColor: currentRow() === key() ? C.line : C.surface,
                  }}
                  hoverStyle={{ backgroundColor: C.line }}
                  focusStyle={{ backgroundColor: C.raised }}
                >
                  <Text
                    numberOfLines={1}
                    style={{
                      color: props.selected() === key() || session()?.hasUnread ? C.accent : C.text,
                    }}
                  >
                    {currentRow() === key() ? '› ' : session()?.hasUnread ? '● ' : '  '}
                    {session()?.title || 'Untitled conversation'}
                  </Text>
                  <Text numberOfLines={1} style={{ color: C.muted }}>
                    {session()?.activity === 'working'
                      ? '● Working · '
                      : session()?.activity === 'waiting'
                        ? '◇ Waiting · '
                        : ''}
                    {session()?.workspace.path}
                  </Text>
                </Button>
              );
            }}
          </Each>
        )}
      </List>
      <If when={browser.loading()}>
        <Text style={{ color: C.muted }}>Loading…</Text>
      </If>
      <If when={navigationNotice()}>
        <Text style={{ color: C.warning }}>{navigationNotice()}</Text>
        <Button onPress={inputAction(props.close, null)}>
          <Text style={{ color: C.accent }}>Return to draft</Text>
        </Button>
      </If>
      <If when={!browser.loading() && !browser.error() && !rowIds().length}>
        <Text style={{ color: C.muted }}>No matching conversations.</Text>
      </If>
      <If when={browser.error() || browser.projectError()}>
        <Text selectable style={{ color: C.danger }}>
          {browser.error() || browser.projectError()}
        </Text>
        <Button onPress={inputAction(refresh, null)}>
          <Text style={{ color: C.accent }}>Refresh browser</Text>
        </Button>
      </If>
      <Button
        disabled={props.switching()}
        onPress={inputAction(props.newConversation, props.selected())}
      >
        <Text style={{ color: C.text }}>
          {props.selected() ? '+ New in current workspace' : '+ New conversation'}
        </Text>
      </Button>
      <If when={props.docked}>
        <View style={{ flexDirection: 'row', justifyContent: 'space-between' }}>
          <Button
            onPress={props.resize ? inputAction(props.resize, -2) : undefined}
            accessibilityLabel="Narrower session browser"
          >
            <Text style={{ color: C.muted }}>‹ Narrower</Text>
          </Button>
          <Button
            onPress={props.resize ? inputAction(props.resize, 2) : undefined}
            accessibilityLabel="Wider session browser"
          >
            <Text style={{ color: C.muted }}>Wider ›</Text>
          </Button>
        </View>
      </If>
      <Text numberOfLines={1} style={{ color: C.muted }}>
        Titles · match case
      </Text>
      <Text numberOfLines={1} style={{ color: C.muted }}>
        Enter selects · Esc closes
      </Text>
    </View>
  );
}
