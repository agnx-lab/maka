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

import { Canvas, computed, effect, inputAction, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { Button } from 'nsh/base';
import { List } from 'nsh/list';
import { platform } from 'nsh/device';
import type { Position } from './history';
import { C, controlStyle, captionTextStyle } from './ui';
import { Icon } from './Icon';

function TurnMark(props: { active: boolean; busy: boolean }) {
  const color = () => (props.active ? C.accent : C.muted);
  if (platform.isTerminal)
    return (
      <Text
        pointerEvents="none"
        accessibilityElementsHidden
        style={{ width: 16, height: 16, lineHeight: 16, textAlign: 'center', color: color() }}
      >
        {props.busy ? '◌' : props.active ? '●' : '━'}
      </Text>
    );
  return (
    <Canvas
      pointerEvents="none"
      accessibilityElementsHidden
      style={{ width: 16, height: 16 }}
      draw={(cx) => {
        cx.strokeStyle = color();
        cx.lineWidth = props.active ? 3 : 2;
        cx.lineCap = 'round';
        cx.beginPath();
        if (props.busy) cx.arc(8, 8, 4, 0, Math.PI * 2);
        else {
          cx.moveTo(props.active ? 2 : 4, 8);
          cx.lineTo(props.active ? 14 : 12, 8);
        }
        cx.stroke();
      }}
    />
  );
}

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
  const activeIndex = computed(() => {
    const active = props.active();
    return props.items().findIndex((item) => item.turnId === active);
  });
  let rail: YNode | undefined;
  effect(() => {
    const active = props.active();
    const index = activeIndex();
    if (active !== null && index >= 0) rail?.scrollToIndex({ index, viewPosition: 0.5 });
  });
  return (
    <View style={{ width: 24, minHeight: 0 }}>
      <List
        ref={rail}
        testID="turn-navigation"
        accessibilityLabel="Turn navigation"
        each={props.items()}
        keyExtractor={(item) => item.turnId}
        estimatedItemSize={24}
        style={{ flex: 1, minHeight: 0 }}
      >
        {(item) => (
          <Button
            accessibilityLabel={`Jump to turn: ${item().label || 'Recorded turn'}`}
            tooltip={item().label || 'Recorded turn'}
            selected={props.active() === item().turnId}
            onPress={inputAction(props.navigate, item())}
            style={{ height: 24, width: 24, alignItems: 'center', justifyContent: 'center' }}
            hoverStyle={{ backgroundColor: C.surface }}
            focusStyle={{ backgroundColor: C.raised }}
          >
            <TurnMark
              active={props.active() === item().turnId}
              busy={props.busy() === item().turnId}
            />
          </Button>
        )}
      </List>
      <If when={props.busy()}>
        <Button
          accessibilityLabel="Cancel turn navigation"
          tooltip="Cancel navigation"
          onPress={props.cancel}
          style={{
            position: 'absolute',
            right: 0,
            bottom: 0,
            width: 24,
            height: 32,
            alignItems: 'center',
            justifyContent: 'center',
            backgroundColor: C.raised,
          }}
        >
          <Icon name="close" color={C.secondary} />
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
          ...controlStyle,
          position: 'absolute',
          right: 32,
          bottom: 8,
          flexDirection: 'row',
          alignItems: 'center',
          gap: 8,
          backgroundColor: C.raised,
        }}
        hoverStyle={{ backgroundColor: C.surface }}
        focusStyle={{ backgroundColor: C.accent }}
      >
        <Icon name="latest" color={C.secondary} />
        <Text style={{ ...captionTextStyle, color: C.text }}>Latest</Text>
      </Button>
    </If>
  );
}
