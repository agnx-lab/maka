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

import { cleanup, effect, inputAction, signal, Text, untracked, View } from 'nsh';
import { Button } from 'nsh/base';
import { List } from 'nsh/list';
import { host } from 'maka/host';
import type { GitStatus, GitStatusEntry } from '../../../../packages/plugin-sdk/src/index.js';
import { gitAreaLabel, type CurrentDiffTarget } from './CurrentDiff';
import { C } from './ui';

type ChangesState =
  | { kind: 'loading' }
  | { kind: 'ready'; status: GitStatus | null }
  | { kind: 'error'; message: string };

export function WorkspaceChanges(props: {
  sessionId: string;
  openDiff(target: CurrentDiffTarget): void;
}) {
  const state = signal<ChangesState>({ kind: 'loading' });
  let generation = 0;
  let alive = true;
  cleanup(() => {
    alive = false;
    generation += 1;
  });
  const refresh = async (sessionId: string) => {
    if (!alive || sessionId !== props.sessionId) return;
    const reading = ++generation;
    state.set({ kind: 'loading' });
    try {
      const status: GitStatus | null = await host.query('agent', 'gitStatus', { sessionId });
      if (alive && generation === reading && props.sessionId === sessionId)
        state.set({ kind: 'ready', status });
    } catch (reason) {
      if (alive && generation === reading && props.sessionId === sessionId)
        state.set({ kind: 'error', message: String(reason) });
    }
  };
  effect(() => {
    const sessionId = props.sessionId;
    untracked(() => void refresh(sessionId));
  });
  const status = () => {
    const current = state();
    return current.kind === 'ready' ? current.status : null;
  };
  const entries = () => status()?.entries ?? [];
  const open = (target: CurrentDiffTarget) => {
    if (
      alive &&
      target.sessionId === props.sessionId &&
      entries().some(
        (entry) => entry.area === target.area && entry.path.bytesBase64 === target.path.bytesBase64,
      )
    )
      props.openDiff(target);
  };
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 16 }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <Text style={{ flex: 1, color: C.text, fontWeight: '700' }}>Current changes</Text>
        <Button
          accessibilityLabel="Refresh current changes"
          onPress={inputAction(refresh, props.sessionId)}
          hoverStyle={{ backgroundColor: C.raised }}
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.accent }}>{state().kind === 'error' ? 'Retry' : 'Refresh'}</Text>
        </Button>
      </View>
      <If when={state().kind === 'loading'}>
        <Text style={{ color: C.muted }}>Reading current changes…</Text>
      </If>
      <If when={state().kind === 'error'}>
        <Text selectable style={{ color: C.danger }}>
          {(state() as Extract<ChangesState, { kind: 'error' }>).message}
        </Text>
      </If>
      <If when={state().kind === 'ready'}>
        <If
          when={status() !== null}
          else={<Text style={{ color: C.muted }}>Not a Git workspace.</Text>}
        >
          <Text numberOfLines={1} style={{ color: C.muted }}>
            {status()?.head ?? 'No commits yet'} · {entries().length} changes
          </Text>
          <List
            accessibilityLabel="Current workspace changes"
            each={entries()}
            keyExtractor={(entry: GitStatusEntry) => `${entry.area}/${entry.path.bytesBase64}`}
            estimatedItemSize={32}
            style={{ flex: 1, minHeight: 0 }}
            empty={<Text style={{ color: C.muted }}>No current changes.</Text>}
          >
            {(entry) => (
              <Button
                accessibilityLabel={`Open ${gitAreaLabel[entry().area]} diff: ${host.pathLabel(entry().path)}`}
                onPress={inputAction(open, {
                  sessionId: props.sessionId,
                  area: entry().area,
                  path: entry().path,
                  title: host.pathLabel(entry().path),
                })}
                style={{ paddingHorizontal: 8, minHeight: 32 }}
                hoverStyle={{ backgroundColor: C.raised }}
                focusStyle={{ backgroundColor: C.line }}
              >
                <Text numberOfLines={1} style={{ color: C.text }}>
                  {host.pathLabel(entry().path)}
                </Text>
                <Text numberOfLines={1} style={{ color: C.muted }}>
                  {gitAreaLabel[entry().area]} · {entry().kind.replace('_', ' ')}
                </Text>
              </Button>
            )}
          </List>
        </If>
      </If>
    </View>
  );
}
