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

import { cleanup, computed, effect, Input, signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { List } from 'nsh/list';
import { host } from 'maka/host';
import { keymap } from 'nsh/keymap';
import { windowSize } from 'nsh/device';
import { Button, Dialog } from 'nsh/base';
import { C } from './ui';

export type PaletteEntry = {
  id: string;
  title: string;
  detail?: string;
  hint?: string;
  run(): void;
  keepOpen?: boolean;
};
export type PaletteKind = 'commands' | 'sessions' | 'models' | 'workspaces';

export function Palette(props: {
  kind: PaletteKind;
  sessionId: string | null;
  commands: PaletteEntry[];
  close(): void;
  selectSession(id: string): Promise<void>;
  create(workspace: any): Promise<void>;
  chooseModel(id: string, choice: any): Promise<void>;
}) {
  const query = signal('');
  const selected = signal(0);
  const entries = signal<PaletteEntry[]>(props.kind === 'commands' ? props.commands : []);
  const loading = signal(false);
  const error = signal('');
  const note = signal('');
  let alive = true;
  let generation = 0;
  let page: any = null;
  let sessions: any[] = [];
  let choices: any[] = [];
  cleanup(() => {
    alive = false;
    generation += 1;
  });
  const matches = computed(() => {
    const needle = query().toLowerCase().trim();
    // Settings performs model search; sessions are filtered within the loaded pages.
    return props.kind === 'models'
      ? entries()
      : entries().filter((entry) =>
          `${entry.title} ${entry.detail ?? ''}`.toLowerCase().includes(needle),
        );
  });
  const index = () => Math.min(selected(), Math.max(0, matches().length - 1));
  const count = () => Math.max(1, Math.min(8, Math.floor((windowSize().height / 16 - 14) / 3)));
  const byId = computed(() => new Map(matches().map((entry) => [entry.id, entry])));
  const ids = computed(() => matches().map((entry) => entry.id));
  let list: YNode | undefined;
  const move = (delta: number) => {
    const next = Math.max(0, Math.min(matches().length - 1, index() + delta));
    selected.set(next);
    list?.scrollToIndex({ index: next, animated: false, viewPosition: delta > 0 ? 1 : 0 });
    if (delta > 0 && next >= matches().length - count()) loadMore();
  };
  const activate = () => {
    const entry = matches()[index()];
    if (!entry) return;
    if (!entry.keepOpen) props.close();
    entry.run();
  };
  keymap.bind('up', () => move(-1), { when: 'always', repeat: true });
  keymap.bind('down', () => move(1), { when: 'always', repeat: true });
  keymap.bind('enter', activate, { when: 'always' });
  const load = async (search: string, more = false) => {
    const reading = ++generation;
    const minimum = more ? matches().length + count() : count();
    loading.set(true);
    error.set('');
    if (props.kind === 'models' && !more) {
      entries.set([]);
      page = null;
      choices = [];
    }
    try {
      if (props.kind === 'sessions') {
        do {
          const result = await host.query('agent', 'list', {
            filter: 'all',
            position: more
              ? { kind: 'after', revision: page.revision, cursor: page.after }
              : { kind: 'start' },
          });
          if (!alive || reading !== generation) return;
          page = result;
          sessions = more ? [...sessions, ...result.sessions] : result.sessions;
          const unique = [...new Map(sessions.map((session) => [session.id, session])).values()];
          entries.set(
            unique
              .filter((session) => !session.archived)
              .sort((a, b) => b.updatedAt - a.updatedAt)
              .map((session) => ({
                id: session.id,
                title: session.title || 'Untitled conversation',
                detail: `${session.workspace.path} · ${session.id}`,
                hint: session.id === props.sessionId ? 'current' : session.activity,
                run: () => {
                  void props.selectSession(session.id);
                },
              })),
          );
          note.set('Type to search conversations');
          more = true;
        } while (
          page.after &&
          matches().length < (query() === search ? minimum : count()) &&
          alive &&
          reading === generation
        );
      } else if (props.kind === 'models') {
        do {
          const result = await host.query(
            'settings',
            'models',
            more
              ? { kind: 'continue', revision: page.revision, cursor: page.next }
              : { kind: 'start', query: search },
          );
          if (!alive || reading !== generation) return;
          if (result.kind !== 'ready')
            throw new Error('The model catalog changed. Edit the search to refresh.');
          page = result;
          choices = more ? [...choices, ...result.choices] : result.choices;
          entries.set(
            choices.map((choice) => ({
              id: `${choice.selection.connectionId}/${choice.selection.modelId}`,
              title: choice.label,
              detail: `${choice.connectionName} · ${choice.selection.modelId}`,
              run: () => {
                if (props.sessionId) void props.chooseModel(props.sessionId, choice);
              },
            })),
          );
          note.set(
            page.next
              ? 'Type to search models'
              : `${choices.length} model${choices.length === 1 ? '' : 's'}`,
          );
          more = true;
        } while (page.next && choices.length < minimum && alive && reading === generation);
      } else if (props.kind === 'workspaces') {
        const result = await host.query('agent', 'directories', { kind: 'roots' });
        if (!alive || reading !== generation) return;
        entries.set(
          result.roots.map((directory: any) => ({
            id: directory.path,
            title: directory.name,
            detail: directory.path,
            run: () => {
              void props.create({ kind: 'directory', selection: directory.selection });
            },
          })),
        );
        note.set(
          result.roots.length
            ? 'Choose where the new conversation can work'
            : 'No workspaces are published by this Host.',
        );
      }
    } catch (reason) {
      if (alive && reading === generation)
        error.set(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (alive && reading === generation) loading.set(false);
    }
  };
  const loadMore = () => {
    if (!alive || loading()) return;
    if ((props.kind === 'sessions' && page?.after) || (props.kind === 'models' && page?.next))
      void load(query(), true);
  };
  if (props.kind === 'models')
    effect(() => {
      void load(query());
    });
  else if (props.kind !== 'commands') void load('');
  if (props.kind === 'sessions') {
    effect(() => {
      query();
      count();
      // A local search can filter out an entire page, leaving no native row
      // from which onEndReached could fire. Fill the visible results instead.
      void Promise.resolve().then(() => {
        if (alive && matches().length < count()) loadMore();
      });
    });
  }
  const label = () =>
    ({ commands: 'Commands', sessions: 'Sessions', models: 'Models', workspaces: 'Workspaces' })[
      props.kind
    ];
  return (
    <Dialog
      animation="none"
      onOpenChange={props.close}
      style={{
        flex: 1,
        justifyContent: 'center',
        alignItems: 'center',
        backgroundColor: '#00000099',
        padding: 16,
      }}
    >
      <View
        style={{
          width: '100%',
          maxWidth: 640,
          backgroundColor: C.surface,
          padding: 16,
          gap: 16,
          borderWidth: 8,
          borderColor: C.line,
        }}
      >
        <View style={{ flexDirection: 'row', justifyContent: 'space-between' }}>
          <Text style={{ color: C.accent, fontWeight: '700' }}>{label()}</Text>
          <Button onPress={props.close}>
            <Text style={{ color: C.muted }}>Close · Esc</Text>
          </Button>
        </View>
        <Input
          testID="palette-search"
          autoFocus
          value={query()}
          onChange={(text) => {
            selected.set(0);
            query.set(text);
            list?.scrollToIndex({ index: 0, animated: false });
          }}
          placeholder={`Search ${props.kind}…`}
          style={{
            placeholderColor: C.muted,
            color: C.text,
            height: 32,
            backgroundColor: C.background,
            paddingHorizontal: 8,
          }}
        />
        <List
          each={ids()}
          ref={(node) => {
            list = node;
          }}
          estimatedItemSize={48}
          onEndReached={loadMore}
          style={{ height: count() * 48 }}
        >
          {(id) => (
            <Each of={[id()]} key={(key: string) => key}>
              {(key) => {
                const initial = byId().get(key());
                if (!initial) return <View />;
                const entry = () => byId().get(key()) ?? initial;
                return (
                  <Button
                    selected={matches()[index()]?.id === entry().id}
                    onPress={() => {
                      const action = entry();
                      if (!action.keepOpen) props.close();
                      action.run();
                    }}
                    style={{
                      height: 48,
                      paddingHorizontal: 8,
                      backgroundColor: matches()[index()]?.id === entry().id ? C.line : C.surface,
                    }}
                  >
                    <View style={{ flexDirection: 'row', justifyContent: 'space-between', gap: 8 }}>
                      <Text
                        numberOfLines={1}
                        style={{
                          color: matches()[index()]?.id === entry().id ? C.accent : C.text,
                          flex: 1,
                        }}
                      >
                        {matches()[index()]?.id === entry().id ? '› ' : '  '}
                        {entry().title}
                      </Text>
                      <Text style={{ color: C.muted }}>{entry().hint ?? ''}</Text>
                    </View>
                    <If when={entry().detail}>
                      <Text numberOfLines={1} style={{ color: C.muted }}>
                        {'  '}
                        {entry().detail}
                      </Text>
                    </If>
                  </Button>
                );
              }}
            </Each>
          )}
        </List>
        <If when={loading()}>
          <Text style={{ color: C.muted }}>Loading…</Text>
        </If>
        <If when={!loading() && !matches().length}>
          <Text style={{ color: C.muted }}>No matches.</Text>
        </If>
        <If when={error()}>
          <Text style={{ color: C.danger }}>{error()}</Text>
        </If>
        <Text style={{ color: C.muted }}>{note() || 'Type to filter · ↑↓ to navigate'}</Text>
        <Text style={{ color: C.muted }}>Enter selects · Esc closes</Text>
      </View>
    </Dialog>
  );
}
