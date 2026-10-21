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

import { signal, Text, View } from 'nsh';
import type { YNode } from 'nsh';
import { Button } from 'nsh/base';
import { platform, windowSize } from 'nsh/device';
import { Icon } from './Icon';

export const C = {
  background: '#161719',
  surface: '#202123',
  raised: '#2a2b2e',
  text: '#f0f1f3',
  secondary: '#c6c9ce',
  muted: '#969ba4',
  accent: '#8ab8f8',
  accentHover: '#b6d3ff',
  warning: '#e7b86b',
  danger: '#f48a8a',
  line: '#37393e',
};

// Cells have a fixed 16-point row; native pixels retain the prose leading.
export const bodyTextStyle = { fontSize: 14, lineHeight: platform.isTerminal ? 16 : 24 };
export const inputTextStyle = { fontSize: 14, lineHeight: platform.isTerminal ? 16 : 20 };
export const codeTextStyle = {
  fontFamily: 'monospace',
  fontSize: 13,
  lineHeight: platform.isTerminal ? 16 : 20,
};
export const compact = () => windowSize().height < 480;
export const controlStyle = {
  minHeight: 32,
  paddingHorizontal: 12,
  paddingVertical: 8,
  borderRadius: 8,
  justifyContent: 'center',
} as const;
export const controlHoverStyle = { backgroundColor: C.raised };
export const controlFocusStyle = { backgroundColor: C.raised, borderColor: C.accent };
export const controlDisabledStyle = { opacity: 0.4 };
export function Header(props: {
  title: string;
  workspace?: string;
  status?: string;
  active?: boolean;
  commands(): void;
  browse?(): void;
  files?(): void;
  filesOpener?(node: YNode): void;
}) {
  const width = signal(windowSize().width);
  return (
    <View
      onLayout={(frame) => width.set(frame.size.width)}
      style={{
        flexDirection: 'row',
        alignItems: 'center',
        paddingHorizontal: 16,
        height: compact() ? 32 : 48,
        gap: 8,
        borderBottomWidth: 1,
        borderBottomColor: C.line,
      }}
    >
      <Button
        onPress={props.commands}
        accessibilityLabel="Open commands"
        style={controlStyle}
        hoverStyle={{ backgroundColor: C.surface }}
        focusStyle={{ backgroundColor: C.raised }}
        activeStyle={{ backgroundColor: C.line }}
      >
        <View pointerEvents="none" style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
          <Text style={{ color: C.secondary, fontSize: 13 }}>Commands</Text>
          <Text style={{ color: C.muted, fontSize: 11 }}>Ctrl+P</Text>
        </View>
      </Button>
      <If when={props.browse}>
        <Button
          onPress={props.browse}
          accessibilityLabel="Browse sessions"
          style={{ ...controlStyle, width: 32, paddingHorizontal: 8, alignItems: 'center' }}
          hoverStyle={{ backgroundColor: C.surface }}
          focusStyle={{ backgroundColor: C.raised }}
        >
          <Icon name="sidebar" color={C.secondary} />
        </Button>
      </If>
      <Text numberOfLines={1} style={{ color: C.text, flex: 1, fontWeight: '500' }}>
        {props.title}
      </Text>
      <If when={props.files}>
        <Button
          accessibilityLabel="Browse workspace files"
          ref={props.filesOpener}
          onPress={props.files}
          style={controlStyle}
          hoverStyle={{ backgroundColor: C.surface }}
          focusStyle={{ backgroundColor: C.raised }}
        >
          <Text style={{ color: C.secondary }}>Files</Text>
        </Button>
      </If>
      <If when={width() >= 880 && props.workspace}>
        <Text numberOfLines={1} style={{ color: C.muted, maxWidth: 240 }}>
          {props.workspace?.split('/').filter(Boolean).at(-1)}
        </Text>
      </If>
      <Text style={{ color: props.active ? C.accent : C.muted, fontSize: 12 }}>
        {props.active ? '● ' : ''}
        {props.status || ''}
      </Text>
    </View>
  );
}
