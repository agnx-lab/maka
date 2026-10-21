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

import { effect, inputAction, signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { Button } from 'nsh/base';
import { List } from 'nsh/list';
import type { Position } from './history';
import { C } from './ui';

export type TurnNavigationItem = {
  turnId: string;
  label: string;
  anchor:
    | { kind: 'loaded'; position: Position }
    | { kind: 'landmark'; through: number; position: Position; lastPosition: Position };
};

/** Sampled Host landmarks and the loaded window share this small native rail. */
export function TurnNavigation(props: {
  items(): readonly TurnNavigationItem[];
  active(): string | null;
  busy(): string | null;
  navigate(item: TurnNavigationItem): void;
  cancel(): void;
}) {
  const hoveredTurn = signal<string | null>(null);
  const pointerInside = signal(false);
  let rail: YNode | undefined;
  effect(() => {
    if (pointerInside()) return;
    const index = props.items().findIndex((item) => item.turnId === props.active());
    if (index >= 0) rail?.scrollToIndex({ index, viewPosition: 0.5 });
  });
  const previewItem = () => props.items().find((item) => item.turnId === hoveredTurn());
  return (
    <View
      style={{ width: 24, minHeight: 0 }}
      onHoverIn={() => pointerInside.set(true)}
      onHoverOut={() => {
        pointerInside.set(false);
        hoveredTurn.set(null);
      }}
    >
      <List
        ref={rail}
        testID="turn-navigation"
        accessibilityLabel="Turn navigation"
        each={props.items()}
        keyExtractor={(item) => item.turnId}
        estimatedItemSize={16}
        style={{ flex: 1, minHeight: 0 }}
      >
        {(item) => (
          <View onHoverIn={() => hoveredTurn.set(item().turnId)}>
            <Button
              accessibilityLabel={`Jump to turn: ${item().label || 'Recorded turn'}`}
              selected={props.active() === item().turnId}
              onPress={inputAction(props.navigate, item())}
              style={{ height: 16, width: 24, alignItems: 'center' }}
              hoverStyle={{ backgroundColor: C.surface }}
              focusStyle={{ backgroundColor: C.raised }}
            >
              <Text
                style={{
                  color: props.active() === item().turnId ? C.accent : C.muted,
                }}
              >
                {props.busy() === item().turnId
                  ? '◌'
                  : props.active() === item().turnId
                    ? '●'
                    : '━'}
              </Text>
            </Button>
          </View>
        )}
      </List>
      <If when={previewItem()}>
        <View
          pointerEvents="none"
          style={{
            position: 'absolute',
            top: 0,
            right: 24,
            width: 240,
            maxWidth: 240,
            padding: 8,
            backgroundColor: C.raised,
          }}
        >
          <Text numberOfLines={3} style={{ color: C.secondary }}>
            {previewItem()?.label || 'Recorded turn'}
          </Text>
        </View>
      </If>
      <If when={props.busy()}>
        <Button
          accessibilityLabel="Cancel turn navigation"
          onPress={props.cancel}
          style={{ position: 'absolute', right: 0, bottom: 0, backgroundColor: C.raised }}
        >
          <Text style={{ color: C.secondary }}>×</Text>
        </Button>
      </If>
    </View>
  );
}

export function ReturnToLatest(props: { visible: boolean; activate(): void }) {
  return (
    <If when={props.visible}>
      <Button
        accessibilityLabel="Return to latest"
        onPress={props.activate}
        style={{
          position: 'absolute',
          right: 32,
          bottom: 8,
          paddingHorizontal: 8,
          backgroundColor: C.raised,
        }}
        hoverStyle={{ backgroundColor: C.surface }}
        focusStyle={{ backgroundColor: C.accent }}
      >
        <Text style={{ color: C.text }}>↓ Latest</Text>
      </Button>
    </If>
  );
}
