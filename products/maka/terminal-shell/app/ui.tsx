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

import { Text, View } from 'nsh';
import { windowSize } from 'nsh/device';

export const C = {
  background: '#121212',
  surface: '#262628',
  text: '#d7d7da',
  muted: '#929298',
  accent: '#7b9cf2',
  warning: '#e7b86b',
  danger: '#f48a8a',
  line: '#303033',
};
export function Header(props: {
  title: string;
  workspace?: string;
  status?: string;
  active?: boolean;
  commands(): void;
}) {
  return (
    <View
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
      <View onPress={props.commands}>
        <Text style={{ color: C.accent, fontWeight: '700' }}>MAKA</Text>
      </View>
      <Text numberOfLines={1} style={{ color: C.text, flex: 1 }}>
        {props.title}
      </Text>
      <If when={windowSize().width >= 880 && props.workspace}>
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
