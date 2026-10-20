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

import { batch, cleanup, inputAction, signal, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import { List } from 'nsh/list';
import { windowSize } from 'nsh/device';
import { host } from 'maka/host';
import type { Subscription } from './history';
import { C } from './ui';

type ChecklistItem = {
  content: string;
  status: 'pending' | 'in_progress' | 'completed';
};
type ChecklistPage = {
  enabled: boolean;
  revision: number | null;
  total: number;
  completed: number;
  current: ChecklistItem | null;
  offset: number;
  items: ChecklistItem[];
  nextOffset: number | null;
};

export function Checklist(props: { sessionId: string }) {
  const sessionId = props.sessionId;
  const snapshot = signal<ChecklistPage | null>(null);
  const items = signal<ChecklistItem[]>([]);
  const nextOffset = signal<number | null>(null);
  const expanded = signal(false);
  const loading = signal(false);
  const error = signal('');
  let alive = true;
  let generation = 0;
  let stream: Subscription | null = null;
  const marker = (item: ChecklistItem) =>
    item.status === 'completed' ? '✓' : item.status === 'in_progress' ? '›' : '·';
  const setExpanded = (action: { sessionId: string; open: boolean }) => {
    const page = snapshot();
    if (!alive || action.sessionId !== sessionId || !page?.enabled || page.total === 0) return;
    expanded.set(action.open);
  };
  const loadMore = async () => {
    const page = snapshot();
    const offset = nextOffset();
    if (!alive || loading() || !page || offset === null) return;
    const currentGeneration = generation;
    loading.set(true);
    error.set('');
    try {
      const next: ChecklistPage = await host.query('agent', 'checklist', { sessionId, offset });
      // A replacement invalidates every previously loaded page. Never stitch
      // pages from two checklist revisions; the watch supplies the new head.
      if (!alive || currentGeneration !== generation || next.revision !== page.revision) return;
      batch(() => {
        items.set([...items(), ...next.items]);
        nextOffset.set(next.nextOffset);
      });
    } catch (reason) {
      if (alive && currentGeneration === generation) error.set(String(reason));
    } finally {
      if (alive && currentGeneration === generation) loading.set(false);
    }
  };
  cleanup(() => {
    alive = false;
    const previous = stream;
    stream = null;
    if (previous) void previous.close().catch(() => {});
  });
  void (async () => {
    const opened = await host.subscribe('agent', 'watchChecklist', { sessionId, offset: 0 });
    if (!alive) {
      await opened.close();
      return;
    }
    stream = opened;
    try {
      for (;;) {
        const page: ChecklistPage | null = await opened.next();
        if (!alive) return;
        if (page === null) throw new Error('Checklist updates ended. Reopen the conversation.');
        if (snapshot() && page.revision === snapshot()!.revision) continue;
        generation += 1;
        batch(() => {
          snapshot.set(page);
          items.set(page.items);
          nextOffset.set(page.nextOffset);
          loading.set(false);
          error.set('');
        });
      }
    } finally {
      if (stream === opened) {
        stream = null;
        await opened.close();
      }
    }
  })().catch((reason) => {
    if (alive) error.set(String(reason));
  });
  return (
    <If when={snapshot()?.enabled && snapshot()!.total > 0}>
      <View style={{ backgroundColor: C.surface, borderRadius: 8 }}>
        <Button
          selected={expanded()}
          onPress={inputAction(setExpanded, { sessionId, open: !expanded() })}
          style={{ height: 16, paddingHorizontal: 16, flexDirection: 'row', gap: 8 }}
          hoverStyle={{ backgroundColor: C.raised }}
          focusStyle={{ backgroundColor: C.line }}
          activeStyle={{ backgroundColor: C.raised }}
        >
          <Text style={{ color: C.text }}>
            {expanded() ? '▾' : '▸'} Todo {snapshot()!.completed}/{snapshot()!.total}
          </Text>
          <Text numberOfLines={1} style={{ flex: 1, color: C.muted }}>
            {snapshot()!.current?.content ??
              (snapshot()!.completed === snapshot()!.total
                ? 'All items complete'
                : 'No active item')}
          </Text>
        </Button>
        <If when={expanded()}>
          <List
            each={items()}
            onEndReached={() => void loadMore()}
            estimatedItemSize={16}
            keyExtractor={(_, index) => String(index)}
            style={{
              height:
                Math.min(
                  items().length,
                  Math.max(2, Math.min(6, Math.floor(windowSize().height / 48))),
                ) * 16,
            }}
          >
            {(item) => (
              <Text
                numberOfLines={1}
                style={{
                  height: 16,
                  paddingHorizontal: 16,
                  color: item().status === 'in_progress' ? C.accent : C.secondary,
                }}
              >
                {marker(item())} {item().content}
              </Text>
            )}
          </List>
          <If when={error()}>
            <Text numberOfLines={1} style={{ paddingHorizontal: 16, color: C.danger }}>
              {error()}
            </Text>
          </If>
        </If>
      </View>
    </If>
  );
}
