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

import { cleanup, computed, effect, signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { Button } from 'nsh/base';
import type { AnimatedValue } from 'nsh/animated';
import { List } from 'nsh/list';
import type { Row } from './history';
import { MessageRow, type MessageViewState } from './MessageRow';
import { C } from './ui';

// These are presentation items over the canonical history, not a second transcript.
export type TurnItem = {
  kind: 'turn';
  key: string;
  turnId: string;
  rows: Row[];
  timing: any;
  active: boolean;
  completed: boolean;
};
export type TranscriptItem = TurnItem | { kind: 'message'; key: string; row: Row };
export type TurnViewState = {
  expanded: ReturnType<typeof signal<{ open: boolean; completed: boolean } | null>>;
  offset: AnimatedValue;
  rows: Map<string, MessageViewState>;
  visited: boolean;
  lease: number;
  list?: YNode;
  requestFocus?: boolean;
};

function ActivityLog(props: {
  turn: () => TurnItem;
  state: TurnViewState;
  sessionId: string;
  through: () => number;
  viewport: () => { width: number; height: number };
}) {
  const state = props.state;
  const stateKey = (row: Row) => (row.tool ? row.key : `${row.messageId}/${row.field}`);
  const rowState = (row: Row) => {
    const key = stateKey(row);
    let value = state.rows.get(key);
    if (!value) {
      value = { expanded: signal(false), sourceMode: signal(false) };
      state.rows.set(key, value);
    }
    return value;
  };
  const rowsByKey = computed(() => new Map(props.turn().rows.map((row) => [row.key, row])));
  const rowKeys = computed(() => props.turn().rows.map((row) => row.key));
  const initial = signal<{ y: number } | null>(null);
  const bodyHeight = () => {
    const columns = Math.max(1, Math.floor(props.viewport().width / 8) - 4);
    const estimated = props
      .turn()
      .rows.reduce(
        (lines, row) =>
          lines +
          (row.tool || row.role === 'thinking'
            ? 1
            : Math.max(2, Math.ceil(row.text.length / columns) + 1)),
        0,
      );
    const available = Math.max(16, props.viewport().height - 80);
    return Math.min(352, available, Math.max(128, estimated * 16));
  };
  const error = signal('');
  const lease = ++state.lease;
  let ownList: YNode | undefined;
  let alive = true;
  cleanup(() => {
    alive = false;
    if (state.list === ownList) state.list = undefined;
  });
  // Read before binding the new Scroll: its first layout would overwrite the gauge.
  // The same object is then retained for this mount, so later renders never reset a user's scroll.
  void state.offset
    .getValue()
    .then((y) => {
      if (alive && lease === state.lease) {
        // Tool source pages are loaded on demand and are not retained across mounts.
        // Their shorter loading view cannot restore a previous source offset.
        const sourceReloads = props
          .turn()
          .rows.some((row) => row.tool && state.rows.get(stateKey(row))?.expanded());
        initial.set({ y: sourceReloads ? 0 : y });
      }
    })
    .catch((reason) => {
      if (alive) error.set(String(reason));
    });
  return (
    <View style={{ height: bodyHeight() }}>
      <If when={initial()}>
        <List
          accessibilityLabel="Turn activity"
          each={rowKeys()}
          estimatedItemSize={16}
          scrollValue={state.offset}
          contentOffset={initial()!}
          maintainVisibleContentPosition={{ autoscrollToBottomThreshold: 16 }}
          contentStyle={{ paddingRight: 8 }}
          ref={(node) => {
            ownList = node;
            state.list = node;
            if (!state.visited) {
              state.visited = true;
              node.scrollToEnd({ animated: false });
            }
            if (state.requestFocus) {
              state.requestFocus = false;
              node.focus();
            }
          }}
          style={{ height: bodyHeight() }}
        >
          {(key) => (
            <Each of={[key()]} key={(id: string) => id}>
              {(ownedKey) => {
                const initial = rowsByKey().get(ownedKey());
                if (!initial) return <View />;
                return (
                  <MessageRow
                    item={() => rowsByKey().get(ownedKey()) ?? initial}
                    viewState={rowState(initial)}
                    sessionId={props.sessionId}
                    through={props.through}
                  />
                );
              }}
            </Each>
          )}
        </List>
      </If>
      <If when={error()}>
        <Text style={{ color: C.danger }}>{error()}</Text>
      </If>
    </View>
  );
}

export function TurnPanel(props: {
  turn: () => TurnItem;
  state: TurnViewState;
  sessionId: string;
  through: () => number;
  viewport: () => { width: number; height: number };
  trajectory: () => boolean;
  reveal: () => void;
}) {
  const active = computed(
    () => props.turn().active && !props.turn().completed && !props.turn().timing?.finished,
  );
  const timing = computed(() => props.turn().timing);
  const startedAt = computed(() => timing()?.startedAt ?? null);
  const outcome = computed(() =>
    props.turn().completed ? 'completed' : (timing()?.finished?.outcome ?? null),
  );
  const clock = signal(Date.now());
  const expanded = () => {
    const chosen = props.state.expanded();
    if (chosen && chosen.completed === props.turn().completed) return chosen.open;
    return props.trajectory() || active() || outcome() !== 'completed';
  };
  effect(() => {
    if (!active() || startedAt() === null) return;
    const timer = setInterval(() => clock.set(Date.now()), 1000);
    cleanup(() => clearInterval(timer));
  });
  const duration = () => {
    if (startedAt() === null) return '';
    const end = timing()?.finished?.at ?? (active() ? clock() : null);
    if (end === null || end < startedAt()) return '';
    const seconds = Math.floor((end - startedAt()) / 1000);
    return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  };
  const summary = () => {
    if (!active())
      return outcome() === 'completed'
        ? 'Completed'
        : outcome() === 'failed'
          ? 'Failed'
          : outcome() === 'aborted'
            ? 'Stopped'
            : 'Activity';
    const latest = props.turn().rows.at(-1);
    if (!latest) return 'Working…';
    if (latest.tool) {
      const call = latest.tool.call;
      return `${latest.tool.result ? 'Working' : 'Running tool'}${call?.name ? ` · ${call.name}` : ''}${call?.title?.fallback ? ` · ${call.title.fallback}` : ''}`;
    }
    const tail = latest.text.slice(-512).trimEnd().split('\n').at(-1)?.trim() ?? '';
    return `${latest.role === 'thinking' ? 'Analyzing the request' : 'Writing response'}${tail ? ` · ${tail.slice(-240)}` : ''}`;
  };
  return (
    <View style={{ paddingVertical: 16, gap: 0 }}>
      <Button
        disabled={props.turn().rows.length === 0}
        selected={expanded()}
        onPress={() => {
          const open = !expanded();
          props.state.expanded.set({ open, completed: props.turn().completed });
          if (open) props.reveal();
        }}
        style={{ height: 16, flexDirection: 'row' }}
      >
        <Text
          numberOfLines={1}
          style={{ flex: 1, color: outcome() === 'failed' ? C.danger : C.muted }}
        >
          {props.turn().rows.length ? (expanded() ? '⌃ ' : '› ') : '✓ '}
          {summary()}
        </Text>
        <If when={!expanded()}>
          <Text style={{ color: C.muted }}>{duration()}</Text>
        </If>
      </Button>
      <If when={expanded()}>
        <ActivityLog
          turn={props.turn}
          state={props.state}
          sessionId={props.sessionId}
          through={props.through}
          viewport={props.viewport}
        />
        <Text
          style={{
            color: active() ? C.accent : C.muted,
            borderTopWidth: 16,
            borderTopColor: C.line,
          }}
        >
          {active() ? 'Working' : outcome() === 'completed' ? 'Worked' : 'Activity'}
          {duration() ? ` for ${duration()}` : ''}
        </Text>
      </If>
    </View>
  );
}
