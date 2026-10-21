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

import { cleanup, effect, inputAction, Scroll, signal, Text, View } from 'nsh';
import { Button } from 'nsh/base';
import { List } from 'nsh/list';
import { host } from 'maka/host';
import type { WorkspacePath } from '../../../../packages/plugin-sdk/src/index.js';
import { C } from './ui';
import type { WorkspaceFilesModel } from './workspaceFiles';

export function WorkspaceFiles(props: { model: WorkspaceFilesModel }) {
  const model = props.model;
  const viewportHeight = signal(0);
  effect(() => {
    if (
      model.active() &&
      !model.loading() &&
      !model.error() &&
      model.entries().length < Math.ceil(viewportHeight() / 32)
    )
      void model.loadMore();
  });
  const label = (path: WorkspacePath) => {
    const text: string = host.pathLabel(path);
    return text.split('/').at(-1) ?? text;
  };
  return (
    <View style={{ flex: 1, minHeight: 0, gap: 8, backgroundColor: C.surface }}>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <Scroll horizontal style={{ flex: 1, minWidth: 0 }}>
          <View style={{ flexDirection: 'row', gap: 8 }}>
            <Each of={model.breadcrumbs()} key={(directory) => directory.path?.bytesBase64 ?? ''}>
              {(directory, index) => (
                <Button
                  disabled={!model.active()}
                  accessibilityLabel={`Open ${directory().title}`}
                  onPress={inputAction(model.navigate, {
                    sessionId: model.sessionId()!,
                    path: directory().path,
                  })}
                  style={{ paddingHorizontal: 8 }}
                  hoverStyle={{ backgroundColor: C.raised }}
                  focusStyle={{ backgroundColor: C.line }}
                >
                  <Text
                    numberOfLines={1}
                    style={{
                      color: index() === model.breadcrumbs().length - 1 ? C.text : C.accent,
                    }}
                  >
                    {index() ? '› ' : ''}
                    {directory().path ? label(directory().path!) : directory().title}
                  </Text>
                </Button>
              )}
            </Each>
          </View>
        </Scroll>
        <Button
          disabled={!model.active() || model.busy()}
          accessibilityLabel="Refresh workspace files"
          onPress={inputAction(model.refresh, null)}
          style={{ paddingHorizontal: 8 }}
          hoverStyle={{ backgroundColor: C.raised }}
          focusStyle={{ backgroundColor: C.line }}
        >
          <Text style={{ color: C.accent }}>↻</Text>
        </Button>
      </View>
      <Each
        of={[
          JSON.stringify([
            model.sessionId(),
            model.breadcrumbs().at(-1)?.path?.bytesBase64 ?? null,
          ]),
        ]}
        key={(key: string) => key}
      >
        {() => {
          const sessionId = model.sessionId();
          const directory = model.breadcrumbs().at(-1)!.path;
          const initialOffset = { ...model.position };
          let mounted = true;
          cleanup(() => {
            mounted = false;
          });
          const ownsView = () =>
            mounted &&
            model.active() &&
            model.sessionId() === sessionId &&
            model.breadcrumbs().at(-1)?.path?.bytesBase64 === directory?.bytesBase64;
          return (
            <List
              each={model.entries()}
              keyExtractor={(entry) => entry.path.bytesBase64}
              estimatedItemSize={32}
              contentOffset={initialOffset}
              onScroll={(event) => {
                if (ownsView()) {
                  model.position.x = event.x;
                  model.position.y = event.y;
                }
              }}
              onLayout={(frame) => {
                if (ownsView()) viewportHeight.set(frame.height);
              }}
              onEndReached={() => {
                if (ownsView()) void model.loadMore();
              }}
              accessibilityLabel="Workspace files"
              style={{ flex: 1, minHeight: 32 }}
            >
              {(entry) => (
                <Button
                  disabled={
                    !model.active() ||
                    model.busy() ||
                    !!model.error() ||
                    (entry().kind !== 'directory' && entry().kind !== 'file')
                  }
                  accessibilityLabel={`${entry().kind}: ${label(entry().path)}`}
                  onPress={inputAction(model.activate, {
                    sessionId: sessionId!,
                    parent: directory,
                    entry: entry(),
                  })}
                  style={{ height: 32, paddingHorizontal: 8, flexDirection: 'row', gap: 8 }}
                  hoverStyle={{ backgroundColor: C.raised }}
                  focusStyle={{ backgroundColor: C.line }}
                  activeStyle={{ backgroundColor: C.line }}
                >
                  <Text style={{ color: entry().kind === 'directory' ? C.accent : C.muted }}>
                    {entry().kind === 'directory' ? '▸' : entry().kind === 'symlink' ? '↗' : '·'}
                  </Text>
                  <Text
                    numberOfLines={1}
                    style={{
                      flex: 1,
                      color:
                        entry().kind === 'file' || entry().kind === 'directory' ? C.text : C.muted,
                    }}
                  >
                    {label(entry().path)}
                  </Text>
                  <If when={entry().kind === 'symlink' || entry().kind === 'other'}>
                    <Text style={{ color: C.muted }}>
                      {entry().kind === 'symlink' ? 'Link' : 'Unsupported'}
                    </Text>
                  </If>
                </Button>
              )}
            </List>
          );
        }}
      </Each>
      <If when={model.loading() || (model.busy() && model.entries().length === 0)}>
        <Text style={{ color: C.muted, paddingHorizontal: 8 }}>Loading files…</Text>
      </If>
      <If when={model.busy() && !model.loading() && model.entries().length > 0}>
        <Text style={{ color: C.muted, paddingHorizontal: 8 }}>Updating files…</Text>
      </If>
      <If when={!model.sessionId()}>
        <Text style={{ color: C.muted, paddingHorizontal: 8 }}>
          Select a conversation to browse its workspace.
        </Text>
      </If>
      <If
        when={model.sessionId() && !model.busy() && !model.error() && model.entries().length === 0}
      >
        <Text style={{ color: C.muted, paddingHorizontal: 8 }}>This directory is empty.</Text>
      </If>
      <If when={model.error()}>
        <Text style={{ color: C.danger, paddingHorizontal: 8 }}>{model.error()}</Text>
      </If>
    </View>
  );
}
