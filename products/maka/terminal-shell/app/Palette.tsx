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

import { cleanup, computed, Input, inputAction, signal, Text, View } from 'nsh';
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
};
export type PaletteKind = 'commands' | 'workspaces';

export function Palette(props: {
  kind: PaletteKind;
  instance: number;
  commands: PaletteEntry[];
  close(instance: number): boolean;
  create(workspace: any): Promise<void>;
}) {
  const query = signal('');
  const selected = signal(0);
  const entries = signal<PaletteEntry[]>([]);
  const loading = signal(false);
  const error = signal('');
  const note = signal('');
  let alive = true;
  let generation = 0;
  let removeSearchKeys = () => {};
  const create = ({ instance, selection }: { instance: number; selection: any }) => {
    if (props.close(instance)) void props.create({ kind: 'directory', selection });
  };
  cleanup(() => {
    alive = false;
    generation += 1;
    removeSearchKeys();
  });
  const matches = computed(() => {
    const needle = query().toLowerCase().trim();
    const available = props.kind === 'commands' ? props.commands : entries();
    return available.filter((entry) =>
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
  };
  const activate = () => {
    const entry = matches()[index()];
    if (!entry) return;
    entry.run();
  };
  const load = async () => {
    const reading = ++generation;
    loading.set(true);
    error.set('');
    try {
      const result = await host.query('agent', 'directories', { kind: 'roots' });
      if (!alive || reading !== generation) return;
      entries.set(
        result.roots.map((directory: any) => ({
          id: directory.path,
          title: directory.name,
          detail: directory.path,
          run: inputAction(create, { instance: props.instance, selection: directory.selection }),
        })),
      );
      note.set(
        result.roots.length
          ? 'Choose where the new conversation can work'
          : 'No workspaces are published by this Host.',
      );
    } catch (reason) {
      if (alive && reading === generation)
        error.set(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (alive && reading === generation) loading.set(false);
    }
  };
  if (props.kind === 'workspaces') void load();
  const label = () => ({ commands: 'Commands', workspaces: 'Workspaces' })[props.kind];
  return (
    <Dialog
      animation="none"
      onOpenChange={() => {
        props.close(props.instance);
      }}
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
          <Button onPress={inputAction(props.close, props.instance)}>
            <Text style={{ color: C.muted }}>Close · Esc</Text>
          </Button>
        </View>
        <Input
          ref={(node) => {
            removeSearchKeys();
            const unbind = [
              keymap.bind('up', () => move(-1), { when: 'always', repeat: true, within: node }),
              keymap.bind('down', () => move(1), { when: 'always', repeat: true, within: node }),
              keymap.bind('enter', activate, { when: 'always', within: node }),
            ];
            removeSearchKeys = () => unbind.forEach((remove) => remove());
          }}
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
                    onPress={entry().run}
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
