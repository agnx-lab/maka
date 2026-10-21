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
import { keymap } from 'nsh/keymap';
import { windowSize } from 'nsh/device';
import { Button, Dialog } from 'nsh/base';
import {
  C,
  compact,
  controlStyle,
  controlHoverStyle,
  controlFocusStyle,
  controlDisabledStyle,
} from './ui';

export type PaletteEntry = {
  id: string;
  title: string;
  detail?: string;
  hint?: string;
  run(): void;
};

export function Palette(props: {
  instance: number;
  commands: PaletteEntry[];
  close(instance: number): boolean;
}) {
  const query = signal('');
  const selected = signal(0);
  let removeSearchKeys = () => {};
  cleanup(() => removeSearchKeys());
  const matches = computed(() => {
    const needle = query().toLowerCase().trim();
    return props.commands.filter((entry) =>
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
          borderWidth: 1,
          borderRadius: 16,
          borderColor: C.line,
        }}
      >
        <View style={{ flexDirection: 'row', justifyContent: 'space-between' }}>
          <Text style={{ color: C.text, fontSize: 18, lineHeight: 24, fontWeight: '600' }}>
            Commands
          </Text>
          <Button
            onPress={inputAction(props.close, props.instance)}
            style={controlStyle}
            hoverStyle={controlHoverStyle}
            focusStyle={controlFocusStyle}
            disabledStyle={controlDisabledStyle}
          >
            <Text style={{ color: C.muted }}>Close</Text>
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
          accessibilityLabel="Search commands"
          autoFocus
          value={query()}
          onChange={(text) => {
            selected.set(0);
            query.set(text);
            list?.scrollToIndex({ index: 0, animated: false });
          }}
          placeholder="Search commands…"
          style={{
            placeholderColor: C.muted,
            color: C.text,
            height: compact() ? 32 : 40,
            backgroundColor: C.background,
            borderRadius: 8,
            paddingHorizontal: 12,
            fontSize: 14,
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
                    hoverStyle={controlHoverStyle}
                    focusStyle={controlFocusStyle}
                    disabledStyle={controlDisabledStyle}
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
        <If when={!matches().length}>
          <Text style={{ color: C.muted }}>No matches.</Text>
        </If>
        <Text style={{ color: C.muted }}>Type to filter · ↑↓ to navigate</Text>
      </View>
    </Dialog>
  );
}
