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
import { Button } from 'nsh/base';
import { windowSize } from 'nsh/device';

export const C = {
  background: '#151517',
  surface: '#232324',
  raised: '#2c2c2e',
  text: '#f9fafb',
  secondary: '#cfd3d6',
  muted: '#adb2b8',
  accent: '#7aaaff',
  accentHover: '#b7c8fe',
  warning: '#e7b86b',
  danger: '#f48a8a',
  line: '#353638',
};
export function Header(props: {
  title: string;
  workspace?: string;
  status?: string;
  active?: boolean;
  commands(): void;
  browse?(): void;
}) {
  const width = signal(windowSize().width);
  return (
    <View
      onLayout={(frame) => width.set(frame.width)}
      style={{
        flexDirection: 'row',
        alignItems: 'center',
        paddingHorizontal: 16,
        paddingVertical: 0,
        gap: 16,
        borderBottomWidth: 0,
        borderBottomColor: C.line,
      }}
    >
      <Button
        onPress={props.commands}
        accessibilityLabel="Open commands"
        hoverStyle={{ backgroundColor: C.surface }}
        focusStyle={{ backgroundColor: C.raised }}
        activeStyle={{ backgroundColor: C.line }}
      >
        <Text style={{ color: C.accent, fontWeight: '700' }}>MAKA</Text>
      </Button>
      <If when={props.browse}>
        <Button
          onPress={props.browse}
          accessibilityLabel="Browse sessions"
          hoverStyle={{ backgroundColor: C.surface }}
          focusStyle={{ backgroundColor: C.raised }}
        >
          <Text style={{ color: C.accent }}>☰</Text>
        </Button>
      </If>
      <Text numberOfLines={1} style={{ color: C.text, flex: 1 }}>
        {props.title}
      </Text>
      <If when={width() >= 880 && props.workspace}>
        <Text numberOfLines={1} style={{ color: C.muted, maxWidth: 240 }}>
          {props.workspace?.split('/').filter(Boolean).at(-1)}
        </Text>
      </If>
      <Text style={{ color: props.active ? C.accent : C.muted }}>
        {props.active ? '● ' : ''}
        {props.status || 'Ctrl+P'}
      </Text>
    </View>
  );
}
