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

import { inputAction, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import { List } from 'nsh/list';
import type { TerminalsModel } from './terminalState';
import { C } from './ui';

export function Terminals(props: { model: TerminalsModel; sessionId: string }) {
  const model = props.model;
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 8 }}>
      <View style={{ flexDirection: 'row', gap: 8, alignItems: 'center' }}>
        <Button
          onPress={inputAction(model.start, null)}
          disabled={model.starting() || !model.known() || !!model.pending().length}
          accessibilityLabel="Start interactive terminal"
          style={{ paddingHorizontal: 8 }}
          hoverStyle={{ backgroundColor: C.raised }}
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.accent }}>
            {model.starting() ? 'Starting shell…' : '+ New terminal'}
          </Text>
        </Button>
        <Button
          onPress={inputAction(model.refresh, null)}
          accessibilityLabel="Refresh terminals"
          style={{ paddingHorizontal: 8 }}
          hoverStyle={{ backgroundColor: C.raised }}
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.accent }}>↻</Text>
        </Button>
      </View>
      <If when={model.error()}>
        <Text selectable style={{ color: C.danger }}>
          {model.error()}
        </Text>
      </If>
      <Each of={model.pending()} key={(operation) => operation.id}>
        {(operation) => (
          <View style={{ gap: 8, padding: 8, backgroundColor: C.raised }}>
            <Text style={{ color: C.warning }}>
              Unresolved {operation().method} · input has not been repeated.
            </Text>
            <View style={{ flexDirection: 'row', gap: 16 }}>
              <Button
                disabled={model.recovering()}
                onPress={inputAction(model.recover, {
                  sessionId: props.sessionId,
                  id: operation().id,
                  abandon: false,
                })}
              >
                <Text style={{ color: C.accent }}>Recover original</Text>
              </Button>
              <If when={!operation().reference}>
                <Button
                  disabled={model.recovering()}
                  onPress={inputAction(model.recover, {
                    sessionId: props.sessionId,
                    id: operation().id,
                    abandon: true,
                  })}
                >
                  <Text style={{ color: C.accent }}>Cancel undispatched action</Text>
                </Button>
              </If>
            </View>
          </View>
        )}
      </Each>
      <If when={model.loading() && !model.entries().length}>
        <Text style={{ color: C.muted }}>Reading terminals…</Text>
      </If>
      <If when={!model.loading() && !model.error() && !model.entries().length}>
        <Text style={{ color: C.muted }}>
          Start a shell in this conversation’s workspace. Agent terminals also appear here.
        </Text>
      </If>
      <List
        each={model.entries()}
        keyExtractor={(entry) => entry.result.ref}
        estimatedItemSize={64}
        accessibilityLabel="Conversation terminals"
        style={{ flex: 1, minHeight: 0 }}
      >
        {(entry) => (
          <Button
            onPress={inputAction(model.open, entry())}
            accessibilityLabel={`Open terminal: ${entry().result.cmd}`}
            style={{ padding: 8, minHeight: 64 }}
            hoverStyle={{ backgroundColor: C.raised }}
            focusStyle={{ backgroundColor: C.line }}
          >
            <Text numberOfLines={1} style={{ color: C.text }}>
              ⌁ {entry().result.cmd || 'Interactive shell'}
            </Text>
            <Text numberOfLines={1} style={{ color: C.muted }}>
              {entry().result.status} · {entry().result.mode} ·{' '}
              {entry().ownership.kind === 'local' ? entry().result.cwd : 'shared · view only'}
            </Text>
          </Button>
        )}
      </List>
    </View>
  );
}
