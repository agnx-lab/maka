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

import { batch, cleanup, computed, effect, Input, inputAction, signal, Text, View } from 'nsh';
import type { Signal, YNode } from 'nsh';
import { Button, Dialog } from 'nsh/base';
import { List } from 'nsh/list';
import { keymap } from 'nsh/keymap';
import { windowSize } from 'nsh/device';
import { host } from 'maka/host';
import type {
  AgentDirectory,
  AgentDirectoryPage,
  AgentDirectorySelection,
  AgentWorkspaceSelection,
} from '../../../../packages/plugin-sdk/src/index.js';
import { C } from './ui';

type Location = {
  id: number;
  trail: AgentDirectory[];
  page: Signal<AgentDirectoryPage | null>;
  entries: Signal<AgentDirectory[]>;
  reading: boolean;
};
type FolderAction = { location: number; selection: AgentDirectorySelection };
const selectionKey = (selection: AgentDirectorySelection) =>
  JSON.stringify([selection.rootId, selection.segments, selection.directoryIdentity]);

export function WorkspacePicker(props: {
  instance: number;
  current(): boolean;
  close(instance: number): boolean;
  create(workspace: AgentWorkspaceSelection): Promise<void>;
}) {
  const query = signal('');
  const selected = signal(0);
  const loading = signal(false);
  const error = signal('');
  let alive = true;
  let serial = 0;
  const newLocation = (trail: AgentDirectory[]): Location => ({
    id: ++serial,
    trail,
    page: signal<AgentDirectoryPage | null>(null),
    entries: signal<AgentDirectory[]>([]),
    reading: false,
  });
  const location = signal(newLocation([]));
  const entries = () => location().entries();
  let list: YNode | undefined;
  let removeKeys = () => {};
  let removeSearchKeys = () => {};
  const owns = (read: Location) => alive && props.current() && location() === read;
  const matches = computed(() => {
    const needle = query().trim().toLowerCase();
    return entries().filter((directory) => directory.name.toLowerCase().includes(needle));
  });
  const count = () => Math.max(1, Math.min(8, Math.floor((windowSize().height - 288) / 32)));
  const index = () => Math.min(selected(), Math.max(0, matches().length - 1));
  const isSelected = (directory: AgentDirectory) => {
    const row = matches()[index()];
    return !!row && selectionKey(row.selection) === selectionKey(directory.selection);
  };
  const currentDirectory = (read = location()) => {
    const page = read.page();
    return page?.kind === 'children' ? page.directory : null;
  };
  const load = async (read = location()) => {
    const previous = read.page();
    if (!owns(read) || read.reading || (previous && (previous.kind === 'roots' || !previous.after)))
      return;
    read.reading = true;
    loading.set(true);
    error.set('');
    try {
      const directory = read.trail.at(-1);
      const after = previous?.kind === 'children' ? previous.after : null;
      const page: AgentDirectoryPage = await host.query(
        'agent',
        'directories',
        directory ? { kind: 'children', selection: directory.selection, after } : { kind: 'roots' },
      );
      if (!owns(read)) return;
      if ((directory && page.kind !== 'children') || (!directory && page.kind !== 'roots'))
        throw new Error('The Host returned a different directory view. Reopen the folder.');
      if (page.kind === 'children' && page.after !== null && page.after === after)
        throw new Error('Folder paging did not advance. Reopen the folder.');
      const rows = new Map(read.entries().map((entry) => [selectionKey(entry.selection), entry]));
      for (const entry of page.kind === 'roots' ? page.roots : page.children)
        rows.set(selectionKey(entry.selection), entry);
      batch(() => {
        read.page.set(page);
        read.entries.set([...rows.values()]);
      });
    } catch (reason) {
      if (owns(read)) error.set(reason instanceof Error ? reason.message : String(reason));
    } finally {
      read.reading = false;
      if (owns(read)) loading.set(false);
    }
  };
  const navigate = (trail: AgentDirectory[]) => {
    if (!alive || !props.current()) return;
    const read = newLocation(trail);
    batch(() => {
      location.set(read);
      query.set('');
      selected.set(0);
      error.set('');
      loading.set(true);
    });
    list = undefined;
    void load(read);
  };
  const browse = (action: FolderAction) => {
    const read = location();
    if (!owns(read) || action.location !== read.id || error()) return;
    const directory = read
      .entries()
      .find((entry) => selectionKey(entry.selection) === selectionKey(action.selection));
    if (directory) navigate([...read.trail, directory]);
  };
  const back = ({ location: original, roots = false }: { location: number; roots?: boolean }) => {
    const read = location();
    if (owns(read) && original === read.id && read.trail.length)
      navigate(roots ? [] : read.trail.slice(0, -1));
  };
  const choose = (action: FolderAction) => {
    const read = location();
    const directory = currentDirectory();
    if (
      !owns(read) ||
      action.location !== read.id ||
      error() ||
      !directory ||
      selectionKey(directory.selection) !== selectionKey(action.selection)
    )
      return;
    if (props.close(props.instance))
      void props.create({ kind: 'directory', selection: action.selection });
  };
  const chooseCurrent = (read: Location) => {
    const directory = currentDirectory(read);
    if (directory) choose({ location: read.id, selection: directory.selection });
  };
  const move = (read: Location, delta: number) => {
    if (!owns(read)) return;
    const next = Math.max(0, Math.min(matches().length - 1, index() + delta));
    selected.set(next);
    list?.scrollToIndex({ index: next, animated: false, viewPosition: delta > 0 ? 1 : 0 });
    if (next >= matches().length - 2) void load();
  };
  const activate = (read: Location) => {
    if (!owns(read)) return;
    const directory = matches()[index()];
    if (directory) browse({ location: read.id, selection: directory.selection });
  };
  effect(() => {
    if (!loading() && !error() && matches().length < count()) void load();
  });
  cleanup(() => {
    alive = false;
    removeKeys();
    removeSearchKeys();
  });
  void load();
  return (
    <Dialog
      animation="none"
      onOpenChange={() => props.close(props.instance)}
      style={{
        flex: 1,
        justifyContent: 'center',
        alignItems: 'center',
        backgroundColor: '#00000099',
        padding: 16,
      }}
    >
      <Each of={[location().id]} key={(id: number) => id}>
        {() => {
          const read = location();
          return (
            <View
              ref={(node) => {
                removeKeys();
                const unbind = [
                  keymap.bind('alt+up', () => back({ location: read.id }), {
                    when: 'always',
                    within: node,
                  }),
                  keymap.bind('ctrl+enter', () => chooseCurrent(read), {
                    when: 'always',
                    within: node,
                  }),
                ];
                removeKeys = () => unbind.forEach((remove) => remove());
              }}
              style={{
                width: '100%',
                maxWidth: 640,
                maxHeight: '100%',
                backgroundColor: C.surface,
                padding: 16,
                gap: 8,
                borderWidth: 8,
                borderColor: C.line,
              }}
            >
              <View style={{ flexDirection: 'row', justifyContent: 'space-between' }}>
                <Text style={{ color: C.accent, fontWeight: '700' }}>Choose workspace</Text>
                <Button
                  accessibilityLabel="Close workspace picker"
                  onPress={inputAction(props.close, props.instance)}
                >
                  <Text style={{ color: C.muted }}>Close · Esc</Text>
                </Button>
              </View>
              <Text numberOfLines={1} style={{ color: C.text }}>
                {read.trail.length ? `Folder · ${read.trail.at(-1)!.name}` : 'Workspace locations'}
              </Text>
              <Text numberOfLines={1} style={{ color: C.muted }}>
                {currentDirectory(read)?.path ??
                  read.trail.at(-1)?.path ??
                  'Open a location, then choose the folder where the conversation can work.'}
              </Text>
              <View style={{ flexDirection: 'row', gap: 8 }}>
                <Button
                  disabled={!read.trail.length}
                  accessibilityLabel="Parent workspace folder"
                  onPress={inputAction(back, { location: read.id })}
                >
                  <Text style={{ color: C.accent }}>↑ Parent</Text>
                </Button>
                <Button
                  disabled={!read.trail.length}
                  accessibilityLabel="Workspace locations"
                  onPress={inputAction(back, { location: read.id, roots: true })}
                >
                  <Text style={{ color: C.accent }}>Locations</Text>
                </Button>
                <Button
                  disabled={!currentDirectory(read) || !!error()}
                  accessibilityLabel="Use this workspace folder"
                  onPress={
                    currentDirectory(read)
                      ? inputAction(choose, {
                          location: read.id,
                          selection: currentDirectory(read)!.selection,
                        })
                      : undefined
                  }
                >
                  <Text style={{ color: currentDirectory(read) && !error() ? C.accent : C.muted }}>
                    Use this folder
                  </Text>
                </Button>
              </View>
              <Input
                ref={(node) => {
                  removeSearchKeys();
                  const unbind = [
                    keymap.bind('up', () => move(read, -1), {
                      when: 'always',
                      repeat: true,
                      within: node,
                    }),
                    keymap.bind('down', () => move(read, 1), {
                      when: 'always',
                      repeat: true,
                      within: node,
                    }),
                    keymap.bind('enter', () => activate(read), { when: 'always', within: node }),
                  ];
                  removeSearchKeys = () => unbind.forEach((remove) => remove());
                }}
                autoFocus
                accessibilityLabel="Search workspace folders"
                value={query()}
                onChange={(text) => {
                  if (!owns(read)) return;
                  selected.set(0);
                  query.set(text);
                  list?.scrollToIndex({ index: 0, animated: false });
                }}
                placeholder="Search workspaces…"
                style={{
                  placeholderColor: C.muted,
                  color: C.text,
                  height: 32,
                  backgroundColor: C.background,
                  paddingHorizontal: 8,
                }}
              />
              <List
                each={matches()}
                keyExtractor={(entry) => selectionKey(entry.selection)}
                ref={(node) => {
                  list = node;
                }}
                estimatedItemSize={32}
                onEndReached={() => {
                  void load(read);
                }}
                accessibilityLabel="Workspace folders"
                style={{ height: count() * 32, flexShrink: 1, minHeight: 32 }}
              >
                {(entry) => (
                  <Button
                    disabled={!!error()}
                    accessibilityLabel={`Browse folder ${entry().name}`}
                    selected={isSelected(entry())}
                    onPress={inputAction(browse, {
                      location: read.id,
                      selection: entry().selection,
                    })}
                    style={{
                      height: 32,
                      paddingHorizontal: 8,
                      backgroundColor: isSelected(entry()) ? C.line : C.surface,
                    }}
                    hoverStyle={{ backgroundColor: C.raised }}
                    focusStyle={{ backgroundColor: C.raised }}
                  >
                    <Text numberOfLines={1} style={{ color: C.text }}>
                      {isSelected(entry()) ? '› ' : '  '}
                      {entry().name} /
                    </Text>
                  </Button>
                )}
              </List>
              <If when={loading()}>
                <Text style={{ color: C.muted }}>Loading folders…</Text>
              </If>
              <If when={!loading() && !error() && !matches().length}>
                <Text style={{ color: C.muted }}>
                  {query()
                    ? 'No matching folders.'
                    : read.trail.length
                      ? 'This folder has no subfolders.'
                      : 'No workspace locations are published by this Host.'}
                </Text>
              </If>
              <If when={error()}>
                <Text numberOfLines={2} style={{ color: C.danger }}>
                  {error()}
                </Text>
                <Button
                  onPress={inputAction(
                    ({ location: original }: { location: number }) => {
                      if (owns(location()) && original === location().id)
                        navigate(location().trail);
                    },
                    { location: read.id },
                  )}
                >
                  <Text style={{ color: C.accent }}>Retry folder</Text>
                </Button>
              </If>
              <Text style={{ color: C.muted }}>
                Enter opens · Ctrl+Enter uses folder · Alt+↑ parent · Esc closes
              </Text>
            </View>
          );
        }}
      </Each>
    </Dialog>
  );
}
